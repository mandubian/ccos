wt/observability-foundation — planned — SEP-003: Add Causal Chain Event Stream + Wire Working Memory Ingestor

Goals & references:
- Issue #6 follow-up: Add Causal Chain Event Stream + Wire Working Memory Ingestor
- #56, [CCOS] Observability: Metrics, logging, and dashboards
- #34, [CCOS] Metrics and logging for critical flows
- #37, [CCOS] Dashboards for intent graph, causal chain, and capability usage (#38)

Planned work:
- Add event stream hooks to CausalChain append path (ActionType::CapabilityCall, delegation events).
	- Hook must append an audit-safe event object to the existing event pipeline (use metadata namespace `observability.*`).
- Wire a Working Memory ingestor to publish selected events to stream/metrics.
	- Implement an adapter capability `observability.ingestor:v1.ingest` that accepts action metadata and publishes to a configurable sink.
- Add basic metrics (counters/histograms) for capability calls, delegation approvals, failures.
	- Expose a minimal Prometheus-compatible endpoint from a test harness or the test runner (see `rtfs_compiler/test_runner`).
- Add logging integration points and a small dashboard spec for intent graph & capability usage.
	- Add structured logs (JSON) for capability calls with: timestamp, capability_id, intent_id, result, latency_ms.

Notes:
- Base branch: origin/main
- Keep changes minimal and well-tested; add integration tests for event emission.

Assumptions
- This work will touch `rtfs_compiler` and may add a small capability adapter under `runtime/` or `runtime/stdlib` as per repo conventions.
- Local verification will be done by building `rtfs_compiler` and running its test runner; top-level workspace has no `Cargo.toml` so run crate-level commands.

Acceptance criteria
- CausalChain append path emits events for ActionType::CapabilityCall and delegation events into the observability pipeline.
- An ingestor capability exists and can be invoked from an integration test to assert event delivery (mock sink allowed).
- Metrics counters/histograms are incremented for capability calls and delegation approvals; a small smoke test verifies metric values.
- No breaking changes to the public API; all new behavior is behind feature flags or opt-in configuration where appropriate.

Implementation checklist
- [ ] Add event emission points in `causal_chain.rs` where actions are appended.
- [ ] Implement `observability.ingestor:v1.ingest` capability (adapter) and register it in capability marketplace.
- [ ] Add metrics collection primitives and lightweight Prometheus exposition in `rtfs_compiler/test_runner` or a test-only endpoint.
- [ ] Add structured logging for capability calls.
- [ ] Add unit tests covering event creation and metadata shape.
- [ ] Add an integration test that runs a synthetic plan and asserts that the ingestor received expected events and metrics were updated.
- [ ] Update docs: `docs/ccos/specs/023-observability-foundation.md` with event schema and dashboard notes.

Local quick verification
- Build the compiler crate (from repo root):
```bash
cd rtfs_compiler
cargo build
```
- Compile tests (no-run) and run the integration test runner locally:
```bash
cd rtfs_compiler
cargo test --test integration_tests -- --nocapture
```
Note: tests and integration runner names may vary; list available tests with `cargo test -- --list`.

Files likely to change
- `rtfs_compiler/src/causal_chain.rs` (emit hooks)
- `rtfs_compiler/src/...` or `runtime/` (ingestor capability)
- `rtfs_compiler/test_runner/*` (integration test harness)
- `docs/ccos/specs/023-observability-foundation.md`

CI and follow-ups
- Add a CI job (or extend existing) that builds `rtfs_compiler` and runs integration tests that assert event emission.
- If adding a networked Prometheus endpoint, gate it behind a test-only config to avoid exposing during normal runs.

Estimated effort
- Small change set (2-4 PRs): core hook + ingestor + tests + docs. Each PR should be small and independently reviewable.

Done initial bootstrap; start with small PR that adds hooks and a mock ingestor + tests.

