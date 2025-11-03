# Claude Code Setup Guide

This guide explains how to use the proxy with the Claude Code CLI.

## Quick Setup

### 1. Start the Proxy

You can run the proxy using Docker (recommended) or from source.

**Docker:**
```bash
# This starts the proxy on http://localhost:8180
docker-compose up -d
```

**From Source:**
```bash
# This starts the proxy on http://localhost:8080
cargo run --release
```

### 2. Configure Claude Code

In your terminal, set two environment variables:

1.  `ANTHROPIC_BASE_URL`: Point Claude Code to your running proxy.
2.  `ANTHROPIC_API_KEY`: Provide an API key that is valid for your backend. The proxy will forward this key.

```bash
# If using Docker
export ANTHROPIC_BASE_URL=http://localhost:8180

# If running from source
export ANTHROPIC_BASE_URL=http://localhost:8080

# Set your backend-compatible API key
export ANTHROPIC_API_KEY=cpk_your_backend_api_key

# Run Claude Code
claude
```

That's it! Claude Code will now send requests to the proxy, which forwards them to your configured backend.

## How It Works

The proxy's role is simple: it translates API formats and forwards requests.

```
Claude Code                Proxy                    Your Backend
─────────────────────────────────────────────────────────────────
Sends Claude           →   Accepts Claude         →   Receives OpenAI
Messages API request       format                     Chat Completions request
with `ANTHROPIC_API_KEY`   (e.g. cpk_...)             with the same API key.
                                                      (Authorization header
                                                       is passed through)

Receives Claude        ←   Converts OpenAI        ←   Sends OpenAI
response format            response to Claude         response
                           format
```

### Authentication Flow

- The proxy extracts the API key sent by the client (e.g., Claude Code).
- It places this key in the `Authorization` header of the request sent to the backend.
- **Important**: The proxy does **not** replace keys. If you send an Anthropic-specific key (like `sk-ant-...`), it will be forwarded to your backend, which will likely reject it. You must provide a key that your backend recognizes.

## Troubleshooting

**Q: I'm getting a `401 Unauthorized` error.**

A: This means your backend rejected the API key. Make sure `ANTHROPIC_API_KEY` is set to a valid key for the service configured in `BACKEND_URL`.

**Q: How do I check which key is being used?**

A: Run the proxy with debug logging to see headers.

```bash
RUST_LOG=debug cargo run --release
```

Look for logs showing the client authorization header. The proxy will mask the full key for security.

