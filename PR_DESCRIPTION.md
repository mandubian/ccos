# PR: wt/observability-foundation — Causal Chain observability, WM ingestor, and minimal metrics exporter

Branch: wt/observability-foundation

Summary
- Modularizes the Causal Chain (ledger, signing, provenance, metrics) and adds event sinks.
- Wires a Working Memory sink for continuous ingestion + a replay helper.
- Adds local capability `observability.ingestor:v1.ingest` supporting single, batch, and replay.
- Introduces lightweight in-memory performance metrics and structured JSON logs.
- Implements a feature-gated Prometheus-like exporter (text format) and a tiny single-request HTTP server.
- Adds a small example `serve_metrics` and updates specs/guides with quickstart and exporter docs.

Files changed (high level)
- `rtfs_compiler/src/ccos/causal_chain/` — modular chain with metrics, logs, sinks.
- `rtfs_compiler/src/runtime/host.rs` — exposes metrics/log getters.
- `rtfs_compiler/src/runtime/ccos_environment.rs` — WM sink + ingestor capability registration.
- `rtfs_compiler/src/runtime/metrics_exporter.rs` — exporter (HELP/TYPE, label escaping, avg gauges, duration histograms) + minimal HTTP.
- `rtfs_compiler/examples/serve_metrics.rs` — test-only server example under `metrics_exporter` feature.
- `docs/ccos/specs/023-observability-foundation.md` (and 024) — specs updated with exporter notes and examples.
- `WORKTREE_COMPLETION_REPORT.md` — this report.

What I verified locally
- Build succeeds with and without `--features metrics_exporter` (warnings only).
- Exporter unit test `runtime::metrics_exporter::tests::test_render_and_server_smoke` passes.
- Feature-gated integration smoke present; metrics output includes cost, counters, gauges, and duration histograms.

Suggested commands (focused)
```bash
cd rtfs_compiler
cargo test --features metrics_exporter --no-run
cargo test --features metrics_exporter runtime::metrics_exporter::tests::test_render_and_server_smoke -- --nocapture
# Optional example server
cargo run --example serve_metrics --features metrics_exporter &
sleep 1 && curl -s http://127.0.0.1:9898/metrics | head -n 20
```

Known issues / CI notes
- Exporter is dev/test-focused and disabled by default via feature gate; no external deps added.
- Histogram aggregation is computed on render from in-memory actions; adequate for the intended scope.

Reviewer guidance
- Confirm Causal Chain modularization and metrics/log surfaces in `host`.
- Check feature-gated exporter’s HELP/TYPE lines, label escaping, and histogram format.
- Skim `023-observability-foundation.md` “Try it locally” and metrics list for clarity.

Next steps (post-merge)
- Consider adding ActionType counters and WM ingest latency metrics.
- If needed, add persistent/cached aggregates for large chains.

---

Include this report and the updated specs in the PR for context.
