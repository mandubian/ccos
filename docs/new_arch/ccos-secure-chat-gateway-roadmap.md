# CCOS Secure Chat Gateway Roadmap (Draft)

## Context
Moltbot’s core product value is an **always-on Gateway** that connects chat channels to a **local agent runtime** with sessions, skills/tools, a dashboard, and node pairing (see [Moltbot docs](https://docs.molt.bot/)).

CCOS/RTFS already provides a stronger **secure compute core**:
- RTFS purity + explicit host boundary (`docs/ccos/specs/004-rtfs-ccos-boundary.md`)
- capability marketplace, manifests, schema validation, effects, discovery→pending→approval (`docs/ccos/specs/030-capability-system-architecture.md`)
- session model in interactive mode via MCP (`docs/ccos/specs/007-mcp-server-interactive-mode.md`, `docs/ccos/specs/036-mcp-server-architecture.md`)
- governance architecture (`docs/ccos/specs/005-security-and-context.md`, `docs/ccos/specs/035-two-tier-governance.md`)
- sandbox isolation primitives (`docs/ccos/specs/022-sandbox-isolation.md`)
- **polyglot sandboxed capabilities** (`docs/drafts/spec-polyglot-sandboxed-capabilities.md`) — execute Python, JS, Go, etc. in isolated microVMs/containers with GK-proxied effects

This roadmap focuses on what CCOS is missing to provide “chat-to-local-agent” services **without adopting Moltbot’s broad personal-data exposure model**.

## Guiding Principles (Non-Negotiable)
- **Default-deny personal data**: raw chat content and attachments are not automatically accessible to planning/tool calls.
- **Transform-first ingestion**: store raw data in a quarantined store; expose only narrow, schema-checked transformations (summaries, extracted entities) by default.
- **No silent escalation**: new effects/permissions/channels/secrets always require explicit approval and are recorded in the causal chain.
- **Edge enforcement**: governance and data policy must be enforced at the gateway/adapter boundary, not left to an external “brain”.
- **Loopback-first**: gateway binds to loopback by default; non-loopback requires explicit token + policy + audit.
- **Resource budget governance**: agents must declare and respect resource budgets; exceeding budget triggers stop or approval flow, never silent continuation.

## Completion Contract (How “Autonomous” Runs End)
Moltbot-style gateways can feel “autonomous” because the daemon stays up and keeps reacting to events, but each **run** must have explicit stop conditions. For CCOS chat gateway, completion must be **first-class, explicit, and governed**.

### Run lifecycle states (required)
- **Done**: goal satisfied; final outputs emitted; no further actions scheduled.
- **Paused (Approval Required)**: blocked on explicit approval (capability/effect/secret/channel/data egress).
- **Paused (Awaiting External Event)**: waiting for next message/webhook/cron tick.
- **Failed**: terminal error; run stops and records failure state (no hidden retries beyond policy).
- **Cancelled**: user/admin stop.

### Hard stop triggers (required)
Each run carries an immutable runtime context budget (examples; exact fields TBD):
- **Wall-clock timeout** (e.g., 60s interactive, 10m background).
- **Step/tool-call budget** (max yields / capability calls).
- **Token/cost budget** (if LLM is involved).
- **Max retries** per capability and per plan segment (governed).

When a budget is exhausted, the run must end as **Failed** (or **Paused** if configured to require human escalation), and it must be logged in the causal chain.

### What decides “finished”
CCOS must not rely on a model “feeling done” as the only termination signal. A run is **Done** only if:
- A declared completion predicate is satisfied (explicit check), and
- No pending approvals are required, and
- No scheduled follow-up work remains for the run.

### Long-running autonomy (checkpoint/resume, not infinite loops)
“Keep working until reaching a state” must be implemented as **bounded segments** with checkpoint/resume between them:
- Segment executes under budgets → yields effects through governance → records outcomes.
- If more work remains, it schedules a follow-up segment (cron/event) or resumes from a checkpoint.
- Every segment transition must be explicit and auditable.

**References**:
- Checkpoint/resume: `docs/ccos/specs/017-checkpoint-resume.md`
- Governance & context budgets: `docs/ccos/specs/005-security-and-context.md`
- Governance checkpoints (per-step): `docs/ccos/specs/035-two-tier-governance.md`

## Resource Budget Contract (Agent Controllability)
Every agent run must be **controllable end-to-end** through explicit resource budgets. CCOS does not allow "infinite" or "unbounded" execution under any circumstances.

### Budget Dimensions (Required)
Each agent run carries an **immutable budget context** with the following dimensions:

| Dimension | Description | Example |
|-----------|-------------|---------|
| **Wall-clock time** | Maximum elapsed time for the run | 60s interactive, 10m background |
| **Step/tool-call count** | Maximum capability invocations | 50 steps |
| **LLM token budget** | Input + output tokens consumed | 100k tokens |
| **LLM cost budget** | Monetary cost of LLM calls | $0.50 |
| **Sandbox CPU-seconds** | Cumulative CPU time in sandboxes | 30 CPU-seconds |
| **Sandbox memory-MB-seconds** | Memory × time integral | 512 MB × 30s |
| **Network egress bytes** | Total bytes sent externally | 10 MB |
| **Storage writes** | Bytes written to persistent storage | 50 MB |

### Upfront Estimation (Optional but Recommended)
Before execution, an agent **may** provide resource estimates:

```clojure
(agent "daily-report"
  :estimated-resources {
    :llm-tokens [10000 50000]     ; [min, max] range
    :steps [5 20]
    :wall-clock-ms [5000 60000]
    :cost-usd [0.05 0.25]
  }
  ...)
```

The GK can:
- **Warn** if estimates exceed configured thresholds
- **Block** if estimates exceed hard limits without pre-approval
- **Track** actual vs estimated for learning/audit

### Enforcement Behavior
When any budget is exhausted:
| Policy | Behavior |
|--------|----------|
| **hard-stop** (default) | Run ends immediately as `Failed`, logged to causal chain |
| **approval-required** | Run enters `Paused (Approval Required)`, waits for human |
| **soft-warn** | Warning logged, run continues (only for monitoring budgets) |

Configuration per budget dimension:
```clojure
:budget-policy {
  :wall-clock    {:limit 60000 :on-exceed :hard-stop}
  :steps         {:limit 50    :on-exceed :hard-stop}
  :llm-tokens    {:limit 100000 :on-exceed :approval-required}
  :cost-usd      {:limit 0.50  :on-exceed :hard-stop}
  :network-bytes {:limit 10485760 :on-exceed :hard-stop}
}
```

### Warning Thresholds
GK issues warnings before hard limits:

| Threshold | Action |
|-----------|--------|
| 50% consumed | Log warning, continue |
| 80% consumed | Log warning, optional pause for approval |
| 100% consumed | Enforce policy (stop or require approval) |

### Per-Step Accounting
Every capability call records resource consumption:

```clojure
{:step-id "step-7"
 :capability "places.search"
 :resources {
   :wall-clock-ms 1234
   :llm-tokens {:input 500 :output 200}
   :sandbox-cpu-ms 150
   :network-bytes {:egress 2048 :ingress 8192}
 }
 :budget-remaining {
   :steps 43
   :llm-tokens 92300
   :cost-usd 0.42
 }}
```

### Budget Inheritance
Budgets propagate hierarchically:
- **Session budget** → applies to all runs in session
- **Run budget** → applies to all steps in run
- **Step budget** → per-capability limits (from manifest `:resources`)

Inner budgets cannot exceed outer budgets. If a capability requests more than the remaining run budget, it is rejected before execution.

### Human Escalation
When budget exhaustion triggers `approval-required`:

1. Run pauses with state saved
2. User sees: "Run paused: LLM token budget exhausted (100k used). Approve additional 50k tokens to continue?"
3. User can:
   - **Approve**: Grant additional budget, run resumes
   - **Deny**: Run ends as `Cancelled`
   - **Modify**: Adjust budget and resume

### Audit Trail
Every budget event is logged to the causal chain:
- Budget allocation at run start
- Warning events at thresholds
- Exhaustion events with enforcement action
- Approval/denial of budget extensions
- Final resource consumption at run end

## Roadmap Overview (Workstreams)
To ship “Moltbot-like services” securely, CCOS needs product/ops layers around the existing core:

- **WS1 Gateway/Daemon**: long-running service, routing, ops, health.
- **WS2 Channel Connectors**: adapters for chat networks + normalization.
- **WS3 Data Protection**: classification, quarantine, redaction, safe exports.
- **WS4 Approvals + Secrets**: secret vault + approval UX + policy diffs.
- **WS5 Sessions + UX**: end-user dashboard, trace inspection/export/replay.
- **WS6 Packaging/Distribution**: signed bundles for “skills/capabilities”.
- **WS7 Remote Nodes/Pairing**: secure pairing + scoped remote surfaces.
- **WS8 Observability**: metrics/logging/tracing for runs, denials, costs.
- **WS9 Polyglot Sandboxed Capabilities** ✅: execute Python/JS/Go capabilities in microVMs with GK-proxied effects. See [Polyglot Sandboxed Capabilities Spec](./spec-polyglot-sandboxed-capabilities.md) and [Implementation Plan](./impl-ws9-polyglot-sandboxed-capabilities.md).
- **WS10 Skills Layer** ✅: natural-language teaching docs that map to governed capabilities. *(Implemented as part of WS9 Phase 4)*
- **WS11 Resource Budget Governance** ✅: upfront estimation, enforcement, warning thresholds, human escalation for all agent runs.
- **WS12 Governed Memory**: working memory + long-term memory with PII classification, storage quotas, and governed read/write (not raw workspace files).

### Implementation Status Summary

| Workstream | Status | Key Components |
|------------|--------|----------------|
| WS9 | ✅ Complete | `sandbox/` module (config, manager, network_proxy, secret_injection, filesystem, resources), manifest types (`RuntimeSpec`, `NetworkPolicy`, `FilesystemSpec`) |
| WS10 | ✅ Complete | `skills/` module (types, parser, mapper), `Skill`, `SkillMapper`, YAML parsing |
| WS11 | ✅ Complete | `budget/` module (context, events, types), `BudgetContext`, checkpoint/resume, approval queue integration |

## Phase 0 — Define “Chat Mode” Security Contract (Spec + Minimal Implementation)
**Goal**: make it impossible to accidentally recreate Moltbot’s unsafe personal-data model.

### Deliverables
- **D0.1: Data classification model** applied to *values*, not only capabilities:
  - classifications like `pii.chat.message`, `pii.attachment`, `secret.token`, `public`.
  - propagation rules (e.g., derived from PII remains PII unless proven redacted).
- **D0.2: Chat Mode policy pack** (constitution + static safety rules):
  - deny raw chat/attachment access by default
  - deny network egress for PII-classified values by default
  - restrict filesystem writes by default (and require explicit allowlisted paths if enabled)
  - require explicit “activation” for group chats (mention/keyword) and strict sender allowlists.
- **D0.3: Quarantine store contract**:
  - raw message bodies and media are stored as opaque blobs
  - only “transform capabilities” can access quarantine (summarize/extract/redact), returning schema-validated outputs.
- **D0.4: Audit requirements**:
  - every inbound message → session step record (metadata only by default)
  - every access to quarantine store → causal-chain event with justification + policy decision.

### Status (now)
- ✅ **Specs written**: `037`–`039` + `042`–`047` define classification, policy pack, quarantine, connector contract, approval/verifier wiring, and audit events.
- ✅ **Minimal implementation shipped** (Phase 0):
  - `ccos/src/chat/mod.rs` (classification metadata, in-memory quarantine, transform+verifier capabilities, egress gating, MCP result filtering, audit event helper)
  - `ccos/tests/chat_mode_tests.rs` (proves deny-by-default + approval/verifier gates; `cargo test -p ccos --test chat_mode_tests`)
  - Approvals plumbed end-to-end via `ApprovalCategory::{ChatPolicyException, ChatPublicDeclassification}` and surfaced in MCP/HTTP approval listings.
- ✅ **Phase 1 MVP wiring in progress**:
  - `ccos/src/chat/quarantine.rs` (persistent encrypted quarantine store + TTL enforcement + purge tooling)
  - `ccos/src/chat/connector.rs` (loopback webhook connector w/ allowlist + activation triggers)
  - `ccos/src/chat/gateway.rs` + `ccos-chat-gateway` binary (chat mode execution path with transform-only + egress gating)
  - Causal-chain query now supports function-prefix filtering for chat audit surfacing.

### References / Alignment
- Host boundary: `docs/ccos/specs/004-rtfs-ccos-boundary.md`
- Governance & context: `docs/ccos/specs/005-security-and-context.md`
- Two-tier governance checkpoints: `docs/ccos/specs/035-two-tier-governance.md`
- Sandbox isolation: `docs/ccos/specs/022-sandbox-isolation.md`
- Chat mode security contract: `docs/ccos/specs/037-chat-mode-security-contract.md`
- Chat mode policy pack: `docs/ccos/specs/038-chat-mode-policy-pack.md`
- Quarantine store contract: `docs/ccos/specs/039-quarantine-store-contract.md`
- Normalized message schema: `docs/ccos/specs/042-chat-message-schema.md`
- Connector adapter interface: `docs/ccos/specs/043-chat-connector-adapter.md`
- Hello world connector flow: `docs/ccos/specs/044-hello-world-connector-flow.md`
- Chat transform capabilities (incl. verifier): `docs/ccos/specs/045-chat-transform-capabilities.md`
- Chat approval flow: `docs/ccos/specs/046-chat-approval-flow.md`
- Chat audit events: `docs/ccos/specs/047-chat-audit-events.md`
- Polyglot sandboxed capabilities: [spec-polyglot-sandboxed-capabilities.md](./spec-polyglot-sandboxed-capabilities.md)

### Success criteria
- A “Hello world chat connector” can receive messages but **cannot** expose raw content to tools without an explicit, audited approval and a narrow transform capability.

## Phase 1 — MVP: Local Gateway + One Connector + Minimal Control Surface
**Goal**: a usable, safe local service that can be driven by an external agent (or later, an internal one).

### WS1 Gateway/Daemon (MVP)
- **D1.1**: single binary “gateway” mode with:
  - loopback-only HTTP/WebSocket control plane
  - persistent config + runtime state
  - session router (per sender / per chat thread)
  - restart/reconnect behavior with backoff.

### WS2 Channel Connector (MVP)
- **D1.2**: implement one connector end-to-end (choose the lowest-risk channel first):
  - normalized message schema (text, attachments metadata, thread info, sender, timestamps)
  - activation rules (mention/allowlist)
  - outbound send with rate limiting.

### WS3 Data Protection (MVP)
- **D1.3**: quarantine store + minimal transforms:
  - “summarize message” (returns redacted summary)
  - “extract tasks/entities” (returns structured map)
  - “attachment text extraction” behind an explicit policy gate.

### WS4 Approvals + Secrets (MVP)
- **D1.4**: minimal approvals CLI:
  - approve new channel connection
  - approve enabling a capability/effect
  - approve secret insertion.
- **D1.5**: minimal secret store abstraction:
  - secrets are never returned to the external agent as plaintext
  - secrets can be *used* by specific capabilities after approval.

### WS5 Sessions + UX (MVP)
- **D1.6**: session viewer (CLI/TUI is fine for MVP):
  - list sessions, steps, denials
  - export a session trace (without raw PII by default)
  - replay a trace deterministically (where possible).

### WS9 Polyglot Sandboxed Capabilities (MVP)
- **D1.7**: container sandbox runtime (nsjail/bubblewrap):
  - virtual filesystem mounting (read-only source, ephemeral scratch)
  - network proxy through GK (allowlist enforcement)
  - secret injection via tmpfs (`/run/secrets/`)
  - basic CPU/memory/time limits.
- **D1.8**: capability manifest schema extension for `:runtime`, `:filesystem`, `:network`, `:secrets`, `:resources`.

See [Polyglot Sandboxed Capabilities Spec](./spec-polyglot-sandboxed-capabilities.md) for full details.

### References / Alignment
- Interactive mode session model: `docs/ccos/specs/007-mcp-server-interactive-mode.md`
- MCP server tool surface: `docs/ccos/specs/036-mcp-server-architecture.md`
- Capability lifecycle + approvals/promotion: `docs/ccos/specs/030-capability-system-architecture.md`
- Polyglot sandboxed capabilities: [spec-polyglot-sandboxed-capabilities.md](./spec-polyglot-sandboxed-capabilities.md)

### Success criteria
- User can run a local gateway, connect one chat channel, and safely perform workflows like:
  - “summarize the last 20 messages” (summary only)
  - “create a TODO list from a thread” (structured output only)
  - “send a reply” (outbound only; no raw transcript leakage unless approved).

## Phase 2 — v1: Multi-Connector, Dashboard, Policy Diffs, and Safer Tool Ecosystem
**Goal**: make it operable for daily use with strong governance and clear UX.

### WS2 Channel Connectors (v1)
- **D2.1**: multiple connectors with a shared adapter contract:
  - message edits/deletes, reactions, threaded replies
  - group membership changes
  - media download pipeline.

### WS3 Data Protection (v1)
- **D2.2**: policy-driven “data egress controls”:
  - block sending PII-derived outputs to network capabilities by default
  - allow explicit, scoped exceptions per domain/capability.
- **D2.3**: retention policies:
  - TTL for quarantine blobs
  - user-triggered purge
  - export with redaction guarantees.

### WS4 Approvals + Secrets (v1)
- **D2.4**: approval UI with **policy diffs**:
  - show effect/permission/schema changes for a capability update
  - show which policy would be violated and why.
- **D2.5**: secret provenance and usage logs:
  - which capability used which secret, when, and for what call id.

### WS5 Sessions + UX (v1)
- **D2.6**: dashboard:
  - sessions, steps, denials, approvals queue
  - capability catalog and trust tiers
  - “export trace to RTFS plan” (safe, non-PII by default).

### WS6 Packaging/Distribution (v1)
- **D2.7**: signed “skill bundles”:
  - set of capabilities + schemas + docs + recommended policies
  - verification on install/update
  - staged rollout and rollback.

### WS9 Polyglot Sandboxed Capabilities (v1)
- **D2.8**: Firecracker microVM runtime:
  - hardware-level isolation for untrusted community code
  - warm pool for fast startup (~125ms)
  - health checks and lifecycle management.
- **D2.9**: sandbox manager with multiple runtime backends:
  - `:rtfs` (no sandbox), `:wasm`, `:container`, `:microvm`
  - trust tier hierarchy (pure → wasm → container → microvm).

### WS10 Skills Layer (v1)
- **D2.10**: skill YAML schema:
  - natural-language teaching docs
  - `ccos.capabilities` field mapping to governed capabilities
  - data classification declarations for chat gateway.
- **D2.11**: skill → capability mapping engine:
  - LLM interprets skill, selects capability
  - all execution goes through GK.

### Success criteria
- Non-technical user can install and operate CCOS chat gateway with:
  - clear approvals, clear risk explanations, and safe defaults
  - easy “turn this workflow into a reusable agent” without leaking raw chats.

## Phase 3 — v2: Remote Nodes/Pairing, Multi-Tenant Routing, and Autonomy (Optional)
**Goal**: enable distributed usage without weakening the security model.

### WS7 Remote Nodes/Pairing (v2)
- **D3.1**: pairing protocol with device identity + key rotation.
- **D3.2**: scoped remote surfaces:
  - a node can expose only a subset of capabilities
  - remote node cannot exfiltrate quarantine data by default.

### WS1 Gateway/Daemon (v2)
- **D3.3**: multi-tenant routing:
  - separate policy packs, secrets, memory, and sessions per “persona/tenant”.

### Autonomy (v2+)
- **D3.4**: optional autonomous agents for background work, still governed:
  - scheduled jobs and long-running tasks
  - checkpoint/resume support where relevant (`docs/ccos/specs/017-checkpoint-resume.md`).

### Success criteria
- Safe remote access via pairing without expanding default data access; “least privilege” holds even across devices.

## Deferred to Later Phases
The following are important but **not blocking for Phase 0-2**:

- **WS12 Governed Memory**: Full memory system with vector search, session memory indexing, and long-term preferences. For now, use existing `ccos_record_learning` / `ccos_recall_memories` with causal chain.
- **Learning from failures**: Automated capability improvement based on run outcomes.
- **Multi-agent coordination**: Agents delegating to other agents with budget splitting.
- **Cost optimization**: Automatic model selection and batching for cost efficiency.

## Out of Scope (Explicit Non-Goals)
- “Full raw-chat access by default” or “LLM can read everything in your chats” modes.
- Silent tool acquisition/installation/auto-approval.
- Implicit secret leakage to external agents or logs.

## Key Risks / Watch Items
- **Interactive-mode governance gap**: `docs/ccos/specs/007-mcp-server-interactive-mode.md` notes governance is often bypassed in MCP mode; for chat gateway, governance must be **enforced**, not voluntary.
- **Data classification correctness**: mislabeling PII as safe is catastrophic; start conservative and require explicit downgrades.
- **Attachment pipeline**: media parsing is a large attack surface; keep it sandboxed (`docs/ccos/specs/022-sandbox-isolation.md`).

## Next Actions (Concrete)
- **Choose MVP connector target** (lowest-risk onboarding + strongest allowlist/activation controls), then implement it against `043-chat-connector-adapter.md`.
- **Wire a gateway/daemon “chat mode” execution path**:
  - connector → `MessageEnvelope` with quarantine pointers
  - transform-only access + egress gating via `ccos.chat.*` capabilities
  - outbound send only after `ccos.chat.egress.prepare_outbound` succeeds.
- **Persist quarantine store** (Phase 1 requirement from `039`):
  - add encryption-at-rest + retention/TTL enforcement + purge tooling
  - keep pointer unguessability + pointer-not-authorization invariant.
- **Integrate audit events with existing observability surfaces**:
  - ensure chat audit actions can be queried/visualized consistently (Causal Chain viewer/TUI).

- Review and finalize [Polyglot Sandboxed Capabilities Spec](./spec-polyglot-sandboxed-capabilities.md):
  - confirm sandbox technology choice (Firecracker vs gVisor vs nsjail)
  - define capability manifest schema for `:runtime` field
  - prototype GK network proxy with allowlist enforcement.
- Design skill YAML schema and skill → capability mapping.

## Related Specifications
- [Polyglot Sandboxed Capabilities](./spec-polyglot-sandboxed-capabilities.md) — execute Python/JS/Go capabilities in isolated sandboxes with GK-governed effects.
- [Resource Budget Enforcement](./spec-resource-budget-enforcement.md) — runtime budget tracking, enforcement, and human escalation.
