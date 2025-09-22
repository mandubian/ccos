# RTFS 2.0: Architectural Trade-offs and Design Rationale

This document clarifies the key architectural trade-offs in RTFS 2.0. These are not flaws, but deliberate design choices that reinforce the core principles of safety, purity, and verifiability, especially in the context of an AI-driven system.

## 1. Consideration: The "Chattiness" of the Host Boundary

The yield-based model is explicit and secure, but it can be "chatty." An RTFS program that performs many small host operations will involve numerous back-and-forth context switches between the RTFS engine and the Host.

**Example**: A loop that reads 100 small items from a database one by one.

```rtfs
(map (fn (id) (call :db.get-item id))
     (range 1 101))
```

This would result in 100 separate yields to the host (`RequiresHost`), each followed by a resumption.

**Potential Impact**:
- **Performance Overhead**: Each yield/resume cycle has a cost. For high-performance, I/O-intensive tasks, this could become a bottleneck.

**Mitigation Strategy**:
The architecture solves this by encouraging a shift away from iterative, single-item processing towards declarative, bulk operations.

1.  **Bulk Host APIs**: The primary solution is to design Host capabilities that operate on batches of data. Instead of `(call :db.get-item id)`, the Host must provide `(call :db.get-items [id1 id2 ...])`. This reduces N yields to 1.
2.  **Structured Concurrency**: For concurrent tasks, the `(call :parallel ...)` pattern is the standard. It bundles N independent asynchronous operations into a single, efficient yield to the Host.

This approach not only improves performance but also leads to a more declarative style that is better suited for AI code generation.

## 2. Consideration: Debugging and Stack Traces

In a traditional language, a stack trace shows a clear, linear sequence of function calls. In RTFS 2.0, the "stack" is split between the RTFS engine and the Host.

**Problem**:
- When an error occurs inside a Host capability (e.g., a network error), how do we present a unified stack trace that includes both the RTFS call site and the Host's internal call stack?
- Tracing the logical flow of an execution requires inspecting both the RTFS code and the Host's execution log.

**Mitigation Strategies**:
1.  **Rich Error Payloads**: When a Host call fails, the Host should resume the RTFS engine with a structured error object, not just a simple string. This object can contain the Host-side stack trace and other diagnostic information.
2.  **Correlated Logging**: The Host is responsible for maintaining a correlation ID for each top-level RTFS execution. All logs, both from the RTFS engine (via a `:log` capability) and from the Host itself, must include this ID. This allows for reconstructing the complete flow in an external logging system.
3.  **Tooling**: A dedicated debugger or trace visualizer is a natural extension for a Host environment. Such a tool would be "Host-aware" and could stitch together the two sides of the execution into a unified view.

## 3. Consideration: The Macro System's Complexity

Macros are incredibly powerful, but they are also a sharp tool. They introduce a second level of evaluation (compile-time) that can be difficult for newcomers to grasp.

**Problem**:
- **"When does this code run?"**: It can be hard to reason about whether a piece of code runs at compile time (in the macro) or at runtime.
- **Error Reporting**: An error in a macro expansion can produce cryptic error messages that point to the generated code, not the original macro call.
- **Abuse**: Overuse of macros can lead to dialects of RTFS that are unrecognizable, defeating the purpose of a simple, common language.

**Mitigation Strategies**:
1.  **Clear Best Practices**: Documentation must strongly advocate for using macros primarily as syntactic sugar for Host capabilities or for eliminating well-defined boilerplate. Discourage "magic" macros that obscure control flow.
2.  **Macro-expansion Tooling**: A standard tool (e.g., a REPL command or an IDE feature) to show the expanded form of a macro call is essential for debugging.
3.  **Restrict Macro-defining Capabilities**: In secure contexts, the Host can choose not to expose `defmacro` at all, only allowing the execution of pre-defined macros.

## 4. Consideration: Concurrency is a Host Responsibility

A deliberate design choice in RTFS 2.0 is that the core language has no built-in model for asynchronous operations (`async/await`, promises, etc.). This responsibility is delegated entirely to the Host.

**Rationale**:
- **Purity and Simplicity**: This keeps the RTFS core engine simple, synchronous, and verifiable. It does not need a complex internal scheduler.
- **Power and Flexibility**: The Host is far better equipped to handle concurrency, using native thread pools and async I/O for maximum performance.

**The Standard Pattern: Structured Concurrency**
As defined in the `08-concurrency-model.md` specification, the official pattern is for the Host to provide a `:parallel` capability.

```rtfs
;; The RTFS code declaratively describes the concurrent work.
(let [results (call :parallel {:user (task :api.get-user 1)
                               :posts (task :api.get-posts-for 1)})]
  ;; The Host executes the tasks and returns the results in a single map.
  (let [user (get results :user)
        posts (get results :posts)]
    ...))
```
This approach keeps the RTFS logic simple and declarative, while delegating the complex execution to the Host. It is not a flaw, but a core feature of the architecture's separation of concerns.

## Conclusion

The architectural trade-offs of RTFS 2.0 are deliberate and mutually reinforcing. The "chattiness" of the Host boundary is mitigated by declarative bulk and concurrent capabilities. The lack of built-in state and asynchrony is not a missing feature, but a design choice that enables a simpler, more secure, and verifiable language core.

By delegating these responsibilities to the Host, RTFS solidifies its role as a pure, portable, and safe logic engine, making it the ideal foundation for a sophisticated AI orchestration system. The official specifications for state management and concurrency provide the standardized patterns for a robust and consistent implementation.
