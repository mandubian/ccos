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
- **`:pause`**: Temporarily halt stream (backpressure). Host stops dequeuing until queue drains or `:resume` is returned.
- **`:resume`**: Resume a paused stream; host re-enters queue processing.
- **`:cancel`**: Terminate stream immediately; host clears the queue and marks stream cancelled.
- **`:complete`**: End stream successfully

### 4.4 Transport Configuration (Environment Overrides)

Phase 6 introduces environment-driven overrides so deployments can point the MCP streaming provider at real transports without code changes:

- `CCOS_MCP_STREAM_ENDPOINT` – highest-priority override; set to any MCP stream URL (SSE, WS, etc.)
- `CCOS_MCP_LOCAL_SSE_URL` – preferred override for local/offline development, defaults to `http://127.0.0.1:2025/sse`
- `CCOS_MCP_CLOUDFLARE_DOCS_SSE_URL` – legacy convenience variable for Cloudflare’s public docs server (still honoured for backward compatibility)
- `CCOS_MCP_STREAM_AUTH_HEADER` – full header string (for example `Authorization: Bearer <token>`)
- `CCOS_MCP_STREAM_BEARER_TOKEN` – alternative to the full header; host constructs `Authorization: Bearer …`

The provider resolves configuration in this order: explicit `server_url` argument → `CCOS_MCP_STREAM_ENDPOINT` → `CCOS_MCP_LOCAL_SSE_URL` → `CCOS_MCP_CLOUDFLARE_DOCS_SSE_URL` → baked-in default (`http://127.0.0.1:2025/sse`).

### 4.5 Transport Abstraction & Test Harness

Phase 6 also decouples transport mechanics from the provider via a new `StreamTransport` trait. The default implementation, `SseStreamTransport`, is responsible for wiring Server-Sent Events, handling follow-up payload fetches, and applying exponential backoff (`250ms`, doubling to `5s` max) across `client_config.retry_attempts`. On failures it retries automatically unless a stop signal is received.

Tests and alternate integrations can swap the transport by constructing the provider with `McpStreamingProvider::new_with_transport`. The repo ships a `MockStreamTransport` that exposes an async channel for feeding synthetic chunks; it replaces the earlier ad-hoc direct calls and makes end-to-end transport behaviour (auto-connect, backpressure, directives) observable without network dependencies.

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

### 6.3 Stream Observability & Inspection

Hosts now expose a lightweight inspection capability so operators (and RTFS code running inside a microVM) can fetch live metrics about active MCP streams. The capability is registered as `:mcp.stream.inspect` and accepts an optional map input:

```rtfs
(call :mcp.stream.inspect
  {:stream-id "mcp-weather.monitor.v1-1234"
   :include-state false
   :include-initial-state false
   :include-queue true})
```

- `:stream-id` (string, optional) — when provided, returns a single stream snapshot; omit to receive an aggregated summary for all active streams.
- `:include-state` (bool, default `true`) — include the current RTFS processor state.
- `:include-initial-state` (bool, default `true`) — include the original initial state captured at registration time.
- `:include-queue` (bool, default `true`) — include a vector of pending queued chunks with their metadata and wait time in milliseconds.

The response is a map containing structured metrics:

```rtfs
{:stream-id "mcp-weather.monitor.v1-1234"
 :processor "process-weather-stream"
 :status {:state :active}
 :queue-capacity 32
 :stats {:processed-chunks 42
   :queued-chunks 0
   :last-latency-ms 3
   :last-event-epoch-ms 1725912345678}
 :transport {:auto-connect true
       :task-present true
       :task-active true
       :timeout-ms 30000
       :retry-attempts 3
       :server-url "http://127.0.0.1:2025/sse"}
 :observed-at-epoch-ms 1725912345680
 :queue []}
```

When called without `:stream-id`, the capability returns a summary map with `:total`, `:streams` (vector of per-stream maps), and top-level provider settings (`:auto-connect`, `:server-url`, `:persistence-enabled`).

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
- Persist logical processor state alongside continuation tokens
- Provide a `resume` pathway that rehydrates state and continuation in a new provider instance
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