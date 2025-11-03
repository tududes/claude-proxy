# Docker Deployment Guide

## Quick Start

```bash
# Start the proxy (uses default Chutes backend)
docker-compose up -d

# View logs
docker-compose logs -f

# Stop the proxy
docker-compose down
```

## Configuration

Set environment variables before running `docker-compose up`:

```bash
# Backend URL (default: https://llm.chutes.ai/v1/chat/completions)
export BACKEND_URL=https://your-backend.com/v1/chat/completions

# Log level (default: info)
export RUST_LOG=debug

# Caddy TLS configuration (default: true)
export CADDY_TLS=false          # Set to false for plaintext HTTP
export CADDY_DOMAIN=            # Your domain (optional)
export CADDY_PORT=8180          # Caddy port (default: 8180)
```

Or create a `.env` file:

```bash
BACKEND_URL=https://llm.chutes.ai/v1/chat/completions
RUST_LOG=info

# Caddy configuration (optional)
CADDY_TLS=true
CADDY_DOMAIN=
CADDY_PORT=8180
```

## Usage Examples

### With Chutes (default)
```bash
docker-compose up -d
```

### With Custom Backend
```bash
BACKEND_URL=http://localhost:8000/v1/chat/completions docker-compose up -d
```

### With Debug Logging
```bash
RUST_LOG=debug docker-compose up -d
```

## Building Manually

```bash
# Build the image
docker build -t claude-openai-proxy .

# Run the container
docker run -d \
  -p 8080:8080 \
  -e BACKEND_URL=https://llm.chutes.ai/v1/chat/completions \
  -e RUST_LOG=info \
  --name claude-proxy \
  claude-openai-proxy
```

## Health Check

The container includes a health check that runs every 30 seconds:

```bash
# Check container health
docker-compose ps

# Manual health check
curl -f http://localhost:8080/health
```

## Troubleshooting

### View logs
```bash
docker-compose logs -f claude-proxy
```