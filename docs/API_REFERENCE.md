# Claude Messages API Specification Reference

This document ensures all test scripts conform to the official Claude Messages API specification.

## Required Format

### Endpoint
```
POST /v1/messages
```

### Required Headers
```
Content-Type: application/json
Authorization: Bearer {API_KEY}  # Optional, can be configured in .env
```

### Request Body (JSON)

**Required fields:**
```json
{
  "model": "string",           // Model identifier (e.g., "claude-3-5-sonnet-20241022")
  "messages": [...],           // Array of message objects
  "max_tokens": integer,       // Maximum tokens to generate
  "stream": boolean            // true for SSE streaming
}
```

**Optional fields:**
```json
{
  "system": "string",          // System prompt
  "temperature": float,        // 0.0 to 1.0
  "top_p": float,             // Nucleus sampling
  "top_k": integer,           // Top-k sampling
  "stop_sequences": [...],    // Array of stop strings
  "tools": [...]              // Tool/function definitions
}
```

### Message Format
```json
{
  "role": "user" | "assistant",
  "content": "string" | [...]   // String or array of content blocks
}
```

### Tool Definition Format
```json
{
  "name": "string",
  "description": "string",
  "input_schema": {
    "type": "object",
    "properties": {...},
    "required": [...]
  }
}
```

## Expected Response (SSE Streaming)

### Event Types (in order):

1. **message_start**
```json
{
  "type": "message_start",
  "message": {
    "id": "msg_...",
    "type": "message",
    "role": "assistant",
    "content": [],
    "model": "...",
    "stop_reason": null,
    "stop_sequence": null,
    "usage": {"input_tokens": 0, "output_tokens": 0}
  }
}
```

2. **content_block_start**
```json
{
  "type": "content_block_start",
  "index": 0,
  "content_block": {"type": "text", "text": ""}
}
```

3. **content_block_delta** (multiple)
```json
{
  "type": "content_block_delta",
  "index": 0,
  "delta": {"type": "text_delta", "text": "chunk"}
}
```

4. **content_block_stop**
```json
{
  "type": "content_block_stop",
  "index": 0
}
```

5. **message_delta**
```json
{
  "type": "message_delta",
  "delta": {"stop_reason": "end_turn", "stop_sequence": null},
  "usage": {"output_tokens": N}
}
```

6. **message_stop**
```json
{
  "type": "message_stop"
}
```

### Tool Use Events

For tool calls:
```json
{
  "type": "content_block_start",
  "index": N,
  "content_block": {
    "type": "tool_use",
    "id": "toolu_...",
    "name": "function_name",
    "input": {}
  }
}
```

```json
{
  "type": "content_block_delta",
  "index": N,
  "delta": {
    "type": "input_json_delta",
    "partial_json": "{\"arg\":"
  }
}
```

## Supported Features

The proxy fully supports all known request patterns from Claude Code:

### 1. String Content
```json
{
  "messages": [
    {"role": "user", "content": "simple text"}
  ]
}
```
Claude Code uses for: Simple questions, basic interactions

### 2. Content Blocks Array (Text)
```json
{
  "messages": [
    {
      "role": "user",
      "content": [
        {"type": "text", "text": "Explain this code"}
      ]
    }
  ]
}
```
Claude Code uses for: Structured text messages

### 3. Mixed Content Blocks (Text + Image)
```json
{
  "messages": [
    {
      "role": "user",
      "content": [
        {"type": "text", "text": "What's in this?"},
        {
          "type": "image",
          "source": {
            "type": "base64",
            "media_type": "image/png",
            "data": "..."
          }
        }
      ]
    }
  ]
}
```
Claude Code uses for: Analyzing images, screenshots
Conversion: Claude base64 → OpenAI data URI format `data:image/png;base64,...`
Backend Requirements: Vision-capable model (GPT-4V, GPT-4o, Claude 3.5 Sonnet)

### 4. System Prompts
```json
{
  "system": "You are an expert developer",
  "messages": [...]
}
```

**Claude Code uses for:** Setting behavior, context, role

### 5. Multi-Turn Conversations
```json
{
  "messages": [
    {"role": "user", "content": "Write code"},
    {"role": "assistant", "content": "def foo()..."},
    {"role": "user", "content": "Now optimize it"}
  ]
}
```

**Claude Code uses for:** Maintaining conversation context, follow-ups

### 6. Tool Definitions
```json
{
  "tools": [
    {
      "name": "read_file",
      "description": "Read file contents",
      "input_schema": {
        "type": "object",
        "properties": {"path": {"type": "string"}},
        "required": ["path"]
      }
    }
  ]
}
```

**Claude Code uses for:** File operations, terminal commands, code editing
**Format:** Uses `input_schema` (not OpenAI's `parameters`)

### 7. Tool Results
```json
{
  "messages": [
    {
      "role": "user",
      "content": [
        {
          "type": "tool_result",
          "tool_use_id": "toolu_123",
          "content": "file contents..."
        }
      ]
    }
  ]
}
```

**Claude Code uses for:** Returning tool execution results
**Conversion:** Splits `tool_result` blocks into OpenAI `{"role": "tool", "tool_call_id": "..."}` format
**Note:** Backend receives properly formatted tool messages for function calling flow

### 8. Temperature & Top-P
```json
{
  "temperature": 0.7,
  "top_p": 0.9
}
```

**Claude Code uses for:** Controlling randomness/creativity

### 9. Stop Sequences
```json
{
  "stop_sequences": ["\n\n", "---"]
}
```

**Claude Code uses for:** Controlling output boundaries

## Response Format

All responses use proper Claude Messages API SSE format:

```
event: message_start
data: {"type":"message_start","message":{...}}

event: content_block_start
data: {"type":"content_block_start","index":0,...}

event: content_block_delta
data: {"type":"content_block_delta","delta":{"type":"text_delta","text":"..."}}

event: content_block_stop
data: {"type":"content_block_stop","index":0}

event: message_delta
data: {"type":"message_delta","delta":{"stop_reason":"end_turn"}}

event: message_stop
data: {"type":"message_stop"}
```

### Tool Use Responses
```
event: content_block_start
data: {"type":"content_block_start","content_block":{"type":"tool_use","id":"toolu_...","name":"read_file","input":{}}}

event: content_block_delta
data: {"type":"content_block_delta","delta":{"type":"input_json_delta","partial_json":"{\"path\""}}
```

## Translation Flow

```
Claude Code Request               Proxy Processing                Backend (OpenAI Format)
══════════════════════════════════════════════════════════════════════════════

content: "text"          →        content: "text"         →      content: "text"
content: [{type:text}]   →        content: [{...}]        →      content: [{...}]
content: [text, image]   →        content: [...]          →      content: [...]
system: "prompt"         →        messages[0]: {          →      messages[0]: {
                                    role: "system",               role: "system",
                                    content: "prompt"             content: "prompt"
                                  }                               }
tools: [{                →        tools: [{                →      tools: [{
  input_schema: {...}               function: {                    function: {
}]                                    parameters: {...}              parameters: {...}
                                    }                               }
                                  }]                              }]

←─────────────────────────────────────────────────────────────────────────────

Backend Response                 Proxy Translation               Claude Code Receives
══════════════════════════════════════════════════════════════════════════════

data: {"choices":[...]}  →       event: message_start     →      event: message_start
delta: {content:"hi"}    →       event: content_block_    →      event: content_block_
                                 delta                            delta
                                 delta: {type:"text_      →      delta: {type:"text_
                                 delta", text:"hi"}               delta", text:"hi"}
```

## Backend Compatibility

The proxy **fully converts** all Claude content types to OpenAI format:

| Feature | Proxy Conversion | Backend Requirements |
|---------|------------------|---------------------|
| **Text** |  Native support | Any OpenAI-compatible backend |
| **Images** |  Base64 → Data URI | Vision-capable model (GPT-4V, GPT-4o, Claude 3.5 Sonnet) |
| **Tool Use** |  Full conversion | Function calling support |
| **Tool Results** |  Message splitting | Function calling support |

**Note:** If backend lacks vision/tool support, it will return 400. This is expected behavior - the proxy does its job correctly.

## Content Block Types

| Type | Proxy Conversion | OpenAI Output |
|------|------------------|---------------|
| `text` | Passthrough or concatenation | `{"content": "text"}` |
| `image` | Base64 → Data URI | `{"type": "image_url", "image_url": {...}}` |
| `tool_use` | Schema conversion | `{"tool_calls": [{...}]}` |
| `tool_result` | Message splitting | `{"role": "tool", "tool_call_id": "..."}` |

## Production Status

All 9 Claude Code request patterns fully supported:
- All message formats (text, images, tool use, tool results)
- Multimodal support (images converted to data URI format)
- Complete tool calling (proper tool_use and tool_result conversion)
- Token counting endpoint
- SSE event streaming with proper block indexing
- Multi-turn conversation context
- Parameter pass-through (temperature, top_p, stop)
- Smart authorization routing

Tested against 4 reference implementations (1rgs, fuergaosi233, istarwyh, ujisati).

Backend requirements:
- Vision: GPT-4V, GPT-4o, or Claude 3.5 Sonnet
- Tools: Any backend with function calling support

## Current Test Scripts

All core tests comply with the specification. Run `./validate_tests.sh` to verify.

## References

- [Claude Messages API Documentation](https://docs.anthropic.com/claude/reference/messages_post)
- [Claude Tool Use Guide](https://docs.anthropic.com/claude/docs/tool-use)
- [SSE Event Stream Format](https://developer.mozilla.org/en-US/docs/Web/API/Server-sent_events/Using_server-sent_events)
- [CLAUDE_CODE_SUPPORT.md (Legacy)](CLAUDE_CODE_SUPPORT.md) - This file is being consolidated into this document and will be deleted after consolidation is complete.

## Common Mistakes to Avoid

1. Using `/v1/chat/completions` (OpenAI format) - should be `/v1/messages`
2. Checking for `choices` array - Claude uses `content` array
3. Looking for `delta.content` - Claude uses `delta.text`
4. Missing `stream: true` - required for SSE responses
5. Wrong tool format - must use `input_schema` not `parameters`
6. Forgetting `max_tokens` - required in Claude API
7. Not supporting image content - must convert Claude base64 to OpenAI data URI format
8. Not handling tool results properly - must split `tool_result` blocks into separate `role: "tool"` messages
9. Not translating `finish_reason` properly - must map OpenAI reasons to Claude reasons (stop→end_turn, length→max_tokens, etc.)

