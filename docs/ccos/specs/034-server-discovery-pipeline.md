---
title: Server Discovery Pipeline
version: 0.1
status: draft
---

# Server Discovery Pipeline

This document specifies the configurable, modular server discovery pipeline for CCOS.
The pipeline is responsible for:

- Discovering candidate servers/APIs from multiple sources
- Introspecting endpoints (MCP/OpenAPI/HTML docs)
- Staging RTFS artifacts in the pending directory
- Creating approval requests and optional secret approvals

All steps are configured via `config/agent_config.toml` under
`[server_discovery_pipeline]`.

## Goals

- **Replaceable stages**: Each stage has an explicit contract and can be replaced.
- **Configurable order**: Stage order is a config value, not a code constant.
- **Extensible sources**: New discovery sources can be added without altering core flow.
- **Auditable behavior**: Staging and approvals are deterministic and logged.

## Configuration

Top-level block (example):

```
[server_discovery_pipeline]
enabled = true
mode = "stage_and_queue"
query_pipeline_order = ["registry_search", "llm_suggest", "rank", "dedupe", "limit"]
introspection_order = ["mcp", "openapi", "browser"]
max_candidates = 30
max_ranked = 15
threshold = 0.65

[server_discovery_pipeline.sources]
mcp_registry = true
npm = true
overrides = true
apis_guru = true
web_search = true
llm_suggest = true
known_apis = true

[server_discovery_pipeline.introspection]
mcp_http = true
mcp_stdio = true
openapi = true
browser = true

[server_discovery_pipeline.staging]
pending_subdir = "capabilities/servers/pending"
server_id_strategy = "sanitize_filename"
layout = "rtfs_layout_v1"

[server_discovery_pipeline.approvals]
enabled = true
expiry_hours = 168
risk_default = "medium"
```

### Precedence rules

- `server_discovery_pipeline.sources.web_search` (if set) overrides
  `missing_capabilities.web_search`.
- If `server_discovery_pipeline.enabled = false`, no discovery runs.

## Pipeline stages and contracts

### Discovery stages (query → candidates)

**Input**
- `DiscoveryQueryContext`:
  - `query` (string): user intent or search query
  - `url_hint` (optional string): docs URL hint
  - `config` (pipeline config)

**Output**
- `Vec<RegistrySearchResult>` candidates, each with:
  - `server_info` (name, endpoint, description, auth hint)
  - `source` (DiscoverySource enum)
  - `category` (DiscoveryCategory enum)
  - `match_score`

**Stages**

1) `registry_search`
   - Uses registry sources: MCP registry, NPM MCP packages, overrides, APIs.guru,
     web search (if enabled).
   - Filters results based on `sources.*` toggles.

2) `llm_suggest`
   - Uses LLM to suggest external APIs.
   - Produces candidates with docs URL as primary endpoint when available.

3) `rank`
   - Uses LLM ranking (optional) to refine match scores.
   - Applies `threshold` to filter low scores.

4) `dedupe`
   - Removes duplicates by name/endpoint.

5) `limit`
   - Applies `max_ranked`.

The order is defined by `query_pipeline_order`.

### Introspection stages (target → introspection result)

**Input**
- `IntrospectionContext`:
  - `target` (string): URL or MCP stdio command
  - `name` (optional string)
  - `auth_env_var` (optional string)
  - `config` (pipeline config)

**Output**
- `IntrospectionResult` (success, source, server_name, optional api_result, optional browser_result, optional manifests)

**Stages**

1) `mcp`
   - MCP HTTP or MCP stdio (depending on target).
   - Uses `MCPDiscoveryService` and MCP introspection.

2) `openapi`
   - Introspects OpenAPI specs (URL heuristic).

3) `browser`
   - Uses headless browser + optional LLM analysis to extract endpoints from docs pages.

The order is defined by `introspection_order`.

### Staging stage (introspection → RTFS artifacts)

**Input**
- `StagingContext`:
  - `target` (string)
  - `server_name`
  - `pending_base` (resolved from `staging.pending_subdir`)
  - `IntrospectionResult`

**Output**
- `RtfsGenerationResult`:
  - `output_dir` (pending server dir)
  - `capability_files` (relative paths)
  - `server_json_path` (server.rtfs)

**Behavior**
- MCP: exports manifests to `pending_subdir/<server_id>/mcp/*.rtfs`.
- OpenAPI/Browser: generates RTFS per endpoint and a server manifest.

### Approval stage (artifacts → approval request)

**Input**
- `ApprovalContext`:
  - `target` (string)
  - `server_name`
  - `capabilities_path`
  - `capability_files`
  - `approvals_config` (expiry, risk level)

**Output**
- `approval_id` (string)

**Behavior**
- Always queues `ServerDiscovery` approvals via `UnifiedApprovalQueue`.
- Optional secret approval is allowed if auth hints are detected.

## Error handling and fallback

- Each stage returns a structured error with context.
- Introspection stages are tried in configured order.
- If all fail, the pipeline returns an error with the last failure reason.

## Extensibility

To add a new stage:

1) Implement a new stage type:
   - `DiscoveryStage` for candidate sources
   - `IntrospectionStage` for new protocols
   - `StagingStage` for new output formats
   - `ApprovalStage` for custom approval systems
2) Add the stage name to config order arrays.
3) Update pipeline wiring to construct the stage when the name is present.

This design allows new sources (e.g. internal catalogs, marketplace registries,
Postman collections) without touching the pipeline core logic.
