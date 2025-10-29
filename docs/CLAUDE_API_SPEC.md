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

## Test Script Compliance Checklist

All `test_*.sh` scripts must:

- Use `/v1/messages` endpoint
- Include `Content-Type: application/json` header
- Send `Authorization: Bearer $API_KEY` if API_KEY is set
- Use `model` field (loaded from .env or defaulted)
- Include `messages` array with proper role/content structure
- Set `stream: true` for SSE responses
- Include `max_tokens` parameter
- Check for Claude SSE event types:
  - `message_start`, `content_block_start`, `content_block_delta`
  - `text_delta`, `tool_use` (for tool tests), `message_stop`

## Current Test Scripts

All core tests comply with the specification. Run `./validate_tests.sh` to verify.

## References

- [Claude Messages API Documentation](https://docs.anthropic.com/claude/reference/messages_post)
- [Claude Tool Use Guide](https://docs.anthropic.com/claude/docs/tool-use)
- [SSE Event Stream Format](https://developer.mozilla.org/en-US/docs/Web/API/Server-sent_events/Using_server-sent_events)

## Common Mistakes to Avoid

1. Using `/v1/chat/completions` (OpenAI format) - should be `/v1/messages`
2. Checking for `choices` array - Claude uses `content` array
3. Looking for `delta.content` - Claude uses `delta.text`
4. Missing `stream: true` - required for SSE responses
5. Wrong tool format - must use `input_schema` not `parameters`
6. Forgetting `max_tokens` - required in Claude API

