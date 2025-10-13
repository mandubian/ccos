# RTFS 2.0: Concurrency Model

## 1. Synchronous Core

RTFS evaluation is fundamentally synchronous - one expression at a time. Concurrency is handled entirely through the host environment using structured concurrency patterns.

## 2. Host-Mediated Parallelism

### Parallel Capability

The host provides a `:parallel` capability for concurrent execution:

```rtfs
;; Declarative parallel execution
(def work {:user (task :api.get-user 123)
           :posts (task :api.get-posts 123)
           :prefs (task :api.get-prefs 123)})

(def results (call :parallel work))

;; Results contain all concurrent operations
(let [user (:user results)
      posts (:posts results)
      prefs (:prefs results)]
  (process-data user posts prefs))
```

### Task Construction

Tasks are pure data structures representing deferred operations:

```rtfs
;; Task creation
(task :capability.name arg1 arg2 ...)
(task :fs.read "/file.txt")
(task :http.get "https://api.example.com/data")
```

## 3. Structured Concurrency

### Single Yield Point

Parallel execution uses one synchronous call to the host:

1. RTFS builds task map (pure data)
2. Single `call :parallel` yields to host
3. Host executes tasks concurrently
4. Host resumes RTFS with result map

### Benefits

- **Declarative**: RTFS describes what, not how
- **Efficient**: Single host round-trip
- **Pure**: RTFS core remains synchronous
- **Safe**: Host controls all parallelism

## 4. Sequential Fallback

For simple cases, sequential processing is preferred:

```rtfs
;; Sequential processing (simpler, more predictable)
(defn process-items [items]
  (map process-item items))
```

## 5. Error Handling in Parallel Operations

Errors in parallel tasks are collected and returned:

```rtfs
(def results (call :parallel
  {:success (task :safe.operation)
   :failure (task :risky.operation)}))

;; Check for errors
(if (:error results)
  (handle-parallel-error (:error results))
  (process-success (:success results)))
```

## 6. Performance Considerations

### When to Use Parallelism

- I/O bound operations (network, file access)
- Independent computations
- Multiple external service calls

### When to Avoid

- CPU-bound computations (GIL/serial execution)
- Dependent operations
- Simple sequential work

## 7. Host Implementation

The host manages:
- Thread pools
- Task scheduling
- Resource limits
- Timeout handling
- Error aggregation

RTFS remains focused on pure computation and data transformation.