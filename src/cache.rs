use crate::pdk::{imports::notify_logging_message, types::*};
use extism_pdk::config;
use serde_json::json;
use std::{
    collections::hash_map::DefaultHasher,
    fs,
    hash::{Hash, Hasher},
    path::Path,
    sync::OnceLock,
    time::{Duration, SystemTime},
};

const CACHE_DIR: &str = "/cache";
const DEFAULT_CACHE_DAYS: u64 = 1;

static CACHE_ENABLED: OnceLock<bool> = OnceLock::new();
static CACHE_TTL: OnceLock<Duration> = OnceLock::new();

fn is_enabled() -> bool {
    *CACHE_ENABLED.get_or_init(|| {
        let exists = Path::new(CACHE_DIR).is_dir();
        if !exists {
            notify_logging_message(LoggingMessageNotificationParam {
                data: json!("Cache directory /cache is not mounted; caching is disabled"),
                level: LoggingLevel::Info,
                ..Default::default()
            })
            .ok();
        }
        exists
    })
}

fn ttl() -> Duration {
    *CACHE_TTL.get_or_init(|| {
        let days = config::get("CACHE_TTL")
            .ok()
            .flatten()
            .and_then(|v| v.parse::<u64>().ok())
            .unwrap_or(DEFAULT_CACHE_DAYS);
        Duration::from_secs(days * 24 * 60 * 60)
    })
}

fn cache_path<T: Hash>(tool_name: &str, args: &T) -> String {
    let mut hasher = DefaultHasher::new();
    args.hash(&mut hasher);
    let hash = hasher.finish();
    format!("{}/{}_{:x}.json", CACHE_DIR, tool_name, hash)
}

fn is_fresh(path: &str) -> bool {
    let Ok(metadata) = fs::metadata(path) else {
        return false;
    };
    let Ok(modified) = metadata.modified() else {
        return false;
    };
    let Ok(elapsed) = SystemTime::now().duration_since(modified) else {
        return false;
    };
    elapsed < ttl()
}

pub(crate) fn get<T: Hash>(tool_name: &str, args: &T) -> Option<CallToolResult> {
    if !is_enabled() {
        return None;
    }

    let path = cache_path(tool_name, args);

    if !is_fresh(&path) {
        return None;
    }

    let data = fs::read_to_string(&path).ok()?;
    let result: CallToolResult = serde_json::from_str(&data).ok()?;
    Some(result)
}

pub(crate) fn put<T: Hash>(tool_name: &str, args: &T, result: &CallToolResult) {
    if !is_enabled() {
        return;
    }

    let path = cache_path(tool_name, args);

    let Ok(data) = serde_json::to_string(result) else {
        notify_logging_message(LoggingMessageNotificationParam {
            data: json!("Failed to serialize cache entry"),
            level: LoggingLevel::Warning,
            ..Default::default()
        })
        .ok();
        return;
    };

    if let Err(e) = fs::write(&path, data) {
        notify_logging_message(LoggingMessageNotificationParam {
            data: json!(format!("Failed to write cache file {}: {}", path, e)),
            level: LoggingLevel::Warning,
            ..Default::default()
        })
        .ok();
    }
}

pub(crate) fn clear() -> CallToolResult {
    if !is_enabled() {
        return CallToolResult {
            content: vec![ContentBlock::Text(TextContent {
                text: "Cache is not enabled (directory not mounted)".to_string(),
                ..Default::default()
            })],
            ..Default::default()
        };
    }

    let entries = match fs::read_dir(CACHE_DIR) {
        Ok(entries) => entries,
        Err(e) => {
            return CallToolResult::error(format!("Failed to read cache directory: {}", e));
        }
    };

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

    if errors.is_empty() {
        CallToolResult {
            content: vec![ContentBlock::Text(TextContent {
                text: format!("Cache cleared successfully ({} entries removed)", removed),
                ..Default::default()
            })],
            ..Default::default()
        }
    } else {
        CallToolResult::error(format!(
            "Failed to remove {} cache entries: {}",
            errors.len(),
            errors.join("; ")
        ))
    }
}
