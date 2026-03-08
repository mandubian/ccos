# Autonoetic Project Plan

## Phase 1: Specification & Design ✅

### Completed Documents

- [x] **[concepts.md](concepts.md)** — Core philosophy & vision
- [x] **[architecture_modules.md](architecture_modules.md)** — System topology, modules, security
  - [x] Gateway (Vault, Route Engine, LLM Driver Abstraction)
  - [x] Agent Orchestrator (SKILL.md Manifest, Ed25519 Signing)
  - [x] Sandbox Workers (Bubblewrap, Docker, MicroVM, WASM)
  - [x] Two-Tier Memory Architecture
  - [x] Artifact Store, Runtime Lock, Cognitive Capsule
  - [x] Capability-Based Security Model
  - [x] Agent Loop Stability (Loop Guard, Session Repair)
  - [x] Security Hardening (Taint Tracking, SSRF, Scanner)
- [x] **[protocols.md](protocols.md)** — Communication standards
  - [x] Internal JSON-RPC (Gateway ↔ Agent)
  - [x] Sandbox Bridge (Agent ↔ Sandbox via Gateway)
  - [x] Artifact Handles & Capsule Transport
  - [x] MCP Client/Server integration
  - [x] OFP Federation (wire-compatible with OpenFang)
  - [x] Causal Chain with hash-chain audit trail
- [x] **[sandbox_sdk.md](sandbox_sdk.md)** — SDK API surface
- [x] **[data_models.md](data_models.md)** — Concrete schemas
- [x] **[cli_interface.md](cli_interface.md)** — CLI surface

### Key Design Decisions
- **Agent = SKILL.md**: Unified format (YAML frontmatter + Markdown body)
- **OFP adopted natively**: Wire-compatible with OpenFang for cross-ecosystem federation
- **MCP yes, external A2A no**: Standard tool ecosystem via MCP; internal agent-to-agent routing via JSON-RPC message bus
- **Textual State**: Tier 1 memory is plain text files; Tier 2 is indexed Gateway substrate
- **Artifacts are content-addressed**: Large data and shared outputs move by immutable handles
- **Skills declare effects**: `metadata.declared_effects` narrows what a Skill may do beyond Agent-level capabilities

---

## Phase 2: Implementation Scaffolding ✅

- [x] Rust workspace: `autonoetic-gateway`, `autonoetic-types`, `autonoetic-ofp`, `autonoetic-mcp`, `autonoetic-sdk`
- [x] `autonoetic` CLI binary (clap-based)
- [x] Stub Gateway: config loading, Agent directory scanning, `runtime.lock` resolution
- [x] Stub Sandbox: bwrap process spawning with stdio piping

---

## Phase 3: Core Engine ✅

- [x] Gateway event loop (tokio async runtime)
- [x] JSON-RPC message router
- [x] Policy engine (capability validation)
- [x] Artifact store (content-addressed cache + handle resolution)
- [x] Causal Chain logger (append-only .jsonl with hash-chain linkage)
- [x] Vault (env var ingestion, secret zeroization)

---

## Phase 4: Agent Runtime ✅

- [x] SKILL.md parser (YAML frontmatter + Markdown body)
- [x] Agent lifecycle (Wake → Context Assembly → Reasoning → Hibernate)
- [x] Tier 1 memory (state/ directory read/write)
- [x] Tier 2 memory (SQLite + vector embeddings)
- [x] Skill declared effects + artifact dependency enforcement
- [x] Loop Guard & Session Repair
- [x] Ed25519 Manifest Signing

---

## Phase 4.5: LLM Drivers ✅

Complete, modular LLM provider system — thin by design (≤250 LOC per driver), no code duplication, all credential/endpoint resolution centralised.

### Architecture (`llm/`)

| File | Role |
|------|------|
| `mod.rs` | Shared types: `CompletionRequest`, `CompletionResponse`, `Message`, `ToolDefinition`, `ToolCall`, `StreamEvent`, `LlmDriver` trait |
| `provider.rs` | `ResolvedProvider` + `ProviderCapabilities` flags (`supports_streaming`, `supports_tool_stream_deltas`, `supports_system_top_level`, `supports_usage_in_stream`). Drivers never read env vars — all credential/URL resolution happens here |
| `openai.rs` | OpenAI-compatible driver: handles all providers below through one code path |
| `anthropic.rs` | Anthropic Messages API: `tool_use`/`tool_result` content blocks, system at top-level, SSE streaming |
| `gemini.rs` | Google Gemini: `functionDeclarations`/`functionCall`/`functionResponse`, `systemInstruction` |

### Provider support (all via `provider::resolve()`)

**Distinct wire formats:**
- `anthropic` / `claude` — Anthropic Messages API
- `gemini` / `google` — Gemini generateContent API

**OpenAI-compatible (single driver, table-driven URLs):**
- `openai`, `openrouter`, `groq`, `together`, `deepseek`, `mistral`, `fireworks`
- `perplexity`, `cohere`, `ai21`, `cerebras`, `sambanova`, `huggingface`
- `xai`, `replicate`, `moonshot`/`kimi`, `qwen`/`dashscope`
- `ollama`, `vllm`, `lmstudio` (local, no key required)

### Features proven in tests

- Tool call round-trips (ToolUse → ToolResult for all 3 wire formats)
- SSE streaming with delta accumulation (text + tool input deltas)
- Exponential backoff retries (3 attempts, 429/529)
- `max_completion_tokens` for GPT-5/o-series reasoning models
- **22 golden tests** (unit, no network) — request & response shapes for all 3 providers
- **2 live integration tests** vs OpenRouter `google/gemini-3-flash-preview`:
  - Simple completion → `"4 total."` ✅
  - Tool call → `get_weather(city="Paris")` ✅

### Agent lifecycle upgrade
`AgentExecutor` upgraded from a stub to a **real agentic loop**:
- Maintains `history: Vec<Message>` across LLM turns
- Dispatches `ToolUse` turns → executes → pushes `ToolResult` → resets `LoopGuard`
- Hibernates cleanly on `EndTurn`

---

## Phase 5: Networking & Federation ✅

- [x] OFP TCP listener + HMAC handshake
- [x] PeerRegistry + extension negotiation
- [x] MCP Client (stdio/SSE transport, tool discovery)
- [x] MCP Server (expose agents as tools)

### Completion notes

- OFP runtime now starts from normal gateway boot path with strict required federation identity env vars.
- Peer lifecycle is wired into `PeerRegistry` (connected/disconnected) and validated with socket-level integration tests.
- MCP servers are persisted in `mcp_servers.json`, probed on registration and gateway startup, and exposed in `gateway status`/`gateway status --json`.
- Agent runtime now dispatches `mcp_<server>_<tool>` tool calls via loaded MCP registry clients.
- Coverage includes `ofp_integration` and `mcp_integration` test suites in `autonoetic-gateway/tests/`.

---

## Phase 6: SDK & Sandbox ✅

- [x] Python `autonoetic_sdk` package
- [x] JavaScript/TypeScript `autonoetic_sdk` package
- [x] Bubblewrap sandbox driver
- [x] Docker sandbox driver
- [x] MicroVM sandbox driver (Firecracker)

### Completion notes

- Added Python SDK package at `autonoetic-sdk/python/` with `init()` and full API namespaces (`memory`, `state`, `secrets`, `message`, `files`, `artifacts`, `events`, `tasks`) over Unix socket JSON-RPC.
- Added TypeScript SDK package at `autonoetic-sdk/typescript/` with equivalent `init()` and API namespaces over Unix socket JSON-RPC.
- Upgraded gateway sandbox runtime to a multi-driver model (`bubblewrap`, `docker`, `microvm`) with strict env-driven configuration for Docker image and Firecracker config path.
- Added unit coverage for sandbox driver parsing and command construction (`sandbox::tests::*`).
- Wired `autonoetic agent run` to the real lifecycle executor so manifest-declared sandbox runtime is now exercised from CLI.
- Added capability-gated `sandbox.exec` tool dispatch with policy enforcement and hard-fail behavior for unknown tools.
- Added thin dependency bootstrap support (Python/Node), with deterministic defaults read from `runtime.lock` (`dependencies`) and explicit tool-call overrides.

---

## Phase 7: Polish & Release 🔜

- [ ] End-to-end integration tests
- [ ] Documentation site
- [ ] Example agents (researcher, coder, auditor)
- [ ] AgentSkills.io marketplace publishing
- [ ] Open-source release (choose license)

### Progress notes

- `autonoetic agent run --interactive` now executes a real stdin chat loop and reuses lifecycle tool execution (`mcp_*` and `sandbox.exec`) instead of warning-only fallback behavior.
- `autonoetic agent init` now scaffolds `SKILL.md`, `runtime.lock` (including `dependencies: []`), and baseline agent directories (`state/`, `history/`, `skills/`, `scripts/`).
- Added CLI tests for runtime policy enforcement and scaffold creation in `autonoetic/src/main.rs`.
- Added binary-level CLI integration coverage in `autonoetic/tests/cli_e2e.rs` for `agent init` + `agent run --interactive` (scripted `/exit`) to validate end-to-end command wiring.
- `SKILL.md` parsing now supports AgentSkills-compliant top-level frontmatter (`name`, `description`, `metadata`) with Autonoetic manifest fields nested under `metadata.autonoetic` for editor/tool compatibility.
- Lifecycle now writes compact causal events by default and supports optional full redacted evidence capture via `AUTONOETIC_EVIDENCE_MODE=full` (`evidence_ref` pointers under `history/evidence/<session_id>/`).
- Causal tracing now includes explicit session semantics (`session/start`, `session/end`, `session_id`, `turn_id`, `event_seq`) so one agent can be run multiple times with unambiguous trace segmentation.
- Causal schema now promotes `session_id`, `turn_id`, and `event_seq` to top-level fields and adds explicit `entry_hash` / `payload_hash` fields for cryptographic integrity and low-cost payload introspection.
- Hash-chain linkage now uses SHA-256 and reloads continuity across process restarts by hydrating `last_hash` from the last persisted causal entry.
- Added CLI trace introspection commands: `autonoetic trace sessions`, `autonoetic trace show <session_id>`, and `autonoetic trace event <log_id>` with optional `--agent` and `--json`.

---

## Phase 8: Gateway Event Ingress & Agent Orchestration 🔜

- [x] Add a real Gateway JSON-RPC listener on `config.port` (line-delimited JSON-RPC over TCP).
- [x] Add `event.ingest` and `agent.spawn` JSON-RPC methods that trigger an agent runtime session.
- [x] Add policy gate for spawn delegation (`AgentSpawn` / `AgentMessage`) before runtime dispatch.
- [x] Implement OFP inbound `AgentMessage` handling to execute targeted local agent sessions (replace `501 Not Implemented` path).
- [x] Add end-to-end integration tests for external event -> gateway ingress -> agent reply -> causal trace verification.
- [x] Add reliability controls for ingress (`max concurrent spawns`, per-agent queueing, and backpressure behavior).

### Progress notes

- Gateway now boots both OFP and JSON-RPC listeners concurrently.
- `agent.spawn` loads agent manifest/instructions from disk and executes one runtime turn via the same lifecycle engine used by CLI, including causal/session tracing.
- `event.ingest` converts incoming event payload into a kickoff prompt and dispatches to the target agent.
- Gateway JSON-RPC dispatch now writes its own causal entries (`*.requested` / `*.completed` / `*.failed`) to `agents/.gateway/history/causal_chain.jsonl` with shared session IDs for cross-layer trace correlation.
- Added a hermetic live-ingress test in `autonoetic-gateway/tests/gateway_ingress_integration.rs` that hits the TCP JSON-RPC listener, runs a real `event.ingest` session through a local OpenAI-compatible stub, and verifies both gateway and agent causal traces share the same session ID.
- Gateway ingress now enforces configurable reliability controls via `max_concurrent_spawns` and `max_pending_spawns_per_agent`, with per-agent serialization and explicit backpressure errors when either the global execution budget or an agent queue is full.
- Added router unit coverage for delegated spawn limits, per-agent queue overflow, and global concurrent execution backpressure.

---

## Phase 9: Governed Background Reevaluation & Evolution 🔜

- [x] Add a gateway-owned reevaluation cadence model for agents/sessions (`evaluation_interval_secs` or BPM-style policy) instead of agent-driven self-respawn.
- [x] Keep transport/process heartbeat separate from work scheduling; heartbeat remains liveness telemetry only.
- [x] Add a scheduler-managed `should_wake` decision path that evaluates whether an agent needs more work before waking it.
- [x] Define wake predicates for: new inbound messages, delegated task completions, unresolved queued work, stale goals, retryable failures, and explicit timers.
- [x] Record wake reasons in the gateway causal chain so every background wake has an auditable cause.
- [x] Reuse existing ingress reliability controls for background wakes so cadence-driven execution respects global concurrency, per-agent admission, and backpressure.
- [x] Add deduplication rules so periodic reevaluation cannot create overlapping or duplicate background runs for the same agent/session.
- [x] Add policy controls for which agents are allowed background reevaluation and what maximum cadence they may request.
- [x] Support two execution modes for cadence wakes: direct approved capability trigger for deterministic jobs, or full agent wake for open-ended reasoning jobs.
- [x] Add memory-aware reevaluation so the scheduler can wake an agent based on persisted state markers and prior-run outcomes, not just time.
- [x] Add integration tests for idle cadence, wake-on-new-work, dedup under rapid ticks, paused-on-approval flows, and backpressure under many due agents.
- [x] Add an end-to-end evolution test: periodic reevaluation -> detect gap -> create candidate skill/code artifact -> request approval -> execute approved capability on a later wake.

### Progress notes

- Design intent: treat cadence as a gateway scheduling primitive, not as an agent recursively waking itself.
- Existing liveness heartbeat in CCOS should remain telemetry; Autonoetic background progress should route through the gateway scheduler and policy engine.
- The first implementation should prefer deterministic wake predicates and approved direct-capability triggers before enabling broader open-ended background reasoning.
- Added `background` manifest policy, `BackgroundReevaluation` capability, gateway scheduler guardrails, and durable scheduler state/approval types.
- Gateway now starts a background scheduler alongside OFP and JSON-RPC listeners; background wakes reuse the shared execution path and existing ingress admission/backpressure controls.
- Added durable `.gateway/scheduler/` storage for agent state, inbox/task-board inputs, and pending/approved/rejected approval decisions, plus CLI commands for `gateway approvals list|approve|reject`.
- Added deterministic direct actions for approved `write_file` and existing `sandbox_exec`, and persisted `state/reevaluation.json` markers (`retry_not_before`, `stale_goal_at`, `last_outcome`, `pending_scheduled_action`, `open_approval_request_ids`).
- Added scheduler coverage for idle timer cadence, wake-on-new-work, rapid-tick dedup, paused-on-approval, due-agent throttling, and approval-to-approved-artifact evolution flow.

### Integration test track

- [ ] Add a dedicated Autonoetic integration-test harness for gateway + agent runtime + local stub LLM.
- [ ] Keep test fixtures and helper utilities inside Autonoetic crates; do not depend on CCOS runtime code or test modules.
- [ ] Use CCOS and OpenFang only as design references for useful patterns; reimplement only the pieces that fit Autonoetic architecture.

#### Test A: Loopback channel -> memory -> outbound -> audit

- [x] Add a loopback channel test transport in Autonoetic test code that simulates inbound user messages and captures outbound agent replies.
- [x] Start the real Autonoetic gateway in test mode on ephemeral ports with temp agent directories, temp storage, and temp causal logs.
- [x] Add a scripted local LLM stub that deterministically produces tool/action choices for the scenario.
- [x] Drive an inbound message through the loopback transport to a real agent session.
- [x] Assert that the agent writes state to its memory substrate during the first turn.
- [x] Drive a later inbound message for the same logical conversation and assert the agent reads the previously stored state.
- [x] Assert that the gateway emits the expected outbound reply through the loopback transport.
- [ ] Strengthen audit assertions so the gateway completion entry and agent memory operation are correlated at per-turn granularity, not only by shared session ID.
- [x] Add negative-path assertions for missing memory key/default handling and outbound backpressure or delivery failure reporting.

#### Test B: Generate skill -> approval -> later approved execution

- [x] Add an Autonoetic-native generated-skill test flow; do not call CCOS `ccos_create_skill` or depend on CCOS approval/runtime plumbing.
- [x] Define the minimal Autonoetic draft-skill artifact shape needed for test generation, loading, and later execution.
- [x] Add a temp learned-skills directory and artifact loader in the Autonoetic integration harness.
- [x] Use a scripted local LLM stub so the first message causes the agent to draft a new skill/capability artifact.
- [x] Execute the generated artifact once as a PoC inside the normal Autonoetic runtime path.
- [x] Submit the generated artifact into an Autonoetic approval queue with explicit evidence from the PoC run.
- [x] Assert that the artifact is not eligible for scheduled/background reuse until approval is granted.
- [x] Programmatically approve the artifact in the test and verify registry/loader state updates accordingly.
- [x] Send a later message that should reuse the approved artifact and assert the agent executes it successfully without regenerating it.
- [x] Assert full audit coverage for draft generation, PoC execution, approval pending, approval granted, and approved reuse.

#### Follow-up extraction tasks

- [x] Factor Autonoetic test helpers into a shared test-support module for temp workspace setup, local LLM stubs, message injection, and causal-log inspection.
- [x] Add deterministic approval-fixture helpers so approval-dependent tests do not need manual steps.
- [x] Add a small catalog of reusable test agents covering: memory recall, outbound messaging, generated skill draft, and approved skill reuse.
- [ ] Document the test philosophy clearly: Autonoetic remains implementation-independent even when borrowing ideas from CCOS/OpenFang.

#### Terminal chat client (gateway-native trigger surface)

- [x] Add an Autonoetic CLI command for terminal chat, separate from local `agent run --interactive` runtime behavior.
- [x] Route terminal chat through gateway JSON-RPC ingress (`event.ingest`) instead of calling agent runtime directly.
- [x] Define a terminal-channel envelope model with stable `channel_id`, `sender_id`, and session identifiers so terminal conversations map cleanly to future channel adapters.
- [x] Support a minimal request/reply UX first: read a user line from stdin, send an ingress event, print the agent reply, repeat until `/exit`.
- [x] Keep the terminal chat client thin; do not duplicate runtime, policy, or orchestration logic already owned by the gateway.
- [x] Add a test mode for the terminal chat client so integration tests can script stdin input and assert stdout replies deterministically.
- [x] Verify that terminal chat exercises the same causal logging, policy checks, memory usage, and backpressure behavior as any other external ingress path.
- [ ] Use the terminal chat client as the primary developer-facing trigger surface before adding Discord, WhatsApp, or other external adapters.

#### Session context (thin per-session continuity)

- [x] Add a gateway-owned `session context` artifact under `state/sessions/<session_id>.json` for bounded per-session continuity.
- [x] Keep `session context` intentionally smaller than durable memory: recent exchange, compact known facts, and open threads only.
- [x] Inject a rendered `session context` summary automatically into each new foreground `event.ingest` execution for the same session.
- [x] Persist minimal session updates after each completed turn without requiring agent-authored tool calls.
- [x] Keep durable memory (`memory.read` / `memory.write`) separate from `session context`; do not auto-inline the whole `state/` directory into prompts.
- [x] Add strict size limits and pruning rules so session context remains cheap and predictable.
- [x] Add focused runtime tests proving session context is persisted after one turn and injected on the next turn for the same session.
- [ ] Later: promote selected durable-memory writes into session context only when there is a small, well-defined summarization rule.
- [ ] Later: consider deriving session context updates from runtime/causal events instead of direct execution-path writes if the direct path becomes too coupled.

#### Disclosure policy (reply governance over memory and tool output) ✅

- [x] Add a first-class disclosure policy layer that evaluates whether tool results and retrieved memory may be repeated verbatim, summarized only, or withheld.
- [x] Keep disclosure policy separate from log/evidence redaction; logs may be redacted while assistant replies still require an explicit disclosure decision.
- [x] Classify output sources at minimum by origin: user input, session context, Tier 1 memory, Tier 2 memory, secret-store/vault material, sandbox output, and external tool output.
- [x] Add deterministic disclosure classes such as `public`, `internal`, `confidential`, and `secret`, with explicit reply behavior for each class.
- [x] Enforce path-based defaults for Tier 1 memory so designated locations can be marked summary-only or non-disclosable without relying on prompt wording.
- [x] Ensure secret-store and vault-managed values are never returned verbatim to the user unless an explicitly approved capability allows it.
- [x] Add a reply-filter step after tool execution and before final assistant output so disallowed content is replaced with safe summaries or refusal text.
- [x] Preserve the existing agent/runtime split: tools may return structured results, but disclosure enforcement should happen centrally in the runtime path.
- [x] Add focused tests proving the system can distinguish between normal memory recall and restricted/secret material in user-visible replies.
- [x] Add terminal-chat coverage that verifies session context may influence reasoning without causing protected values to be echoed back automatically.

### Progress notes

- Current enforcement is intentionally exact-substring (verbatim) reply filtering for deterministic, auditable behavior.
- This is explicit verbatim leak prevention, not fuzzy or semantic redaction.
- Secret-store extracted values are explicitly tainted as `secret`, including short values.
- Registry-backed metadata extraction allows handlers to classify outputs (e.g. file paths) without the core loop knowing tool-specific argument formats.
- Verified via `test_disclosure_enforcement_in_executor_loop`.

#### Native tool modularization (thin registry, not a plugin framework) ✅

- [x] Replace the hard-coded native tool `if/else` chain in `runtime/lifecycle.rs` with a small registry-based dispatch path.
- [x] Keep the design intentionally thin: static in-process handlers only, no dynamic loading, no external plugin protocol, no separate service layer.
- [x] Define one native tool handler shape that covers: tool name, availability check from manifest/policy, invocation, and optional metadata extraction for audit/disclosure.
- [x] Preserve the current MCP-first behavior so external MCP tools still bypass native dispatch when present.
- [x] Move `sandbox.exec`, `memory.read`, `memory.write`, and `skill.draft` into registry-backed handlers without changing user-facing behavior.
- [x] Stop special-casing native-tool path extraction in the main lifecycle loop; let handlers expose any disclosure/audit metadata they produce.
- [x] Keep tool-result redaction, disclosure, and causal logging centralized in the lifecycle path even after dispatch is modularized.
- [x] Add focused runtime tests proving known native tools still execute, unknown tools still fail cleanly, and MCP/native precedence remains unchanged.
- [x] Add one extension-oriented test that registers or wires a small additional native test tool with minimal lifecycle churn to prove the composability goal.

#### Refactor backlog (simpler, thinner, easier to reason about)

Execution order matters here: do the identity/loading and trace/session ownership work first, then split larger files around those new seams.

##### 1. Agent repository and identity unification

- [x] Introduce a single `AgentRepository` / `LoadedAgent` path that owns directory scan, `SKILL.md` load, manifest parse, and instruction extraction.
- [x] Stop duplicating manifest load/parse flows across gateway execution, scheduler, router helpers, and CLI paths.
- [x] Decide one identity rule and enforce it consistently: require `dir_name == manifest.agent.id`.
- [x] Replace direct `scan_agents` + local parse patterns with repository calls in the highest-traffic runtime paths first.

Acceptance criteria:
- One reusable load path returns `LoadedAgent { dir, manifest, instructions }`.
- Directory-name identity drift is either rejected with a clear error or made impossible by design.
- Gateway execution and scheduler use the same repository abstraction.

Progress notes:
- `execution.rs` and `scheduler.rs` now load agents through `AgentRepository`.
- Remaining cleanup is mostly edge-path consistency (notably CLI trace loading still enumerates directories directly before repository loads).
- Identity mismatch enforcement now hard-fails in repository load paths (`get_sync`, `list_loaded_sync`) and scheduler tick loading, so mismatches are surfaced deterministically instead of being skipped.

##### 2. Session tracer / trace-session ownership

- [x] Introduce a small `SessionTracer` / `TraceSession` abstraction that owns `session_id`, event sequencing, causal logger access, and shared trace helpers.
- [x] Stop resetting caller-local `event_seq` counters in router and scheduler request/tick handlers.
- [x] Move common gateway trace emission (`requested` / `completed` / `failed` / `skipped`) behind the tracer helper instead of open-coded calls.
- [x] Fix trace viewing so long-lived sessions are not ordered only by `event_seq` when duplicates may exist.

Acceptance criteria:
- Router and scheduler no longer manually manage raw sequence counters.
- Trace sorting is stable for long-lived sessions and background session IDs.
- Trace-related tests cover at least one multi-event session that would previously have duplicate `event_seq` values.

Progress notes:
- Router and scheduler now create trace sessions with explicit gateway/session IDs and shared tracer helpers for `requested`/`completed`/`failed`/`skipped` events.
- CLI trace view now sorts by `timestamp` then `event_seq`, avoiding ambiguous ordering when sequence values reset across requests/sessions.
- Regression coverage includes tracer-level duplicate-sequence behavior and CLI timestamp-ordering checks.

##### 3. Scheduler split by domain responsibility

- [x] Split `scheduler.rs` into smaller modules such as `scheduler/decision.rs`, `scheduler/store.rs`, `scheduler/approval.rs`, and `scheduler/runner.rs`.
- [x] Separate wake-decision logic from persistence helpers and side-effecting execution.
- [x] Revisit approval-resolution wake behavior while doing the split so predicates are enforced intentionally, not incidentally.
- [x] Remove dead or misleading parameters and helpers that become unnecessary after the split.

Acceptance criteria:
- Wake-decision code is readable without scrolling through approval and JSON persistence helpers.
- Approval, inbox/task-board persistence, and runner side effects live behind module boundaries.
- Existing scheduler integration tests still pass without behavior drift.

Progress notes:
- Scheduler domain logic is split into `scheduler/decision.rs`, `scheduler/store.rs`, `scheduler/approval.rs`, and `scheduler/runner.rs`, with `scheduler.rs` acting as thin orchestration entry points.
- Scheduler tracing paths now use `TraceSession` helpers in decision/approval/runner modules instead of manual gateway event-sequence plumbing.
- Verified with focused scheduler and tracing runs: `cargo test -p autonoetic-gateway scheduler -- --nocapture` and `cargo test -p autonoetic-gateway test_trace_ordering_with_duplicate_event_seqs -- --nocapture`.

##### 4. Router spawn/ingest unification

- [ ] Extract one internal helper for the shared `agent.spawn` / `event.ingest` workflow: parse input, resolve session, emit gateway causal events, optionally append task-board metadata, and call the execution service.
- [ ] Stop reloading target agent data in router-only prechecks when the same path is reloaded again by the execution layer.
- [ ] Keep param parsing and user-facing response shapes distinct, but collapse the duplicated orchestration path.

Acceptance criteria:
- `agent.spawn` and `event.ingest` share one internal execution pipeline.
- Gateway causal logging shape remains unchanged externally.
- Tests continue covering both JSON-RPC methods independently.

##### 5. Lifecycle collaborator extraction

- [ ] Shrink `runtime/lifecycle.rs` by extracting at least `SessionTracer`, `ToolCallProcessor`, and reevaluation-state persistence helpers into focused collaborators.
- [ ] Keep the agent loop centered on: build completion request, invoke model, dispatch stop reason, apply reply filter.
- [ ] Preserve centralized disclosure, evidence, and causal logging semantics while moving plumbing out of the loop body.

Acceptance criteria:
- The main loop reads primarily as orchestration, not as a long list of audit/persistence details.
- Tool execution remains MCP-first, then native.
- Current lifecycle/disclosure/tool tests remain green.

##### 6. CLI split into focused command modules

- [ ] Split the Autonoetic CLI binary into focused modules such as `cli/gateway.rs`, `cli/agent.rs`, `cli/trace.rs`, `cli/chat.rs`, and `cli/mcp.rs`.
- [ ] Introduce small helper services where needed (`TraceStore`, `McpRegistry` wrappers, output formatters) instead of leaving all command behavior in `main.rs`.
- [ ] Keep the top-level clap surface unchanged while reducing the size and coupling of `main.rs`.

Acceptance criteria:
- `main.rs` becomes command wiring rather than command implementation.
- Trace, gateway, agent, MCP, and chat flows can be read in isolation.
- Existing CLI tests continue passing without flag or output regressions.

##### 7. Tool registry cleanup and shared path-tool helpers

- [ ] Remove dead or redundant registry types such as unused discovery metadata.
- [ ] Factor shared JSON argument parsing / path extraction for file-backed native tools.
- [ ] Keep the registry intentionally thin and static; do not turn it into a plugin framework.

Acceptance criteria:
- Path-based native tools do not duplicate argument parsing and metadata extraction logic.
- Registry code stays small and in-process only.
- Tool behavior and MCP/native precedence remain unchanged.
```
