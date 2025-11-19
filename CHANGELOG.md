# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [0.1.8] - 2025-11-19

### Fixed
- **Streaming parser UTF-8 safety** - Reworked `SseEventParser` to buffer raw bytes, preventing corrupted characters when SSE chunks split multi-byte sequences.
- **SSE chunk parsing overhead** - Removed double JSON parsing per chunk, reducing CPU usage under heavy streaming loads.

### Changed
- **Version bump** - Updated crate metadata to `0.1.8` for release tagging.

## [0.1.7] - 2025-11-19

### Fixed
- **Parallel Tool Use** - Mapped `disable_parallel_tool_use: true` in `tool_choice` to `parallel_tool_calls: false` in OpenAI request.
- **Stop Sequences** - Truncated `stop_sequences` to 4 items to prevent OpenAI backend errors.
- **API Compatibility** - Added `parallel_tool_calls` support to `OAIChatReq` model.

### Changed
- **Refactoring** - Updated `convert_tool_choice` to extract parallel tool use settings.

## [0.1.6] - 2025-11-10

### Fixed
- **Token counting accuracy** - Fixed token counting in `/v1/messages` endpoint
- **Output token tracking** - Improved output token tracking for synthetic responses
- **Usage reporting** - Fixed unused fields warning in OpenAI usage struct

### Changed
- **Install script** - Improved model selection in installation script

## [0.1.5] - 2025-11-08

### Added
- **`tool_choice` parameter support** - Can now force model to use specific tools or disable tool usage
- **`top_k` sampling parameter support** - Advanced sampling control now available
- **Validation warnings** - Warns when `metadata` or `service_tier` parameters are used (accepted but not forwarded to backend)

### Changed
- **Increased message limit from 1,000 to 10,000** - 10x capacity increase for long conversations
- **Improved API compatibility** - Now at ~95% compatibility (up from ~90%)

### Documentation
- Created comprehensive API specification analysis
  - `docs/API_COMPARISON.md` - Detailed Anthropic vs OpenAI spec comparison
  - `docs/SPEC_ANALYSIS_SUMMARY.md` - Executive summary of compatibility
  - `SPEC_SOURCES.md` - Information about cloned API specs
- Added 81 unit tests with 90%+ coverage of critical utilities
  - Content extraction tests (32)
  - Authentication tests (24)
  - SSE streaming tests (25)
- Created test coverage documentation
  - `docs/TEST_COVERAGE_ANALYSIS.md` - Planning and analysis
  - `docs/TEST_COVERAGE_REPORT.md` - Detailed results
  - `TEST_COVERAGE_SUMMARY.md` - Executive summary

### Fixed
- Better error messaging for unsupported parameters

## [0.1.4] - Previous Release

(Previous changes not documented in this changelog)

