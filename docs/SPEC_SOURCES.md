# API Specification Sources

This document describes the API specification sources cloned for analysis and comparison.

## Directory Structure

```
claude-proxy/
├── anthropic-spec/          # Anthropic TypeScript SDK (spec source)
├── openai-spec/             # OpenAI spec pointer (main branch)
├── openai-spec-manual/      # OpenAI manual spec (actual YAML)
└── docs/
    ├── API_COMPARISON.md    # Detailed feature comparison
    └── SPEC_ANALYSIS_SUMMARY.md  # Executive summary
```

## Anthropic Messages API

**Source:** `anthropic-spec/` (cloned from `github.com/anthropics/anthropic-sdk-typescript`)

**Key Files:**
- `src/resources/messages/messages.ts` - Message API types and interfaces
- `src/resources/messages/batches.ts` - Batch API
- `api.md` - API documentation index
- `examples/` - Usage examples

**What to Look At:**
- `MessageCreateParams` interface - Request structure
- `Message` interface - Response structure
- `ContentBlock` types - All supported content types (text, image, tool_use, etc.)
- `Tool` types - Tool definitions
- `RawMessageStreamEvent` - Streaming event types

**Official Docs:** https://docs.anthropic.com/en/api/messages

## OpenAI Chat Completions API

**Source:** `openai-spec-manual/` (cloned from `github.com/openai/openai-openapi`, branch: `manual_spec`)

**Key Files:**
- `openapi.yaml` - Complete OpenAPI 3.0 specification
- Lines 1642-2915: `/chat/completions` endpoint
- Lines 19285-19656: `CreateChatCompletionRequest` schema
- Lines 19656-19850: `CreateChatCompletionResponse` schema

**Also Available:**
- `openai-spec/openapi.documented.yml` - Auto-generated spec from Stainless

**What to Look At:**
- Search for "CreateChatCompletionRequest" - Request schema
- Search for "CreateChatCompletionResponse" - Response schema  
- Search for "ChatCompletionRequestMessage" - Message types
- Search for "/chat/completions" - API endpoints
- Tool/function definitions

**Official Docs:** https://platform.openai.com/docs/api-reference/chat

## Quick Comparison Commands

### Search Anthropic Spec
```bash
# Find all message types
grep -r "interface.*Message" anthropic-spec/src/resources/messages/

# Find content block types
grep -r "ContentBlock" anthropic-spec/src/resources/messages/messages.ts

# Find tool definitions
grep -r "Tool" anthropic-spec/src/resources/messages/messages.ts | grep "interface\|type"
```

### Search OpenAI Spec
```bash
# Find chat completion schemas
grep -n "CreateChatCompletionRequest:" openai-spec-manual/openapi.yaml

# Find message types
grep -n "ChatCompletionRequestMessage" openai-spec-manual/openapi.yaml

# Find streaming response
grep -n "ChatCompletionChunk" openai-spec-manual/openapi.yaml

# Find tool definitions
grep -n "ChatCompletionTool" openai-spec-manual/openapi.yaml
```

## Updating Specs

To refresh the specifications:

```bash
cd /root/claude-proxy

# Update Anthropic spec
cd anthropic-spec
git pull origin main
rm -rf .git
cd ..

# Update OpenAI spec
cd openai-spec-manual
git pull origin manual_spec
rm -rf .git
cd ..

# Download latest auto-generated OpenAI spec
cd openai-spec
curl -o openapi.documented.yml https://app.stainless.com/api/spec/documented/openai/openapi.documented.yml
cd ..
```

## Spec Versions

**As of 2025-11-08:**
- Anthropic SDK: Latest from main branch
- OpenAI Manual Spec: Latest from manual_spec branch
- OpenAI Auto Spec: Downloaded from Stainless

These specs are **NOT tracked in git** (they're cloned externally for reference only).

## Analysis Documents

After cloning and analyzing these specs, we created:

1. **`docs/API_COMPARISON.md`** (comprehensive)
   - Line-by-line comparison of request/response structures
   - Feature support matrix
   - Translation fidelity analysis
   - Missing features catalog

2. **`docs/SPEC_ANALYSIS_SUMMARY.md`** (executive summary)
   - Key findings
   - Compatibility assessment
   - Recommendations
   - Use case analysis

## Integration with Proxy

Our proxy (`src/handlers/messages.rs`) translates:

```
Claude Messages API → OpenAI Chat Completions → Backend → OpenAI SSE → Claude SSE
```

Key translation points:
- `ClaudeRequest` → `OAIChatReq` (request transformation)
- OpenAI SSE chunks → Claude SSE events (response transformation)
- Tool calls: `tool_use` ↔ `tool_calls`
- Finish reasons: `end_turn` ↔ `stop`, `max_tokens` ↔ `length`, etc.

See `src/utils/content_extraction.rs` for translation utilities.

## Notes

- These spec directories are for **reference only**
- They are **NOT** built into the proxy
- They are used for **manual comparison** and **documentation**
- The proxy implementation is based on practical compatibility, not 100% spec compliance
- When in doubt, refer to official docs (links above)

## License

- Anthropic SDK: MIT (see `anthropic-spec/LICENSE`)
- OpenAI Spec: MIT (see `openai-spec-manual/LICENSE`)
- This proxy: MIT (see `LICENSE`)

