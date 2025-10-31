use axum::{
    extract::State,
    http::{header::AUTHORIZATION, HeaderMap, HeaderName, StatusCode},
    response::sse::{Event, Sse},
    routing::{get, post},
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
    time::{Duration, SystemTime, UNIX_EPOCH},
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
    ToolUse { id: String, name: String, input: Value },
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
    content: Value, // String or Vec<ClaudeContentBlock>
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
    content: Value, // String or Array for multimodal
    #[serde(skip_serializing_if = "Option::is_none")]
    tool_call_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    tool_calls: Option<Vec<Value>>,
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
    // Extended reasoning streams (optional in some backends)
    #[serde(default)]
    reasoning_content: Option<String>,
}
#[derive(Deserialize, Default, Debug)]
struct OAIChoice {
    #[serde(default)]
    _index: usize,
    // Streaming responses use 'delta', non-streaming use 'message'
    #[serde(default)]
    delta: Option<OAIChoiceDelta>,
    // Non-streaming complete response (fallback)
    #[serde(default)]
    message: Option<serde_json::Value>,
    #[serde(default)]
    finish_reason: Option<String>,
}
#[derive(Deserialize, Default, Debug)]
// Unknown fields are ignored by default (no deny_unknown_fields attribute)
struct OAIStreamChunk {
    _id: Option<String>,
    _object: Option<String>,
    _created: Option<i64>,
    _model: Option<String>,
    #[serde(default)]
    choices: Vec<OAIChoice>,
    // Allow error fields for graceful handling
    #[serde(default)]
    error: Option<serde_json::Value>,
}

// ---------- Model info with pricing ----------
#[derive(Clone, Debug)]
struct ModelInfo {
    id: String,
    input_price_usd: Option<f64>,
    output_price_usd: Option<f64>,
    supported_features: Vec<String>,
}

// ---------- App with cached models and circuit breaker ----------
#[derive(Clone)]
struct App {
    client: reqwest::Client,
    backend_url: String,
    backend_key: Option<String>,
    models_cache: Arc<RwLock<Option<Vec<ModelInfo>>>>,
    circuit_breaker: Arc<RwLock<CircuitBreakerState>>,
}

// ---------- Circuit breaker state ----------
#[derive(Clone, Debug)]
struct CircuitBreakerState {
    consecutive_failures: u32,
    last_failure_time: Option<SystemTime>,
    is_open: bool,
}

impl CircuitBreakerState {
    fn new() -> Self {
        Self {
            consecutive_failures: 0,
            last_failure_time: None,
            is_open: false,
        }
    }

    fn record_success(&mut self) {
        self.consecutive_failures = 0;
        self.is_open = false;
        self.last_failure_time = None;
    }

    fn record_failure(&mut self) {
        self.consecutive_failures += 1;
        self.last_failure_time = Some(SystemTime::now());
        if self.consecutive_failures >= 5 {
            self.is_open = true;
            warn!("🔴 Circuit breaker opened after {} consecutive failures", self.consecutive_failures);
        }
    }

    fn should_allow_request(&mut self) -> bool {
        if !self.is_open {
            return true;
        }
        // Try to recover after 30 seconds
        if let Some(last_fail) = self.last_failure_time {
            if let Ok(elapsed) = SystemTime::now().duration_since(last_fail) {
                if elapsed.as_secs() >= 30 {
                    info!("🟡 Circuit breaker attempting half-open state");
                    self.is_open = false;
                    self.consecutive_failures = 0;
                    return true;
                }
            }
        }
        false
    }
}

#[tokio::main]
async fn main() {
    let _ = dotenvy::dotenv();

    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info")).init();

    let backend_url = env::var("BACKEND_URL")
        .unwrap_or_else(|_| "http://127.0.0.1:8000/v1/chat/completions".into());
    let backend_key = env::var("BACKEND_KEY").ok();
    let backend_timeout_secs = env::var("BACKEND_TIMEOUT_SECS")
        .ok()
        .and_then(|s| s.parse::<u64>().ok())
        .unwrap_or(600);

    info!("🚀 Claude-to-OpenAI Proxy starting...");
    info!("   Backend URL: {}", backend_url);
    info!(
        "   Backend Key: {}",
        if backend_key.is_some() { "Set (fallback)" } else { "Not set" }
    );
    info!("   Backend Timeout: {}s", backend_timeout_secs);
    info!("   Mode: Passthrough with case-correction");

    let models_cache = Arc::new(RwLock::new(None));
    let circuit_breaker = Arc::new(RwLock::new(CircuitBreakerState::new()));

    let app = App {
        client: reqwest::Client::builder()
            .pool_max_idle_per_host(1024)
            .tcp_keepalive(Some(Duration::from_secs(60)))
            .connect_timeout(Duration::from_secs(10))
            .timeout(Duration::from_secs(backend_timeout_secs))
            .build()
            .unwrap(),
        backend_url: backend_url.clone(),
        backend_key,
        models_cache: models_cache.clone(),
        circuit_breaker: circuit_breaker.clone(),
    };

    // Background model cache refresh (every 60s)
    {
        let app_clone = app.clone();
        tokio::spawn(async move {
            loop {
                if let Err(e) = refresh_models_cache(&app_clone).await {
                    warn!("Failed to refresh models cache: {}", e);
                }
                tokio::time::sleep(Duration::from_secs(60)).await;
            }
        });
    }

    let router = Router::new()
        .route("/health", get(health_check))
        .route("/v1/messages", post(messages))
        .route("/v1/messages/count_tokens", post(count_tokens))
        .layer(axum::extract::DefaultBodyLimit::max(10 * 1024 * 1024)) // 10MB limit
        .layer(tower_http::compression::CompressionLayer::new())
        .with_state(app);

    let port = env::var("HOST_PORT")
        .unwrap_or_else(|_| "8080".into())
        .parse::<u16>()
        .unwrap_or(8080);
    let listener = tokio::net::TcpListener::bind(format!("0.0.0.0:{}", port))
        .await
        .unwrap();
    info!("   Listening on: 0.0.0.0:{}", port);
    axum::serve(listener, router).await.unwrap();
}

/// Build `/v1/models` URL from backend chat completions URL.
fn models_url_from_backend_url(backend_url: &str) -> String {
    // best-effort: replace trailing `/v1/chat/completions` with `/v1/models`
    if let Some(idx) = backend_url.rfind("/v1/chat/completions") {
        let mut s = String::with_capacity(backend_url.len());
        s.push_str(&backend_url[..idx]);
        s.push_str("/v1/models");
        s
    } else {
        // fallback: assume same host, standard path
        format!("{}/../models", backend_url.trim_end_matches('/'))
    }
}

/// Refresh the models cache from backend
async fn refresh_models_cache(app: &App) -> Result<(), Box<dyn std::error::Error>> {
    let models_url = models_url_from_backend_url(&app.backend_url);
    info!("🔄 Fetching available models from {}", models_url);

    // Models endpoint is public (no auth required)
    let res = app.client.get(&models_url).send().await?;
    let status = res.status();
    if !status.is_success() {
        // Read error body for debugging
        let error_text = res.text().await.unwrap_or_else(|_| "".into());
        warn!(
            "❌ Models endpoint returned {} - response: {}",
            status,
            if error_text.len() > 200 {
                &error_text[..200]
            } else {
                &error_text
            }
        );
        return Err(format!("Models endpoint returned {}", status).into());
    }

    let data: Value = res.json().await?;
    let models: Vec<ModelInfo> = data["data"]
        .as_array()
        .map(|arr| {
            arr.iter()
                .filter_map(|m| {
                    let id = m["id"].as_str()?.to_string();
                    let input_price = m["price"]["input"]["usd"]
                        .as_f64()
                        .or_else(|| m["pricing"]["prompt"].as_f64());
                    let output_price = m["price"]["output"]["usd"]
                        .as_f64()
                        .or_else(|| m["pricing"]["completion"].as_f64());
                    let supported_features = m["supported_features"]
                        .as_array()
                        .map(|arr| {
                            arr.iter()
                                .filter_map(|v| v.as_str().map(String::from))
                                .collect()
                        })
                        .unwrap_or_default();
                    Some(ModelInfo {
                        id,
                        input_price_usd: input_price,
                        output_price_usd: output_price,
                        supported_features,
                    })
                })
                .collect()
        })
        .unwrap_or_default();

    info!("✅ Cached {} models from backend", models.len());
    let mut cache = app.models_cache.write().await;
    *cache = Some(models);
    Ok(())
}

/// Get cached models or fetch if not available
async fn get_available_models(app: &App) -> Vec<ModelInfo> {
    {
        let cache = app.models_cache.read().await;
        if let Some(models) = cache.as_ref() {
            return models.clone();
        }
    }
    if let Err(e) = refresh_models_cache(app).await {
        warn!("Failed to fetch models: {}", e);
        return vec![];
    }
    let cache = app.models_cache.read().await;
    cache.as_ref().cloned().unwrap_or_default()
}

/// Passthrough model with case-correction from cache
async fn normalize_model_name(model: &str, models_cache: &Arc<RwLock<Option<Vec<ModelInfo>>>>) -> String {
    let model_lower = model.to_lowercase();
    let cache = models_cache.read().await;
    if let Some(models) = cache.as_ref() {
        if models.iter().any(|m| m.id == model) {
            return model.to_string();
        }
        if let Some(matched) = models.iter().find(|m| m.id.to_lowercase() == model_lower) {
            info!("🔄 Model: {} → {} (case-corrected)", model, matched.id);
            return matched.id.clone();
        }
    }
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
        sys.clone()
    }
}

/// Build OpenAI tools array from Claude tools
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

/// Translate OpenAI finish_reason to Claude stop_reason
fn translate_finish_reason(oai_reason: Option<&str>) -> &'static str {
    match oai_reason {
        Some("stop") => "end_turn",
        Some("length") => "max_tokens",
        Some("tool_calls") | Some("function_call") => "tool_use",
        Some("content_filter") => "end_turn", // No direct equivalent
        Some("error") => "error",
        Some(other) => {
            log::debug!("⚠️  Unknown finish_reason '{}', using 'end_turn'", other);
            "end_turn"
        }
        None => "end_turn",
    }
}

/// Determine price tier based on input + output pricing (per million tokens)
fn get_price_tier(input_price: Option<f64>, output_price: Option<f64>) -> &'static str {
    let total_price = input_price.unwrap_or(0.0) + output_price.unwrap_or(0.0);
    if input_price.is_none() && output_price.is_none() {
        return "SUB";
    }
    if total_price == 0.0 {
        "SUB"
    } else if total_price <= 0.5 {
        "$"
    } else if total_price <= 2.0 {
        "$$"
    } else if total_price <= 5.0 {
        "$$$"
    } else {
        "$$$$"
    }
}

/// Format backend error into user-friendly structured message
fn format_backend_error(error_msg: &str, raw_json: &str) -> String {
    // Try to extract model name from context if available
    let model_name = if let Ok(val) = serde_json::from_str::<Value>(raw_json) {
        val.get("model")
            .and_then(|m| m.as_str())
            .map(String::from)
    } else {
        None
    };
    
    let mut formatted = String::from("⚠️ Backend Error\n\n");
    
    if let Some(model) = model_name {
        formatted.push_str(&format!("Model: {}\n", model));
    }
    
    formatted.push_str(&format!("Error: {}\n\n", error_msg));
    
    // Add specific suggestions based on error type
    if error_msg.contains("token") && error_msg.contains("exceed") {
        if let Some(requested) = error_msg.split("total of ").nth(1).and_then(|s| s.split(" tokens").next()) {
            formatted.push_str(&format!("Requested: {} tokens\n", requested));
        }
        if let Some(limit) = error_msg.split("maximum context length of ").nth(1).and_then(|s| s.split(" tokens").next()) {
            formatted.push_str(&format!("Limit: {} tokens\n\n", limit));
        }
        formatted.push_str("💡 Suggestions:\n");
        formatted.push_str("• Reduce message history\n");
        formatted.push_str("• Use a model with larger context\n");
        formatted.push_str("• Decrease max_tokens parameter\n");
    } else if error_msg.contains("rate limit") {
        formatted.push_str("💡 Suggestions:\n");
        formatted.push_str("• Wait a moment before retrying\n");
        formatted.push_str("• Check your API quota\n");
    } else if error_msg.contains("insufficient") || error_msg.contains("quota") {
        formatted.push_str("💡 Suggestions:\n");
        formatted.push_str("• Check your account balance\n");
        formatted.push_str("• Verify API key permissions\n");
    }
    
    formatted
}

/// Build markdown content for synthetic 404 response listing available models
fn build_model_list_content(requested_model: &str, models: &[ModelInfo]) -> String {
    let mut content = format!(
        "❌ Model `{}` not found.\n\n## 📋 Available Models ({} total)\n\n",
        requested_model,
        models.len()
    );

    let mut reasoning_models: Vec<&ModelInfo> = vec![];
    let mut standard_models: Vec<&ModelInfo> = vec![];

    for model in models {
        let has_reasoning = model
            .supported_features
            .iter()
            .any(|f| f.to_lowercase().contains("reasoning"));

        if has_reasoning {
            reasoning_models.push(model);
        } else {
            standard_models.push(model);
        }
    }

    let sort_models = |a: &&ModelInfo, b: &&ModelInfo| -> std::cmp::Ordering {
        let a_parts: Vec<&str> = a.id.split('/').collect();
        let b_parts: Vec<&str> = b.id.split('/').collect();

        let first_cmp = a_parts
            .get(0)
            .unwrap_or(&"")
            .to_lowercase()
            .cmp(&b_parts.get(0).unwrap_or(&"").to_lowercase());

        if first_cmp != std::cmp::Ordering::Equal {
            return first_cmp;
        }

        b_parts
            .get(1)
            .unwrap_or(&"")
            .to_lowercase()
            .cmp(&a_parts.get(1).unwrap_or(&"").to_lowercase())
    };

    reasoning_models.sort_by(sort_models);
    standard_models.sort_by(sort_models);

    let format_two_columns = |models: &[&ModelInfo]| -> String {
        let mut result = String::new();
        let half = (models.len() + 1) / 2;
        for i in 0..half {
            if let Some(&left_model) = models.get(i) {
                let left_price = get_price_tier(left_model.input_price_usd, left_model.output_price_usd);
                let left_formatted = format!("{:4} {}", left_price, left_model.id);
                if let Some(&right_model) = models.get(i + half) {
                    let right_price =
                        get_price_tier(right_model.input_price_usd, right_model.output_price_usd);
                    let right_formatted = format!("{:4} {}", right_price, right_model.id);
                    result.push_str(&format!("  {:48} {}\n", left_formatted, right_formatted));
                } else {
                    result.push_str(&format!("  {}\n", left_formatted));
                }
            }
        }
        result
    };

    if !reasoning_models.is_empty() {
        content.push_str("### 🧠 REASONING (Extended Thinking)\n\n");
        content.push_str(&format_two_columns(&reasoning_models));
        content.push('\n');
    }
    if !standard_models.is_empty() {
        content.push_str("### ⚡ STANDARD\n\n");
        content.push_str(&format_two_columns(&standard_models));
        content.push('\n');
    }

    content.push_str("---\n\n💡 **To switch models:** Use `/model <model-name>`");
    content
}

/// Extract text content from Claude content value (string or array of blocks)
/// Returns tuple: (text_content, image_count)
fn extract_text_from_content(content: &Value) -> (String, usize) {
    if let Some(s) = content.as_str() {
        return (s.to_string(), 0);
    }
    if let Some(blocks) = content.as_array() {
        let mut texts: Vec<String> = Vec::new();
        let mut image_count = 0;
        for block in blocks {
            if let Some(obj) = block.as_object() {
                let block_type = obj.get("type").and_then(|t| t.as_str());
                match block_type {
                    Some("text") => {
                        if let Some(text) = obj.get("text").and_then(|t| t.as_str()) {
                            texts.push(text.to_string());
                        }
                    }
                    Some("image") => {
                        image_count += 1;
                    }
                    Some("tool_result") => {
                        if let Some(result_content) = obj.get("content") {
                            if let Some(text) = result_content.as_str() {
                                texts.push(text.to_string());
                            } else if let Some(arr) = result_content.as_array() {
                                for item in arr {
                                    if let Some(text_obj) = item.as_object() {
                                        if text_obj.get("type").and_then(|t| t.as_str()) == Some("text") {
                                            if let Some(text) =
                                                text_obj.get("text").and_then(|t| t.as_str())
                                            {
                                                texts.push(text.to_string());
                                            }
                                        }
                                    } else if let Some(text) = item.as_str() {
                                        texts.push(text.to_string());
                                    }
                                }
                            }
                        }
                    }
                    Some("tool_use") => {
                        if let Some(name) = obj.get("name").and_then(|n| n.as_str()) {
                            texts.push(name.to_string());
                        }
                        if let Some(input) = obj.get("input") {
                            if let Ok(input_str) = serde_json::to_string(input) {
                                texts.push(input_str);
                            }
                        }
                    }
                    _ => {}
                }
            } else if let Some(s) = block.as_str() {
                texts.push(s.to_string());
            }
        }
        return (texts.join("\n"), image_count);
    }
    (String::new(), 0)
}

/// Health check endpoint
async fn health_check(State(app): State<App>) -> axum::Json<Value> {
    let models = get_available_models(&app).await;
    let circuit_breaker = app.circuit_breaker.read().await;
    
    let status = if circuit_breaker.is_open {
        "unhealthy"
    } else {
        "healthy"
    };
    
    axum::Json(json!({
        "status": status,
        "backend_url": app.backend_url,
        "models_cached": models.len(),
        "circuit_breaker": {
            "is_open": circuit_breaker.is_open,
            "consecutive_failures": circuit_breaker.consecutive_failures
        }
    }))
}

/// Count tokens using tiktoken (cl100k_base encoding baseline)
async fn count_tokens(
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
        let (msg_text, msg_image_count) = extract_text_from_content(&msg.content);
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
                let image_tokens = image_count * 85;
                text_tokens + image_tokens
            }
            Err(e) => {
                warn!("Failed to initialize tiktoken: {}, falling back to estimation", e);
                let text_estimate = std::cmp::max(1, combined_text.len() / 4);
                let image_tokens = image_count * 85;
                text_estimate + image_tokens
            }
        }
    })
    .await
    .map_err(|_| (StatusCode::INTERNAL_SERVER_ERROR, "tokenization_failed"))?;

    Ok(axum::Json(json!({ "input_tokens": token_count })))
}

/// Simple SSE event parser that accumulates lines until a blank line, then yields the combined `data:` payload.
/// This follows the SSE spec: multiple `data:` lines per event are joined by `\n`.
struct SseEventParser {
    buf: String,
    // Accumulates data: lines for the current event until blank line.
    cur_data_lines: Vec<String>,
}
impl SseEventParser {
    fn new() -> Self {
        Self {
            buf: String::with_capacity(16 * 1024),
            cur_data_lines: Vec::with_capacity(4),
        }
    }

    /// Feed bytes and extract zero or more complete SSE event payloads (already joined).
    fn push_and_drain_events(&mut self, chunk: &[u8]) -> Vec<String> {
        let s = String::from_utf8_lossy(chunk);
        self.buf.push_str(&s);
        let mut out = Vec::new();

        loop {
            // Find next newline
            let Some(pos) = self.buf.find('\n') else { break };
            // Take one line (retain possible preceding \r, we'll trim)
            let mut line = self.buf.drain(..=pos).collect::<String>();
            if line.ends_with('\n') {
                line.pop();
            }
            if line.ends_with('\r') {
                line.pop();
            }
            let trimmed = line.as_str();

            // Blank line => event terminator
            if trimmed.is_empty() {
                if !self.cur_data_lines.is_empty() {
                    let payload = self.cur_data_lines.join("\n");
                    self.cur_data_lines.clear();
                    out.push(payload);
                }
                continue;
            }

            // Only collect `data:` lines, ignore others (e.g., `event:`/`id:`)
            if let Some(rest) = trimmed.strip_prefix("data:") {
                self.cur_data_lines.push(rest.trim_start().to_string());
            }
        }

        out
    }

    /// Flush at end-of-stream (if the server doesn't send a final blank line).
    fn flush(mut self) -> Option<String> {
        if !self.cur_data_lines.is_empty() {
            let payload = self.cur_data_lines.join("\n");
            self.cur_data_lines.clear();
            Some(payload)
        } else {
            None
        }
    }
}

async fn messages(
    State(app): State<App>,
    headers: HeaderMap,
    axum::Json(cr): axum::Json<ClaudeRequest>,
) -> Result<
    (HeaderMap, Sse<impl Stream<Item = Result<Event, Infallible>>>),
    (StatusCode, &'static str),
> {
    let request_start = SystemTime::now();
    
    // Circuit breaker check
    {
        let mut cb = app.circuit_breaker.write().await;
        if !cb.should_allow_request() {
            error!("🔴 Circuit breaker is open - rejecting request");
            return Err((StatusCode::SERVICE_UNAVAILABLE, "backend_unavailable_circuit_open"));
        }
    }
    
    // Request validation
    if cr.messages.is_empty() {
        warn!("❌ Validation failed: empty messages");
        return Err((StatusCode::BAD_REQUEST, "empty_messages"));
    }
    
    if cr.messages.len() > 1000 {
        warn!("❌ Validation failed: too many messages ({})", cr.messages.len());
        return Err((StatusCode::BAD_REQUEST, "too_many_messages"));
    }
    
    // Validate message size (rough check)
    let total_content_size: usize = cr.messages.iter()
        .map(|m| {
            if let Some(s) = m.content.as_str() {
                s.len()
            } else {
                serde_json::to_string(&m.content).unwrap_or_default().len()
            }
        })
        .sum();
    
    if total_content_size > 5 * 1024 * 1024 {  // 5MB content limit
        warn!("❌ Validation failed: content too large ({} bytes)", total_content_size);
        return Err((StatusCode::PAYLOAD_TOO_LARGE, "content_too_large"));
    }
    
    // Debug: Log incoming headers (names only)
    log::debug!("📥 Incoming headers:");
    for (name, _) in headers.iter() {
        log::debug!("   {}", name);
    }

    // Auth extraction: Authorization or x-api-key
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
    } else {
        info!("🔑 No client API key (no 'authorization' or 'x-api-key' header)");
    }

    let has_client_auth = client_key.is_some();
    info!(
        "📨 Request: model={}, client_auth={}, backend={}",
        cr.model, has_client_auth, app.backend_url
    );

    // Normalize model name (case-correction only)
    let backend_model = normalize_model_name(&cr.model, &app.models_cache).await;
    let backend_model_for_metrics = backend_model.clone();
    let mut msgs = Vec::with_capacity(cr.messages.len() + 1);
    if let Some(sys) = cr.system {
        let system_content = convert_system_content(&sys);
        msgs.push(OAIMessage {
            role: "system".into(),
            content: system_content,
            tool_call_id: None,
            tool_calls: None,
        });
    }

    let original_message_count = cr.messages.len();

    // Convert Claude messages → OpenAI messages
    for m in cr.messages {
        if m.content.is_string() {
            // Simple string passthrough
            log::debug!("📝 Simple string message (role={})", m.role);
            msgs.push(OAIMessage {
                role: m.role,
                content: m.content,
                tool_call_id: None,
                tool_calls: None,
            });
            continue;
        }

        // Parse content blocks
        log::debug!("🔍 Parsing content blocks (role={})", m.role);
        let blocks = match serde_json::from_value::<Vec<ClaudeContentBlock>>(m.content.clone()) {
            Ok(b) => b,
            Err(e) => {
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

        // tool_result blocks require separate "tool" messages
        let has_tool_results = blocks.iter().any(|b| matches!(b, ClaudeContentBlock::ToolResult { .. }));

        if has_tool_results && m.role == "user" {
            // Split tool_result → OpenAI tool messages
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

            // Also pass any user text (if present) after tool results
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
            // Assistant messages may include tool_use blocks → OpenAI tool_calls
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
            // User messages with possible images
            let mut has_images = false;
            let mut oai_content_blocks = Vec::new();

            for block in &blocks {
                match block {
                    ClaudeContentBlock::Text { text } => {
                        oai_content_blocks.push(json!({ "type": "text", "text": text }));
                    }
                    ClaudeContentBlock::Image { source } => {
                        has_images = true;
                        info!(
                            "🖼️ Processing image: media_type={}, size={} bytes",
                            source.media_type,
                            source.data.len()
                        );
                        if source.data.starts_with("data:") {
                            warn!("⚠️ Image data already appears to be a data URI (double-encoding?)");
                        }
                        // Convert Claude image to OpenAI data URL
                        let data_uri = format!("data:{};base64,{}", source.media_type, source.data);
                        oai_content_blocks.push(json!({
                            "type": "image_url",
                            "image_url": { "url": data_uri }
                        }));
                    }
                    _ => {}
                }
            }

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

    log::debug!(
        "📊 Converted {} Claude messages into {} OpenAI messages",
        original_message_count,
        msgs.len()
    );

    // Claude Code sometimes adds an *empty* assistant placeholder; only remove if truly empty.
    if let Some(last_msg) = msgs.last() {
        let last_is_empty_assistant = last_msg.role == "assistant"
            && (last_msg.content.is_null()
                || (last_msg.content.is_string() && last_msg.content.as_str().unwrap_or("").is_empty()))
            && last_msg.tool_calls.as_ref().map(|v| v.is_empty()).unwrap_or(true);

        if last_is_empty_assistant {
            info!("🚮 Removing empty assistant placeholder message from client history.");
            let _ = msgs.pop();
            log::debug!("📊 After filtering: {} messages remaining", msgs.len());
        }
    }

    if msgs.is_empty() {
        error!("❌ No messages remaining after conversion!");
        return Err((StatusCode::BAD_REQUEST, "no_messages"));
    }

    let tools = build_oai_tools(cr.tools);

    let backend_model_for_error = backend_model.clone();

    // Preserve your behavior: always stream SSE to backend
    let oai = OAIChatReq {
        model: backend_model,
        messages: msgs,
        // Do not hard-default; allow backend default if None (safer across models)
        max_tokens: cr.max_tokens,
        temperature: cr.temperature,
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
    // - If client sends backend-compatible key (e.g., cpk_*), forward it
    // - If client sends Anthropic token (sk-ant-*), replace with BACKEND_KEY
    // - If no client auth, use BACKEND_KEY as fallback
    let mut forwarded_auth_token: Option<String> = None;
    let mut fallback_backend_token: Option<String> = None;

    if let Some(key) = &client_key {
        if key.contains("sk-ant-") {
            info!("🔄 Auth: Replacing Anthropic OAuth token with BACKEND_KEY");
        } else {
            req = req.bearer_auth(key);
            forwarded_auth_token = Some(key.clone());
            info!("🔄 Auth: Forwarding client key to backend");
        }
    }

    if forwarded_auth_token.is_none() {
        if let Some(k) = &app.backend_key {
            req = req.bearer_auth(k);
            fallback_backend_token = Some(k.clone());
            log::debug!("🔑 Using configured BACKEND_KEY");
        } else {
            warn!("⚠️  No BACKEND_KEY configured - backend request may fail auth");
        }
    }

    // Debug request body (image data truncated)
    if log::log_enabled!(log::Level::Debug) {
        if let Ok(mut json_body) = serde_json::to_string_pretty(&oai) {
            if json_body.contains("\"image_url\"") {
                // Try to truncate large data URL bodies in logs
                let needle = "\"url\": \"data:";
                if let Some(start) = json_body.find(needle) {
                    // naive truncation around the data url
                    let after = &json_body[start + needle.len()..];
                    if let Some(end_quote) = after.find('"') {
                        if end_quote > 120 {
                            let replace = format!("{}{}...TRUNCATED...\"", needle, &after[..120]);
                            let end_abs = start + needle.len() + end_quote + 1;
                            json_body.replace_range(start..end_abs, &replace);
                        }
                    }
                }
                log::info!("📸 Request contains image data (truncated in logs)");
            }
            let auth_header_str = forwarded_auth_token
                .as_ref()
                .map(|k| format!("Bearer {}", mask_token(k)))
                .or_else(|| fallback_backend_token.as_ref().map(|k| format!("Bearer {}", mask_token(k))))
                .unwrap_or_else(|| "Not Set".into());
            log::debug!(
                "\n------------------ Request to Backend ------------------\n\
                 POST {}\n\
                 Authorization: {}\n\
                 Content-Type: application/json\n\n\
                 {}\n\
                 ------------------------------------------------------------",
                app.backend_url,
                auth_header_str,
                json_body
            );
        }
    }

    log::debug!("🚀 Sending request to backend with {} messages", oai.messages.len());
    let res = req.json(&oai).send().await.map_err(|e| {
        error!("❌ Backend connection failed: {}", e);
        // Record circuit breaker failure
        tokio::spawn({
            let cb = app.circuit_breaker.clone();
            async move {
                cb.write().await.record_failure();
            }
        });
        (StatusCode::BAD_GATEWAY, "backend_unavailable")
    })?;

    let status = res.status();
    log::debug!("📥 Backend response status: {}", status);

    // Validate Content-Type for better error messages
    let content_type = res.headers()
        .get("content-type")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");
    log::debug!("📥 Backend Content-Type: {}", content_type);
    
    // Warn if unexpected content type (but don't fail - be permissive)
    if !content_type.is_empty() 
        && !content_type.contains("text/event-stream")
        && !content_type.contains("application/json") 
        && !content_type.contains("application/octet-stream") {
        warn!("⚠️  Unexpected Content-Type: {} (expected text/event-stream or application/json)", content_type);
    }

    if !status.is_success() {
        // Record circuit breaker failure
        tokio::spawn({
            let cb = app.circuit_breaker.clone();
            async move {
                cb.write().await.record_failure();
            }
        });
        
        // If 404, return synthetic Claude-like SSE with model list
        if status == StatusCode::NOT_FOUND {
            let models = get_available_models(&app).await;
            if !models.is_empty() {
                info!("💡 Model '{}' not found - sending model list to user", backend_model_for_error);

                let (tx, rx) = tokio::sync::mpsc::channel::<Event>(64);
                let requested_model = backend_model_for_error.clone();
                let model_name_for_response = backend_model_for_error.clone();
                let models_for_task = models.clone();

                tokio::spawn(async move {
                    log::debug!(
                        "🎬 Synthetic 404 response task started for model: {}",
                        requested_model
                    );
                    let now = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos();

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
                            "usage": { "input_tokens": 0, "output_tokens": 0 }
                        }
                    });
                    let _ = tx.send(Event::default().event("message_start").data(start.to_string())).await;

                    let block_start = json!({
                        "type": "content_block_start",
                        "index": 0,
                        "content_block": { "type": "text", "text": "" }
                    });
                    let _ = tx.send(Event::default().event("content_block_start").data(block_start.to_string())).await;

                    let content = build_model_list_content(&requested_model, &models_for_task);

                    let delta = json!({
                        "type": "content_block_delta",
                        "index": 0,
                        "delta": { "type": "text_delta", "text": content }
                    });
                    let _ = tx.send(Event::default().event("content_block_delta").data(delta.to_string())).await;

                    let block_stop = json!({ "type": "content_block_stop", "index": 0 });
                    let _ = tx.send(Event::default().event("content_block_stop").data(block_stop.to_string())).await;

                    let msg_delta = json!({
                        "type": "message_delta",
                        "delta": { "stop_reason": "end_turn", "stop_sequence": Value::Null },
                        "usage": { "output_tokens": 50 }
                    });
                    let _ = tx.send(Event::default().event("message_delta").data(msg_delta.to_string())).await;

                    let msg_stop = json!({ "type": "message_stop" });
                    let _ = tx.send(Event::default().event("message_stop").data(msg_stop.to_string())).await;
                    log::debug!("🏁 Synthetic 404 response completed");
                });

                let mut headers = HeaderMap::new();
                headers.insert("cache-control", "no-cache".parse().unwrap());
                headers.insert("connection", "keep-alive".parse().unwrap());
                headers.insert("x-accel-buffering", "no".parse().unwrap());
                let stream = ReceiverStream::new(rx).map(Ok::<Event, Infallible>);
                return Ok((headers, Sse::new(stream)));
            }
        }
        error!(
            "❌ Backend returned error: {} {}",
            status.as_u16(),
            status.canonical_reason().unwrap_or("")
        );
        return Err((StatusCode::BAD_GATEWAY, "backend_error"));
    }

    info!("✅ Backend responded successfully ({})", status);

    let (tx, rx) = tokio::sync::mpsc::channel::<Event>(64);

    // Per-request ephemeral state for re-chunking.
    let model_for_header = oai.model.clone();

    tokio::spawn(async move {
        log::debug!("🎬 Streaming task started");

        // Emit Claude "message_start"
        let now = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos();
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
        let _ = tx
            .send(Event::default().event("message_start").data(start.to_string()))
            .await;

        let mut bytes_stream = res.bytes_stream();

        // Block indexing
        let mut next_block_index: i32 = 0;
        let mut text_open = false;
        let mut text_index: i32 = -1;

        #[derive(Clone)]
        struct ToolBuf {
            block_index: i32,
            id: String,
            name: String,
        }
        let mut tools: HashMap<usize, ToolBuf> = HashMap::new();

        let mut sse_parser = SseEventParser::new();
        let mut done = false;
        let mut final_stop_reason = "end_turn"; // Default, will be updated if backend provides finish_reason
        let mut fatal_error = false;

        log::debug!("🌊 Begin processing SSE from backend");
        while let Some(item) = bytes_stream.next().await {
            let Ok(chunk) = item else {
                log::debug!("❌ Error reading chunk from stream");
                break;
            };

            for payload in sse_parser.push_and_drain_events(&chunk) {
                let data = payload.trim();
                if data == "[DONE]" {
                    log::debug!("🏁 Received [DONE] marker from backend");
                    done = true;
                    break;
                }
                if data.is_empty() {
                    continue;
                }

                // First, try to parse as generic JSON to understand the structure
                let json_value: serde_json::Result<Value> = serde_json::from_str(data);
                let parsed: serde_json::Result<OAIStreamChunk> = serde_json::from_str(data);
                
                let chunk = match parsed {
                    Ok(c) => c,
                    Err(e) => {
                        // Try to extract useful information from the raw JSON
                        if let Ok(val) = json_value {
                            // Check if it's an error response
                            if let Some(error_obj) = val.get("error") {
                                let error_msg = error_obj.get("message")
                                    .or_else(|| error_obj.get("type"))
                                    .and_then(|v| v.as_str())
                                    .unwrap_or("Unknown error");
                                let error_details = if error_msg.is_empty() {
                                    serde_json::to_string(error_obj).unwrap_or_else(|_| "Unknown backend error".into())
                                } else {
                                    error_msg.to_string()
                                };
                                
                                log::warn!("⚠️  Backend returned error in chunk: {}", error_details);

                                // Close any open text block before emitting the error
                                if text_open {
                                    let stop = json!({"type":"content_block_stop","index":text_index});
                                    let _ = tx
                                        .send(Event::default().event("content_block_stop").data(stop.to_string()))
                                        .await;
                                    text_open = false;
                                }

                                // Emit error message to the client as a text block
                                let error_index = next_block_index;
                                next_block_index += 1;

                                let start = json!({
                                    "type":"content_block_start",
                                    "index":error_index,
                                    "content_block":{"type":"text","text":""}
                                });
                                let _ = tx
                                    .send(Event::default().event("content_block_start").data(start.to_string()))
                                    .await;

                                // Format structured error message
                                let formatted_error = format_backend_error(&error_details, data);
                                
                                let delta = json!({
                                    "type":"content_block_delta",
                                    "index":error_index,
                                    "delta":{"type":"text_delta","text":formatted_error}
                                });
                                let _ = tx
                                    .send(Event::default().event("content_block_delta").data(delta.to_string()))
                                    .await;

                                let stop = json!({
                                    "type":"content_block_stop",
                                    "index":error_index
                                });
                                let _ = tx
                                    .send(Event::default().event("content_block_stop").data(stop.to_string()))
                                    .await;

                                final_stop_reason = "error";
                                done = true;
                                fatal_error = true;
                                break;
                            }
                            
                            // Check if it's a valid JSON object but missing required fields
                            if val.is_object() {
                                let preview = if data.len() > 500 {
                                    format!("{}...", &data[..500])
                                } else {
                                    data.to_string()
                                };
                                log::warn!("⚠️  Chunk missing 'choices' field ({} chars), structure: {}", data.len(), preview);
                                continue;
                            }
                        }
                        
                        // Malformed JSON or unexpected format
                        let preview = if data.len() > 500 {
                            format!("{}...", &data[..500])
                        } else {
                            data.to_string()
                        };
                        log::warn!("⚠️  JSON parse failed ({} chars): {}\nResponse preview: {}", data.len(), e, preview);
                        continue;
                    }
                };

                // Handle error responses in parsed chunk
                if let Some(error_val) = &chunk.error {
                    let error_msg = error_val
                        .get("message")
                        .or_else(|| error_val.get("type"))
                        .and_then(|v| v.as_str())
                        .unwrap_or("Unknown error");
                    let error_details = if error_msg.is_empty() {
                        serde_json::to_string(error_val).unwrap_or_else(|_| "Unknown backend error".into())
                    } else {
                        error_msg.to_string()
                    };

                    log::warn!("⚠️  Backend returned error: {}", error_details);

                    // Close any open text block before emitting the error
                    if text_open {
                        let stop = json!({"type":"content_block_stop","index":text_index});
                        let _ = tx
                            .send(Event::default().event("content_block_stop").data(stop.to_string()))
                            .await;
                        text_open = false;
                    }

                    // Emit error message to the client as a text block
                    let error_index = next_block_index;
                    next_block_index += 1;

                    let start = json!({
                        "type":"content_block_start",
                        "index":error_index,
                        "content_block":{"type":"text","text":""}
                    });
                    let _ = tx
                        .send(Event::default().event("content_block_start").data(start.to_string()))
                        .await;

                                // Format structured error message
                                let formatted_error = format_backend_error(&error_details, data);
                                
                                let delta = json!({
                                    "type":"content_block_delta",
                                    "index":error_index,
                                    "delta":{"type":"text_delta","text":formatted_error}
                                });
                    let _ = tx
                        .send(Event::default().event("content_block_delta").data(delta.to_string()))
                        .await;

                    let stop = json!({
                        "type":"content_block_stop",
                        "index":error_index
                    });
                    let _ = tx
                        .send(Event::default().event("content_block_stop").data(stop.to_string()))
                        .await;

                    final_stop_reason = "error";
                    done = true;
                    fatal_error = true;
                    break;
                }

                if chunk.choices.is_empty() {
                    log::debug!("⚠️  Chunk has no choices, skipping");
                    continue;
                }
                
                let choice = &chunk.choices[0];
                
                // Capture finish_reason if provided
                if let Some(reason) = &choice.finish_reason {
                    final_stop_reason = translate_finish_reason(Some(reason));
                    log::debug!("📍 Backend finish_reason: {} → Claude stop_reason: {}", reason, final_stop_reason);
                }
                
                // Handle non-streaming complete response (fallback)
                if let Some(message) = &choice.message {
                    log::debug!("📦 Received non-streaming complete response, converting to SSE");
                    if let Some(content_str) = message.get("content").and_then(|v| v.as_str()) {
                        if !text_open {
                            text_index = next_block_index;
                            let ev = json!({
                                "type":"content_block_start",
                                "index":text_index,
                                "content_block":{"type":"text","text":""}
                            });
                            let _ = tx
                                .send(Event::default().event("content_block_start").data(ev.to_string()))
                                .await;
                            text_open = true;
                        }
                        let ev = json!({
                            "type":"content_block_delta",
                            "index":text_index,
                            "delta":{"type":"text_delta","text":content_str}
                        });
                        let _ = tx
                            .send(Event::default().event("content_block_delta").data(ev.to_string()))
                            .await;
                    }
                    continue;
                }
                
                // Handle streaming delta response
                let Some(d) = &choice.delta else {
                    log::debug!("⚠️  Chunk has no delta or message, skipping");
                    continue;
                };

                // Reasoning content (not displayed; we just acknowledge)
                if let Some(r) = &d.reasoning_content {
                    if !r.is_empty() {
                        log::debug!("🧠 Reasoning content ({} chars)", r.len());
                    }
                }

                // Text deltas
                if let Some(c) = &d.content {
                    if !c.is_empty() {
                        if !text_open {
                            text_index = next_block_index;
                            next_block_index += 1;
                            let ev = json!({
                                "type":"content_block_start",
                                "index":text_index,
                                "content_block":{"type":"text","text":""}
                            });
                            let _ = tx
                                .send(Event::default().event("content_block_start").data(ev.to_string()))
                                .await;
                            text_open = true;
                        }
                        let ev = json!({
                            "type":"content_block_delta",
                            "index":text_index,
                            "delta":{"type":"text_delta","text":c}
                        });
                        let _ = tx
                            .send(Event::default().event("content_block_delta").data(ev.to_string()))
                            .await;
                    }
                }

                // Tool call deltas
                if let Some(tool_calls) = &d.tool_calls {
                    if !tool_calls.is_empty() {
                        // Close text block if open
                        if text_open {
                            let ev = json!({"type":"content_block_stop","index":text_index});
                            let _ = tx
                                .send(Event::default().event("content_block_stop").data(ev.to_string()))
                                .await;
                            text_open = false;
                        }

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

                                let start = json!({
                                    "type":"content_block_start",
                                    "index":tb.block_index,
                                    "content_block":{
                                        "type":"tool_use",
                                        "id":tb.id,
                                        "name":tb.name,
                                        "input":{}
                                    }
                                });
                                let _ = tx
                                    .send(Event::default().event("content_block_start").data(start.to_string()))
                                    .await;
                                tools.insert(idx, tb);
                            }
                            if let Some(f) = &tc.function {
                                if let Some(args) = &f.arguments {
                                    let ev = json!({
                                        "type":"content_block_delta",
                                        "index": tools.get(&idx).unwrap().block_index,
                                        "delta":{"type":"input_json_delta","partial_json": args}
                                    });
                                    let _ = tx
                                        .send(Event::default().event("content_block_delta").data(ev.to_string()))
                                        .await;
                                }
                            }
                        }
                    }
                }
            }

            if fatal_error {
                break;
            }

            if done {
                break;
            }
        }

        // Flush any trailing event if backend didn't send final blank line
        if !done {
            if let Some(payload) = sse_parser.flush() {
                let data = payload.trim();
                if data != "[DONE]" && !data.is_empty() {
                    if let Ok(chunk) = serde_json::from_str::<OAIStreamChunk>(data) {
                        if let Some(c) = chunk.choices.get(0).and_then(|ch| ch.delta.as_ref()).and_then(|d| d.content.as_ref()) {
                            if !c.is_empty() {
                                if !text_open {
                                    text_index = next_block_index;
                                    next_block_index += 1;
                                    let ev = json!({
                                        "type":"content_block_start",
                                        "index":text_index,
                                        "content_block":{"type":"text","text":""}
                                    });
                                    let _ = tx
                                        .send(Event::default().event("content_block_start").data(ev.to_string()))
                                        .await;
                                    text_open = true;
                                }
                                let ev = json!({
                                    "type":"content_block_delta",
                                    "index":text_index,
                                    "delta":{"type":"text_delta","text":c}
                                });
                                let _ = tx
                                    .send(Event::default().event("content_block_delta").data(ev.to_string()))
                                    .await;
                            }
                        }
                    }
                }
            }
        }

        // Close any open blocks and finish message
        if text_open {
            let ev = json!({ "type":"content_block_stop", "index":text_index });
            let _ = tx
                .send(Event::default().event("content_block_stop").data(ev.to_string()))
                .await;
        }
        for tb in tools.values() {
            let stop = json!({ "type":"content_block_stop", "index":tb.block_index });
            let _ = tx
                .send(Event::default().event("content_block_stop").data(stop.to_string()))
                .await;
        }

        let md = json!({
            "type":"message_delta",
            "delta":{"stop_reason":final_stop_reason,"stop_sequence":null},
            "usage":{"output_tokens":0}
        });
        let _ = tx
            .send(Event::default().event("message_delta").data(md.to_string()))
            .await;

        let _ = tx
            .send(Event::default().event("message_stop").data(json!({"type":"message_stop"}).to_string()))
            .await;

        log::debug!("🏁 Streaming task completed");
        
        // Record circuit breaker success if no fatal error
        if !fatal_error {
            let cb_clone = app.circuit_breaker.clone();
            tokio::spawn(async move {
                cb_clone.write().await.record_success();
            });
        }
    });

    let mut out_headers = HeaderMap::new();
    out_headers.insert("cache-control", "no-cache".parse().unwrap());
    out_headers.insert("connection", "keep-alive".parse().unwrap());
    out_headers.insert("x-accel-buffering", "no".parse().unwrap());

    let stream = ReceiverStream::new(rx).map(Ok::<Event, Infallible>);
    
    // Log structured metrics
    if let Ok(elapsed) = request_start.elapsed() {
        info!(target: "metrics",
            "request_completed: model={}, duration_ms={}, messages={}, status=success",
            backend_model_for_metrics, elapsed.as_millis(), original_message_count
        );
    }
    
    Ok((out_headers, Sse::new(stream)))
}
