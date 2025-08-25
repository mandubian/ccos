## CCOS Specification 019: Task Delegation Protocol (TaskRequest / TaskResult)

- **Status**: Draft
- **Author**: AI Assistant
- **Created**: 2025-08-13
- **Related Issues**: #90, #92, #18, #23
- **Depends On Specs**: 001 (Intent Graph), 002 (Plans & Orchestration), 003 (Causal Chain), 006 (Arbiter), 018 (Agent Registry & Discovery Layers)

### 1. Abstract
This spec defines the minimal protocol envelope used when CCOS delegates execution of an Intent (or a portion of its realization) to an external autonomous agent or execution surface that operates *outside* the local RTFS evaluator. The protocol introduces two serializable data structures: `TaskRequest` (outbound) and `TaskResult` (inbound). They form the stable contract between the Arbiter / Orchestrator layer and external agents, keeping RTFS language semantics (`Intent`, `Plan`, `Action`) decoupled from transport and heterogeneous execution concerns.

### 2. Motivation
Delegation today (Issue #23) selects an agent and records lifecycle events, but actual cross‑process execution handoff requires a neutral, minimal envelope:
* Avoid leaking internal RTFS Plan AST or evaluator specifics.
* Provide correlation (audit trail) through the Causal Chain.
* Support heterogeneous agents (LLM wrappers, rule engines, service bots) without each needing to parse RTFS.
* Establish a foundation before richer parameter and streaming semantics evolve.

### 3. Position in the Architecture
| Layer Concept | Purpose | Relationship to Task Protocol |
|---------------|---------|--------------------------------|
| Intent (Spec 001) | High-level user / system goal | `intent_id` ties Task envelopes back to origin |
| Plan (Spec 002) | Structured RTFS program `(do ...)` | May be absent or partially generated prior to delegation; not serialized in TaskRequest |
| Action (Spec 003) | Ledger entry for execution/audit | Sending & receiving Task envelopes will produce Actions (delegation events + eventual result) |
| Arbiter (Spec 006) | NL → Intent / Plan scaffolding | Decides (or DelegatingArbiter chooses agent) then constructs `TaskRequest` |
| Agent Registry (Spec 018) | Metadata + scoring | Supplies `agent_id` and receives feedback from `TaskResult` |
| Orchestrator (Spec 002) | Deterministic plan execution | Transports TaskRequest, ingests TaskResult, updates ledger & feedback |

### 4. Data Structures

#### 4.1 TaskRequest
```rust
pub struct TaskRequest {
    pub intent_id: String,   // Correlation back to originating intent
    pub agent_id: String,    // Selected external agent identity (registry key)
    pub goal: String,        // Human-readable NL objective (auditable & agent-friendly)
    pub params: HashMap<String, Value>, // Optional structured args (reserved; empty for now)
}
```
Field Rationale:
* `intent_id` – Ensures result routing & immutable audit linking.
* `agent_id` – Disambiguates multi-agent ecosystems; enables feedback attribution.
* `goal` – Minimal semantic payload letting an agent operate even if it cannot parse RTFS.
* `params` – Future structured delegation inputs (e.g., capabilities extracted from Plan, typed arguments). Intentionally empty at bootstrap (reduces churn while AST-based capability extraction (#91) is designed).

#### 4.2 TaskResult
```rust
pub struct TaskResult {
    pub intent_id: String, // Must match request
    pub agent_id: String,  // Must match request
    pub success: bool,     // Execution outcome
    pub output: Option<Value>, // Optional structured return (RTFS Value space)
    pub error: Option<String>, // Present when success == false (human-readable summary)
}
```
Constraints:
* Exactly one of (`output`, `error`) should be `Some` (soft rule; enforced by constructor helpers in later iteration).
* `intent_id` + `agent_id` pair authenticates correlation and mitigates cross-talk.

### 5. Lifecycle & State Transitions
High-level sequence (with current + planned steps):
1. Intent created & stored (IntentGraph).
2. DelegatingArbiter scores agents (Spec 018) → selects `agent_id`; governance validates delegation.
3. Delegation Approved Action appended (Causal Chain) with selection metadata.
4. Orchestrator constructs `TaskRequest` (using NL goal from Intent; future: include derived params) and dispatches (Issue #92 implementation).
5. External agent executes; may internally generate its own sub-plan or tool calls.
6. Agent returns `TaskResult` (transport TBD: HTTP, message bus, capability callback, etc.).
7. Orchestrator appends TaskResult Action including success/failure outcome & any `output`.
8. Feedback loop updates AgentRegistry success metrics (affecting adaptive threshold (#96)).
9. (Optional Future) Result output may seed follow-on Plan synthesis or satisfy Intent closure.

State Diagram (simplified):
```
Intent(New) -> DelegationProposed -> DelegationApproved -> TaskDispatched -> TaskCompleted{Success|Failure} -> FeedbackApplied -> Intent{Satisfied|NeedsFollowUp}
```

### 6. Design Principles
* **Minimal Surface** – Only fields proven necessary for correlation & basic execution now.
* **Forward-Compatible** – Extra metadata added via extensible maps (e.g., `params`, future `meta` map) instead of breaking struct fields.
* **Language Isolation** – RTFS Plans are not shipped wholesale; preserves internal evolution freedom & reduces exposure.
* **Auditability** – All transitions recorded as Actions referencing `intent_id`.
* **Deterministic Core** – Protocol creation & ingestion logic must be side-effect deterministic (no hidden network calls inside governance path).

### 7. Non-Goals (Current Phase)
* Streaming partial results or tokens.
* Progress / heartbeat updates (would require `TaskUpdate`).
* Multi-agent task decomposition (only single agent per TaskRequest).
* Security primitives (signatures, auth) – deferred until security spec extension (Spec 005 alignment).
* Version negotiation – initial protocol implicitly version 1; add `version` field only when first incompatible change proposed.

### 8. Validation & Error Handling
Planned (Issue #92):
* Reject `TaskResult` if `intent_id` / `agent_id` mismatch → record governance denial / anomaly Action.
* Enforce one-of output/error invariant.
* Timeout handling: Orchestrator may record `TaskResult` synthetic failure after deadline (future; not part of bootstrap acceptance).

### 9. Feedback Integration
Upon a `TaskResult`, success boolean feeds AgentRegistry `record_feedback`, adjusting rolling success metrics (Spec 018) which influence:
* Delegation candidate ordering.
* (Future) Adaptive threshold (#96) dynamic adjustments.

### 10. Future Extensions / Open Questions
| Extension | Description | Linked Issue |
|-----------|-------------|--------------|
| Structured Params Derivation | Populate `params` via AST capability extraction pass | #91 |
| Orchestrator Wiring & Transport | Implement dispatch + ingestion pipeline | #92 |
| Adaptive Threshold Influence | Dynamic delegation gate based on rolling success | ✅ #96 (Implemented) |
| Streaming / Incremental Results | Introduce `TaskUpdate` & finalize success on terminal message | (TBD) |
| Security Envelope | Signatures, nonce, integrity hash, optional encryption | (TBD) |
| Version Field | Add `protocol_version` once first incompatible change needed | (TBD) |

### 11. Implementation Notes (Current Code Snapshot)
File: `rtfs_compiler/src/ccos/task_protocol.rs`
Status: Structs + simple constructors only; no orchestrator wiring yet. Serialization derives ensure compatibility with multiple transports (JSON, CBOR, etc.). `Value` reuse lets outputs feed directly into future Plan generation or ledger recording without conversion.

### 12. Acceptance Criteria for Issue #92
To mark #92 complete, the following must be implemented referencing this spec:
1. Orchestrator hook: when delegation approved, build & dispatch `TaskRequest`.
2. Pluggable dispatcher trait (sync mock for tests, async real later).
3. Receipt handler: accept `TaskResult`, validate, append Action, invoke feedback.
4. Tests: happy path success + failure + mismatched agent_id rejection + feedback metric update.
5. Docs: Update this spec status to "Implemented" and summarize any deviations.

### 13. Security Considerations
* Replay attacks possible without nonce/signature (accepted risk at bootstrap). Future security extension will wrap envelopes before transport.
* Output injection risk mitigated by governance filters before secondary Plan generation (if any) — must re-sanitize any NL-derived continuations.

### 14. Observability
Minimum telemetry fields in Causal Chain Actions (future wiring): chosen agent, dispatch timestamp, duration (if measurable), success boolean, truncated error message. Full payload logging should be optional & policy-controlled for privacy.

### 15. Backwards Compatibility Strategy
Additive evolution only until first breaking change; prefer metadata maps over altering existing required fields. Introduce `protocol_version` field concurrently with first incompatible feature *and* provide migration shims.

### 16. Glossary
* **External Agent**: An autonomous component outside the local RTFS evaluator that can fulfill a delegated goal.
* **Task Envelope**: General term for `TaskRequest` / `TaskResult` pairing.
* **Delegation Lifecycle**: Sequence of events from selection to feedback (see Section 5).

---
End of Spec 019.
