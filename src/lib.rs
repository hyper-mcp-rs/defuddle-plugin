mod cache;
mod pdk;
mod types;

use anyhow::{Result, anyhow};
use extism_pdk::*;
use pdk::{
    http::http_request_with_retry,
    imports::{notify_logging_message, notify_resource_updated},
    types::*,
};
use schemars::schema_for;
use serde_json::{Value, json};
use types::*;

const DEFUDDLE_API_BASE_URL: &str = "https://defuddle.md";

/// Validate that a URL string is http or https and return it parsed.
fn validate_url(url: &str) -> Result<url::Url, String> {
    let parsed = url::Url::parse(url).map_err(|e| format!("Invalid URL: {e}"))?;
    match parsed.scheme() {
        "http" | "https" => Ok(parsed),
        other => Err(format!("URL scheme must be http or https, got '{other}'")),
    }
}

/// Shared implementation for both the tool call and resource read: validate the
/// URL, check cache, call defuddle.md, cache the result, return markdown.
fn fetch_defuddle_markdown(url: &str) -> CallToolResult {
    // Validate URL scheme
    if let Err(e) = validate_url(url) {
        return CallToolResult::error(e);
    }

    // Check cache
    if let Some(cached) = cache::get("defuddle", &url.to_string()) {
        notify_logging_message(LoggingMessageNotificationParam {
            data: json!(format!("Cache hit for URL: {}", url)),
            level: LoggingLevel::Debug,
            ..Default::default()
        })
        .ok();
        return cached;
    }

    // Fetch from defuddle.md
    fn fetch_markdown_from_defuddle(url: &str) -> Result<String, String> {
        let path = if let Some(rest) = url.strip_prefix("https://") {
            rest
        } else if let Some(rest) = url.strip_prefix("http://") {
            rest
        } else {
            url
        };
        let api_url = format!("{}/{}", DEFUDDLE_API_BASE_URL, path);

        let req = HttpRequest::new(&api_url).with_method("GET");

        let res = http_request_with_retry(&req).map_err(|e| format!("HTTP request failed: {e}"))?;

        let body = String::from_utf8_lossy(&res.body()).to_string();

        if res.status_code() < 200 || res.status_code() >= 300 {
            return Err(format!(
                "defuddle.md API returned status {}: {}",
                res.status_code(),
                body
            ));
        }

        Ok(body)
    }

    let markdown = match fetch_markdown_from_defuddle(url) {
        Ok(md) => md,
        Err(e) => return CallToolResult::error(e),
    };

    let result = CallToolResult {
        content: vec![ContentBlock::Text(TextContent {
            text: markdown,
            ..Default::default()
        })],
        ..Default::default()
    };

    // Cache the result
    cache::put("defuddle", &url.to_string(), &result);

    // Notify subscribers that this resource has been updated (fresh fetch, not from cache)
    notify_resource_updated(ResourceUpdatedNotificationParam {
        uri: url.to_string(),
    })
    .ok();

    result
}

// ---------------------------------------------------------------------------
// MCP: call_tool
// ---------------------------------------------------------------------------

pub(crate) fn call_tool(input: CallToolRequest) -> Result<CallToolResult> {
    Ok(match input.request.name.as_str() {
        "defuddle" => {
            let args: DefuddleArguments = match serde_json::from_value(Value::Object(
                input.request.arguments.unwrap_or_default(),
            )) {
                Ok(a) => a,
                Err(e) => return Ok(CallToolResult::error(format!("Invalid arguments: {e}"))),
            };

            fetch_defuddle_markdown(&args.url)
        }
        "clear_cache" => cache::clear(),
        _ => CallToolResult::error(format!("Unknown tool: {}", input.request.name)),
    })
}

// ---------------------------------------------------------------------------
// MCP: list_tools
// ---------------------------------------------------------------------------

pub(crate) fn list_tools(_input: ListToolsRequest) -> Result<ListToolsResult> {
    Ok(ListToolsResult {
        tools: vec![
            Tool {
                name: "defuddle".to_string(),
                annotations: Some(ToolAnnotations {
                    read_only_hint: Some(true),
                    ..Default::default()
                }),
                description: Some(
                    "Fetches a URL and returns the content as Markdown.\n\n\
                     Uses defuddle.md to convert the HTML to clean, readable Markdown with YAML frontmatter.\n\
                     The URL must use the http or https scheme."
                        .to_string(),
                ),
                input_schema: schema_for!(DefuddleArguments),
                title: Some("Fetch URL as Markdown".to_string()),

                ..Default::default()
            },
            Tool {
                name: "clear_cache".to_string(),
                annotations: Some(ToolAnnotations {
                    destructive_hint: Some(true),
                    read_only_hint: Some(false),
                    ..Default::default()
                }),
                description: Some(
                    "Clears the local defuddle markdown cache. Use this when cached results appear stale or outdated."
                        .to_string(),
                ),
                input_schema: schema_for!(ClearCacheArguments),
                title: Some("Clear Cache".to_string()),

                ..Default::default()
            },
        ],
    })
}

// ---------------------------------------------------------------------------
// MCP: list_resource_templates
// ---------------------------------------------------------------------------

pub(crate) fn list_resource_templates(
    _input: ListResourceTemplatesRequest,
) -> Result<ListResourceTemplatesResult> {
    Ok(ListResourceTemplatesResult {
        resource_templates: vec![
            ResourceTemplate {
                annotations: None,
                description: Some(
                    "Fetch any https URL and return its content as Markdown via defuddle.md"
                        .to_string(),
                ),
                mime_type: Some("text/markdown".to_string()),
                name: "defuddle-https".to_string(),
                title: Some("Web page to Markdown (HTTPS)".to_string()),
                uri_template: "https://{+url}".to_string(),
            },
            ResourceTemplate {
                annotations: None,
                description: Some(
                    "Fetch any http URL and return its content as Markdown via defuddle.md"
                        .to_string(),
                ),
                mime_type: Some("text/markdown".to_string()),
                name: "defuddle-http".to_string(),
                title: Some("Web page to Markdown (HTTP)".to_string()),
                uri_template: "http://{+url}".to_string(),
            },
        ],
    })
}

// ---------------------------------------------------------------------------
// MCP: read_resource
// ---------------------------------------------------------------------------

pub(crate) fn read_resource(input: ReadResourceRequest) -> Result<ReadResourceResult> {
    let uri = &input.request.uri;

    // Validate that the URI is http or https
    if let Err(e) = validate_url(uri) {
        return Err(anyhow!(e));
    }

    let tool_result = fetch_defuddle_markdown(uri);

    // If defuddle returned an error, propagate it
    if tool_result.is_error.unwrap_or(false) {
        let msg = tool_result
            .content
            .first()
            .and_then(|c| match c {
                ContentBlock::Text(t) => Some(t.text.clone()),
                _ => None,
            })
            .unwrap_or_else(|| "Unknown error fetching resource".to_string());
        return Err(anyhow!(msg));
    }

    // Extract the markdown text from the tool result
    let markdown = tool_result
        .content
        .into_iter()
        .filter_map(|c| match c {
            ContentBlock::Text(t) => Some(t.text),
            _ => None,
        })
        .collect::<Vec<_>>()
        .join("\n");

    Ok(ReadResourceResult {
        contents: vec![ResourceContents::Text(TextResourceContents {
            meta: None,
            mime_type: Some("text/markdown".to_string()),
            text: markdown,
            uri: uri.clone(),
        })],
    })
}

// ---------------------------------------------------------------------------
// MCP: stubs for unimplemented capabilities
// ---------------------------------------------------------------------------

pub(crate) fn complete(_input: CompleteRequest) -> Result<CompleteResult> {
    Ok(CompleteResult::default())
}

pub(crate) fn get_prompt(_input: GetPromptRequest) -> Result<GetPromptResult> {
    Err(anyhow!("get_prompt not implemented"))
}

pub(crate) fn list_prompts(_input: ListPromptsRequest) -> Result<ListPromptsResult> {
    Ok(ListPromptsResult::default())
}

pub(crate) fn list_resources(_input: ListResourcesRequest) -> Result<ListResourcesResult> {
    Ok(ListResourcesResult::default())
}

pub(crate) fn on_roots_list_changed(_input: PluginNotificationContext) -> Result<()> {
    Ok(())
}
