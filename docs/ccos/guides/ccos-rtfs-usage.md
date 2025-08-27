# CCOS/RTFS Usage Guide (Practical How-To)

This guide shows how to use CCOS with RTFS in practice: call capabilities from Rust, inspect metrics/logs, and optionally execute a tiny RTFS plan via the CLI.

Audience: engineers integrating CCOS runtime or writing smoke tests.

## Programmatic usage (Rust)

Minimal example that calls a capability and inspects observability:

```rust
use rtfs_compiler::runtime::ccos_environment::CCOSBuilder;
use rtfs_compiler::runtime::host_interface::HostInterface;
use rtfs_compiler::runtime::values::{MapKey, Value};

#[test]
fn ccos_usage_smoke_test() {
    // Helpful for tests without explicit context
    std::env::set_var("CCOS_TEST_FALLBACK_CONTEXT", "1");

    let env = CCOSBuilder::new().build().expect("env");

    // Execute a capability (observability ingestor, single mode)
    let mut rec = std::collections::HashMap::new();
    rec.insert(MapKey::String("action_id".into()), Value::String("doc-1".into()));
    rec.insert(MapKey::String("summary".into()), Value::String("from-usage-guide".into()));
    rec.insert(MapKey::String("content".into()), Value::String("hello-ccos".into()));

    let args = vec![Value::String("single".into()), Value::Map(rec)];
    let _ = env
        .host
        .execute_capability("observability.ingestor:v1.ingest", &args)
        .expect("cap execution");

    // Metrics: capability + function views
    let cap = env
        .host
        .get_capability_metrics("observability.ingestor:v1.ingest")
        .expect("cap metrics");
    assert!(cap.total_calls >= 1);

    let fun = env
        .host
        .get_function_metrics("observability.ingestor:v1.ingest")
        .expect("fun metrics");
    assert!(fun.total_calls >= 1);

    // Structured logs (recent)
    let logs = env.host.get_recent_logs(32);
    assert!(logs.iter().any(|l| l.contains("action_appended") || l.contains("action_result_recorded")));
}
```

Notes
- CCOSBuilder wires defaults, including the local `observability.ingestor` capability and WM sink (if enabled).
- Prefer small, deterministic assertions (>=1) for counters in smoke tests.

## Optional: run an RTFS plan with the CLI

Create `examples/ingest.rtfs` (under `rtfs_compiler/` if using the workspace layout):

```lisp
(do
  (step "ingest one record"
    (call :observability.ingestor:v1.ingest {
      "mode": "single",
      "record": { "action_id": "rtfs-1", "summary": "hello", "content": "from-rtfs" }
    })))
```

Then run (from `rtfs_compiler/`):

```bash
cargo run --bin rtfs-compiler -- --input examples/ingest.rtfs --execute --show-timing --show-stats
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
