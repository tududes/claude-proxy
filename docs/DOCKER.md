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
```

Or create a `.env` file:

```bash
BACKEND_URL=https://llm.chutes.ai/v1/chat/completions
BACKEND_KEY=
RUST_LOG=info
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

## Production Deployment

For production, consider:

1. **Use a reverse proxy** (nginx, traefik) with SSL/TLS
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

