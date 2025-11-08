use axum::{
    extract::State,
    http::{HeaderMap, StatusCode},
    response::sse::{Event, Sse},
};
use futures::{Stream, StreamExt};
use serde_json::{json, Value};
use std::{
    collections::HashMap,
    convert::Infallible,
    time::{SystemTime, UNIX_EPOCH},
};
use tokio_stream::wrappers::ReceiverStream;
use crate::models::{App, ClaudeRequest, ClaudeContentBlock, OAIMessage, OAIChatReq, OAIStreamChunk};
use crate::services::{SseEventParser, ToolBuf, ToolsMap, extract_client_key, mask_token,
                     get_available_models, format_backend_error, build_model_list_content};
use crate::utils::normalize_model_name;
use crate::utils::content_extraction::{translate_finish_reason, build_oai_tools, convert_system_content, serialize_tool_result_content};

pub async fn messages(
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
            log::error!("🔴 Circuit breaker is open - rejecting request");
            return Err((StatusCode::SERVICE_UNAVAILABLE, "backend_unavailable_circuit_open"));
        }
    }

    // Request validation
    if cr.messages.is_empty() {
        log::warn!("❌ Validation failed: empty messages");
        return Err((StatusCode::BAD_REQUEST, "empty_messages"));
    }

    if cr.messages.len() > 10_000 {
        log::warn!("❌ Validation failed: too many messages ({})", cr.messages.len());
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
        log::warn!("❌ Validation failed: content too large ({} bytes)", total_content_size);
        return Err((StatusCode::PAYLOAD_TOO_LARGE, "content_too_large"));
    }

    // Validate max_tokens if provided
    if let Some(max_tokens) = cr.max_tokens {
        if max_tokens < 1 || max_tokens > 100_000 {
            log::warn!("❌ Validation failed: max_tokens out of range ({})", max_tokens);
            return Err((StatusCode::BAD_REQUEST, "invalid_max_tokens"));
        }
    }

    // Validate system prompt length if provided
    if let Some(ref system) = cr.system {
        let system_size = match system {
            serde_json::Value::String(s) => s.len(),
            other => serde_json::to_string(other).unwrap_or_default().len(),
        };
        if system_size > 100 * 1024 {  // 100KB limit
            log::warn!("❌ Validation failed: system prompt too large ({} bytes)", system_size);
            return Err((StatusCode::BAD_REQUEST, "system_prompt_too_large"));
        }
    }

    // Log warnings for unsupported parameters (accepted but ignored)
    if cr.metadata.is_some() {
        log::warn!("⚠️  'metadata' parameter not supported by backend (accepted but ignored)");
    }
    if cr.service_tier.is_some() {
        log::warn!("⚠️  'service_tier' parameter not supported by backend (accepted but ignored)");
    }

    // Debug: Log incoming headers (names only)
    log::debug!("📥 Incoming headers:");
    for (name, _) in headers.iter() {
        log::debug!("   {}", name);
    }

    // Auth extraction: Authorization or x-api-key
    let client_key = extract_client_key(&headers);

    if let Some(key) = &client_key {
        log::info!("🔑 Client API Key: Bearer {}", mask_token(key));
    } else {
        log::info!("🔑 No client API key (no 'authorization' or 'x-api-key' header)");
    }

    let has_client_auth = client_key.is_some();
    log::info!(
        "📨 Request: model={}, client_auth={}, backend={}",
        cr.model, has_client_auth, app.backend_url
    );

    // Normalize model name (case-correction only)
    let backend_model = normalize_model_name(&cr.model, &app.models_cache).await;
    let backend_model_for_metrics = backend_model.clone();
    
    // Auto-enable thinking for reasoning models if not explicitly provided
    let thinking_config = if cr.thinking.is_some() {
        cr.thinking
    } else {
        // Check if this is a reasoning model by querying model cache
        let is_reasoning_model = {
            let cache = app.models_cache.read().await;
            cache.as_ref()
                .and_then(|models| {
                    // Look for model in cache
                    models.iter()
                        .find(|m| m.id.eq_ignore_ascii_case(&backend_model))
                        .map(|model_info| {
                            // Check if model supports thinking features
                            model_info.supported_features.iter().any(|f| {
                                f.eq_ignore_ascii_case("thinking") || 
                                f.eq_ignore_ascii_case("extended_thinking")
                            })
                        })
                })
                .unwrap_or(false)  // Default to false if model not found
        };
        
        if is_reasoning_model {
            log::info!("🧠 Auto-enabling thinking for reasoning model: {}", backend_model);
            Some(crate::models::ThinkingConfig {
                type_: "enabled".to_string(),
                budget_tokens: 10000,
            })
        } else {
            None
        }
    };
    
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
            let mut thinking_parts = Vec::new();
            let mut text_parts = Vec::new();
            let mut tool_calls = Vec::new();

            for block in &blocks {
                match block {
                    ClaudeContentBlock::Thinking { thinking } => {
                        thinking_parts.push(thinking.as_str());
                        log::info!("🧠 INPUT: Extracted thinking block ({} chars) from assistant message", thinking.len());
                    }
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

            // Interleave thinking: prepend thinking blocks as <think> tags
            let content = if thinking_parts.is_empty() && text_parts.is_empty() {
                Value::Null
            } else {
                let mut combined = String::new();
                
                // Add thinking content first, wrapped in <think> tags
                if !thinking_parts.is_empty() {
                    let thinking_text = thinking_parts.join("\n");
                    let thinking_len = thinking_text.len();
                    combined.push_str(&format!("<think>{}</think>\n", thinking_text));
                    log::info!("🧠 INPUT: Converted {} thinking block(s) ({} chars) to interleaved <think> format", thinking_parts.len(), thinking_len);
                }
                
                // Add regular text content
                if !text_parts.is_empty() {
                    combined.push_str(&text_parts.join("\n"));
                }
                
                json!(combined)
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
                        log::info!(
                            "🖼️ Processing image: media_type={}, size={} bytes",
                            source.media_type,
                            source.data.len()
                        );
                        if source.data.starts_with("data:") {
                            log::warn!("⚠️ Image data already appears to be a data URI (double-encoding?)");
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
            log::info!("🚮 Removing empty assistant placeholder message from client history.");
            let _ = msgs.pop();
            log::debug!("📊 After filtering: {} messages remaining", msgs.len());
        }
    }

    if msgs.is_empty() {
        log::error!("❌ No messages remaining after conversion!");
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
        top_k: cr.top_k,
        stop: cr.stop_sequences,
        tools,
        tool_choice: cr.tool_choice,
        thinking: thinking_config.map(|tc| serde_json::to_value(tc).unwrap_or(Value::Null)),
        stream: true,
    };

    let mut req = app
        .client
        .post(&app.backend_url)
        .header("content-type", "application/json");

    // Auth: Forward client key to backend, or reject if invalid/missing
    if let Some(key) = &client_key {
        if key.contains("sk-ant-") {
            log::warn!("❌ Anthropic OAuth tokens (sk-ant-*) are not supported - use backend-compatible key (cpk_*)");
            return Err((StatusCode::UNAUTHORIZED, "invalid_auth_token"));
        }
        req = req.bearer_auth(key);
        log::info!("🔄 Auth: Forwarding client key to backend");
    } else {
        log::warn!("❌ No client API key provided");
        return Err((StatusCode::UNAUTHORIZED, "missing_api_key"));
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
            let auth_header_str = client_key
                .as_ref()
                .map(|k| format!("Bearer {}", mask_token(k)))
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
        log::error!("❌ Backend connection failed: {}", e);
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
        log::warn!("⚠️  Unexpected Content-Type: {} (expected text/event-stream or application/json)", content_type);
    }

    if !status.is_success() {
        // Record circuit breaker failure
        tokio::spawn({
            let cb = app.circuit_breaker.clone();
            async move {
                cb.write().await.record_failure();
            }
        });

        // Read error response body
        let error_body = res.text().await.unwrap_or_else(|_| "Unknown error".to_string());
        
        log::error!(
            "❌ Backend returned error: {} {} - {}",
            status.as_u16(),
            status.canonical_reason().unwrap_or(""),
            error_body
        );

        // If 404, return synthetic Claude-like SSE with model list
        if status == StatusCode::NOT_FOUND {
            let models = get_available_models(&app).await;
            if !models.is_empty() {
                log::info!("💡 Model '{}' not found - sending model list to user", backend_model_for_error);

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

        // For retryable errors (rate limits, server errors), pass through HTTP status
        // so Claude Code can retry automatically
        if matches!(status, 
            StatusCode::TOO_MANY_REQUESTS |  // 429
            StatusCode::INTERNAL_SERVER_ERROR |  // 500
            StatusCode::BAD_GATEWAY |  // 502
            StatusCode::SERVICE_UNAVAILABLE |  // 503
            StatusCode::GATEWAY_TIMEOUT  // 504
        ) {
            log::info!("⚠️  Returning retryable error status {} for automatic retry", status);
            return Err((status, "backend_error_retryable"));
        }

        // For non-retryable errors (auth, bad request), return formatted SSE message
        let (tx, rx) = tokio::sync::mpsc::channel::<Event>(64);
        let error_msg = format_backend_error(&error_body, &error_body);
        let model_name = backend_model_for_error.clone();

        tokio::spawn(async move {
            log::debug!("🎬 Synthetic error response task started");
            let now = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos();

            let start = json!({
                "type": "message_start",
                "message": {
                    "id": format!("msg_{}", now),
                    "type": "message",
                    "role": "assistant",
                    "content": [],
                    "model": model_name,
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

            let delta = json!({
                "type": "content_block_delta",
                "index": 0,
                "delta": { "type": "text_delta", "text": error_msg }
            });
            let _ = tx.send(Event::default().event("content_block_delta").data(delta.to_string())).await;

            let block_stop = json!({ "type": "content_block_stop", "index": 0 });
            let _ = tx.send(Event::default().event("content_block_stop").data(block_stop.to_string())).await;

            let msg_delta = json!({
                "type": "message_delta",
                "delta": { "stop_reason": "error", "stop_sequence": Value::Null },
                "usage": { "output_tokens": 0 }
            });
            let _ = tx.send(Event::default().event("message_delta").data(msg_delta.to_string())).await;

            let msg_stop = json!({ "type": "message_stop" });
            let _ = tx.send(Event::default().event("message_stop").data(msg_stop.to_string())).await;
            log::debug!("🏁 Synthetic error response completed");
        });

        let mut headers = HeaderMap::new();
        headers.insert("cache-control", "no-cache".parse().unwrap());
        headers.insert("connection", "keep-alive".parse().unwrap());
        headers.insert("x-accel-buffering", "no".parse().unwrap());
        let stream = ReceiverStream::new(rx).map(Ok::<Event, Infallible>);
        return Ok((headers, Sse::new(stream)));
    }

    log::info!("✅ Backend responded successfully ({})", status);

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
        let mut thinking_open = false;
        let mut thinking_index: i32 = -1;
        let mut text_open = false;
        let mut text_index: i32 = -1;

        let mut tools: ToolsMap = HashMap::new();

        let mut sse_parser = SseEventParser::new();
        let mut done = false;
        let mut final_stop_reason = "end_turn"; // Default, will be updated if backend provides finish_reason
        let mut fatal_error = false;

        log::debug!("🌊 Begin processing SSE from backend");
        while let Some(item) = bytes_stream.next().await {
            let chunk = match item {
                Ok(chunk) => chunk,
                Err(_) => {
                    log::debug!("❌ Error reading chunk from stream");
                    break;
                }
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

                // Reasoning/thinking content - stream as proper thinking blocks
                if let Some(r) = &d.reasoning_content {
                    if !r.is_empty() {
                        if !thinking_open {
                            thinking_index = next_block_index;
                            next_block_index += 1;
                            let ev = json!({
                                "type":"content_block_start",
                                "index":thinking_index,
                                "content_block":{"type":"thinking","thinking":""}
                            });
                            let _ = tx
                                .send(Event::default().event("content_block_start").data(ev.to_string()))
                                .await;
                            thinking_open = true;
                            log::info!("🧠 OUTPUT: Opened thinking block (index={})", thinking_index);
                        }
                        let ev = json!({
                            "type":"content_block_delta",
                            "index":thinking_index,
                            "delta":{"type":"thinking_delta","thinking":r}
                        });
                        let _ = tx
                            .send(Event::default().event("content_block_delta").data(ev.to_string()))
                            .await;
                        log::debug!("🧠 OUTPUT: Streamed thinking delta ({} chars)", r.len());
                    }
                }

                // Text deltas
                if let Some(c) = &d.content {
                    if !c.is_empty() {
                        // Close thinking block if still open (thinking comes before text)
                        if thinking_open {
                            let ev = json!({ "type":"content_block_stop", "index":thinking_index });
                            let _ = tx
                                .send(Event::default().event("content_block_stop").data(ev.to_string()))
                                .await;
                            thinking_open = false;
                            log::info!("🧠 OUTPUT: Closed thinking block before text (index={})", thinking_index);
                        }
                        
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
        if thinking_open {
            let ev = json!({ "type":"content_block_stop", "index":thinking_index });
            let _ = tx
                .send(Event::default().event("content_block_stop").data(ev.to_string()))
                .await;
            log::info!("🧠 OUTPUT: Closed thinking block at end (index={})", thinking_index);
        }
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

        // Drain any remaining bytes from backend stream to avoid cancelling the request
        // This ensures the backend doesn't see a connection reset/cancellation
        log::debug!("🔄 Draining remaining backend stream...");
        let mut drained_bytes = 0;
        while let Some(item) = bytes_stream.next().await {
            if let Ok(chunk) = item {
                drained_bytes += chunk.len();
            }
        }
        if drained_bytes > 0 {
            log::debug!("🔄 Drained {} additional bytes from backend stream", drained_bytes);
        } else {
            log::debug!("✅ Backend stream was already fully consumed");
        }

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
        log::info!(target: "metrics",
            "request_completed: model={}, duration_ms={}, messages={}, status=success",
            backend_model_for_metrics, elapsed.as_millis(), original_message_count
        );
    }

    Ok((out_headers, Sse::new(stream)))
}