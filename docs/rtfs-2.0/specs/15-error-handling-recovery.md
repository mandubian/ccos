# RTFS 2.0: Error Handling

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