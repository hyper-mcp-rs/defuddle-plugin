use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use std::collections::hash_map::DefaultHasher;
use std::fs;
use std::hash::{Hash, Hasher};
use std::path::{Path, PathBuf};
use std::thread;
use std::time::Duration;
use tempfile::TempDir;

// ---------------------------------------------------------------------------
// Duplicated types from pdk::types that we need for native tests.
// We only model the subset actually used by the cache (Text content blocks).
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

// TextContent uses the same tagged serialization as the real pdk type.
// The pdk implementation manually injects `"type": "text"` during
// serialization, so we replicate that here so round-trip JSON matches.
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

impl CallToolResult {
    fn error(msg: String) -> Self {
        CallToolResult {
            is_error: Some(true),
            content: vec![ContentBlock::Text(TextContent {
                text: msg,
                ..Default::default()
            })],
            ..Default::default()
        }
    }
}

// ---------------------------------------------------------------------------
// Duplicated argument types (must match Hash behaviour from types.rs)
// ---------------------------------------------------------------------------

#[derive(Default, Debug, Clone, Serialize, Deserialize)]
struct DefuddleArguments {
    pub url: String,
}

/// Hash is derived from the URL so cache lookups are deterministic.
impl Hash for DefuddleArguments {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.url.hash(state);
    }
}

// ---------------------------------------------------------------------------
// Replicated cache helpers that mirror cache.rs but accept a configurable
// cache directory and TTL so we can test without the PDK runtime.
// ---------------------------------------------------------------------------

fn compute_hash<T: Hash>(args: &T) -> u64 {
    let mut hasher = DefaultHasher::new();
    args.hash(&mut hasher);
    hasher.finish()
}

fn cache_path<T: Hash>(cache_dir: &Path, tool_name: &str, args: &T) -> PathBuf {
    let hash = compute_hash(args);
    cache_dir.join(format!("{}_{:x}.json", tool_name, hash))
}

fn is_fresh(path: &Path, ttl: Duration) -> bool {
    let Ok(metadata) = fs::metadata(path) else {
        return false;
    };
    let Ok(modified) = metadata.modified() else {
        return false;
    };
    let Ok(elapsed) = std::time::SystemTime::now().duration_since(modified) else {
        return false;
    };
    elapsed < ttl
}

fn cache_get<T: Hash>(
    cache_dir: &Path,
    tool_name: &str,
    args: &T,
    ttl: Duration,
) -> Option<CallToolResult> {
    let path = cache_path(cache_dir, tool_name, args);
    if !is_fresh(&path, ttl) {
        return None;
    }
    let data = fs::read_to_string(&path).ok()?;
    serde_json::from_str(&data).ok()
}

fn cache_put<T: Hash>(cache_dir: &Path, tool_name: &str, args: &T, result: &CallToolResult) {
    let path = cache_path(cache_dir, tool_name, args);
    let data = serde_json::to_string(result).expect("Failed to serialize CallToolResult");
    fs::write(&path, data).expect("Failed to write cache file");
}

fn cache_clear(cache_dir: &Path) -> (u64, Vec<String>) {
    let entries = fs::read_dir(cache_dir).expect("Failed to read cache dir");
    let mut removed = 0u64;
    let mut errors = Vec::new();

    for entry in entries {
        let Ok(entry) = entry else {
            continue;
        };
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) == Some("json") {
            match fs::remove_file(&path) {
                Ok(()) => removed += 1,
                Err(e) => errors.push(format!("{}: {}", path.display(), e)),
            }
        }
    }
    (removed, errors)
}

// ---------------------------------------------------------------------------
// Helpers to build CallToolResult values for testing
// ---------------------------------------------------------------------------

fn make_text_result(text: &str) -> CallToolResult {
    CallToolResult {
        content: vec![ContentBlock::Text(TextContent {
            text: text.to_string(),
            ..Default::default()
        })],
        ..Default::default()
    }
}

fn make_structured_result(text: &str, key: &str, value: &str) -> CallToolResult {
    let mut map = Map::new();
    map.insert(key.to_string(), Value::String(value.to_string()));
    CallToolResult {
        content: vec![ContentBlock::Text(TextContent {
            text: text.to_string(),
            ..Default::default()
        })],
        structured_content: Some(map),
        ..Default::default()
    }
}

// ===========================================================================
// Tests
// ===========================================================================

// --- Hash determinism ---

#[test]
fn test_hash_determinism_same_url() {
    let args1 = DefuddleArguments {
        url: "https://example.com/page".to_string(),
    };
    let args2 = DefuddleArguments {
        url: "https://example.com/page".to_string(),
    };
    assert_eq!(
        compute_hash(&args1),
        compute_hash(&args2),
        "Identical URLs must produce the same hash"
    );
}

#[test]
fn test_hash_determinism_different_url() {
    let args1 = DefuddleArguments {
        url: "https://example.com/page-a".to_string(),
    };
    let args2 = DefuddleArguments {
        url: "https://example.com/page-b".to_string(),
    };
    assert_ne!(
        compute_hash(&args1),
        compute_hash(&args2),
        "Different URLs must produce different hashes"
    );
}

#[test]
fn test_hash_determinism_http_vs_https() {
    let args_http = DefuddleArguments {
        url: "http://example.com".to_string(),
    };
    let args_https = DefuddleArguments {
        url: "https://example.com".to_string(),
    };
    assert_ne!(
        compute_hash(&args_http),
        compute_hash(&args_https),
        "http and https URLs must produce different hashes"
    );
}

#[test]
fn test_hash_determinism_url_with_query_string() {
    let args1 = DefuddleArguments {
        url: "https://example.com/search?q=rust".to_string(),
    };
    let args2 = DefuddleArguments {
        url: "https://example.com/search?q=rust".to_string(),
    };
    assert_eq!(
        compute_hash(&args1),
        compute_hash(&args2),
        "Identical URLs with query strings must produce the same hash"
    );
}

#[test]
fn test_hash_determinism_url_with_different_query_strings() {
    let args1 = DefuddleArguments {
        url: "https://example.com/search?q=rust".to_string(),
    };
    let args2 = DefuddleArguments {
        url: "https://example.com/search?q=python".to_string(),
    };
    assert_ne!(
        compute_hash(&args1),
        compute_hash(&args2),
        "URLs with different query strings must produce different hashes"
    );
}

#[test]
fn test_hash_determinism_url_with_fragment() {
    let args1 = DefuddleArguments {
        url: "https://example.com/page#section-1".to_string(),
    };
    let args2 = DefuddleArguments {
        url: "https://example.com/page#section-2".to_string(),
    };
    assert_ne!(
        compute_hash(&args1),
        compute_hash(&args2),
        "URLs with different fragments must produce different hashes"
    );
}

/// The cache uses both tool_name and hash so that two different tools
/// caching the same URL string will not collide.
#[test]
fn test_different_tool_names_produce_different_cache_paths() {
    let args = DefuddleArguments {
        url: "https://example.com".to_string(),
    };
    let path1 = cache_path(Path::new("/cache"), "defuddle", &args);
    let path2 = cache_path(Path::new("/cache"), "other_tool", &args);
    assert_ne!(
        path1, path2,
        "Different tool names must produce different cache paths"
    );
}

// --- Cache path generation ---

#[test]
fn test_cache_path_format() {
    let args = DefuddleArguments {
        url: "https://example.com/article".to_string(),
    };
    let path = cache_path(Path::new("/cache"), "defuddle", &args);
    let filename = path.file_name().unwrap().to_str().unwrap();

    assert!(
        filename.starts_with("defuddle_"),
        "Cache filename should start with tool name: {}",
        filename
    );
    assert!(
        filename.ends_with(".json"),
        "Cache filename should end with .json: {}",
        filename
    );
    // The middle part should be a hex hash
    let hex_part = &filename["defuddle_".len()..filename.len() - ".json".len()];
    assert!(!hex_part.is_empty(), "Hash portion should not be empty");
    assert!(
        hex_part.chars().all(|c| c.is_ascii_hexdigit()),
        "Hash portion should be hex: {}",
        hex_part
    );
}

#[test]
fn test_cache_path_uses_tool_name_prefix() {
    let args = DefuddleArguments {
        url: "https://example.com".to_string(),
    };
    let path = cache_path(Path::new("/tmp/test_cache"), "defuddle", &args);
    assert!(
        path.to_str()
            .unwrap()
            .starts_with("/tmp/test_cache/defuddle_")
    );
}

#[test]
fn test_cache_path_stable_across_calls() {
    let args = DefuddleArguments {
        url: "https://example.com/stable".to_string(),
    };
    let path1 = cache_path(Path::new("/cache"), "defuddle", &args);
    let path2 = cache_path(Path::new("/cache"), "defuddle", &args);
    assert_eq!(
        path1, path2,
        "cache_path should return the same path for the same inputs"
    );
}

// --- CallToolResult serialization round-trip ---

#[test]
fn test_call_tool_result_text_round_trip() {
    let result = make_text_result("# Hello World\n\nSome markdown content.");
    let json = serde_json::to_string(&result).expect("serialize");
    let deserialized: CallToolResult = serde_json::from_str(&json).expect("deserialize");
    assert_eq!(result, deserialized);
}

#[test]
fn test_call_tool_result_structured_round_trip() {
    let result = make_structured_result("some text", "title", "Example Page");
    let json = serde_json::to_string(&result).expect("serialize");
    let deserialized: CallToolResult = serde_json::from_str(&json).expect("deserialize");
    assert_eq!(result, deserialized);
}

#[test]
fn test_call_tool_result_error_round_trip() {
    let result = CallToolResult::error("URL scheme must be http or https".to_string());
    let json = serde_json::to_string(&result).expect("serialize");
    let deserialized: CallToolResult = serde_json::from_str(&json).expect("deserialize");
    assert_eq!(result.is_error, deserialized.is_error);
    assert_eq!(result.content.len(), deserialized.content.len());
}

#[test]
fn test_call_tool_result_with_markdown_content_round_trip() {
    let markdown = r#"---
title: "Example Domain"
source: "https://example.com"
word_count: 16
---

This domain is for use in documentation examples.

[Learn more](https://iana.org/domains/example)
"#;
    let result = make_text_result(markdown);
    let json = serde_json::to_string(&result).expect("serialize");
    let deserialized: CallToolResult = serde_json::from_str(&json).expect("deserialize");
    assert_eq!(result, deserialized);
}

#[test]
fn test_call_tool_result_empty_content_round_trip() {
    let result = make_text_result("");
    let json = serde_json::to_string(&result).expect("serialize");
    let deserialized: CallToolResult = serde_json::from_str(&json).expect("deserialize");
    assert_eq!(result, deserialized);
}

// --- Cache put / get ---

#[test]
fn test_cache_put_then_get() {
    let dir = TempDir::new().unwrap();
    let args = DefuddleArguments {
        url: "https://example.com/article".to_string(),
    };
    let result = make_text_result("# Cached Article\n\nSome content.");
    let ttl = Duration::from_secs(3600);

    cache_put(dir.path(), "defuddle", &args, &result);
    let cached = cache_get(dir.path(), "defuddle", &args, ttl);

    assert!(cached.is_some(), "Should get a cache hit after put");
    assert_eq!(cached.unwrap(), result);
}

#[test]
fn test_cache_put_then_get_preserves_markdown() {
    let dir = TempDir::new().unwrap();
    let markdown = r#"---
title: "Test Page"
source: "https://test.example.com"
word_count: 42
---

# Heading

Some **bold** and *italic* text.

- List item 1
- List item 2

```rust
fn main() {
    println!("Hello, world!");
}
```
"#;
    let args = DefuddleArguments {
        url: "https://test.example.com".to_string(),
    };
    let result = make_text_result(markdown);
    let ttl = Duration::from_secs(3600);

    cache_put(dir.path(), "defuddle", &args, &result);
    let cached = cache_get(dir.path(), "defuddle", &args, ttl).unwrap();

    assert_eq!(
        cached, result,
        "Cached markdown should be preserved exactly"
    );
}

#[test]
fn test_cache_miss_on_empty_directory() {
    let dir = TempDir::new().unwrap();
    let args = DefuddleArguments {
        url: "https://example.com/nothing".to_string(),
    };
    let ttl = Duration::from_secs(3600);

    let cached = cache_get(dir.path(), "defuddle", &args, ttl);
    assert!(cached.is_none(), "Empty cache should return None");
}

#[test]
fn test_cache_miss_on_different_url() {
    let dir = TempDir::new().unwrap();
    let args1 = DefuddleArguments {
        url: "https://example.com/page-a".to_string(),
    };
    let args2 = DefuddleArguments {
        url: "https://example.com/page-b".to_string(),
    };
    let result = make_text_result("cached for page-a");
    let ttl = Duration::from_secs(3600);

    cache_put(dir.path(), "defuddle", &args1, &result);
    let cached = cache_get(dir.path(), "defuddle", &args2, ttl);

    assert!(
        cached.is_none(),
        "Different URLs should not produce a cache hit"
    );
}

#[test]
fn test_cache_miss_on_different_tool_name() {
    let dir = TempDir::new().unwrap();
    let args = DefuddleArguments {
        url: "https://example.com".to_string(),
    };
    let result = make_text_result("cached content");
    let ttl = Duration::from_secs(3600);

    cache_put(dir.path(), "defuddle", &args, &result);
    let cached = cache_get(dir.path(), "some_other_tool", &args, ttl);

    assert!(
        cached.is_none(),
        "Different tool names should not share cache entries"
    );
}

#[test]
fn test_cache_put_overwrites_existing() {
    let dir = TempDir::new().unwrap();
    let args = DefuddleArguments {
        url: "https://example.com/mutable".to_string(),
    };
    let ttl = Duration::from_secs(3600);

    let result1 = make_text_result("first version of the page");
    cache_put(dir.path(), "defuddle", &args, &result1);

    let result2 = make_text_result("second version of the page");
    cache_put(dir.path(), "defuddle", &args, &result2);

    let cached = cache_get(dir.path(), "defuddle", &args, ttl).unwrap();
    assert_eq!(cached, result2, "Should return the latest cached value");
}

#[test]
fn test_cache_with_multiple_urls() {
    let dir = TempDir::new().unwrap();
    let ttl = Duration::from_secs(3600);

    let urls = vec![
        "https://example.com",
        "https://example.org/path",
        "http://example.net/page?q=test",
        "https://example.com/article#section",
    ];

    // Store all entries
    for url in &urls {
        let args = DefuddleArguments {
            url: url.to_string(),
        };
        let result = make_text_result(&format!("Content for {}", url));
        cache_put(dir.path(), "defuddle", &args, &result);
    }

    // Verify all entries can be retrieved independently
    for url in &urls {
        let args = DefuddleArguments {
            url: url.to_string(),
        };
        let cached = cache_get(dir.path(), "defuddle", &args, ttl);
        assert!(cached.is_some(), "Should find cache entry for {}", url);

        let expected = make_text_result(&format!("Content for {}", url));
        assert_eq!(
            cached.unwrap(),
            expected,
            "Cache entry for {} should match",
            url
        );
    }
}

/// The production code hashes `url.to_string()` (a plain String).
/// Verify that hashing a String directly matches our DefuddleArguments
/// approach — both should give the same hash since DefuddleArguments
/// only hashes its `url` field.
#[test]
fn test_cache_key_matches_string_hash() {
    let url = "https://example.com/article";
    let args = DefuddleArguments {
        url: url.to_string(),
    };

    // The production code does: cache::get("defuddle", &url.to_string())
    // which hashes a String. DefuddleArguments hashes self.url which is
    // also a String. Because String's Hash impl hashes the str contents,
    // and DefuddleArguments delegates to self.url.hash(), these should match.
    let hash_from_args = compute_hash(&args);
    let hash_from_string = compute_hash(&url.to_string());

    assert_eq!(
        hash_from_args, hash_from_string,
        "Hash of DefuddleArguments(url) should match hash of the url String directly, \
         since both delegate to str's Hash impl"
    );
}

// --- Staleness ---

#[test]
fn test_cache_fresh_entry_is_returned() {
    let dir = TempDir::new().unwrap();
    let args = DefuddleArguments {
        url: "https://example.com/fresh".to_string(),
    };
    let result = make_text_result("fresh content");
    let ttl = Duration::from_secs(3600); // 1 hour

    cache_put(dir.path(), "defuddle", &args, &result);
    // Just written, so it should be fresh
    let cached = cache_get(dir.path(), "defuddle", &args, ttl);
    assert!(
        cached.is_some(),
        "Freshly written cache entry should be returned"
    );
}

#[test]
fn test_cache_stale_entry_is_not_returned() {
    let dir = TempDir::new().unwrap();
    let args = DefuddleArguments {
        url: "https://example.com/stale".to_string(),
    };
    let result = make_text_result("will become stale");
    // Use a very short TTL
    let ttl = Duration::from_millis(50);

    cache_put(dir.path(), "defuddle", &args, &result);

    // Wait for the entry to become stale
    thread::sleep(Duration::from_millis(100));

    let cached = cache_get(dir.path(), "defuddle", &args, ttl);
    assert!(cached.is_none(), "Stale cache entry should not be returned");
}

#[test]
fn test_cache_zero_ttl_always_stale() {
    let dir = TempDir::new().unwrap();
    let args = DefuddleArguments {
        url: "https://example.com/zero-ttl".to_string(),
    };
    let result = make_text_result("zero ttl content");
    let ttl = Duration::ZERO;

    cache_put(dir.path(), "defuddle", &args, &result);
    let cached = cache_get(dir.path(), "defuddle", &args, ttl);

    assert!(
        cached.is_none(),
        "Zero TTL should always treat entries as stale"
    );
}

#[test]
fn test_is_fresh_nonexistent_file() {
    assert!(
        !is_fresh(
            Path::new("/nonexistent/path/file.json"),
            Duration::from_secs(3600)
        ),
        "Non-existent file should not be fresh"
    );
}

#[test]
fn test_is_fresh_with_large_ttl() {
    let dir = TempDir::new().unwrap();
    let path = dir.path().join("test.json");
    fs::write(&path, "{}").unwrap();

    let one_year = Duration::from_secs(365 * 24 * 60 * 60);
    assert!(
        is_fresh(&path, one_year),
        "Just-written file should be fresh with a large TTL"
    );
}

// --- Default TTL calculation ---

#[test]
fn test_default_cache_days_ttl() {
    // Verify the default TTL calculation: 1 day = 86400 seconds
    let default_days: u64 = 1;
    let ttl = Duration::from_secs(default_days * 24 * 60 * 60);
    assert_eq!(ttl.as_secs(), 86400);
}

#[test]
fn test_custom_cache_days_ttl() {
    // Verify TTL calculation for a custom number of days
    let days: u64 = 7;
    let ttl = Duration::from_secs(days * 24 * 60 * 60);
    assert_eq!(ttl.as_secs(), 604800);
}

// --- Cache clear ---

#[test]
fn test_clear_empty_cache() {
    let dir = TempDir::new().unwrap();
    let (removed, errors) = cache_clear(dir.path());

    assert_eq!(removed, 0, "Should remove 0 entries from empty cache");
    assert!(errors.is_empty(), "Should have no errors on empty cache");
}

#[test]
fn test_clear_removes_json_files() {
    let dir = TempDir::new().unwrap();
    let args1 = DefuddleArguments {
        url: "https://example.com/one".to_string(),
    };
    let args2 = DefuddleArguments {
        url: "https://example.com/two".to_string(),
    };

    cache_put(dir.path(), "defuddle", &args1, &make_text_result("one"));
    cache_put(dir.path(), "defuddle", &args2, &make_text_result("two"));

    let (removed, errors) = cache_clear(dir.path());

    assert_eq!(removed, 2, "Should remove 2 cache entries");
    assert!(errors.is_empty());

    // Verify files are gone
    let ttl = Duration::from_secs(3600);
    assert!(cache_get(dir.path(), "defuddle", &args1, ttl).is_none());
    assert!(cache_get(dir.path(), "defuddle", &args2, ttl).is_none());
}

#[test]
fn test_clear_leaves_non_json_files() {
    let dir = TempDir::new().unwrap();

    // Create a non-json file
    let non_json = dir.path().join("README.txt");
    fs::write(&non_json, "do not delete me").unwrap();

    // Create a cache entry
    let args = DefuddleArguments {
        url: "https://example.com/cached".to_string(),
    };
    cache_put(dir.path(), "defuddle", &args, &make_text_result("cached"));

    let (removed, errors) = cache_clear(dir.path());

    assert_eq!(removed, 1, "Should remove only the json file");
    assert!(errors.is_empty());
    assert!(
        non_json.exists(),
        "Non-JSON files should not be removed by clear"
    );
}

#[test]
fn test_clear_then_put_works() {
    let dir = TempDir::new().unwrap();
    let args = DefuddleArguments {
        url: "https://example.com/cleared".to_string(),
    };
    let ttl = Duration::from_secs(3600);

    cache_put(
        dir.path(),
        "defuddle",
        &args,
        &make_text_result("before clear"),
    );
    cache_clear(dir.path());

    assert!(
        cache_get(dir.path(), "defuddle", &args, ttl).is_none(),
        "Cache should be empty after clear"
    );

    cache_put(
        dir.path(),
        "defuddle",
        &args,
        &make_text_result("after clear"),
    );
    let cached = cache_get(dir.path(), "defuddle", &args, ttl);

    assert!(cached.is_some(), "Should be able to cache after clearing");
    assert_eq!(cached.unwrap(), make_text_result("after clear"));
}

#[test]
fn test_clear_removes_entries_from_multiple_urls() {
    let dir = TempDir::new().unwrap();

    let args1 = DefuddleArguments {
        url: "https://example.com/a".to_string(),
    };
    let args2 = DefuddleArguments {
        url: "https://example.org/b".to_string(),
    };
    let args3 = DefuddleArguments {
        url: "http://example.net/c".to_string(),
    };

    cache_put(dir.path(), "defuddle", &args1, &make_text_result("page a"));
    cache_put(dir.path(), "defuddle", &args2, &make_text_result("page b"));
    cache_put(dir.path(), "defuddle", &args3, &make_text_result("page c"));

    let (removed, errors) = cache_clear(dir.path());
    assert_eq!(removed, 3);
    assert!(errors.is_empty());
}

// --- Cache file content verification ---

#[test]
fn test_cache_file_is_valid_json() {
    let dir = TempDir::new().unwrap();
    let args = DefuddleArguments {
        url: "https://example.com/check-json".to_string(),
    };
    let result = make_text_result("# Some Markdown\n\nWith content.");

    cache_put(dir.path(), "defuddle", &args, &result);

    let path = cache_path(dir.path(), "defuddle", &args);
    let raw = fs::read_to_string(&path).expect("Should be able to read cache file");

    // Verify it's valid JSON
    let parsed: Value = serde_json::from_str(&raw).expect("Cache file should contain valid JSON");
    assert!(
        parsed.is_object(),
        "Cache file root should be a JSON object"
    );
}

#[test]
fn test_cache_file_contains_expected_fields() {
    let dir = TempDir::new().unwrap();
    let args = DefuddleArguments {
        url: "https://example.com/fields-check".to_string(),
    };
    let result = make_text_result("---\ntitle: Test\n---\n\n# Hello");

    cache_put(dir.path(), "defuddle", &args, &result);

    let path = cache_path(dir.path(), "defuddle", &args);
    let raw = fs::read_to_string(&path).unwrap();
    let parsed: Value = serde_json::from_str(&raw).unwrap();

    assert!(
        parsed.get("content").is_some(),
        "Cache file should have 'content' field"
    );

    let content = parsed.get("content").unwrap().as_array().unwrap();
    assert_eq!(content.len(), 1, "Should have one content block");

    let text_block = &content[0];
    assert_eq!(
        text_block.get("type").and_then(|v| v.as_str()),
        Some("text"),
        "Content block type should be 'text'"
    );
    assert!(
        text_block.get("text").is_some(),
        "Content block should have 'text' field"
    );
}

#[test]
fn test_cache_file_preserves_markdown_with_frontmatter() {
    let dir = TempDir::new().unwrap();
    let markdown = "---\ntitle: \"Example Domain\"\nsource: \"https://example.com\"\nword_count: 16\n---\n\nThis domain is for use in documentation.\n\n[Learn more](https://iana.org/domains/example)\n";
    let args = DefuddleArguments {
        url: "https://example.com".to_string(),
    };
    let result = make_text_result(markdown);

    cache_put(dir.path(), "defuddle", &args, &result);

    let path = cache_path(dir.path(), "defuddle", &args);
    let raw = fs::read_to_string(&path).unwrap();
    let parsed: Value = serde_json::from_str(&raw).unwrap();

    let text = parsed["content"][0]["text"].as_str().unwrap();
    assert_eq!(
        text, markdown,
        "Cached markdown should be preserved exactly"
    );
}

// --- Corrupted / malformed cache files ---

#[test]
fn test_cache_get_returns_none_for_corrupted_file() {
    let dir = TempDir::new().unwrap();
    let args = DefuddleArguments {
        url: "https://example.com/corrupted".to_string(),
    };
    let ttl = Duration::from_secs(3600);

    // Write garbage to the expected cache path
    let path = cache_path(dir.path(), "defuddle", &args);
    fs::write(&path, "this is not valid json!!!").unwrap();

    let cached = cache_get(dir.path(), "defuddle", &args, ttl);
    assert!(
        cached.is_none(),
        "Corrupted cache file should return None, not panic"
    );
}

#[test]
fn test_cache_get_returns_none_for_empty_file() {
    let dir = TempDir::new().unwrap();
    let args = DefuddleArguments {
        url: "https://example.com/empty".to_string(),
    };
    let ttl = Duration::from_secs(3600);

    let path = cache_path(dir.path(), "defuddle", &args);
    fs::write(&path, "").unwrap();

    let cached = cache_get(dir.path(), "defuddle", &args, ttl);
    assert!(cached.is_none(), "Empty cache file should return None");
}

#[test]
fn test_cache_get_returns_none_for_wrong_json_shape() {
    let dir = TempDir::new().unwrap();
    let args = DefuddleArguments {
        url: "https://example.com/wrong-shape".to_string(),
    };
    let ttl = Duration::from_secs(3600);

    // Valid JSON but not a CallToolResult
    let path = cache_path(dir.path(), "defuddle", &args);
    fs::write(&path, r#"{"unexpected": "structure"}"#).unwrap();

    let cached = cache_get(dir.path(), "defuddle", &args, ttl);
    assert!(cached.is_none(), "JSON with wrong shape should return None");
}

#[test]
fn test_cache_get_returns_none_for_partial_json() {
    let dir = TempDir::new().unwrap();
    let args = DefuddleArguments {
        url: "https://example.com/partial".to_string(),
    };
    let ttl = Duration::from_secs(3600);

    // Truncated JSON
    let path = cache_path(dir.path(), "defuddle", &args);
    fs::write(&path, r#"{"content": [{"text": "hello", "type": "te"#).unwrap();

    let cached = cache_get(dir.path(), "defuddle", &args, ttl);
    assert!(
        cached.is_none(),
        "Truncated JSON cache file should return None"
    );
}
