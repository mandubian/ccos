# LLM Documentation Crawler - Implementation Status

**Date**: 2026-01-02  
**Status**: Core functionality complete, pending end-to-end testing

## Overview

The LLM-powered documentation crawler enables CCOS to discover and introspect APIs from human-readable documentation pages when OpenAPI specs are unavailable.

## Architecture

```
User: "discover coinbase"
     │
     ▼
┌─────────────────────────────────────┐
│  RegistrySearcher                   │
│  - MCP Registry                     │
│  - APIs.guru                        │
│  - Web Search (optional)            │
└─────────────────────────────────────┘
     │
     ▼ RegistrySearchResult[]
┌─────────────────────────────────────┐
│  DialoguePlanner / TurnProcessor    │
│  - Presents results to user         │
│  - Handles explore/connect commands │
└─────────────────────────────────────┘
     │
     ▼ User: "explore 1"
┌─────────────────────────────────────┐
│  LlmDocParser                       │
│  - explore_documentation()          │
│    → Finds API links (REST/WS/etc)  │
│  - parse_documentation()            │
│    → Extracts endpoints from HTML   │
└─────────────────────────────────────┘
     │
     ▼ User: "connect 1"
┌─────────────────────────────────────┐
│  Configuration Instructions         │
│  - Shows how to configure in MCP    │
│  - Env vars for authentication      │
└─────────────────────────────────────┘
```

## Implemented Features

### 1. `explore <N>` Command
- **File**: `ccos/src/planner/dialogue_planner/turn_processor.rs`
- Takes an index from discovery results
- Calls `LlmDocParser::explore_documentation()` to find API links
- If page is API documentation, calls `parse_documentation()` to extract endpoints
- Adds discovered links to results for recursive exploration

### 2. LLM-based Link Discovery
- **File**: `ccos/src/synthesis/introspection/llm_doc_parser.rs`
- `explore_documentation()`: Fetches page, uses LLM to identify:
  - REST API documentation links
  - WebSocket API links
  - OpenAPI/Swagger spec URLs
  - GraphQL endpoints

### 3. LLM-based Endpoint Parsing
- **File**: `ccos/src/synthesis/introspection/llm_doc_parser.rs`
- `parse_documentation()`: Extracts from HTML:
  - API endpoints (method, path, parameters)
  - Authentication requirements
  - Data types

### 4. OpenAPI Introspection
- **File**: `ccos/src/synthesis/introspection/api_introspector.rs`
- `introspect_from_openapi()`: Fetches and parses OpenAPI specs
- Converts to `APIIntrospectionResult` with typed schemas

### 5. LLM Provider Integration
- **File**: `ccos/src/arbiter/delegating_arbiter.rs`
- Added `get_llm_provider()` method
- `TurnProcessor` receives `Arc<dyn LlmProvider>` via `with_llm_provider()`

## Key Files Modified

| File | Changes |
|------|---------|
| `turn_processor.rs` | Added `Explore` handler, OpenAPI introspection, endpoint parsing |
| `llm_doc_parser.rs` | Added `parse_documentation()` public method |
| `delegating_arbiter.rs` | Added `get_llm_provider()` |
| `planner.rs` | Wires LLM provider into TurnProcessor |
| `types.rs` | Added `Explore { index }` intent |
| `entity.rs` | Added `explore N` command parsing |

## Current State

### ✅ Complete
- [x] `explore_documentation()` - Find API links on pages
- [x] `parse_documentation()` - Extract endpoints from HTML
- [x] OpenAPI spec introspection
- [x] Recursive exploration (links added to results)
- [x] LLM provider wiring through arbiter
- [x] `explore <N>` command handler

### 🔄 Partial
- [ ] `connect` for parsed APIs - Shows config instructions but doesn't auto-generate adapters

### ⏳ Pending
- [ ] End-to-end testing with Coinbase/Kraken
- [ ] WebSocket API parsing
- [ ] Capability persistence (save parsed endpoints to .rtfs files)
- [ ] Depth limiting for recursive crawls
- [ ] URL deduplication during crawling

## Usage Example

```
> discover coinbase
📦 Found 5 results:
  [1] Coinbase Advanced Trade (Web)
  [2] Coinbase Commerce API (APIs.guru)
  ...

> explore 1
🔍 Exploring documentation at: https://docs.cdp.coinbase.com/...
✅ Analysis complete
  🔗 API Links:
    - [REST API](https://docs.../rest-api) - rest
    - [WebSocket](https://docs.../websocket) - websocket

> explore 1  (now points to REST API link)
📖 Parsing endpoints from documentation...
✨ Discovered 12 Endpoints:
  - GET /accounts - List accounts
  - POST /orders - Create order
  ...
```

## Environment Variables

- `OPENAI_API_KEY` - Required for LLM parsing
- `CCOS_ENABLE_WEB_SEARCH` - Enable web search discovery
- `CCOS_DEBUG` - Enable debug output

## Next Steps

1. **Test with real APIs**: Run against Coinbase, Kraken, Binance docs
2. **WebSocket parsing**: Extend LLM prompts for WS message formats
3. **Capability persistence**: Save parsed endpoints as .rtfs capabilities
4. **Auth detection**: Better inference of auth requirements from docs
