# Worktree: wt/orch-core-status-context — Orchestrator status + context

Scope: Wire audited intent status transitions in the orchestrator and add minimal execution context support needed for plan wiring.

Deliverables (done)
- Orchestrator sets Active → Executing before evaluation via `IntentGraph.set_intent_status_with_audit(...)` with causal-chain audit (includes reason + triggering_action_id).
- Final status set via `IntentGraph.update_intent_with_audit(...)` to Completed or Failed, also audited in the causal chain.
- Causal-chain lifecycle events: PlanStarted, PlanCompleted/PlanAborted logged.
- Minimal execution context: runtime host execution context set/cleared; evaluator context manager initialized; checkpoint/resume helpers added (PlanPaused/PlanResumed actions).

Tests
- `orchestrator_emits_executing_and_completed` — verifies audited Active→Executing and Executing→Completed transitions with metadata.
- `orchestrator_emits_failed_on_error` — verifies audited Active→Executing and Executing→Failed transitions with metadata.

Notes / follow-ups
- Extend tests to cover multi-intent and child-intent status transitions.
- Optional: add explicit metrics counters beyond causal-chain events.
