# CCOS Specification 006: Delegating Arbiter RTFS-First with JSON Fallback

**Status: Implemented**
**Scope**: Defines the parsing strategy for Arbiter responses (RTFS primary, JSON fallback).

## Overview

The `DelegatingArbiter` now implements a RTFS-first approach for intent generation with graceful JSON fallback. This ensures we prioritize the native RTFS format while maintaining compatibility with LLMs that prefer to respond in JSON.

## Implementation

### Intent Generation Flow

1. **Prompt Creation**: The system explicitly requests RTFS format output from the LLM
   - Added clear instructions: "CRITICAL: You must respond with RTFS syntax, NOT JSON"
   - Emphasizes output should be pure RTFS without markdown wrappers

2. **RTFS Parsing (Primary)**: First attempt to parse the response as RTFS
   - Uses the existing RTFS parser (`crate::parser::parse`)
   - Extracts intent blocks using balanced parenthesis matching
   - Sanitizes regex literals for parser compatibility

3. **JSON Fallback (Secondary)**: If RTFS parsing fails, attempt JSON parsing
   - Extracts JSON from response (handles markdown code blocks)
   - Parses structured JSON fields: `goal`, `name`, `constraints`, `preferences`
   - Case-insensitive field matching (supports `Goal`, `goal`, `GOAL`, etc.)
   - Converts JSON types to RTFS `Value` types

4. **Metadata Tracking**: Intents parsed from JSON are marked with metadata
   - Key: `"parse_format"`
   - Value: `"json_fallback"`
   - Allows downstream systems to track parsing method

## Code Changes

### Modified Methods

#### `generate_intent_with_llm`
- Changed from calling `llm_provider.generate_intent()` (which expects JSON)
- Now calls `llm_provider.generate_text()` to get raw response
- Implements try-parse-RTFS-first, then-fallback-to-JSON logic
- Logs both attempts for debugging

#### `create_intent_prompt`
- Updated to explicitly request RTFS output format
- Added warnings against JSON responses
- Emphasizes pure s-expression output without markdown

### New Methods

#### `parse_json_intent_response`
- Parses JSON responses as fallback
- Handles flexible field names (case-insensitive)
- Converts JSON types to RTFS Value types
- Marks intents with `"parse_format": "json_fallback"` metadata

## Benefits

1. **RTFS Native**: Encourages LLMs to use the native RTFS format
2. **Graceful Degradation**: Falls back to JSON when needed
3. **Transparency**: Metadata tracking shows which format was used
4. **Backward Compatibility**: Existing JSON-based LLMs still work
5. **Future-Ready**: Can detect and optimize for RTFS-capable models

## Testing

### Unit Tests

1. **`test_intent_generation`**: Verifies RTFS parsing from stub provider
2. **`test_json_fallback_parsing`**: Explicitly tests JSON fallback path
3. **`test_agent_registry`**: Ensures agent selection still works
4. **`test_delegating_arbiter_creation`**: Basic initialization test

### Test Results
```
✓ Successfully parsed intent from RTFS format
✓ Successfully parsed intent from JSON format
test result: ok. 4 passed; 0 failed
```

## Logging

The system logs parsing attempts to `logs/arbiter_llm.log`:
- Event: `llm_intent_generation` - Raw LLM response
- Format detection and fallback triggers
- Success/failure of each parsing method

## Future Enhancements

1. **Adaptive Format Selection**: Track which models prefer RTFS vs JSON
2. **Format Hints**: Include model-specific format preferences in prompts
3. **Hybrid Parsing**: Attempt both parsers in parallel for maximum robustness
4. **Quality Scoring**: Compare RTFS vs JSON outputs for accuracy
5. **Model Registry**: Store format capabilities per LLM provider

## Related Files

- `ccos/src/arbiter/delegating_arbiter.rs` - Main implementation
- `ccos/src/arbiter/llm_provider.rs` - LLM provider interface
- `rtfs/src/parser/mod.rs` - RTFS parser (formerly `parser.rs`)
- `logs/arbiter_llm.log` - Runtime logging

## Migration Notes

- No breaking changes to existing API
- Existing tests continue to pass
- JSON-based systems work without modification
- RTFS-capable LLMs now get prioritized correctly
