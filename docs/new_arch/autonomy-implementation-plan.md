# CCOS Autonomy Core - Implementation Plan & Status

This document tracks the implementation of the CCOS Autonomy Core features, ensuring a generic, secure, and budget-aware environment for autonomous agents.

## Phase 1: Genericity Audit & Fixes ✅ COMPLETE

Ensure `ccos-agent`, `ccos-chat`, and `ccos-chat-gateway` are fully generic and not coupled to specific demo implementations.

| Component | Status | Description |
|-----------|--------|-------------|
| `ccos-agent` | ✅ | Replaced demo-specific terminology with generic "onboarding" concepts. |
| `ccos-chat` | ✅ | Refactored `moltbook_url` into an optional `status_url` for generic external status tracking. |
| `ccos-chat-gateway`| ✅ | Verified as fully generic. |
| `skills/` module | ✅ | Verified as fully generic. |

## Phase 2: Run Lifecycle & Orchestration 🔶 IN-PROGRESS

Implementing the state machine and API endpoints for managing autonomous goals (Runs).

| Task | Status | Description |
|-----------|--------|-------------|
| **Run State Machine** | ✅ | Defined `RunState` (Active, Done, Paused, Failed, Cancelled) and `Run` struct. |
| **Run Storage** | ✅ | Implemented in-memory `SharedRunStore` with session-scoped indexing. |
| **Run Endpoints** | ✅ | Added `POST /chat/run`, `GET /chat/run/{run_id}`, `POST /chat/run/{run_id}/cancel`, and `POST /chat/run/{run_id}/transition`. |
| **Run Listing** | ✅ | Added `GET /chat/run?session_id=...` to list runs for a session (latest first). |
| **Run Trace** | ✅ | Added `GET /chat/run/:run_id/actions` to list causal-chain actions correlated to a run (latest first). |
| **Run ID Propagation** | ✅ | Gateway spawns agent with `--run-id`; agent uses it when sending capability executions for correlation. Inbound messages correlate to the active run when present. |
| **Run Kickoff** | ✅ | `POST /chat/run` enqueues a synthetic system message ("Run started... Goal: ...") into the session inbox to start execution without a user chat message. |
| **Single Active Run** | ✅ | `POST /chat/run` returns `409` if a session already has an active run (avoids competing orchestrations). |
| **Pause/Resume (External)** | ✅ | When a run transitions to `PausedExternalEvent`, the next inbound message for the session auto-resumes the run to `Active` for continued execution. |
| **Pause Correlation** | ✅ | Inbound messages correlate to the latest paused run (PausedExternalEvent auto-resumes; PausedApproval correlates without resuming). |
| **Run Budget Gate** | ✅ | `/chat/execute` enforces run state and budget (refuses non-chat capabilities when paused/terminal or budget exceeded; records audit events). |
| **Budget Window Reset** | ✅ | Transitioning a run to `Active` resets the run's budget window/counters so "continue" can actually progress. |
| **Completion Predicates** | 🔶 | `completion_predicate` is exposed via `GET /chat/run/:run_id`. Agent respects `manual/always/never` and won't auto-complete unknown predicates. Gateway enforces `never` and supports `capability_succeeded:<capability_id>` for transitions to Done. **Next**: generic predicate DSL (`all_of`, `any_of`, `state_exists`) with evaluator. |
| **Persistence** | ✅ | Run lifecycle events recorded to Causal Chain (`run.create`, `run.cancel`, `run.transition`). `RunStore::rebuild_from_chain()` hydrates in-memory state on startup. |
| **Audit Correlation**| ✅ | Correlate capability-call causal-chain actions with `run_id` / `step_id` (metadata). |
| **Audit Querying** | ✅ | `GET /chat/audit` supports filtering by `session_id` and/or `run_id` for debugging. |

## Phase 3: Budget Enforcement ✅ COMPLETE

Enforce resource limits within the agent loop to prevent infinite runs and uncontrolled resource consumption.

| Feature | Status | CLI Argument / Env Var |
|---------|--------|------------------------|
| **Step Limit** | ✅ | `--max-steps` / `CCOS_AGENT_MAX_STEPS` |
| **Time Limit** | ✅ | `--max-duration-secs` / `CCOS_AGENT_MAX_DURATION_SECS` |
| **Budget Policy**| ✅ | `--budget-policy` (`hard_stop` or `pause_approval`) |
| **Pause/Resume** | ✅ | Agent pauses on exhaustion and can be resumed via "continue" message. |

## Phase 4: Skill Safety & Validation ✅ COMPLETE

Ensure instruction sources (skills, prompts, docs) are handled safely, inputs are verified, and secrets are managed securely.

| Task | Status | Description |
|-----------|--------|-------------|
| **Fail-Fast Loading** | ✅ | Rejects skills with missing ID, operations, or capabilities during load time. |
| **Input Validation** | ✅ | Validates parameters against schema before execution; supports prompting for missing fields. |
| **Secret Persistence**| ✅ | Opt-in: agent can persist discovered per-skill bearer tokens to `SecretStore` (`--persist-skill-secrets` / `CCOS_AGENT_PERSIST_SKILL_SECRETS`) and reuse them across restarts. |
| **Sanitized Logging** | ✅ | Redacts secrets and execution errors in agent and gateway logs. |

## Phase 5: Hardening & Future Features 🔶 IN-PROGRESS

| Feature | Priority | Status | Notes |
|---------|----------|--------|-------|
| **Generic Predicate Engine** | P1 | ✅ Done | `Predicate` enum with RTFS evaluator. `Display` trait for human-readable blueprints. |
| **Skill Onboarding Blueprint Injection** | P2 | ✅ Done | `DelegatingCognitiveEngine` injects onboarding blueprints for non-operational skills. State tracked in WorkingMemory. |
| **Durable Runs** | P0 | ✅ Done | `RunStore::rebuild_from_chain()` implemented. Gateway replays causal-chain events on startup for restart-safe orchestration. |
| **Secrets Governance** | P1 | 🔶 Partial | Approval flow implemented; two-phase commit (stage → approve → persist) needs hardening. |
| **Governed Egress Boundary (No Direct HTTP)** | P0 | ✅ Done | Route all outbound fetches through `ccos.network.http-fetch`. Added network byte metering and budget integration. |
| **Instruction Resource Ingestion** | P0 | ⏳ Planned | Introduce a generic “instruction resource” concept. |
| **Resource Governance (Untrusted Instructions)** | P1 | ⏳ Planned | Treat all instruction resources as untrusted. |
| **Tool Registry Delta / Unload Semantics** | P1 | ⏳ Planned | Tool registration deltas and unload/reset. |
| **Checkpoint/Resume** | P2 | ✅ Done | Durable checkpoints persisted to causal chain. `/checkpoint` and `/resume` endpoints. |
| **Jailed Spawner** | P2 | ✅ Done | `JailedProcessSpawner` using `bubblewrap` for isolation. |
| **Scheduler/Cron** | P2 | ✅ Done | `CronScheduler` for managing recurring and delayed runs. |


---

*Last Updated: 2026-02-05*
