# Claude Proxy for Chutes.ai

Lightweight proxy that translates Claude Messages API to OpenAI Chat Completions format with SSE streaming.

Routes Claude Code / Claude API requests to any OpenAI-compatible backend (SGLang, vLLM, Ollama, etc.)

## Features

- Full Claude API compatibility (text, images, tool_use, tool_result)
- SSE streaming with proper event formatting
- Client key forwarding (forwards client API keys directly to backend)
- Model discovery with 60s cache refresh
- Case-insensitive model matching with helpful 404 responses
- Token counting endpoint (tiktoken-based)
- Circuit breaker (opens after 5 failures, recovers after 30s)
- Health check endpoint
- Request validation (max 1000 messages, 5MB content)

## Quick Start

**Docker (default port 8181):**
```bash
docker-compose up -d
export ANTHROPIC_BASE_URL=http://localhost:8181
export ANTHROPIC_API_KEY=cpk_your_api_key
claude
```

**From Source (default port 8080):**
```bash
cargo build --release
cargo run --release
export ANTHROPIC_BASE_URL=http://localhost:8080
export ANTHROPIC_API_KEY=cpk_your_api_key
claude
```

## Configuration

Environment variables:
- `BACKEND_URL` - Backend chat completions endpoint (default: `http://127.0.0.1:8000/v1/chat/completions`)
- `HOST_PORT` - Port to listen on (default: `8080`)
- `RUST_LOG` - Log level: `error`, `warn`, `info`, `debug`, `trace` (default: `info`)
- `BACKEND_TIMEOUT_SECS` - Backend request timeout in seconds (default: `600`)

**Example `.env`:**
```bash
BACKEND_URL=https://llm.chutes.ai/v1/chat/completions
HOST_PORT=8080
RUST_LOG=info
```

**Authentication:**
- Client API key (`cpk_*` or backend-compatible) → forwarded directly to backend
- Anthropic OAuth tokens (`sk-ant-*`) → rejected with 401 (not supported)
- No client auth → rejected with 401

## API Endpoints

- `POST /v1/messages` - Main Claude Messages API endpoint
- `POST /v1/messages/count_tokens` - Token counting (tiktoken-based)
- `GET /health` - Health check with circuit breaker status

**Example request:**
```bash
curl -N http://localhost:8080/v1/messages \
  -H 'Content-Type: application/json' \
  -H 'Authorization: Bearer cpk_your_key' \
  -d '{
    "model": "zai-org/GLM-4.5-Air",
    "messages": [{"role": "user", "content": "Hello"}],
    "max_tokens": 128,
    "stream": true
  }'
```

## Supported Features

- **Text content** - String or content blocks
- **Images** - Base64 encoded, converted to OpenAI data URI format
- **Tool use/results** - Full function calling support
- **System prompts** - Converted to system message
- **Multi-turn conversations** - Context preservation
- **Model discovery** - Auto-refresh every 60s, case-insensitive matching

## Usage with Claude Code

**Model selection:**
```bash
/model zai-org/GLM-4.5-Air              # Free
/model deepseek/DeepSeek-R1             # Reasoning  
/model anthropic/claude-3-5-sonnet      # Standard
```

**Other SDKs:**
```python
from anthropic import Anthropic
client = Anthropic(
    base_url="http://localhost:8080",
    api_key="cpk_your_key"
)
```

```javascript
const client = new Anthropic({
  baseURL: 'http://localhost:8080',
  apiKey: 'cpk_your_key'
});
```

## Deployment

**Docker Compose:**
```bash
docker-compose up -d
```

Includes Caddy reverse proxy for SSL/TLS. See [docs/DOCKER.md](docs/DOCKER.md) for production setup.

**Remote Client Connection:**
```bash
export ANTHROPIC_BASE_URL=https://your-domain.com
export ANTHROPIC_API_KEY=cpk_your_api_key
claude
```

## Testing

```bash
./test.sh --all    # Run full test suite
```

Set `CHUTES_TEST_API_KEY=cpk_your_key` in `.env` or export before running tests.

## Building

```bash
cargo build --release    # Binary: target/release/claude_openai_proxy (~4MB)
cargo test              # Run unit tests
```

## Documentation

- [API Reference](docs/API_REFERENCE.md) - Complete API specification
- [Docker Guide](docs/DOCKER.md) - Deployment with SSL/TLS
- [Production Guide](docs/PRODUCTION_GUIDE.md) - Production features and monitoring
- [Implementation Details](docs/IMPLEMENTATION_DETAILS.md) - Architecture and design

## Troubleshooting

- **401 Unauthorized** - Ensure client sends valid backend-compatible API key (`cpk_*`). Anthropic OAuth tokens (`sk-ant-*`) are not supported.
- **404 Model Not Found** - Use `/model` in Claude Code to see available models
- **Circuit breaker open** - Backend failing; check health endpoint: `curl http://localhost:8080/health`
- **Debug logging** - `RUST_LOG=debug cargo run --release`
