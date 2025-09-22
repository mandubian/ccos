# RTFS 2.0: The Concurrency Model

Status: Active
Last updated: 2025-09-18

## 1. The Principle: The RTFS Core is Synchronous

The RTFS 2.0 language engine is fundamentally **single-threaded and synchronous**. It evaluates one expression at a time. The core language has no built-in concepts of `async`, `await`, `futures`, `promises`, or threads.

This is not a limitation; it is a deliberate design choice that preserves the simplicity, purity, and predictability of the language core. The responsibility for managing concurrency and asynchronous operations belongs entirely to the **Host environment**.

## 2. Rationale: Why Concurrency is a Host Concern

1.  **Preserving Purity and Simplicity**: Introducing an `async` event loop and scheduler into the RTFS evaluator would dramatically increase its complexity. It would mean the engine is no longer a simple, pure "calculator" but a complex runtime system. This would violate the core philosophy.
2.  **Delegation of Power**: The Host environment (like CCOS) is far better equipped to manage concurrency. It can maintain thread pools, handle network I/O with native efficiency, implement sophisticated cancellation and timeout policies, and integrate with the underlying operating system's scheduler. Forcing RTFS to manage this would be reinventing the wheel poorly.
3.  **Avoiding "Colored Functions"**: Languages with built-in `async` often suffer from the "what color is your function?" problem, where `async` and `sync` code do not mix easily. By keeping the RTFS core purely synchronous, we avoid this entire class of problems within the language itself. The boundary is clean: all RTFS code is sync, and all asynchrony is managed outside of it by the Host.

## 3. The Model: Structured Concurrency via Host Capabilities

The recommended pattern for handling concurrency is for the Host to provide a **structured concurrency** capability. This allows RTFS code to declaratively request the parallel execution of multiple tasks without having any knowledge of how that parallelism is achieved.

### The `:parallel` Capability Pattern

The Host should expose a capability, conventionally named `:parallel`, that takes a map of tasks and executes them concurrently.

-   **`task` form**: This is a simple data constructor, perhaps a macro `(task ...)` that expands to `(list :task '(...))`, which represents a deferred operation. It is just data.
-   **`(call :parallel tasks-map)`**: This is the single, synchronous call to the Host.
    -   The RTFS engine yields to the Host, passing the map of `task` data structures.
    -   The Host receives the map, unpacks the tasks, and uses its own scheduler to execute them all in parallel.
    -   The Host waits for **all** tasks to complete.
    -   The Host resumes the RTFS engine with a single map containing the results of each task, using the same keys.

### Example

```rtfs
;; This RTFS code wants to fetch a user and their posts concurrently.
;; It declaratively describes the work to be done.

(let [work-to-do {:user (task :api.get-user 123)
                  :posts (task :api.get-posts-for-user 123)
                  :prefs (task :api.get-user-prefs 123)}]

  ;; A single, blocking yield to the Host.
  (let [results (call :parallel work-to-do)]

    ;; The Host has returned, and 'results' contains all the data.
    ;; The rest of the code is pure data transformation.
    (let [user (:user results)
          posts (:posts results)
          prefs (:prefs results)]
      
      (process-user-dashboard user posts prefs))))
```

### Benefits of this Model

-   **Declarative**: The RTFS code describes *what* it wants, not *how* to execute it concurrently. This is ideal for AI-generated code.
-   **Efficient**: It reduces the communication between RTFS and the Host to a single yield-and-resume cycle for a whole batch of concurrent work.
-   **Maintains Purity**: The RTFS engine's role is simple: it builds a data structure (a map) and later receives a data structure (a map). It remains a pure, synchronous calculator.
-   **Host Power**: The Host is free to use the most efficient possible concurrency strategy (e.g., a native thread pool, an async event loop like Tokio or asyncio) without the RTFS language needing to know any of the details.

## 4. Alternative (Discouraged) Pattern: Host-Managed Futures

An alternative is for the Host to provide capabilities that return an opaque "future" or "promise" token, along with an `:await` capability.

```rtfs
;; This pattern is more "chatty" and less declarative.
(let [user-future (call :api.get-user-async 123)
      posts-future (call :api.get-posts-for-user-async 123)]
  
  (let [user (call :await user-future)
        posts (call :await posts-future)]
    ...))
```

While this works, it is generally discouraged because it requires multiple round-trips to the Host and introduces more complexity into the RTFS code's control flow. The structured concurrency pattern is cleaner and more aligned with the RTFS philosophy.
