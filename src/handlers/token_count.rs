use axum::{
    extract::State,
    http::StatusCode,
    response::Result,
};
use serde_json::{json, Value};
use crate::constants::*;
use crate::models::{App, ClaudeTokenCountRequest};

/// Count tokens using tiktoken (cl100k_base encoding baseline)
pub async fn count_tokens(
    State(_app): State<App>,
    axum::Json(req): axum::Json<ClaudeTokenCountRequest>,
) -> Result<axum::Json<Value>, (StatusCode, &'static str)> {
    let mut text_parts = Vec::new();
    let mut image_count = 0;

    if let Some(sys) = &req.system {
        let sys_text = if sys.is_string() {
            sys.as_str().unwrap_or("").to_string()
        } else if let Some(blocks) = sys.as_array() {
            blocks
                .iter()
                .filter_map(|block| {
                    block
                        .as_object()
                        .and_then(|obj| {
                            if obj.get("type").and_then(|t| t.as_str()) == Some("text") {
                                obj.get("text").and_then(|t| t.as_str())
                            } else {
                                None
                            }
                        })
                })
                .collect::<Vec<_>>()
                .join("\n")
        } else {
            String::new()
        };
        if !sys_text.is_empty() {
            text_parts.push(sys_text);
        }
    }

    for msg in &req.messages {
        let (msg_text, msg_image_count) = crate::utils::content_extraction::extract_text_from_content(&msg.content);
        if !msg_text.is_empty() {
            text_parts.push(format!("{}: {}", msg.role, msg_text));
        }
        image_count += msg_image_count;
    }

    if let Some(tools) = &req.tools {
        for tool in tools {
            text_parts.push(tool.name.clone());
            if let Some(desc) = &tool.description {
                text_parts.push(desc.clone());
            }
            if let Ok(schema_str) = serde_json::to_string(&tool.input_schema) {
                text_parts.push(schema_str);
            }
        }
    }

    let combined_text = text_parts.join("\n");

    let token_count = tokio::task::spawn_blocking(move || {
        match tiktoken_rs::cl100k_base() {
            Ok(encoder) => {
                let text_tokens = encoder.encode_with_special_tokens(&combined_text).len();
                let image_tokens = image_count * TOKENS_PER_IMAGE;
                text_tokens + image_tokens
            }
            Err(e) => {
                log::warn!("Failed to initialize tiktoken: {}, falling back to estimation", e);
                let text_estimate = std::cmp::max(1, combined_text.len() / CHARS_PER_TOKEN);
                let image_tokens = image_count * TOKENS_PER_IMAGE;
                text_estimate + image_tokens
            }
        }
    })
    .await
    .map_err(|_| (StatusCode::INTERNAL_SERVER_ERROR, "tokenization_failed"))?;

    Ok(axum::Json(json!({ "input_tokens": token_count })))
}