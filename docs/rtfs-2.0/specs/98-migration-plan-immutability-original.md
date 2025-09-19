 # Migration Plan: Achieving Full Immutability in RTFS 2.0

Status: Proposed
Audience: RTFS compiler/runtime owners
Related: `docs/rtfs-2.0/specs-new/07-immutability-and-state.md`

## 1. Goals

The primary goal of this migration is to align the RTFS codebase with the principle of **absolute immutability** as defined in the RTFS 2.0 specifications. This is a critical step to fully realize the new architecture.

The successful completion of this plan will:
-   **Simplify the Language Core**: Remove the complexity of managing mutable state, making the runtime easier to verify and maintain.
-   **Enhance Security and Predictability**: Eliminate a class of bugs related to state corruption and make the language a safer, more predictable target for AI code generation.
-   **Enforce the Architectural Vision**: Solidify the clean separation between the pure RTFS engine and the stateful Host environment.
-   **Enable Safe Continuations**: Provide the foundation for the continuation-passing and yield mechanism. A pure, immutable state is essential for creating serializable, re-entrant continuations that can be safely managed by the Host.

## 2. Scope of Removal

This migration will completely remove the following constructs and their underlying implementations from the `rtfs_compiler` crate:

-   **Special Form**: `set!`
-   **Functions**: `atom`, `deref`, `reset!`, `swap!`
-   **Reader Macro**: The `@` reader macro, if any part of it was implemented for `deref`.
-   **Value Type**: The `Value::Atom` variant in the core value enum.

## 3. Migration Steps

This plan is broken down into phases to ensure a structured and verifiable transition.

### Phase 1: Static Analysis and Identification

1.  **[ ] Grep for all usages**: Perform a workspace-wide search for the following keywords to identify all affected files:
    -   `set!`
    -   `atom`
    -   `deref`
    -   `reset!`
    -   `swap!`
    -   `Value::Atom`
2.  **[ ] Categorize Usages**: Review the search results and categorize them:
    -   **Core Implementation**: Code in the evaluator/runtime that implements the primitives.
    -   **Standard Library**: Any stdlib functions that might use these primitives.
    -   **Internal Tests**: Unit and integration tests that verify the behavior of the mutation primitives.
    -   **Application-level Code (if any)**: Any higher-level logic within the compiler or related tools that might use atoms or set! for its own state management.

### Phase 2: Code Removal and Refactoring

1.  **[ ] Remove `Value::Atom`**:
    -   Delete the `Atom` variant from the `Value` enum (likely in `rtfs_compiler/src/value.rs` or similar).
    -   This will cause a cascade of compilation errors. Use these errors as a guide for the next steps.

2.  **[ ] Delete Core Implementations**:
    -   Remove the code blocks from the evaluator (e.g., in `evaluator.rs` or `ir_runtime.rs`) that handle the `set!` special form.
    -   Delete the function implementations for `atom`, `deref`, `reset!`, and `swap!` from the standard library (e.g., in `stdlib.rs`).

3.  **[ ] Delete All Related Tests**:
    -   Remove all test files and test cases that were specifically designed to validate the behavior of `set!` and atoms. These tests are now obsolete.

4.  **[ ] Refactor Affected Internal Code**:
    -   This is the most critical step. Any internal code identified in Phase 1 that used atoms or `set!` for its own state management must be refactored to use a pure, functional style.
    -   This typically involves changing functions that mutated a shared atom to instead take state as a parameter and return a new state.
    -   **Example**: A test helper that used an atom as a counter must be changed to a function that takes the current count and returns the next count.

### Phase 3: Validation and Cleanup

1.  **[ ] Full Compilation**: Ensure the entire `rtfs_compiler` crate compiles without errors or warnings related to the removed primitives.
2.  **[ ] Run Full Test Suite**: Execute all remaining tests (`cargo test`). The goal is to ensure that the removal of mutation has not caused any regressions in other, unrelated parts of the language.
    -   Expect some tests to fail if they indirectly relied on mutable behavior. These tests must be updated to reflect the new immutable paradigm.
3.  **[ ] Update Documentation**:
    -   Remove the `16-mutation-and-state.md` spec file.
    -   Ensure all other documentation (READMEs, examples, etc.) no longer references `set!` or atoms.
    -   Ensure the new `07-immutability-and-state.md` spec is correctly referenced.

## 4. Potential Risks and Challenges

-   **Hidden Dependencies**: There may be subtle, indirect dependencies on the mutation model in complex parts of the system or in integration tests. The compiler errors will catch most, but runtime failures in tests may reveal others.
-   **Refactoring Complexity**: Refactoring internal tooling that relied on atoms may be non-trivial and require careful redesign to manage state in an immutable way.
-   **Mindset Shift**: The development team will need to fully embrace the immutable-by-default paradigm for all future work on the RTFS core.

## 5. Success Criteria

The migration will be considered complete when:
-   All code related to `set!` and atoms has been removed from the codebase.
-   The entire project compiles successfully.
-   The full test suite passes with no regressions (after necessary test updates).
-   The official documentation reflects that RTFS is a purely functional and immutable language.
