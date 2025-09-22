# RTFS 2.0: The Host Boundary and Capabilities

## 1. The Core Principle: Separation of Concerns

The most critical architectural feature of RTFS 2.0 is the strict separation between the **pure RTFS language engine** and the **Host environment**.

-   **RTFS Engine**: Responsible for parsing, evaluation, scoping, and executing pure, deterministic logic. It has no built-in knowledge of the outside world.
-   **Host Environment**: The system that embeds the RTFS engine (e.g., CCOS). It is responsible for all side effects, such as I/O, network requests, and interaction with system resources.

This separation is what makes RTFS safe, portable, and verifiable.

## 2. The `HostInterface`: A Bridge to the World

The RTFS engine communicates with the Host through a single, well-defined mechanism: the `HostInterface`. This is a trait (or interface) that the Host must implement.

The RTFS runtime is given an implementation of this interface when it is created. From the engine's perspective, it is simply calling methods on an abstract object.

## 3. Yielding for Effects: The `RequiresHost` Outcome

RTFS does not *execute* side effects. It *requests* them. This is achieved through a **yield-based control flow**.

When the RTFS evaluator encounters a function call that it cannot resolve as a pure, built-in function or a user-defined function, it assumes the call is intended for the Host. Instead of producing an error, it **yields** control.

The `execute` method of the RTFS engine returns an `ExecutionOutcome` enum with two variants:

-   `Complete(Value)`: The expression was fully evaluated within the pure RTFS engine, and here is the final value.
-   `RequiresHost(HostCall)`: The engine has encountered a request for a host operation. The `HostCall` struct contains the details of the request, typically the function name (a symbol or keyword) and the evaluated arguments.

### The Execution Loop

The Host is responsible for driving the execution loop:

1.  **Host**: Calls `rtfs_engine.execute(expression)`.
2.  **RTFS Engine**:
    -   If the expression is pure, it returns `Complete(result)`. The loop terminates.
    -   If the expression contains a host call, it returns `RequiresHost(host_call_details)`.
3.  **Host**: Receives the `RequiresHost` request.
    -   It inspects `host_call_details`.
    -   It decides whether to fulfill the request (based on its own security policies).
    -   It performs the requested action (e.g., reads a file).
    -   It constructs a new expression to resume the RTFS engine, often by injecting the result of the host action.
    -   It goes back to step 1, calling `execute` again with the new expression.

This model ensures the Host has ultimate authority. It can deny, modify, or log any request from the RTFS code.

### Example: Reading a File

Consider the RTFS code: `(process-file "/data.txt")`

```rtfs
(def process-file (fn (path)
  (let [content (call :fs.read path)] ;; Host call
    (str "Content length: " (len content)))))
```

The evaluation flow would be:

1.  **Host**: Calls `execute` with `(process-file "/data.txt")`.
2.  **RTFS**: Evaluates the function call. Inside `process-file`, it encounters `(call :fs.read path)`.
3.  **RTFS**: `call` is a special form that signals a host operation. The engine evaluates the arguments (`:fs.read` and `"/data.txt"`) and yields, returning `RequiresHost({ function: :fs.read, args: ["/data.txt"] })`.
4.  **Host**: Receives the request.
    -   It checks if the code is allowed to read files.
    -   It reads the content of `/data.txt`, which is `"hello"`.
    -   It resumes the RTFS execution, effectively replacing the `(call ...)` form with the result, `"hello"`.
5.  **RTFS**: The `let` binding `content` is now bound to `"hello"`. The engine continues, calculates the length, and concatenates the string.
6.  **RTFS**: The expression is now fully evaluated. It returns `Complete("Content length: 5")`.
7.  **Host**: Receives the final result.

## 4. Capabilities: The Host's Vocabulary

A **capability** is a function that the Host exposes to the RTFS environment. These are the "verbs" that RTFS code can use to interact with the world.

Capabilities are typically identified by keywords (e.g., `:fs.read`, `:http.get`, `:db.query`). This is a convention, not a strict rule.

The `(call ...)` special form is the standard way to invoke a host capability.

**Syntax**: `(call capability-name arg1 arg2 ...)`

This form makes it explicit in the code that a boundary is being crossed.

### Standard vs. Custom Capabilities

A Host environment can provide any set of capabilities it chooses. However, we can envision a set of "standard" capability namespaces that promote interoperability:

-   `:fs.*`: Filesystem operations (`:fs.read`, `:fs.write`, `:fs.list`).
-   `:net.*`: Network operations (`:net.tcp.connect`, `:net.http.get`).
-   `:time.*`: Time-related functions (`:time.now`).
-   `:log.*`: Logging (`:log.info`, `:log.error`).
-   `:env.*`: Environment variables (`:env.get`).

A minimal Host used for pure validation might provide no capabilities at all. A powerful orchestration system like CCOS would provide a rich set of capabilities for interacting with its various subsystems.

## 5. Security and Sandboxing

The yield-based model is the foundation of RTFS security. Since the RTFS engine itself cannot perform side effects, it is inherently sandboxed.

The Host acts as the security guard. When it receives a `RequiresHost` request, it can use any information available to it—such as the identity of the code's author, the context of the execution, or a predefined set of policies—to decide whether to allow the operation.

This allows for fine-grained security control:

## 6. The Host in an AI System: The CCOS Example

To make the Host concept concrete, consider its implementation in CCOS (Cognitive-Causal Orchestration System). In CCOS, the "Host" is not a single object but a composition of several key components:

-   **The Orchestrator**: This is the component that drives the RTFS execution loop. It invokes the RTFS engine and is the first to receive a `RequiresHost` yield.

-   **The Governance Kernel**: When the Orchestrator receives a `RequiresHost` request, it does not execute it directly. Instead, it forwards the request to the Governance Kernel. This component is the primary security guard. It consults a set of policies (the "constitution") to determine if the requested capability call is permissible in the current context (e.g., which agent made the request, what are its permissions).

-   **The Capability Marketplace**: If the Governance Kernel approves the request, the Orchestrator dispatches it to the Capability Marketplace. This component is a registry of all available capabilities. It finds the provider for the requested capability (e.g., the `:fs.read` provider) and executes it.

-   **The Causal Chain**: The result of the capability execution, along with the original request and the governance decision, is recorded immutably in the Causal Chain for auditing and replay.

This layered architecture in CCOS demonstrates the power of the RTFS Host model:

1.  **RTFS code is simple**: It just makes a `(call ...)` request.
2.  The Host (CCOS) implements a sophisticated, multi-stage validation and execution pipeline.
3.  Security, auditing, and execution are cleanly separated, with the RTFS engine remaining unaware of this complexity.

This makes the system as a whole robust and secure, fulfilling the core requirements for coordinating autonomous AI agents.

-   A script might be allowed to read from `/tmp` but not `/etc`.
-   A script might be allowed to make HTTP GET requests to a specific domain but not POST requests.
-   A script running in a "dry-run" mode might have all of its write operations logged but not actually executed.

This is all managed by the Host, keeping the RTFS language core simple and secure by default.
