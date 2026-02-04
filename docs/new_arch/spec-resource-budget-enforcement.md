# Resource Budget Enforcement (Runs + Agent Loop)

**Status**: Partially Implemented  
**Version**: 0.2.0  
**Last Updated**: 2026-02-04  
**Related**: [Autonomy Implementation Plan](./autonomy-implementation-plan.md)

This document describes what is enforced today (agent loop budgets + gateway Run budget gate), what gets audited/correlated, and what’s still missing (token/cost/network metering, durable checkpoints, etc.).

---

## 1. What We Enforce Today

### 1.1 Agent Loop Budgets (ccos-agent)

The agent runtime enforces budgets while processing events / executing planned actions:

- `--max-steps` / `CCOS_AGENT_MAX_STEPS`
- `--max-duration-secs` / `CCOS_AGENT_MAX_DURATION_SECS`
- `--budget-policy` / `CCOS_AGENT_BUDGET_POLICY`
  - `hard_stop`: stop processing and mark outcome as failed/terminal (agent-side)
  - `pause_approval`: pause and wait for an external “continue” (human approval / external event)

These guard against infinite loops even if the Gateway is misconfigured.

### 1.2 Run Budgets (ccos-chat-gateway)

Runs have an attached budget (stored with the run and visible via `GET /chat/run/:run_id`):

- `max_steps`
- `max_duration_secs`

The Gateway enforces run state + budget in `POST /chat/execute`:

- If a run is **Paused** or **Terminal**, non-chat capabilities are refused.
- If a run **exceeds its budget**, the Gateway transitions the run to `PausedApproval` and refuses the capability call.
- Chat egress/transform capabilities remain allowed even when paused/terminal so the system can communicate next steps.

### 1.3 Budget Window Reset on Resume

Run budgets apply to a **budget window** (not necessarily the full lifetime of the run).

When a run transitions to `Active` via `POST /chat/run/:run_id/transition`, the Gateway resets the budget window start time and counters so “resume/continue” can actually progress.

---

## 2. Budget Dimensions (Current vs Planned)

### Implemented

- Steps (capability calls counted against `max_steps`)
- Wall-clock time since `budget_started_at` (checked against `max_duration_secs`)

### Reserved / Partially Wired

The Run model includes `max_tokens` and `tokens_consumed`, but token metering is not end-to-end enforced yet. Treat token fields as informational until metering is implemented.

### Planned (Not Implemented)

- LLM tokens (input/output) metering and enforcement
- Cost metering and enforcement
- Network egress bytes metering and enforcement
- Storage write bytes metering and enforcement
- Retry budgets per step (run-level, not only agent-loop heuristics)

---

## 3. Enforcement Semantics

### 3.1 Gateway `/chat/execute` Gate

The Gateway’s primary role is to prevent side effects when a run should not progress.

Policy summary:

- Allowed while paused/terminal: `ccos.chat.egress.*`, `ccos.chat.transform.*`
- Refused while paused/terminal: everything else
- On budget exhaustion: transition run → `PausedApproval` and refuse the call

### 3.2 Agent Behavior on Exhaustion

Agent-side budgets are a second line of defense. When exhausted:

- `hard_stop`: end the agent loop (agent-side)
- `pause_approval`: pause and rely on an external continue signal (message) to resume

If the agent is run-aware (`--run-id`), it also attempts to transition the Run in the Gateway accordingly.

---

## 4. Auditing and Debugging

Budget-related events are visible through:

- Run trace: `GET /chat/run/:run_id/actions`
- Audit trail: `GET /chat/audit?run_id=...` (and/or `?session_id=...`)

Budget gate transitions are recorded as run lifecycle events (e.g. `run.transition`) and budget exceeded markers (e.g. `run.budget_exceeded`) in the causal chain/audit trail.

---

## 5. API Surface (Budgets)

### 5.1 Create Run With Budget

`POST /chat/run` accepts an optional `budget` object:

```json
{
  "session_id": "sess_123",
  "goal": "Do a thing safely",
  "completion_predicate": "manual",
  "budget": { "max_steps": 20, "max_duration_secs": 60 }
}
```

### 5.2 Update Budget / Reset Window on Resume

`POST /chat/run/:run_id/transition` may include a budget update. When transitioning to `Active`, the budget window resets.

---

## 6. Known Gaps

- Durable run storage (runs are in-memory; lifecycle is recorded in causal chain but not replayed into a run store on restart).
- Checkpoint/resume segments (bounded execution segments with durable checkpoints).
- End-to-end token/cost metering (requires propagating usage metrics and consistent accounting).

