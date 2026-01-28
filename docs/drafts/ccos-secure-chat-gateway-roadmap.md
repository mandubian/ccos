# CCOS Secure Chat Gateway Roadmap (Draft)

## Context
Moltbot’s core product value is an **always-on Gateway** that connects chat channels to a **local agent runtime** with sessions, skills/tools, a dashboard, and node pairing (see [Moltbot docs](https://docs.molt.bot/)).

CCOS/RTFS already provides a stronger **secure compute core**:
- RTFS purity + explicit host boundary (`docs/ccos/specs/004-rtfs-ccos-boundary.md`)
- capability marketplace, manifests, schema validation, effects, discovery→pending→approval (`docs/ccos/specs/030-capability-system-architecture.md`)
- session model in interactive mode via MCP (`docs/ccos/specs/007-mcp-server-interactive-mode.md`, `docs/ccos/specs/036-mcp-server-architecture.md`)
- governance architecture (`docs/ccos/specs/005-security-and-context.md`, `docs/ccos/specs/035-two-tier-governance.md`)
- sandbox isolation primitives (`docs/ccos/specs/022-sandbox-isolation.md`)

This roadmap focuses on what CCOS is missing to provide “chat-to-local-agent” services **without adopting Moltbot’s broad personal-data exposure model**.

## Guiding Principles (Non-Negotiable)
- **Default-deny personal data**: raw chat content and attachments are not automatically accessible to planning/tool calls.
- **Transform-first ingestion**: store raw data in a quarantined store; expose only narrow, schema-checked transformations (summaries, extracted entities) by default.
- **No silent escalation**: new effects/permissions/channels/secrets always require explicit approval and are recorded in the causal chain.
- **Edge enforcement**: governance and data policy must be enforced at the gateway/adapter boundary, not left to an external “brain”.
- **Loopback-first**: gateway binds to loopback by default; non-loopback requires explicit token + policy + audit.

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

### References / Alignment
- Host boundary: `docs/ccos/specs/004-rtfs-ccos-boundary.md`
- Governance & context: `docs/ccos/specs/005-security-and-context.md`
- Two-tier governance checkpoints: `docs/ccos/specs/035-two-tier-governance.md`
- Sandbox isolation: `docs/ccos/specs/022-sandbox-isolation.md`

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

### References / Alignment
- Interactive mode session model: `docs/ccos/specs/007-mcp-server-interactive-mode.md`
- MCP server tool surface: `docs/ccos/specs/036-mcp-server-architecture.md`
- Capability lifecycle + approvals/promotion: `docs/ccos/specs/030-capability-system-architecture.md`

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

## Out of Scope (Explicit Non-Goals)
- “Full raw-chat access by default” or “LLM can read everything in your chats” modes.
- Silent tool acquisition/installation/auto-approval.
- Implicit secret leakage to external agents or logs.

## Key Risks / Watch Items
- **Interactive-mode governance gap**: `docs/ccos/specs/007-mcp-server-interactive-mode.md` notes governance is often bypassed in MCP mode; for chat gateway, governance must be **enforced**, not voluntary.
- **Data classification correctness**: mislabeling PII as safe is catastrophic; start conservative and require explicit downgrades.
- **Attachment pipeline**: media parsing is a large attack surface; keep it sandboxed (`docs/ccos/specs/022-sandbox-isolation.md`).

## Next Actions (Concrete)
- Write a short “Chat Mode Security Contract” spec draft (Phase 0) that defines:
  - data classes, quarantine store, transform capabilities, egress policy defaults.
- Choose MVP connector target based on lowest-risk onboarding and strongest allowlist/activation controls.
- Define the normalized message/event schema and the adapter interface once, then implement the first connector against it.

