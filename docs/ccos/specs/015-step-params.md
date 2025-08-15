# CCOS Specification 015: `step :params` binding

Status: Draft
Version: 1.0
Date: 2025-08-14

## Summary

This document defines the semantics of supplying a `:params` map to the `(step ...)` special form. It specifies how parameter expressions are evaluated, how the bound values are introduced into the step body, and the visibility/scope rules.

## Syntax

A `step` form may include an optional keyword `:params` followed by a map literal where keys are strings and values are expressions. Example:

    (step "my-step" :params {"a" 1 "b" (call-svc ...) } ...body...)

Only string keys are accepted in the `:params` map. Non-string keys will cause a runtime validation error before the step body is executed.

## Semantics

- Parameter evaluation order: Each value expression in the `:params` map is evaluated once, immediately before the step body is executed and after the `PlanStepStarted` notification has been emitted.

- Isolation: Parameter evaluation occurs in the same environment that will be used to evaluate the step body. Any side-effects they cause are visible to the step body and to subsequent steps, subject to existing execution context isolation rules.

- Binding: The resulting map of bound parameter values is introduced into the environment as a runtime map under the reserved symbol `%params`.
  - Keys in `%params` are string keys accessible via standard map accessors (e.g., `(get %params "a")`).
  - Values are the fully evaluated `Value` objects resulting from evaluating their corresponding expressions.

- Error handling: If evaluation of any parameter expression fails, the step transitions into a failed state and the error is propagated; the step body is not executed. The failure includes an error message indicating which parameter failed to evaluate.

## Validation

- The runtime will validate that the `:params` object is a map with string keys. Any other map key types will result in a `RuntimeError::InvalidArguments`.

- Parameter expression evaluation errors are reported as `ParamBinding` errors and converted into standard runtime errors with context.

## Scope & Lifetime

- The `%params` binding exists for the duration of the step body evaluation and is removed when the step completes (success or failure) together with the step's local execution context.

- Nested steps evaluate their own `:params` independently; the `%params` symbol in the inner step shadows outer step `%params` for the duration of the inner step body.

## Examples

- Simple literal params:

    (step "s1" :params {"a" 1 "b" "x"} (println (get %params "b")))

- Params that call services:

    (step "s2" :params {"user" (call-svc :users.get {:id 123})} (process-user (get %params "user")))

## Implementation notes

- The runtime implements a helper `bind_parameters` that accepts a map of AST expressions and an evaluator callback so it can be reused by both the AST tree-walking evaluator and any future IR-based runtime.

- The `%params` reserved symbol is chosen for minimal namespace collision; consumers should access it read-only in step bodies.

- Future work: Add optional schema validation for params (e.g., type hints) and support for computed default values.
