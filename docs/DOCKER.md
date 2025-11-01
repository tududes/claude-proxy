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

# Backend API key (optional - leave empty to forward client's key)
export BACKEND_KEY=your-api-key-here

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
BACKEND_KEY=
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
curl -f http://localhost:8080/v1/messages/count_tokens \
  -H "Content-Type: application/json" \
  -d '{"model":"test","messages":[]}'
```

## Troubleshooting

### View logs
```bash
docker-compose logs -f claude-proxy
```

### Restart container
```bash
docker-compose restart
```

### Rebuild after code changes
```bash
docker-compose build --no-cache
docker-compose up -d
```

### Connect to running container
```bash
docker-compose exec claude-proxy /bin/bash
```

## Caddy TLS Configuration

The docker-compose setup includes Caddy for optional SSL/TLS termination.

### Plaintext HTTP (Development)
```bash
export CADDY_TLS=false
docker-compose up -d
# Access at http://localhost:8180
```

### Auto-HTTPS with Let's Encrypt (Production)
```bash
export CADDY_TLS=true
export CADDY_DOMAIN=your-domain.com
export CADDY_PORT=443
docker-compose up -d
# Caddy automatically provisions SSL certificate
# Access at https://your-domain.com
```

### How it works
- `CADDY_TLS=true` (default): Caddy enables auto-HTTPS with automatic certificate provisioning
- `CADDY_TLS=false`: Caddy runs in plaintext mode, no SSL/TLS overhead
- The entrypoint script (`caddy-entrypoint.sh`) sets the protocol prefix based on `CADDY_TLS`

## Production Deployment

For production, consider:

1. **Built-in Caddy with TLS** (recommended):
```bash
CADDY_TLS=true CADDY_DOMAIN=your-domain.com CADDY_PORT=443 docker-compose up -d
```

2. **Resource limits** in docker-compose.yaml:
```yaml
deploy:
  resources:
    limits:
      cpus: '1'
      memory: 512M
```

3. **Persistent logs**:
```yaml
volumes:
  - ./logs:/app/logs
```

4. **Network isolation**:
```yaml
networks:
  - proxy-network
```

