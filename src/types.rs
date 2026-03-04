use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::hash::{Hash, Hasher};

/// Arguments for the `defuddle` tool that converts a web page to Markdown.
#[derive(Default, Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub(crate) struct DefuddleArguments {
    #[schemars(
        description = "The URL to fetch and convert to Markdown using defuddle.md. Must be an http:// or https:// URL."
    )]
    pub url: String,
}

/// Hash is derived from the URL so cache lookups are deterministic.
impl Hash for DefuddleArguments {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.url.hash(state);
    }
}

/// Arguments for the `clear_cache` tool (no parameters required).
#[allow(dead_code)]
#[derive(Default, Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub(crate) struct ClearCacheArguments {}
