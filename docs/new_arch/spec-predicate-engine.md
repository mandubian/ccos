# Specification - Generic Completion Predicate Engine (RTFS)

The Generic Completion Predicate Engine provides a declarative way to specify when an autonomous run should transition to a `Done` state. While it supports a JSON-based DSL for external compatibility, RTFS is the kernel language for logic internally.

## Architecture

The Predicate Engine operates on the causal chain's audit trace. After every event is recorded in the causal chain, the engine evaluates the run's `completion_predicate` against the history of actions.

### RTFS Integration

The engine uses the RTFS evaluator to process arbitrary logic. It exposes specific "audit query" functions to the RTFS environment:

- `(audit.succeeded? "function_name")`: Returns `true` if an action with the given name succeeded in the trace.
- `(audit.failed? "function_name")`: Returns `true` if an action with the given name failed.
- `(audit.metadata? "function_name" "key" "value")`: Returns `true` if an action's metadata matches the criteria.

### Logical Composition

Standard RTFS logical operators (`and`, `or`, `not`) are used to compose complex criteria:

```rtfs
(and 
  (audit.succeeded? "ccos.chat.send")
  (not (audit.failed? "github.create_issue")))
```

## Data Model

The `completion_predicate` is stored in the `Run` metadata within the causal chain.

### JSON DSL (Compatibility Layer)

For external services communicating via JSON, the engine supports a structured enum:

| Variant | Description |
| :--- | :--- |
| `action_succeeded` | `{ function_name: "..." }` |
| `action_failed` | `{ function_name: "..." }` |
| `action_metadata_matches` | `{ function_name: "...", key: "...", value: "..." }` |
| `and`, `or`, `not` | Logical compositions of other predicates. |
| `rtfs` | `"(rtfs code string)"` |

## Evaluation Lifecycle

1. **Event Recording**: When `gateway.rs` calls `record_run_event`, a new action is appended to the causal chain.
2. **Polling**: The `evaluate_run_completion` helper is invoked.
3. **Trace Retrieval**: The full audit trail for the run is loaded from the causal chain.
4. **Logic Evaluation**:
   - If the predicate is `Rtfs`, the RTFS evaluator is run with audit builtins.
   - If the predicate is a structured variant, the Rust-native evaluator is run.
5. **Transition**: If evaluation returns `true`, the run is transitioned to `Done` and an audit event is recorded.
