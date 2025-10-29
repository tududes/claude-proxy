# Claude Code Setup Guide

Complete guide to using this proxy with Claude Code CLI.

## Quick Setup (3 Steps)

### 1. Configure Your Chutes API Key

```bash
cd /path/to/claude-proxy
cp .env.example .env
```

Edit `.env` and add your Chutes API key:
```bash
BACKEND_URL=https://llm.chutes.ai/v1/chat/completions
BACKEND_KEY=cpk_YOUR_CHUTES_API_KEY_HERE
MODEL=zai-org/GLM-4.5-Air
RUST_LOG=info
```

### 2. Start the Proxy

```bash
cargo run --release

# Expected output:
# [INFO] 🚀 Claude-to-OpenAI Proxy starting...
# [INFO]    Backend URL: https://llm.chutes.ai/v1/chat/completions
# [INFO]    Backend Key: Set (fallback)
# [INFO]    Listening on: 0.0.0.0:8080
```

### 3. Run Claude Code

```bash
# Point Claude Code to the proxy
export ANTHROPIC_BASE_URL=http://localhost:8080

# Run Claude Code with your model
claude --model zai-org/GLM-4.5-Air
```

**Done!** Claude Code will now use your Chutes backend through the proxy.

## How It Works

```
Claude Code                Proxy                    Chutes Backend
─────────────────────────────────────────────────────────────────

Sends request with     →  Accepts Claude         →  Receives OpenAI
Anthropic OAuth token     Messages API format        Chat Completions
(sk-ant-*)                                          with YOUR Chutes
                          Detects sk-ant-*           API key (cpk_*)
                          token
                                                     Processes request
                          Replaces with           ←
                          BACKEND_KEY from .env      Returns OpenAI
                                                     streaming response
                          Translates to OpenAI   →
                          format
                                                  ←  Converts back to
                          Converts response to      Claude SSE events
Receives Claude SSE    ←  Claude SSE format
events
```

## Auth Flow Diagram

```
┌─────────────────────────────────────────────────────────────┐
│ Client sends key?                                           │
└────────────┬────────────────────────────────────────────────┘
             │
    ┌────────▼─────────┐
    │ Has Authorization │
    │ header?           │
    └────────┬─────────┘
             │
      ┌──────▼──────┐
      │ Key type?    │
      └──────┬───────┘
             │
   ┌─────────┼──────────┐
   │         │          │
   ▼         ▼          ▼
cpk_*    sk-ant-*    No auth
   │         │          │
   │         │          │
Forward  Replace    Use
client   with       BACKEND_KEY
key      BACKEND    from .env
         _KEY
   │         │          │
   └─────────┴──────────┘
             │
        ┌────▼────┐
        │ Backend │
        │ Request │
        └─────────┘
```

## What You See in Proxy Logs

### Scenario 1: Claude Code (Default Behavior)

Claude Code sends Anthropic OAuth token → Proxy replaces it

```
[INFO] 🔑 Client Authorization: Bearer sk-a...TwAA
[INFO] 🔄 Auth: Replacing Anthropic OAuth token with BACKEND_KEY
[DEBUG] 🔑 Using configured BACKEND_KEY
[INFO] ✅ Backend responded successfully
```

### Scenario 2: User Passes Chutes Key Directly

User sets `ANTHROPIC_API_KEY=cpk_...` → Proxy forwards it

```
[INFO] 🔑 Client Authorization: Bearer cpk_...xyz9
[INFO] 🔄 Auth: Forwarding client key to backend
[INFO] ✅ Backend responded successfully
```

### Scenario 3: No Client Auth

Claude Code sends no auth → Proxy uses BACKEND_KEY

```
[INFO] 🔑 No client Authorization header
[DEBUG] 🔑 Using configured BACKEND_KEY
[INFO] ✅ Backend responded successfully
```

## User Instructions (For Your Documentation)

**To use Claude Code with your Chutes account:**

1. Download and configure the proxy:
   ```bash
   git clone https://github.com/YOUR_USERNAME/claude-proxy
   cd claude-proxy
   cargo build --release
   ```

2. Add your Chutes API key to `.env`:
   ```bash
   cp .env.example .env
   # Edit .env and set BACKEND_KEY=your_chutes_api_key
   ```

3. Start the proxy:
   ```bash
   cargo run --release
   ```

4. In a new terminal, run Claude Code:
   ```bash
   export ANTHROPIC_BASE_URL=http://localhost:8080
   claude --model zai-org/GLM-4.5-Air
   ```

5. Use Claude Code normally! All requests go through the proxy to Chutes.

## Testing the Connection

```bash
# Terminal 1: Start proxy with DEBUG logging
RUST_LOG=debug cargo run --release

# Terminal 2: Test request
curl -N http://localhost:8080/v1/messages \
  -H "Content-Type: application/json" \
  -d '{
    "model": "zai-org/GLM-4.5-Air",
    "messages": [{"role": "user", "content": "Say hi"}],
    "max_tokens": 50,
    "stream": true
  }'

# Watch Terminal 1 for:
# [INFO] 🔑 No client Authorization header
# [DEBUG] 🔑 Using configured BACKEND_KEY
# [INFO] ✅ Backend responded successfully
```

## FAQ

**Q: Why does proxy replace Claude Code's auth token?**  
A: Claude Code sends Anthropic OAuth tokens (`sk-ant-*`) which don't work with other backends. The proxy detects these and replaces them with your configured `BACKEND_KEY`.

**Q: Can users pass their own backend keys?**  
A: Yes! If they set `ANTHROPIC_API_KEY=cpk_their_key`, the proxy will forward it directly.

**Q: What if I want all users to use one backend key?**  
A: Perfect! Just set `BACKEND_KEY` in `.env`. All requests use that key regardless of client auth.

**Q: How do I see what key is being used?**  
A: Run with `RUST_LOG=debug` to see full auth headers and backend responses.

**Q: Does this work with Cursor IDE?**  
A: Yes! Set the same `ANTHROPIC_BASE_URL` in Cursor's settings or environment.

## Security Notes

- `RUST_LOG=info` (default): Masks API keys (shows first/last 4 chars only)
- `RUST_LOG=debug`: Shows full keys (use only for debugging)
- `RUST_LOG=error`: No auth logging
- `.env` file is gitignored (keep your keys safe)

## Advanced: Multiple Backends

Run multiple proxies on different ports for different backends:

```bash
# Terminal 1: Chutes backend
BACKEND_URL=https://llm.chutes.ai/v1/chat/completions \
BACKEND_KEY=your_chutes_key \
./target/release/claude_openai_proxy
# Listens on :8080

# Terminal 2: Another backend
BACKEND_URL=http://localhost:8000/v1/chat/completions \
BACKEND_KEY=your_other_key \
cargo run --release
# Would need to change port in code, or use PORT env var
```

Then point Claude Code to whichever backend:
```bash
export ANTHROPIC_BASE_URL=http://localhost:8080  # Chutes
# or
export ANTHROPIC_BASE_URL=http://localhost:8081  # Other backend
```

