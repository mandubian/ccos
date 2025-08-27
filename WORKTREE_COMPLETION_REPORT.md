# Worktree Completion Report — wt/observability-foundation

Status: Ready for PR
Date: 2025-08-27

Scope
- Establish an observability foundation centered on the Causal Chain, including:
	- Modularized Causal Chain (ledger, signing, provenance, metrics).
	- Event sink wiring and Working Memory ingestion + replay helper.
	- Local capability: observability.ingestor:v1.ingest (single, batch, replay).
	- Lightweight in-memory metrics and structured JSON logs.
	- Minimal Prometheus-like exporter (feature-gated) + tiny example server.
	- Specs and guides updates with usage notes.

Key changes (high-level)
- rtfs_compiler/src/ccos/causal_chain/*: split into submodules; added metrics, recent logs, and sink notifications.
- rtfs_compiler/src/runtime/host.rs: surfaced getters for capability/function metrics and recent logs.
- rtfs_compiler/src/runtime/ccos_environment.rs: registered WM sink and ingestor capability on bootstrap.
- rtfs_compiler/src/runtime/metrics_exporter.rs: exporter with HELP/TYPE lines, label-escaped series; added duration histograms and a tiny HTTP server.
- rtfs_compiler/examples/serve_metrics.rs: feature-gated, single-request metrics server example.
- tests: added/updated exporter smoke test (feature-gated) and existing observability tests pass locally.
- docs/ccos/specs/023-observability-foundation.md and 024-observability-ingestor-capability.md: documented design and exporter usage; linked guides.

Validation
- Build: cargo build (default) and with --features metrics_exporter both succeed (warnings only).
- Tests:
	- Exporter unit test: runtime::metrics_exporter::tests::test_render_and_server_smoke — PASS.
	- Integration: feature-gated exporter smoke present; broader unrelated tests are filtered/out-of-scope.
- Manual smoke:
	- Example server builds and serves metrics once; metrics include cost, counters, gauges, and duration histograms.

How to reproduce (focused)
```bash
cd rtfs_compiler
# Build tests with exporter feature
cargo test --features metrics_exporter --no-run
# Run exporter unit test
cargo test --features metrics_exporter runtime::metrics_exporter::tests::test_render_and_server_smoke -- --nocapture
# Optional: run example server and curl once
cargo run --example serve_metrics --features metrics_exporter &
sleep 1 && curl -s http://127.0.0.1:9898/metrics | head -n 20
```

Notes / constraints
- Exporter is intentionally minimal and behind a Cargo feature to avoid prod impact and deps.
- Histograms are computed from the in-memory action list at render time; acceptable for tests/dev.

Next steps (optional follow-ups)
- Consider persisting histogram-ready aggregates if chains become large.
- Add dashboard snippets or scrape config examples to docs.
- Explore additional metrics (e.g., per ActionType counters, WM ingest latency).
