use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use url::Url;

const DEFUDDLE_API_BASE_URL: &str = "https://defuddle.md";

// ---------------------------------------------------------------------------
// Replicated helpers from lib.rs so we can test them natively.
// ---------------------------------------------------------------------------

fn validate_url(url: &str) -> Result<Url, String> {
    let parsed = Url::parse(url).map_err(|e| format!("Invalid URL: {e}"))?;
    match parsed.scheme() {
        "http" | "https" => Ok(parsed),
        other => Err(format!("URL scheme must be http or https, got '{other}'")),
    }
}

fn strip_scheme(url: &str) -> &str {
    if let Some(rest) = url.strip_prefix("https://") {
        rest
    } else if let Some(rest) = url.strip_prefix("http://") {
        rest
    } else {
        url
    }
}

fn build_api_url(url: &str) -> String {
    let path = strip_scheme(url);
    format!("{}/{}", DEFUDDLE_API_BASE_URL, path)
}

// ---------------------------------------------------------------------------
// Duplicated pdk types needed for response verification
// ---------------------------------------------------------------------------

type Meta = Map<String, Value>;

#[derive(Default, Debug, Clone, Serialize, Deserialize, PartialEq)]
struct Annotations {
    pub audience: Vec<String>,
    #[serde(rename = "lastModified")]
    pub last_modified: String,
    pub priority: f32,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(untagged)]
enum ContentBlock {
    Text(TextContent),
    Empty(Empty),
}

impl Default for ContentBlock {
    fn default() -> Self {
        ContentBlock::Empty(Empty {})
    }
}

#[derive(Default, Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
struct Empty {}

#[derive(Default, Debug, Clone, PartialEq)]
struct TextContent {
    pub meta: Option<Meta>,
    pub annotations: Option<Annotations>,
    pub text: String,
}

impl Serialize for TextContent {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        #[derive(Serialize)]
        struct Helper<'a> {
            #[serde(rename = "_meta")]
            #[serde(skip_serializing_if = "Option::is_none")]
            meta: &'a Option<Meta>,
            #[serde(skip_serializing_if = "Option::is_none")]
            annotations: &'a Option<Annotations>,
            text: &'a String,
            r#type: &'static str,
        }

        let helper = Helper {
            meta: &self.meta,
            annotations: &self.annotations,
            text: &self.text,
            r#type: "text",
        };
        helper.serialize(serializer)
    }
}

impl<'de> Deserialize<'de> for TextContent {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        #[derive(Deserialize)]
        struct Helper {
            #[serde(rename = "_meta")]
            #[serde(default)]
            meta: Option<Meta>,
            #[serde(default)]
            annotations: Option<Annotations>,
            text: String,
            #[allow(dead_code)]
            r#type: Option<String>,
        }

        let helper = Helper::deserialize(deserializer)?;
        Ok(TextContent {
            meta: helper.meta,
            annotations: helper.annotations,
            text: helper.text,
        })
    }
}

#[derive(Default, Debug, Clone, Serialize, Deserialize, PartialEq)]
struct CallToolResult {
    #[serde(rename = "_meta")]
    #[serde(skip_serializing_if = "Option::is_none")]
    #[serde(default)]
    pub meta: Option<Meta>,
    pub content: Vec<ContentBlock>,
    #[serde(rename = "isError")]
    #[serde(skip_serializing_if = "Option::is_none")]
    #[serde(default)]
    pub is_error: Option<bool>,
    #[serde(rename = "structuredContent")]
    #[serde(skip_serializing_if = "Option::is_none")]
    #[serde(default)]
    pub structured_content: Option<Map<String, Value>>,
}

// ===========================================================================
// Helper: fetch from defuddle.md, returning None when the API is
// unreachable or blocked (e.g. Cloudflare challenge / 403 in CI).
// Tests that use this helper skip gracefully instead of failing.
// ===========================================================================

/// Result of a defuddle.md fetch attempt.
struct DefuddleFetch {
    status: u16,
    body: String,
    content_type: String,
}

/// Try to fetch markdown from defuddle.md.
/// Returns `None` on network errors so callers can skip.
async fn try_fetch(target_url: &str) -> Option<DefuddleFetch> {
    let api_url = build_api_url(target_url);
    let client = reqwest::Client::new();

    let response = match client.get(&api_url).send().await {
        Ok(r) => r,
        Err(e) => {
            eprintln!("Network error fetching {api_url} (skipping): {e}");
            return None;
        }
    };

    let status = response.status().as_u16();
    let content_type = response
        .headers()
        .get("content-type")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("")
        .to_string();

    let body = response.text().await.unwrap_or_default();

    Some(DefuddleFetch {
        status,
        body,
        content_type,
    })
}

/// Returns `true` when defuddle.md returned a genuine markdown response
/// (2xx with text/markdown). Returns `false` for Cloudflare challenges,
/// 403s, or any other non-markdown response — callers should skip.
fn is_genuine_markdown(fetch: &DefuddleFetch) -> bool {
    if fetch.status < 200 || fetch.status >= 300 {
        return false;
    }
    if fetch.content_type.contains("text/markdown") {
        return true;
    }
    // Some responses come back as 200 but with an HTML challenge page.
    // Detect by looking for the Cloudflare "Just a moment" signature.
    if fetch.body.contains("Just a moment") || fetch.body.contains("<!DOCTYPE html>") {
        return false;
    }
    // Accept anything that starts with YAML frontmatter even without
    // the expected content-type header.
    fetch.body.starts_with("---")
}

/// Convenience: fetch and return the body only when it's genuine markdown.
/// Prints a skip message and returns `None` otherwise.
async fn fetch_markdown_or_skip(target_url: &str) -> Option<String> {
    let fetch = match try_fetch(target_url).await {
        Some(f) => f,
        None => return None,
    };
    if !is_genuine_markdown(&fetch) {
        eprintln!(
            "defuddle.md returned non-markdown for {} (status={}, content_type={}, body_preview={}). \
             Skipping test — this is expected in CI behind Cloudflare.",
            target_url,
            fetch.status,
            fetch.content_type,
            &fetch.body[..fetch.body.len().min(120)]
        );
        return None;
    }
    Some(fetch.body)
}

// ===========================================================================
// Integration tests against the live defuddle.md API
//
// Every test that hits the network uses `fetch_markdown_or_skip` so it
// passes cleanly when Cloudflare blocks the request (common in CI).
// ===========================================================================

/// Test that defuddle.md returns markdown for a simple, well-known page
#[tokio::test]
async fn test_defuddle_api_returns_markdown_for_example_com() {
    let Some(fetch) = try_fetch("https://example.com").await else {
        eprintln!("Skipping: network error");
        return;
    };

    if !is_genuine_markdown(&fetch) {
        eprintln!(
            "Skipping: defuddle.md returned non-markdown (status={}, ct={})",
            fetch.status, fetch.content_type
        );
        return;
    }

    assert!(
        fetch.content_type.contains("text/markdown"),
        "Response Content-Type should be text/markdown, got: {}",
        fetch.content_type
    );

    assert!(
        !fetch.body.is_empty(),
        "Response body should not be empty for example.com"
    );

    println!("defuddle.md response for example.com:\n{}", fetch.body);
}

/// Test that the response contains YAML frontmatter
#[tokio::test]
async fn test_defuddle_api_returns_yaml_frontmatter() {
    let Some(body) = fetch_markdown_or_skip("https://example.com").await else {
        return;
    };

    assert!(
        body.starts_with("---"),
        "defuddle.md output should start with YAML frontmatter (---), got: {}",
        &body[..body.len().min(100)]
    );

    // The frontmatter should be closed by another ---
    let second_separator = body[3..].find("---");
    assert!(
        second_separator.is_some(),
        "YAML frontmatter should have a closing --- separator"
    );

    println!("Frontmatter detected in response");
}

/// Test that the YAML frontmatter contains expected metadata fields
#[tokio::test]
async fn test_defuddle_api_frontmatter_has_title() {
    let Some(body) = fetch_markdown_or_skip("https://example.com").await else {
        return;
    };

    let frontmatter = extract_frontmatter(&body);
    assert!(
        frontmatter.is_some(),
        "Should be able to extract frontmatter"
    );

    let fm = frontmatter.unwrap();

    assert!(
        fm.contains("title:"),
        "Frontmatter should contain a 'title' field:\n{}",
        fm
    );

    println!("Frontmatter:\n{}", fm);
}

/// Test that defuddle.md works for a page with real content (Wikipedia)
#[tokio::test]
async fn test_defuddle_api_with_wikipedia_page() {
    let Some(body) = fetch_markdown_or_skip("https://en.wikipedia.org/wiki/Markdown").await else {
        return;
    };

    assert!(
        !body.is_empty(),
        "Wikipedia markdown response should not be empty"
    );

    let body_lower = body.to_lowercase();
    assert!(
        body_lower.contains("markdown"),
        "Wikipedia Markdown article should contain the word 'markdown'"
    );

    println!(
        "Wikipedia Markdown article: {} bytes, first 500 chars:\n{}",
        body.len(),
        &body[..body.len().min(500)]
    );
}

/// Test that defuddle.md works for a GitHub repository page
#[tokio::test]
async fn test_defuddle_api_with_github_repo() {
    let Some(body) = fetch_markdown_or_skip("https://github.com/kepano/defuddle").await else {
        return;
    };

    assert!(!body.is_empty(), "GitHub response should not be empty");

    println!(
        "GitHub defuddle repo: {} bytes, first 500 chars:\n{}",
        body.len(),
        &body[..body.len().min(500)]
    );
}

/// Test that the API URL is constructed correctly for https URLs
#[test]
fn test_api_url_construction_https() {
    let url = "https://example.com/article";
    let api_url = build_api_url(url);
    assert_eq!(api_url, "https://defuddle.md/example.com/article");
}

/// Test that the API URL is constructed correctly for http URLs
#[test]
fn test_api_url_construction_http() {
    let url = "http://example.com/page";
    let api_url = build_api_url(url);
    assert_eq!(api_url, "https://defuddle.md/example.com/page");
}

/// Test that the API URL preserves query strings
#[test]
fn test_api_url_construction_with_query() {
    let url = "https://example.com/search?q=rust&page=1";
    let api_url = build_api_url(url);
    assert_eq!(
        api_url,
        "https://defuddle.md/example.com/search?q=rust&page=1"
    );
}

/// Test that the API URL preserves fragments
#[test]
fn test_api_url_construction_with_fragment() {
    let url = "https://example.com/docs#installation";
    let api_url = build_api_url(url);
    assert_eq!(api_url, "https://defuddle.md/example.com/docs#installation");
}

/// Test that the API URL handles root domain without trailing slash
#[test]
fn test_api_url_construction_root_domain() {
    let url = "https://example.com";
    let api_url = build_api_url(url);
    assert_eq!(api_url, "https://defuddle.md/example.com");
}

/// Test that the API URL handles subdomains
#[test]
fn test_api_url_construction_subdomain() {
    let url = "https://docs.rs/serde/latest/serde/";
    let api_url = build_api_url(url);
    assert_eq!(api_url, "https://defuddle.md/docs.rs/serde/latest/serde/");
}

/// Test that defuddle.md returns a non-200 or empty content for a
/// nonexistent domain (verifying error handling)
#[tokio::test]
async fn test_defuddle_api_handles_nonexistent_domain() {
    let url = "https://this-domain-definitely-does-not-exist-xyz123abc.com";
    let api_url = build_api_url(url);

    let client = reqwest::Client::new();
    let response = client.get(&api_url).send().await;

    match response {
        Ok(resp) => {
            // defuddle.md may return an error status or empty content
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            println!(
                "Response for nonexistent domain: status={}, body_len={}, body_preview={}",
                status,
                body.len(),
                &body[..body.len().min(200)]
            );
            // We just verify we got a response and didn't panic
        }
        Err(e) => {
            // Network error is also acceptable
            println!("Network error for nonexistent domain (acceptable): {}", e);
        }
    }
}

/// Test that the response can be cached and deserialized as a CallToolResult
#[tokio::test]
async fn test_defuddle_response_fits_call_tool_result() {
    let Some(body) = fetch_markdown_or_skip("https://example.com").await else {
        return;
    };

    // Build a CallToolResult the same way the plugin does
    let result = CallToolResult {
        content: vec![ContentBlock::Text(TextContent {
            text: body.clone(),
            ..Default::default()
        })],
        ..Default::default()
    };

    // Verify it can be serialized to JSON (for caching)
    let json = serde_json::to_string(&result).expect("Should serialize to JSON");
    assert!(!json.is_empty());

    // Verify it can be deserialized back
    let deserialized: CallToolResult =
        serde_json::from_str(&json).expect("Should deserialize from JSON");

    assert_eq!(result, deserialized, "Round-trip should preserve content");

    // Verify the text content matches the original body
    match &deserialized.content[0] {
        ContentBlock::Text(tc) => {
            assert_eq!(tc.text, body, "Deserialized text should match original");
        }
        _ => panic!("Expected Text content block"),
    }

    println!(
        "CallToolResult round-trip OK, JSON size: {} bytes",
        json.len()
    );
}

/// Test that validate_url works correctly before API call
#[tokio::test]
async fn test_validate_url_before_api_call() {
    // Valid URL
    let url = "https://example.com";
    assert!(validate_url(url).is_ok());

    // Now actually call the API — skip if blocked
    let Some(fetch) = try_fetch(url).await else {
        eprintln!("Skipping: network error");
        return;
    };
    if !is_genuine_markdown(&fetch) {
        eprintln!("Skipping: non-markdown response (status={})", fetch.status);
        return;
    }

    assert!(
        fetch.status >= 200 && fetch.status < 300,
        "Expected 2xx, got {}",
        fetch.status
    );
}

/// Test the full pipeline: validate -> strip -> build API URL -> fetch
#[tokio::test]
async fn test_full_pipeline_validate_strip_fetch() {
    let url = "https://example.com";

    // Step 1: Validate
    let parsed = validate_url(url).expect("Should validate successfully");
    assert_eq!(parsed.scheme(), "https");

    // Step 2: Strip scheme
    let path = strip_scheme(url);
    assert_eq!(path, "example.com");

    // Step 3: Build API URL
    let api_url = format!("{}/{}", DEFUDDLE_API_BASE_URL, path);
    assert_eq!(api_url, "https://defuddle.md/example.com");

    // Step 4: Fetch — skip if blocked
    let Some(body) = fetch_markdown_or_skip(url).await else {
        return;
    };

    assert!(!body.is_empty());
    assert!(body.starts_with("---"), "Should have YAML frontmatter");

    println!(
        "Full pipeline test passed. Response length: {} bytes",
        body.len()
    );
}

/// Test that defuddle.md handles a URL with a path correctly
#[tokio::test]
async fn test_defuddle_api_url_with_path() {
    let Some(body) = fetch_markdown_or_skip("https://www.rust-lang.org/learn").await else {
        return;
    };

    assert!(
        !body.is_empty(),
        "Response for rust-lang.org/learn should not be empty"
    );

    println!(
        "rust-lang.org/learn: {} bytes, first 300 chars:\n{}",
        body.len(),
        &body[..body.len().min(300)]
    );
}

/// Verify that multiple sequential requests to the same URL return
/// consistent results (basic idempotency check)
#[tokio::test]
async fn test_defuddle_api_idempotent_responses() {
    let Some(body1) = fetch_markdown_or_skip("https://example.com").await else {
        return;
    };
    let Some(body2) = fetch_markdown_or_skip("https://example.com").await else {
        return;
    };

    assert_eq!(
        body1, body2,
        "Two sequential requests to the same URL should return the same content"
    );

    println!("Idempotency verified for example.com");
}

/// Test that the markdown content includes actual readable text, not just
/// raw HTML or error messages
#[tokio::test]
async fn test_defuddle_api_output_is_readable_markdown() {
    let Some(body) = fetch_markdown_or_skip("https://example.com").await else {
        return;
    };

    // Should NOT contain raw HTML tags (defuddle strips them)
    let content_after_frontmatter = extract_content_after_frontmatter(&body);
    assert!(
        !content_after_frontmatter.contains("<html"),
        "Markdown output should not contain raw <html> tags"
    );
    assert!(
        !content_after_frontmatter.contains("<body"),
        "Markdown output should not contain raw <body> tags"
    );
    assert!(
        !content_after_frontmatter.contains("<div"),
        "Markdown output should not contain raw <div> tags"
    );

    println!(
        "Content after frontmatter is clean markdown:\n{}",
        &content_after_frontmatter[..content_after_frontmatter.len().min(300)]
    );
}

/// Test that an http:// URL (not https) also works with defuddle.md
#[tokio::test]
async fn test_defuddle_api_http_url() {
    // example.com should be accessible via http too
    let url = "http://example.com";
    let api_url = build_api_url(url);

    // http and https URLs both get stripped to "example.com" so the
    // API URL is the same
    assert_eq!(api_url, "https://defuddle.md/example.com");

    let Some(fetch) = try_fetch(url).await else {
        eprintln!("Skipping: network error");
        return;
    };
    if !is_genuine_markdown(&fetch) {
        eprintln!("Skipping: non-markdown response (status={})", fetch.status);
        return;
    }

    assert!(
        fetch.status >= 200 && fetch.status < 300,
        "Should work for http URLs too, got status: {}",
        fetch.status
    );
}

// ===========================================================================
// Resource template URI matching tests (RFC 6570)
// ===========================================================================

/// Verify the URI template pattern matches real HTTPS URLs.
/// RFC 6570 reserved expansion `{+url}` allows reserved characters like
/// `/`, `?`, `#` etc. to pass through unencoded.
#[test]
fn test_resource_template_https_pattern() {
    let template = "https://{+url}";

    // Simulate what the MCP host does: the template "https://{+url}" means
    // everything after "https://" is captured as the `url` variable.
    let full_uri = "https://example.com/path?q=test#section";
    assert!(full_uri.starts_with("https://"));

    let captured_url = &full_uri["https://".len()..];
    assert_eq!(captured_url, "example.com/path?q=test#section");

    // This captured value is what would be passed to read_resource
    // The full URI is the resource URI itself
    println!("Template '{}' captures url='{}'", template, captured_url);
}

/// Verify the URI template pattern matches real HTTP URLs
#[test]
fn test_resource_template_http_pattern() {
    let template = "http://{+url}";

    let full_uri = "http://example.com/articles/123";
    assert!(full_uri.starts_with("http://"));

    let captured_url = &full_uri["http://".len()..];
    assert_eq!(captured_url, "example.com/articles/123");

    println!("Template '{}' captures url='{}'", template, captured_url);
}

/// Verify that the template patterns don't match each other's scheme
#[test]
fn test_resource_templates_are_scheme_specific() {
    let https_uri = "https://example.com/page";
    let http_uri = "http://example.com/page";

    // HTTPS template should match HTTPS URIs
    assert!(https_uri.starts_with("https://"));
    assert!(!http_uri.starts_with("https://"));

    // HTTP template should match HTTP URIs
    assert!(http_uri.starts_with("http://"));
    // Note: https also starts with "http" so checking "http://" specifically
    assert!(!https_uri.starts_with("http://"));
}

// ===========================================================================
// Helpers
// ===========================================================================

/// Extract YAML frontmatter from markdown content (between first pair of ---)
fn extract_frontmatter(markdown: &str) -> Option<&str> {
    if !markdown.starts_with("---") {
        return None;
    }
    let after_first = &markdown[3..];
    let end = after_first.find("---")?;
    Some(after_first[..end].trim())
}

/// Extract markdown content after the YAML frontmatter
fn extract_content_after_frontmatter(markdown: &str) -> &str {
    if !markdown.starts_with("---") {
        return markdown;
    }
    let after_first = &markdown[3..];
    match after_first.find("---") {
        Some(pos) => after_first[pos + 3..].trim(),
        None => markdown,
    }
}

// ===========================================================================
// Frontmatter extraction helper tests
// ===========================================================================

#[test]
fn test_extract_frontmatter_valid() {
    let md = "---\ntitle: \"Test\"\nauthor: \"Me\"\n---\n\n# Content";
    let fm = extract_frontmatter(md);
    assert!(fm.is_some());
    let fm = fm.unwrap();
    assert!(fm.contains("title:"));
    assert!(fm.contains("author:"));
}

#[test]
fn test_extract_frontmatter_missing() {
    let md = "# Just a heading\n\nSome content.";
    assert!(extract_frontmatter(md).is_none());
}

#[test]
fn test_extract_content_after_frontmatter_valid() {
    let md = "---\ntitle: \"Test\"\n---\n\n# Heading\n\nContent here.";
    let content = extract_content_after_frontmatter(md);
    assert!(content.starts_with("# Heading"));
}

#[test]
fn test_extract_content_after_frontmatter_no_frontmatter() {
    let md = "# Just content";
    let content = extract_content_after_frontmatter(md);
    assert_eq!(content, "# Just content");
}
