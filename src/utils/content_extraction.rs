use serde_json::{json, Value};

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

/// Convert Claude `tool_choice` schema to OpenAI-compatible values.
pub fn convert_tool_choice(tool_choice: Option<Value>) -> Option<Value> {
    let Some(choice) = tool_choice else {
        return None;
    };

    match choice {
        Value::String(s) => match s.to_ascii_lowercase().as_str() {
            "auto" => Some(Value::String("auto".into())),
            "none" => Some(Value::String("none".into())),
            "any" => {
                log::info!("🔧 tool_choice: 'any' → 'required' for OpenAI compatibility");
                Some(Value::String("required".into()))
            }
            "required" => Some(Value::String("required".into())),
            other => {
                log::warn!("⚠️ Unknown string tool_choice '{}'; passing through", other);
                Some(Value::String(s))
            }
        },
        Value::Object(obj) => {
            let Some(kind) = obj.get("type").and_then(|v| v.as_str()) else {
                log::warn!("⚠️ tool_choice object missing 'type'; passing through");
                return Some(Value::Object(obj));
            };
            match kind.to_ascii_lowercase().as_str() {
                "tool" => {
                    let name = obj
                        .get("name")
                        .and_then(|v| v.as_str())
                        .or_else(|| obj.get("tool_name").and_then(|v| v.as_str()));
                    if let Some(name) = name {
                        if obj.get("disable_parallel_tool_use").is_some() {
                            log::info!("ℹ️ disable_parallel_tool_use not supported; ignoring");
                        }
                        log::info!("🔧 tool_choice: forcing tool '{}' via function format", name);
                        Some(json!({
                            "type": "function",
                            "function": { "name": name }
                        }))
                    } else {
                        log::warn!("⚠️ tool_choice 'tool' missing 'name'; dropping constraint");
                        None
                    }
                }
                "function" => Some(Value::Object(obj)),
                "auto" => Some(Value::String("auto".into())),
                "none" => Some(Value::String("none".into())),
                "any" => {
                    if obj.get("disable_parallel_tool_use").is_some() {
                        log::info!("ℹ️ disable_parallel_tool_use not supported for 'any'; ignoring");
                    }
                    log::info!("🔧 tool_choice: type 'any' → 'required'");
                    Some(Value::String("required".into()))
                }
                "required" => Some(Value::String("required".into())),
                other => {
                    log::warn!("⚠️ Unknown tool_choice type '{}'; passing through", other);
                    Some(Value::Object(obj))
                }
            }
        }
        other => {
            log::warn!(
                "⚠️ tool_choice should be string or object; received {:?}, passing through",
                other
            );
            Some(other)
        }
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

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    // ============================================================================
    // extract_text_from_content tests
    // ============================================================================

    #[test]
    fn test_extract_text_simple_string() {
        let content = json!("Hello, world!");
        let (text, images) = extract_text_from_content(&content);
        assert_eq!(text, "Hello, world!");
        assert_eq!(images, 0);
    }

    #[test]
    fn test_extract_text_empty_string() {
        let content = json!("");
        let (text, images) = extract_text_from_content(&content);
        assert_eq!(text, "");
        assert_eq!(images, 0);
    }

    #[test]
    fn test_extract_text_single_text_block() {
        let content = json!([
            {"type": "text", "text": "This is a test"}
        ]);
        let (text, images) = extract_text_from_content(&content);
        assert_eq!(text, "This is a test");
        assert_eq!(images, 0);
    }

    #[test]
    fn test_extract_text_multiple_text_blocks() {
        let content = json!([
            {"type": "text", "text": "First block"},
            {"type": "text", "text": "Second block"}
        ]);
        let (text, images) = extract_text_from_content(&content);
        assert_eq!(text, "First block\nSecond block");
        assert_eq!(images, 0);
    }

    #[test]
    fn test_extract_text_with_image() {
        let content = json!([
            {"type": "text", "text": "Description"},
            {"type": "image", "source": {"type": "base64", "data": "..."}}
        ]);
        let (text, images) = extract_text_from_content(&content);
        assert_eq!(text, "Description");
        assert_eq!(images, 1);
    }

    #[test]
    fn test_extract_text_multiple_images() {
        let content = json!([
            {"type": "image", "source": {}},
            {"type": "text", "text": "Between images"},
            {"type": "image", "source": {}}
        ]);
        let (text, images) = extract_text_from_content(&content);
        assert_eq!(text, "Between images");
        assert_eq!(images, 2);
    }

    #[test]
    fn test_extract_text_tool_use() {
        let content = json!([
            {"type": "tool_use", "name": "calculator", "input": {"a": 1, "b": 2}}
        ]);
        let (text, images) = extract_text_from_content(&content);
        assert!(text.contains("calculator"));
        assert!(text.contains(r#""a":1"#) || text.contains(r#""a": 1"#));
        assert_eq!(images, 0);
    }

    #[test]
    fn test_extract_text_tool_result_string() {
        let content = json!([
            {"type": "tool_result", "content": "Result text"}
        ]);
        let (text, images) = extract_text_from_content(&content);
        assert_eq!(text, "Result text");
        assert_eq!(images, 0);
    }

    #[test]
    fn test_extract_text_tool_result_array() {
        let content = json!([
            {
                "type": "tool_result",
                "content": [
                    {"type": "text", "text": "First"},
                    {"type": "text", "text": "Second"}
                ]
            }
        ]);
        let (text, images) = extract_text_from_content(&content);
        assert_eq!(text, "First\nSecond");
        assert_eq!(images, 0);
    }

    #[test]
    fn test_extract_text_unknown_block_type() {
        let content = json!([
            {"type": "unknown", "data": "ignored"},
            {"type": "text", "text": "Visible"}
        ]);
        let (text, images) = extract_text_from_content(&content);
        assert_eq!(text, "Visible");
        assert_eq!(images, 0);
    }

    #[test]
    fn test_extract_text_null() {
        let content = json!(null);
        let (text, images) = extract_text_from_content(&content);
        assert_eq!(text, "");
        assert_eq!(images, 0);
    }

    #[test]
    fn test_extract_text_empty_array() {
        let content = json!([]);
        let (text, images) = extract_text_from_content(&content);
        assert_eq!(text, "");
        assert_eq!(images, 0);
    }

    // ============================================================================
    // convert_system_content tests
    // ============================================================================

    #[test]
    fn test_convert_system_simple_string() {
        let system = json!("You are a helpful assistant");
        let result = convert_system_content(&system);
        assert_eq!(result, json!("You are a helpful assistant"));
    }

    #[test]
    fn test_convert_system_empty_string() {
        let system = json!("");
        let result = convert_system_content(&system);
        assert_eq!(result, json!(""));
    }

    #[test]
    fn test_convert_system_single_block() {
        let system = json!([
            {"type": "text", "text": "System instruction"}
        ]);
        let result = convert_system_content(&system);
        assert_eq!(result, json!("System instruction"));
    }

    #[test]
    fn test_convert_system_multiple_blocks() {
        let system = json!([
            {"type": "text", "text": "First instruction"},
            {"type": "text", "text": "Second instruction"}
        ]);
        let result = convert_system_content(&system);
        assert_eq!(result, json!("First instruction\nSecond instruction"));
    }

    #[test]
    fn test_convert_system_mixed_blocks() {
        let system = json!([
            {"type": "text", "text": "Visible"},
            {"type": "image", "data": "ignored"},
            {"type": "text", "text": "Also visible"}
        ]);
        let result = convert_system_content(&system);
        assert_eq!(result, json!("Visible\nAlso visible"));
    }

    #[test]
    fn test_convert_system_empty_array() {
        let system = json!([]);
        let result = convert_system_content(&system);
        assert_eq!(result, json!(""));
    }

    #[test]
    fn test_convert_system_null() {
        let system = json!(null);
        let result = convert_system_content(&system);
        assert_eq!(result, json!(null));
    }

    // ============================================================================
    // serialize_tool_result_content tests
    // ============================================================================

    #[test]
    fn test_serialize_tool_result_simple_string() {
        let content = json!("Success");
        let result = serialize_tool_result_content(&content);
        assert_eq!(result, "Success");
    }

    #[test]
    fn test_serialize_tool_result_empty_string() {
        let content = json!("");
        let result = serialize_tool_result_content(&content);
        assert_eq!(result, "");
    }

    #[test]
    fn test_serialize_tool_result_array_of_text() {
        let content = json!([
            {"type": "text", "text": "Line 1"},
            {"type": "text", "text": "Line 2"}
        ]);
        let result = serialize_tool_result_content(&content);
        assert_eq!(result, "Line 1\nLine 2");
    }

    #[test]
    fn test_serialize_tool_result_array_of_strings() {
        let content = json!(["First", "Second", "Third"]);
        let result = serialize_tool_result_content(&content);
        assert_eq!(result, "First\nSecond\nThird");
    }

    #[test]
    fn test_serialize_tool_result_mixed_array() {
        let content = json!([
            "Plain string",
            {"type": "text", "text": "Text block"}
        ]);
        let result = serialize_tool_result_content(&content);
        assert!(result.contains("Plain string"));
        assert!(result.contains("Text block"));
    }

    #[test]
    fn test_serialize_tool_result_complex_object() {
        let content = json!({"result": "success", "data": [1, 2, 3]});
        let result = serialize_tool_result_content(&content);
        assert!(result.contains("result"));
        assert!(result.contains("success"));
    }

    #[test]
    fn test_serialize_tool_result_empty_array() {
        let content = json!([]);
        let result = serialize_tool_result_content(&content);
        assert_eq!(result, "");
    }

    // ============================================================================
    // convert_tool_choice tests
    // ============================================================================

    #[test]
    fn test_convert_tool_choice_string_auto() {
        let result = convert_tool_choice(Some(json!("auto")));
        assert_eq!(result, Some(json!("auto")));
    }

    #[test]
    fn test_convert_tool_choice_string_any() {
        let result = convert_tool_choice(Some(json!("any")));
        assert_eq!(result, Some(json!("required")));
    }

    #[test]
    fn test_convert_tool_choice_tool_object() {
        let result = convert_tool_choice(Some(json!({
            "type": "tool",
            "name": "calculator"
        })));
        assert_eq!(
            result,
            Some(json!({
                "type": "function",
                "function": { "name": "calculator" }
            }))
        );
    }

    #[test]
    fn test_convert_tool_choice_auto_object() {
        let result = convert_tool_choice(Some(json!({ "type": "auto" })));
        assert_eq!(result, Some(json!("auto")));
    }

    #[test]
    fn test_convert_tool_choice_invalid_tool() {
        let result = convert_tool_choice(Some(json!({ "type": "tool" })));
        assert_eq!(result, None);
    }

    // ============================================================================
    // translate_finish_reason tests
    // ============================================================================

    #[test]
    fn test_translate_finish_reason_stop() {
        assert_eq!(translate_finish_reason(Some("stop")), "end_turn");
    }

    #[test]
    fn test_translate_finish_reason_length() {
        assert_eq!(translate_finish_reason(Some("length")), "max_tokens");
    }

    #[test]
    fn test_translate_finish_reason_tool_calls() {
        assert_eq!(translate_finish_reason(Some("tool_calls")), "tool_use");
    }

    #[test]
    fn test_translate_finish_reason_function_call() {
        assert_eq!(translate_finish_reason(Some("function_call")), "tool_use");
    }

    #[test]
    fn test_translate_finish_reason_content_filter() {
        assert_eq!(translate_finish_reason(Some("content_filter")), "end_turn");
    }

    #[test]
    fn test_translate_finish_reason_error() {
        assert_eq!(translate_finish_reason(Some("error")), "error");
    }

    #[test]
    fn test_translate_finish_reason_unknown() {
        assert_eq!(translate_finish_reason(Some("unknown_reason")), "end_turn");
    }

    #[test]
    fn test_translate_finish_reason_none() {
        assert_eq!(translate_finish_reason(None), "end_turn");
    }

    #[test]
    fn test_translate_finish_reason_empty_string() {
        assert_eq!(translate_finish_reason(Some("")), "end_turn");
    }
}