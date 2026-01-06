# RTFS 2.0: Error Handling

## Implementation Status

**✅ Implemented - Fully functional**

Error handling in RTFS 2.0 is fully implemented with comprehensive error types and recovery mechanisms. The implementation status is:

| Feature | Status | Notes |
|---------|--------|-------|
| **Try-Catch-Finally** | ✅ **Implemented** | `try`, `catch`, `finally` special forms in grammar and AST |
| **RuntimeError** | ✅ **Implemented** | Evaluation errors, type mismatches, undefined symbols |
| **HostError** | ✅ **Implemented** | Capability execution failures, network errors, permissions |
| **ValidationError** | ✅ **Implemented** | Type validation failures, schema violations |
| **Error Propagation** | ✅ **Implemented** | Stack unwinding with proper context preservation |
| **Exception Objects** | ✅ **Implemented** | Structured error information with type and details |
| **Multiple Catch Clauses** | ✅ **Implemented** | Pattern matching on error types |
| **Resource Cleanup** | ✅ **Implemented** | `finally` blocks execute regardless of success/error |
| **Error Recovery Patterns** | ✅ **Implemented** | Retry, fallback, degradation via host capabilities |

### Key Implementation Details
- **Structured Error Hierarchy**: `RuntimeError`, `HostError`, `ValidationError` with detailed context
- **Stack Trace Preservation**: Error objects include execution context for debugging
- **Host Boundary Integration**: Host errors propagate as `HostError` with capability context
- **Type System Integration**: Validation errors from type checking include schema details
- **Recovery Patterns**: Common patterns implemented via standard library functions
- **Deterministic Behavior**: Error handling maintains referential transparency

### Implementation Reference
- `ast.rs`: `TryCatch` AST node with `catch` and `finally` branches
- `runtime/error.rs`: `RuntimeError` enum with detailed error variants
- `runtime/execution_outcome.rs`: Error propagation through `ExecutionOutcome`
- `runtime/host.rs`: `HostError` conversion from capability failures
- `runtime/type_validator.rs`: `ValidationError` generation for type violations
- Integration tests: Comprehensive error handling test coverage

**Note**: Error handling is production-ready with comprehensive test coverage. All error types and recovery mechanisms are implemented and integrated with the host boundary and type system.

## 1. Error Types

RTFS defines three primary error types that can occur during execution:

### RuntimeError
Runtime errors occur during expression evaluation:
- Type mismatches
- Undefined symbol references
- Invalid function calls
- Arithmetic errors (division by zero, overflow)

### HostError
Host errors occur when the host environment cannot fulfill a capability request:
- File not found
- Network connection failures
- Permission denied
- Invalid capability arguments

### ValidationError
Validation errors occur during type checking and validation:
- Type annotation violations
- Schema validation failures
- Contract violations

## 2. Try-Catch Error Handling

RTFS provides basic exception handling through try-catch blocks:

```rtfs
(try
  (risky-operation)
  (catch RuntimeError e
    (handle-runtime-error e))
  (catch HostError e
    (handle-host-error e)))
```

### Catch Clauses
- Multiple catch clauses for different error types
- Exception object contains error details
- Catch-all clause using base Exception type

## 3. Error Propagation

Errors propagate up the call stack until caught:

```rtfs
(defn outer []
  (try
    (inner)
    (catch Exception e
      (println "Caught in outer:" e)
      (throw e))))

(defn inner []
  (throw (RuntimeError. "Something went wrong")))
```

## 4. Host Boundary Error Handling

When host calls fail, the error is propagated back to RTFS:

```rtfs
;; Host call that might fail
(try
  (call :fs.read "/nonexistent/file")
  (catch HostError e
    (case (:type e)
      :not-found (error "File not found")
      :permission-denied (error "Access denied"))))
```

## 5. Error Context

Errors can include contextual information:

```rtfs
(defn process-user [user]
  (try
    (validate-user user)
    (save-user user)
    (catch Exception e
      (throw (with-context e {:operation "user-processing"
                              :user-id (:id user)})))))
```

## 6. Error Recovery Patterns

### Basic Retry Logic

```rtfs
(defn retry-operation [op max-attempts]
  (loop [attempts 0]
    (try
      (op)
      (catch HostError e
        (if (< attempts max-attempts)
          (do
            (sleep (* 1000 attempts)) ; Simple backoff
            (recur (inc attempts)))
          (throw e))))))
```

### Fallback Strategies

```rtfs
(defn safe-read-config []
  (try
    (call :fs.read "/config.json")
    (catch HostError e
      (default-config))))
```

## 7. Error Logging

Errors can be logged through host capabilities:

```rtfs
(try
  (risky-operation)
  (catch Exception e
    (call :log.error {:message "Operation failed"
                      :error e
                      :context {:operation "risky-op"}})
    (throw e)))
```

## 8. Error Types in Type System

Error types are part of the basic type system:

```rtfs
(defn divide [a b]
  {:type {:args [Integer Integer] :return {:ok Integer | :error String}}}
  (if (= b 0)
    {:error "Division by zero"}
    {:ok (/ a b)}))
```

This provides a foundation for more sophisticated error handling patterns while maintaining RTFS's simplicity and host-mediated design.