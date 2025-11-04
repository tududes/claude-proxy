# Thinking Content Logging Reference

This document describes the logging output for thinking/reasoning content processing.

## Log Levels

- **INFO**: Key thinking operations (opening, closing, conversion)
- **DEBUG**: Streaming deltas (can be verbose during streaming)

## Input Side Logs (Client → Backend)

When processing client messages with thinking content:

```
🧠 INPUT: Extracted thinking block (1234 chars) from assistant message
🧠 INPUT: Converted 1 thinking block(s) (1234 chars) to interleaved <think> format
```

These logs appear when:
- Client sends assistant messages with `type: "thinking"` content blocks
- Proxy extracts them and converts to `<think>...</think>` format for backend

## Output Side Logs (Backend → Client)

When receiving and streaming thinking content from backend:

```
🧠 OUTPUT: Opened thinking block (index=0)
🧠 OUTPUT: Streamed thinking delta (50 chars)
🧠 OUTPUT: Streamed thinking delta (48 chars)
...
🧠 OUTPUT: Closed thinking block before text (index=0)
```

Or if no text follows:

```
🧠 OUTPUT: Closed thinking block at end (index=0)
```

These logs appear when:
- Backend sends `reasoning_content` in response
- Proxy converts it to proper Claude thinking blocks with proper event sequence

## Auto-Enable Log

When thinking is automatically enabled for reasoning models:

```
🧠 Auto-enabling thinking for reasoning model: deepseek-r1
```

This appears when:
- Model name contains "reasoning", "r1", or "deep"
- Client didn't explicitly provide `thinking` parameter
- Default budget: 10,000 tokens

## Viewing Logs

**Default (INFO level):**
```bash
cargo run --release
# or
RUST_LOG=info cargo run --release
```

**Verbose (DEBUG level, includes delta streaming):**
```bash
RUST_LOG=debug cargo run --release
```

## Example Full Flow

```
📨 Request: model=deepseek-r1, client_auth=true, backend=http://...
🧠 Auto-enabling thinking for reasoning model: deepseek-r1
🧠 INPUT: Extracted thinking block (234 chars) from assistant message
🧠 INPUT: Converted 1 thinking block(s) (234 chars) to interleaved <think> format
✅ Backend responded successfully (200)
🧠 OUTPUT: Opened thinking block (index=0)
🧠 OUTPUT: Streamed thinking delta (52 chars)
🧠 OUTPUT: Streamed thinking delta (48 chars)
🧠 OUTPUT: Closed thinking block before text (index=0)
🏁 Streaming task completed
```

