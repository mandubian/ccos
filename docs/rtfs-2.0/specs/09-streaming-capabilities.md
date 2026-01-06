# RTFS 2.0: Streaming Capabilities

## Implementation Status

**⚠️ Partial - Host-mediated via capabilities**

Streaming in RTFS 2.0 is implemented through host-mediated capabilities rather than native language primitives. The implementation status is:

| Feature | Status | Notes |
|---------|--------|-------|
| **MCP Streaming** | ✅ **Implemented** | `mcp-stream` macro with continuation-based processing (Phase 4-5 features) |
| **Streaming Inspection** | ✅ **Implemented** | `:mcp.stream.inspect` capability for live metrics |
| **Source/Channel-based** | ⚠️ **Partial** | Via `:marketplace.stream.start` and `:marketplace.stream.next` |
| **Callback-based** | ⚠️ **Partial** | Via `:marketplace.stream.register-callbacks` |
| **Sink/Produce** | ⚠️ **Partial** | Via `:marketplace.stream.open-sink` and `:marketplace.stream.send` |
| **Advanced Streaming** | ❌ **Design** | File streams, network streams, processing operations (map/filter/aggregation) |
| **Backpressure** | ⚠️ **Basic** | Queue capacity and pause/resume directives via processor actions |
| **State Management** | ⚠️ **Partial** | Processor state persists across chunks with continuation tokens |
| **Type Validation** | ❌ **Not Implemented** | Stream schemas and type validation |

### Key Implementation Details
- **MCP Integration**: Fully implemented with `mcp-stream` macro that lowers to `(call :mcp.stream.start ...)`
- **Continuation Processing**: Processors return `{:action :continue/:pause/:complete}` with updated state
- **Observability**: `:mcp.stream.inspect` capability provides live metrics and queue diagnostics
- **Host Boundary**: All streaming goes through CCOS capability marketplace
- **Phase Features**: Current implementation includes Phase 4-5 features (persistent state, bounded queues)

### Implementation Reference
– `mcp_streaming_provider.rs`: MCP streaming provider implementation
– `McpStreamingProvider::process_chunk`: Continuation processing logic
– `examples/stream_mcp_example.rtfs`: Example usage
– `tests/ccos-integration/mcp_streaming_mock_tests.rs`: Integration tests

**Note**: This specification describes comprehensive streaming capabilities, but only MCP streaming and basic marketplace integration are currently implemented. File streams, network streams, and advanced processing operations are design specifications for future implementation.

## 1. Streaming Overview

RTFS provides comprehensive streaming capabilities for processing large or continuous data through the host boundary. Streaming enables efficient handling of data that doesn't fit in memory, real-time processing, and incremental computation.

### Core Principles

- **Host-Mediated**: All streaming operations go through the host boundary
- **Backpressure-Aware**: Automatic flow control prevents resource exhaustion
- **Composable**: Streams can be chained and transformed
- **Resource-Safe**: Automatic cleanup and resource management

## 2. Stream Types and Sources

RTFS supports multiple stream types for different data sources.

### File Streams

```rtfs
;; Read large files incrementally
(call :stream.file.read "/large-dataset.json"
  {:chunk-size 8192
   :encoding :utf8
   :on-chunk (fn [chunk] (process-json-chunk chunk))
   :on-complete (fn [] (finalize-processing))
   :on-error (fn [err] (handle-file-error err))})

;; Write streams
(call :stream.file.write "/output.txt"
  {:data-source lazy-data-sequence
   :on-complete (fn [] (println "Write complete"))})
```

### Network Streams

```rtfs
;; HTTP streaming
(call :stream.http.get "https://api.example.com/large-dataset"
  {:headers {"Accept" "application/json"}
   :on-data (fn [chunk] (accumulate-data chunk))
   :on-end (fn [] (process-complete-data))
   :timeout 30000})

;; WebSocket streams
(call :stream.ws.connect "wss://api.example.com/realtime"
  {:on-open (fn [] (println "Connected"))
   :on-message (fn [msg] (handle-realtime-data msg))
   :on-error (fn [err] (handle-connection-error err))
   :on-close (fn [] (cleanup-resources))})
```

### In-Memory Streams

```rtfs
;; Stream from collections
(call :stream.from-collection [1 2 3 4 5 6 7 8 9 10]
  {:chunk-size 3
   :on-chunk (fn [chunk] (process-batch chunk))})

;; Stream to collection
(call :stream.to-collection
  {:source input-stream
   :on-complete (fn [result] (println "Collected:" result))})
```

### Generated Streams

```rtfs
;; Infinite streams
(call :stream.generate
  {:generator (fn [state]
                (if (< state 1000)
                    [state (inc state)]
                    :done))
   :initial-state 0
   :on-value (fn [value] (process-generated value))})

;; Periodic streams
(call :stream.periodic
  {:interval 1000  ; milliseconds
   :generator (fn [] (current-timestamp))
   :on-value (fn [timestamp] (log-heartbeat timestamp))})
```

## 3. Stream Processing Operations

RTFS provides functional stream processing operations.

### Mapping and Transformation

```rtfs
;; Transform each chunk
(call :stream.map input-stream
  {:transform (fn [chunk] (string/upper-case chunk))
   :on-chunk (fn [transformed] (send-to-output transformed))})

;; Parse JSON chunks
(call :stream.map json-stream
  {:transform (fn [chunk] (json/parse chunk))
   :on-chunk (fn [parsed] (validate-and-store parsed))})
```

### Filtering

```rtfs
;; Filter chunks based on predicate
(call :stream.filter data-stream
  {:predicate (fn [chunk] (> (count chunk) 0))
   :on-chunk (fn [filtered] (process-valid-chunk filtered))})

;; Remove duplicates
(call :stream.distinct data-stream
  {:key-fn (fn [item] (:id item))
   :on-chunk (fn [unique] (store-unique-item unique))})
```

### Aggregation and Reduction

```rtfs
;; Count items
(call :stream.reduce data-stream
  {:initial 0
   :reducer (fn [acc item] (inc acc))
   :on-result (fn [count] (println "Total items:" count))})

;; Group by key
(call :stream.group-by data-stream
  {:key-fn (fn [item] (:category item))
   :on-groups (fn [groups] (process-grouped-data groups))})
```

### Batching and Windowing

```rtfs
;; Fixed-size batches
(call :stream.batch data-stream
  {:size 100
   :on-batch (fn [batch] (process-batch batch))})

;; Time-based windows
(call :stream.window data-stream
  {:duration 5000  ; 5 seconds
   :on-window (fn [window] (analyze-time-window window))})

;; Sliding windows
(call :stream.sliding-window data-stream
  {:size 10
   :slide 5
   :on-window (fn [window] (compute-moving-average window))})
```

## 4. Stream Composition and Pipelines

Streams can be composed into processing pipelines.

### Pipeline Construction

```rtfs
;; Build processing pipeline
(defn create-data-pipeline [input-stream]
  (-> input-stream
      (stream.map validate-chunk)
      (stream.filter valid?)
      (stream.batch 50)
      (stream.map process-batch)
      (stream.reduce aggregate-results)))

;; Execute pipeline
(call :stream.pipeline
  {:pipeline (create-data-pipeline input-stream)
   :on-result (fn [final-result] (store-result final-result))})
```

### Branching and Merging

```rtfs
;; Split stream into multiple processing paths
(call :stream.branch input-stream
  {:branches [{:name :analytics
               :processor (fn [data] (send-to-analytics data))}
              {:name :storage
               :processor (fn [data] (store-for-later data))}]
   :on-complete (fn [] (println "All branches complete"))})

;; Merge multiple streams
(call :stream.merge
  {:streams [stream1 stream2 stream3]
   :strategy :round-robin  ; :round-robin, :priority, :fair
   :on-merged (fn [item] (process-merged-item item))})
```

### Error Handling in Pipelines

```rtfs
;; Pipeline with error recovery
(call :stream.pipeline-with-recovery
  {:pipeline processing-pipeline
   :error-handler (fn [error chunk]
                    (log-error error)
                    (retry-or-skip chunk))
   :on-result (fn [result] (handle-final-result result))})
```

## 5. Backpressure and Flow Control

RTFS streaming includes automatic backpressure management.

### Automatic Backpressure

```rtfs
;; Producer with backpressure
(call :stream.producer
  {:generator slow-data-generator
   :buffer-size 100
   :high-water-mark 80
   :low-water-mark 20
   :on-backpressure (fn [] (slow-down-production))})

;; Consumer with backpressure
(call :stream.consumer
  {:source input-stream
   :processor slow-processor
   :buffer-size 50
   :on-buffer-full (fn [] (signal-slow-down))})
```

### Manual Flow Control

```rtfs
;; Pause and resume streams
(let [stream-id (call :stream.create controllable-stream)]
  (call :stream.pause stream-id)
  ;; ... do other work ...
  (call :stream.resume stream-id))

;; Rate limiting
(call :stream.rate-limit input-stream
  {:rate 100  ; items per second
   :burst 10  ; burst allowance
   :on-item (fn [item] (process-rate-limited item))})
```

## 6. State Management in Streams

Streams can maintain state across processing.

### Stateful Processing

```rtfs
;; Accumulate state across chunks
(call :stream.stateful input-stream
  {:initial-state {}
   :processor (fn [state chunk]
                (let [new-state (update-state state chunk)]
                  [new-state (process-with-state new-state chunk)]))
   :on-result (fn [final-state] (cleanup-state final-state))})

;; Session-based processing
(call :stream.session-window input-stream
  {:session-key (fn [item] (:user-id item))
   :timeout 300000  ; 5 minutes
   :processor (fn [session-data] (analyze-user-session session-data))})
```

### Checkpointing and Recovery

```rtfs
;; Checkpoint stream state
(call :stream.with-checkpointing stream
  {:checkpoint-interval 1000
   :checkpoint-store :redis
   :on-checkpoint (fn [state] (save-state-to-persistent-store state))})

;; Resume from checkpoint
(call :stream.resume-from-checkpoint
  {:stream-id saved-stream-id
   :checkpoint-id last-good-checkpoint
   :on-resumed (fn [] (continue-processing))})
```

## 7. Stream Types and Schemas

RTFS supports typed streams for better error handling.

### Type Validation

```rtfs
;; Typed stream processing
(call :stream.validate-types input-stream
  {:schema {:type :map
            :required [:id :name]
            :properties {:id {:type :integer}
                        :name {:type :string}}}
   :on-valid (fn [valid-item] (process-typed-item valid-item))
   :on-invalid (fn [errors item] (handle-validation-error errors item))})
```

### Schema Evolution

```rtfs
;; Handle schema changes
(call :stream.schema-evolution input-stream
  {:current-schema v2-schema
   :migration-fn (fn [old-item] (migrate-v1-to-v2 old-item))
   :on-migrated (fn [new-item] (process-with-new-schema new-item))})
```

## 8. Performance Optimization

### Parallel Processing

```rtfs
;; Parallel stream processing
(call :stream.parallel input-stream
  {:parallelism 4
   :partition-fn (fn [item] (mod (:id item) 4))
   :processor (fn [partitioned-stream]
                (process-partition partitioned-stream))
   :combiner (fn [results] (combine-partition-results results))})
```

### Memory Management

```rtfs
;; Memory-efficient processing
(call :stream.memory-efficient input-stream
  {:max-memory "1GB"
   :spill-to-disk true
   :temp-dir "/tmp/stream-spill"
   :on-spill (fn [] (log-memory-pressure))})
```

### Caching and Memoization

```rtfs
;; Cache expensive computations
(call :stream.with-cache input-stream
  {:cache-key (fn [item] (:id item))
   :cache-ttl 3600000  ; 1 hour
   :compute-fn expensive-computation
   :on-hit (fn [cached] (use-cached-result cached))
   :on-miss (fn [item] (compute-and-cache item))})
```

## 9. Monitoring and Observability

### Stream Metrics

```rtfs
;; Collect stream metrics
(call :stream.with-metrics stream
  {:metrics-name "data-processing-pipeline"
   :collect {:throughput true
            :latency true
            :errors true
            :backpressure true}
   :on-metrics (fn [metrics] (send-to-monitoring-system metrics))})
```

### Debugging Streams

```rtfs
;; Debug stream processing
(call :stream.debug stream
  {:log-level :debug
   :sample-rate 0.1  ; log 10% of items
   :log-fn (fn [event] (debug-log-stream-event event))})
```

## 10. Integration Patterns

### Database Streaming

```rtfs
;; Stream from database
(call :stream.db.query "SELECT * FROM large_table"
  {:connection db-conn
   :fetch-size 1000
   :on-row (fn [row] (process-database-row row))})

;; Stream to database
(call :stream.db.bulk-insert "target_table"
  {:data-stream input-stream
   :batch-size 1000
   :on-batch-complete (fn [count] (log-progress count))})
```

### Message Queue Integration

```rtfs
;; Consume from queue
(call :stream.queue.consume "input-queue"
  {:batch-size 10
   :visibility-timeout 300
   :on-batch (fn [messages] (process-message-batch messages))
   :on-ack (fn [message-ids] (acknowledge-processed message-ids))})

;; Produce to queue
(call :stream.queue.produce "output-queue"
  {:data-stream processed-data
   :batch-size 50
   :on-sent (fn [count] (update-progress count))})
```

### External Service Integration

```rtfs
;; Stream to external API
(call :stream.http.bulk-post "https://api.example.com/batch"
  {:data-stream items-to-send
   :batch-size 100
   :retry-policy {:max-attempts 3 :backoff :exponential}
   :on-success (fn [response] (handle-success response))
   :on-error (fn [error batch] (handle-batch-error error batch))})
```

## 11. Error Handling and Recovery

### Stream Error Patterns

```rtfs
;; Dead letter queue
(call :stream.with-dead-letter stream
  {:error-predicate (fn [error] (is-recoverable? error))
   :dead-letter-queue "failed-items"
   :max-retries 3
   :on-dead-letter (fn [item error] (log-unrecoverable-error item error))})

;; Circuit breaker for streams
(call :stream.with-circuit-breaker stream
  {:failure-threshold 0.5  ; 50% failure rate
   :recovery-timeout 60000 ; 1 minute
   :on-open (fn [] (log-circuit-open))
   :on-close (fn [] (log-circuit-closed))})
```

### Graceful Shutdown

```rtfs
;; Handle shutdown signals
(call :stream.graceful-shutdown stream
  {:shutdown-timeout 30000
   :drain-strategy :finish-current-batch
   :on-shutdown-complete (fn [] (cleanup-resources))})
```

This comprehensive streaming system enables RTFS to efficiently process large-scale data while maintaining safety, composability, and performance through the host boundary architecture.