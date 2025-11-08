use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Deserialize, Serialize, Debug, Clone)]
pub struct ThinkingConfig {
    #[serde(rename = "type")]
    pub type_: String, // "enabled"
    pub budget_tokens: u32,
}

#[derive(Deserialize, Debug)]
pub struct ClaudeImageSource {
    #[serde(rename = "type")]
    #[allow(dead_code)]
    pub source_type: String,
    pub media_type: String,
    pub data: String,
}

#[derive(Deserialize, Debug)]
#[serde(tag = "type")]
pub enum ClaudeContentBlock {
    #[serde(rename = "text")]
    Text { text: String },
    #[serde(rename = "image")]
    Image { source: ClaudeImageSource },
    #[serde(rename = "thinking")]
    Thinking { thinking: String },
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
pub struct ClaudeMessage {
    pub role: String,
    pub content: Value, // String or Vec<ClaudeContentBlock>
}

#[derive(Deserialize)]
pub struct ClaudeTool {
    pub name: String,
    #[serde(default)]
    pub description: Option<String>,
    pub input_schema: Value,
}

#[derive(Deserialize)]
pub struct ClaudeRequest {
    pub model: String,
    pub messages: Vec<ClaudeMessage>,
    #[serde(default)]
    pub system: Option<Value>,
    #[serde(default)]
    pub max_tokens: Option<u32>,
    #[serde(default)]
    pub temperature: Option<f32>,
    #[serde(default)]
    pub top_p: Option<f32>,
    #[serde(default)]
    pub top_k: Option<u32>,
    #[serde(default)]
    pub stop_sequences: Option<Vec<String>>,
    #[serde(default)]
    pub tools: Option<Vec<ClaudeTool>>,
    #[serde(default)]
    pub tool_choice: Option<Value>,
    #[serde(default)]
    pub thinking: Option<ThinkingConfig>,
    #[serde(default)]
    pub _stream: Option<bool>,
    // Fields for validation warnings (accepted but not used)
    #[serde(default)]
    pub metadata: Option<Value>,
    #[serde(default)]
    pub service_tier: Option<String>,
}

#[derive(Deserialize)]
pub struct ClaudeTokenCountRequest {
    #[allow(dead_code)]
    pub model: String,
    pub messages: Vec<ClaudeMessage>,
    #[serde(default)]
    pub system: Option<Value>,
    #[serde(default)]
    pub tools: Option<Vec<ClaudeTool>>,
}