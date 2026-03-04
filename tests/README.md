# Defuddle Plugin Test Suite

This directory contains tests for the defuddle plugin, covering URL validation, cache behavior, and integration with the live defuddle.md API.

## Overview

Because this is a WASM project (compiled for `wasm32-wasip1`), the tests must be run with an explicit native target. The tests replicate the relevant production helpers and types so they can run natively without the Extism PDK runtime.

## Test Files

### `url_validation_tests.rs`

Unit tests for the URL validation and scheme-stripping logic used by both the tool and resource read paths.

**Categories:**

- **`validate_url` – accepted schemes**: Verifies that `http://` and `https://` URLs are accepted, including URLs with paths, query strings, fragments, ports, userinfo, encoded characters, and subdomains.
- **`validate_url` – rejected schemes**: Verifies that non-HTTP schemes (`ftp`, `file`, `ssh`, `data`, `javascript`, `mailto`, `ws`, `wss`) are rejected with descriptive error messages.
- **`validate_url` – malformed input**: Verifies that empty strings, bare hostnames, and gibberish are rejected.
- **`validate_url` – error message quality**: Verifies error messages contain useful context (e.g., the rejected scheme name, "Invalid URL" for unparseable input).
- **`strip_scheme` – https / http**: Verifies correct stripping of `https://` and `http://` prefixes, preserving the rest of the URL including paths, query strings, fragments, ports, and userinfo.
- **`strip_scheme` – passthrough**: Verifies that unknown schemes (`ftp://`), empty strings, and bare hostnames are returned unchanged.
- **`strip_scheme` – edge cases**: Verifies case sensitivity, that `https://` embedded in a path is not stripped, and that complex URLs are preserved correctly.
- **defuddle.md API URL construction**: Verifies that the production URL construction (`https://defuddle.md/{stripped_url}`) produces the expected output for various input URLs.
- **End-to-end validate-then-strip**: Verifies the validate → strip pipeline works correctly for accepted and rejected URLs.
- **Real-world URLs**: Tests against URLs users are likely to provide (GitHub, Wikipedia, YouTube, docs.rs, localhost, IP addresses, Unicode domains, long paths, complex query strings).

### `cache_tests.rs`

Unit tests for the file-based caching logic, mirroring the approach used by the context7-plugin's cache test suite. Uses `tempfile::TempDir` to create isolated cache directories.

**Categories:**

- **Hash determinism**: Verifies that identical URLs produce identical hashes, different URLs produce different hashes, and that `http://` vs `https://` URLs hash differently. Also verifies that the `DefuddleArguments` hash matches a plain `String` hash of the URL (matching production behavior).
- **Cache path generation**: Verifies the `{tool_name}_{hex_hash}.json` filename format, tool name prefixing, and path stability across calls.
- **CallToolResult serialization round-trip**: Verifies that text results, structured results, error results, markdown content, and empty content all survive JSON serialization and deserialization.
- **Cache put / get**: Verifies basic put-then-get, markdown content preservation, cache misses on empty directories, misses on different URLs, misses on different tool names, overwrites of existing entries, and concurrent storage of multiple URLs.
- **Staleness**: Verifies that fresh entries are returned, stale entries (past TTL) are not, zero-TTL always returns stale, nonexistent files are not fresh, and large TTLs work correctly.
- **Default TTL calculation**: Verifies the 1-day and 7-day TTL arithmetic.
- **Cache clear**: Verifies clearing empty caches, removing JSON files, leaving non-JSON files, put-after-clear, and clearing entries from multiple URLs.
- **Cache file content verification**: Verifies cache files contain valid JSON with expected fields.
- **Corrupted / malformed cache files**: Verifies graceful handling of corrupted data, empty files, and wrong JSON shapes.

### `defuddle_api_tests.rs`

Integration tests that make real HTTP requests to the live `defuddle.md` API.

**Categories:**

- **Basic API responses**: Verifies that `defuddle.md` returns markdown with `text/markdown` content type for `example.com`.
- **YAML frontmatter**: Verifies that responses contain YAML frontmatter (delimited by `---`) with expected metadata fields like `title`.
- **Real-world pages**: Tests against Wikipedia, GitHub, and rust-lang.org to verify defuddle handles diverse page structures.
- **Error handling**: Tests behavior for nonexistent domains.
- **CallToolResult compatibility**: Verifies that API responses can be wrapped in a `CallToolResult`, serialized to JSON (for caching), and deserialized back without data loss.
- **Full pipeline**: Tests the complete validate → strip → build API URL → fetch pipeline.
- **Idempotency**: Verifies that sequential requests to the same URL return identical content.
- **Output quality**: Verifies that markdown output does not contain raw HTML tags.
- **HTTP scheme handling**: Verifies that `http://` URLs work (they resolve to the same defuddle.md API path as `https://`).
- **RFC 6570 resource template patterns**: Verifies that the `https://{+url}` and `http://{+url}` URI templates correctly capture URL components.
- **Frontmatter extraction helpers**: Unit tests for the helper functions used by other tests.

## Running Tests

Because this is a WASM project (compiled for `wasm32-wasip1`), the tests must be run with an explicit native target:

```bash
# Run all tests
cargo test --target $(rustc -vV | grep host | cut -d' ' -f2) -- --nocapture

# Run only URL validation tests
cargo test --test url_validation_tests --target $(rustc -vV | grep host | cut -d' ' -f2) -- --nocapture

# Run only cache tests
cargo test --test cache_tests --target $(rustc -vV | grep host | cut -d' ' -f2) -- --nocapture

# Run only API integration tests
cargo test --test defuddle_api_tests --target $(rustc -vV | grep host | cut -d' ' -f2) -- --nocapture
```

Or specify your target explicitly:

```bash
cargo test --target aarch64-apple-darwin -- --nocapture   # macOS ARM
cargo test --target x86_64-apple-darwin -- --nocapture    # macOS Intel
cargo test --target x86_64-unknown-linux-gnu -- --nocapture  # Linux
```

The `--nocapture` flag allows you to see `println!` output from the tests, which includes API response previews and debug information.

## Dependencies

The test suite requires the following dev dependencies (defined in `Cargo.toml`):

- `reqwest` — For making HTTP requests in integration tests
- `tokio` — Async runtime for reqwest
- `tempfile` — For creating isolated temporary cache directories

Note: `url`, `serde`, and `serde_json` are already included in the main `[dependencies]` section.

## Notes

- The API integration tests in `defuddle_api_tests.rs` require network access to `https://defuddle.md`
- Response data may vary over time as page content changes; tests verify structure and format rather than exact content
- The cache tests use temporary directories and are fully isolated from each other
- All types are duplicated from `pdk::types` to enable native compilation (the PDK types require the WASM target)