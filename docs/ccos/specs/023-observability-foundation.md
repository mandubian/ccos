````markdown
# CCOS Specification 023: Observability Foundation (Causal Chain → Working Memory)

**Status:** Proposed
**Version:** 1.0
**Date:** 2025-08-26
**Related:**
- [SEP-003: Causal Chain](./003-causal-chain.md)
- [SEP-013: Working Memory](./013-working-memory.md)
- [SEP-024: Observability Ingestor Capability](./024-observability-ingestor-capability.md)

This document outlines the minimal, production‑ready observability path that turns audited Causal Chain actions into compact, queryable Working Memory (WM) entries.

## Components

- Causal Chain (SEP‑003): immutable action ledger with append notifications.
- CausalChainEventSink: observer interface notified on each append.
- WorkingMemorySink: adapter that maps `Action` → `ActionRecord` → `WorkingMemoryEntry`.
- Working Memory: light recall store with budgets and tags.

## Data Flow

1. An action is appended to the Causal Chain (e.g., capability call, plan lifecycle, intent audit).
2. Registered sinks receive `on_action_appended(&Action)` callbacks.
3. `WorkingMemorySink` distills the action into an entry with tags:
   - `causal-chain`, `distillation`, `wisdom`, and lowercased action kind.
   - Minimal content string: type/plan/intent/timestamp plus function name, arg count, cost, duration.
   - Meta includes `action_id`, `plan_id`, `intent_id`, and `signature` if present.
4. The entry is appended to WM idempotently using `action_id + content_hash`.

## Enablement

- Default: enabled in `CCOSConfig`.
- Env toggles (override config):
  - `CCOS_ENABLE_WM_INGESTOR=1|true`
  - `CCOS_WM_MAX_ENTRIES=<usize>`
  - `CCOS_WM_MAX_TOKENS=<usize>`

## APIs

- Registering sink: done in `CCOSEnvironment::new` when enabled.
- Access WM:
  - `CCOSEnvironment::working_memory() -> Option<Arc<Mutex<WorkingMemory>>>`
- Rebuild from history:
  - `CCOSEnvironment::rebuild_working_memory_from_chain()` — snapshots chain and replays using identical derivation.

### Capability: observability.ingestor:v1.ingest

On-demand Working Memory ingestion and replay exposed as a local capability and registered at environment bootstrap.

See [SEP-024: Observability Ingestor Capability](./024-observability-ingestor-capability.md) for the full contract and examples.

## Tests

- Unit tests in `working_memory/ingestor.rs` exercise hashing, derivation, and idempotency.
- A sink integration test registers the sink on a fresh chain and asserts WM entries are produced.

## Metrics and Structured Logs

- The Causal Chain tracks lightweight performance metrics per capability and per function.
  - Counters: total_calls, total_duration_ms, average_duration_ms, total_cost.
- The Causal Chain emits structured JSON log lines for key events:
  - `action_appended`, `action_result_recorded`, `plan_event`, `delegation_event`.
- A small in-memory log buffer keeps the most recent entries (capacity via `CCOS_LOG_BUFFER_CAPACITY`, default 256).

### Verification (Tests/Dev)

  - `host.get_capability_metrics("observability.ingestor:v1.ingest") -> CapabilityMetrics`
  - `host.get_function_metrics("observability.ingestor:v1.ingest") -> FunctionMetrics`
  - `host.get_recent_logs(32) -> Vec<String>` (structured JSON lines)

Example test assertions:

  
Quickstart guide: See docs/ccos/guides/observability-quickstart.md for a minimal end-to-end usage sample from Rust.

### Test-only Prometheus endpoint (feature-gated)

- A minimal Prometheus-like exporter is available behind the `metrics_exporter` feature.
- It renders counters and gauges from in-memory Causal Chain metrics and serves them via a tiny HTTP listener.
- Intended for tests/dev only; off by default and has no external dependencies.
- Enable with the Cargo feature and use `runtime::metrics_exporter::{render_prometheus_text, start_metrics_server}`.

Metrics exposed:
- `ccos_total_cost` (gauge)
- `ccos_capability_calls_total{id="<capability>"}` (counter)
- `ccos_capability_avg_duration_ms{id="<capability>"}` (gauge)
- `ccos_function_calls_total{name="<function>"}` (counter)
- `ccos_function_avg_duration_ms{name="<function>"}` (gauge)
- Duration histograms (ms) with cumulative buckets, sum, and count:
  - Capability: base `ccos_capability_duration_ms` with series:
    - `ccos_capability_duration_ms_bucket{id="<capability>",le="<bound>|+Inf"}`
    - `ccos_capability_duration_ms_sum{id="<capability>"}`
    - `ccos_capability_duration_ms_count{id="<capability>"}`
  - Function: base `ccos_function_duration_ms` with series:
    - `ccos_function_duration_ms_bucket{name="<function>",le="<bound>|+Inf"}`
    - `ccos_function_duration_ms_sum{name="<function>"}`
    - `ccos_function_duration_ms_count{name="<function>"}`

Buckets used: 5, 10, 25, 50, 100, 250, 500, 1000, 2500, 5000, 10000 (milliseconds) plus `+Inf`.

#### Try it locally

- Build and run the example (single-request server):
  - cargo run --example serve_metrics --features metrics_exporter
- Then curl http://127.0.0.1:9898/metrics once (the server exits after one request).

## Notes & Future Work

- The content string is intentionally compact and stable; switch to structured serialization when schema stabilizes.
- Consider a Prometheus exporter with counters by `ActionType` and WM ingest latency.
- Add capability `observability.ingestor:v1.ingest` for explicit ingestion if needed outside the event sink path.
````