# Streaming Roadmap (Prototype → Mature Capability)

Status: Tracking / Roadmap
Related Specs: `docs/rtfs-2.0/specs/10-mcp-streaming-integration.md`, `docs/rtfs-2.0/guides/streaming-basics.md`

## Current Prototype State
- Macro: `(mcp-stream <endpoint> <processor> <initial-state?>)` → lowered to `(call :mcp.stream.start {...})`.
- Provider: `McpStreamingProvider` registers processor metadata (name + placeholder continuation + initial state).
- Mock event loop: `run_mock_stream_loop` feeds synthetic chunks.
- Tests: `mcp_streaming_mock_tests.rs` validate macro lowering + mock processing path.
- No real continuation re-entry or state mutation yet.

## Phase 1 – Minimal Stateful Processing (Completed)
Goal: Each chunk updates stored state; tests assert evolution. (Merged)
Tasks:
1. Store per-stream `current_state: Value` inside `StreamProcessorRegistration`.
2. Enhance `process_chunk` to:
   - Fetch mutable registration
   - Derive new state (temporary placeholder: increment counter if `:count` present)
   - Persist updated state
3. Expose a lightweight getter (for tests) to read current state.
4. Extend integration test to assert `:count` increments across 5 chunks.
5. Return early error if stream not found.

## Phase 2 – Directive Handling Scaffold (Completed)
Goal: Processors can signal basic flow control.
Implemented:
1. `:action` directives recognized: `:complete`, `:stop`; unknown → Error status.
2. Added `StreamStatus` enum { Active, Completed, Stopped, Error(String) }.
3. Post-terminal chunks ignored (state frozen) with tests for completion, stop, and unknown directive error.
4. Added tests: completion, stop, unknown directive sets error.

## Phase 3 – Real Processor Invocation (Completed)
Goal: Actually invoke RTFS function instead of placeholder mutation. (Merged)
Implemented:
1. Added optional `processor_invoker` hook to `McpStreamingProvider` to invoke RTFS function with args `[state chunk metadata]`.
2. Return shape interpretation: `{:state <v> :action <kw>? :output <v>?}`; unrecognized plain map treated as new state (backward compatible).
3. Updated state persistence on each invocation; output currently ignored (future event/log pipeline).
4. Invalid return (non-map) yields descriptive `RuntimeError` and sets stream status `Error`.
5. Added tests `mcp_streaming_phase3_tests.rs` exercising successful invocation and invalid return shape error.
6. Backward compatibility maintained: legacy increment path used if no invoker or processor missing.
7. Clear error surfaced if processor function symbol not found by invoker.

## Phase 4 – Continuation & Persistence (Completed)
Goal: Resumable streams across host cycles. (Merged)
Implemented:
1. Introduced `StreamPersistence` trait with default in-memory implementation; `McpStreamingProvider` persists snapshots (state + continuation token + status) on each chunk.
2. Added `resume_stream(stream_id)` API that rehydrates provider state from persisted snapshots; provider constructor accepts optional persistence backend.
3. Tests (`mcp_streaming_phase4_tests.rs`) cover persistence + restart + resume flow and error path for missing snapshot.

## Phase 5 – Backpressure & Flow Control (Completed)
Goal: Regulate chunk ingestion. (Merged)
Implemented:
1. Added bounded per-stream queue (`queue-capacity` configurable via start params, default 32) with metrics (`StreamStats`).
2. Recognized processor directives `:pause`, `:resume`, `:cancel` alongside existing `:complete`/`:stop`; host enforces status transitions and queue draining.
3. Queue overflow automatically pauses intake until resumed; resuming drains queued items with latency tracking.
4. Tests (`mcp_streaming_phase5_tests.rs`) cover overflow pause/resume cycle and cancel semantics.

## Phase 6 – Real MCP Transport (In Progress)
Goal: Replace mock loop with actual MCP streaming (WS/SSE or tool polling).
Status: MVP local SSE transport in-progress (using bundled `mcp-local-server`). Legacy Cloudflare endpoint kept as fallback only.
Tasks:
1. Implement transport trait abstraction with mock + SSE client (WebSocket planned next).
2. Parse MCP incremental messages → `Value` chunk maps.
3. Pluggable retry + exponential backoff (configurable).
4. Test using embedded mock server harness.
5. Wire environment overrides (`CCOS_MCP_STREAM_ENDPOINT`, `CCOS_MCP_LOCAL_SSE_URL`, `CCOS_MCP_CLOUDFLARE_DOCS_SSE_URL`, `CCOS_MCP_STREAM_AUTH_HEADER`, `CCOS_MCP_STREAM_BEARER_TOKEN`).

## Phase 7 – Observability & Tooling
Goal: Introspection and developer experience.
Tasks:
1. Add debug capability: `:mcp.stream.inspect` → returns state & stats.
2. Structured logging (trace chunk seq, action decisions, errors).
3. Optional metrics sink (prometheus-style struct).
4. Add guide section for debugging streams.

## Phase 8 – Type & Effect Integration
Goal: Typed streaming pipeline.
Tasks:
1. Allow optional input/output schemas per stream registration.
2. Validate processor return shape matches schema.
3. Associate effect labels (e.g., `:network`) and enforce via security context.
4. Tests for schema mismatch + effect denial.

## Definitions / Data Model (Incremental)
```rust
// Roadmap target shape (future)
pub struct StreamProcessorRegistration {
    pub processor_fn: String,
    pub continuation: Vec<u8>,
    pub current_state: Value,      // Phase 1
    pub status: StreamStatus,      // Phase 2
    pub stats: StreamStats,        // Phase 5/7
    pub schema: Option<StreamSchema>, // Phase 8
}
```

## Open Questions
- Should processor invocation be isolated (sandboxed evaluator) per stream for safety?
- Do we unify directive semantics across all streaming providers (marketplace vs MCP)?
- Where to persist state (in-memory only vs pluggable store)?
- How to surface partial failures (chunk-level vs stream-level)?

## Acceptance for Phase 1 Completion
- State mutates across mock chunks.
- Test asserts final count == number of chunks.
- No panics; descriptive error if stream id missing.

---
Tracking Owner: streaming feature lead
Update Frequency: After each phase merge
