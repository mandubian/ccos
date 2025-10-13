# RTFS - Resource Management Specification (Draft)

This document outlines the approach to managing external resources (like files, network connections, database handles) within the standalone RTFS language, aiming for safety and predictability.

## 1. Goals

*   **Safety:** Prevent resource leaks (e.g., forgetting to close files or release handles).
*   **Prevent Use-After-Release:** Disallow operations on resources that have already been released or closed.
*   **Clarity:** Make resource lifetimes explicit through scoped constructs.
*   **Composability:** Allow safe composition of functions that use resources.

## 2. Core Concepts

*   **Resource Handles:** External resources requiring explicit lifecycle management (opening/closing, connecting/disconnecting, allocating/releasing) are represented by opaque **handles**. These are first-class values in RTFS.
*   **Resource Types:** Each handle has a specific **resource type** (e.g., `FileHandle`, `SocketHandle`, `DatabaseConnectionHandle`, `TensorHandle`) defined by the type system using `[:resource ResourceName]`. See `type_system.md`.
*   **Distinction from Value Types:** Standard value types (like `:int`, `:string`, `[:vector :int]`, or even `[:array :float [10 20]]`) generally do *not* require this explicit management. Their lifetime is typically tied to lexical scope and garbage collection (if applicable in the runtime). Resource handles represent things that need explicit cleanup actions beyond simple memory deallocation.
*   **Scoped Management:** The primary mechanism for ensuring safe and timely cleanup of resource handles is lexical scoping combined with the `with-resource` construct.

## 3. Language Constructs

*   **`with-resource` Block:**
    *   Syntax: `(with-resource [binding [:resource ResourceType] init-expr] body...)`
    *   Semantics:
        1.  Evaluates `init-expr`, which must return a resource handle of a type compatible with `[:resource ResourceType]` (e.g., `(tool:open-file ...)` returning `[:resource FileHandle]`).
        2.  Binds the acquired handle to `binding` within the `body`. The handle is valid for use within this lexical scope.
        3.  Executes the `body` expressions. The handle can be passed to other functions called within the body.
        4.  **Automatic Cleanup:** When execution leaves the `body` scope (either normally by reaching the end, or prematurely due to an error propagating out), a cleanup operation associated with the `ResourceType` (e.g., `tool:close-file` for `FileHandle`, `tool:disconnect-db` for `DatabaseConnectionHandle`, `tool:release-tensor` for `TensorHandle`) is **automatically invoked** by the runtime on the handle.
        5.  After cleanup, the handle is considered released or closed.
    *   Examples:
        ```acl
        ;; Local File
        (with-resource [f [:resource FileHandle] (tool:open-file "data.txt")]
          (tool:read-line f)) ;; 'f' is automatically closed after this block

        ;; Database Connection
        (with-resource [conn [:resource DatabaseConnectionHandle] (tool:connect-db "jdbc:mydb://host/db?user=x&pass=y")]
          (let [results (tool:query conn "SELECT name FROM users")]
            (process-results results))) ;; 'conn' is automatically disconnected after this block

        ;; Nested Resources
        (with-resource [in-handle [:resource FileHandle] (tool:open-file "in.txt")]
          (with-resource [out-handle [:resource FileHandle] (tool:open-file "out.txt")]
            (transfer-data in-handle out-handle)) ;; out-handle closed first, then in-handle
          )
        ```
    *   **Error Safety:** The automatic cleanup ensures resources are released even if an error occurs within the `body` and propagates outwards (assuming the runtime correctly implements the unwinding mechanism for `with-resource`).

## 4. Runtime Checks for Safety

Since RTFS relies on runtime enforcement rather than strict compile-time linearity checking:

*   **Handle State Tracking:** The runtime must associate a state with each active resource handle (e.g., `:active`, `:released`).
*   **Operation Validation:** Before performing any operation using a resource handle (e.g., `tool:read-line`, `tool:query`), the runtime must check the handle's state.
*   **Use-After-Release Prevention:** Attempting to use a handle that is already in the `:released` state must result in a runtime error (e.g., `{:type :error/resource-released :message "Attempted to use released handle"}`).
*   **State Update:** When the cleanup action associated with `with-resource` completes successfully, the runtime atomically updates the handle's state to `:released`.

## 5. Type System Integration

*   Resource types (`[:resource FileHandle]`, etc.) are distinct opaque types known to the type system.
*   The type checker verifies that the `init-expr` in `with-resource` produces a handle of the expected resource type.
*   The type checker ensures that handles are passed to functions expecting the correct resource type.
*   While the core type system doesn't prevent use-after-release statically, the combination of `with-resource` for guaranteed cleanup and runtime state checks provides the necessary safety guarantees.

This approach provides a pragmatic and robust way to manage resources in RTFS, leveraging scoped constructs for automatic cleanup and runtime checks to prevent invalid handle usage. It avoids the complexity of full linear types while achieving similar safety goals for managed resources.
