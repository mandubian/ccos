# CCOS/RTFS Usage Guide (Practical How-To)

This guide shows how to use CCOS with RTFS in practice: call capabilities from Rust, inspect metrics/logs, and execute RTFS plans via the CLI.

## Programmatic usage (Rust)

Minimal example that calls a capability and inspects observability. Note that CCOS now uses a unified workspace:

```rust
use ccos::ccos_core::CCOSBuilder;
use rtfs::runtime::values::{MapKey, Value};

#[tokio::test]
async fn ccos_usage_smoke_test() {
    let engine = CCOSBuilder::new().build().expect("engine");

    // Execute a capability via the orchestrator
    // ...
}
```

## Optional: run an RTFS plan with the CLI

Create `ingest.rtfs`:

```lisp
(do
  (step "ingest one record"
    (call :observability.ingestor.ingest {
      :mode "single"
      :record { :action_id "rtfs-1" :summary "hello" :content "from-rtfs" }
    })))
```

Then run from the root directory:

```bash
cargo run --bin rtfs-compiler -- --input ingest.rtfs --execute
```

Tip: set `CCOS_LOG_BUFFER_CAPACITY=256` to retain more in-memory structured logs during local runs.

## Common environment flags
- CCOS_TEST_FALLBACK_CONTEXT=1: tests without full plan context.
- CCOS_ENABLE_WM_INGESTOR=1: ensure Working Memory ingestor sink is active.
- CCOS_WM_MAX_ENTRIES / CCOS_WM_MAX_TOKENS: bound WM memory usage.
- CCOS_LOG_BUFFER_CAPACITY: size of in-memory structured log ring buffer.

## Troubleshooting
- No logs/metrics? Ensure the env flags above are set appropriately and the capability call succeeded.
- Capability not found? Verify it’s registered at bootstrap (default CCOSBuilder includes local ingestor).
- RTFS CLI run errors? Validate the plan uses only supported forms: `(do ...)` with `(call :cap.id:vN.op { ... })` inside steps.

## References
- Specs: SEP-023 (Observability Foundation), SEP-024 (Ingestor Capability)
- Guide: Observability Quickstart (metrics + structured logs) — `docs/ccos/guides/observability-quickstart.md`
