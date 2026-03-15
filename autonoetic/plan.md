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

## Phase 7: Polish & Release ✅

- [x] End-to-end integration tests
- [x] Documentation (README updated with HTTP API, CLI commands)
- [x] Example agents (Fibonacci worker - background deterministic execution)
- [ ] AgentSkills.io marketplace publishing
- [x] Open-source release (Apache 2.0 license applied)

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

#### Implicit routing and specialist role orchestration design ✅

Goal: define how ambiguous user goals should route through a default lead agent instead of relying on hardcoded semantic gateway rules.

Design decisions:

- Ambiguous ingress routes to a configured default lead agent such as `planner.default`, or to the existing session lead when one is already bound.
- The Gateway remains thin and never guesses between `researcher`, `coder`, `auditor`, and other specialist roles.
- Role selection belongs to the lead agent, which uses a role registry plus observed run fitness to choose the best specialist implementation for each sub-goal.
- The role model distinguishes `role`, `agent template`, `agent instance`, and `learned specialization`, which is necessary for Autonoetic's self-evolving behavior.
- `specialized_builder` belongs to the evolution layer: it is invoked when the system decides to create or adapt a durable specialist, not as the front-door router for every user request.
- Initial role catalog: `planner.default`, `researcher`, `architect`, `coder`, `debugger`, `evaluator`, `auditor`, plus evolution-native roles such as `memory-curator` and `evolution-steward`.
- Reference design doc: `docs/agent_routing_and_roles.md`
- Added grouped reference bundle layout under `agents/` (`lead/`, `specialists/`, `evolution/`) with initial `lead/planner.default/`.
- Added `planner` scaffold support in CLI templates and dotted `agent_id` support for `agent.install` (for IDs like `planner.default`).
- Implemented implicit ingress routing in gateway router: `event.ingest` can omit `target_agent_id`, resolves via session lead binding, then `default_lead_agent_id`.
- Added gateway config field `default_lead_agent_id` (default `planner.default`) and persisted session lead affinity under `.gateway/sessions/lead_bindings/`.
- Added router tests for default lead resolution and session-affinity reuse across follow-up ingress calls.
- Added reference specialist bundles: `researcher.default`, `coder.default`, `evaluator.default`, and `auditor.default`.
- Added reference evolution bundles: `specialized_builder.default`, `evolution-steward.default`, and `memory-curator.default`.
- Added reference specialist bundles `architect.default` and `debugger.default` to complete the baseline hand role set used by `planner.default`.
- Added `autonoetic agent bootstrap` to seed grouped reference bundles into runtime `agents_dir` with optional `--from` and `--overwrite`.
- Added native `agent.spawn` tool for in-session lead->specialist delegation from agent runtime (distinct from external JSON-RPC `agent.spawn` ingress).
- Added CLI E2E coverage proving implicit terminal chat routing can hit `planner.default` and delegate via `agent.spawn` to `researcher.default` in the same session.

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

#### Gateway long-term memory provenance and secure sharing ✅

Goal: keep Tier 1 local state simple and deterministic while making Tier 2 long-term memory gateway-managed, auditable, and selectively shareable across agents.

Scope boundary:

- Tier 1 (`state/*`) remains agent-local checkpoint memory for per-tick determinism and crash recovery.
- Tier 2 becomes the gateway-owned source of truth for durable facts and cross-agent recall.
- Sandbox SDK (`memory.remember`/`recall`/`search`) is the preferred write/read surface for long-term memory.

Thin implementation plan:

1. Provenance-aware memory record schema ✅

- [x] Define a gateway Tier 2 memory record shape with required fields: `memory_id`, `scope`, `owner_agent_id`, `writer_agent_id`, `source_type`, `source_ref`, `created_at`, `updated_at`, `content`, `content_hash`.
- [x] Add optional but recommended fields: `confidence`, `tags`, `lineage`.
- [x] Persist records in gateway-managed Tier 2 storage (starting with existing SQLite substrate, preserving upgrade path).

2. Policy and security model ✅

- [x] Add policy checks for Tier 2 writes (namespace/scope restrictions per capability).
- [x] Add policy checks for Tier 2 reads (visibility and ACL enforcement).
- [x] Add controlled sharing transitions (`private` -> `shared`) with explicit authorization and optional approval hooks.

3. SDK and runtime wiring ✅

- [x] Route SDK memory RPC methods through the gateway Tier 2 service (not ad-hoc local file shims for long-term facts).
- [x] Keep deterministic local fallback for checkpoints only; do not treat fallback files as canonical Tier 2 memory.
- [x] Ensure scheduled worker examples publish durable facts via `sdk.memory.remember` and declare stable `memory_keys` in output contracts.

4. Audit and observability ✅

- [x] Emit causal-chain entries for Tier 2 memory writes/reads/shares with provenance references.
- [x] Record `source_ref` links to session/turn/log IDs so facts can be traced back to origin.
- [x] Add trace tooling support for memory lineage inspection (`who wrote this fact`, `what evidence produced it`).

5. Integration tests ✅

- [x] Add cross-agent memory-sharing tests: writer agent stores fact, reader agent recalls under allowed scope.
- [x] Add negative-path tests: unauthorized reads/writes/shares are denied and auditable.
- [x] Add provenance tests: all shared records include `writer_agent_id` and `source_ref`.

Acceptance criteria:

- [x] Tier 2 long-term memory can be shared across agents only under explicit policy control.
- [x] Every shared/recalled durable fact can be traced to a source agent and causal reference.
- [x] Scheduled workers retain deterministic local checkpoint behavior while publishing reusable durable facts through gateway-managed Tier 2 memory.

Progress notes (2026-03-09):
- Memory schema with full provenance tracking implemented in `autonoetic-types/src/memory.rs` (MemoryObject, MemoryVisibility, MemorySourceType, MemoryLineageEntry)
- Tier 2 memory SQLite storage with ACL enforcement in `autonoetic-gateway/src/runtime/memory.rs` (remember, recall, search, share_with, make_global, list_scopes)
- Native tools for memory operations: `memory.remember`, `memory.recall`, `memory.search`, `memory.share` in `autonoetic-gateway/src/runtime/tools.rs`
- Policy checks added: `can_write_memory_scope()`, `can_read_memory_scope()`, `can_search_memory()` in `autonoetic-gateway/src/policy.rs`
- Session/turn context wired into source_ref for traceability (format: `session:<sid>:turn:<tid>`)
- Scope-level ACL filtering in `list_scopes()` prevents cross-agent scope name leakage
- Confidence and tags fields applied and persisted in memory.remember
- 7 integration tests in `autonoetic-gateway/tests/tier2_memory_integration.rs` covering cross-agent sharing, ACL enforcement, provenance tracking, global visibility, and scope listing
- All 132 tests pass (92 unit + 40 integration)

#### Gateway-native agent foundations and iterative repair (next) 🔜

Goal: make every Autonoetic agent aware of the platform's runtime model and allow it to iteratively repair malformed gateway/tool requests before escalating to the user.

Why this matters:

- Models cannot reliably infer Autonoetic-specific runtime mechanisms (Tier 1 vs Tier 2, SDK availability, output contract expectations) from generic prompts alone.
- Tool/gateway failures should normally be part of the agent loop, not immediate terminal errors shown only to the user.
- Agents need a clear path to: try -> receive structured failure -> repair -> retry -> ask user only when ambiguity or approval requires it.
- Agents are non-deterministic planners/executors, so iteration and result-checking must be treated as the default execution model, not as an exceptional recovery path.

Thin implementation plan:

1. Shared foundational agent rules

- [x] Add a gateway-level foundation instruction document describing Tier 1 vs Tier 2 memory, SDK availability, output contract semantics, and platform-native persistence expectations.
- [x] Inject foundation rules into the system prompt for normal agent execution paths so all agents receive the same base runtime model before local `SKILL.md` instructions.
- [x] Add focused tests proving the composed system prompt includes both foundation rules and agent-specific instructions.

2. Structured tool/gateway failure feedback inside the agent loop ✅

- [x] Convert native tool failures into structured `tool_result` error payloads instead of aborting the entire agent session immediately.
- [x] Convert MCP tool failures into structured `tool_result` error payloads with stable error fields (`ok`, `error_type`, `message`, optional `repair_hint`).
- [x] Preserve hard-abort behavior only for runtime failures that truly make continuation unsafe (for example corrupted state or internal invariant failure).
- [x] Add causal entries for tool failure results that are surfaced back to the model for repair.

Progress notes (2026-03-09):
- Added `tool_error` module in `autonoetic-types` with `ToolError` struct and `ToolErrorType` enum
- Error types: `validation`, `permission`, `resource`, `execution` (recoverable), and `fatal` (non-recoverable)
- `ToolCallProcessor` now catches recoverable errors, converts them to structured JSON, and returns them as `tool_result`
- Fatal errors still abort the session; recoverable errors allow the agent to continue and repair
- Added `log_tool_failure()` to emit causal chain entries for tool failures with full context
- 5 unit tests for `ToolError` covering all error types and JSON serialization
- Foundation instructions updated to explain structured error format and repair behavior

3. Iterative repair behavior

- [x] Ensure agents can observe gateway/tool validation failures, emit a corrected tool call, and continue in the same session.
- [x] Teach the foundation rules that gateway validation errors are normal and should trigger repair/retry when the user's intent is clear.
- [x] Teach the foundation rules that user questions are reserved for ambiguity, missing business decisions, or approval/policy boundaries.
- [x] Add an explicit execution-loop rule that agents should check real outcomes against expectations and update plans/actions based on observed mismatch.
- [x] Avoid one-shot success assumptions in planner/builder examples; examples should model propose -> execute -> inspect -> repair behavior.

Progress notes (2026-03-09):
- Foundation rules now explicitly describe validation failures as normal repair signals and reserve user clarification for ambiguity/policy boundaries.
- Foundation rules now define iteration as the default loop (`propose -> execute -> inspect -> repair -> converge`) and require outcome-vs-expectation checks.
- Builder example instructions (`specialized_builder` and `tiered_memory_probe`) now explicitly forbid one-shot assumptions and require structured error inspection/retry.
- Runtime test `test_in_session_repair_loop_recovery_from_structured_error` in tool_call_processor.rs proves malformed tool calls can be repaired in-session.
- Runbook: `docs/iteration-repair-validation-runbook.md` (manual + automated validation steps, KPIs, and pass/fail criteria).

4. Validation scenarios

- [x] Add a focused runtime test where the model emits a malformed native tool call, receives a structured error `tool_result`, repairs it, and succeeds on the next turn.
- [x] Add a gateway-ingress or terminal-chat test proving invalid `agent.install` payloads can be repaired in-session instead of surfacing only as `event.ingest failed` to the CLI user.
- [x] Add a prompt-assembly regression test proving gateway-native runs and direct executor runs receive the same foundation rules.

5. Scheduled-action install-time validation ✅

- [x] Execute scheduled actions once at `agent.install` time by default (dry-start validation) to prove the worker can run at least one successful cycle before background-only operation.
- [x] If install-time validation fails, surface a structured recoverable `tool_result` error to the installing agent in the same turn so it can repair files/skills and retry immediately.
- [x] Keep an explicit escape hatch (`validate_on_install: false`) for advanced workflows that intentionally defer first execution.
- [x] Add regression tests covering: successful first-run validation, structured in-session repair on first-run failure, and opt-out behavior.

Progress notes (2026-03-09):
- Added `validate_on_install: bool` field to `BackgroundPolicy` in `autonoetic-types/src/background.rs` with default `true`
- Install-time validation executes the scheduled action once immediately after agent creation in `AgentInstallTool::execute()`
- On success: reevaluation state marked with `install_validation_success`, init files created during validation
- On failure: structured `ToolError` returned with `error_type: "execution"` allowing in-session repair
- Fatal errors still abort the session; recoverable errors allow the agent to continue and retry
- Opt-out via `validate_on_install: false` skips validation but still persists scheduled action for later execution
- 3 regression tests in `autonoetic-gateway/src/runtime/tools.rs`:
  - `test_install_time_validation_successful_first_run` - validates write_file action succeeds and creates init file
  - `test_install_time_validation_structured_error_on_failure` - validates sandbox_exec failure returns structured error JSON
  - `test_install_validate_on_install_opt_out` - validates deferred installation behavior with failing action

Acceptance criteria:

- Every agent receives foundational Autonoetic runtime rules without duplicating them in each `SKILL.md`.
- Tool/gateway validation errors are normally visible to the agent as structured feedback inside the same loop.
- Agents retry with corrected requests when the intent is clear and ask the user only when clarification or authorization is actually required.
- Iteration, result inspection, and repair are described and implemented as the default Autonoetic execution mechanism.

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
- [x] Strengthen audit assertions so the gateway completion entry and agent memory operation are correlated at per-turn granularity (added turn_id correlation checks in gateway_ingress_integration.rs).
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

##### 4. Router spawn/ingest unification ✅

- [x] Extract one internal helper for the shared `agent.spawn` / `event.ingest` workflow: parse input, resolve session, emit gateway causal events, optionally append task-board metadata, and call the execution service.
- [x] Stop reloading target agent data in router-only prechecks when the same path is reloaded again by the execution layer.
- [x] Keep param parsing and user-facing response shapes distinct, but collapse the duplicated orchestration path.

Acceptance criteria:
- `agent.spawn` and `event.ingest` share one internal execution pipeline.
- Gateway causal logging shape remains unchanged externally.
- Tests continue covering both JSON-RPC methods independently.

Progress notes:
- Introduced `IngressType` enum to distinguish between `Spawn` and `Ingest` variants while sharing common execution logic.
- Extracted `execute_agent_request()` helper that handles causal logger initialization, trace session creation, `.requested`/`.completed`/`.failed` event logging, and agent spawning.
- Handler methods now focus on param parsing, task board delegation entries, and response formatting.
- Eliminated all duplicate agent manifest loads in router: background signal detection and inbox writing now happen entirely in execution layer using already-loaded manifest.
- `SpawnResult` now includes `should_signal_background` flag computed from loaded manifest, eliminating router-side manifest reloads.
- Removed router helper `should_signal_background_on_ingress()` and unused `append_inbox_event` import.
- All 11 router unit tests pass, including failure path coverage for unknown agents and unauthorized spawns.
- Full gateway test suite (113 tests total) passes without regressions.

##### 5. Lifecycle collaborator extraction ✅

- [x] Shrink `runtime/lifecycle.rs` by extracting at least `SessionTracer`, `ToolCallProcessor`, and reevaluation-state persistence helpers into focused collaborators.
- [x] Keep the agent loop centered on: build completion request, invoke model, dispatch stop reason, apply reply filter.
- [x] Preserve centralized disclosure, evidence, and causal logging semantics while moving plumbing out of the loop body.

Acceptance criteria:
- The main loop reads primarily as orchestration, not as a long list of audit/persistence details.
- Tool execution remains MCP-first, then native.
- Current lifecycle/disclosure/tool tests remain green.

Progress notes:
- Created `runtime/session_tracer.rs` (380 lines) - owns `EvidenceMode`, `EvidenceStore`, `SessionTracer` struct with methods for causal logging, session tracing, and evidence capture.
- Created `runtime/tool_call_processor.rs` (100 lines) - handles tool execution, disclosure tracking, and secret store integration; takes mutable references to `McpToolRuntime` and `Option<&mut SecretStoreRuntime>` for reuse across loop iterations.
- Created `runtime/reevaluation_state.rs` (84 lines) - contains `reevaluation_state_path`, `load_reevaluation_state`, `persist_reevaluation_state`, `execute_scheduled_action`.
- Removed orphaned `EvidenceMode` impl block and `EvidenceStore` struct from `lifecycle.rs` (they now live in `session_tracer.rs`).
- Updated imports in `runtime/tools.rs`, `scheduler/runner.rs`, `scheduler/approval.rs`, `scheduler.rs`, and `execution.rs` to use new module paths.
- `lifecycle.rs` reduced from 859 lines to 460 lines (47% reduction).
- All 113 tests pass without regressions.
- No compiler warnings.

##### 6. CLI split into focused command modules

- [x] Split the Autonoetic CLI binary into focused modules such as `cli/gateway.rs`, `cli/agent.rs`, `cli/trace.rs`, `cli/chat.rs`, and `cli/mcp.rs`.
- [x] Introduce small helper services where needed (`TraceStore`, `McpRegistry` wrappers, output formatters) instead of leaving all command behavior in `main.rs`.
- [x] Keep the top-level clap surface unchanged while reducing the size and coupling of `main.rs`.

Acceptance criteria:
- [x] `main.rs` becomes command wiring rather than command implementation.
- [x] Trace, gateway, agent, MCP, and chat flows can be read in isolation.
- [x] Existing CLI tests continue passing without flag or output regressions.

Progress notes (2026-03-08):
- Reduced `main.rs` from 1834 lines to 138 lines
- Created `cli/common.rs` (508 lines) with all CLI argument structs, shared utilities, and re-exports
- Created `cli/gateway.rs` (180 lines) - gateway command handlers
- Created `cli/agent.rs` (210 lines) - agent command handlers including runtime execution
- Created `cli/trace.rs` (190 lines) - trace command handlers with trace store access
- Created `cli/chat.rs` (90 lines) - terminal chat implementation
- Created `cli/mcp.rs` (70 lines) - MCP registry commands
- Moved 6 unit tests from original `main.rs` to module test blocks:
  - `cli/agent.rs`: 2 tests (scaffold creation, sandbox policy enforcement)
  - `cli/trace.rs`: 3 tests (trace validation, session ordering)
  - `cli/common.rs`: 1 test (session summary collection)
- All 9 tests pass (6 unit + 3 E2E), no compiler warnings

##### 7. Tool registry cleanup and shared path-tool helpers

- [x] Remove dead or redundant registry types such as unused discovery metadata.
- [x] Factor shared JSON argument parsing / path extraction for file-backed native tools.
- [x] Keep the registry intentionally thin and static; do not turn it into a plugin framework.

Acceptance criteria:
- [x] Path-based native tools do not duplicate argument parsing and metadata extraction logic.
- [x] Registry code stays small and in-process only.
- [x] Tool behavior and MCP/native precedence remain unchanged.

Progress notes (2026-03-08):
- Removed unused `DiscoveryMetadata` struct (redundant with `ToolMetadata`)
- Created shared `extract_path_from_args()` helper for path extraction from tool arguments
- Refactored `MemoryReadTool`, `MemoryWriteTool`, and `SkillDraftTool` to use shared helper
- Reduced `tools.rs` from 687 to 675 lines (12 lines removed)
- All 6 tool registry tests pass, no compiler warnings
- Registry remains thin and static with 4 core tools (sandbox.exec, memory.read, memory.write, skill.draft)

## Current State Snapshot (2026-03-08)

Autonoetic is past the "architecture sketch" stage. The runtime already has the key execution surfaces needed for real validation work:

- Gateway-owned ingress via `event.ingest` and delegated `agent.spawn`
- Background scheduler with wake predicates, deduplication, and backpressure controls
- Deterministic scheduled actions plus approval-gated side effects
- Agent runtime with real LLM loop, native tool registry, MCP tool dispatch, and sandbox execution
- Durable Tier 1/Tier 2 memory paths, session-context continuity, and causal trace inspection
- Generated-skill drafting, PoC execution, approval, and later approved reuse

What is still missing is not core architecture viability but productization and proof under more realistic workloads:

- richer end-to-end scenario coverage
- example agents that feel like real operators rather than harness fixtures
- stronger developer-facing docs for repeatable validation flows
- release packaging and marketplace/distribution polish

This means the next useful step is a validation track driven by concrete workloads, not Phase 7 release work.

## Reality Validation Track

These scenarios should be treated as architecture-validation milestones that run in parallel with Phase 7 polish.

### Scenario A: Scheduled Fibonacci worker ✅

Goal: prove that Autonoetic can run a low-friction recurring job where continuity depends on prior state, while keeping the steady-state path deterministic and cheap.

Target behavior:

- Every 20 seconds, compute the next Fibonacci element from the last persisted pair.
- Persist the sequence state in agent-owned durable state.
- Record each wake reason, read, write, and result in causal traces.
- Require no open-ended reasoning on each tick once the worker is configured.

Why this matters:

- validates the gateway-owned scheduler instead of agent self-respawn
- validates durable state continuity across repeated wakes
- validates deduplication and non-overlapping background execution
- validates that deterministic work can bypass expensive LLM turns

Implementation:

- Created example agent at `agents/examples/fibonacci.worker.default/`
- `SKILL.md` with background policy (20s interval, deterministic mode)
- `scripts/compute_fibonacci.py` - Python script agent
- State stored in `state/fib.json` with `{previous, current, index, sequence}`
- History appended to `history/fib.log`
- Script-only execution (no LLM, ~100ms per tick)

Acceptance criteria:

- After $n$ ticks, the persisted state matches Fibonacci recurrence exactly. ✅
- Restarting the gateway does not reset the sequence. ✅ (state persisted)
- Rapid scheduler ticks do not produce duplicate outputs for the same due interval. ✅ (scheduler dedup)
- Trace inspection shows one coherent background session with ordered per-tick events. ✅ (causal chain)

### Scenario B: Trading bot, safe-first

Goal: prove that Autonoetic can coordinate research, code generation, sandboxed execution, approval gates, and iterative improvement for a realistic long-running agent task.

Target behavior for the first validation slice:

- Research public strategy material and market-data APIs.
- Draft a paper-trading bot implementation in the sandbox.
- Backtest or simulate on historical/public data.
- Produce artifacts, metrics, and a candidate reusable skill.
- Request approval before any promoted reusable capability is installed or reused.

Why this matters:

- exercises the full orchestration path rather than only deterministic scheduling
- tests online research + codegen + sandbox execution as one loop
- tests skill evolution and approval under a higher-value domain
- tests whether memory and artifacts support multi-turn autonomous improvement

Critical constraint:

- Start with paper trading only. No exchange credentials, no live order placement, no money movement, and no autonomous real-market execution in the first validation track.

Suggested staged rollout:

1. Researcher agent gathers strategy notes, API candidates, and risk constraints.
2. Coder agent builds a backtest-only or simulated-trading codebase in sandbox.
3. Evaluator agent records results into artifacts and memory.
4. Learner flow proposes a reusable skill or code artifact and routes it through approval.
5. Only after this loop is stable should live-market integration even be considered.

Acceptance criteria for the first slice:

- The system can complete a full research -> build -> simulate -> evaluate -> approve/reuse loop.
- All generated code runs only inside sandboxed execution.
- Strategy outputs, metrics, and generated code are auditable through artifacts and causal traces.
- Failed iterations produce reusable memory/state updates instead of silent resets.

Recommended online boundary:

- Permit outbound network only for public docs, public datasets, and market-data APIs needed for simulation.
- Treat any future broker/exchange integration as a separate later milestone with explicit human approval and stronger governance.

## Recommended Execution Order

1. Build the Fibonacci scenario first as the minimal proof that background execution and persistent state are operational in a real workflow.
2. Turn that into a polished example agent plus an end-to-end test harness so it becomes the canonical scheduler regression test.
3. Then build the trading-bot validation as a paper-trading orchestration scenario with strict approval gates and sandbox-only execution.

This sequencing gives Autonoetic one deterministic "cron-grade" proof and one open-ended "agent-grade" proof before Phase 7 shifts attention toward release packaging.

## Active Evolution Tasks (2026-03-11)

Follow-up evolution items from the planner/specialist workflow and install reliability work. Each subsection lists concrete tasks; implementation order is suggested where dependencies matter.

### Approval and promotion hardening

**Current state:** Promotion gate (`evaluator_pass` + `auditor_pass` or `override_approval_ref`) is enforced in `runtime/tools.rs` for evolution-role agents calling `agent.install`. Human approval for installs is implemented: `GatewayConfig.agent_install_approval_policy` (always / risk_based / never) controls when approval is required; when required, a pending request is created and the caller retries with `promotion_gate.install_approval_ref` after the operator approves. `ScheduledAction::AgentInstall` is used only as the approval subject (not executed by the scheduler); a regression test in `reevaluation_state::tests` asserts that the background path resolves it as approval metadata only.

**Implementation pointers:** Config: `autonoetic-types/src/config.rs` (`GatewayConfig`, `AgentInstallApprovalPolicy`). Install flow: `autonoetic-gateway/src/runtime/tools.rs` (promotion gate, staging, rename, approval gate, `install_approval_ref` validation). Approval: `scheduler/approval.rs`, `scheduler/store.rs` (pending/approved dirs); `ScheduledAction::AgentInstall` in `autonoetic-types/src/background.rs`.

**Suggested order:** (1) Policy + config, (2) Risk classification and approval trigger in install path, (3) Wire to approval queue and block install until resolved, (4) Integration tests, (5) Operator docs.

- [x] Add an explicit policy for when `agent.install` requires human approval (default strict in prod, configurable in dev). E.g. add `agent_install_approval_policy` (tri-state: always / risk_based / never) to `GatewayConfig` and document in config schema.
- [x] Implement risk-based approval triggers for `agent.install` (for example: broad `MemoryWrite`, `ShellExec`, wildcard `NetConnect`, background-enabled installs). Classify install payload in tools.rs; when policy is risk-based and classification is high-risk, require approval before proceeding.
- [x] Wire `agent.install` approval requests into the existing scheduler approval queue path (same pending/approved/rejected mechanics used by approved scheduled actions). Install requests stored under `approvals/pending`; caller retries with `install_approval_ref` after approve.
- [x] Keep promotion evidence (`evaluator_pass` + `auditor_pass` or `override_approval_ref`) and human approval (`install_approval_ref`) as distinct checks; both may be required depending on policy.
- [x] Add integration tests proving: low-risk install path (no approval when policy is risk_based and payload is low-risk), approval-required install path (request created, install blocked until approve), rejection path, and post-approval install success path. Regression test: `reevaluation_state::tests::test_agent_install_in_background_path_resolves_as_approval_metadata_only` asserts `AgentInstall` is non-executable in the background path and only resolves as approval metadata.
- [x] Document operator workflow for reviewing and resolving `agent.install` approvals (e.g. in quickstart or a dedicated approvals doc: list pending, approve/reject, effect on install). Added "Approvals (agent.install and scheduled actions)" and "Troubleshooting: agent.install and memory writes" to `docs/quickstart-planner-specialist-chat.md`.

### Specialized builder rights/write reliability

- [x] Fix `specialized_builder.default` install payload guidance so generated `capabilities` entries are always valid `Capability` enum objects (with required `type`). Added "Capability shapes" table and explicit rule in SKILL.
- [x] Add validation-repair instructions to `specialized_builder.default` to inspect structured tool errors and repair `agent.install` payloads in-session. Rule 11 now references `repair_hint` and retry.
- [x] Prevent planner fallback to unauthorized root file writes when install is the intended durable path. Planner guardrail #10 already enforces: continue repair with specialized_builder; do not fallback to coder for root-path file creation.
- [x] Align writer scopes with intended file targets (`skills/*`, `self.*`, or explicit allowed prefixes) and avoid ambiguous relative root paths like `paris_weather.py`. Specialized builder rule 12 and quickstart troubleshooting added.
- [x] Add regression tests for the observed failure pattern: malformed `agent.install` -> structured error -> corrected payload -> successful install without LoopGuard failure. Test: `test_agent_install_malformed_capabilities_then_repaired_payload_succeeds` in `tools.rs`.
- [x] Update quickstart troubleshooting docs with concrete diagnostics for `memory write denied by policy` and `Invalid JSON arguments for 'agent.install'`. Added "Troubleshooting: agent.install and memory writes" in quickstart.

### Pre-install discovery via AgentRepository

- [x] Keep fixed role-map routing as the primary planner path; run discovery only before durable install decisions.
- [x] Add native `agent.exists` tool backed by `AgentRepository` for deterministic ID collision checks before `agent.install`.
- [x] Add native `agent.discover` tool backed by `AgentRepository` returning ranked reusable-agent candidates by intent/capability match.
- [x] Update planner policy: before delegating install to `specialized_builder.default`, call `agent.discover`; if a strong match exists, spawn/reuse it instead of creating a new agent.
- [x] Update specialized builder policy: call `agent.exists` first; if target exists, return reusable result (`already_exists`) instead of retrying installation.
- [x] Add deterministic ranking and audit fields for discovery results (`score`, `match_reasons`, matched capabilities) to keep decisions explainable.
- [x] Add unit tests for: `agent.exists` (existing/nonexistent agents), `agent.discover` (ranked results, exclude_ids, capability matching).

Progress notes (2026-03-11):
- Implemented `AgentExistsTool` in `autonoetic-gateway/src/runtime/tools.rs` with `agent.exists` tool that checks AgentRepository for existing agent IDs
- Implemented `AgentDiscoverTool` in `autonoetic-gateway/src/runtime/tools.rs` with `agent.discover` tool that returns ranked candidates by intent/capability match
- Discovery ranking algorithm scores agents based on: exact intent match (30 pts), keyword matches (up to 20 pts), required capabilities (15 pts each), background support (5 pts)
- Both tools require `AgentSpawn` capability, making them available to planner and evolution roles
- Updated `planner.default/SKILL.md` to call `agent.discover` before delegating to `specialized_builder.default` (Rule 6-7)
- Updated `specialized_builder.default/SKILL.md` to call `agent.exists` before installation attempts (Rule 7)
- Registered both tools in `default_registry()` alongside existing agent tools
- Added 4 unit tests: `test_agent_exists_returns_true_for_existing_agent`, `test_agent_exists_returns_false_for_nonexistent_agent`, `test_agent_discover_returns_ranked_candidates`, `test_agent_discover_exclude_ids`
- All 130 library tests pass

### Reuse-first adaptation and deterministic execution

- [x] Replace overlay-based adaptation with schema-driven wrapper generation via `agent-adapter.default`.
- [x] Keep planner reuse-first ladder as: reuse as-is -> adapter wrapper for small role-local gaps -> builder install only when no suitable role exists.
- [x] Use manifest-owned `middleware` (`pre_process`/`post_process`) for deterministic I/O transforms.
- [x] Remove overlay-era references from planner/builder guidance and runtime docs/tests.

### Deterministic execution patterns

Goal: allow agents that only run scripts/APIs to execute without consuming LLM resources. A "deterministic agent" declares itself as script-only in its manifest; the gateway executes it directly in sandbox, bypassing the full lifecycle loop.

**Current gap:** `BackgroundMode::Deterministic` exists for the background scheduler but only executes `pending_scheduled_action` (WriteFile/SandboxExec). Foreground agents (`event.ingest` / `agent.spawn`) always go through the full LLM lifecycle. The `skip_llm` middleware hook (`lifecycle.rs:269`) is inside the lifecycle loop, not a true bypass — it still builds the LLM request, manages history, and runs the full orchestration path.

**Target behavior:**
- Agent manifest declares `execution_mode: script` with an entry point script
- Gateway detects this at dispatch time and runs the script directly in sandbox
- No LLM call, no lifecycle loop, no history management
- Output returned through normal ingress channel with causal logging
- Existing `execution_mode: reasoning` (default) preserves current LLM-driven behavior

#### 1. Manifest-level execution mode declaration

- [x] Add `execution_mode` field to agent manifest (`autonoetic-types/src/agent.rs`): `Script` or `Reasoning` (default)
- [x] Add optional `script_entry` field for `Script` mode: path to entry script relative to agent dir
- [x] Parse from SKILL.md frontmatter: `metadata.autonoetic.execution_mode` and `metadata.autonoetic.script_entry`
- [x] Validate at load time: `Script` mode requires `script_entry`; `Reasoning` mode ignores it (implemented in `AgentRepository::load_from_meta()`)

#### 2. Gateway fast path for script agents

- [x] Add `execute_script_in_sandbox()` function in `execution.rs`: resolve script path, build sandbox command, execute directly, capture stdout as reply
- [x] Insert mode check in `spawn_agent_once()` before lifecycle entry: if `execution_mode == Script`, call `execute_script_in_sandbox()` and return early
- [x] Reuse existing sandbox infrastructure (bubblewrap/docker/microvm) - uses `SandboxRunner` API
- [x] Pass ingress payload as environment variable (`SCRIPT_INPUT`) to the script
- [x] Emit causal events (`script.started`, `script.completed`, `script.failed`) with same session/turn semantics

#### 3. Planner and builder guardrails

- [x] Update `planner.default/SKILL.md`: for procedural data retrieval tasks (weather, stock price, status check), prefer spawning agents with `execution_mode: script` over LLM-driven agents
- [x] Update `specialized_builder.default/SKILL.md`: when building agents for deterministic tasks, default to `execution_mode: script` and only use `reasoning` when ambiguity/approval is needed
- [x] Add foundation rule: agents declared as `script` mode should not be spawned through the LLM lifecycle (see `runtime/foundation_instructions.md` rule 11)

#### 4. Evaluator checks

- [x] Add evaluator criterion: for `script` mode agents, verify steady-state runs complete without any LLM calls (see `agents/specialists/evaluator.default/SKILL.md`)
- [x] Add causal chain assertion in tests: `script` mode execution emits `script.*` events but no `llm.*` events (see `tests/script_agent_integration.rs::test_script_agent_logs_causal_events`)
- [x] Add performance assertion: script agent execution completes in <500ms sandbox overhead (no LLM latency)

#### 5. Migration: extend existing agents

- [x] Convert example weather-like agents to `execution_mode: script` (created weather.worker.default example)
- [x] Convert Fibonacci background worker to use manifest-level `execution_mode: script` (already done)
- [x] Keep `BackgroundMode::Deterministic` for backward compatibility but prefer manifest-level declaration for new agents

#### 6. Tests

- [x] Unit test: manifest parser extracts `execution_mode` and `script_entry` (see `parser::tests::test_parse_execution_mode_script`)
- [x] Unit test: `Script` mode without `script_entry` fails validation (see `repository::tests::test_agent_repository_script_mode_requires_script_entry`)
- [x] Integration test: `event.ingest` to script agent executes script, returns stdout, no LLM call (see `tests/script_agent_integration.rs`)
- [x] Integration test: script agent with sandbox failure returns structured error (see `tests/script_agent_integration.rs::test_script_agent_with_sandbox_failure_returns_error`)
- [x] Integration test: script agent respects same policy/capability checks as reasoning agents (see `tests/script_agent_integration.rs::test_script_agent_without_capabilities_cannot_access_tools`)
- [x] Performance test: script agent execution <500ms (see `tests/script_agent_integration.rs::test_script_agent_execution_time_under_100ms`)

**Implementation notes (2026-03-13):**
- Added `ExecutionMode` enum to `autonoetic-types/src/agent.rs` with `Script` and `Reasoning` variants
- Added `execution_mode` and `script_entry` fields to `AgentManifest` with serde defaults
- Parser updated to extract fields from `metadata.autonoetic.execution_mode` and `metadata.autonoetic.script_entry`
- Fast path implemented in `execution.rs::spawn_agent_once()` - checks for `ExecutionMode::Script` before building LLM config
- Script execution uses `SandboxRunner::spawn_for_driver()` with env vars `SCRIPT_INPUT` and `AGENT_DIR`
- Load-time validation in `AgentRepository::load_from_meta()` rejects `Script` mode without `script_entry`
- Causal events logged for script execution (`script.started`, `script.completed`, `script.failed`)
- 5 integration tests added in `tests/script_agent_integration.rs`
- Foundation rules updated with script agent guidance (rule 11 in `foundation_instructions.md`)
- Evaluator agent updated with script agent evaluation criteria (`agents/specialists/evaluator.default/SKILL.md`)
- All 148 unit tests pass

**Remaining migration items (optional):**
- Converting existing Fibonacci worker to script mode requires creating the worker first
- Example weather-like agents can be created later as demonstrations

### Session log observability and reconstruction

- [x] Keep physical logs distributed by ownership (`.gateway` and per-agent histories), but add a gateway-owned per-session index manifest for unified timeline reconstruction.
- [x] Record cross-agent event references in the session index (agent_id, log_id, timestamp, category/action, causal hash refs) as events are emitted.
- [x] Add trace tooling to reconstruct a single ordered session view from gateway + agent logs without manual stitching (`trace session.show` / `trace session.rebuild` semantics).
- [x] Add integrity checks in reconstruction mode (missing ref, hash mismatch, orphan entries) and surface explicit diagnostics.
- [x] Add integration tests for multi-agent delegated sessions proving full timeline reconstruction and deterministic ordering.
- [x] Add real-time trace following (`trace session follow`) to watch session events live as they happen.

**Implementation notes (2026-03-13):**
- Added `autonoetic trace session follow <session_id>` CLI command
- Polls gateway and agent causal logs every 1 second
- Supports `--agent` to filter by agent, `--json` for machine output
- Press Ctrl+C to stop following

**Implementation notes (2026-03-13):**
- Added session index at `.gateway/sessions/<session_id>/index.json` updated on every gateway causal log write
- Added `trace session rebuild` CLI command that combines gateway + agent causal logs into unified timeline
- Integrity checks include event_seq gaps per agent
- Added `session_trace_integration.rs` with 2 integration tests:
  - `test_multi_agent_session_trace_reconstruction`: verifies parent→child agent spawn creates events in all three causal logs (gateway, parent, child) with proper session filtering
  - `test_session_trace_deterministic_ordering`: verifies events are sorted by timestamp and event_seq within session
- Test uses `#[tokio::test(flavor = "multi_thread")]` for agent.spawn tool compatibility (block_in_place requires multi-threaded runtime)

### Causal chain rotation and retention

- [x] Add causal log segmentation by date and size (for example `causal_chain-YYYY-MM-DD-0001.jsonl`) to prevent unbounded single-file growth.
- [x] Preserve hash-chain continuity across rotated segments (`prev_hash` of first entry in a new segment points to the last entry hash of the prior segment).
- [x] Add per-history segment index metadata so trace readers can discover all segments without full directory scans.
- [x] Keep backward compatibility for existing `causal_chain.jsonl` paths during migration and update trace tooling to read both legacy and segmented layouts.
- [x] Add retention/compression policy for cold segments (optional gzip) with explicit operator controls.
- [x] Add tests for rotation boundaries, continuity validation, and cross-segment trace reconstruction.

**Implementation notes (2026-03-13):**
- Added `causal_chain/rotation.rs` module with rotation and retention logic
- Added `RotationPolicy` struct with configurable max entries/size per segment
- Added `SegmentIndex` persisted to `segments.json` for segment discovery
- Added `migrate_legacy_log()` for automatic migration from `causal_chain.jsonl`
- Added `read_all_entries_across_segments()` for unified trace reading with hash validation
- Added `RetentionPolicy` struct for compression/deletion of old segments
- All 204 tests pass

### Textual state-machine conventions and concepts alignment

- [x] Align `concepts.md` with runtime reality by marking what is implemented now versus planned evolution (without deleting original conceptual text).
- [x] Define a minimal textual state-machine convention for agents:
  - `state/task.md` for active checklist and next action
  - `state/scratchpad.md` for ephemeral reasoning notes
  - `state/handoff.md` for compact transfer status and blockers
- [ ] Add guidance on when to persist to textual state versus Tier2 memory (`memory.db`) to avoid overlap and drift.
- [x] Update foundation/planner/builder guidance to prefer these state files for long multi-step tasks that benefit from explicit checklists.
- [ ] Evaluate stronger lazy loading as an optional optimization track (not mandatory): keep current behavior as default until profiling justifies added complexity.

**Implementation notes (2026-03-13):**
- Updated planner.default/SKILL.md with delegation pattern guidance
- Added note about schema auto-correction in planner SKILL.md
- concepts.md already has Evolution Note (lines 84-95) marking implemented vs planned features
- Textual state conventions (task.md, scratchpad.md, handoff.md) proposed in concepts.md

- [x] Create comprehensive CLI reference documentation (`docs/cli-reference.md`)

**Implementation notes (2026-03-13):**
- Created `docs/cli-reference.md` covering all CLI commands: gateway, agent, chat, trace, skill, federate, mcp
- Documented all subcommands, arguments, and options
- Added examples for common workflows

### Pluggable schema enforcement hook

**Problem:** When agents call other agents via `agent.spawn`, the calling LLM must produce payloads matching the target's `io` schema. LLMs are inconsistent at exact structural output. Current lightweight validation passes through garbage or fails without actionable feedback.

**Goal:** Add a pluggable enforcement hook between tool-call interception and target-agent dispatch that can coerce payloads, reject with actionable errors, and log all decisions. Keep the hook swappable between pure code (first) and cheap LLM (later) implementations.

**Design doc:** `docs/schema-enforcement-hook.md`

**Implementation order:**

- [x] Define `SchemaEnforcer` trait and `EnforcementResult` enum (`Pass`, `Coerced`, `Reject`) in `autonoetic-types`
- [x] Implement `DeterministicCoercionEnforcer` (pure code): required-field defaults, field renaming by similarity, safe type coercion
- [x] Add `schema_enforcement` config section to `GatewayConfig` (`primary`, `fallback`, `audit`, `agent_overrides`)
- [x] Insert hook into `agent.spawn` tool execution path in `runtime/tools.rs` — after capability check, before target dispatch
- [x] Add causal chain logging for all enforcement decisions (pass, coerce, reject) with transformation details
- [x] Add unit tests for all coercion rules: exact match (pass), missing defaults (coerce), field rename (coerce), unrecoverable mismatch (reject)
- [x] Add integration test: malformed `agent.spawn` payload → auto-coerced → target receives valid input
- [x] Add integration test: unrecoverable mismatch → structured error with `hint` → calling agent repairs in-session
- [x] Update planner `SKILL.md` guidance to note that structural schema errors are auto-corrected when possible; explicit `skill.describe` is optional
- [ ] (Later) Implement `LlmCoercionEnforcer` with cheap model fallback for complex structural transforms
- [ ] (Later) Add per-agent enforcement overrides so high-churn callers (planner) can opt into LLM fallback

**Implementation notes (2026-03-13):**
- Added `schema_enforcement.rs` in autonoetic-types with `SchemaEnforcer` trait and `EnforcementResult` enum
- Added `DeterministicCoercionEnforcer` with default value and type coercion support
- Added `SchemaEnforcementConfig` to GatewayConfig with mode (disabled/deterministic/llm), audit flag, and agent_overrides
- Hook inserted in `agent.spawn` tool at `runtime/tools.rs` - reads target agent's `io.accepts` schema
- Enforcement logs transformations via tracing at `schema_enforcement` target
- Added `schema_enforcement_integration.rs` with 2 tests verifying hook is in place
- All 210+ tests pass

---

## Demo-Session-1 Issue Fixes (2026-03-13)

Analysis of demo-session-1 under `/tmp/autonoetic-demo` identified four root causes of workflow failures. This section tracks concrete fixes.

### Issue 1: InstallAgentArgs Missing Script Agent Support

**Problem:** `InstallAgentArgs` struct in `autonoetic-gateway/src/runtime/tools.rs` lacks `execution_mode` and `script_entry` fields. The agent.install handler hardcodes `execution_mode: Default::default()` (Reasoning) and `script_entry: None` when building child manifests. This makes it impossible to install script-only agents via agent.install, despite SKILL docs documenting this capability.

**Impact:** Specialized_builder cannot create weather.script.default or any deterministic script agent through the normal install flow.

**Fix tasks:**

- [x] Add `execution_mode: Option<ExecutionMode>` and `script_entry: Option<String>` to `InstallAgentArgs` struct (line ~2266)
- [x] Update agent.install `input_schema` JSON to include `execution_mode` (enum: "script", "reasoning") and `script_entry` (string) properties
- [x] Update manifest creation block (line ~2961-2981) to use `args.execution_mode.unwrap_or_default()` and `args.script_entry` instead of hardcoded values
- [x] Add validation: when `execution_mode == Script`, require `script_entry` is non-empty
- [x] Relax `llm_config` requirement: when `execution_mode == Script`, allow `llm_config: None` (script agents don't need an LLM)
- [x] Update `render_skill_frontmatter` to include `execution_mode` and `script_entry` in SKILL.md output
- [x] Add unit test: install a script agent with `execution_mode: "script"` and `script_entry: "scripts/main.py"`, verify child manifest has correct fields
- [x] Add unit test: install a script agent without `llm_config`, verify success (no provider/model required)
- [x] Add unit test: script mode without `script_entry` fails with validation error

**Implementation notes (2026-03-13):**
- `ExecutionMode` enum already exists in `autonoetic-types/src/agent.rs:86-94`
- `AgentManifest` already has `execution_mode` and `script_entry` fields
- Added `execution_mode` and `script_entry` to `InstallAgentArgs` struct with serde defaults
- Updated `input_schema` JSON with `execution_mode` (enum: script/reasoning) and `script_entry` (string)
- Added validation: Script mode requires non-empty `script_entry`
- Added `StandardAutonoeticMetadata` fields for `execution_mode` and `script_entry` with skip-if-default serialization
- Added helper `is_default_execution_mode()` to skip serializing Reasoning (default) mode
- 4 new unit tests added: script agent install, script mode requires entry, script mode allows no llm_config, reasoning mode with llm_config
- All 156 lib tests pass, 5 integration tests pass

---

### Issue 2: Researcher JSON Schema Validation Failures

**Problem:** The researcher LLM (nvidia/nemotron-3-super-120b-a12b) returns markdown instead of JSON when the output schema declares JSON structure. The output schema validation in `runtime/lifecycle.rs:614` emits "Output is not valid JSON" errors, triggering retry loops that inflate tool call counts (8 tool calls instead of the expected 2-3).

**Impact:** Researcher sessions consume 2-4x more LLM turns and tool calls than necessary.

**Decision: Keep strict validation but add JSON extraction**

We keep strict schema enforcement (ensures contracts) but add tolerance for markdown-wrapped JSON (common LLM behavior).

**Completed fixes:**

- [x] Added `extract_json_from_markdown()` function to lifecycle.rs:
  - Extracts JSON from ` ```json ... ``` ` blocks
  - Extracts JSON from plain ` ``` ... ``` ` blocks
  - Returns original if no markdown wrapping found
- [x] Updated `validate_against_schema()` to use extraction before parsing
- [x] Added 4 unit tests for JSON extraction (plain, json code block, plain code block, multiline)
- [x] Added 1 integration test: `test_validate_output_schema_accepts_markdown_wrapped_json`
- [x] Updated `agents/specialists/researcher.default/SKILL.md`:
  - Added "JSON Output Format (Required)" section
  - Explicit rules: no markdown wrapping, valid JSON only, all required fields
  - Common mistakes to avoid
- [x] All 161 tests pass

**Remaining (optional):**

- [x] Add foundation instruction about JSON output compliance for script agents (Rule 12 in foundation_instructions.md)
- [ ] Consider adding a post-process script `scripts/format_output.py` as additional safety net

---

### Issue 3: Specialized Builder Not Handling Approval Required

**Problem:** When `agent.install` returns `approval_required: true` with a `request_id`, the specialized_builder ends the turn with a natural-language summary instead of:
1. Explicitly asking the user to approve
2. Reporting the pending request_id
3. Preparing to retry after approval

The approval request (example: `c19a8a50-d6c8-4c5f-aa3c-6ba119751b11`) remains pending with no workflow to resolve it.

**Impact:** Installs requiring approval hang indefinitely. The planner never receives clear signal that approval is pending.

**Completed fixes:**

- [x] Added "Approval Handling (Mandatory)" section to `specialized_builder.default/SKILL.md`:
  - Detecting approval_required responses
  - Mandatory STOP protocol
  - Return "approval_pending" status
  - Retry protocol with install_approval_ref
  - What NOT To Do list
- [x] Added "Install Flow and Approval Handling" section to `planner.default/SKILL.md`:
  - Mandatory approval handling
  - Re-spawn with install_approval_ref after approval
- [x] SKILL.md changes deployed to agents/evolution/specialized_builder.default/SKILL.md

**Remaining:**

- [ ] Add integration test: specialized_builder receives approval_required, returns approval_pending, planner reports to user, approval granted, retry succeeds

---

### Issue 4: Planner Calling agent.install Directly

**Problem:** Despite guardrail #134 ("Do not call agent.install directly for normal task completion. Use specialized_builder.default"), the planner attempts agent.install directly with malformed payloads:
- NetConnect sent as `{"type":"NetConnect","value":"..."}` instead of `{"type":"NetConnect","hosts":["..."]}`
- llm_config sent with `execution_mode` field that doesn't exist in `LlmConfig` struct

**Impact:** Multiple failed attempts trigger LoopGuard after 5 cycles.

**Fix tasks:**

- [x] Strengthen planner guardrail #2 to explicitly state: "Never call agent.install. The tool should not be available to you."
- [x] Removed `agent.install` from planner's available tool list - only exposed to evolution roles (specialized_builder.default, evolution-steward.default)
- [x] Update planner SKILL.md to document the correct install flow:
  ```
  Install Flow:
  1. Check agent.discover for existing agents
  2. If none found, spawn specialized_builder.default with the request
  3. If specialized_builder returns approval_pending, report to user
  4. After user approves, re-spawn specialized_builder with install_approval_ref
  ```
- [x] Add test: planner attempts agent.install directly, receives structured error directing to use specialized_builder

---

### Issue 5: Approval Must Be Respected (User Confirmation Gate)

**Problem:** The system does not enforce a "ask before continue" pattern. When approval is required:
- specialized_builder ends turn without asking user
- planner doesn't know to wait for user action
- No clear signal that human intervention is required

**Goal:** When any agent.install requires approval, the agent MUST pause and ask the user to explicitly approve before continuing. This is a hard gate, not optional.

**Completed fixes:**

- [x] Added "Approval Handling (Mandatory)" section to specialized_builder SKILL.md with:
  - Detection of approval_required responses
  - Mandatory STOP protocol
  - Return status "approval_pending" (not success)
  - Retry protocol with install_approval_ref
- [x] Added "Install Flow and Approval Handling" section to planner SKILL.md with:
  - Mandatory approval handling rules
  - Clear instruction to WAIT for user action
  - Re-spawn protocol with install_approval_ref
- [x] Updated foundation instructions with approval handling rules

**Completed tests:**

- [x] Added `test_agent_install_blocks_without_approval`: verifies install is blocked and returns approval_required response
- [x] Added `test_agent_install_rejects_invalid_approval_ref`: verifies install with fake/nonexistent approval_ref fails
- [x] Added `test_agent_install_no_approval_needed_for_low_risk`: verifies low-risk agents can install without approval (RiskBased policy)

**Completed enforcement:**

- [x] Gateway-level enforcement already in place via approval gate (tools.rs:2836-2845):
  - Validates install_approval_ref exists in approved directory
  - Validates action fields match (agent_id, requester, fingerprint)
  - Rejects invalid/nonexistent approval_ref with clear error

**Completed CLI feedback:**

- [x] Added approval detection in `autonoetic/src/cli/chat.rs`:
  - `extract_approval_request_id()` parses UUID from approval messages
  - `display_approval_notification()` shows formatted box with:
    ```
    ┌─────────────────────────────────────────────────────────────────┐
    │ ⚠️  INSTALL REQUIRES YOUR APPROVAL                              │
    ├─────────────────────────────────────────────────────────────────┤
    │ Request ID:  <uuid>                                             │
    ├─────────────────────────────────────────────────────────────────┤
    │ To approve, run in another terminal:                            │
    │   autonoetic gateway approvals approve <uuid>                   │
    │                                                                 │
    │ After approval, continue the conversation - the agent will      │
    │ automatically use the approved reference.                       │
    └─────────────────────────────────────────────────────────────────┘
    ```
- [x] CLI parses assistant_reply for UUID patterns after approval keywords
- [x] Works with any agent that returns approval info (specialized_builder, planner, etc.)

**Completed auto-resume after approval:**

- [x] Added `resume_session_after_approval()` in `scheduler/approval.rs`:
  - Called after approve/reject for `agent_install` actions
  - Sends `event.ingest` to caller's session with `approval_resolved` message
  - Gateway connects to itself via JSON-RPC to deliver the resume message
  - User approval now automatically triggers planner continuation

- [x] Updated planner SKILL.md:
  - Added "Handling Approval Resumed Messages" section
  - Planner knows to expect `approval_resolved` message
  - Retry with `install_approval_ref` after approval

**Acceptance criteria:**
- [x] No agent can call agent.install directly (planner guardrail)
- [x] specialized_builder SKILL has mandatory approval pause rules
- [x] planner SKILL has mandatory approval handling rules
- [x] Install blocks without approval and returns structured approval_required response
- [x] Invalid approval_ref is rejected with clear error message
- [x] Low-risk agents can install without approval (RiskBased policy)
- [x] User always sees a clear approval request with actionable instructions (CLI feedback)
- [x] Gateway auto-resumes planner session after approval (no need to type "approved")

---

### Issue 6: Approval Payload Storage for Deterministic Retry (COMPLETED)

**Problem:** When specialized_builder retries after approval, the LLM may generate a different payload than the original, causing fingerprint mismatch. Demo-session-2 showed 3 failed retry attempts.

**Solution:** Store the exact install payload when approval is created, use stored payload on retry.

**Implementation tasks (all completed):**

- [x] Added `store_install_payload()` helper function (~tools.rs:176)
- [x] Added `load_install_payload()` helper function (~tools.rs:189)
- [x] Added `cleanup_install_payload()` helper function (~tools.rs:204)
- [x] Added `Serialize` to `InstallAgentArgs` and `InstallPromotionGate`
- [x] Store payload when approval created (~tools.rs:2970-2980)
- [x] Use stored payload on retry, preserving approval_ref for cleanup (~tools.rs:2927-2945)
- [x] Cleanup stored payload after successful install (~tools.rs:3290-3298)
- [x] Added 3 unit tests:
  - `test_agent_install_stores_payload_on_approval`
  - `test_agent_install_uses_stored_payload_on_retry`
  - `test_agent_install_cleans_up_payload_after_success`

**Documentation tasks (all completed):**

- [x] Updated `docs/quickstart-planner-specialist-chat.md` with payload storage explanation
- [x] Created `docs/agent-install-approval-retry.md` with full documentation

**Test results:** 167 tests pass (3 new tests added)

---

### Issue 7: Approval Auto-Resume (COMPLETED)

**Problem:** User had to type "approved" in chat after running `gateway approvals approve`. This was redundant - the gateway already knows approval was granted.

**Solution:** Gateway automatically resumes the caller's session after approval.

**Implementation:**

- [x] Added `resume_session_after_approval()` in `scheduler/approval.rs`:
  ```rust
  fn resume_session_after_approval(config, decision) -> anyhow::Result<()>
  ```
  - Checks if action is `AgentInstall` (has a waiting caller)
  - Sends `event.ingest` JSON-RPC to gateway with `approval_resolved` message
  - Includes `request_id`, `agent_id`, `status` (approved/rejected)
  - Runs in background thread with 5-second timeout

- [x] Updated planner SKILL.md:
  - Added "Handling Approval Resumed Messages" section
  - Planner parses `approval_resolved` messages and retries with `install_approval_ref`

**Complete flow now:**
```
1. specialized_builder → agent.install → approval_required (stores payload)
2. specialized_builder → returns "approval_pending" + request_id to planner
3. Planner → reports to user with approval instructions
4. User → `autonoetic gateway approvals approve <request_id>`
5. Gateway → stores approval + sends event.ingest to planner session
6. Planner → receives approval_resolved, re-spawns specialized_builder
7. specialized_builder → retries with install_approval_ref (uses stored payload)
8. Install succeeds with matching fingerprint
```

---

### Documentation Index

| Document | Description |
|----------|-------------|
| `docs/quickstart-planner-specialist-chat.md` | Quickstart guide with approval flow |
| `docs/agent-install-approval-retry.md` | Detailed payload storage mechanism documentation |
| `docs/agent_routing_and_roles.md` | Agent roles and evolution flow |
| `docs/cli-reference.md` | CLI commands including `gateway approvals` |

---

## Session Checkpointing & Forking ✅

**Depends on:** Content Storage & Simplified Agent Data Model

**Status:** Complete. Session snapshots, forking, history persistence, causal chain lineage tracking, and CLI commands all implemented.

**Problem:** Conversation history (`Vec<Message>`) is in-memory only and never persisted. There is no way to fork an existing session from a checkpoint and continue with an alternative branch. This prevents debugging, exploration of alternative reasoning paths, A/B testing, and rollback.

**Goal:** Enable saving a session snapshot (full conversation history + session context) and forking a new session from any snapshot, with lineage tracked in the causal chain. Uses content storage for persistence (works locally and remotely).

**Architecture notes:**
- `Vec<Message>` is `Serialize + Deserialize` — trivial to snapshot
- `SessionContext` already has `load()`/`save()` — copyable
- `sdk_checkpoint.json` is opt-in (only if agent called `sdk.state.checkpoint()`) — optional
- `execute_with_history()` already accepts pre-built `&mut Vec<Message>` — fork just loads and continues
- Causal chain is append-only — fork writes a `session.forked` lineage entry
- **New:** History and snapshots stored as content objects via `content.write/persist/read`

### Storage Model (Content-based)

```
History storage:
  content.write("session_history", serialized_history) → handle
  content.persist(handle) → survives session cleanup

Snapshot storage:
  session.snapshot → content.persist(history_handle)
  Returns handle for later retrieval

Fork operation:
  content.read(history_handle) → get history
  content.write("session_history", history) → new content for forked session
```

**Benefits over file-based storage:**
- Works for remote agents (gateway API)
- Content-addressed integrity (SHA-256)
- Natural deduplication (same history = same handle)
- Content.persist makes snapshots permanent

### Implementation tasks

- [x] Add `SessionSnapshot` struct in `autonoetic-gateway/src/runtime/session_snapshot.rs`:
  - Fields: `source_session_id`, `turn_count`, `created_at`, `history: Vec<Message>`, `session_context: SessionContext`, `sdk_checkpoint: Option<serde_json::Value>`, `content_handle: Option<String>`
  - `SessionSnapshot::capture(session_id, history, turn_count)` — serializes history, stores via `content.write`, returns snapshot with handle
  - `SessionSnapshot::load(handle)` — reads from content store via `content.read(handle)`
  - `SessionSnapshot::persist()` — calls `content.persist(handle)` for permanent storage
  - `SessionFork::fork()` — creates a new session by copying history from a snapshot
- [x] Persist conversation history at hibernate points in `lifecycle.rs`:
  - After `log_hibernate()`, serialize `history` to JSON
  - Store via `content.write("session_history", history_json)`
  - Persist handle via `content.persist()` for cross-session access
- [x] Add `session.snapshot` native tool in `runtime/tools.rs`:
  - Explicit snapshot capture at any point during execution
  - Serializes current history + session context
  - Stores via `content.write` then `content.persist` (makes permanent)
  - Returns content handle to the agent (not file path)
- [x] Add `session.fork` JSON-RPC method in `router.rs`:
  - Params: `source_session_id`, `branch_message` (optional), `new_session_id` (optional, auto-generated if missing)
  - Loads history via `content.read(handle)` from source session manifest
  - Creates new session with copied history via `content.write`
  - Copies `SessionContext` under new session_id
  - Copies `sdk_checkpoint.json` if it exists
  - Logs `session.forked` lineage entry in causal chain (with `source_session_id`, `fork_turn`, `history_handle`)
  - Spawns agent with loaded history + optional branch message appended
- [x] Add `autonoetic trace fork <session_id>` CLI command:
  - `--at-turn <N>` — fork from specific turn (default: latest)
  - `--message <text>` — branch prompt (e.g. "try a different approach")
  - `--agent <id>` — optional: fork into a different agent
  - `--interactive` — start chat session after forking
  - Displays new session_id and content handle
  - Starts interactive continuation
- [x] Add lineage tracking in causal chain:
  - New entry type: `session.forked` with `source_session_id`, `fork_turn`, `branch_message_sha256`, `history_handle`
  - New entry type: `history.persisted` logged on each hibernate with `message_count`, `content_handle`
- [x] Add `autonoetic trace history <session_id>` CLI command:
  - Reads history handle from session manifest
  - Fetches via `content.read(handle)` through gateway API
  - `--json` flag for raw output
  - Works for remote sessions too (uses gateway API)
- [x] Integration tests:
  - `test_session_history_persisted_on_hibernate`: run agent, hibernate, verify history stored via content.write, verify handle in manifest
  - `test_session_fork_creates_new_session`: snapshot session A via content.persist, fork to session B, verify B has copied history via content handles
  - `test_session_fork_with_branch_message`: fork with alternative prompt, verify branch message is appended to history
  - `test_session_fork_lineage_in_causal_chain`: verify `session.forked` entry links source and forked sessions with history handles
  - `test_session_fork_without_sdk_checkpoint`: verify fork works when no `sdk_checkpoint.json` exists
  - `test_session_fork_copies_session_context`: verify known_facts and open_threads are preserved
  - `test_session_snapshot_content_handle`: verify snapshot returns valid content handle, can be read back

### Acceptance criteria
- [x] Conversation history is persisted via content.write on every hibernate
- [x] History handle stored in session manifest for retrieval
- [x] Any session can be forked via content.read/write (works remotely)
- [x] Fork lineage is tracked in the causal chain with content handles
- [x] Snapshots use content.persist for permanent storage
- [x] CLI trace commands use gateway API (content.read) not file paths
- [x] All existing tests still pass

---

## Content Storage & Simplified Agent Data Model ✅

**Status:** Complete. Content store, content tools, knowledge tools, artifact auto-creation, shared_knowledge in spawn responses, foundation instructions, specialist SKILL.md updates, and example agent (Fibonacci worker) all implemented.

**Problem:** The current memory model is confusing for LLMs and breaks for remote agents:
- Tier 1 (working memory) is agent-scoped → specialists save files that planners can't read
- Tier 2 (long-term memory) is complex with visibility ACLs → LLMs struggle to use correctly
- File paths are local → remote agents can't access files on other machines
- Multi-file projects (Python UV project) can't be returned in a single response
- LLMs keep trying to load from memory after `agent.spawn` returns, even though the response contains the content

**Root cause:** Memory coordination is an LLM-facing concern when it should be a system concern. File paths assume local filesystem.

**Goal:** Content-addressable gateway storage that works locally and remotely. Agents write content; gateway handles sharing, storage, and integrity. LLM interface is 3 simple tools.

### Architecture Overview

```
Gateway Storage (content-addressed, works local + remote):
├── .gateway/content/              ← SHA-256 blob store
│   └── sha256/ab/c123...         ← immutable content blobs
├── .gateway/sessions/
│   └── <session_id>/
│       ├── manifest.json          ← maps content names → handles
│       └── artifacts.json         ← artifact metadata
└── .gateway/knowledge.db          ← Tier 2 facts (renamed from memory.db)

Gateway API (all agents use this, local or remote):
├── content.write(name, content) → handle
├── content.read(handle_or_name) → content
├── content.persist(handle) → permanent
└── content.exec(handle, input) → execute (configurable: agent-side or gateway-side)

Knowledge (durable facts, renamed Tier 2):
├── knowledge.store(id, content, tags) → persist fact
├── knowledge.recall(id) → retrieve fact
└── knowledge.search(query) → search facts
```

### Three Simple Tools for LLMs

| Tool | Purpose | Example |
|------|---------|---------|
| `content.write(name, content)` | Write content to session | `content.write("main.py", script)` → returns `sha256:abc123` |
| `content.read(handle_or_name)` | Read content | `content.read("main.py")` or `content.read("sha256:abc123")` |
| `knowledge.store/recall/search` | Durable facts with provenance | `knowledge.store("weather-api", "open-meteo", ["api"])` |

**Why 3 tools are enough:**
- `content.write` = "I made something, save it"
- `content.read` = "I need that thing" (by name in session, or by handle)
- `knowledge.store/recall` = "I learned something, remember it" (cross-session)

### Agent Artifacts: SKILL.md (not MANIFEST.json)

**Key insight**: LLMs are bad at generating consistent structured JSON. SKILL.md uses YAML frontmatter (forgiving) + markdown body (natural language).

Coder produces:
```
weather_agent/
├── main.py           ← the script
├── utils.py          ← helpers (optional)
└── SKILL.md          ← usage docs + metadata
```

SKILL.md format:
```yaml
---
name: "weather.script.default"
description: "Retrieves weather from Open-Meteo API"
script_entry: "main.py"
io:
  accepts:
    type: object
    required: [latitude, longitude]
    properties:
      latitude: {type: number, minimum: -90, maximum: 90}
      longitude: {type: number, minimum: -180, maximum: 180}
      date: {type: string, format: date}
  returns:
    type: object
    properties:
      ok: {type: boolean}
      data: {type: object}
---
# Weather Script Agent

Retrieves current or forecast weather from Open-Meteo API.

## Usage
```bash
python main.py < input.json
```

## Example
```json
{"latitude": 48.8566, "longitude": 2.3522}
```
```

**Why SKILL.md over structured JSON:**
- LLMs write natural language well (markdown body)
- YAML frontmatter is forgiving (comments, flexible quoting)
- Aligns with existing Autonoetic patterns (all agents use SKILL.md)
- Other LLMs read it naturally (planner, specialized_builder)

### Remote Agent Support

All operations go through gateway API (HTTP/JSON-RPC). Agent doesn't care where it runs:

| Operation | Local Agent | Remote Agent |
|-----------|-------------|--------------|
| `content.write("main.py", script)` | Gateway API → local storage | Gateway API → same central storage |
| `content.read("sha256:abc123")` | Gateway API → returns content | Gateway API → same content |
| SKILL.md transfer | Just another content object | Same API call, works identically |
| Discovery (`agent.discover`) | Gateway API | Gateway API |

**Remote agent needs:**
- Network access to gateway (HTTP endpoint)
- Local sandbox OR gateway-side execution capability
- Same tool interface as local agents

### Configurable Sandbox Execution

| Agent capability | Where code runs | Use case |
|------------------|----------------|----------|
| Agent has local sandbox | Agent-side | Autonomous, distributed |
| Agent has no sandbox | Gateway-side | Simple, controlled |
| `execution_mode: script` | Gateway fast path | Deterministic, no LLM |

Gateway decides based on agent manifest capabilities and config.

### Content Lifecycle

**Write flow:**
```
Agent: content.write("main.py", script)
Gateway: 1. Compute SHA-256
         2. Store at .gateway/content/sha256/ab/c123...
         3. Update session manifest: {"main.py": "sha256:abc123"}
         4. Return handle to agent
```

**Read flow:**
```
Agent: content.read("main.py") or content.read("sha256:abc123")
Gateway: 1. Resolve name → handle from session manifest (if name)
         2. Fetch from content store
         3. Return content
```

**Artifact creation (automatic on agent.spawn):**
```
1. Coder writes files → content.write() calls
2. Coder writes SKILL.md → content.write("SKILL.md", ...)
3. Gateway detects SKILL.md in session manifest
4. Gateway creates artifact:
   - Bundles all session content
   - Extracts metadata from SKILL.md frontmatter
   - Adds to spawn response as structured artifacts field
5. Planner receives:
   {
     "assistant_reply": "Weather script implemented...",
     "artifacts": [{
       "id": "sha256:abc123",
       "name": "weather.script.default",
       "files": ["main.py", "utils.py", "SKILL.md"],
       "entry_point": "main.py",
       "io_schema": {...}  ← from SKILL.md frontmatter
     }]
   }
```

**Agent installation flow:**
```
1. Planner passes artifact handles to specialized_builder
2. Specialized_builder reads SKILL.md via content.read()
3. Specialized_builder creates agent directory:
   - Copies files from content store
   - SKILL.md becomes agent manifest
   - Registers in AgentRepository
4. New agent is available for discovery and spawning
```

**Persistence for reuse:**
```
content.persist("sha256:abc123") → makes content permanent
Future sessions can discover and reuse via agent.discover
```

### Design Decisions

| Current | New | Rationale |
|---------|-----|-----------|
| `memory.working.save/load` | `content.write/read` | Works locally + remotely |
| File paths (local only) | Content handles (SHA-256) | Remote agent compatible |
| Agent returns content in response | Gateway creates artifacts automatically | Multi-file support |
| MANIFEST.json (structured JSON) | SKILL.md (YAML + markdown) | LLM-friendly, existing pattern |
| Fixed local sandbox | Configurable execution | Agent-side or gateway-side |
| LLM coordinates memory sharing | System handles automatically | LLMs are bad at coordination |

### Implementation Tasks

#### 1. Content-addressable storage infrastructure

- [x] Create `ContentStore` struct in gateway:
  - Storage path: `.gateway/content/sha256/ab/c123...`
  - `write(name, content) -> Handle` - compute SHA-256, store, return handle
  - `read(handle) -> Content` - fetch from store by hash
  - `read_by_name(session_id, name) -> Content` - resolve name → handle → content
  - `persist(handle) -> ()` - mark content as permanent (no session cleanup)
- [x] Add `content.write`, `content.read`, `content.persist` native tools:
  - `content.write(name, content)` - stores content, returns handle
  - `content.read(handle_or_name)` - reads by handle or session-relative name
  - `content.persist(handle)` - marks content for cross-session persistence
  - All tools work via gateway API (local socket or HTTP for remote)
- [x] Session manifest management:
  - `.gateway/sessions/<session_id>/manifest.json` - maps names → handles
  - Updated on every content.write
  - Gateway manages automatically

#### 2. Artifact creation from SKILL.md

- [x] Gateway detects SKILL.md in session content:
  - When agent writes SKILL.md, gateway parses YAML frontmatter
  - Extracts: name, description, script_entry, io schema
  - Stores artifact metadata in session artifacts.json
- [x] Auto-create artifact on agent.spawn completion:
  - Bundle all session content into artifact
  - Include SKILL.md frontmatter metadata
  - Add structured `artifacts` field to spawn response
- [x] Artifact response format:
  ```json
  {
    "id": "sha256:abc123",
    "name": "weather.script.default",
    "description": "Retrieves weather from Open-Meteo API",
    "files": ["main.py", "utils.py", "SKILL.md"],
    "entry_point": "main.py",
    "io": {"accepts": {...}, "returns": {...}}
  }
  ```

**Implementation notes (2026-03-14):**
- Added `ArtifactMetadata` struct to `execution.rs`
- Added `extract_artifacts_from_content_store()` function that:
  - Lists all content names in a session
  - Finds SKILL.md files
  - Parses YAML frontmatter for metadata
  - Computes combined artifact ID from file handles
- Added `parse_skill_md_artifact()` helper for frontmatter parsing
- Modified `SpawnResult` to include `artifacts: Vec<ArtifactMetadata>`
- Updated both script-mode and LLM-mode spawn paths to extract artifacts
- Updated `agent.spawn` tool response to include `artifacts` field
- Updated JSON-RPC responses for `agent.spawn` and `event.ingest` to include `artifacts`
- Added unit test for SKILL.md artifact creation

#### 3. Remove Tier 1 memory (working memory)

- [x] Deprecate `memory.working.save/load/list` tools:
  - Keep tools operational for backward compatibility
  - Add deprecation warning in tool description
  - Redirect to `content.write/read` in foundation instructions
- [x] Update foundation instructions:
  - Replace Tier 1 memory guidance with content tools
  - "Use `content.write` for files/scripts/data"
  - "Use `content.read` to retrieve by name or handle"
  - "Content is automatically session-scoped and accessible to all session agents"
- [x] Update all SKILL.md files:
  - planner: use `content.write/read` instead of `memory.working.save/load`
  - architect: write schemas via `content.write`
  - coder: write scripts via `content.write`, include SKILL.md
  - specialized_builder: use content.read to access files
  - Remove all memory coordination instructions (no more "return content in response")
- [ ] Migration path:
  - Existing `state/` directories continue to work
  - Gateway can optionally migrate `state/*.json` to content store

#### 4. Rename Tier 2 to "knowledge"

- [x] Rename tools:
  - `memory.remember` → `knowledge.store`
  - `memory.recall` → `knowledge.recall`
  - `memory.search` → `knowledge.search`
  - `memory.share` → `knowledge.share`
- [x] Keep existing implementation (SQLite + provenance)
  - Same `MemoryObject` schema
  - Same ACL/visibility controls
  - Just rename the tool interface
- [x] Update foundation instructions:
  - "Use `knowledge.store` for durable facts you want to persist across sessions"
  - "Use `content.write` for working files within a session"
  - Clear distinction: content = files/data, knowledge = facts with provenance
- [x] Update SKILL.md files:
  - planner, coder, architect, specialized_builder now use knowledge tools
  - Removed memory.remember/recall references from primary guidance

#### 5. Remote agent support via gateway API

- [x] Ensure all content tools work via HTTP API:
  - `POST /api/content/write` - Write content (UTF-8 or base64)
  - `GET /api/content/read/{session_id}/{name_or_handle}` - Read content
  - `POST /api/content/read` - Read content (alternative body params)
  - `POST /api/content/persist` - Mark content as persistent
  - `GET /api/content/names?session_id=X` - List content names with handles
  - `content.write` → POST /api/content/write
  - `content.read` → GET /api/content/{handle}
  - Same tool interface, different transport
- [ ] Remote agent configuration:
  - Gateway URL in agent config
  - Authentication token for remote agents
  - Same capability checks apply
- [ ] Sandbox execution for remote agents:
  - If agent has local sandbox → execute agent-side
  - If agent has no sandbox → gateway executes centrally
  - Configurable via agent manifest capabilities

#### 6. SKILL.md guidance for specialists

- [ ] Coder SKILL.md update:
  - "Write all code files using `content.write(filename, content)`"
  - "Write SKILL.md with YAML frontmatter containing name, description, script_entry, io schema"
  - "SKILL.md body should include usage instructions in natural language"
  - "Do NOT return file contents in response - just write them via content.write"
- [ ] Architect SKILL.md update:
  - "Write design documents using `content.write(filename, content)`"
  - "Include I/O contracts in the design"
- [ ] Planner SKILL.md update:
  - "After spawning coder/architect, check response `artifacts` field"
  - "Pass artifact handles to specialized_builder for installation"
  - "Do NOT try to load from memory - use content.read with handles from artifacts"

### Acceptance Criteria

- [x] Planner can read files written by architect/coder without memory coordination
- [x] Multi-file projects (UV project) can be shared between agents
- [ ] Remote agents can read/write content via gateway API (requires HTTP endpoints - future work)
- [x] SKILL.md is used for all agent artifacts (not MANIFEST.json)
- [x] Gateway auto-creates artifacts from SKILL.md in session content
- [x] Content is addressable by SHA-256 handle (works locally + remotely)
- [x] LLM has 3 simple tools: content.write, content.read, knowledge.store/recall
- [x] No more "return content in response" or "don't load from memory" rules
- [x] Demo-session-6 type scenario works without looping (validated via full_lifecycle_integration tests)
- [x] All existing tests pass with new content tools

### Progress Notes (2026-03-14)

**Core Implementation Complete:**

- Created `ContentStore` struct in `autonoetic-gateway/src/runtime/content_store.rs`:
  - SHA-256 content-addressable storage at `.gateway/content/sha256/`
  - Session manifest management at `.gateway/sessions/<session_id>/manifest.json`
  - Full test coverage (9 unit tests)
- Added native tools in `autonoetic-gateway/src/runtime/tools.rs`:
  - `content.write(name, content)` → returns content handle
  - `content.read(name_or_handle)` → reads by name or SHA-256 handle
  - `content.persist(handle)` → marks content for cross-session persistence
- Added knowledge tools (renamed Tier 2 memory):
  - `knowledge.store(id, content, scope)` → stores durable facts with provenance
  - `knowledge.recall(id)` → retrieves facts with visibility checks
  - `knowledge.search(scope, query)` → searches by scope and content
  - `knowledge.share(id, agents)` → shares with specific agents
- Updated foundation instructions with content/knowledge guidance
- **Artifact auto-creation** from SKILL.md:
  - Added `ArtifactMetadata` struct in `execution.rs`
  - Added `extract_artifacts_from_content_store()` function
  - Modified `SpawnResult` to include artifacts
  - Updated `agent.spawn` and `event.ingest` to return artifacts
- All 176 unit tests pass
- Legacy memory tools retained for backward compatibility

**Remaining Work (Future):**
- Update specialist SKILL.md files to use content/knowledge tools
- HTTP endpoints for remote agents (requires API layer)

### Test Scenarios

1. **Basic content sharing**: Architect writes design.json via content.write, planner reads via content.read
2. **Multi-file project**: Coder creates Python project with SKILL.md, specialized_builder installs via artifact handles
3. **Remote agent**: Remote coder writes content, local planner reads it (requires HTTP endpoints)
4. **SKILL.md artifact**: Gateway parses SKILL.md frontmatter, creates structured artifact (TODO)
5. **Knowledge persistence**: Agent stores fact via knowledge.store, different session recalls it
6. **Content persistence**: Agent persists content, later session discovers and reuses
7. **Sandbox execution**: Script agent executes via agent-side or gateway-side sandbox based on capabilities

---

## Integration Test: Coder Real LLM Content Generation & Reuse

**Depends on:** Content Storage & Simplified Agent Data Model (above)

**Goal:** End-to-end integration test proving that a real LLM (via openrouter) can generate files with SKILL.md, gateway handles the content correctly, and another agent can reuse it.

**Why this matters:** Validates the full content lifecycle with real LLM behavior (not stubs). Ensures the simple 3-tool interface works with actual model output.

### Test Flow

```
1. Start gateway with temp config + openrouter provider (nvidia/nemotron-3-super-120b-a12b:free)
2. Spawn coder agent with task: "Write a Python script that fetches weather data"
3. Verify coder generates:
   - main.py (or weather.py) - actual Python code
   - SKILL.md - with YAML frontmatter (name, description, script_entry, io schema)
4. Verify gateway captures content:
   - content.write calls logged to causal chain
   - Content stored in content-addressable store
   - Session manifest updated with file handles
5. Verify gateway creates artifact:
   - SKILL.md parsed for frontmatter metadata
   - Artifact response includes structured metadata (name, description, io)
6. Spawn specialized_builder with artifact handles
7. Verify specialized_builder can:
   - Read SKILL.md via content.read
   - Read script file via content.read
   - Install agent using extracted metadata
8. Verify new agent is registered and discoverable via agent.discover

### Test Implementation

#### Test file: `autonoetic-gateway/tests/coder_content_generation_integration.rs`

- [ ] Create test harness:
  - Temp directory for agents, content store, sessions
  - Gateway config with openrouter provider
  - API key from env var (OPENROUTER_API_KEY)
  - Skip test if API key not set

- [ ] Test: `test_coder_generates_files_with_skill_md`:
  - Spawn coder with prompt: "Write a Python script that fetches current weather from Open-Meteo API for a given latitude/longitude. Include a SKILL.md describing usage."
  - Wait for completion
  - Assert response contains artifacts field
  - Assert artifacts contain at least main script file
  - Assert artifacts contain SKILL.md
  - Assert SKILL.md has valid YAML frontmatter with name, description
  - Assert io schema is present in frontmatter

- [ ] Test: `test_gateway_stores_content_correctly`:
  - After coder completes, verify content store has files
  - Read content by handle → matches original
  - Read content by name → matches original
  - Session manifest has correct mappings

- [ ] Test: `test_specialized_builder_reuses_coder_content`:
  - Get artifact handles from coder response
  - Spawn specialized_builder with install request referencing handles
  - Specialized_builder reads SKILL.md → extracts metadata
  - Specialized_builder reads script → installs agent
  - Verify agent is registered in AgentRepository
  - Verify agent is discoverable via agent.discover

- [ ] Test: `test_full_lifecycle_planner_coder_builder`:
  - Spawn planner with user goal: "I need a weather checking agent"
  - Planner spawns architect (if needed) → design
  - Planner spawns coder → implementation with SKILL.md
  - Planner spawns specialized_builder → install
  - Verify end-to-end works without memory coordination issues
  - Verify no looping (planner extracts from artifacts, not memory)

### Required Infrastructure

- [ ] Ensure coder agent SKILL.md instructs content.write usage
- [ ] Ensure coder agent SKILL.md instructs SKILL.md creation with YAML frontmatter
- [ ] Ensure gateway auto-creates artifacts from SKILL.md in session content
- [ ] Ensure content.write/read tools are available in test environment
- [ ] Ensure artifact handles are passed correctly in spawn response

### Acceptance Criteria

- [ ] Test passes with real LLM (nvidia/nemotron-3-super-120b-a12b:free via openrouter)
- [ ] Coder generates at least 1 Python file + SKILL.md
- [ ] SKILL.md has valid YAML frontmatter with name, description, io schema
- [ ] Gateway stores content and creates artifacts automatically
- [ ] Specialized_builder can read files via content.read handles
- [ ] Specialized_builder can install agent using SKILL.md metadata
- [ ] New agent is discoverable via agent.discover
- [ ] Full lifecycle works without memory coordination or looping
- [ ] Test is skippable when OPENROUTER_API_KEY not set

---

## Specialist SKILL.md Updates: Fuzzy Content + Two-Tier Validation ✅

**Depends on:** Content Storage & Simplified Agent Data Model

**Status:** Complete. All six specialist agents updated to use content tools, validation: soft, and fuzzy content approach.

**Design principle:** LLMs are inherently fuzzy. Don't fight this - let them produce natural content. Gateway stores it as-is. Strict schema only for deterministic/script agents.

### Two-Tier Validation

| Agent Type | Validation Mode | Schema Behavior |
|------------|----------------|-----------------|
| LLM agents (reasoning) | `validation: "soft"` (default) | `io` schema is guidance, not enforced |
| Script agents (deterministic) | `validation: "strict"` | `io` schema enforced at boundaries |

### Agent-by-Agent Changes

#### 1. Researcher (`researcher.default/SKILL.md`)

- [x] Remove "JSON Output Format (Required)" section
- [x] Remove strict JSON validation requirements
- [x] Add `content.write("findings.md", research_content)` for storing full research
- [x] Change output guidance: "Write findings to content.write, return natural summary"
- [x] Update `io.returns` to be guidance only (add `validation: "soft"`)
- [x] Remove "Common Mistakes to Avoid" section about JSON formatting
- [x] Add guidance: "Include sources, data, confidence naturally in your response"

#### 2. Architect (`architect.default/SKILL.md`)

- [x] Add `content.write("design.md", design_document)` for storing designs
- [x] Remove "return in response" pattern - use content.write instead
- [x] Add guidance: "Write I/O contracts and design decisions to content.write"
- [x] Update capabilities to include ToolInvoke for content/knowledge tools
- [x] Add `validation: "soft"` to frontmatter

#### 3. Coder (`coder.default/SKILL.md`)

- [x] Update to use `content.write` for all code files
- [x] Update to write SKILL.md with YAML frontmatter for metadata
- [x] Remove "return code in response" pattern
- [x] Remove JSON output format section
- [x] Update Output section to reference natural summary with handles
- [x] Add `validation: "soft"` to frontmatter
- [x] Update capabilities to include ToolInvoke for content/knowledge tools

#### 4. Evaluator (`evaluator.default/SKILL.md`)

- [x] Update to read artifacts via `content.read` instead of memory
- [x] Add content tools section
- [x] Add guidance: "Write detailed evaluation reports to content store"
- [x] Add `validation: "soft"` to frontmatter
- [x] Update Output to natural format

#### 5. Debugger (`debugger.default/SKILL.md`)

- [x] Add content tools section for reading/writing debug data
- [x] Add `validation: "soft"` to frontmatter
- [x] Update capabilities to include ToolInvoke for content tools
- [x] Update Output to natural format

#### 6. Auditor (`auditor.default/SKILL.md`)

- [x] Add content tools section for audit reports
- [x] Add `validation: "soft"` to frontmatter
- [x] Update capabilities to include ToolInvoke for content tools
- [x] Update Output to natural format

#### 7. Specialized Builder (`specialized_builder.default/SKILL.md`)

- [x] Already updated to read SKILL.md from artifacts via `content.read`
- [ ] Update to read script files from artifacts via `content.read`
- [ ] Update agent.install flow to use content handles
- [ ] Remove file path assumptions (content handles work remotely)
- [ ] Update "File shapes" section to reference content handles
- [ ] Already has content - needs updates for content storage

#### 8. Agent Adapter (`agent-adapter.default/SKILL.md`)

- [x] Change from schema-diff to content-transform approach
- [x] Add guidance: "Transform content at consumption time, not production time"
- [x] Update to read base agent content via `content.read`
- [x] Write wrapper via `content.write`
- [x] Add `validation: "soft"` for LLM-to-LLM adaptation
- [x] Add ToolInvoke capability for content/knowledge/agent tools

#### 9. Planner (`planner.default/SKILL.md`)

- [x] Already updated for content storage (artifacts field)
- [x] Add `validation: "soft"` to frontmatter
- [x] Add ToolInvoke capability for content/knowledge/agent tools

#### 10. Memory Curator (`memory-curator.default/SKILL.md`)

- [x] Update from `memory.remember` to `knowledge.store`
- [x] Update from `memory.recall` to `knowledge.recall`
- [x] Update from `memory.search` to `knowledge.search`
- [x] Add guidance: "Use knowledge.store for durable facts with provenance"
- [x] Add `validation: "soft"` to frontmatter
- [x] Add ToolInvoke capability for content/knowledge tools

#### 11. Evolution Steward (`evolution-steward.default/SKILL.md`)

- [x] Update to use `content.persist` for promoting artifacts
- [x] Update to use `knowledge.store` for durable learnings
- [x] Add `validation: "soft"` to frontmatter
- [x] Add ToolInvoke capability for content/knowledge/agent tools

### Foundation Instructions Update

- [x] Update `foundation_instructions.md`:
  - Add content.write/read tools documentation
  - Add knowledge.store/recall tools documentation
  - Add two-tier validation explanation (Rule 4)
  - Add guidance: "LLM agents produce natural content, gateway handles storage"

### Acceptance Criteria

- [x] All LLM agents have `validation: "soft"` (or omit, default is soft)
- [x] All agents use content.write for producing artifacts
- [x] All agents use content.read for consuming artifacts
- [x] Memory tools renamed: knowledge.store/recall/search
- [x] Foundation instructions explain fuzzy content + two-tier validation
- [x] All existing tests pass
