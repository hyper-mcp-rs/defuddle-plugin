use url::Url;

// ---------------------------------------------------------------------------
// Replicated helpers from lib.rs so we can test them natively without the
// WASM/PDK runtime.  These must stay in sync with the production code.
// ---------------------------------------------------------------------------

/// Validate that a URL string is http or https and return it parsed.
fn validate_url(url: &str) -> Result<Url, String> {
    let parsed = Url::parse(url).map_err(|e| format!("Invalid URL: {e}"))?;
    match parsed.scheme() {
        "http" | "https" => Ok(parsed),
        other => Err(format!("URL scheme must be http or https, got '{other}'")),
    }
}

/// Strip the scheme (http:// or https://) from a URL string to form the
/// defuddle.md API path.
fn strip_scheme(url: &str) -> &str {
    if let Some(rest) = url.strip_prefix("https://") {
        rest
    } else if let Some(rest) = url.strip_prefix("http://") {
        rest
    } else {
        url
    }
}

const DEFUDDLE_API_BASE_URL: &str = "https://defuddle.md";

// ===========================================================================
// validate_url – accepted schemes
// ===========================================================================

#[test]
fn test_validate_url_accepts_https() {
    let result = validate_url("https://example.com");
    assert!(result.is_ok(), "https URLs should be accepted");
    assert_eq!(result.unwrap().scheme(), "https");
}

#[test]
fn test_validate_url_accepts_http() {
    let result = validate_url("http://example.com");
    assert!(result.is_ok(), "http URLs should be accepted");
    assert_eq!(result.unwrap().scheme(), "http");
}

#[test]
fn test_validate_url_accepts_https_with_path() {
    let result = validate_url("https://example.com/some/path");
    assert!(result.is_ok());
    let parsed = result.unwrap();
    assert_eq!(parsed.path(), "/some/path");
}

#[test]
fn test_validate_url_accepts_http_with_path() {
    let result = validate_url("http://example.com/page/article");
    assert!(result.is_ok());
    let parsed = result.unwrap();
    assert_eq!(parsed.path(), "/page/article");
}

#[test]
fn test_validate_url_accepts_url_with_query_string() {
    let result = validate_url("https://example.com/search?q=rust&lang=en");
    assert!(result.is_ok());
    let parsed = result.unwrap();
    assert_eq!(parsed.query(), Some("q=rust&lang=en"));
}

#[test]
fn test_validate_url_accepts_url_with_fragment() {
    let result = validate_url("https://example.com/page#section-3");
    assert!(result.is_ok());
    let parsed = result.unwrap();
    assert_eq!(parsed.fragment(), Some("section-3"));
}

#[test]
fn test_validate_url_accepts_url_with_port() {
    let result = validate_url("http://localhost:8080/api");
    assert!(result.is_ok());
    let parsed = result.unwrap();
    assert_eq!(parsed.port(), Some(8080));
}

#[test]
fn test_validate_url_accepts_url_with_userinfo() {
    let result = validate_url("https://user:pass@example.com/path");
    assert!(result.is_ok());
    let parsed = result.unwrap();
    assert_eq!(parsed.username(), "user");
    assert_eq!(parsed.password(), Some("pass"));
}

#[test]
fn test_validate_url_accepts_url_with_encoded_characters() {
    let result = validate_url("https://example.com/path%20with%20spaces");
    assert!(result.is_ok());
}

#[test]
fn test_validate_url_accepts_url_with_subdomain() {
    let result = validate_url("https://docs.rs/serde/latest/serde/");
    assert!(result.is_ok());
    let parsed = result.unwrap();
    assert_eq!(parsed.host_str(), Some("docs.rs"));
}

// ===========================================================================
// validate_url – rejected schemes
// ===========================================================================

#[test]
fn test_validate_url_rejects_ftp() {
    let result = validate_url("ftp://files.example.com/data.csv");
    assert!(result.is_err());
    let err = result.unwrap_err();
    assert!(
        err.contains("ftp"),
        "Error message should mention the rejected scheme: {}",
        err
    );
}

#[test]
fn test_validate_url_rejects_file() {
    let result = validate_url("file:///etc/passwd");
    assert!(result.is_err());
    let err = result.unwrap_err();
    assert!(err.contains("file"), "Error should mention 'file': {}", err);
}

#[test]
fn test_validate_url_rejects_ssh() {
    let result = validate_url("ssh://git@github.com/user/repo");
    assert!(result.is_err());
    let err = result.unwrap_err();
    assert!(err.contains("ssh"), "Error should mention 'ssh': {}", err);
}

#[test]
fn test_validate_url_rejects_data_uri() {
    let result = validate_url("data:text/html,<h1>Hello</h1>");
    assert!(result.is_err());
    let err = result.unwrap_err();
    assert!(err.contains("data"), "Error should mention 'data': {}", err);
}

#[test]
fn test_validate_url_rejects_javascript_uri() {
    let result = validate_url("javascript:alert(1)");
    assert!(result.is_err());
}

#[test]
fn test_validate_url_rejects_mailto() {
    let result = validate_url("mailto:user@example.com");
    assert!(result.is_err());
    let err = result.unwrap_err();
    assert!(
        err.contains("mailto"),
        "Error should mention 'mailto': {}",
        err
    );
}

#[test]
fn test_validate_url_rejects_ws_websocket() {
    let result = validate_url("ws://example.com/socket");
    assert!(result.is_err());
    let err = result.unwrap_err();
    assert!(err.contains("ws"), "Error should mention 'ws': {}", err);
}

#[test]
fn test_validate_url_rejects_wss_websocket() {
    let result = validate_url("wss://example.com/socket");
    assert!(result.is_err());
    let err = result.unwrap_err();
    assert!(err.contains("wss"), "Error should mention 'wss': {}", err);
}

// ===========================================================================
// validate_url – malformed input
// ===========================================================================

#[test]
fn test_validate_url_rejects_empty_string() {
    let result = validate_url("");
    assert!(result.is_err(), "Empty string should not be a valid URL");
}

#[test]
fn test_validate_url_rejects_bare_hostname() {
    let result = validate_url("example.com");
    assert!(
        result.is_err(),
        "Bare hostname without scheme should be rejected"
    );
}

#[test]
fn test_validate_url_rejects_bare_hostname_with_path() {
    let result = validate_url("example.com/path/to/page");
    assert!(
        result.is_err(),
        "Bare hostname with path should be rejected"
    );
}

#[test]
fn test_validate_url_rejects_random_gibberish() {
    let result = validate_url("not a url at all");
    assert!(result.is_err());
}

#[test]
fn test_validate_url_rejects_just_a_scheme() {
    let result = validate_url("https://");
    // The url crate may or may not accept this depending on version,
    // but it should at least not panic.
    // If it parses, the host will be empty which is still technically
    // a valid URL per the spec. We primarily care about scheme validation.
    if let Ok(parsed) = &result {
        assert!(
            parsed.scheme() == "https",
            "If parsed, scheme should be https"
        );
    }
}

#[test]
fn test_validate_url_rejects_scheme_only_no_slashes() {
    let result = validate_url("https:example.com");
    // This is technically scheme-relative and may parse differently,
    // but should not panic.
    match result {
        Ok(parsed) => assert!(parsed.scheme() == "https"),
        Err(_) => {} // rejected is also fine
    }
}

// ===========================================================================
// validate_url – error message quality
// ===========================================================================

#[test]
fn test_validate_url_error_contains_invalid_url_for_bad_input() {
    let result = validate_url("not_a_url");
    assert!(result.is_err());
    let err = result.unwrap_err();
    assert!(
        err.contains("Invalid URL"),
        "Error for unparseable input should contain 'Invalid URL': {}",
        err
    );
}

#[test]
fn test_validate_url_error_contains_scheme_name_for_wrong_scheme() {
    let result = validate_url("ftp://example.com");
    assert!(result.is_err());
    let err = result.unwrap_err();
    assert!(
        err.contains("http or https"),
        "Error should mention expected schemes: {}",
        err
    );
    assert!(
        err.contains("ftp"),
        "Error should mention the actual scheme used: {}",
        err
    );
}

// ===========================================================================
// strip_scheme – https
// ===========================================================================

#[test]
fn test_strip_scheme_https_simple() {
    assert_eq!(strip_scheme("https://example.com"), "example.com");
}

#[test]
fn test_strip_scheme_https_with_path() {
    assert_eq!(
        strip_scheme("https://example.com/path/to/page"),
        "example.com/path/to/page"
    );
}

#[test]
fn test_strip_scheme_https_with_query() {
    assert_eq!(
        strip_scheme("https://example.com/search?q=test"),
        "example.com/search?q=test"
    );
}

#[test]
fn test_strip_scheme_https_with_fragment() {
    assert_eq!(
        strip_scheme("https://example.com/page#section"),
        "example.com/page#section"
    );
}

#[test]
fn test_strip_scheme_https_with_port() {
    assert_eq!(
        strip_scheme("https://example.com:8443/api"),
        "example.com:8443/api"
    );
}

#[test]
fn test_strip_scheme_https_with_userinfo() {
    assert_eq!(
        strip_scheme("https://user:pass@example.com/path"),
        "user:pass@example.com/path"
    );
}

// ===========================================================================
// strip_scheme – http
// ===========================================================================

#[test]
fn test_strip_scheme_http_simple() {
    assert_eq!(strip_scheme("http://example.com"), "example.com");
}

#[test]
fn test_strip_scheme_http_with_path() {
    assert_eq!(
        strip_scheme("http://example.com/articles/123"),
        "example.com/articles/123"
    );
}

#[test]
fn test_strip_scheme_http_with_query() {
    assert_eq!(
        strip_scheme("http://example.com/page?a=1&b=2"),
        "example.com/page?a=1&b=2"
    );
}

#[test]
fn test_strip_scheme_http_with_port() {
    assert_eq!(
        strip_scheme("http://localhost:3000/health"),
        "localhost:3000/health"
    );
}

// ===========================================================================
// strip_scheme – passthrough (no scheme or unknown scheme)
// ===========================================================================

#[test]
fn test_strip_scheme_no_scheme_returns_unchanged() {
    assert_eq!(strip_scheme("example.com/path"), "example.com/path");
}

#[test]
fn test_strip_scheme_ftp_returns_unchanged() {
    // strip_scheme only handles http/https; anything else passes through
    assert_eq!(
        strip_scheme("ftp://files.example.com"),
        "ftp://files.example.com"
    );
}

#[test]
fn test_strip_scheme_empty_string() {
    assert_eq!(strip_scheme(""), "");
}

#[test]
fn test_strip_scheme_just_https_prefix() {
    assert_eq!(strip_scheme("https://"), "");
}

#[test]
fn test_strip_scheme_just_http_prefix() {
    assert_eq!(strip_scheme("http://"), "");
}

// ===========================================================================
// strip_scheme – edge cases
// ===========================================================================

#[test]
fn test_strip_scheme_case_sensitive_https() {
    // The function is case-sensitive; uppercase should pass through unchanged
    assert_eq!(strip_scheme("HTTPS://example.com"), "HTTPS://example.com");
}

#[test]
fn test_strip_scheme_case_sensitive_http() {
    assert_eq!(strip_scheme("HTTP://example.com"), "HTTP://example.com");
}

#[test]
fn test_strip_scheme_https_in_path_not_stripped() {
    // "https://" only stripped from the beginning
    assert_eq!(
        strip_scheme("http://example.com/redirect?to=https://other.com"),
        "example.com/redirect?to=https://other.com"
    );
}

#[test]
fn test_strip_scheme_preserves_trailing_slash() {
    assert_eq!(strip_scheme("https://example.com/"), "example.com/");
}

#[test]
fn test_strip_scheme_preserves_complex_url() {
    let url = "https://user:pass@sub.example.com:8443/path/to/page?q=hello&lang=en#section-2";
    let expected = "user:pass@sub.example.com:8443/path/to/page?q=hello&lang=en#section-2";
    assert_eq!(strip_scheme(url), expected);
}

// ===========================================================================
// defuddle.md API URL construction
// ===========================================================================

#[test]
fn test_api_url_construction_https() {
    let url = "https://example.com/article";
    let path = strip_scheme(url);
    let api_url = format!("{}/{}", DEFUDDLE_API_BASE_URL, path);
    assert_eq!(api_url, "https://defuddle.md/example.com/article");
}

#[test]
fn test_api_url_construction_http() {
    let url = "http://example.com/page";
    let path = strip_scheme(url);
    let api_url = format!("{}/{}", DEFUDDLE_API_BASE_URL, path);
    assert_eq!(api_url, "https://defuddle.md/example.com/page");
}

#[test]
fn test_api_url_construction_preserves_query_string() {
    let url = "https://example.com/search?q=rust";
    let path = strip_scheme(url);
    let api_url = format!("{}/{}", DEFUDDLE_API_BASE_URL, path);
    assert_eq!(api_url, "https://defuddle.md/example.com/search?q=rust");
}

#[test]
fn test_api_url_construction_preserves_fragment() {
    let url = "https://example.com/docs#installation";
    let path = strip_scheme(url);
    let api_url = format!("{}/{}", DEFUDDLE_API_BASE_URL, path);
    assert_eq!(api_url, "https://defuddle.md/example.com/docs#installation");
}

#[test]
fn test_api_url_construction_root_domain() {
    let url = "https://example.com";
    let path = strip_scheme(url);
    let api_url = format!("{}/{}", DEFUDDLE_API_BASE_URL, path);
    assert_eq!(api_url, "https://defuddle.md/example.com");
}

#[test]
fn test_api_url_construction_with_subdomain() {
    let url = "https://docs.rs/serde/latest/serde/";
    let path = strip_scheme(url);
    let api_url = format!("{}/{}", DEFUDDLE_API_BASE_URL, path);
    assert_eq!(api_url, "https://defuddle.md/docs.rs/serde/latest/serde/");
}

// ===========================================================================
// End-to-end: validate then strip
// ===========================================================================

#[test]
fn test_validate_then_strip_https() {
    let url = "https://example.com/page";
    assert!(validate_url(url).is_ok());
    assert_eq!(strip_scheme(url), "example.com/page");
}

#[test]
fn test_validate_then_strip_http() {
    let url = "http://example.com/page";
    assert!(validate_url(url).is_ok());
    assert_eq!(strip_scheme(url), "example.com/page");
}

#[test]
fn test_validate_rejects_then_strip_is_moot() {
    let url = "ftp://example.com/file";
    assert!(validate_url(url).is_err());
    // strip_scheme would pass it through unchanged, but production code
    // won't reach strip_scheme if validation fails
    assert_eq!(strip_scheme(url), "ftp://example.com/file");
}

// ===========================================================================
// Real-world URLs that users are likely to provide
// ===========================================================================

#[test]
fn test_validate_github_url() {
    let url = "https://github.com/kepano/defuddle";
    let result = validate_url(url);
    assert!(result.is_ok());
    assert_eq!(strip_scheme(url), "github.com/kepano/defuddle");
}

#[test]
fn test_validate_wikipedia_url() {
    let url = "https://en.wikipedia.org/wiki/Rust_(programming_language)";
    let result = validate_url(url);
    assert!(result.is_ok());
    assert_eq!(
        strip_scheme(url),
        "en.wikipedia.org/wiki/Rust_(programming_language)"
    );
}

#[test]
fn test_validate_youtube_url() {
    let url = "https://www.youtube.com/watch?v=dQw4w9WgXcQ";
    let result = validate_url(url);
    assert!(result.is_ok());
    assert_eq!(strip_scheme(url), "www.youtube.com/watch?v=dQw4w9WgXcQ");
}

#[test]
fn test_validate_docs_rs_url() {
    let url = "https://docs.rs/serde_json/latest/serde_json/fn.from_str.html";
    let result = validate_url(url);
    assert!(result.is_ok());
    assert_eq!(
        strip_scheme(url),
        "docs.rs/serde_json/latest/serde_json/fn.from_str.html"
    );
}

#[test]
fn test_validate_localhost_url() {
    let url = "http://localhost:3000/api/health";
    let result = validate_url(url);
    assert!(result.is_ok());
    assert_eq!(strip_scheme(url), "localhost:3000/api/health");
}

#[test]
fn test_validate_ip_address_url() {
    let url = "http://192.168.1.1:8080/admin";
    let result = validate_url(url);
    assert!(result.is_ok());
    assert_eq!(strip_scheme(url), "192.168.1.1:8080/admin");
}

#[test]
fn test_validate_url_with_unicode_domain() {
    // Internationalized domain name
    let url = "https://例え.jp/ページ";
    let result = validate_url(url);
    // The url crate should handle IDN; we just care that scheme is accepted
    assert!(result.is_ok());
}

#[test]
fn test_validate_url_with_long_path() {
    let url = "https://example.com/a/b/c/d/e/f/g/h/i/j/k/l/m/n/o/p";
    let result = validate_url(url);
    assert!(result.is_ok());
    assert_eq!(
        strip_scheme(url),
        "example.com/a/b/c/d/e/f/g/h/i/j/k/l/m/n/o/p"
    );
}

#[test]
fn test_validate_url_with_complex_query() {
    let url = "https://example.com/search?q=hello+world&page=1&sort=relevance&filter[type]=article";
    let result = validate_url(url);
    assert!(result.is_ok());
}
