# Skill Interpreter and Capability Mapping Specification

## Overview

This specification defines how CCOS agents dynamically load, interpret, and execute **skills** from external sources (e.g., `skill.md` files). The goal is to enable autonomous capability discovery while maintaining CCOS's security guarantees: all execution flows through governed capabilities, never raw shell commands.

## Implementation Status (as of 2026-02-04)

This document mixes current behavior and a longer-term target architecture. Current implementation highlights:

- `ccos.skill.load` has URL guardrails: it rejects non-skill-looking URLs by default; use `force=true` to override. This prevents accidental attempts to “load a skill” from arbitrary links (notably `x.com/...` / `twitter.com/...`).
- The agent LLM prompt is tightened to avoid interpreting arbitrary user-provided URLs as skill definitions.
- Skill execution is mediated through governed capabilities and the capability marketplace, with secrets injected via `SecretStore` (`.ccos/secrets.toml`) rather than returned to the agent.

Not yet implemented end-to-end (still aspirational in this spec):

- Automatic mapping of arbitrary shell toolchains (`ffmpeg`, `sed`, complex pipelines) into a polyglot sandbox.
- OpenAPI ingestion and rich schema synthesis beyond what the skill definition provides.
- Robust unload/delta synchronization semantics for tool registries.

## Problem Statement

When an agent fetches a skill definition (e.g., from `https://moltbook.com/skill.md`), the skill often describes operations in terms of:
- Shell commands (`curl`, `jq`, `python script.py`)
- API calls (REST endpoints with auth)
- Data transformations (parse JSON, extract fields)

Without a mapping layer, agents:
1. Cannot execute these descriptions (CCOS has no raw shell)
2. Fall back to suggesting commands to the user (defeats autonomy)
3. Lose CCOS's security model (governance, PII protection, budgets)

## Solution: Three-Layer Skill Execution

```
┌─────────────────────────────────────────────────────────────────┐
│                        Skill Definition                          │
│   (skill.md/skill.yaml from external source)                     │
└────────────────────────────┬────────────────────────────────────┘
                             │
                             ▼
┌─────────────────────────────────────────────────────────────────┐
│              Layer 1: Skill Parser                               │
│   - Parse skill.md/yaml/json                                     │
│   - Extract operations, endpoints, auth requirements             │
│   - Identify tool/command references                             │
└────────────────────────────┬────────────────────────────────────┘
                             │
                             ▼
┌─────────────────────────────────────────────────────────────────┐
│              Layer 2: Capability Mapper                          │
│   - Map known primitives → CCOS capabilities                     │
│   - Route unknown tools → Polyglot Sandbox (WS9)                 │
│   - Generate capability manifests with effects/schemas           │
└────────────────────────────┬────────────────────────────────────┘
                             │
                             ▼
┌─────────────────────────────────────────────────────────────────┐
│              Layer 3: Governed Execution                         │
│   - Execute via CapabilityMarketplace                            │
│   - Enforce budgets, governance, PII protection                  │
│   - Record in causal chain                                       │
└─────────────────────────────────────────────────────────────────┘
```

## Layer 1: Skill Parser

### Supported Skill Formats

| Format | Extension | Structure |
|--------|-----------|-----------|
| Markdown | `.md` | Code blocks with annotations |
| YAML | `.yaml`, `.yml` | Structured skill definition |
| JSON | `.json` | Machine-readable definition |
| OpenAPI | `.json`, `.yaml` | API specification |

### Skill Schema (YAML canonical form)

```yaml
skill:
  name: "moltbook"
  version: "1.0.0"
  description: "Interact with Moltbook notebooks"
  
  # Required data classifications (for chat gateway integration)
  data_classifications:
    - pii.user_query  # May contain user questions
    
  operations:
    - name: "search"
      description: "Search notebooks by query"
      method: "POST"
      endpoint: "https://api.moltbook.com/v1/search"
      auth:
        type: "bearer"
        env_var: "MOLTBOOK_API_KEY"
      input:
        query: { type: "string", required: true }
        limit: { type: "integer", default: 10 }
      output:
        results: { type: "array", items: { type: "object" } }
      
    - name: "create_notebook"
      description: "Create a new notebook"
      method: "POST"
      endpoint: "https://api.moltbook.com/v1/notebooks"
      # ... auth, input, output
      
    - name: "run_analysis"
      description: "Run Python analysis on notebook data"
      runtime: "python:3.11"
      script: |
        import pandas as pd
        # ... analysis code
      sandbox:
        network: { egress: ["api.moltbook.com"] }
        filesystem: { read: ["/input"], write: ["/output"] }
```

### Parsing from Markdown

When a skill is described in natural language markdown:

```markdown
# Moltbook Skill

## Search Notebooks
Use curl to search:
```bash
curl -X POST https://api.moltbook.com/v1/search \
  -H "Authorization: Bearer $MOLTBOOK_API_KEY" \
  -d '{"query": "machine learning", "limit": 10}'
```

The parser extracts:
- **Operation**: search
- **Method**: POST
- **Endpoint**: `https://api.moltbook.com/v1/search`
- **Auth**: Bearer token from `MOLTBOOK_API_KEY`
- **Input schema**: `{ query: string, limit: integer }`

## Layer 2: Capability Mapper

### Known Primitive Mappings

The mapper maintains a registry of common tool patterns → CCOS capabilities:

| Pattern | Detection | CCOS Capability | Notes |
|---------|-----------|-----------------|-------|
| `curl -X GET/POST ...` | URL + method extraction | `ccos.network.http-fetch` | Headers/body mapped |
| `curl ... \| jq .foo` | Pipeline with jq | `ccos.network.http-fetch` + `ccos.json.parse` | Chained execution |
| `python script.py` | Python runtime | `ccos.sandbox.python` (WS9) | Sandboxed execution |
| `node script.js` | Node runtime | `ccos.sandbox.node` (WS9) | Sandboxed execution |
| `echo`, `printf` | Output | `ccos.io.println` | Safe output |
| `cat file` | File read | `ccos.io.read-file` | Governed file access |
| `jq .path` | JSON extraction | RTFS `get-in` | Pure transformation |

### Mapping Algorithm

```
function map_operation(op):
    # 1. Check for direct API endpoint
    if op.endpoint:
        return create_http_capability(op)
    
    # 2. Check for known tool patterns
    if op.command matches /^curl/:
        return parse_curl_to_http(op.command)
    
    if op.command matches /^python|^node|^go run/:
        return create_sandbox_capability(op)
    
    # 3. Check for data transformation patterns
    if op.command matches /^jq|^sed|^awk/:
        return create_transform_capability(op)
    
    # 4. Unknown command → sandboxed shell (requires approval)
    return create_shell_sandbox_capability(op)
```

### Generated Capability Manifest

For each operation, the mapper generates a capability manifest:

```clojure
(capability "moltbook.search"
  :name "Moltbook Search"
  :description "Search notebooks by query"
  :version "1.0.0"
  
  ;; Maps to existing CCOS capability
  :implementation {:type :delegate
                   :capability "ccos.network.http-fetch"
                   :transform (fn [input]
                     {:url "https://api.moltbook.com/v1/search"
                      :method "POST"
                      :headers {:Authorization (str "Bearer " (secret "MOLTBOOK_API_KEY"))}
                      :body (json-stringify input)})}
  
  ;; Schema from skill definition
  :input-schema {:query :string :limit [:optional :int]}
  :output-schema {:results [:vector :map]}
  
  ;; Effects and governance
  :effects [:network-egress]
  :requires-secret "MOLTBOOK_API_KEY"
  :trust-tier :pending  ;; Needs approval before first use
  
  ;; Resource estimates
  :estimated-resources {:network-bytes 10000
                        :wall-clock-ms 2000})
```

### Unknown Tools → Sandbox Routing

When a skill references a tool CCOS doesn't know:

```yaml
operations:
  - name: "convert_video"
    command: "ffmpeg -i input.mp4 -c:v libx264 output.mp4"
```

The mapper creates a sandboxed capability:

```clojure
(capability "skill.convert_video"
  :name "Convert Video (Sandboxed)"
  :description "FFmpeg video conversion - REQUIRES SANDBOX APPROVAL"
  
  :implementation {:type :sandbox
                   :runtime "container:ubuntu:22.04"
                   :packages ["ffmpeg"]
                   :command ["ffmpeg" "-i" "input.mp4" "-c:v" "libx264" "output.mp4"]}
  
  ;; Strict sandbox policy
  :sandbox-policy {:network {:egress :none}
                   :filesystem {:read ["/input"] :write ["/output"]}
                   :resources {:cpu-seconds 60 :memory-mb 512}}
  
  :trust-tier :pending
  :requires-approval true
  :approval-reason "Unknown tool 'ffmpeg' requires sandboxed execution")
```

## Layer 3: Governed Execution

All skill-derived capabilities execute through the standard CCOS pipeline:

### Execution Flow

```
1. Agent calls: (call "moltbook.search" {:query "foo"})

2. CapabilityMarketplace.execute_capability("moltbook.search", input)
   │
   ├─► Check capability exists (registered from skill)
   ├─► Check approval status (pending → require approval first)
   ├─► Check resource budgets
   ├─► Check secret availability
   │
   └─► Execute via delegate capability (ccos.network.http-fetch)
       │
       ├─► Apply input transform
       ├─► Inject secret (never exposed to agent)
       ├─► Make HTTP request
       ├─► Apply output transform
       │
       └─► Return result to agent
       
3. Record in causal chain:
   {:event :capability-executed
    :capability "moltbook.search"
    :derived-from "https://moltbook.com/skill.md"
    :delegated-to "ccos.network.http-fetch"
    :resources-consumed {...}}
```

### Approval Requirements

| Scenario | Requires Approval |
|----------|-------------------|
| HTTP to known safe domain | No (if `ccos.network.http-fetch` already approved) |
| HTTP with new secret | Yes (secret insertion approval) |
| Sandboxed Python/Node | Yes (sandbox execution approval) |
| Unknown tool in sandbox | Yes (tool + sandbox approval) |
| Any PII-touching operation | Yes (per chat gateway policy) |

## New Capabilities

### `ccos.skill.load`

Load a skill from URL and register its capabilities:

```clojure
(call "ccos.skill.load" {:url "https://moltbook.com/skill.md"})

;; Returns:
{:skill-id "moltbook"
 :version "1.0.0"
 :capabilities [{:id "moltbook.search"
                 :maps-to "ccos.network.http-fetch"
                 :status :pending
                 :requires ["secret:MOLTBOOK_API_KEY"]}
                {:id "moltbook.create_notebook"
                 :maps-to "ccos.network.http-fetch"
                 :status :pending}
                {:id "moltbook.run_analysis"
                 :maps-to "ccos.sandbox.python"
                 :status :pending
                 :requires-approval true}]
 :pending-approvals 3
 :hint "Use ccos.approval.request to approve these capabilities"}
```

### `ccos.skill.execute`

Execute a skill operation (alternative to direct capability call):

```clojure
(call "ccos.skill.execute" 
  {:skill "moltbook"
   :operation "search"
   :params {:query "machine learning" :limit 5}})
```

### `ccos.primitive.map`

Map a command description to CCOS capability (for introspection):

```clojure
(call "ccos.primitive.map" {:command "curl -X POST https://api.example.com"})

;; Returns:
{:maps-to "ccos.network.http-fetch"
 :params {:url "https://api.example.com" :method "POST"}
 :confidence 0.95}
```

## Security Considerations

### Principle: Defense in Depth

1. **Parse-time validation**: Reject malformed skills, validate URLs
2. **Map-time governance**: Unknown tools → mandatory sandbox
3. **Execute-time enforcement**: Budgets, approvals, PII checks
4. **Audit-time traceability**: Full causal chain with skill provenance

### Threat Mitigations

| Threat | Mitigation |
|--------|------------|
| Malicious skill.md with shell injection | No raw shell; all commands mapped to capabilities |
| Skill exfiltrating secrets | Secrets never returned to agent; injected at execution |
| Skill accessing arbitrary files | Sandbox filesystem policy; no raw file access |
| Skill making unauthorized network calls | Network proxy through GK with allowlist |
| Skill consuming excessive resources | Budget enforcement at execution layer |

### Trust Tiers for Skill-Derived Capabilities

```
┌─────────────────────────────────────────────────────────────────┐
│  Tier 0: Core CCOS (inherently trusted)                          │
│  - ccos.network.http-fetch, ccos.json.parse, ccos.io.*           │
└─────────────────────────────────────────────────────────────────┘
                             │
                             ▼
┌─────────────────────────────────────────────────────────────────┐
│  Tier 1: Skill-Delegated (inherits trust of delegate)           │
│  - moltbook.search → delegates to ccos.network.http-fetch       │
│  - No new trust required if delegate already approved           │
└─────────────────────────────────────────────────────────────────┘
                             │
                             ▼
┌─────────────────────────────────────────────────────────────────┐
│  Tier 2: Skill-Sandboxed (requires explicit approval)           │
│  - moltbook.run_analysis → runs in Python sandbox               │
│  - Approval shows sandbox policy, resource limits               │
└─────────────────────────────────────────────────────────────────┘
                             │
                             ▼
┌─────────────────────────────────────────────────────────────────┐
│  Tier 3: Unknown Tool Sandbox (high-friction approval)          │
│  - skill.convert_video → ffmpeg in container                    │
│  - Warning: "Unknown tool requires full sandbox"                │
└─────────────────────────────────────────────────────────────────┘
```

## Integration with Existing Systems

### Relationship to WS9 (Polyglot Sandboxed Capabilities)

The Skill Interpreter **uses** WS9 infrastructure:
- Unknown Python/JS/Go scripts → routed to WS9 sandbox manager
- Sandbox policy derived from skill definition
- GK network proxy enforces egress allowlists

### Relationship to WS10 (Skills Layer)

This spec **extends** WS10's skill YAML schema:
- WS10 defines static skill → capability mappings
- This spec adds **dynamic loading** from URLs
- This spec adds **command pattern recognition** for natural-language skills

### Relationship to Chat Gateway

Skills loaded through chat mode inherit chat gateway policies:
- PII classification from skill's `data_classifications`
- Transform-only access to quarantine data
- Egress gating for PII-derived outputs

## Implementation Phases

### Phase 1: Core Parser + HTTP Mapping
- Parse skill.md/yaml to internal schema
- Map `curl` commands → `ccos.network.http-fetch`
- Register capabilities in marketplace
- Basic approval flow

### Phase 2: Sandbox Routing
- Integrate with WS9 sandbox manager
- Route Python/Node scripts to sandboxes
- Unknown command detection → sandbox fallback

### Phase 3: Dynamic Learning
- Learn common patterns from usage
- Suggest capability mappings for unknown tools
- Build community skill registry

## Success Criteria

1. Agent can `(call "ccos.skill.load" {:url "..."})` and get usable capabilities
2. All skill execution flows through CCOS governance
3. No raw shell commands ever executed
4. Unknown tools automatically sandboxed with approval required
5. Full audit trail links execution back to skill provenance

---

## Implementation Status (January 2026)

- [x] Skill parsing supports YAML, Markdown (with `###` naming), and JSON.
- [x] `data_classifications` list is accepted.
- [x] Capabilities `ccos.skill.load`, `ccos.skill.execute`, `ccos.primitive.map` registered.
- [x] Primitive mapping (`curl` -> `http-fetch`) and Secret Injection (`Authorization` header) implemented and verified.
- [x] Unknown commands routed to sandboxed capability (but see *Shadow Execution Risk* below).
- [x] Approval enforcement and Secret Injection working end-to-end.

### Critical Finding: Shadow Execution Risk
The Moltbook onboarding demo revealed that an agent running outside a sandbox (e.g., in an IDE) can bypass the Skill Interpreter entirely by executing direct shell commands (`curl`).
*   **Fix**: Integration with WS9 (Jailed Agent Runtime) is mandatory to force usage of this interpreter.

### Not Yet Implemented
- Rich pipeline chaining beyond basic pipes.
- Automatic creation of sandbox policies from skill definitions.
- Agent planning loop integration.
