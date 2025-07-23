# CCOS Specification 014: Design Rationale for the `(step ...)` Primitive

**Status:** Decided
**Version:** 1.0
**Date:** 2025-07-21

## 1. Abstract

This document records the design decision for implementing the `(step ...)` orchestration primitive within the RTFS language. The `(step ...)` form is a cornerstone of CCOS plan execution, allowing the Orchestrator to log actions to the Causal Chain before and after a piece of code is executed. This document outlines the chosen implementation strategy and the rationale for rejecting several powerful but ultimately unsuitable alternatives.

## 2. The Core Problem: Evaluating `(step ...)`

The `(step "name" ...body)` primitive has a unique requirement: it must execute logic *before* and *after* its `body` is evaluated.
1.  **Before**: A `PlanStepStarted` action must be logged to the Causal Chain.
2.  **During**: The `...body` expressions must be evaluated in the current environment.
3.  **After**: A `PlanStepCompleted` or `PlanStepFailed` action must be logged with the result.

A standard function call in RTFS (and most languages) evaluates all its arguments *first*, then passes the resulting *values* to the function. If `step` were a standard function, its body would execute before the `step` function itself had a chance to run, making it impossible to log the "started" event first.

## 3. The Chosen Solution: A Built-in Special Form

It was decided that `(step ...)` must be implemented as a **special form** within the RTFS `Evaluator`.

-   **Definition**: A special form is a language construct with custom evaluation rules, handled directly by the compiler or evaluator. Unlike a function, it receives its arguments as unevaluated code (AST nodes).
-   **Implementation**: The `Evaluator`'s main evaluation logic will identify lists that begin with the symbol `step`. It will then dispatch to a dedicated internal handler (`eval_step`) which performs the three-part process described above: notify the host, evaluate the body, and notify the host of the result.
-   **Delegation**: The `eval_step` handler uses the `HostInterface` to delegate the CCOS-specific actions (logging to the Causal Chain) to the `RuntimeHost`, keeping the `Evaluator` itself pure and focused on execution.

### 3.1. Future Refinement

While the initial implementation may use a simple `if` or `match` statement to detect the `step` symbol, the long-term vision is to refactor this into a more elegant dispatch table (e.g., a `HashMap`) within the `Evaluator`. This would map special form names to their handler functions, making the `Evaluator`'s core logic cleaner and more extensible for core language developers, without changing the security model.

## 4. Rejected Alternative 1: Lazy Evaluation

The idea of making RTFS a **lazily evaluated** language was considered. In this model, `step` could be a normal library function because its arguments (specifically the `body`) would be passed as unevaluated "thunks" of code.

-   **Pros**: This is an elegant, purely functional solution that would allow `step` to be defined in a library rather than the compiler core.
-   **Cons (Deal-Breakers for CCOS)**:
    1.  **Loss of Predictability**: The order of execution in a lazy language can be non-obvious, making it difficult to reason about when side effects will occur.
    2.  **Auditability Failure**: A predictable, sequential execution order is paramount for the integrity of the **Causal Chain**. Lazy evaluation would make the chain incredibly difficult to interpret and audit.
    3.  **Security Risk**: Static analysis of a plan by the **Governance Kernel** becomes nearly impossible if the execution order is not guaranteed.

**Decision**: The elegance of lazy evaluation is not worth sacrificing the core CCOS principles of auditability, predictability, and security.

## 5. Rejected Alternative 2: A User-Definable Macro System

The possibility of adding a full-fledged, Lisp-style macro system (`defmacro`) was also discussed. `(step ...)` could then be implemented as a macro in a standard library.

-   **Pros**: This offers the ultimate extensibility, allowing users or the Arbiter to define new control structures and create powerful Domain-Specific Languages (DSLs).
-   **Cons (Deal-Breakers for CCOS)**:
    1.  **Extreme Security Risk**: Allowing arbitrary, user-defined code transformations to run before evaluation would completely bypass the Governance Kernel's ability to validate a plan. A malicious macro could rewrite safe-looking code into a dangerous form.
    2.  **Implementation Complexity**: A hygienic macro system is notoriously complex to implement correctly.
    3.  **Debugging Difficulty**: Debugging code that has been transformed by multiple layers of macros is significantly harder.

**Decision**: A user-definable macro system is fundamentally incompatible with the CCOS zero-trust security model. The ability for the Governance Kernel to statically analyze a plan relies on the language's evaluation rules being a fixed, known, and trusted set.

## 6. Final Rationale

Implementing `(step ...)` as a **built-in special form** provides the necessary control over evaluation order in a way that is simple, performant, and, most importantly, **secure and auditable**. It maintains a clear boundary between the fixed rules of the language and the dynamic capabilities it can orchestrate, which is the foundational architectural pattern of CCOS.
