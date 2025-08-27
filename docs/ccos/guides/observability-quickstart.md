# CCOS Guide: Observability Quickstart (Metrics + Structured Logs)

Audience: CCOS/RTFS users who want a minimal, practical example to verify observability (metrics + logs) from code.

Prerequisites
- You’re using the `rtfs_compiler` crate in tests or a small harness.
- Working Memory ingestor enabled (default) or via env `CCOS_ENABLE_WM_INGESTOR=1`.

Minimal example (Rust)

```rust
use rtfs_compiler::runtime::ccos_environment::CCOSBuilder;
use rtfs_compiler::runtime::host_interface::HostInterface;
use rtfs_compiler::runtime::values::{MapKey, Value};

#[test]
fn observability_quickstart() {
    // For tests without explicit plan context
    std::env::set_var("CCOS_TEST_FALLBACK_CONTEXT", "1");

    let env = CCOSBuilder::new().build().expect("env");

    // 1) Call the ingestor capability once (single mode)
    let mut rec = std::collections::HashMap::new();
    rec.insert(MapKey::String("action_id".into()), Value::String("demo-1".into()));
    rec.insert(MapKey::String("summary".into()), Value::String("hello".into()));
    rec.insert(MapKey::String("content".into()), Value::String("payload".into()));
    let args = vec![Value::String("single".into()), Value::Map(rec)];
    let _ = env.host.execute_capability("observability.ingestor:v1.ingest", &args).expect("cap ok");

    // 2) Read capability and function metrics
    let cap = env.host
        .get_capability_metrics("observability.ingestor:v1.ingest")
        .expect("cap metrics");
    assert!(cap.total_calls >= 1);

    let fun = env.host
        .get_function_metrics("observability.ingestor:v1.ingest")
        .expect("fun metrics");
    assert!(fun.total_calls >= 1);

    // 3) Inspect recent structured logs
    let logs = env.host.get_recent_logs(16);
    assert!(logs.iter().any(|l| l.contains("action_appended") || l.contains("action_result_recorded")));
}
```

What this demonstrates
- A capability call records two chain events: the append and later the result; both are captured in structured logs.
- Metrics are tracked for both capability id and function name; your assertions can use either.

Troubleshooting
- If metrics/logs are empty in tests, ensure `CCOS_TEST_FALLBACK_CONTEXT=1` is set or that you set the execution context explicitly.
- Ensure the ingestor capability is registered (it is by default in `CCOSEnvironment::new`).

See also
- SEP-023: Observability Foundation
- SEP-024: Observability Ingestor Capability
- CCOS/RTFS Usage Guide: docs/ccos/guides/ccos-rtfs-usage.md
