# RTFS 2.0 Spec: MCP Streaming Integration

- **Status**: `Draft`
- **Version**: `0.1.0`
- **Last Updated**: `2025-09-23`

## 1. Abstract

This document specifies how RTFS integrates with MCP (Model Context Protocol) streaming endpoints. It extends the continuation-based execution model to handle incremental, asynchronous data streams while maintaining RTFS's purity and determinism.

## 2. The Challenge: Streaming in a Pure Functional Language

MCP streaming endpoints present unique challenges for RTFS:

- **Incremental Data**: Streams deliver data in chunks over time, requiring RTFS to process data reactively
- **Asynchronous Nature**: Streams are inherently async, but RTFS execution is synchronous and continuation-based
- **Lifecycle Complexity**: Streams have start/pause/resume/cancel states that must be managed
- **Backpressure**: RTFS processing speed may not match stream production rate
- **Error Propagation**: Stream errors must be handled deterministically

## 3. Core Design: Stream Processing as Host-Mediated Effects

RTFS handles streaming through **stream processors** - pure functions that transform stream data. The Host manages all streaming infrastructure, yielding control to RTFS for each data chunk.

### 3.1 Stream Processor Model

A stream processor is a pure RTFS function that receives stream data and returns processing directives:

```rtfs
(defn process-weather-stream
  [stream-state chunk]
  :[ :map { :state :any :action :keyword :output :any } ]

  (let [new-state (update-state stream-state chunk)]
    (cond
      ;; Continue processing
      (should-continue? new-state)
      {:state new-state :action :continue :output (extract-data chunk)}

      ;; Pause stream (backpressure)
      (should-pause? new-state)
      {:state new-state :action :pause :output nil}

      ;; Complete processing
      (is-complete? new-state)
      {:state new-state :action :complete :output (final-result new-state)})))
```

### 3.2 Stream Lifecycle via Continuation Chain

MCP streaming uses a **continuation chain** where each data chunk triggers RTFS re-entry:

1. **Initiate Stream**: RTFS calls `(call :mcp.stream.start {...})`
2. **Host Setup**: Host establishes MCP connection and registers stream processor
3. **Chunk Processing**: For each data chunk, Host resumes RTFS with processor function
4. **Dynamic Control**: Processor can pause/resume/cancel stream via return actions
5. **Completion**: Stream ends when processor returns `:complete` or on error

## 4. MCP Streaming Syntax

### 4.1 Basic Stream Consumption

```rtfs
;; Start MCP weather stream
(def stream-handle
  (call :mcp.stream.start
    {:endpoint "weather.stream.v1"
     :config {:city "Paris" :interval_seconds 300}
     :processor process-weather-stream
     :initial-state {:readings [] :last-temp nil}}))

;; Stream runs asynchronously, processor called for each chunk
;; RTFS execution continues immediately after start call
```

### 4.2 Processor Function Signature

```rtfs
(defn my-stream-processor
  [state chunk metadata]
  :[ :map { :state :any
            :action :[ :enum :continue :pause :resume :cancel :complete ]
            :output :any
            :config :[ :map { :buffer_size :int :timeout_ms :int } ] } ]

  ;; Pure function - no side effects, deterministic output
  {:state (update-state state chunk)
   :action :continue
   :output (transform-data chunk)
   :config {:buffer_size 100}})
```

### 4.3 Stream Control Actions

- **`:continue`**: Process chunk and continue streaming
- **`:pause`**: Temporarily halt stream (backpressure)
- **`:resume`**: Resume paused stream
- **`:cancel`**: Terminate stream immediately
- **`:complete`**: End stream successfully

## 5. Continuation-Based Execution Flow

### 5.1 Stream Initiation

```rtfs
;; RTFS code initiates stream
(def handle
  (call :mcp.stream.start
    {:endpoint "weather.v1.stream"
     :processor 'process-weather-chunks
     :initial-state {:count 0}}))

;; Execution yields here, Host gets: HostCall{request: {...}, continuation: <opaque>}
```

### 5.2 Host Stream Management

```
Host receives HostCall
├── Parses request: {endpoint, processor, initial_state}
├── Establishes MCP connection
├── Registers stream processor callback
├── Returns stream_handle to RTFS
└── Begins async stream consumption
```

### 5.3 Chunk Processing Loop

```
For each MCP data chunk:
├── Host captures current continuation (if needed)
├── Host calls: engine.resume(continuation, {chunk, metadata})
├── RTFS executes: (processor state chunk metadata)
├── Processor returns: {state, action, output, config}
├── Host applies action (pause/resume/cancel/complete)
├── If continuing: Host stores new continuation for next chunk
└── RTFS execution completes, yielding control back to Host
```

## 6. Advanced Patterns

### 6.1 Stateful Stream Processing

```rtfs
(defn aggregate-readings
  [state chunk metadata]
  (let [new-readings (conj (:readings state) (:temperature chunk))
        avg-temp (calculate-average new-readings)]

    (cond
      ;; Continue collecting until we have enough data
      (< (count new-readings) 10)
      {:state {:readings new-readings}
       :action :continue
       :output {:current_avg avg-temp}}

      ;; Complete and return final aggregate
      :else
      {:state {:readings new-readings :final_avg avg-temp}
       :action :complete
       :output {:readings new-readings :average avg-temp}})))
```

### 6.2 Backpressure Handling

```rtfs
(defn backpressure-processor
  [state chunk metadata]
  (let [queue-size (count (:queue state))]

    (cond
      ;; Queue getting full - pause stream
      (> queue-size 1000)
      {:state (update state :queue conj chunk)
       :action :pause
       :output {:status "buffering" :queue_size queue-size}}

      ;; Normal processing
      (< queue-size 100)
      {:state (process-and-dequeue state chunk)
       :action :continue
       :output (process-chunk chunk)}

      ;; Resume when queue drained
      (and (= (:status state) :paused)
           (< queue-size 100))
      {:state (assoc state :status :active)
       :action :resume
       :output {:status "resuming"}})))
```

### 6.3 Error Handling and Recovery

```rtfs
(defn resilient-processor
  [state chunk metadata]
  (try
    (let [result (process-chunk chunk)]
      {:state (update-metrics state :success)
       :action :continue
       :output result})

    (catch Exception e
      (let [error-count (inc (:error_count state))]
        (cond
          ;; Retry on transient errors
          (and (transient-error? e) (< error-count 3))
          {:state (update state :error_count error-count)
           :action :continue  ;; Stream continues, processor handles retry
           :output {:error "transient" :retry_count error-count}}

          ;; Fail stream on persistent errors
          :else
          {:state (assoc state :final_error e)
           :action :cancel
           :output {:error "persistent" :error_count error-count}})))))
```

## 7. Host Implementation Requirements

### 7.1 Stream Manager Interface

The Host must provide a stream manager that:

- Maintains stream handles and processor registrations
- Manages MCP connection lifecycle
- Implements continuation storage and resumption
- Handles backpressure and flow control
- Provides timeout and cancellation semantics

### 7.2 Continuation Storage

For long-running streams, the Host must:

- Serialize continuations for persistence across restarts
- Implement continuation timeout policies
- Provide stream status queries
- Support stream multiplexing (multiple streams per RTFS program)

## 8. Benefits of This Design

### 8.1 Purity Preserved
- RTFS functions remain pure and deterministic
- All streaming effects mediated through Host boundary
- No async/await complexity in RTFS core

### 8.2 Flexibility
- Processors can implement any streaming logic (filtering, aggregation, transformation)
- Dynamic control over stream lifecycle
- Backpressure handling without blocking

### 8.3 Reliability
- Continuation-based resumption handles failures gracefully
- Host can persist stream state across restarts
- Clear error boundaries and recovery patterns

### 8.4 Performance
- Minimal RTFS re-entry overhead for high-frequency streams
- Host can optimize stream processing (batching, parallelization)
- Efficient backpressure without busy-waiting

## 9. Example: Complete MCP Weather Stream

```rtfs
;; Define stream processor
(defn weather-aggregator
  [state chunk metadata]
  (let [readings (conj (:readings state) chunk)
        stats (calculate-stats readings)]

    (cond
      ;; Continue collecting data
      (< (count readings) (:target_samples state))
      {:state (assoc state :readings readings :stats stats)
       :action :continue
       :output {:sample_count (count readings) :current_stats stats}}

      ;; Complete collection
      :else
      {:state (assoc state :readings readings :final_stats stats :completed_at (current-time))
       :action :complete
       :output {:readings readings :statistics stats :duration (:duration metadata)}})))

;; Start weather monitoring stream
(def weather-stream
  (call :mcp.stream.start
    {:endpoint "weather.monitoring.v1"
     :config {:location "Paris" :sampling_interval 60}
     :processor weather-aggregator
     :initial-state {:readings [] :target_samples 100}}))

;; RTFS execution continues immediately
;; Stream processing happens asynchronously via continuation chain
;; Results can be queried later via stream status calls
```

This design enables RTFS to effectively handle MCP streaming endpoints while maintaining its core principles of purity, determinism, and clear host boundaries.</content>
<parameter name="filePath">/home/mandubian/workspaces/mandubian/ccos/docs/rtfs-2.0/specs/10-mcp-streaming-integration.md