# RTFS 2.0 Streaming Syntax Specification

**Date:** December 8, 2024  
**Version:** 0.1.0-draft  
**Status:** Draft

## Overview

This document defines the RTFS 2.0 syntax for expressing streaming capabilities in homoiconic plans that can be executed by the CCOS capability marketplace runtime.

## Design Principles

1. **Homoiconic**: Stream operations are expressed as S-expressions that can be manipulated as data
2. **Type-Safe**: Leverage RTFS 2.0 type system for stream schemas
3. **Versioned**: Use versioned namespacing for streaming capabilities
4. **Unified**: Support all 5 streaming patterns with consistent syntax
5. **Resource-Aware**: Integrate with RTFS 2.0 resource reference system

## Stream Type Definitions

### Basic Stream Types
```rtfs
;; Stream type with element schema
[:stream [:map [:event-id :string] [:data :any]]]

;; Bidirectional stream (input and output schemas)
[:stream-bi 
  :input [:map [:command :string] [:params :any]]
  :output [:map [:result :any] [:status :keyword]]]

;; Duplex stream (separate input/output types)
[:stream-duplex
  :input [:map [:request :string]]
  :output [:map [:response :string]]]
```

### Streaming Capability Registration

```rtfs
;; Register a stream source capability
(register-stream-capability
  :capability-id "com.example:v1.0:data-feed"
  :type :stream-source
  :output-schema [:stream [:map [:timestamp :string] [:value :float]]]
  :config {:buffer-size 1000 :enable-callbacks true}
  :metadata {:description "Live data feed"})

;; Register a stream sink capability  
(register-stream-capability
  :capability-id "com.example:v1.0:data-processor"
  :type :stream-sink
  :input-schema [:stream [:map [:data :any]]]
  :config {:enable-callbacks false}
  :metadata {:description "Data processor"})

;; Register bidirectional stream capability
(register-stream-capability
  :capability-id "com.example:v1.0:chat-bot"
  :type :stream-bidirectional
  :input-schema [:stream [:map [:message :string]]]
  :output-schema [:stream [:map [:response :string] [:confidence :float]]]
  :config {:enable-callbacks true}
  :metadata {:description "Interactive chat bot"})
```

## Stream Operations in RTFS Plans

### 1. Stream Source Operations

```rtfs
;; Create a stream source
(def source-handle 
  (stream-source "com.example:v1.0:data-feed" 
    {:config {:buffer-size 500}}))

;; Consume from stream source with channel-based approach
(stream-consume source-handle 
  {item-binding => 
    (do
      (log-step :info "Received item: " item-binding)
      (process-data item-binding))})

;; Consume with callback-based approach
(stream-consume source-handle
  {:enable-callbacks true
   :on-item (fn [item] (process-item item))
   :on-error (fn [err] (log-error err))
   :on-complete (fn [] (log-info "Stream completed"))})
```

### 2. Stream Sink Operations

```rtfs
;; Create a stream sink
(def sink-handle 
  (stream-sink "com.example:v1.0:data-processor"))

;; Send items to sink
(stream-produce sink-handle 
  {:event-id "evt-123" :data "sample data"})

;; Batch produce with callback confirmation
(stream-produce sink-handle 
  [{:event-id "evt-1" :data "data1"}
   {:event-id "evt-2" :data "data2"}]
  {:enable-callbacks true
   :on-success (fn [item] (log-info "Sent: " item))
   :on-error (fn [item err] (log-error "Failed: " item err))})
```

### 3. Stream Transform Operations

```rtfs
;; Create a transform stream
(def transform-handle
  (stream-transform
    :input-stream (resource:ref "upstream-source")
    :output-stream (resource:ref "downstream-sink")
    :transform-fn (fn [item] 
      (assoc item :processed-at (timestamp:now)))
    :config {:buffer-size 100}))

;; Transform with callback monitoring
(stream-transform
  :input-stream input-handle
  :output-stream output-handle
  :transform-fn transform-fn
  :config {:enable-callbacks true
           :on-transform-success (fn [in out] (metrics:increment "transforms"))
           :on-transform-error (fn [in err] (metrics:increment "errors"))})
```

### 4. Bidirectional Stream Operations

```rtfs
;; Create bidirectional stream
(def bi-stream 
  (stream-bidirectional "com.example:v1.0:chat-bot"))

;; Send and receive with channel-based approach
(stream-interact bi-stream
  :send {:message "Hello, how are you?"}
  :receive {response =>
    (do
      (log-step :info "Bot response: " response)
      (display-response response))})

;; Interactive session with callbacks
(stream-interact bi-stream
  :config {:enable-callbacks true
           :on-send (fn [msg] (log-info "Sent: " msg))
           :on-receive (fn [resp] (handle-response resp))
           :on-error (fn [err] (reconnect-if-needed err))})
```

### 5. Duplex Stream Operations

```rtfs
;; Create duplex stream with separate input/output
(def duplex-handle
  (stream-duplex
    :input-capability "com.example:v1.0:command-input"
    :output-capability "com.example:v1.0:event-output"
    :config {:enable-callbacks true}))

;; Send commands and handle events separately
(parallel
  [input-task (stream-produce duplex-handle 
                {:command "start-monitoring" :params {}})]
  [output-task (stream-consume duplex-handle
                 {event =>
                   (match (:type event)
                     :alert (handle-alert event)
                     :status (update-status event)
                     :error (handle-error event))})])
```

## Advanced Streaming Features

### Stream Composition and Pipelines

```rtfs
;; Create a streaming pipeline
(def pipeline
  (stream-pipeline
    [(stream-source "com.example:v1.0:raw-data")
     (stream-transform :fn data-cleaner)
     (stream-transform :fn data-enricher)
     (stream-sink "com.example:v1.0:processed-data")]))

;; Execute pipeline with monitoring
(stream-execute pipeline
  {:config {:enable-callbacks true
            :on-pipeline-start (fn [] (log-info "Pipeline started"))
            :on-pipeline-complete (fn [] (log-info "Pipeline completed"))
            :on-stage-error (fn [stage err] (handle-stage-error stage err))}})
```

### Resource Management with Streams

```rtfs
;; Automatic resource cleanup
(with-resource [stream-handle StreamHandle 
                (stream-source "com.example:v1.0:data-feed")]
  (stream-consume stream-handle
    {item => (process-item item)}))
;; Stream automatically closed when exiting scope

;; Manual resource management
(def stream-handle (stream-source "com.example:v1.0:data-feed"))
(try
  (stream-consume stream-handle {item => (process-item item)})
  (catch :stream-error err
    (log-error "Stream error: " err))
  (finally
    (stream-close stream-handle)))
```

### Stream Multiplexing and Demultiplexing

```rtfs
;; Multiplex multiple streams
(def multiplexed-stream
  (stream-multiplex
    [(stream-source "com.example:v1.0:sensor-1")
     (stream-source "com.example:v1.0:sensor-2")
     (stream-source "com.example:v1.0:sensor-3")]
    {:strategy :round-robin}))

;; Demultiplex by criteria
(stream-demultiplex multiplexed-stream
  {:criteria (fn [item] (:sensor-type item))
   :outputs {"temp" (stream-sink "com.example:v1.0:temp-processor")
             "pressure" (stream-sink "com.example:v1.0:pressure-processor")
             "humidity" (stream-sink "com.example:v1.0:humidity-processor")}})
```

## Callback System Integration

### Event-Driven Stream Processing

```rtfs
;; Register stream callbacks with full event coverage
(stream-callbacks
  :stream-handle handle
  :events {:on-item (fn [item] (process-item item))
           :on-error (fn [err] (handle-error err))
           :on-complete (fn [] (cleanup-resources))
           :on-start (fn [] (initialize-processing))
           :on-pause (fn [] (pause-processing))
           :on-resume (fn [] (resume-processing))
           :on-cancel (fn [] (cancel-processing))
           :on-backpressure (fn [] (slow-down-processing))})

;; Conditional callback registration
(when (config:get :enable-monitoring)
  (stream-callbacks
    :stream-handle handle
    :events {:on-item (fn [item] (metrics:record-item item))
             :on-error (fn [err] (metrics:record-error err))}))
```

### Stream Metrics and Monitoring

```rtfs
;; Built-in metrics collection
(def monitored-stream
  (stream-source "com.example:v1.0:data-feed"
    {:config {:enable-callbacks true
              :collect-metrics true}}))

;; Custom metrics with callbacks
(stream-callbacks
  :stream-handle monitored-stream
  :events {:on-item (fn [item] 
             (metrics:increment "items-processed")
             (metrics:record-latency (- (timestamp:now) (:timestamp item))))
           :on-error (fn [err]
             (metrics:increment "errors")
             (alert:send "Stream error: " err))})
```

## Type System Integration

### Stream Schema Validation

```rtfs
;; Strict schema validation
(def validated-stream
  (stream-source "com.example:v1.0:typed-data"
    {:schema [:stream [:map 
                [:id [:and :string [:min-length 1]]]
                [:value [:and :number [:>= 0]]]
                [:timestamp :timestamp]]]
     :strict-validation true}))

;; Schema evolution support
(stream-transform
  :input-stream legacy-stream
  :output-stream new-stream
  :transform-fn (fn [item]
    (-> item
        (assoc :version "2.0")
        (update :timestamp timestamp:parse)))
  :input-schema [:stream [:map [:id :string] [:value :number] [:timestamp :string]]]
  :output-schema [:stream [:map [:id :string] [:value :number] [:timestamp :timestamp] [:version :string]]])
```

### Generic Stream Types

```rtfs
;; Generic stream type with constraints
(def generic-processor
  (stream-transform
    :input-stream (resource:ref "input")
    :output-stream (resource:ref "output")
    :transform-fn (fn [item] (process-generic item))
    :type-constraints {:input [:stream :any]
                       :output [:stream :any]
                       :element-constraint (fn [in out] (= (:type in) (:type out)))}))
```

## Error Handling and Resilience

### Stream-Specific Error Handling

```rtfs
;; Comprehensive error handling
(stream-consume source-handle
  {item => (process-item item)}
  {:error-handling {:on-parse-error :skip
                    :on-schema-error :retry
                    :on-processing-error :dead-letter
                    :retry-attempts 3
                    :retry-delay-ms 1000}})

;; Circuit breaker pattern
(def circuit-breaker
  (stream-circuit-breaker
    :failure-threshold 5
    :timeout-ms 30000
    :on-open (fn [] (log-warn "Circuit breaker opened"))
    :on-half-open (fn [] (log-info "Circuit breaker half-open"))
    :on-close (fn [] (log-info "Circuit breaker closed"))))

(stream-consume source-handle
  {item => (circuit-breaker:call (fn [] (process-item item)))}
  {:enable-callbacks true})
```

### Backpressure Management

```rtfs
;; Automatic backpressure handling
(stream-consume source-handle
  {item => (slow-processing item)}
  {:backpressure {:strategy :drop-oldest
                  :buffer-size 1000
                  :on-backpressure (fn [] (log-warn "Backpressure detected"))}})

;; Manual backpressure control
(stream-consume source-handle
  {item => 
    (if (< (queue:size processing-queue) 100)
      (queue:put processing-queue item)
      (do
        (log-warn "Queue full, dropping item")
        (metrics:increment "dropped-items")))}
  {:enable-callbacks true})
```

## Integration with CCOS Runtime

### Capability Marketplace Integration

```rtfs
;; Register streaming capability with marketplace
(marketplace:register-capability
  :capability-id "com.example:v1.0:stream-processor"
  :type :streaming
  :stream-patterns [:source :sink :transform :bidirectional :duplex]
  :implementation stream-processor-impl
  :config {:enable-callbacks true
           :buffer-size 1000
           :max-connections 100})

;; Discover streaming capabilities
(def available-streams
  (marketplace:discover-capabilities
    :type :streaming
    :filters {:pattern :source
              :schema-compatible [:stream [:map [:data :any]]]}))
```

### Plan Generation and Execution

```rtfs
;; Generate streaming plan
(def streaming-plan
  (plan
    :type :rtfs.core:v2.0:streaming-plan
    :plan-id "streaming-plan-123"
    :resources [(resource:ref "input-stream")
                (resource:ref "output-stream")]
    :program (do
      (def input (stream-source (resource:ref "input-stream")))
      (def output (stream-sink (resource:ref "output-stream")))
      (stream-transform
        :input-stream input
        :output-stream output
        :transform-fn data-processor
        :config {:enable-callbacks true}))))

;; Execute streaming plan
(marketplace:execute-plan streaming-plan
  {:execution-mode :streaming
   :monitoring {:collect-metrics true
                :callback-events true}})
```

## Performance Considerations

### Optimization Hints

```rtfs
;; Performance-optimized stream configuration
(stream-source "com.example:v1.0:high-throughput"
  {:config {:buffer-size 10000
            :enable-callbacks false  ; Disable for maximum throughput
            :batch-size 100
            :compression :zstd
            :serialization :bincode}})

;; Memory-efficient streaming
(stream-transform
  :input-stream input
  :output-stream output
  :transform-fn lazy-transform
  :config {:lazy-evaluation true
           :memory-limit "100MB"
           :gc-trigger 0.8})
```

### Monitoring and Debugging

```rtfs
;; Debug mode with detailed logging
(when (config:get :debug-streams)
  (stream-callbacks
    :stream-handle handle
    :events {:on-item (fn [item] (log-debug "Item: " item))
             :on-error (fn [err] (log-debug "Error: " err))
             :on-start (fn [] (log-debug "Stream started"))
             :on-complete (fn [] (log-debug "Stream completed"))}))

;; Performance profiling
(stream-profile handle
  {:metrics [:throughput :latency :memory-usage :cpu-usage]
   :sampling-rate 0.1
   :output-format :json})
```

## Conclusion

This RTFS 2.0 streaming syntax specification provides a comprehensive, homoiconic approach to expressing streaming operations that can be executed by the CCOS capability marketplace. The syntax supports all streaming patterns, integrates with the type system, provides robust error handling, and maintains the data-as-code philosophy of RTFS.

The design enables AI agents to generate, manipulate, and execute streaming plans as first-class RTFS data structures, supporting the full lifecycle of streaming applications in the CCOS environment.
