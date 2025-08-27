# PR Checklist — wt/observability-foundation

- [ ] Causal Chain modularization: ledger/signing/provenance/metrics compile and are used.
- [ ] Working Memory sink registered by default; replay helper available.
- [ ] Capability `observability.ingestor:v1.ingest` registered; basic happy-path tests pass.
- [ ] Structured logs and metrics surfaced via `RuntimeHost` getters.
- [ ] Prometheus exporter behind `metrics_exporter` feature; HELP/TYPE lines present; label escaping correct.
- [ ] Duration histograms emitted for capability/function with expected buckets; trailing newline present.
- [ ] Example `serve_metrics` builds and serves one request.
- [ ] Docs: `023-observability-foundation.md` updated (metrics list + Try it), `024` cross-referenced; README links preserved.
- [ ] `WORKTREE_COMPLETION_REPORT.md` filled with summary and how-to-run.

Quick verification (optional)
- [ ] Build with feature: `cargo test --features metrics_exporter --no-run`
- [ ] Run unit test: `cargo test --features metrics_exporter runtime::metrics_exporter::tests::test_render_and_server_smoke -- --nocapture`
- [ ] Example server: `cargo run --example serve_metrics --features metrics_exporter` then `curl -s localhost:9898/metrics`