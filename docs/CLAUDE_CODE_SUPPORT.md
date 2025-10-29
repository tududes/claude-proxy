# Claude Code Support Matrix

This proxy fully supports all known request patterns from Claude Code.

## Supported Request Formats

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

## Testing

Run validation:
```bash
./tests/test_claude_code_patterns.sh    # Tests all 9 patterns
```

All patterns supported. See `tests/README.md` for test documentation.

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

