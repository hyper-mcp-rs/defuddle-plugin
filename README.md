# Defuddle Plugin

A [Hyper MCP](https://github.com/hyper-mcp-rs/hyper-mcp) plugin that converts web pages to clean, readable Markdown using [defuddle.md](https://defuddle.md). It exposes both an MCP tool and MCP resource templates, so AI assistants can fetch and read any web page as Markdown.

## Features

- **Tool:** `defuddle` — fetch any `http://` or `https://` URL and get back clean Markdown with YAML frontmatter
- **Resource templates:** `https://{+url}` and `http://{+url}` — read any web page as a Markdown resource
- **Caching:** optional on-disk cache (identical to the [context7-plugin](https://github.com/hyper-mcp-rs/context7-plugin) cache) keyed by a hash of the URL
- **Retry logic:** automatic retries with back-off on 429 / 5xx responses

## How It Works

The plugin delegates HTML-to-Markdown conversion to the [defuddle.md](https://defuddle.md) web service. For any URL you provide, the plugin:

1. Validates that the scheme is `http` or `https`
2. Checks the local cache (if enabled) for a previous result
3. Strips the scheme and appends the remainder to `https://defuddle.md/` — e.g. `https://example.com/page` becomes `https://defuddle.md/example.com/page`
4. Returns the Markdown response (with YAML frontmatter containing title, source URL, word count, etc.)
5. Caches the result for future requests

## Configuration

### Minimal

```json
{
  "plugins": {
    "defuddle": {
      "url": "oci://ghcr.io/hyper-mcp-rs/defuddle-plugin:latest",
      "runtime_config": {
        "allowed_hosts": ["defuddle.md"]
      }
    }
  }
}
```

For nightly builds:

```json
{
  "plugins": {
    "defuddle": {
      "url": "oci://ghcr.io/hyper-mcp-rs/defuddle-plugin:nightly",
      "runtime_config": {
        "allowed_hosts": ["defuddle.md"]
      }
    }
  }
}
```

### With Caching

Add `/cache` to `allowed_paths`, mapping it to a directory on the host:

```json
{
  "plugins": {
    "defuddle": {
      "url": "oci://ghcr.io/hyper-mcp-rs/defuddle-plugin:latest",
      "runtime_config": {
        "allowed_hosts": ["defuddle.md"],
        "allowed_paths": ["/path/on/host/defuddle-cache:/cache"]
      }
    }
  }
}
```

If the `/cache` directory is not mounted the plugin will log an info-level message and operate without caching:

```
Cache directory /cache is not mounted; caching is disabled
```

### Cache TTL

By default cached responses expire after **1 day**. Customize this with the `CACHE_TTL` environment variable (value is in days):

```json
{
  "plugins": {
    "defuddle": {
      "url": "oci://ghcr.io/hyper-mcp-rs/defuddle-plugin:latest",
      "runtime_config": {
        "allowed_hosts": ["defuddle.md"],
        "allowed_paths": ["/path/on/host/defuddle-cache:/cache"],
        "env_vars": {
          "CACHE_TTL": "7"
        }
      }
    }
  }
}
```

### Full Configuration Example

```json
{
  "plugins": {
    "defuddle": {
      "url": "oci://ghcr.io/hyper-mcp-rs/defuddle-plugin:latest",
      "runtime_config": {
        "allowed_hosts": ["defuddle.md"],
        "allowed_paths": ["/path/on/host/defuddle-cache:/cache"],
        "env_vars": {
          "CACHE_TTL": "3"
        }
      }
    }
  }
}
```

### How the Cache Works

- Entries are stored as JSON files in `/cache`, named `defuddle_{hex_hash}.json` where the hash is derived from the URL string.
- Staleness is determined by comparing the file's last-modified time against the configured TTL.
- Only successful responses are cached; errors are never cached.
- The `clear_cache` tool can be used to manually invalidate all cached entries.
- Non-JSON files in the cache directory are left untouched by `clear_cache`.

## Tools

### 1. `defuddle`

Fetches a URL and returns the page content as Markdown.

Uses the [defuddle.md](https://defuddle.md) service to extract the main content from the page, strip away clutter (sidebars, headers, footers, ads, etc.), and convert the result to clean Markdown with YAML frontmatter.

**Input Schema:**

```json
{
  "url": "string (required) — The URL to fetch. Must use the http:// or https:// scheme."
}
```

**Example Input:**

```json
{
  "url": "https://docs.rs/serde/latest/serde/"
}
```

**Example Output:**

```markdown
---
title: "serde - Rust"
source: "https://docs.rs/serde/latest/serde/"
domain: "docs.rs"
word_count: 542
---

# Serde

Serde is a framework for **ser**ializing and **de**serializing Rust data structures
efficiently and generically.

...
```

**Behavior:**

- Returns a `CallToolResult` with a single text content block containing the Markdown
- If the URL scheme is not `http` or `https`, returns an error result
- If the URL has been fetched before and the cache entry is fresh, the cached result is returned
- Retries up to 3 times on 429 (rate limit) and 5xx (server error) responses, respecting the `Retry-After` header when present

### 2. `clear_cache`

Clears the on-disk Markdown cache. Use this when cached results appear stale or outdated.

This tool takes no arguments.

**Example Output (success):**

```
Cache cleared successfully (12 entries removed)
```

**Example Output (cache not mounted):**

```
Cache is not enabled (directory not mounted)
```

## Resource Templates

The plugin registers two [RFC 6570](https://www.rfc-editor.org/rfc/rfc6570) URI templates that allow MCP clients to read any web page as a Markdown resource.

### `https://{+url}`

Matches any HTTPS URL. The `{+url}` variable uses RFC 6570 **reserved expansion**, which allows reserved characters like `/`, `?`, `#`, and `&` to pass through without percent-encoding.

| Property | Value |
|---|---|
| **Name** | `defuddle-https` |
| **URI Template** | `https://{+url}` |
| **MIME Type** | `text/markdown` |
| **Description** | Fetch any https URL and return its content as Markdown via defuddle.md |

### `http://{+url}`

Matches any HTTP URL. Identical behavior to the HTTPS template.

| Property | Value |
|---|---|
| **Name** | `defuddle-http` |
| **URI Template** | `http://{+url}` |
| **MIME Type** | `text/markdown` |
| **Description** | Fetch any http URL and return its content as Markdown via defuddle.md |

### How Resource Templates Work

When an MCP client resolves a resource URI like `https://en.wikipedia.org/wiki/Rust_(programming_language)`:

1. The client matches it against the `https://{+url}` template
2. The full URI is passed to the plugin's `read_resource` handler
3. The plugin validates the scheme, checks the cache, and calls defuddle.md
4. The result is returned as a `TextResourceContents` with `mimeType: text/markdown`

The `read_resource` implementation shares the same validation, fetching, and caching logic as the `defuddle` tool — the only difference is the return type (`ReadResourceResult` with `TextResourceContents` instead of `CallToolResult`).

### Example

A client reading `https://example.com` as a resource receives:

```json
{
  "contents": [
    {
      "uri": "https://example.com",
      "mimeType": "text/markdown",
      "text": "---\ntitle: \"Example Domain\"\nsource: \"https://example.com\"\nword_count: 16\n---\n\nThis domain is for use in documentation examples without needing permission. Avoid use in operations.\n\n[Learn more](https://iana.org/domains/example)\n"
    }
  ]
}
```

## API Endpoint

The plugin uses the [defuddle.md](https://defuddle.md) web service:

- **Base URL:** `https://defuddle.md`
- **Usage:** `GET https://defuddle.md/{url_without_scheme}`
- **Response:** `text/markdown` with YAML frontmatter

For example, fetching `https://example.com/page` results in a request to `https://defuddle.md/example.com/page`.

### YAML Frontmatter Fields

The defuddle.md service returns Markdown with YAML frontmatter containing metadata extracted from the page:

| Field | Type | Description |
|---|---|---|
| `title` | string | Page title |
| `author` | string | Author (when available) |
| `published` | string | Publication date (when available) |
| `source` | string | Original URL |
| `domain` | string | Domain name (when available) |
| `description` | string | Page description / summary (when available) |
| `word_count` | number | Word count of the extracted content |

## Development

### Building

Build the WASM plugin:

```bash
cargo build --release --target wasm32-wasip1
```

The compiled plugin will be available at `target/wasm32-wasip1/release/plugin.wasm`.

### Testing

The plugin includes a comprehensive test suite with **132 tests** across three test files. Because this is a WASM project (compiled for `wasm32-wasip1`), the tests must be run with an explicit native target:

```bash
# Run all tests
cargo test --target $(rustc -vV | grep host | cut -d' ' -f2)

# With output visible
cargo test --target $(rustc -vV | grep host | cut -d' ' -f2) -- --nocapture
```

Or run individual test suites:

```bash
# URL validation and scheme-stripping logic (64 tests, no network)
cargo test --test url_validation_tests --target $(rustc -vV | grep host | cut -d' ' -f2)

# Cache functionality (42 tests, no network)
cargo test --test cache_tests --target $(rustc -vV | grep host | cut -d' ' -f2)

# Live API integration tests (26 tests, requires network)
cargo test --test defuddle_api_tests --target $(rustc -vV | grep host | cut -d' ' -f2)
```

Or specify your target explicitly:

```bash
cargo test --target aarch64-apple-darwin    # macOS ARM
cargo test --target x86_64-apple-darwin     # macOS Intel
cargo test --target x86_64-unknown-linux-gnu  # Linux
```

#### URL Validation Tests (`url_validation_tests`)

Tests verify:

- ✅ `http://` and `https://` URLs accepted (with paths, query strings, fragments, ports, userinfo, encoded characters, subdomains)
- ✅ Non-HTTP schemes rejected (`ftp`, `file`, `ssh`, `data`, `javascript`, `mailto`, `ws`, `wss`)
- ✅ Malformed input rejected (empty strings, bare hostnames, gibberish)
- ✅ Error messages contain useful context (rejected scheme name, "Invalid URL" for unparseable input)
- ✅ Scheme stripping preserves the rest of the URL exactly
- ✅ Case sensitivity (only lowercase `http://` / `https://` are stripped)
- ✅ API URL construction produces correct `https://defuddle.md/{path}` output
- ✅ Real-world URLs (GitHub, Wikipedia, YouTube, docs.rs, localhost, IP addresses, Unicode domains)

#### Cache Tests (`cache_tests`)

Tests verify:

- ✅ Hash determinism (same URL → same hash, different URLs → different hashes)
- ✅ `http://` vs `https://` produce different cache keys
- ✅ Cache key from `DefuddleArguments` matches plain `String` hash (as used in production)
- ✅ Cache path format: `{tool_name}_{hex_hash}.json`
- ✅ `CallToolResult` serialization round-trip (text, structured, error, Markdown with frontmatter)
- ✅ Cache put/get: basic hit, Markdown preservation, overwrite, multi-URL storage
- ✅ Cache misses: empty directory, different URL, different tool name
- ✅ TTL / staleness: fresh entries returned, stale entries rejected, zero-TTL always stale
- ✅ Cache clear: removes `.json` files only, leaves non-JSON files, supports put-after-clear
- ✅ Corrupted files: garbage data, empty files, wrong JSON shape, truncated JSON — all handled gracefully

#### API Integration Tests (`defuddle_api_tests`)

Tests verify:

- ✅ defuddle.md returns `text/markdown` content type
- ✅ Response contains YAML frontmatter with `title` field
- ✅ Real-world pages work (Wikipedia, GitHub, rust-lang.org)
- ✅ Nonexistent domains handled gracefully
- ✅ API response wraps into `CallToolResult` and survives JSON round-trip (for caching)
- ✅ Full pipeline: validate → strip → build API URL → fetch
- ✅ Sequential requests return identical content (idempotency)
- ✅ Markdown output is clean (no raw HTML tags)
- ✅ RFC 6570 resource template patterns capture URL components correctly

See [tests/README.md](tests/README.md) for detailed test documentation.

### Code Quality

```bash
# Check formatting
cargo fmt -- --check

# Run clippy
cargo clippy -- -D warnings
```

### Continuous Integration

The CI workflow runs on every push to `main` and on pull requests:

1. **clippy** — lint checks with `-D warnings`
2. **fmt** — formatting check
3. **build** — `cargo build --release --target wasm32-wasip1`

## Project Structure

```
defuddle-plugin/
├── src/
│   ├── lib.rs          # Plugin entry points: call_tool, list_tools, list_resource_templates, read_resource
│   ├── cache.rs        # On-disk cache (mirrors context7-plugin's cache module)
│   ├── types.rs        # DefuddleArguments, ClearCacheArguments
│   └── pdk/            # Auto-generated PDK bindings (types, imports, exports)
├── tests/
│   ├── url_validation_tests.rs   # URL validation + scheme stripping (64 tests)
│   ├── cache_tests.rs            # Cache logic (42 tests)
│   ├── defuddle_api_tests.rs     # Live API integration (26 tests)
│   └── README.md                 # Test documentation
├── Cargo.toml
├── Dockerfile
└── README.md
```

## License

Apache License 2.0 — see [LICENSE](LICENSE) for details.