# Streaming Basics with RTFS and CCOS

Status: Guide
Related: ../specs/04-streaming-syntax.md, ../specs-incoming/07-effect-system.md

This guide shows how to use streaming from RTFS while keeping RTFS pure. All operational streaming is executed by CCOS via capability calls. RTFS provides types and macro forms that lower to `(call ...)`.

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

