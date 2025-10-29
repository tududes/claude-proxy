# Claude Proxy for Chutes.ai

Lightweight, stateless proxy that translates Claude Messages API to OpenAI Chat Completions format with SSE streaming.

**Use Case:** Route Claude Code / Claude API requests to any OpenAI-compatible backend (SGLang, vLLM, Ollama, Chutes, etc.)

## Features

- Full Claude API compatibility (text, images, tool_use, tool_result)
- Real-time SSE streaming with proper event formatting
- Smart authentication routing and key forwarding
- Automatic model discovery with 60-second cache refresh
- Case-insensitive model matching with helpful 404 responses
- Token counting endpoint
- Docker-ready with health checks
- Single ~4MB binary, minimal dependencies

## Quick Reference

**Using Remote Proxy (Production):**
```bash
export ANTHROPIC_BASE_URL=https://claude-proxy.chutes.ai
export ANTHROPIC_API_KEY=cpk_your_chutes_api_key
claude
```

**Running Local Proxy (Development):**
```bash
docker-compose up -d
export ANTHROPIC_BASE_URL=http://localhost:8080
export ANTHROPIC_API_KEY=cpk_your_chutes_api_key
claude
```

## Quick Start

**With Docker:**
```bash
# Start proxy (uses Chutes backend by default)
docker-compose up -d

# Configure Claude Code
export ANTHROPIC_BASE_URL=http://localhost:8080
export ANTHROPIC_API_KEY=cpk_your_chutes_api_key
claude
```

**From Source:**
```bash
# Build and run
cargo build --release
cargo run --release

# Configure Claude Code
export ANTHROPIC_BASE_URL=http://localhost:8080
export ANTHROPIC_API_KEY=cpk_your_chutes_api_key
claude
```

**Test:**
```bash
./test.sh --all    # Runs full test suite
```

## Deployment

**Server deployment:**
```bash
# Deploy with Docker
docker-compose up -d

# Add SSL with reverse proxy (nginx/caddy)
# See DOCKER.md for detailed setup
```

**Client connection to remote proxy:**
```bash
export ANTHROPIC_BASE_URL=https://your-domain.com:8080
export ANTHROPIC_API_KEY=cpk_your_chutes_api_key
claude
```

See [DOCKER.md](DOCKER.md) for complete deployment guide including SSL/TLS setup.

## Configuration

Create a `.env` file:
```bash
# Backend
BACKEND_URL=https://llm.chutes.ai/v1/chat/completions
BACKEND_KEY=cpk_your_api_key           # Optional fallback

# Testing
CHUTES_TEST_API_KEY=cpk_your_key       # For test suite

# Logging (error, warn, info, debug, trace)
RUST_LOG=info
```

**Environment variables:**
```bash
# Override backend
BACKEND_URL=http://localhost:8000/v1/chat/completions cargo run --release

# Override auth
BACKEND_KEY=your-api-key cargo run --release
```

**Authentication flow:**
1. Client sends backend-compatible key (`cpk_*`) → forwarded to backend
2. Client sends Anthropic token (`sk-ant-*`) → replaced with `BACKEND_KEY`
3. No client auth → uses `BACKEND_KEY` as fallback

## Usage

**Model selection:**
```bash
/model zai-org/GLM-4.5-Air              # Free
/model deepseek/DeepSeek-R1             # Reasoning  
/model anthropic/claude-3-5-sonnet      # Standard
```

**Features:**
- Automatic model discovery (60s cache refresh)
- Case-insensitive matching: `glm-4.5-air` → `GLM-4.5-Air`
- Helpful 404 responses with categorized model lists

**Other SDKs:**
```python
# Python
from anthropic import Anthropic
client = Anthropic(base_url="http://localhost:8080", api_key="cpk_key")
```

```javascript
// JavaScript
const client = new Anthropic({
  baseURL: 'http://localhost:8080',
  apiKey: 'cpk_key'
});
```

**Troubleshooting:**
- 401 Unauthorized: Check `BACKEND_KEY` in `.env`
- 404 Model Not Found: Use `/model` in Claude Code to see available models
- Debug logging: `RUST_LOG=debug cargo run --release`

## API

**Endpoints:**
- `POST /v1/messages` - Main Claude Messages API endpoint
- `POST /v1/messages/count_tokens` - Token estimation (~4 chars per token)

**Supported content types:**
- Text (string or content blocks)
- Images (base64 encoded, automatically converted to OpenAI data URI format)
- Tool use and tool results
- Multi-turn conversations with system prompts

**Example request:**
```bash
curl -N http://localhost:8080/v1/messages \
  -H 'content-type: application/json' \
  -d '{
    "model": "zai-org/GLM-4.5-Air",
    "messages": [{"role": "user", "content": "Hello"}],
    "max_tokens": 128,
    "stream": true
  }'
```

See [docs/CLAUDE_CODE_SUPPORT.md](docs/CLAUDE_CODE_SUPPORT.md) for complete API documentation and all 9 Claude Code patterns.

## Image Support

The proxy fully supports image forwarding from Claude to OpenAI-compatible backends.

**Automatic Conversion:**
- Claude format: `{type: "image", source: {media_type: "image/png", data: "base64..."}}`
- OpenAI format: `{type: "image_url", image_url: {url: "data:image/png;base64,..."}}` ✅

**Vision-Capable Models (Tested):**
- `shisa-ai/shisa-v2-llama3.3-70b` ✅ 
- `claude-3-5-sonnet-*` ✅
- Models with "vision" in name/features ✅

**Non-Vision Models:**
- `zai-org/GLM-4.6-turbo` ❌ (reasoning-only, no image support)
- Most standard text models ❌

**Testing Images:**
```bash
# Test backend directly (uses .env)
./test_backend_direct.sh               # Default model
./test_backend_direct.sh $KEY MODEL    # Specific model

# Test through proxy
./test_image.sh YOUR_API_KEY          # Quick test
python test_image.py YOUR_API_KEY     # Detailed test
```

**Debugging:**
- Run with `RUST_LOG=debug cargo run` to see image processing
- Look for: `🖼️ Processing image`, `🔍 Detected image type: PNG/JPEG`
- If model doesn't respond to images, it likely lacks vision support

## Testing

**Quick test:**
```bash
./test.sh                               # Interactive, prompts for CHUTES_TEST_API_KEY
./test.sh --all                         # Run all tests
CHUTES_TEST_API_KEY=cpk_key ./test.sh --ci --all  # CI mode
```

**Set API key:**
```bash
export CHUTES_TEST_API_KEY=cpk_your_key
# Or add to .env
echo "CHUTES_TEST_API_KEY=cpk_your_key" >> .env
```

**Available tests:**
- `test.sh --basic` - Basic request
- `test.sh --conversation` - Multi-turn
- `test.sh --parallel` - Concurrent requests
- `./validate_tests.sh` - Compliance check

See [tests/README.md](tests/README.md) for complete test documentation.

## Building

```bash
cargo build --release       # Binary: target/release/claude_openai_proxy (~6MB)
cargo test                  # Run unit tests
```

