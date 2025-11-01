use serde_json::Value;

/// Extract text content from Claude content value (string or array of blocks)
/// Returns tuple: (text_content, image_count)
pub fn extract_text_from_content(content: &Value) -> (String, usize) {
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

/// Convert Claude system prompt value (string or array of blocks) into OpenAI system content
pub fn convert_system_content(sys: &Value) -> Value {
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
        serde_json::json!(combined_text)
    } else {
        sys.clone()
    }
}

/// Serialize tool_result content to a string for OpenAI
pub fn serialize_tool_result_content(content: &Value) -> String {
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

/// Build OpenAI tools array from Claude tools
pub fn build_oai_tools(tools: Option<Vec<crate::models::ClaudeTool>>) -> Option<Vec<crate::models::OAITool>> {
    match tools {
        Some(ts) if !ts.is_empty() => Some(
            ts.into_iter()
                .map(|t| crate::models::OAITool {
                    type_: "function".into(),
                    function: crate::models::OAIFunction {
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
pub fn translate_finish_reason(oai_reason: Option<&str>) -> &'static str {
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