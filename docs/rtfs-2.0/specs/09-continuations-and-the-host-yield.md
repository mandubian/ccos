# RTFS 2.0 Spec: Continuations and the Host Yield

-   **Status**: `Draft`
-   **Version**: `0.1.0`
-   **Last Updated**: `2025-09-18`

## 1. Abstract

This document specifies the core mechanism that enables the pure RTFS engine to interact with the effectful Host environment: **continuation-passing style execution**. It details how the RTFS runtime can be paused, yield control to a host, and be resumed later. This is the cornerstone of the RTFS/Host boundary.

## 2. The Problem: Bridging Pure Code and an Impure World

The RTFS language is purely functional. It performs no I/O, has no concept of time, and cannot directly perform any side effects. The Host environment (e.g., CCOS), however, is all about side effects: accessing networks, reading files, managing state, and running concurrent tasks.

The central challenge is to allow a pure RTFS program to request services from the impure Host without polluting the language's semantics. The solution is a non-blocking, yield-based control flow.

## 3. The Continuation-Passing Mechanism

The interaction is a well-defined, three-step dance: **Execute -> Yield -> Resume**.

### Step 1: Execution in the Engine

The Host initiates a computation by asking the RTFS engine to evaluate an expression. The engine executes the code normally, following all the rules of pure functional evaluation.

### Step 2: Yielding on a `(call ...)`

When the evaluator encounters a `(call :capability.name {args})` expression, it does **not** attempt to resolve the call. Instead, it immediately halts its execution and yields control back to the Host.

Upon yielding, the engine provides the Host with a single, crucial object, which we'll call `HostCall`. This object contains two fields:

1.  **`request`**: An object describing the requested operation. This includes the capability name (`:capability.name`) and the evaluated arguments (`{args}`).
2.  **`continuation`**: An **opaque** data structure that represents the "rest of the program." This is a snapshot of the engine's execution state, including the call stack, lexical environments, and the instruction pointer.

From the Host's perspective, the `continuation` is a black box. It should not be inspected or modified, only stored.

### Step 3: Host Action and Resumption

The Host now has control. It inspects the `request` object and performs the required side effect. This can be a synchronous or asynchronous operation. The RTFS engine is dormant during this period and consumes no resources.

Once the Host's operation is complete, it will have a result (or an error). To continue the RTFS program, the Host invokes the RTFS engine's `resume` entry point, providing two arguments:

1.  The `continuation` object it received in Step 2.
2.  The `result` of the host operation.

The RTFS engine, being **re-entrant**, uses the `continuation` to perfectly restore its previous state. It then takes the `result` provided by the Host and treats it as the return value of the original `(call ...)` expression. The program then continues to execute from that point forward.

## 4. Lifecycle of the RTFS Engine

This model clarifies the lifecycle of the RTFS engine:

-   **The engine does not need to "stay alive"** in a persistent process or thread while waiting for the Host.
-   A `continuation` can be thought of as a **serializable, frozen state** of the RTFS virtual machine.
-   The Host can hold onto a continuation for microseconds or for days. When it decides to resume, the engine can be rehydrated from that state.

This has profound implications:

-   **Portability & Sandboxing**: A continuation could be serialized and sent over a network, or used to resume execution inside a completely different process or a new Wasm instance. This is ideal for security and resource management.
-   **Durability**: A Host could persist a continuation to disk, allowing a long-running RTFS "process" to survive host restarts.
-   **Advanced Control Flow**: The Host gains ultimate control over the execution of RTFS code, enabling it to implement complex scheduling, pre-emption, or debugging.

## 5. Example Flow

1.  **RTFS Code**:
    ```rtfs
    (let [user-id 123]
      (let [user-data (call :db.users:v1.get {id: user-id})]
        (step "Process user data"
          (string.upcase (:name user-data)))))
    ```

2.  **Execution & Yield**:
    -   Host asks Engine to run the code.
    -   Engine evaluates `(let [user-id 123] ...)`
    -   Engine hits `(call :db.users:v1.get {id: 123})`.
    -   Engine **yields** to Host with:
        -   `request`: `{ capability: ":db.users:v1.get", args: {id: 123} }`
        -   `continuation`: `<OpaqueContinuationObject>`

3.  **Host Action & Resume**:
    -   Host receives the `HostCall`.
    -   Host sees it needs to query the database for user `123`.
    -   Host performs the DB query and gets `{ "id": 123, "name": "alice" }`.
    -   Host calls `engine.resume()` with:
        -   The `<OpaqueContinuationObject>` it was holding.
        -   The result: `{ "id": 123, "name": "alice" }`.

4.  **Program Completion**:
    -   Engine restores its state. The `(call ...)` expression now has a value: the map `{id: 123, name: "alice"}`.
    -   This map is bound to the `user-data` variable.
    -   The program continues, evaluating `(string.upcase (:name user-data))`.
    -   The final result, `"ALICE"`, is returned to the Host.

This explicit, state-passing mechanism is fundamental to the power and safety of the RTFS 2.0 architecture.
