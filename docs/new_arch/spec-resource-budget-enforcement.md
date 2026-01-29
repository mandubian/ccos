# Resource Budget Enforcement Specification

**Status**: In Development  
**Version**: 0.1.0  
**Related**: [Secure Chat Gateway Roadmap](./ccos-secure-chat-gateway-roadmap.md) (WS11)

## 1. Executive Summary

CCOS enforces resource budgets on all agent runs to ensure controllability. Every run has explicit limits on steps, time, tokens, and cost. Budget exhaustion triggers defined enforcement actions—never silent continuation.

## 2. Motivation

### Problem
Unbounded agent execution poses risks:
- **Cost explosion**: Runaway loops consume unlimited LLM tokens
- **Resource exhaustion**: Long-running tasks block other work
- **Silent failures**: Agents continue past useful limits without notification
- **Audit gaps**: No record of resource consumption per step

### Solution
Runtime budget enforcement with:
- Immutable limits set at run start
- Pre-call budget checks before each capability
- Post-call metering/accounting
- Configurable enforcement policies (hard-stop, approval-required, soft-warn)
- Full audit trail in causal chain

## 3. Budget Dimensions

| Dimension | Unit | Default | Description |
|-----------|------|---------|-------------|
| `steps` | count | 50 | Capability calls |
| `wall_clock_ms` | milliseconds | 60,000 | Total run time |
| `llm_tokens` | count | 100,000 | Input + output tokens |
| `cost_usd` | dollars | 0.50 | Total monetary cost |
| `network_egress_bytes` | bytes | 10 MB | Outbound network |
| `storage_write_bytes` | bytes | 50 MB | Disk writes |

## 4. Core Types

### 4.1 BudgetLimits
Immutable limits for a run:

```rust
pub struct BudgetLimits {
    pub steps: u32,
    pub wall_clock_ms: u64,
    pub llm_tokens: u64,
    pub cost_usd: f64,
    pub network_egress_bytes: u64,
    pub storage_write_bytes: u64,
}
```

### 4.2 ExhaustionPolicy
Action when budget is exhausted:

```rust
pub enum ExhaustionPolicy {
    HardStop,         // Run ends as Failed
    ApprovalRequired, // Run pauses for human
    SoftWarn,         // Log only, continue
}
```

Default policies:
- `steps`, `wall_clock`, `cost`, `network`, `storage` → **HardStop**
- `llm_tokens` → **ApprovalRequired**

### 4.3 BudgetContext
Runtime state for budget tracking:

```rust
pub struct BudgetContext {
    limits: BudgetLimits,
    policies: BudgetPolicies,
    consumed: BudgetConsumed,
    start_time: Instant,
    warnings_issued: HashSet<BudgetWarning>,
}
```

## 5. Execution Flow

```
┌─────────────────────────────────────────────────────────────┐
│                     Agent Run Start                          │
│  1. Create BudgetContext with limits/policies                │
│  2. Log BudgetEvent::Allocation to causal chain              │
└────────────────────────┬────────────────────────────────────┘
                         │
                         ▼
┌─────────────────────────────────────────────────────────────┐
│                   Capability Call Loop                       │
│                                                              │
│  ┌─────────────────┐                                         │
│  │  Pre-Call Check │◄─────────────────────────────────────┐  │
│  └────────┬────────┘                                      │  │
│           │                                               │  │
│           ▼                                               │  │
│  ┌─────────────────────────────────────────────────────┐  │  │
│  │ BudgetCheckResult::Ok         → proceed             │  │  │
│  │ BudgetCheckResult::Warning    → log, proceed        │  │  │
│  │ BudgetCheckResult::Exhausted  → enforce policy      │  │  │
│  └─────────────────────────────────────────────────────┘  │  │
│           │                                               │  │
│           ▼                                               │  │
│  ┌─────────────────┐                                      │  │
│  │ Execute Capability                                    │  │
│  └────────┬────────┘                                      │  │
│           │                                               │  │
│           ▼                                               │  │
│  ┌─────────────────┐                                      │  │
│  │ Post-Call Meter │                                      │  │
│  │ • Record consumption                                   │  │
│  │ • Log BudgetEvent::Consumption                        │  │
│  └────────┬────────┘                                      │  │
│           │                                               │  │
│           └───────────────────────────────────────────────┘  │
└─────────────────────────────────────────────────────────────┘
                         │
                         ▼
┌─────────────────────────────────────────────────────────────┐
│                     Run Complete                             │
│  Log BudgetEvent::RunCompleted with final consumption        │
└─────────────────────────────────────────────────────────────┘
```

## 6. Warning Thresholds

| Threshold | Action |
|-----------|--------|
| 50% | Log warning (once per dimension) |
| 80% | Log warning (once per dimension) |
| 100% | Enforce policy |

Warnings are deduplicated—each threshold logs only once per run.

## 7. Human Escalation

When `ExhaustionPolicy::ApprovalRequired` triggers:

1. Run enters **Paused** state
2. Approval request sent to user/admin with:
   - Dimension exhausted
   - Current consumption
   - Proposed extension amount
3. User approves/denies/modifies extension
4. If approved:
  - Budget extended via `extend_*()` methods or `execution_hints.budget_extend`
   - `BudgetEvent::Extended` logged
   - Run resumes
5. If denied:
   - Run ends as **Cancelled**

## 8. Causal Chain Events

All budget events are logged for audit:

```rust
pub enum BudgetEvent {
    Allocation { run_id, limits },
    Consumption { step_id, capability_id, resources, remaining },
    Warning { dimension, percent, consumed, limit },
    Exhausted { dimension, policy, consumed, limit },
    Extended { dimension, additional, approved_by, reason },
    RunCompleted { run_id, final_consumption },
}
```

## 9. Configuration

### Agent Config
```yaml
governance:
  policies:
    default:
      budgets:
        max_cost_usd: 0.50
        token_budget: 100000
        steps: 50
        wall_clock_ms: 60000
      exhaustion_policies:
        steps: hard_stop
        llm_tokens: approval_required
```

### Per-Run Override
Runs can specify custom limits via RTFS metadata:

```clojure
(run
  :budget {:steps 100
           :cost-usd 1.0
           :policies {:cost-usd :approval-required}}
  :body (do ...))
```

### Session Budget (Inheritance)
Session-level limits can be provided via execution context and will clamp per-run budgets:

```clojure
{:session_budget_limits
  {:steps 200
   :wall_clock_ms 600000
   :llm_tokens 200000
   :cost_usd 1.0
   :network_egress_bytes 10485760
   :storage_write_bytes 52428800}}
```

## 10. Integration Points

### RuntimeHost
- `check_budget_pre_call()`: Intercepts host calls before execution to verify remaining budget.
- `record_budget_consumption()`: Extracts consumption data from `RuntimeResult<Value>` metadata.
  - Recognizes keys like `usage.llm_input_tokens`, `usage_output_tokens`, `total_cost_usd`, etc.
  - **Provider-first**: uses provider-reported usage when available.
  - **Fallback estimation**: if provider usage is missing, estimate tokens from serialized inputs/outputs and mark as estimated.
  - Records results into `BudgetContext` and logs to **Causal Chain**.

### GovernanceKernel
- `handle_host_call_governed()`: Central entry point for all governed host calls.
- Validates current execution hints and budget before delegating to the `Orchestrator`.

### Causal Chain
- All `BudgetEvent` variants are recorded as `Action` nodes with `ActionType::BudgetConsumptionRecorded`.
- Metadata includes `duration_ms`, `llm_input_tokens`, `llm_output_tokens`, and `cost_usd`.

## 11. Future Work

- [x] Budget inheritance (session → run → step)
- [x] Approval-driven budget extensions (execution hints + approval queue)
- [x] Budget extension visibility in approvals UI (main pane + side panel)
- [ ] Predictive budget estimation before run
- [ ] Automatic model tier selection based on remaining budget
- [ ] Cost optimization via batching

## 12. References

- [Secure Chat Gateway Roadmap](./ccos-secure-chat-gateway-roadmap.md) - WS11 Resource Budget Governance
- [Polyglot Sandboxed Capabilities](./spec-polyglot-sandboxed-capabilities.md) - Section 7.4 Resource Budget Integration
