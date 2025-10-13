# RTFS 2.0: Continuation-Passing and Host Boundary

## 1. The Host Boundary Principle

RTFS implements a strict separation between pure computation and external effects through the **Host Boundary**. This architectural decision ensures safety, testability, and composability by making all side effects explicit and controllable.

### Core Philosophy

- **RTFS Engine**: Pure, deterministic, side-effect-free computation
- **Host Environment**: Manages all external interactions (I/O, time, randomness, etc.)
- **Boundary Crossing**: Explicit, auditable transitions between pure and effectful code

### Why This Matters

```rtfs
;; Pure RTFS code - always safe, testable, predictable
(defn pure-computation [data]
  (let [result (transform data)]
    (validate result)))

;; Effectful operations - mediated by host
(call :fs.write "/file.txt" (pure-computation input))
```

## 2. Continuation-Passing Execution Model

RTFS uses **continuation-passing style (CPS)** internally, where functions return control flow decisions rather than final values.

### Execution Outcomes

Every RTFS evaluation returns one of two outcomes:

1. **Final Value**: `ExecutionOutcome::Value(value)` - computation complete
2. **Host Request**: `ExecutionOutcome::RequiresHost(host_call)` - needs external effect

### Host Call Structure

```rtfs
;; Conceptual structure of a host call
{:capability :fs.read           ; Which host capability to invoke
 :args ["/path/to/file"]        ; Arguments to pass
 :continuation continuation-fn  ; What to do with the result
 :metadata {:timeout 5000}}     ; Optional control parameters
```

### Execution Flow

```
RTFS Code Execution
        ↓
    Evaluates expression
        ↓
   Needs external effect?
   ├─ No → Return final value
   └─ Yes → Yield to host
             ↓
       Host processes request
             ↓
       Host resumes RTFS with result
             ↓
       Continue evaluation
```

## 3. Host Capability System

Host capabilities provide controlled access to external resources through a structured API.

### Core Capabilities

#### File System Operations
```rtfs
;; Reading files
(call :fs.read "/path/to/file")
(call :fs.read-text "/path/to/file")
(call :fs.write "/path/to/file" "content")
(call :fs.append "/path/to/file" "more content")
(call :fs.exists? "/path/to/file")
(call :fs.list "/directory")
(call :fs.mkdir "/new/directory")
(call :fs.delete "/path/to/file")
```

#### Network Operations
```rtfs
;; HTTP requests
(call :http.get "https://api.example.com/data")
(call :http.post "https://api.example.com/submit" {:data payload})
(call :http.put "https://api.example.com/update" {:id 123 :data payload})
(call :http.delete "https://api.example.com/item/123")

;; WebSocket connections
(call :ws.connect "wss://api.example.com/stream")
(call :ws.send connection-id message)
(call :ws.receive connection-id)
```

#### Time and Scheduling
```rtfs
;; Current time
(call :time.now)
(call :time.unix-timestamp)

;; Delays and timeouts
(call :time.delay 1000)  ; milliseconds
(call :time.timeout operation 5000)

;; Scheduling
(call :time.schedule-at timestamp callback)
(call :time.schedule-in 60000 callback)  ; 1 minute from now
```

#### Randomness and Cryptography
```rtfs
;; Random values
(call :random.uuid)
(call :random.integer 1 100)
(call :random.bytes 32)

;; Cryptographic operations
(call :crypto.hash :sha256 "data")
(call :crypto.hmac :sha256 "key" "message")
(call :crypto.encrypt :aes "key" "plaintext")
(call :crypto.decrypt :aes "key" "ciphertext")
```

#### System Information
```rtfs
;; Environment
(call :env.get "PATH")
(call :env.get-all)
(call :env.set "MY_VAR" "value")

;; Process information
(call :process.pid)
(call :process.cwd)
(call :process.args)

;; System resources
(call :system.memory-usage)
(call :system.cpu-usage)
(call :system.platform)
```

### Custom Capabilities

Hosts can provide domain-specific capabilities:

```rtfs
;; Database operations
(call :db.query "SELECT * FROM users WHERE id = ?" [user-id])
(call :db.insert :users {:name "Alice" :email "alice@example.com"})
(call :db.update :users {:id 123 :name "Bob"})
(call :db.delete :users 123)

;; Message queues
(call :queue.publish "my-queue" message)
(call :queue.consume "my-queue" handler)
(call :queue.ack message-id)

;; External service calls
(call :service.call :user-service :get-profile {:user-id 123})
(call :service.call :payment-service :charge {:amount 99.99 :card card-data})
```

## 4. Continuation Patterns

RTFS supports several continuation patterns for complex control flow.

### Callback Continuations

```rtfs
;; Basic callback
(call :async.compute
  (fn [result]
    (println "Result:" result)
    (process result)))

;; Nested callbacks (callback hell)
(call :fs.read "/config.json"
  (fn [config]
    (call :http.get (:api-url config)
      (fn [response]
        (call :fs.write "/result.json" response
          (fn [_]
            (println "Done!")))))))
```

### Promise-like Operations

```rtfs
;; Sequential operations
(defn fetch-and-process [url]
  (call :http.get url
    (fn [data]
      (let [processed (transform data)]
        (call :fs.write "/output.json" processed
          (fn [_] :success))))))

;; Parallel operations with coordination
(defn fetch-multiple [urls]
  (let [results (atom {})]
    (doseq [url urls]
      (call :http.get url
        (fn [data]
          (swap! results assoc url data)
          (when (= (count @results) (count urls))
            (finalize @results)))))))
```

### Error Handling in Continuations

```rtfs
;; Error propagation
(call :fs.read "/file.txt"
  (fn [result]
    (if (error? result)
        (handle-error result)
        (process result))))

;; Try-catch with continuations
(defn safe-operation []
  (try
    (call :risky.operation)
    (catch :network-error e
      (retry-with-backoff e))
    (catch :timeout e
      (use-cached-result))))
```

## 5. Streaming and Incremental Processing

RTFS supports incremental data processing through host-mediated streaming.

### Stream Types

```rtfs
;; File streaming
(call :stream.file.read "/large-file.txt"
  {:chunk-size 8192
   :on-chunk (fn [chunk] (process-chunk chunk))
   :on-complete (fn [] (finalize-processing))})

;; HTTP streaming
(call :stream.http.get "https://api.example.com/stream"
  {:on-data (fn [chunk] (accumulate-data chunk))
   :on-end (fn [] (process-complete-data))})

;; WebSocket streaming
(call :stream.ws.connect "wss://api.example.com/live"
  {:on-message (fn [msg] (handle-realtime-update msg))
   :on-error (fn [err] (handle-connection-error err))})
```

### Backpressure and Flow Control

```rtfs
;; Controlled streaming with backpressure
(call :stream.controlled
  {:source source-stream
   :processor (fn [chunk]
                (if (can-process-more?)
                    (process-chunk chunk)
                    :pause-stream))
   :high-water-mark 1000
   :low-water-mark 100})
```

### Stream Composition

```rtfs
;; Pipeline of stream processors
(defn process-data-pipeline [input-stream]
  (-> input-stream
      (map-stream validate-chunk)
      (filter-stream valid?)
      (map-stream transform-data)
      (reduce-stream aggregate-results)))
```

## 6. Resource Management

The host boundary enables proper resource lifecycle management.

### Automatic Resource Cleanup

```rtfs
;; File handles
(call :fs.with-open "/file.txt" :read
  (fn [handle]
    (let [content (read-all handle)]
      (process content))))
;; File automatically closed when continuation completes

;; Database connections
(call :db.with-connection "postgresql://..."
  (fn [conn]
    (call :db.query conn "SELECT * FROM users")))
;; Connection automatically returned to pool

;; Network connections
(call :http.with-client {:timeout 30000}
  (fn [client]
    (call :http.get client "https://api.example.com")))
;; Client automatically cleaned up
```

### Manual Resource Management

```rtfs
;; Explicit resource control
(let [conn (call :db.connect "postgresql://...")]
  (try
    (let [result (call :db.query conn "SELECT * FROM users")]
      (process result))
    (finally
      (call :db.close conn))))
```

## 7. Security and Sandboxing

The host boundary provides the foundation for RTFS's security model.

### Capability-Based Access Control

```rtfs
;; Host enforces permissions
(call :fs.read "/sensitive/file")  ; May be denied based on context

;; Fine-grained permissions
(call :db.query "SELECT * FROM users"
  {:allowed-tables ["users"]})    ; Host validates access
```

### Audit Trail

Every host call is logged and auditable:

```rtfs
;; Host logs all calls
(call :fs.write "/audit.log" "User action performed")
;; Log entry: {timestamp, capability, args, context, result}
```

### Sandboxing Levels

- **Pure RTFS**: No external access
- **Controlled**: Limited capabilities based on context
- **Trusted**: Full host access (for system components)

## 8. Performance Considerations

### Yield Overhead

Each host call involves context switching:

```rtfs
;; High-frequency calls (inefficient)
(doseq [item items]
  (call :db.save item))  ; N separate yields

;; Batched operations (efficient)
(call :db.save-batch items)  ; Single yield
```

### Optimization Strategies

- **Batch Operations**: Group related calls
- **Streaming**: Process large data incrementally
- **Caching**: Host-managed result caching
- **Connection Pooling**: Reuse expensive resources

### Benchmarking Host Calls

```rtfs
;; Measure host call performance
(call :benchmark.time
  (fn []
    (call :expensive.operation)))
;; Returns timing information
```

## 9. Testing and Mocking

The host boundary enables comprehensive testing.

### Mock Host for Testing

```rtfs
;; Test with mock host
(with-mock-host {:fs.read (fn [_] "mock content")}
  (let [content (call :fs.read "/test.txt")]
    (assert (= content "mock content"))))
```

### Integration Testing

```rtfs
;; Test with real host but controlled environment
(with-test-environment
  (let [result (my-rtfs-program)]
    (assert-expected-result result)))
```

## 10. Implementation Architecture

### Host Interface

```rust
trait HostInterface {
    fn call_capability(&self, call: HostCall) -> ExecutionOutcome;
    fn list_capabilities(&self) -> Vec<String>;
    fn get_capability_info(&self, name: &str) -> Option<CapabilityInfo>;
}
```

### Execution Engine

```rust
enum ExecutionOutcome {
    Value(Value),
    RequiresHost(HostCall),
}

struct HostCall {
    capability: String,
    args: Vec<Value>,
    continuation: Continuation,
    metadata: HashMap<String, Value>,
}
```

### Continuation Representation

```rust
type Continuation = Box<dyn FnOnce(Value) -> ExecutionOutcome>;
```

## 11. Advanced Patterns

### Cooperative Multitasking

```rtfs
;; Yield control cooperatively
(defn cooperative-task []
  (loop [state initial-state]
    (let [result (process-batch state)]
      (if (more-work? result)
          (call :yield.control
            (fn [_] (recur (next-state result))))
          result))))
```

### State Machines

```rtfs
;; State machine with host-mediated transitions
(defn state-machine [initial-state]
  (letfn [(step [state]
            (case (:phase state)
              :init (call :init-phase
                      (fn [result] (step (assoc state :data result))))
              :process (call :process-phase (:data state)
                        (fn [result] (step (assoc state :result result))))
              :done (:result state)))]
    (step initial-state)))
```

### Error Recovery

```rtfs
;; Circuit breaker pattern
(defn resilient-call [capability args]
  (call :circuit-breaker.execute
    {:capability capability
     :args args
     :fallback (fn [] default-value)
     :timeout 5000
     :retry-count 3}))
```

This continuation-passing architecture provides RTFS with powerful control flow capabilities while maintaining strict safety and composability guarantees through the host boundary.