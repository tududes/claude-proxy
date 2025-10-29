use axum::{
    extract::State,
    http::{header::AUTHORIZATION, HeaderMap, HeaderName, StatusCode},
    response::sse::{Event, Sse},
    routing::post,
    Router,
};
use futures::{Stream, StreamExt};
use log::{error, info, warn};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::{
    collections::HashMap,
    convert::Infallible,
    env,
    sync::Arc,
    time::{SystemTime, UNIX_EPOCH, Duration},
};
use tokio::sync::RwLock;
use tokio_stream::wrappers::ReceiverStream;

// ---------- Claude (complete types) ----------
#[derive(Deserialize, Debug)]
struct ClaudeImageSource {
    #[serde(rename = "type")]
    #[allow(dead_code)]
    source_type: String,
    media_type: String,
    data: String,
}

#[derive(Deserialize, Debug)]
#[serde(tag = "type")]
enum ClaudeContentBlock {
    #[serde(rename = "text")]
    Text { text: String },
    #[serde(rename = "image")]
    Image { source: ClaudeImageSource },
    #[serde(rename = "tool_use")]
    ToolUse {
        id: String,
        name: String,
        input: Value,
    },
    #[serde(rename = "tool_result")]
    ToolResult {
        tool_use_id: String,
        content: Value,
        #[serde(default)]
        #[allow(dead_code)]
        is_error: Option<bool>,
    },
}

#[derive(Deserialize)]
struct ClaudeMessage {
    role: String,
    content: Value, // Can be String or Vec<ClaudeContentBlock>
}

#[derive(Deserialize)]
struct ClaudeTool {
    name: String,
    #[serde(default)]
    description: Option<String>,
    input_schema: Value,
}

#[derive(Deserialize)]
struct ClaudeRequest {
    model: String,
    messages: Vec<ClaudeMessage>,
    #[serde(default)]
    system: Option<Value>,
    #[serde(default)]
    max_tokens: Option<u32>,
    #[serde(default)]
    temperature: Option<f32>,
    #[serde(default)]
    top_p: Option<f32>,
    #[serde(default)]
    stop_sequences: Option<Vec<String>>,
    #[serde(default)]
    tools: Option<Vec<ClaudeTool>>,
    #[serde(default)]
    _stream: Option<bool>,
}

#[derive(Deserialize)]
struct ClaudeTokenCountRequest {
    #[allow(dead_code)]
    model: String,
    messages: Vec<ClaudeMessage>,
    #[serde(default)]
    system: Option<Value>,
    #[serde(default)]
    tools: Option<Vec<ClaudeTool>>,
}

// ---------- OpenAI (complete types) ----------
#[derive(Serialize)]
struct OAIMessage {
    role: String,
    content: Value, // Can be String or Array for multimodal
    #[serde(skip_serializing_if = "Option::is_none")]
    tool_call_id: Option<String>, // For tool response messages
    #[serde(skip_serializing_if = "Option::is_none")]
    tool_calls: Option<Vec<Value>>, // For assistant tool use messages
}
#[derive(Serialize)]
struct OAIFunction {
    name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    description: Option<String>,
    parameters: Value,
}
#[derive(Serialize)]
struct OAITool {
    #[serde(rename = "type")]
    type_: String,
    function: OAIFunction,
}
#[derive(Serialize)]
struct OAIChatReq {
    model: String,
    messages: Vec<OAIMessage>,
    #[serde(skip_serializing_if = "Option::is_none")]
    max_tokens: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    temperature: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    top_p: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    stop: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    tools: Option<Vec<OAITool>>,
    stream: bool,
}

#[derive(Deserialize, Default, Debug)]
struct OAIToolFunctionDelta {
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    arguments: Option<String>,
}
#[derive(Deserialize, Default, Debug)]
struct OAIToolCallDelta {
    #[serde(default)]
    index: Option<usize>,
    #[serde(default)]
    id: Option<String>,
    #[serde(default, rename = "type")]
    _type: Option<String>,
    #[serde(default)]
    function: Option<OAIToolFunctionDelta>,
}
#[derive(Deserialize, Default, Debug)]
struct OAIChoiceDelta {
    #[serde(default)]
    _role: Option<String>,
    #[serde(default)]
    content: Option<String>,
    #[serde(default)]
    tool_calls: Option<Vec<OAIToolCallDelta>>,
    // Claude extended thinking/reasoning mode
    #[serde(default)]
    reasoning_content: Option<String>,
}
#[derive(Deserialize, Default, Debug)]
struct OAIChoice {
    #[serde(default)]
    _index: usize,
    delta: OAIChoiceDelta,
    #[serde(default)]
    _finish_reason: Option<String>,
}
#[derive(Deserialize, Default, Debug)]
struct OAIStreamChunk {
    _id: Option<String>,
    _object: Option<String>,
    _created: Option<i64>,
    _model: Option<String>,
    choices: Vec<OAIChoice>,
}

// ---------- App with cached models ----------
#[derive(Clone)]
struct App {
    client: reqwest::Client,
    backend_url: String,
    backend_key: Option<String>,
    models_cache: Arc<RwLock<Option<Vec<String>>>>,
}

#[tokio::main]
async fn main() {
    // Load .env file if present
    let _ = dotenvy::dotenv();

    // Initialize logger with RUST_LOG env var (defaults to INFO)
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info")).init();

    let backend_url = env::var("BACKEND_URL")
        .unwrap_or_else(|_| "http://127.0.0.1:8000/v1/chat/completions".into());
    let backend_key = env::var("BACKEND_KEY").ok();

    info!("🚀 Claude-to-OpenAI Proxy starting...");
    info!("   Backend URL: {}", backend_url);
    info!(
        "   Backend Key: {}",
        if backend_key.is_some() {
            "Set (fallback)"
        } else {
            "Not set"
        }
    );
    info!("   Mode: Passthrough with case-correction");
    info!("   Listening on: 0.0.0.0:8080");

    let models_cache = Arc::new(RwLock::new(None));
    
    let app = App {
        client: reqwest::Client::builder()
            .pool_max_idle_per_host(1024)
            .build()
            .unwrap(),
        backend_url: backend_url.clone(),
        backend_key,
        models_cache: models_cache.clone(),
    };
    
    // Spawn background task to refresh models cache every 60 seconds
    {
        let app_clone = app.clone();
        tokio::spawn(async move {
            loop {
                if let Err(e) = refresh_models_cache(&app_clone).await {
                    warn!("Failed to refresh models cache: {}", e);
                }
                tokio::time::sleep(Duration::from_secs(60)).await; // 60 seconds
            }
        });
    }

    let router = Router::new()
        .route("/v1/messages", post(messages))
        .route("/v1/messages/count_tokens", post(count_tokens))
        .with_state(app);

    let listener = tokio::net::TcpListener::bind("0.0.0.0:8080").await.unwrap();
    axum::serve(listener, router).await.unwrap();
}

/// Refresh the models cache from backend
async fn refresh_models_cache(app: &App) -> Result<(), Box<dyn std::error::Error>> {
    // Extract base URL (remove /chat/completions)
    let models_url = app.backend_url.replace("/v1/chat/completions", "/v1/models");
    
    info!("🔄 Fetching available models from {}", models_url);
    
    let res = app.client
        .get(&models_url)
        .send()
        .await?;
    
    if !res.status().is_success() {
        return Err(format!("Models endpoint returned {}", res.status()).into());
    }
    
    let data: Value = res.json().await?;
    let models: Vec<String> = data["data"]
        .as_array()
        .map(|arr| {
            arr.iter()
                .filter_map(|m| m["id"].as_str().map(String::from))
                .collect()
        })
        .unwrap_or_default();
    
    info!("✅ Cached {} models from backend", models.len());
    
    let mut cache = app.models_cache.write().await;
    *cache = Some(models);
    
    Ok(())
}

/// Get cached models or fetch if not available
async fn get_available_models(app: &App) -> Vec<String> {
    // Try cache first
    {
        let cache = app.models_cache.read().await;
        if let Some(models) = cache.as_ref() {
            return models.clone();
        }
    }
    
    // Cache miss - fetch now
    if let Err(e) = refresh_models_cache(app).await {
        warn!("Failed to fetch models: {}", e);
        return vec![];
    }
    
    let cache = app.models_cache.read().await;
    cache.as_ref().cloned().unwrap_or_default()
}

/// Passthrough model with case-correction from cache
async fn normalize_model_name(model: &str, models_cache: &Arc<RwLock<Option<Vec<String>>>>) -> String {
    let model_lower = model.to_lowercase();
    
    // Check cache for case-insensitive match
    let cache = models_cache.read().await;
    if let Some(models) = cache.as_ref() {
        // Find exact match first
        if models.iter().any(|m| m == model) {
            return model.to_string();
        }
        
        // Try case-insensitive match and correct casing
        if let Some(matched) = models.iter().find(|m| m.to_lowercase() == model_lower) {
            info!("🔄 Model: {} → {} (case-corrected)", model, matched);
            return matched.clone();
        }
    }
    
    // No match found in cache, pass through as-is
    model.to_string()
}

/// Serialize tool_result content to a string for OpenAI
fn serialize_tool_result_content(content: &Value) -> String {
    if let Some(s) = content.as_str() {
        return s.to_string();
    }
    
    if let Some(arr) = content.as_array() {
        let parts: Vec<String> = arr
            .iter()
            .filter_map(|item| {
                if let Some(obj) = item.as_object() {
                    if obj.get("type").and_then(|t| t.as_str()) == Some("text") {
                        obj.get("text").and_then(|t| t.as_str()).map(String::from)
                    } else {
                        Some(serde_json::to_string(item).unwrap_or_else(|_| "{}".into()))
                    }
                } else if let Some(s) = item.as_str() {
                    Some(s.to_string())
                } else {
                    Some(serde_json::to_string(item).unwrap_or_else(|_| "{}".into()))
                }
            })
            .collect();
        return parts.join("\n");
    }
    
    serde_json::to_string(content).unwrap_or_else(|_| "{}".into())
}

/// Mask sensitive tokens for logs while keeping useful context
fn mask_token(token: &str) -> String {
    if token.len() > 12 {
        format!("{}...{}", &token[..6], &token[token.len() - 4..])
    } else if !token.is_empty() {
        "***".to_string()
    } else {
        "<empty>".into()
    }
}

/// Normalize an Authorization header value into a bare API key
fn normalize_auth_value_to_key(value: &str) -> String {
    value
        .trim()
        .strip_prefix("Bearer ")
        .map(str::trim)
        .unwrap_or(value.trim())
        .to_string()
}

/// Convert Claude system prompt value (string or array of blocks) into OpenAI system content
fn convert_system_content(sys: &Value) -> Value {
    if sys.is_string() {
        sys.clone()
    } else if let Some(blocks) = sys.as_array() {
        let combined_text = blocks
            .iter()
            .filter_map(|block| block.as_object())
            .filter(|obj| obj.get("type").and_then(|t| t.as_str()) == Some("text"))
            .filter_map(|obj| obj.get("text").and_then(|t| t.as_str()))
            .collect::<Vec<_>>()
            .join("\n");
        json!(combined_text)
    } else {
        sys.clone() // Fallback for unknown format
    }
}

/// Build OpenAI tools array from Claude tools, preserving previous behavior of defaulting to an empty tools array
fn build_oai_tools(tools: Option<Vec<ClaudeTool>>) -> Option<Vec<OAITool>> {
    match tools {
        Some(ts) if !ts.is_empty() => Some(
            ts.into_iter()
                .map(|t| OAITool {
                    type_: "function".into(),
                    function: OAIFunction {
                        name: t.name,
                        description: t.description,
                        parameters: t.input_schema,
                    },
                })
                .collect::<Vec<_>>(),
        ),
        _ => Some(vec![]),
    }
}

/// Build the markdown content for the synthetic 404 response listing available models
fn build_model_list_content(requested_model: &str, models: &[String]) -> String {
    let mut content = format!(
        "❌ Model `{}` not found.\n\n## 📋 Available Models ({} total)\n\n",
        requested_model,
        models.len()
    );

    // Categorize models
    let mut free_models: Vec<&str> = vec![];
    let mut reasoning_models: Vec<&str> = vec![];
    let mut non_reasoning_models: Vec<&str> = vec![];

    for model in models {
        let lower = model.to_lowercase();
        if lower.contains("glm")
            || lower.contains("gemma")
            || lower.contains("qwen")
            || lower.contains("gemini-2.0-flash")
        {
            free_models.push(model.as_str());
        } else if lower.contains("deepseek-r1") || lower.contains("qwq") || lower.contains("skywork-o1") {
            reasoning_models.push(model.as_str());
        } else {
            non_reasoning_models.push(model.as_str());
        }
    }

    // Price label helper
    let get_price = |m: &str| -> &'static str {
        let lower = m.to_lowercase();
        if lower.contains("glm") || lower.contains("gemma") {
            "FREE"
        } else if lower.contains("qwen") || lower.contains("gemini-2.0-flash") {
            "FREE"
        } else if lower.contains("deepseek-r1") && !lower.contains("distill") {
            "$$$"
        } else if lower.contains("deepseek-r1-distill") {
            "$$"
        } else if lower.contains("qwq") || lower.contains("skywork-o1") {
            "$$$"
        } else if lower.contains("claude-3-5-haiku") {
            "$"
        } else if lower.contains("claude-3-5-sonnet") {
            "$$"
        } else if lower.contains("claude-3-7-sonnet") {
            "$$$"
        } else if lower.contains("claude-3-opus") {
            "$$$$"
        } else if lower.contains("gpt-4o-mini") {
            "$"
        } else if lower.contains("gpt-4o") {
            "$$"
        } else if lower.contains("gpt-4") {
            "$$$"
        } else if lower.contains("deepseek-chat") {
            "$"
        } else if lower.contains("deepseek") {
            "$"
        } else {
            "$$"
        }
    };

    // Sort alphabetically by name (descending Z→A)
    free_models.sort_by(|a, b| b.to_lowercase().cmp(&a.to_lowercase()));
    reasoning_models.sort_by(|a, b| b.to_lowercase().cmp(&a.to_lowercase()));
    non_reasoning_models.sort_by(|a, b| b.to_lowercase().cmp(&a.to_lowercase()));

    // Helper function to format models in two columns with prices
    let format_two_columns = |models: &[&str]| -> String {
        let mut result = String::new();
        let half = (models.len() + 1) / 2;
        for i in 0..half {
            if let Some(&left_model) = models.get(i) {
                let left_price = get_price(left_model);
                let left_formatted = format!("{:4} {}", left_price, left_model);
                if let Some(&right_model) = models.get(i + half) {
                    let right_price = get_price(right_model);
                    let right_formatted = format!("{:4} {}", right_price, right_model);
                    result.push_str(&format!("  {:48} {}\n", left_formatted, right_formatted));
                } else {
                    result.push_str(&format!("  {}\n", left_formatted));
                }
            }
        }
        result
    };

    if !free_models.is_empty() {
        content.push_str("### 💚 FREE / LOW-COST\n\n");
        content.push_str(&format_two_columns(&free_models));
        content.push('\n');
    }
    if !reasoning_models.is_empty() {
        content.push_str("### 🧠 REASONING (Extended Thinking)\n\n");
        content.push_str(&format_two_columns(&reasoning_models));
        content.push('\n');
    }
    if !non_reasoning_models.is_empty() {
        content.push_str("### ⚡ STANDARD\n\n");
        content.push_str(&format_two_columns(&non_reasoning_models));
        content.push('\n');
    }

    content.push_str("---\n\n💡 **To switch models:** Use `/model <model-name>`");
    content
}

async fn count_tokens(
    State(_app): State<App>,
    axum::Json(req): axum::Json<ClaudeTokenCountRequest>,
) -> Result<axum::Json<Value>, (StatusCode, &'static str)> {
    // Simple token estimation (4 chars ≈ 1 token)
    let mut total_chars = 0;
    
    // Count system message
    if let Some(sys) = &req.system {
        if let Some(s) = sys.as_str() {
            total_chars += s.len();
        } else if let Some(blocks) = sys.as_array() {
            for block in blocks {
                if let Some(text) = block.get("text").and_then(|t| t.as_str()) {
                    total_chars += text.len();
                }
            }
        }
    }
    
    // Count message content
    for msg in &req.messages {
        if let Some(s) = msg.content.as_str() {
            total_chars += s.len();
        } else if let Some(blocks) = msg.content.as_array() {
            for block in blocks {
                if let Some(text) = block.get("text").and_then(|t| t.as_str()) {
                    total_chars += text.len();
                }
            }
        }
    }
    
    // Count tool schemas (rough estimate)
    if let Some(tools) = &req.tools {
        for tool in tools {
            total_chars += tool.name.len();
            if let Some(desc) = &tool.description {
                total_chars += desc.len();
            }
            total_chars += serde_json::to_string(&tool.input_schema)
                .unwrap_or_default()
                .len();
        }
    }
    
    let estimated_tokens = std::cmp::max(1, total_chars / 4);
    
    Ok(axum::Json(json!({
        "input_tokens": estimated_tokens
    })))
}

async fn messages(
    State(app): State<App>,
    headers: HeaderMap,
    axum::Json(cr): axum::Json<ClaudeRequest>,
) -> Result<
    (
        HeaderMap,
        Sse<impl Stream<Item = Result<Event, Infallible>>>,
    ),
    (StatusCode, &'static str),
> {
    // Debug: Log ALL incoming headers
    log::debug!("📥 All incoming headers:");
    for (name, value) in headers.iter() {
        log::debug!("   {}: {:?}", name, value);
    }

    // Log incoming authorization - check both 'authorization' and 'x-api-key' headers
    let x_api_key_header = HeaderName::from_static("x-api-key");
    let raw_x_api_key = headers
        .get(&x_api_key_header)
        .and_then(|h| h.to_str().ok())
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty());
    let raw_authorization = headers
        .get(AUTHORIZATION)
        .and_then(|h| h.to_str().ok())
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty());

    let client_key = raw_authorization
        .as_ref()
        .map(|auth| normalize_auth_value_to_key(auth))
        .or_else(|| raw_x_api_key.clone());

    if let Some(key) = &client_key {
        info!("🔑 Client API Key: Bearer {}", mask_token(key));
        log::debug!("🔑 Full client token: {}", key);
    } else {
        info!("🔑 No client API key (no 'authorization' or 'x-api-key' header)");
    }

    if let Some(raw_key) = &raw_x_api_key {
        log::debug!("🔑 Client x-api-key header: {}", mask_token(raw_key));
    }

    let has_client_auth = client_key.is_some();
    info!(
        "📨 Request: model={}, client_auth={}, backend={}",
        cr.model, has_client_auth, app.backend_url
    );

    // Normalize model name (case-correction only)
    let backend_model = normalize_model_name(&cr.model, &app.models_cache).await;
    let mut msgs = Vec::with_capacity(cr.messages.len() + 1);
    if let Some(sys) = cr.system {
        // Transform system prompt from Claude's format (string or content blocks) to OpenAI's simple string format
        let system_content = convert_system_content(&sys);
        msgs.push(OAIMessage {
            role: "system".into(),
            content: system_content,
            tool_call_id: None,
            tool_calls: None,
        });
    }
    
    let original_message_count = cr.messages.len();
    
    // Process messages with comprehensive content block support
    for m in cr.messages {
        if m.content.is_string() {
            // Simple string content - passthrough
            log::debug!("📝 Processing simple string message (role={})", m.role);
            msgs.push(OAIMessage {
                role: m.role,
                content: m.content,
                tool_call_id: None,
                tool_calls: None,
            });
            continue;
        }

        // Parse content blocks
        log::debug!("🔍 Attempting to parse content blocks for role={}", m.role);
        let blocks = match serde_json::from_value::<Vec<ClaudeContentBlock>>(m.content.clone()) {
            Ok(b) => {
                log::debug!("✅ Successfully parsed {} content blocks", b.len());
                b
            }
            Err(e) => {
                // Fallback: treat as simple text
                log::debug!("⚠️  Failed to parse content blocks ({}), using fallback", e);
                msgs.push(OAIMessage {
                    role: m.role.clone(),
                    content: m.content,
                    tool_call_id: None,
                    tool_calls: None,
                });
                continue;
            }
        };

        // Check for tool_result blocks - these need special handling
        let has_tool_results = blocks.iter().any(|b| matches!(b, ClaudeContentBlock::ToolResult { .. }));
        
        if has_tool_results && m.role == "user" {
            // Split tool_result blocks into separate OpenAI tool messages
            for block in &blocks {
                if let ClaudeContentBlock::ToolResult { tool_use_id, content, .. } = block {
                    let tool_content = serialize_tool_result_content(content);
                    msgs.push(OAIMessage {
                        role: "tool".into(),
                        content: json!(tool_content),
                        tool_call_id: Some(tool_use_id.clone()),
                        tool_calls: None,
                    });
                }
            }
            
            // If there are also text blocks, add them as a user message
            let text_parts: Vec<&str> = blocks
                .iter()
                .filter_map(|b| match b {
                    ClaudeContentBlock::Text { text } => Some(text.as_str()),
                    _ => None,
                })
                .collect();
            
            if !text_parts.is_empty() {
                msgs.push(OAIMessage {
                    role: m.role,
                    content: json!(text_parts.join("\n")),
                    tool_call_id: None,
                    tool_calls: None,
                });
            }
        } else if m.role == "assistant" {
            // Handle assistant messages with potential tool_use blocks
            let mut text_parts = Vec::new();
            let mut tool_calls = Vec::new();
            
            for block in &blocks {
                match block {
                    ClaudeContentBlock::Text { text } => text_parts.push(text.as_str()),
                    ClaudeContentBlock::ToolUse { id, name, input } => {
                        tool_calls.push(json!({
                            "id": id,
                            "type": "function",
                            "function": {
                                "name": name,
                                "arguments": serde_json::to_string(input).unwrap_or_else(|_| "{}".into())
                            }
                        }));
                    }
                    _ => {}
                }
            }
            
            let content = if text_parts.is_empty() {
                Value::Null
            } else {
                json!(text_parts.join("\n"))
            };
            
            msgs.push(OAIMessage {
                role: m.role,
                content,
                tool_call_id: None,
                tool_calls: if tool_calls.is_empty() { None } else { Some(tool_calls) },
            });
        } else {
            // User messages: handle text and images
            let mut has_images = false;
            let mut oai_content_blocks = Vec::new();
            
            for block in &blocks {
                match block {
                    ClaudeContentBlock::Text { text } => {
                        oai_content_blocks.push(json!({
                            "type": "text",
                            "text": text
                        }));
                    }
                    ClaudeContentBlock::Image { source } => {
                        has_images = true;
                        // Convert Claude image format to OpenAI format
                        let data_uri = format!(
                            "data:{};base64,{}",
                            source.media_type,
                            source.data
                        );
                        oai_content_blocks.push(json!({
                            "type": "image_url",
                            "image_url": {
                                "url": data_uri
                            }
                        }));
                    }
                    _ => {}
                }
            }
            
            // If multimodal (has images), use array format; otherwise combine text
            let content = if has_images {
                json!(oai_content_blocks)
            } else {
                let text = oai_content_blocks
                    .iter()
                    .filter_map(|v| v.get("text").and_then(|t| t.as_str()))
                    .collect::<Vec<_>>()
                    .join("\n");
                json!(text)
            };
            
            msgs.push(OAIMessage {
                role: m.role,
                content,
                tool_call_id: None,
                tool_calls: None,
            });
        }
    }

    log::debug!("📊 Converted {} Claude messages into {} OpenAI messages", original_message_count, msgs.len());
    
    // Claude Code sometimes adds a partial assistant message to the end of the history.
    // We need to remove it to form a valid OpenAI request.
    if let Some(last_msg) = msgs.last() {
        if last_msg.role == "assistant" {
            info!("🚮 Filtering out final assistant placeholder message from Claude Code client.");
            msgs.pop();
            log::debug!("📊 After filtering: {} messages remaining", msgs.len());
        }
    }
    
    if msgs.is_empty() {
        error!("❌ No messages remaining after conversion!");
        return Err((StatusCode::BAD_REQUEST, "no_messages"));
    }

    let tools = build_oai_tools(cr.tools);

    let backend_model_for_error = backend_model.clone();
    
    let oai = OAIChatReq {
        model: backend_model,
        messages: msgs,
        max_tokens: cr.max_tokens.or(Some(4096)), // Default if not specified
        temperature: cr.temperature.or(Some(1.0)),
        top_p: cr.top_p,
        stop: cr.stop_sequences,
        tools,
        stream: true,
    };

    let mut req = app
        .client
        .post(&app.backend_url)
        .header("content-type", "application/json");

    // Smart auth routing:
    // - If client sends a backend-compatible key (e.g., cpk_*), forward it
    // - If client sends Anthropic key (sk-ant-*), replace with BACKEND_KEY
    // - If no client auth, use BACKEND_KEY as fallback
    let mut forwarded_auth_token: Option<String> = None;
    let mut fallback_backend_token: Option<String> = None;

    if let Some(key) = &client_key {
        // Treat anything not explicitly Anthropic OAuth as backend-ready
        if key.contains("sk-ant-") {
            info!("🔄 Auth: Replacing Anthropic OAuth token with BACKEND_KEY");
            log::debug!("   (Client sent sk-ant-* token, using configured backend key instead)");
        } else {
            req = req.bearer_auth(key);
            forwarded_auth_token = Some(key.clone());
            log::debug!("🔑 Forwarding client's backend-compatible key");
            info!("🔄 Auth: Forwarding client key to backend");
        }
    }

    if forwarded_auth_token.is_none() {
        if let Some(k) = &app.backend_key {
            req = req.bearer_auth(k);
            fallback_backend_token = Some(k.clone());
            log::debug!("🔑 Using configured BACKEND_KEY");
        } else {
            warn!("⚠️  No BACKEND_KEY configured - backend request will fail auth");
        }
    }

    // --- Start: New Request Logging ---
    let final_auth_header_for_log: Option<String> = forwarded_auth_token
        .as_ref()
        .map(|k| format!("Bearer {}", k))
        .or_else(|| {
            fallback_backend_token
                .as_ref()
                .map(|k| format!("Bearer {}", k))
        });

    if log::log_enabled!(log::Level::Debug) {
        if let Ok(json_body) = serde_json::to_string_pretty(&oai) {
            let auth_header_str = final_auth_header_for_log.as_deref().unwrap_or("Not Set");
            let masked_auth = if auth_header_str.starts_with("Bearer ") {
                format!(
                    "Bearer {}",
                    mask_token(auth_header_str.trim_start_matches("Bearer ").trim())
                )
            } else {
                mask_token(auth_header_str)
            };
            log::debug!(
                "\n------------------ Request to Chutes Backend ------------------\n\
                POST {}\n\
                Authorization: {}\n\
                Content-Type: application/json\n\
                \n\
                {}\n\
                -----------------------------------------------------------------",
                app.backend_url,
                masked_auth,
                json_body
            );
        }
    }
    // --- End: New Request Logging ---

    log::debug!("🚀 Sending request to backend with {} messages", oai.messages.len());
    let res = req.json(&oai).send().await.map_err(|e| {
        error!("❌ Backend connection failed: {}", e);
        (StatusCode::BAD_GATEWAY, "backend_unavailable")
    })?;

    let status = res.status();
    log::debug!("📥 Backend response status: {}", status);
    
    if !status.is_success() {
        // If 404, return synthetic response with available models
        if status == StatusCode::NOT_FOUND {
            let models = get_available_models(&app).await;
            if !models.is_empty() {
                info!("💡 Model '{}' not found - sending model list to user", backend_model_for_error);
                
                // Create synthetic SSE response with model list
                let (tx, rx) = tokio::sync::mpsc::channel::<Event>(64);
                let requested_model = backend_model_for_error.clone();
                let model_name_for_response = backend_model_for_error.clone();
                let models_for_task = models.clone(); // Clone models for the spawned task
                
                tokio::spawn(async move {
                    let models = models_for_task; // Use the cloned models inside task
                    log::debug!("🎬 Synthetic 404 response task started for model: {}", requested_model);
                    let now = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos();
                    
                    // message_start - Match exact format of successful responses
                    let start = json!({
                        "type": "message_start",
                        "message": {
                            "id": format!("msg_{}", now),
                            "type": "message",
                            "role": "assistant",
                            "content": [],
                            "model": model_name_for_response,
                            "stop_reason": Value::Null,
                            "stop_sequence": Value::Null,
                            "usage": {
                                "input_tokens": 0,
                                "output_tokens": 0
                            }
                        }
                    });
                    log::debug!("📤 Sending message_start for 404 response");
                    if tx.send(Event::default().event("message_start").data(start.to_string())).await.is_err() {
                        log::error!("❌ Failed to send message_start event");
                        return;
                    }
                    
                    // content_block_start
                    let block_start = json!({
                        "type": "content_block_start",
                        "index": 0,
                        "content_block": {
                            "type": "text",
                            "text": ""
                        }
                    });
                    log::debug!("📤 Sending content_block_start for 404 response");
                    if tx.send(Event::default().event("content_block_start").data(block_start.to_string())).await.is_err() {
                        log::error!("❌ Failed to send content_block_start event");
                        return;
                    }
                    
                    // Build content message with categorized models
                    let content = build_model_list_content(&requested_model, &models);
                    
                    // content_block_delta - Send the full content in one delta
                    let delta = json!({
                        "type": "content_block_delta",
                        "index": 0,
                        "delta": {
                            "type": "text_delta",
                            "text": content
                        }
                    });
                    log::debug!("📤 Sending content_block_delta for 404 response ({} chars)", content.len());
                    if tx.send(Event::default().event("content_block_delta").data(delta.to_string())).await.is_err() {
                        log::error!("❌ Failed to send content_block_delta event");
                        return;
                    }
                    
                    // content_block_stop
                    let block_stop = json!({
                        "type": "content_block_stop",
                        "index": 0
                    });
                    log::debug!("📤 Sending content_block_stop for 404 response");
                    if tx.send(Event::default().event("content_block_stop").data(block_stop.to_string())).await.is_err() {
                        log::error!("❌ Failed to send content_block_stop event");
                        return;
                    }
                    
                    // message_delta
                    let msg_delta = json!({
                        "type": "message_delta",
                        "delta": {
                            "stop_reason": "end_turn",
                            "stop_sequence": Value::Null
                        },
                        "usage": {
                            "output_tokens": 50
                        }
                    });
                    log::debug!("📤 Sending message_delta for 404 response");
                    if tx.send(Event::default().event("message_delta").data(msg_delta.to_string())).await.is_err() {
                        log::error!("❌ Failed to send message_delta event");
                        return;
                    }
                    
                    // message_stop
                    let msg_stop = json!({
                        "type": "message_stop"
                    });
                    log::debug!("📤 Sending message_stop for 404 response");
                    if tx.send(Event::default().event("message_stop").data(msg_stop.to_string())).await.is_err() {
                        log::error!("❌ Failed to send message_stop event");
                        return;
                    }
                    
                    log::debug!("🏁 Synthetic 404 response completed successfully");
                });
                
                let mut headers = HeaderMap::new();
                headers.insert("cache-control", "no-cache".parse().unwrap());
                headers.insert("connection", "keep-alive".parse().unwrap());
                let stream = ReceiverStream::new(rx).map(Ok::<Event, Infallible>);
                return Ok((headers, Sse::new(stream)));
            }
        }
        
        error!("❌ Backend returned error: {} {}", status.as_u16(), status.canonical_reason().unwrap_or(""));
        return Err((StatusCode::BAD_GATEWAY, "backend_error"));
    }

    info!("✅ Backend responded successfully ({})", status);

    let (tx, rx) = tokio::sync::mpsc::channel::<Event>(64);

    // Per-request ephemeral state for re-chunking.
    let model_for_header = oai.model.clone();

    tokio::spawn(async move {
        log::debug!("🎬 Streaming task started");
        
        // message_start
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let start = json!({
            "type":"message_start",
            "message": {
                "id": format!("msg_{now}"),
                "type":"message", "role":"assistant",
                "content": [], "model": model_for_header,
                "stop_reason": serde_json::Value::Null,
                "stop_sequence": serde_json::Value::Null,
                "usage": {"input_tokens":0, "output_tokens":0}
            }
        });
        log::debug!("📤 Sending message_start event");
        let _ = tx
            .send(
                Event::default()
                    .event("message_start")
                    .data(start.to_string()),
            )
            .await;

        let mut bytes_stream = res.bytes_stream();
        let mut buf: Vec<u8> = Vec::with_capacity(16 * 1024);

        // Block indexing
        let mut next_block_index: i32 = 0;
        let mut text_open = false;
        let mut text_index: i32 = -1;
        struct ToolBuf {
            block_index: i32,
            id: String,
            name: String,
        }
        let mut tools: HashMap<usize, ToolBuf> = HashMap::new();
        let mut chunk_count = 0;

        let mut json_accumulator = String::new();
        
        log::debug!("🌊 Starting to process byte stream from backend");
        while let Some(item) = bytes_stream.next().await {
            let Ok(chunk) = item else { 
                log::debug!("❌ Error reading chunk from stream");
                break 
            };
            chunk_count += 1;
            if chunk_count == 1 {
                log::debug!("📦 Received first chunk ({} bytes)", chunk.len());
            }
            buf.extend_from_slice(&chunk);

            // Process complete lines
            loop {
                let pos = buf.iter().position(|&b| b == b'\n');
                if pos.is_none() {
                    break;
                }
                let pos = pos.unwrap();
                let mut line = buf.drain(..=pos).collect::<Vec<u8>>();
                if line.ends_with(b"\n") {
                    line.pop();
                }
                if line.ends_with(b"\r") {
                    line.pop();
                }
                if line.is_empty() {
                    continue;
                }
                if !line.starts_with(b"data:") {
                    continue;
                }

                let data = String::from_utf8_lossy(&line[5..]).trim().to_string();
                if data == "[DONE]" {
                    log::debug!("🏁 Received [DONE] marker from backend");
                    break;
                }
                
                if data.is_empty() {
                    continue;
                }

                // Accumulate JSON across multiple data: lines
                json_accumulator.push_str(&data);
                
                // Check if we have complete JSON (balanced braces)
                let open_braces = json_accumulator.chars().filter(|&c| c == '{').count();
                let close_braces = json_accumulator.chars().filter(|&c| c == '}').count();
                if open_braces != close_braces {
                    // Need more data
                    continue;
                }

                // Try to parse the accumulated JSON
                let parsed: serde_json::Result<OAIStreamChunk> = serde_json::from_str(&json_accumulator);
                let chunk = match parsed {
                    Ok(c) => c,
                    Err(e) => {
                        log::warn!("⚠️  JSON parse failed after {} chars: {}", json_accumulator.len(), e);
                        json_accumulator.clear();
                        continue;
                    }
                };
                
                // Clear accumulator for next chunk
                json_accumulator.clear();
                if chunk.choices.is_empty() {
                    log::debug!("⚠️  Chunk has no choices, skipping");
                    continue;
                }
                let d = &chunk.choices[0].delta;

                log::debug!("📝 Delta: content={:?}, reasoning={:?}, tool_calls={}", 
                    d.content.as_ref().map(|c| &c[..std::cmp::min(50, c.len())]),
                    d.reasoning_content.as_ref().map(|r| &r[..std::cmp::min(30, r.len())]),
                    d.tool_calls.as_ref().map(|t| t.len()).unwrap_or(0));

                // Handle reasoning content (extended thinking mode) - emit but don't display
                // Claude Code expects this but may not show it to the user
                if let Some(r) = &d.reasoning_content {
                    if !r.is_empty() {
                        log::debug!("🧠 Reasoning content ({}  chars)", r.len());
                        // Reasoning content is streamed but typically not displayed in Claude Code
                        // We could emit it as a separate block type if needed, but for now we log it
                    }
                }

                if let Some(c) = &d.content {
                    if !c.is_empty() {
                        // Emit text start if not open
                        if !text_open {
                            text_index = next_block_index;
                            next_block_index += 1;
                            let ev = json!({"type":"content_block_start","index":text_index,
                                "content_block":{"type":"text","text":""}});
                            let _ = tx
                                .send(
                                    Event::default()
                                        .event("content_block_start")
                                        .data(ev.to_string()),
                                )
                                .await;
                            text_open = true;
                        }
                        let ev = json!({"type":"content_block_delta","index":text_index,
                            "delta":{"type":"text_delta","text":c}});
                        let _ = tx
                            .send(
                                Event::default()
                                    .event("content_block_delta")
                                    .data(ev.to_string()),
                            )
                            .await;
                    }
                }
                if let Some(tool_calls) = &d.tool_calls {
                    if !tool_calls.is_empty() {
                        // Emit text stop if open
                        if text_open {
                            let ev = json!({"type":"content_block_stop","index":text_index});
                            let _ = tx
                                .send(
                                    Event::default()
                                        .event("content_block_stop")
                                        .data(ev.to_string()),
                                )
                                .await;
                            text_open = false;
                        }
                        #[allow(clippy::map_entry)]
                        for tc in tool_calls {
                        let idx = tc.index.unwrap_or(0);
                        if !tools.contains_key(&idx) {
                            let id = tc.id.clone().unwrap_or_else(|| format!("tool_{idx}"));
                            let name = tc
                                .function
                                .as_ref()
                                .and_then(|f| f.name.clone())
                                .unwrap_or_else(|| "tool".into());
                            let tb = ToolBuf {
                                block_index: next_block_index,
                                id,
                                name,
                            };
                            next_block_index += 1;
                            let start = json!({"type":"content_block_start","index":tb.block_index,
                                "content_block":{"type":"tool_use","id":tb.id,"name":tb.name,"input":{}}});
                            let _ = tx
                                .send(
                                    Event::default()
                                        .event("content_block_start")
                                        .data(start.to_string()),
                                )
                                .await;
                            tools.insert(idx, tb);
                        }
                        if let Some(f) = &tc.function {
                            if let Some(args) = &f.arguments {
                                let ev = json!({"type":"content_block_delta",
                                    "index": tools.get(&idx).unwrap().block_index,
                                    "delta":{"type":"input_json_delta","partial_json": args}});
                                let _ = tx
                                    .send(
                                        Event::default()
                                            .event("content_block_delta")
                                            .data(ev.to_string()),
                                    )
                                    .await;
                            }
                        }
                    }
                    }
                }
            }
        }

        // Close any open blocks and finish message
        if text_open {
            let ev = json!({"type":"content_block_stop","index":text_index});
            let _ = tx
                .send(
                    Event::default()
                        .event("content_block_stop")
                        .data(ev.to_string()),
                )
                .await;
        }
        for tb in tools.values() {
            let stop = json!({"type":"content_block_stop","index":tb.block_index});
            let _ = tx
                .send(
                    Event::default()
                        .event("content_block_stop")
                        .data(stop.to_string()),
                )
                .await;
        }
        let md = json!({"type":"message_delta","delta":{"stop_reason":"end_turn","stop_sequence":null},
                        "usage":{"output_tokens":0}});
        log::debug!("📤 Sending message_delta event");
        let _ = tx
            .send(Event::default().event("message_delta").data(md.to_string()))
            .await;
        log::debug!("📤 Sending message_stop event");
        let _ = tx
            .send(
                Event::default()
                    .event("message_stop")
                    .data(json!({"type":"message_stop"}).to_string()),
            )
            .await;
        log::debug!("🏁 Streaming task completed");
    });

    let mut headers = HeaderMap::new();
    headers.insert("cache-control", "no-cache".parse().unwrap());
    headers.insert("connection", "keep-alive".parse().unwrap());

    let stream = ReceiverStream::new(rx).map(Ok::<Event, Infallible>);
    Ok((headers, Sse::new(stream)))
}
