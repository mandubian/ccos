# Streaming Basics with RTFS and CCOS

Status: Guide
Related: ../specs/04-streaming-syntax.md, ../specs/10-mcp-streaming-integration.md, ../specs-incoming/07-effect-system.md

This guide shows how to use streaming from RTFS while keeping RTFS pure. All operational streaming is executed by CCOS via capability calls. RTFS provides types and macro forms that lower to `(call ...)`.

## MCP Streaming with Continuation-Based Processing

> **Phase 4–5 Updates**
>
> Streams now persist their logical state + continuation tokens (Phase 4) and support bounded queues with
> pause/resume/cancel directives (Phase 5). Your stream processor signals these actions by returning
> `{:action :pause}` / `:resume` / `:cancel` alongside the updated state (or by sending a directive chunk such as
> `{:action :pause}`). The host records snapshots on each chunk, can `resume_stream` after a restart, and throttles
> intake when the per-stream queue reaches `queue-capacity` (default 32).

MCP streaming uses a continuation-chain pattern where RTFS processes each data chunk reactively:

```rtfs
;; Define a stream processor function
(defn weather-processor
  [state chunk metadata]
  (let [temp (:temperature chunk)
        readings (conj (:readings state) temp)]
    (cond
      ;; Continue collecting data
      (< (count readings) 10)
      {:state {:readings readings}
       :action :continue
       :output {:current_avg (avg readings)}}

      (> (count readings) 50)
      {:state {:readings readings :status :paused}
       :action :pause}

      ;; Complete collection
      :else
      {:state {:readings readings :final_avg (avg readings)}
       :action :complete
       :output {:readings readings :average (avg readings)}})))

;; Start MCP weather stream
(def weather-stream
  (mcp-stream "weather.monitoring.v1"
    {:location "Paris" :interval_seconds 60}
    weather-processor
    {:readings [] :target_samples 10}))
```

Lowered form:

```rtfs
(defn weather-processor [state chunk metadata] ...)
(def weather-stream
  (call :mcp.stream.start
    {:endpoint "weather.monitoring.v1"
     :config {:location "Paris" :interval_seconds 60}
     :processor 'weather-processor
     :initial-state {:readings [] :target_samples 10}}))
```

### New Lightweight Macro Form (Prototype)

For early experimentation a simplified surface form is available and documented in the code as a prototype:

```rtfs
;; Prototype macro (mcp-stream <endpoint> <processor-fn> <initial-state?>)
(mcp-stream "weather.monitor.v1" process-weather-chunk {:count 0})
```

This is lowered at parse/AST normalization time into the canonical call form:

```rtfs
(call :mcp.stream.start {:endpoint "weather.monitor.v1"
                         :processor "process-weather-chunk"
                         :initial-state {:count 0}})
```

Notes:
* The macro currently only supports the tuple `(endpoint processor initial-state?)`—a richer `:config` map will be added later (tracked in future spec update).
* Processor is stored by name; continuation re‑entry is still a placeholder in `McpStreamingProvider::process_chunk`.
* The example file `examples/stream_mcp_example.rtfs` shows a processor skeleton.
* Integration test: `tests/ccos-integration/mcp_streaming_mock_tests.rs` validates macro lowering and mock chunk loop.

Limitations (current prototype):
1. No backpressure directives yet (all chunks are just printed).
2. No persistence of updated state across chunks (will wire through continuation serialization next phase).
3. No real MCP transport—`mock_loop.rs` feeds synthetic chunks.
4. Error handling is minimal; missing keys return simple runtime errors.

Planned enhancements (roadmap):
* Add `:action` directives (:continue :pause :resume :cancel :complete) to processor return map.
* Serialize and store continuation/state per stream id for resumability.
* Introduce backpressure signaling to the host event loop.
* Add optional `:config` map parameter to macro before the initial state argument.
* Real MCP client (WebSocket / HTTP SSE) integration replacing mock loop.


## Source and Channel-based Consumption

```rtfs
;; Create a source (macro → :marketplace.stream.start)
(def source-handle
  (stream-source "com.example:v1.0:data-feed" {:config {:buffer-size 500}}))

;; Channel-based (macro → bounded pull loop using :marketplace.stream.next)
(stream-consume source-handle
  {item =>
    (do
      (log-step :info "Received item: " item)
      (process-data item))})
```

Equivalent lowered form:

```rtfs
(def h (call :marketplace.stream.start {:id "com.example:v1.0:data-feed" :config {:buffer-size 500}}))
(loop []
  (when-let [x (call :marketplace.stream.next {:handle h :timeout_ms 1000})]
    (process-data x)
    (recur)))
```

## Callback-based Consumption

```rtfs
;; Macro → :marketplace.stream.register-callbacks
(stream-consume source-handle
  {:on-item process-item
   :on-error handle-error
   :on-complete cleanup})
```

Lowered form:

```rtfs
(call :marketplace.stream.register-callbacks
  {:handle source-handle
   :on-item   {:fn 'process-item}
   :on-error  {:fn 'handle-error}
   :on-complete {:fn 'cleanup}})
```

## Sink and Produce

```rtfs
(def sink-handle (stream-sink "com.example:v1.0:data-processor"))
(stream-produce sink-handle {:event-id "evt-123" :data "sample"})
```

Lowered form:

```rtfs
(def sink-handle (call :marketplace.stream.open-sink {:id "com.example:v1.0:data-processor"}))
(call :marketplace.stream.send {:handle sink-handle :item {:event-id "evt-123" :data "sample"}})
```

## Types and Effects

- Stream schemas are RTFS types (see spec). Use them to annotate producers/consumers.
- Streaming implies effects (e.g., `:network`, `:ipc`); declare and validate via the effect system.

## Notes

- With a PureHost, these calls no-op/fail; with a CCOS host, they route to providers with policy and audit.
- For advanced patterns (pipelines, multiplex/demux), use macros that lower to the appropriate marketplace capability IDs.

