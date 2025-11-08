# API Specification Comparison

## Overview

This document compares the official Anthropic Messages API and OpenAI Chat Completions API specifications against the `claude-proxy` implementation.

**Specification Sources:**
- Anthropic: `anthropic-spec/` (anthropic-sdk-typescript repository)
- OpenAI: `openai-spec-manual/openapi.yaml` (OpenAI OpenAPI specification)

**Last Updated:** 2025-11-08

---

## Anthropic Messages API

### Request Structure

**Official Spec (MessageCreateParams):**
```typescript
interface MessageCreateParamsBase {
  max_tokens: number;                           // Required
  messages: Array<MessageParam>;                 // Required, up to 100,000 messages
  model: Model;                                  // Required
  metadata?: Metadata;
  service_tier?: 'auto' | 'standard_only';
  stop_sequences?: string[];
  stream?: boolean;
  system?: string | Array<TextBlockParam>;       // Optional system prompt
  temperature?: number;                          // 0.0 to 1.0
  thinking?: ThinkingConfigParam;                // Optional thinking/reasoning config
  tool_choice?: ToolChoice;
  tools?: Array<ToolUnion>;
  top_k?: number;
  top_p?: number;                                // 0.0 to 1.0
}
```

**Our Implementation (ClaudeRequest):**
```rust
pub struct ClaudeRequest {
    pub model: String,                             // ✅ Supported
    pub messages: Vec<ClaudeMessage>,              // ✅ Supported (validation: max 10,000)
    pub system: Option<Value>,                     // ✅ Supported (string or array)
    pub max_tokens: Option<u32>,                   // ✅ Supported (validation: 1-100,000)
    pub temperature: Option<f32>,                  // ✅ Supported
    pub top_p: Option<f32>,                        // ✅ Supported
    pub top_k: Option<u32>,                        // ✅ Supported
    pub stop_sequences: Option<Vec<String>>,       // ✅ Supported
    pub tools: Option<Vec<ClaudeTool>>,            // ✅ Supported
    pub tool_choice: Option<Value>,                // ✅ Supported
    pub thinking: Option<ThinkingConfig>,          // ✅ Supported
    pub _stream: Option<bool>,                     // ⚠️  Accepted but ignored (always streams to backend)
    pub metadata: Option<Value>,                   // ⚠️  Accepted with warning (not forwarded)
    pub service_tier: Option<String>,              // ⚠️  Accepted with warning (not forwarded)
}
```

### Message Structure

**Official Spec:**
```typescript
interface MessageParam {
  content: string | Array<ContentBlockParam>;
  role: 'user' | 'assistant';
}

type ContentBlockParam = 
  | TextBlockParam 
  | ImageBlockParam 
  | DocumentBlockParam
  | ToolUseBlockParam 
  | ToolResultBlockParam
  | ThinkingBlockParam
  | RedactedThinkingBlockParam
  | ServerToolUseBlockParam
  | SearchResultBlockParam
  | WebSearchToolResultBlockParam;
```

**Our Implementation:**
```rust
pub enum ClaudeContentBlock {
    Text { text: String },                         // ✅ Supported
    Image { source: ClaudeImageSource },           // ✅ Supported (base64 images)
    Thinking { thinking: String },                 // ✅ Supported
    ToolUse { id: String, name: String, input: Value }, // ✅ Supported
    ToolResult { tool_use_id: String, content: Value, is_error: Option<bool> }, // ✅ Supported
    // ❌ Missing: DocumentBlockParam, RedactedThinkingBlockParam, ServerToolUseBlockParam, 
    //            SearchResultBlockParam, WebSearchToolResultBlockParam
}
```

### Response Structure

**Official Spec (Message):**
```typescript
interface Message {
  id: string;
  content: Array<ContentBlock>;
  model: Model;
  role: 'assistant';
  stop_reason: StopReason | null;
  stop_sequence: string | null;
  type: 'message';
  usage: Usage;
}

type StopReason = 'end_turn' | 'max_tokens' | 'stop_sequence' | 'tool_use' | 'pause_turn' | 'refusal';
```

**Our Implementation:**
- We translate the Anthropic Messages API to OpenAI format
- We DON'T preserve the Anthropic response format
- Instead, we convert OpenAI SSE chunks back to Claude SSE format

**Finish Reason Mapping:**
```rust
// src/utils/content_extraction.rs
pub fn translate_finish_reason(oai_reason: &str) -> &str {
    match oai_reason {
        "stop" => "end_turn",
        "length" => "max_tokens",
        "tool_calls" => "tool_use",
        _ => oai_reason,
    }
}
```

### Content Block Types

| Anthropic Spec | Our Support | Notes |
|----------------|-------------|-------|
| `text` | ✅ Full | Text content blocks |
| `image` | ✅ Full | Base64 image sources (jpeg, png, gif, webp) |
| `thinking` | ✅ Full | Reasoning/thinking blocks (auto-enabled for reasoning models) |
| `tool_use` | ✅ Full | Tool invocation blocks |
| `tool_result` | ✅ Full | Tool response blocks (translated to OpenAI "tool" role) |
| `document` (PDF) | ❌ Missing | PDF document blocks |
| `redacted_thinking` | ❌ Missing | Redacted thinking blocks |
| `server_tool_use` | ❌ Missing | Server-side tool usage (bash, text_editor) |
| `search_result` | ❌ Missing | Search result blocks |
| `web_search_tool_result` | ❌ Missing | Web search tool results |

### Tool Support

**Official Spec:**
```typescript
interface Tool {
  name: string;
  description?: string;
  input_schema: JSONSchema;
  cache_control?: CacheControlEphemeral;
}

type ToolUnion = 
  | Tool 
  | ToolBash20250124 
  | ToolTextEditor20250124 
  | WebSearchTool20250305;
```

**Our Implementation:**
```rust
pub struct ClaudeTool {
    pub name: String,                              // ✅ Supported
    pub description: Option<String>,               // ✅ Supported
    pub input_schema: Value,                       // ✅ Supported
    // ❌ Missing: cache_control
    // ❌ Missing: Server tools (bash, text_editor, web_search)
}
```

**Tool Translation:**
- Claude tools → OpenAI `tools` with `type: "function"`
- `input_schema` → `parameters` (renamed field)

### Streaming

**Official Spec:**
- SSE events: `message_start`, `content_block_start`, `content_block_delta`, `content_block_stop`, `message_delta`, `message_stop`
- Supports streaming thinking blocks

**Our Implementation:**
- ✅ Full SSE streaming support
- ✅ Thinking block streaming (auto-enabled for reasoning models)
- ✅ Tool call streaming
- ✅ Text delta streaming
- Implementation: Translates OpenAI SSE → Claude SSE format

---

## OpenAI Chat Completions API

### Request Structure

**Official Spec (CreateChatCompletionRequest):**
```yaml
CreateChatCompletionRequest:
  properties:
    model: string (required)
    messages: array (required, minItems: 1)
    max_completion_tokens: integer (nullable)
    temperature: number (0-2, default: 1)
    top_p: number (0-1, default: 1)
    stop: string | array
    stream: boolean (default: false)
    tools: array (function definitions)
    tool_choice: string | object
    frequency_penalty: number (-2 to 2)
    presence_penalty: number (-2 to 2)
    logit_bias: object
    logprobs: boolean
    top_logprobs: integer (0-20)
    n: integer (choices count)
    response_format: object (json_schema, json_object, text)
    seed: integer
    user: string
    reasoning_effort: string (o-series models)
    web_search_options: object
    audio: object (audio output params)
    store: boolean
    metadata: object
    modalities: array
```

**What We Send to Backend (OAIChatReq):**
```rust
pub struct OAIChatReq {
    pub model: String,                             // ✅ From Claude request
    pub messages: Vec<OAIMessage>,                 // ✅ Translated from Claude
    pub max_tokens: Option<u32>,                   // ✅ From Claude (Note: OpenAI uses max_completion_tokens)
    pub temperature: Option<f32>,                  // ✅ From Claude
    pub top_p: Option<f32>,                        // ✅ From Claude
    pub top_k: Option<u32>,                        // ✅ From Claude
    pub stop: Option<Vec<String>>,                 // ✅ From Claude (stop_sequences → stop)
    pub tools: Option<Vec<OAITool>>,               // ✅ Translated from Claude
    pub tool_choice: Option<Value>,                // ✅ From Claude
    pub thinking: Option<Value>,                   // ✅ From Claude (non-standard)
    pub stream: bool,                              // ✅ Always true (we always stream to backend)
    // ❌ Not forwarded: frequency_penalty, presence_penalty, logit_bias, logprobs, 
    //                  top_logprobs, n, response_format, seed, user, reasoning_effort,
    //                  web_search_options, audio, store, metadata, modalities
}
```

### Message Structure

**Official Spec:**
```typescript
type ChatCompletionRequestMessage = 
  | SystemMessage 
  | UserMessage 
  | AssistantMessage 
  | ToolMessage 
  | FunctionMessage;

interface SystemMessage {
  role: 'system';
  content: string | Array<TextContent>;
  name?: string;
}

interface UserMessage {
  role: 'user';
  content: string | Array<TextContent | ImageContent>;
  name?: string;
}

interface AssistantMessage {
  role: 'assistant';
  content?: string | null;
  tool_calls?: Array<ToolCall>;
  name?: string;
}

interface ToolMessage {
  role: 'tool';
  content: string;
  tool_call_id: string;
}
```

**Our Implementation:**
```rust
pub struct OAIMessage {
    pub role: String,                              // ✅ 'system', 'user', 'assistant', 'tool'
    pub content: Value,                            // ✅ String or Array
    pub tool_call_id: Option<String>,              // ✅ For 'tool' role messages
    pub tool_calls: Option<Vec<Value>>,            // ✅ For assistant tool calls
    // ❌ Missing: name field
}
```

**Message Translation Logic:**
1. Claude `system` param → OpenAI `system` message (first message)
2. Claude `user`/`assistant` text → OpenAI `user`/`assistant` text
3. Claude `tool_use` block → OpenAI assistant `tool_calls`
4. Claude `tool_result` block → OpenAI `tool` message (separate message)
5. Claude multi-modal content → OpenAI multi-modal array format
6. Empty assistant placeholder (from Claude Code) → removed

### Response Structure

**Official Spec (CreateChatCompletionResponse):**
```yaml
CreateChatCompletionResponse:
  properties:
    id: string
    object: string ('chat.completion' | 'chat.completion.chunk')
    created: integer
    model: string
    choices: array
      - index: integer
        message: ChatCompletionResponseMessage
        finish_reason: 'stop' | 'length' | 'tool_calls' | 'content_filter' | 'function_call'
        logprobs: object | null
    usage: object
      - prompt_tokens: integer
      - completion_tokens: integer
      - total_tokens: integer
    system_fingerprint: string
```

**What We Receive and Translate:**
```rust
pub struct OAIStreamChunk {
    pub _id: Option<String>,                       // ✅ Parsed (e.g., "chatcmpl-xxx")
    pub _object: Option<String>,                   // ✅ Parsed
    pub _created: Option<i64>,                     // ✅ Parsed
    pub _model: Option<String>,                    // ✅ Parsed
    pub choices: Vec<OAIChoice>,                   // ✅ Parsed
    pub error: Option<Value>,                      // ✅ Handled
}

pub struct OAIChoice {
    pub delta: Option<OAIChoiceDelta>,             // ✅ Streaming delta
    pub message: Option<Value>,                    // ✅ Non-streaming complete message
    pub finish_reason: Option<String>,             // ✅ Translated to Claude format
}

pub struct OAIChoiceDelta {
    pub content: Option<String>,                   // ✅ Text delta
    pub tool_calls: Option<Vec<OAIToolCallDelta>>, // ✅ Tool call deltas
    pub reasoning_content: Option<String>,         // ⚠️  Non-standard (thinking/reasoning)
}
```

**Translation Flow:**
1. OpenAI SSE chunk → Parse JSON
2. Extract `choices[0].delta.content` → Claude `content_block_delta` (text)
3. Extract `choices[0].delta.tool_calls` → Claude `content_block_delta` (tool_use)
4. Extract `choices[0].delta.reasoning_content` → Claude `content_block_delta` (thinking)
5. Extract `finish_reason` → Translate to Claude format

### Tool Support

**Official Spec:**
```typescript
interface Tool {
  type: 'function';
  function: {
    name: string;
    description?: string;
    parameters: JSONSchema;
    strict?: boolean;
  };
}

interface ToolCall {
  id: string;
  type: 'function';
  function: {
    name: string;
    arguments: string; // JSON string
  };
}
```

**Our Implementation:**
```rust
pub struct OAITool {
    pub type_: String,                             // ✅ Always "function"
    pub function: OAIFunction,                     // ✅ Function definition
}

pub struct OAIFunction {
    pub name: String,                              // ✅ Tool name
    pub description: Option<String>,               // ✅ Tool description
    pub parameters: Value,                         // ✅ JSON schema
    // ❌ Missing: strict (structured output enforcement)
}
```

**Tool Call Handling:**
- ✅ Streaming tool calls (incremental JSON)
- ✅ Tool call buffering and reconstruction
- ✅ Multiple tool calls in a single response
- ✅ Tool result translation (Claude tool_result → OpenAI tool message)

### Streaming

**Official Spec:**
- SSE events with `data:` prefix
- Final event: `data: [DONE]`
- Each chunk is a JSON object with `choices[].delta`

**Our Implementation:**
- ✅ Full SSE parsing
- ✅ Handles `data: [DONE]`
- ✅ Handles empty SSE events
- ✅ Error recovery (malformed JSON, connection errors)
- ✅ Graceful fallback for unexpected content types

---

## Key Differences & Limitations

### Claude → OpenAI Translation

| Feature | Support | Notes |
|---------|---------|-------|
| Basic text messages | ✅ Full | Direct translation |
| System prompts | ✅ Full | Converted to first message |
| Multi-modal (images) | ✅ Full | Base64 images preserved |
| Tool use | ✅ Full | Claude tool_use → OpenAI tool_calls |
| Tool results | ✅ Full | Claude tool_result → OpenAI tool message (separate) |
| Thinking/reasoning | ✅ Full | Auto-enabled for reasoning models |
| Streaming | ✅ Full | OpenAI SSE → Claude SSE |
| Model name | ✅ Full | Case-insensitive with cache |
| Stop sequences | ✅ Full | `stop_sequences` → `stop` |
| Temperature | ✅ Full | Direct passthrough |
| Top-p | ✅ Full | Direct passthrough |
| Top-k | ✅ Full | Direct passthrough |
| Max tokens | ✅ Full | `max_tokens` (Claude) → `max_tokens` (OpenAI) |
| Tool choice | ✅ Full | Direct passthrough |

### Unsupported Anthropic Features

| Feature | Status | Impact |
|---------|--------|--------|
| `metadata` field | ⚠️  Accepted with warning | Low - Anthropic-specific tracking (not forwarded) |
| `service_tier` | ⚠️  Accepted with warning | Low - Priority capacity selection (not forwarded) |
| `tool_choice` | ✅ **Supported** | Medium - Can force specific tool |
| `top_k` sampling | ✅ **Supported** | Low - Advanced sampling parameter |
| PDF documents | ❌ Not supported | Medium - Document understanding |
| Web search tool | ❌ Not supported | Medium - Built-in web search |
| Server tools (bash, editor) | ❌ Not supported | Low - Server-side execution |
| Citations | ❌ Not supported | Low - Source attribution |
| Cache control | ❌ Not supported | Medium - Prompt caching optimization |

### Unsupported OpenAI Features

| Feature | Status | Impact |
|---------|--------|--------|
| `frequency_penalty` | ❌ Not forwarded | Low - Advanced generation control |
| `presence_penalty` | ❌ Not forwarded | Low - Advanced generation control |
| `logit_bias` | ❌ Not forwarded | Low - Token probability manipulation |
| `logprobs` | ❌ Not forwarded | Low - Probability information |
| `n` (multiple choices) | ❌ Not supported | Low - Always returns single response |
| `response_format` (JSON mode) | ❌ Not forwarded | Medium - Structured output |
| `seed` (reproducibility) | ❌ Not forwarded | Low - Deterministic generation |
| `user` (abuse tracking) | ❌ Not forwarded | Low - User identification |
| Audio output | ❌ Not supported | Low - TTS integration |
| Vision (URLs) | ⚠️  Partial | Only base64 images supported |
| Function calling (deprecated) | ✅ N/A | Use tools instead |

### Validation Differences

| Validation | Anthropic Spec | Our Implementation | Difference |
|------------|----------------|-------------------|------------|
| Max messages | 100,000 | 10,000 | More permissive than before (was 1,000) |
| Max content size | Not specified | 5 MB | Additional safety limit |
| Max system size | Not specified | 100 KB | Additional safety limit |
| Max tokens range | Model-dependent | 1 - 100,000 | Hard-coded range |

---

## Compatibility Assessment

### High Compatibility ✅

The proxy provides excellent compatibility for:
- Standard text conversations
- Multi-modal (base64 images)
- Tool calling (function calling)
- Streaming responses
- Temperature/top-p/stop sequences
- Thinking/reasoning models (auto-detection)

### Medium Compatibility ⚠️ 

Partial support or workarounds needed for:
- Advanced tool control (`tool_choice`)
- Structured outputs (`response_format`)
- PDF documents
- Prompt caching (`cache_control`)
- Large message histories (1K limit vs 100K spec)

### Low Compatibility ❌

Not supported (would require significant changes):
- Server-side tools (bash, text_editor, web_search)
- Citations and source attribution
- Service tier selection
- Audio output/input
- Multiple response choices (`n > 1`)
- Advanced sampling (`top_k`, `logit_bias`)

---

## Recommendations

### For Maximum Compatibility

1. **Stick to core features:**
   - Text conversations
   - Images (base64 only)
   - Standard tool calling
   - Basic sampling parameters

2. **Avoid advanced features:**
   - Server tools
   - PDF documents
   - Web search
   - Structured output enforcement
   - Prompt caching

3. **Handle edge cases:**
   - Empty assistant messages (automatically removed)
   - Tool result errors (translated to content)
   - Missing finish reasons (default to "end_turn")

### Potential Improvements

1. **Add support for:**
   - ~~`tool_choice` parameter~~ ✅ **DONE** (v0.1.5)
   - ~~`top_k` sampling~~ ✅ **DONE** (v0.1.5)
   - ~~Increase message limit (1K → 10K+)~~ ✅ **DONE** (v0.1.5)
   - `response_format` (JSON mode)

2. **Error handling:**
   - Better validation error messages
   - Graceful degradation for unsupported features

3. **Performance:**
   - Connection pooling improvements
   - Better SSE parsing efficiency

---

## Testing Recommendations

### Critical Test Cases

1. **Basic conversation:**
   - Single user message
   - Multi-turn conversation
   - System prompt handling

2. **Tool calling:**
   - Single tool call
   - Multiple tool calls
   - Tool call errors
   - Tool result with complex content

3. **Streaming:**
   - Text streaming
   - Tool call streaming
   - Thinking block streaming
   - Error mid-stream

4. **Multi-modal:**
   - Single image
   - Multiple images
   - Mixed text + images

5. **Edge cases:**
   - Empty messages
   - Very long messages
   - Malformed tool calls
   - Connection errors
   - Backend errors

### Test Coverage Gaps

- ❌ No tests for PDF documents (unsupported)
- ❌ No tests for server tools (unsupported)
- ❌ No tests for `tool_choice` enforcement
- ❌ No tests for structured output (`response_format`)
- ❌ Limited tests for error scenarios

---

## Conclusion

The `claude-proxy` implementation provides **strong compatibility** for the most common use cases:
- ✅ Standard text conversations
- ✅ Tool calling / function calling
- ✅ Streaming responses
- ✅ Multi-modal (images)
- ✅ Thinking/reasoning models

However, it intentionally **omits several advanced features** from both APIs:
- Server-side tools (web search, bash, text editor)
- Advanced generation controls (logit_bias, penalties)
- Structured outputs (response_format)
- Prompt caching
- PDF document understanding

This design makes the proxy **simple and focused** on the core translation task, suitable for most conversational and tool-calling workloads.

For production use with advanced features, consider:
1. ~~Adding validation warnings for unsupported parameters~~ ✅ **DONE** (v0.1.5)
2. ~~Implementing `tool_choice`~~ ✅ **DONE** (v0.1.5) | `response_format` if needed
3. ~~Increasing message limits for long conversations~~ ✅ **DONE** (v0.1.5 - now 10K)
4. Adding comprehensive error handling tests

