# Autonoetic: Data Models & Schemas

This document defines the strict, concrete data models that form the contract between the Rust Gateway, the Agent Orchestrator, the SDK, and the external ecosystem. All components MUST adhere to these schemas.

## 1. Agent Manifest (`SKILL.md`)

The Manifest defines an Agent's identity, routing, UI configuration, and access control. It lives at the root of the Agent's directory. By natively adopting the `SKILL.md` format (YAML frontmatter + Markdown body), Autonoetic aligns with the AgentSkills.io standard, treating Agents themselves as highly capable, persistent skills.

```markdown
---
name: "agent-research-alpha"
description: "Specialized in fetching and summarizing academic papers."
compatibility: "Autonoetic Gateway >=0.1.0; requires internet access for research APIs."
metadata:
  autonoetic:
    version: "1.0"

    # Runtime declaration — what engine runs this Agent
    runtime:
      engine: "autonoetic"            # Declares this as an Autonoetic-managed agent
      gateway_version: ">=0.1.0"      # Minimum compatible Gateway binary version
      sdk_version: ">=0.1.0"          # Minimum compatible SDK version
      type: "stateful"                # stateful (Agent with memory/loop) vs stateless (Tool)
      sandbox: "bubblewrap"           # Execution environment: bubblewrap | docker | microvm | wasm
      runtime_lock: "runtime.lock"    # Pinned runtime closure file bundled with the agent

    agent:
      id: "agent_research_alpha"
      name: "Deep Researcher"
      description: "Specialized in fetching and summarizing academic papers."

    # Standard JSON Schema for UI Configuration
    # Rendered by frontends (CLI/Web) before booting; results injected to Tier 1 memory.
    input_schema:
      type: "object"
      required: ["research_depth"]
      properties:
        research_depth:
          type: "string"
          enum: ["quick", "thorough", "exhaustive"]
          default: "thorough"

    # Maps Gateway metrics or Tier 2 Memory keys to frontend charts
    dashboard:
      metrics:
        - label: "Queries Solved"
          memory_key: "research_stats.queries_solved"
          format: "counter"
        - label: "Sources Cited"
          memory_key: "research_stats.sources_cited"
          format: "gauge"

    capabilities:
      # The typed Capability Enum required by the Gateway
      - type: "ToolInvoke"
        allowed: ["web_fetch", "file_read", "mcp_github_create_issue"]
      - type: "MemoryRead"
        scopes: ["self.state.*", "global.facts.*"]
      - type: "MemoryWrite"
        scopes: ["self.state.*"]
      - type: "NetConnect"
        hosts: ["api.semanticscholar.org", "arxiv.org"]
      - type: "AgentSpawn"
        max_children: 3
      - type: "AgentMessage"
        patterns: ["agent_coder_*"]
      - type: "ShellExec"
        patterns: ["python3 scripts/*", "uv run *"]

    llm_config:
      # Abstract driver resolution
      provider: "anthropic"    # Can override Gateway default
      model: "claude-3-5-sonnet-latest"
      temperature: 0.2
      fallback_provider: "openai"
      fallback_model: "gpt-4o"

    limits:
      max_memory_mb: 512
      max_execution_time_sec: 120
      token_budget_monthly: 5000000
---

# Deep Researcher System Prompt

You are an autonomous research agent. Your goal is to fetch, synthesize, and format academic papers for the user.

## Core Directives
1. Always evaluate sources for credibility.
2. Cross-reference claims across multiple documents.
3. Output findings strictly in the format defined in your UI settings.
```

## 2. Runtime Lock (`runtime.lock`)

The Runtime Lock pins the execution closure required to reproduce an Agent or Skill on another Gateway.

```yaml
gateway:
  artifact: "marketplace://gateway/ccos-gateway"
  version: "0.1.0"
  sha256: "abc123..."
  signature: "ed25519:deadbeef..."

sdk:
  version: "0.1.0"

sandbox:
  backend: "bubblewrap"

dependencies:
  - runtime: "python" # "python" | "nodejs"
    packages:
      - "requests==2.32.3"

artifacts:
  - name: "ripgrep"
    version: "14.1.0"
    sha256: "def456..."
    source: "marketplace://tools/ripgrep"
  - name: "pdf-parser-bundle"
    version: "0.3.2"
    sha256: "987654..."
    source: "marketplace://skills/pdf-parser-bundle"
```

## 3. Dynamic Skill Metadata (`SKILL.md` Frontmatter)

When an Agent autonomously generates a new skill in the `skills/` directory, it MUST provide strict YAML frontmatter defining execution constraints for the Sandbox.

```yaml
---
name: "pdf_table_extractor"
description: "Extracts tables from PDFs into structured JSON using Python."
version: "1.0.0"

# Schema enforced by the Gateway SDK before execution
input_schema:
  type: "object"
  required: ["pdf_path"]
  properties:
    pdf_path:
      type: "string"
      description: "Absolute path to the PDF inside the sandbox."

output_schema:
  type: "object"
  required: ["tables_extracted"]
  properties:
    tables_extracted:
      type: "integer"
    json_path:
      type: "string"

# Physical constraints applied to the bwrap/Docker sandbox
resource_limits:
  max_memory_mb: 256
  timeout_seconds: 30
  net_access: false

declared_effects:
  memory_read: ["self.state.*"]
  memory_write: ["self.state.output.*"]
  net_connect: ["api.ocr.com"]
  secrets_get: []
  message_send: ["agent_research_*"]
  agent_spawn: false

artifact_dependencies:
  - name: "ocr-engine"
    version: "2.4.1"
    sha256: "deadbeef..."
    source: "marketplace://tools/ocr-engine"
---
```

## 4. Artifact Handle

Large files, binaries, datasets, and shared outputs are represented as immutable content-addressed artifact handles.

```json
{
  "artifact_id": "artifact_01jabc...",
  "sha256": "7d865e959b2466918c9863a465f1...",
  "kind": "dataset",                      // binary, skill_bundle, dataset, gateway_runtime, report
  "owner_id": "agent_research_alpha",
  "visibility": "shared",                 // private, shared, capsule
  "size_bytes": 104857600,
  "mime_type": "application/parquet",
  "created_at": "2026-03-05T10:12:00Z",
  "summary": "Competitor dataset extracted from Q1 filings",
  "source_runtime": {
    "gateway_version": "0.1.0",
    "skill_name": "pdf_table_extractor"
  }
}
```

## 5. Cognitive Capsule Manifest (`capsule.json`)

A Cognitive Capsule packages an Agent bundle together with its runtime closure for portable relaunch.

```json
{
  "capsule_id": "capsule_01jdef...",
  "agent_id": "agent_research_alpha",
  "mode": "hermetic",                     // thin or hermetic
  "created_at": "2026-03-05T10:20:00Z",
  "entrypoint": "SKILL.md",
  "runtime_lock": "runtime.lock",
  "included_artifacts": [
    {"artifact_id": "artifact_01jabc...", "sha256": "7d865e959b2466918c9863a465f1..."}
  ],
  "gateway_runtime": {
    "artifact": "marketplace://gateway/ccos-gateway",
    "version": "0.1.0",
    "sha256": "abc123..."
  },
  "redactions": ["secrets", "channel_sessions"]
}
```

## 6. Causal Chain Log Entry (`.jsonl`)

The fundamental unit of observability in Autonoetic. Every API call, message, and SDK action is appended to the immutable Causal Chain.

```json
{
  "timestamp": "2026-03-05T10:15:30.123Z",
  "log_id": "uuid-v4-string",
  "actor_id": "agent_alpha_75g",       // Agent, Gateway, or Auditor
  "category": "sandbox_execution",     // routing, sdk, tool_invoke, lifecycle
  "action": "sdk.secret.get",
  "target": "GITHUB_API_TOKEN",
  "status": "DENIED",                  // SUCCESS, DENIED, ERROR
  "reason": "policy/strict_auth_required",
  "payload": {
    "attempted_key": "GITHUB_API_TOKEN"
  },
  "prev_hash": "a3f8c2b7e1...hash...9x1z" // Hash-chain linkage
}
```

## 7. OFP WireMessage (Federation & IPC)

The base envelope used for all TCP Gateway-to-Gateway Federation, and internal Unix Socket communication where applicable.

```json
{
  "id": "req-12345",                   // JSON-RPC style incremental or UUID
  "sender": "node_nyc_gateway_1",      // Peer ID
  "signature": "hmac_sha256_hex_str",  // Per-message integrity (if extension negotiated)
  "seq_num": 42,                       // Replay prevention
  
  // The JSON-RPC 2.0 Payload
  "payload": {
    "jsonrpc": "2.0",
    "method": "federate.route_message",
    "params": {
      "target_agent": "global_auditor",
      "envelope": {
        // ... The Agent <-> Gateway envelope defined in protocols.md
      }
    }
  }
}
```

## 8. Tier 2 Memory Object (Gateway Substrate)

When an agent uses `sdk.memory.remember()` or `recall()`, the Gateway persists this internal schema in its database (SQLite/KV).

```json
{
  "key": "user_preferences.format",
  "value": "JSON Lines",                 // Text payload visible to Agent
  "owner": "agent_research_alpha",       // Who created it
  "visibility": "global",                // global vs private
  "created_at": "2026-03-05T10:10:00Z",
  "updated_at": "2026-03-05T10:15:00Z",
  "embedding": [0.12, -0.04, 0.88, ...]  // Vector representation for semantic search
}
```

## 9. Task Board Entry

When an agent posts to the shared `.tasks` queue natively.

```json
{
  "task_id": "task_99b",
  "creator_id": "agent_research_alpha",
  "title": "Parse competitor PDF",
  "description": "Extract tables from report.pdf using OCR",
  "status": "pending",                   // pending, claimed, completed, failed
  "assignee_id": null,                   // Populated when claimed
  "created_at": "2026-03-05T10:10:00Z",
  "capabilities_required": ["ToolInvoke(pdf_ocr)", "NetConnect(api.ocr.com)"],
  "result": null
}
```
