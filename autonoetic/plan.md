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
- [ ] Example agents (lead/planner plus specialist hands such as researcher, architect, coder, debugger, evaluator, and auditor)
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

### Scenario A: Scheduled Fibonacci worker

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

Implementation shape:

- Add a minimal example agent dedicated to background deterministic jobs.
- Store state as a small file such as `state/fib.json` with `{previous, current, index}`.
- Use background mode `deterministic` with a direct approved action path.
- First version can append outputs to `history/` or `artifacts/` for inspection.

Acceptance criteria:

- After $n$ ticks, the persisted state matches Fibonacci recurrence exactly.
- Restarting the gateway does not reset the sequence.
- Rapid scheduler ticks do not produce duplicate outputs for the same due interval.
- Trace inspection shows one coherent background session with ordered per-tick events.

Recommended constraint:

- Keep this scenario fully local and deterministic. Do not involve MCP or external services.

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

- [ ] Convert example weather-like agents to `execution_mode: script` (requires creating new example agents)
- [ ] Convert Fibonacci background worker to use manifest-level `execution_mode: script` instead of `pending_scheduled_action` plumbing
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

- [ ] Keep physical logs distributed by ownership (`.gateway` and per-agent histories), but add a gateway-owned per-session index manifest for unified timeline reconstruction.
- [ ] Record cross-agent event references in the session index (agent_id, log_id, timestamp, category/action, causal hash refs) as events are emitted.
- [ ] Add trace tooling to reconstruct a single ordered session view from gateway + agent logs without manual stitching (`trace session.show` / `trace session.rebuild` semantics).
- [ ] Add integrity checks in reconstruction mode (missing ref, hash mismatch, orphan entries) and surface explicit diagnostics.
- [ ] Add integration tests for multi-agent delegated sessions proving full timeline reconstruction and deterministic ordering.

### Causal chain rotation and retention

- [ ] Add causal log segmentation by date and size (for example `causal_chain-YYYY-MM-DD-0001.jsonl`) to prevent unbounded single-file growth.
- [ ] Preserve hash-chain continuity across rotated segments (`prev_hash` of first entry in a new segment points to the last entry hash of the prior segment).
- [ ] Add per-history segment index metadata so trace readers can discover all segments without full directory scans.
- [ ] Keep backward compatibility for existing `causal_chain.jsonl` paths during migration and update trace tooling to read both legacy and segmented layouts.
- [ ] Add retention/compression policy for cold segments (optional gzip) with explicit operator controls.
- [ ] Add tests for rotation boundaries, continuity validation, and cross-segment trace reconstruction.

### Textual state-machine conventions and concepts alignment

- [ ] Align `concepts.md` with runtime reality by marking what is implemented now versus planned evolution (without deleting original conceptual text).
- [ ] Define a minimal textual state-machine convention for agents:
  - `state/task.md` for active checklist and next action
  - `state/scratchpad.md` for ephemeral reasoning notes
  - `state/handoff.md` for compact transfer status and blockers
- [ ] Add guidance on when to persist to textual state versus Tier2 memory (`memory.db`) to avoid overlap and drift.
- [ ] Update foundation/planner/builder guidance to prefer these state files for long multi-step tasks that benefit from explicit checklists.
- [ ] Evaluate stronger lazy loading as an optional optimization track (not mandatory): keep current behavior as default until profiling justifies added complexity.

### Pluggable schema enforcement hook

**Problem:** When agents call other agents via `agent.spawn`, the calling LLM must produce payloads matching the target's `io` schema. LLMs are inconsistent at exact structural output. Current lightweight validation passes through garbage or fails without actionable feedback.

**Goal:** Add a pluggable enforcement hook between tool-call interception and target-agent dispatch that can coerce payloads, reject with actionable errors, and log all decisions. Keep the hook swappable between pure code (first) and cheap LLM (later) implementations.

**Design doc:** `docs/schema-enforcement-hook.md`

**Implementation order:**

- [ ] Define `SchemaEnforcer` trait and `EnforcementResult` enum (`Pass`, `Coerced`, `Reject`) in `autonoetic-types`
- [ ] Implement `DeterministicCoercionEnforcer` (pure code): required-field defaults, field renaming by similarity, safe type coercion
- [ ] Add `schema_enforcement` config section to `GatewayConfig` (`primary`, `fallback`, `audit`, `agent_overrides`)
- [ ] Insert hook into `agent.spawn` tool execution path in `runtime/tools.rs` — after capability check, before target dispatch
- [ ] Add causal chain logging for all enforcement decisions (pass, coerce, reject) with transformation details
- [ ] Add unit tests for all coercion rules: exact match (pass), missing defaults (coerce), field rename (coerce), unrecoverable mismatch (reject)
- [ ] Add integration test: malformed `agent.spawn` payload → auto-coerced → target receives valid input
- [ ] Add integration test: unrecoverable mismatch → structured error with `hint` → calling agent repairs in-session
- [ ] Update planner `SKILL.md` guidance to note that structural schema errors are auto-corrected when possible; explicit `skill.describe` is optional
- [ ] (Later) Implement `LlmCoercionEnforcer` with cheap model fallback for complex structural transforms
- [ ] (Later) Add per-agent enforcement overrides so high-churn callers (planner) can opt into LLM fallback
