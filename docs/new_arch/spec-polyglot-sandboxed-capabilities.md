# CCOS Polyglot Sandboxed Capabilities Specification (Draft)

**Status**: Draft  
**Related**: [CCOS Secure Chat Gateway Roadmap](./ccos-secure-chat-gateway-roadmap.md), [Sandbox Isolation Spec](../ccos/specs/022-sandbox-isolation.md)

## 1. Executive Summary

This specification defines how CCOS can execute capabilities written in **any programming language** (Python, JavaScript, Go, Rust, etc.) while maintaining the security guarantees of the RTFS host boundary model. The key insight is that **RTFS remains the governance language**, but capability *implementations* can run in isolated sandboxes with all effects proxied through the Governance Kernel (GK).

Additionally, this spec introduces **Skills** as a natural-language teaching layer that maps to governed capabilities, enabling easy authoring without sacrificing security.

## 2. Motivation

### 2.1 The Moltbot Pattern (Unsafe)

Systems like Moltbot use "Claude Skills" - markdown files that teach an LLM how to execute tools:

```yaml
---
name: local-places
description: Search for places via Google Places API proxy
metadata: {"requires": {"bins": ["uv"], "env": ["GOOGLE_PLACES_API_KEY"]}}
---
# Setup
cd {baseDir} && uv run uvicorn local_places.main:app --port 8000
# Usage
curl -X POST http://127.0.0.1:8000/places/search -d '{"query": "coffee"}'
```

**Security problems**:
- Python server runs with **full host access**
- No filesystem isolation (can read `/etc/passwd`, `~/.ssh/`)
- No network restrictions (can exfiltrate data anywhere)
- Secrets visible in environment to all processes
- No governance, audit, or approval flow
- LLM generates raw shell commands with no validation

### 2.2 The CCOS Goal

Enable the same capability (Python API server) to run with:
- **Isolated filesystem** (virtual FS per capability)
- **Proxied network** (all requests through GK, allowlisted hosts only)
- **Scoped secrets** (injected per-capability, not global env)
- **Governed lifecycle** (approval, budgets, audit trail)
- **Schema validation** (typed inputs/outputs)

## 3. Architecture Overview

```
┌─────────────────────────────────────────────────────────────────────────────┐
│                              CCOS Host                                       │
│  ┌─────────────────────────────────────────────────────────────────────────┐│
│  │                     Governance Kernel (GK)                              ││
│  │  • Effect routing     • Secret injection    • Schema validation        ││
│  │  • Approval checks    • Audit logging       • Budget enforcement       ││
│  └─────────────────────────────────────────────────────────────────────────┘│
│         │                        │                        │                  │
│         ▼                        ▼                        ▼                  │
│  ┌─────────────┐          ┌─────────────┐          ┌─────────────┐          │
│  │  Sandbox 1  │          │  Sandbox 2  │          │  Sandbox 3  │          │
│  │  Python     │          │  Node.js    │          │  RTFS Pure  │          │
│  │             │          │             │          │             │          │
│  │ Virtual FS  │          │ Virtual FS  │          │ (no sandbox │          │
│  │ Net: proxy  │          │ Net: proxy  │          │  needed)    │          │
│  │ CPU/Mem cap │          │ CPU/Mem cap │          │             │          │
│  └─────────────┘          └─────────────┘          └─────────────┘          │
└─────────────────────────────────────────────────────────────────────────────┘
```

### 3.1 Core Principle

> **All effects cross the sandbox boundary through the GK.**

A sandboxed capability cannot:
- Access host filesystem directly
- Make network requests directly
- Read secrets from host environment
- Spawn unrestricted processes

Instead, it must:
- Request file access from GK (virtual FS mount)
- Route network through GK proxy (allowlist enforced)
- Receive secrets via GK injection (scoped, audited)
- Run within resource budgets (CPU, memory, time)

## 4. Capability Manifest Schema

### 4.1 Polyglot Capability Definition

```clojure
(capability "places.search"
  :description "Search for nearby places using Google Places API"
  :version "1.0.0"
  
  ;; Schema (same as pure RTFS capabilities)
  :input-schema [:map
    [:query :string]
    [:location [:map [:lat :float] [:lng :float]]]
    [:filters {:optional true} [:map
      [:open_now {:optional true} :bool]
      [:min_rating {:optional true} [:and :float [:>= 0] [:<= 5]]]
      [:types {:optional true} [:vector :string]]]]]
  
  :output-schema [:map
    [:results [:vector [:map
      [:place_id :string]
      [:name :string]
      [:address :string]
      [:rating {:optional true} :float]
      [:open_now {:optional true} :bool]]]]
    [:next_page_token {:optional true} :string]]
  
  ;; Effects (same as pure RTFS capabilities)
  :effects [:network]
  
  ;; Runtime specification (NEW - polyglot)
  :runtime {
    :type :microvm                        ; or :wasm, :container, :native
    :image "python:3.12-slim"
    :entrypoint ["uvicorn" "local_places.main:app" "--host" "0.0.0.0" "--port" "8000"]
    :port 8000
    :startup-timeout-ms 5000
    :health-check "/ping"
  }
  
  ;; Filesystem policy
  :filesystem {
    :mode :ephemeral                      ; or :persistent
    :mounts [
      {:host "capabilities/places/src"    ; relative to workspace
       :guest "/app"
       :mode :ro}                         ; read-only
    ]
    :quota-mb 100                         ; max writable space
  }
  
  ;; Network policy  
  :network {
    :mode :proxy                          ; all via GK proxy
    :allowed-hosts [
      "places.googleapis.com"
      "maps.googleapis.com"
    ]
    :allowed-ports [443]
    :egress-rate-limit 100                ; requests per minute
  }
  
  ;; Secrets (injected by GK, not in env)
  :secrets [:GOOGLE_PLACES_API_KEY]
  
  ;; Resource limits
  :resources {
    :cpu-shares 256                       ; relative CPU allocation
    :memory-mb 512                        ; max memory
    :timeout-ms 30000                     ; max execution time per call
  }
  
  ;; Approval tier
  :approval {
    :tier :standard                       ; :preapproved, :standard, :elevated
    :requires-per-use false               ; true = ask every time
  }
)
```

**Interim metadata wiring (WS9 Phase 2)**
- `sandbox_filesystem`: JSON-encoded `VirtualFilesystem`
- `sandbox_resources`: JSON-encoded `ResourceLimits`
- `sandbox_runtime`: JSON-encoded runtime spec (for future manifest parsing)

These metadata keys are used until the manifest `:filesystem` and `:resources` fields
are parsed and enforced directly.

### 4.2 Runtime Types

| Type | Isolation | Startup | Use Case |
|------|-----------|---------|----------|
| `:rtfs` | Language-level | <1ms | Pure computation, data transforms |
| `:wasm` | Memory-safe sandbox | ~10ms | Hot-path, trusted code, portable |
| `:container` | Linux namespaces | ~100ms | Lightweight isolation, Docker-compatible |
| `:microvm` | Hardware-level (KVM) | ~125ms | Untrusted code, strong isolation |
| `:native` | None (host process) | <1ms | Trusted system capabilities only |

### 4.3 Filesystem Modes

| Mode | Behavior | Use Case |
|------|----------|----------|
| `:ephemeral` | Destroyed after each invocation | Stateless APIs |
| `:session` | Persists within session, destroyed after | Multi-step workflows |
| `:persistent` | Persists across invocations (with quota) | Databases, caches |

## 5. Execution Flow

### 5.1 Capability Invocation

```
Agent: (call "places.search" {:query "coffee" :location {:lat 51.5 :lng -0.1}})

┌──────────────────────────────────────────────────────────────────┐
│ 1. RTFS Runtime receives call                                    │
│    → Resolves "places.search" capability                         │
│    → Detects :runtime {:type :microvm}                           │
└──────────────────────────────────────────────────────────────────┘
                              │
                              ▼
┌──────────────────────────────────────────────────────────────────┐
│ 2. Governance Kernel (GK) pre-checks                             │
│    → Validate input against :input-schema                        │
│    → Check :effects [:network] approved for this session         │
│    → Check secret GOOGLE_PLACES_API_KEY is available             │
│    → Check resource budgets not exhausted                        │
└──────────────────────────────────────────────────────────────────┘
                              │
                              ▼
┌──────────────────────────────────────────────────────────────────┐
│ 3. Sandbox Manager                                               │
│    → Look for warm sandbox (reuse if available)                  │
│    → Or spin up new microVM with:                                │
│      • Image: python:3.12-slim                                   │
│      • Virtual FS with /app mounted read-only                    │
│      • Network namespace with proxy to GK                        │
│      • Secret injected as /run/secrets/GOOGLE_PLACES_API_KEY     │
│    → Wait for health check (/ping returns 200)                   │
└──────────────────────────────────────────────────────────────────┘
                              │
                              ▼
┌──────────────────────────────────────────────────────────────────┐
│ 4. Request Forwarding                                            │
│    → GK proxies request to sandbox:8000                          │
│    → POST /places/search with JSON body                          │
│    → Timer starts for :timeout-ms                                │
└──────────────────────────────────────────────────────────────────┘
                              │
                              ▼
┌──────────────────────────────────────────────────────────────────┐
│ 5. Sandbox Execution                                             │
│    → Python code handles request                                 │
│    → Tries to call places.googleapis.com                         │
│    → Request intercepted by network proxy                        │
│    → GK checks: host in :allowed-hosts? ✓                        │
│    → GK injects API key header, forwards request                 │
│    → Response returns through proxy                              │
└──────────────────────────────────────────────────────────────────┘
                              │
                              ▼
┌──────────────────────────────────────────────────────────────────┐
│ 6. Response Handling                                             │
│    → GK receives response from sandbox                           │
│    → Validate against :output-schema                             │
│    → Log to causal chain (capability, inputs, outputs, timing)   │
│    → Return to RTFS runtime                                      │
└──────────────────────────────────────────────────────────────────┘
                              │
                              ▼
┌──────────────────────────────────────────────────────────────────┐
│ 7. Sandbox Lifecycle                                             │
│    → If :filesystem :ephemeral → destroy sandbox                 │
│    → If warm pool enabled → return to pool (with TTL)            │
│    → If timeout/error → force-kill, log failure                  │
└──────────────────────────────────────────────────────────────────┘
```

### 5.2 Network Proxy Behavior

All outbound network requests from sandboxes route through the GK proxy:

```
Sandbox → GK Proxy → Internet
              │
              ├─ Check: is host in :allowed-hosts?
              │   └─ No → REJECT, log denial
              │
              ├─ Check: is port in :allowed-ports?
              │   └─ No → REJECT, log denial
              │
              ├─ Check: under :egress-rate-limit?
              │   └─ No → REJECT, log rate limit
              │
              ├─ Inject auth if secret mapping exists
              │   └─ Add header: Authorization: Bearer {secret}
              │
              └─ Forward request, log to causal chain
```

### 5.3 Secret Injection

Secrets are **never** exposed as environment variables. Instead:

1. GK reads secret from secure store
2. GK creates a tmpfs mount at `/run/secrets/`
3. Each secret is a file: `/run/secrets/GOOGLE_PLACES_API_KEY`
4. Capability code reads from file (or GK injects into headers)
5. On sandbox teardown, tmpfs is destroyed

```python
# Capability code reads secrets from mounted file
def get_api_key():
    with open("/run/secrets/GOOGLE_PLACES_API_KEY") as f:
        return f.read().strip()
```

## 6. Skills Layer

### 6.1 Skill Definition

Skills are **natural language teaching documents** that reference governed capabilities:

```yaml
---
name: local-places
description: Search for places (restaurants, cafes, etc.) nearby
version: "1.0.0"

# CCOS governance metadata
ccos:
  # Skills reference capabilities, not raw binaries
  capabilities:
    - places.search          # Required capability
    - places.resolve         # Required capability
    - places.details         # Optional capability
  
  # Inherited from capabilities (for display)
  effects: [network]
  secrets: [GOOGLE_PLACES_API_KEY]
  
  # Data classification (for chat gateway)
  data_classes:
    input: [public]          # User queries are not PII
    output: [public]         # Place data is public
  
  # Approval tier for the skill
  approval:
    tier: standard
    requires_per_use: false

# Display metadata
display:
  emoji: "📍"
  category: "Location"
---

# 📍 Local Places

Search for nearby places like restaurants, cafes, and shops.

## Usage

1. **Resolve a location** (if user gives vague location):
   Use `places.resolve` with the location text.
   
2. **Search for places**:
   Use `places.search` with query and location coordinates.
   
3. **Get details**:
   Use `places.details` with a place_id.

## Conversation Flow

1. If user says "near me" or gives vague location → call `places.resolve` first
2. If multiple location results → show numbered list, ask user to pick
3. Ask for preferences: type, open now, rating, price level
4. Call `places.search` with filters
5. Present results with name, rating, address, open status
6. Offer to fetch details or refine search

## Example

User: "Find coffee shops near Soho, London"

1. Call `places.resolve` with `{"location_text": "Soho, London"}`
2. Get coordinates: `{lat: 51.5137, lng: -0.1366}`
3. Call `places.search` with:
   ```json
   {
     "query": "coffee shop",
     "location": {"lat": 51.5137, "lng": -0.1366},
     "filters": {"open_now": true}
   }
   ```
4. Present top 5 results with ratings
```

### 6.2 Skill ↔ Capability Mapping

```
┌─────────────────────────────────────────────────────────────────┐
│                    Skill (Natural Language)                     │
│  "Find coffee shops near Soho, London"                         │
└───────────────────────────┬─────────────────────────────────────┘
                            │ LLM interprets skill instructions
                            │ Generates structured intent
                            ▼
┌─────────────────────────────────────────────────────────────────┐
│                    Capability Selection                         │
│  Skill declares: capabilities: [places.search, places.resolve] │
│  LLM chooses: places.resolve, then places.search               │
└───────────────────────────┬─────────────────────────────────────┘
                            │ Call routed to CCOS
                            ▼
┌─────────────────────────────────────────────────────────────────┐
│              RTFS Runtime + Governance Kernel                   │
│  (call "places.search" {:query "coffee" :location {...}})      │
│  Schema validated, effects approved, sandbox executed           │
└─────────────────────────────────────────────────────────────────┘
```

### 6.3 Skill Tiers

| Tier | Capabilities | Effects | Use Case |
|------|-------------|---------|----------|
| **Informational** | None (empty list) | None | Pure teaching, explanations |
| **Read-only** | Only `:effects []` capabilities | None | Safe queries, lookups |
| **Standard** | Any approved capabilities | Declared | Most skills |
| **Elevated** | Admin capabilities | `:state`, `:fs` | System management |

### 6.4 Skill Bundles

Skills can be packaged as signed bundles:

```
local-places-skill/
├── skill.yaml              # Skill definition
├── capabilities/
│   ├── places.search.rtfs  # Capability manifest
│   └── places.resolve.rtfs
├── src/                    # Implementation code
│   └── local_places/
│       └── main.py
├── signature.json          # Bundle signature
└── metadata.json           # Version, author, trust tier
```

Bundle verification on install:
1. Check signature against known publishers
2. Verify capability manifests match bundle claims
3. Review effects and approval requirements
4. User approves installation
5. Capabilities registered with marketplace

## 7. Security Guarantees

### 7.1 What Sandboxes Prevent

| Attack Vector | Mitigation |
|---------------|------------|
| **Read host files** | Virtual FS - only mounted paths visible |
| **Write to host** | Ephemeral FS or quota-limited persistent |
| **Network exfiltration** | Proxy with allowlist, no direct access |
| **Steal other secrets** | Per-capability secret injection only |
| **DoS via resources** | CPU/memory/time limits enforced |
| **Escape via kernel exploit** | MicroVM uses separate kernel (Firecracker) |
| **Persist malware** | Ephemeral FS destroyed after use |
| **Privilege escalation** | No root in sandbox, capabilities dropped |

### 7.2 Trust Hierarchy

```
┌─────────────────────────────────────────────────────────────┐
│ TRUST LEVEL 1: RTFS Pure                                    │
│ • No sandbox needed                                         │
│ • Deterministic, no side effects                            │
│ • Can be replayed/verified                                  │
└─────────────────────────────────────────────────────────────┘
                              │
                              ▼
┌─────────────────────────────────────────────────────────────┐
│ TRUST LEVEL 2: WASM Sandbox                                 │
│ • Memory-safe sandbox                                       │
│ • Fast startup, portable                                    │
│ • Limited syscalls (WASI)                                   │
└─────────────────────────────────────────────────────────────┘
                              │
                              ▼
┌─────────────────────────────────────────────────────────────┐
│ TRUST LEVEL 3: Container Sandbox                            │
│ • Namespace isolation                                       │
│ • Shared kernel with host                                   │
│ • Good for Docker-packaged tools                            │
└─────────────────────────────────────────────────────────────┘
                              │
                              ▼
┌─────────────────────────────────────────────────────────────┐
│ TRUST LEVEL 4: MicroVM Sandbox                              │
│ • Separate kernel (Firecracker/gVisor)                      │
│ • Hardware-level isolation                                  │
│ • For untrusted community code                              │
└─────────────────────────────────────────────────────────────┘
```

### 7.3 Data Classification Flow

For chat gateway integration, data classification propagates through capabilities:

```clojure
;; Capability declares it handles PII
(capability "chat.summarize"
  :data-classes {
    :input [:pii.chat.message]           ; CAN access PII
    :output [:redacted]                   ; Outputs are redacted
  }
  :egress [:none]                         ; CANNOT send data externally
  ...)

;; Calling this capability:
;; - GK allows access to quarantine store (input has PII)
;; - GK blocks any network egress (egress: none)
;; - Output is tagged as :redacted, can be returned to user
```

### 7.4 Resource Budget Integration

Sandboxed capabilities integrate with CCOS resource budget governance:

#### Per-Capability Budgets
Each capability manifest declares resource limits:

```clojure
:resources {
  :cpu-shares 256           ; relative CPU allocation
  :memory-mb 512            ; max memory
  :timeout-ms 30000         ; max execution time per call
  :network-bytes 1048576    ; max egress per call
}
```

#### Runtime Metering
The sandbox manager meters actual consumption:

| Resource | How Measured |
|----------|--------------|
| CPU time | cgroups `cpuacct.usage` |
| Memory | cgroups `memory.usage_in_bytes` |
| Wall-clock | Timer started at request forward |
| Network | GK proxy byte counters |

**Current implementation (WS9 Phase 2)**
- Network/storage usage is collected from sandbox execution metadata and surfaced as
  `usage.network_egress_bytes` and `usage.storage_write_bytes` in capability results.
- CPU/memory metering is captured in metadata but not yet enforced as run-level budget
  limits.

#### Budget Enforcement in Sandboxes
When a sandbox exceeds its per-call budget:

1. **Timeout**: Sandbox is force-killed, run continues with error
2. **Memory**: OOM killer triggered, capability fails
3. **Network**: Proxy rejects further requests

These per-call limits are independent of (and must not exceed) the run-level budget.

#### Run-Level Budget Aggregation
Each capability call's resource consumption is added to the run budget:

```clojure
;; After calling places.search:
{:run-budget-consumed {
   :steps 1
   :wall-clock-ms 1234
   :sandbox-cpu-ms 150
   :network-bytes 10240
 }
 :run-budget-remaining {
   :steps 49
   :wall-clock-ms 58766
   :sandbox-cpu-ms 29850
   :network-bytes 10475520
 }}
```

If the run budget would be exceeded by a capability's declared `:resources`, the call is **rejected before execution**.

#### Estimation for Capability Calls
Capabilities with predictable resource usage can declare estimates:

```clojure
:resource-estimates {
  :wall-clock-ms [500 2000]      ; [typical, worst-case]
  :network-bytes [5000 50000]
  :llm-tokens nil                ; N/A for non-LLM capability
}
```

The GK uses these estimates for:
- Pre-flight budget checks (reject if worst-case exceeds remaining)
- Scheduling decisions (warm pool sizing)
- Cost estimation for user approval

## 8. Implementation Roadmap

### Implementation Status (WS9)
- [x] GK network proxy with allowlist
- [x] Secret injection for sandboxed calls
- [x] Virtual FS mount wiring for MicroVM process provider
- [x] Resource limit wiring (timeout/memory/CPU) for MicroVM process provider
- [x] Usage reporting for network/storage in sandboxed results
- [ ] Manifest parsing for `:runtime`, `:filesystem`, and `:resources`

### Phase 0: Foundation
- [ ] Define capability manifest schema for `:runtime` field
- [ ] Implement sandbox manager interface (abstract over runtime types)
- [ ] Implement GK network proxy with allowlist

### Phase 1: Container Runtime
- [ ] Container sandbox using nsjail or bubblewrap
- [ ] Virtual FS mounting
- [ ] Secret injection via tmpfs
- [ ] Basic resource limits

### Phase 2: MicroVM Runtime  
- [ ] Firecracker integration
- [ ] Warm pool for fast startup
- [ ] Health check and lifecycle management
- [ ] GPU passthrough (optional)

### Phase 3: Skills Layer
- [ ] Skill YAML schema and parser
- [ ] Skill → capability mapping
- [ ] Skill bundle packaging and signing
- [ ] Skill marketplace integration

### Phase 4: WASM Runtime
- [ ] Wasmtime integration
- [ ] WASI filesystem mapping
- [ ] WASI network proxy
- [ ] Component model support

## 9. Open Questions

1. **Warm pool management**: How many sandboxes to keep warm? Per-capability or shared pool?

2. **Debugging**: How do developers debug failures inside sandboxes? Logging? Trace export?

3. **State sharing**: Can capabilities share state? (Probably no - explicit data passing only)

4. **GPU access**: ML capabilities need GPU. How to sandbox GPU access?

5. **Multi-language single capability**: Can one capability use multiple runtimes? (e.g., Python + Rust)

6. **Cost attribution**: How to attribute cloud costs to specific capabilities/sessions?

## 10. References

- [CCOS Sandbox Isolation Spec](../ccos/specs/022-sandbox-isolation.md)
- [CCOS Host Boundary Spec](../ccos/specs/004-rtfs-ccos-boundary.md)
- [CCOS Governance Architecture](../ccos/specs/005-security-and-context.md)
- [Firecracker MicroVMs](https://firecracker-microvm.github.io/)
- [gVisor Container Sandbox](https://gvisor.dev/)
- [Wasmtime Runtime](https://wasmtime.dev/)
