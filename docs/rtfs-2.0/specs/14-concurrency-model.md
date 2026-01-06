# RTFS 2.0: Concurrency Model

## Implementation Status

**⚠️ Host-mediated via capabilities**

Concurrency in RTFS 2.0 is implemented through host-mediated capabilities rather than native language primitives. The implementation status is:

| Feature | Status | Notes |
|---------|--------|-------|
| **`step-parallel` Special Form** | ❌ **Not Implemented** | Design specification only; not in grammar or AST |
| **Host-Mediated Parallelism** | ✅ **Implemented** | Via `:ccos.concurrent/parallel` capability and other concurrency capabilities |
| **Async Operations** | ✅ **Implemented** | Futures and promises via host capabilities |
| **Isolated Execution** | ✅ **Implemented** | Forked evaluators with shared code but isolated environments |
| **Task Coordination** | ⚠️ **Via Capabilities** | Cancellation, timeouts through host capabilities |
| **Parallel Collections** | ❌ **Design** | `pmap`, `preduce` operations not implemented |
| **Channel Communication** | ❌ **Design** | Go-style channels not implemented |

### Key Implementation Details
- **Host Delegation**: All concurrent execution delegated to CCOS host via capabilities
- **Closure-Based Tasks**: Parallel work units defined as RTFS closures capturing lexical environment
- **Safety by Design**: Isolated execution contexts prevent shared mutable state
- **Capability Integration**: Uses existing `:ccos.concurrent/parallel` capability for parallel execution
- **No Native Concurrency**: RTFS evaluator remains single-threaded and synchronous

### Implementation Reference
- **Host Capabilities**: `:ccos.concurrent/parallel`, `:ccos.async/compute-heavy-task`, `:ccos.async/await`
- **Runtime Context**: Forked evaluators with shared macro/type registries
- **Security Model**: Concurrency capabilities subject to same governance as other host calls

**Note**: This specification describes a comprehensive concurrency model, but only host-mediated parallelism via capabilities is currently implemented. The `step-parallel` special form and other concurrency primitives are design specifications for future implementation.

## 1. Overview

RTFS 2.0 enforces a **strict host-mediated concurrency model**. The RTFS evaluator itself is single-threaded and synchronous. All parallel execution is achieved by delegating units of work (Tasks) to the CCOS Host.

### Core Principles

1.  **Synchronous Core**: The RTFS evaluator never manages threads or locks directly.
2.  **Host Delegation**: Concurrency is a side-effect provided by the Host via capabilities.
3.  **Closure-Based Units**: Parallel tasks are defined as RTFS closures (functions) capturing their environment.
4.  **Isolated Execution**: Parallel branches run in isolated contexts; they cannot mutate shared RTFS state (variables), ensuring thread safety by design.

## 2. The `step-parallel` Special Form

The primary interface for concurrency is the `step-parallel` special form.

### Syntax

```clojure
(step-parallel
  (expression-1)
  (expression-2)
  ...)
```

### Semantics (Specification)

When the evaluator encounters `step-parallel`:

1.  **Capture**: It captures the current lexical environment.
2.  **Package**: It wraps each expression into a 0-arity closure: `(fn [] expression-n)`.
3.  **Yield**: It yields control to the Host with a request to the `:ccos.concurrent/parallel` capability, passing the closures as arguments.
4.  **Resume**: It waits for the Host to return a vector of results (matching the order of expressions).

### Example

```clojure
;; RTFS Code
(let [user-id 123]
  (step-parallel
    (call :user.get-profile user-id)    ; Branch 1
    (call :user.get-history user-id)))  ; Branch 2
```

**Internal Execution Flow**:
1.  Evaluator creates closures: `[(fn [] (call ...)), (fn [] (call ...))]`.
2.  Evaluator yields: `RequiresHost(:ccos.concurrent/parallel, [Closure1, Closure2])`.
3.  Host spawns 2 async tasks.
4.  Host creates 2 new Evaluators (sharing code/registry, forked environment).
5.  Host executes closures in parallel.
6.  Host returns `[Profile, History]`.
7.  Evaluator resumes and returns the vector.

## 3. Host Capability: `:ccos.concurrent/parallel`

The Host must implement the `:ccos.concurrent/parallel` capability to support this model.

### Capability Contract

-   **ID**: `ccos.concurrent/parallel`
-   **Input**: `[Function]` (Variadic list of RTFS functions)
-   **Output**: `[Value]` (Vector of results in order)
-   **Error Behavior**:
    -   If any branch fails, the Host typically returns the first error (fail-fast) or a composite error object (depending on configured policy).
    -   Cancellations are propagated to all running branches.

### Isolation and Safety

Since RTFS data structures are immutable and closures capture the environment at the point of definition:
-   **No Shared Mutable State**: Branches cannot modify variables in the parent scope.
-   **Thread Safety**: The Host can safely run branches in separate threads without locking RTFS data.

## 4. Advanced Patterns

### Parallel Map

User-space functions can build on `step-parallel` to implement patterns like `pmap`.

*(Note: This requires `apply` or macro support to expand dynamic lists into `step-parallel` arguments, or a direct `pmap` capability from the Host).*

```clojure
;; Conceptual pmap implementation using host capability directly
(defn pmap [f coll]
  (let [tasks (map (fn [x] (fn [] (f x))) coll)]
    (call :ccos.concurrent/parallel tasks)))
```

### Futures and Async/Await

For non-blocking coordination without strict parallelism blocks, RTFS relies on Host "Futures".

```clojure
;; Start a background task via Host
(let [future-id (call :ccos.async/spawn (fn [] (heavy-work)))]
  ;; Do local work...
  (local-work)
  ;; Await result
  (call :ccos.async/await future-id))
```

## 5. Implementation Strategy (Guide)

To implement this model in CCOS:

1.  **Evaluator**: Modify `eval_step_parallel_form` to stop executing loop sequentially. Instead, construct `Value::Function` objects for each expression and yield `ExecutionOutcome::RequiresHost`.
2.  **Host**: Register `ccos.concurrent/parallel`.
3.  **Runtime**: The capability implementation needs access to the `ModuleRegistry` to spawn lightweight `Evaluator` instances for each task.

## 6. Comparison with Previous Models

| Feature | Old Model (Deprecated) | New Model (Host-Mediated) |
| :--- | :--- | :--- |
| **Keyword** | `parallel` | `step-parallel` (or direct capability call) |
| **Execution** | Unclear/Mixed | Explicit Host Delegation |
| **State** | Potential races | Strictly Isolated (Immutable capture) |
| **Mechanism** | Evaluator magic | Standard Capability System |
