# Multi-stage build for optimal image size
FROM rust:latest AS builder

WORKDIR /app

# Copy manifests
COPY Cargo.toml Cargo.lock ./

# Copy source code
COPY src ./src

# Build for release
RUN cargo build --release

# Runtime stage - use slim Debian image
FROM debian:bookworm-slim

# Install CA certificates and curl for HTTPS and healthchecks
RUN apt-get update && \
    apt-get install -y ca-certificates curl && \
    rm -rf /var/lib/apt/lists/*

WORKDIR /app

# Copy the binary from builder
COPY --from=builder /app/target/release/claude_openai_proxy /app/claude_openai_proxy

# Expose port 8080
EXPOSE 8080

# Set environment variables (can be overridden in docker-compose)
ENV RUST_LOG=info
ENV BACKEND_URL=http://127.0.0.1:8000/v1/chat/completions

# Run the proxy
CMD ["/app/claude_openai_proxy"]

