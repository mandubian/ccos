# Two-Tier Governance Architecture

## Overview

CCOS implements a two-tier governance model for execution hints:
- **Tier 1 (Global)**: Pre-execution plan validation by Gov Kernel
- **Tier 2 (Atomic)**: Per-step governance checkpoints during execution

## Execution Flows

### Direction 1: Top-Down (CCOS → RTFS)

```
User Intent → Arbiter.intent_to_plan()
    ↓
Plan with ^{:runtime.learning.retry ...} metadata
    ↓
GovKernel.validate_and_execute()
    ├── sanitize_intent()
    ├── scaffold_plan()
    ├── validate_against_constitution()
    └── validate_execution_hints() ← TIER 1: Policy limits check
    ↓
Orchestrator.execute_plan()
    ↓
RuntimeHost → Evaluator.evaluate(plan_body)
```

### Direction 2: Bottom-Up (RTFS → Effects)

```
Evaluator.eval_expr(^{:runtime.learning.retry ...} (call :capability args))
    ↓
Extract metadata → host.set_execution_hint(key, value)
    ↓
eval_expr(inner) → RequiresHost(HostCall)
    ↓
RuntimeHost.execute_capability()
    └── build_call_metadata() includes execution_hints
    ↓
Orchestrator.handle_host_call() ← TIER 2: Atomic governance
    ├── if security_level >= "medium": record_decision()
    ├── validate_execution_hints() - enforces limits
    ├── apply_execution_hints() - retry/timeout/fallback
    └── record_outcome()
    ↓
CapabilityMarketplace.execute_capability_enhanced()
```

## Governance Checkpoints

| Level | When | What | Where |
|-------|------|------|-------|
| Global | Plan submission | Constitution rules, hint limits | `GovKernel.validate_and_execute()` |
| Atomic | Each capability call | Security level check, hint application | `Orchestrator.handle_host_call()` |

## Hint Flow

```
Arbiter generates: ^{:runtime.learning.retry {:max-retries 3}}
                    ↓
Evaluator extracts: host.set_execution_hint("runtime.learning.retry", value)
                    ↓
RuntimeHost stores: execution_hints["runtime.learning.retry"] = value
                    ↓
CallMetadata built: metadata.execution_hints = hints
                    ↓
Orchestrator reads: validate + apply hints
```

## Security Guarantees

1. **Allowlist**: Only known hint keys (`runtime.learning.*`) are accepted
2. **Policy limits**: max_retries, max_timeout_multiplier enforced
3. **No bypass**: All capability calls flow through `handle_host_call()`
4. **Audit trail**: Risky operations logged to CausalChain

## Files

| Component | File |
|-----------|------|
| Constitution & Policies | `ccos/src/governance_kernel.rs` |
| Atomic Checkpoints | `ccos/src/orchestrator.rs` |
| Hint Extraction | `rtfs/src/runtime/evaluator.rs` |
| Hint Storage | `ccos/src/host.rs` |
