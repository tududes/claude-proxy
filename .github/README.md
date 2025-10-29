# GitHub Actions CI/CD

Automated testing pipeline for the Claude-to-OpenAI proxy.

## Workflow: test.yml

**Triggers:**
- Push to `main` or `develop` branches
- Pull requests to `main` or `develop`
- Manual workflow dispatch

## Jobs

### 1. Build (Compile Proxy)
```yaml
- Checkout code
- Setup Rust toolchain (stable)
- Cache cargo dependencies
- Build release binary
- Upload artifact for test jobs
```

**Outputs:** 
- `claude_openai_proxy` binary artifact
- Cached for ~2-3 minute builds on subsequent runs

### 2. Test (Matrix Strategy)
```yaml
strategy:
  matrix:
    test: [basic, conversation, parallel]
  fail-fast: false
```

**Each matrix job:**
1. Downloads pre-built proxy binary
2. Makes all scripts executable (`test.sh`, `test/*.sh`)
3. Starts proxy in background (port 8080)
4. Waits for proxy to be ready (health check)
5. Runs: `./test.sh --ci --{basic|conversation|parallel}`
6. Stops proxy (cleanup)

**Uses:**
- `test/payloads/*.json` - Request templates
- `test.sh` - Unified test runner
- Claude Messages API format for all requests

### 3. Test-All (Integration)
Runs complete test suite in single job:
```bash
./test.sh --ci --all
```

Tests:
- ✓ Basic request with system prompt
- ✓ Multi-turn conversation (4 scenarios)
- ✓ Parallel requests (5 concurrent)

### 4. Validate (Spec Compliance)
```bash
./validate_tests.sh
```

Validates all `test/*.sh` scripts for:
- ✓ `/v1/messages` endpoint
- ✓ Claude request format
- ✓ Proper headers
- ✓ SSE event validation
- ✓ MODEL from .env
- ✓ max_tokens required field

## Configuration

### Environment Variables (Workflow Level)
```yaml
env:
  RUST_LOG: info              # Log level for tests
  MODEL: zai-org/GLM-4.5-Air  # Default model
```

### GitHub Secrets (Optional)

Configure in: Repository Settings → Secrets and variables → Actions

```
BACKEND_URL       Backend OpenAI-compatible endpoint
                  Default: http://localhost:8000/v1/chat/completions

BACKEND_KEY       API key for backend (fallback if client doesn't provide)
                  Default: (none)

API_KEY           API key for test requests
                  Default: (none - tests will fail auth but check format)
```

**Note:** Tests will run even without secrets, but backend requests will fail with 401/502. The CI verifies:
- ✓ Proxy starts correctly
- ✓ Accepts Claude API requests
- ✓ Validates request format
- ✓ Forwards to backend
- ✓ Returns proper error codes

## Test Execution Flow

```
GitHub Event (push/PR)
  ↓
Build Job (parallel cache)
  ↓
Test Matrix (parallel: basic, conversation, parallel)
  ├─→ Start proxy → test.sh --ci --basic → Stop
  ├─→ Start proxy → test.sh --ci --conversation → Stop
  └─→ Start proxy → test.sh --ci --parallel → Stop
  ↓
Test-All (sequential all tests)
  └─→ Start proxy → test.sh --ci --all → Stop
  ↓
Validate (spec compliance)
  └─→ validate_tests.sh → Check all test/*.sh
  ↓
✓ All jobs complete
```

## Adding New Tests

1. Create JSON payload in `test/payloads/new_test.json`
2. Add test function to `test.sh`:
   ```bash
   test_new() {
     make_request "test/payloads/new_test.json" "Description"
   }
   ```
3. Add to CLI args and menu
4. Update workflow matrix (optional):
   ```yaml
   matrix:
     test: [basic, conversation, parallel, new]
   ```

## Monitoring CI

**Status Badge (add to README.md):**
```markdown
![Tests](https://github.com/YOUR_USERNAME/claude-proxy/actions/workflows/test.yml/badge.svg)
```

**View Results:**
- Go to: Repository → Actions → test.yml
- See individual job logs
- Download artifacts if needed

## Local CI Simulation

Test exactly what CI will run:

```bash
# Full CI test suite
cargo build --release
RUST_LOG=warn cargo run --release &
sleep 3
./test.sh --ci --all
pkill -f claude_openai_proxy

# Validate compliance
./validate_tests.sh
```

## Troubleshooting

**Tests fail in CI but work locally:**
- Check GitHub secrets are set correctly
- Verify BACKEND_URL is accessible from GitHub runners
- Check logs: Actions → Workflow run → Job → Step details

**Build cache issues:**
- Workflow → Re-run jobs → Re-run all jobs (clears cache)
- Or update Cargo.lock to invalidate cache

**Timeout issues:**
- Increase sleep times in workflow if needed
- Check backend response times
- Consider adding retry logic

