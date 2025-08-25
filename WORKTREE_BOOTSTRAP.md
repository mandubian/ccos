wt/observability-foundation — planned — SEP-003: Add Causal Chain Event Stream + Wire Working Memory Ingestor

Goals & references:
- Issue #6 follow-up: Add Causal Chain Event Stream + Wire Working Memory Ingestor
- #56, [CCOS] Observability: Metrics, logging, and dashboards
- #34, [CCOS] Metrics and logging for critical flows
- #37, [CCOS] Dashboards for intent graph, causal chain, and capability usage (#38)

Planned work:
- Add event stream hooks to CausalChain append path (ActionType::CapabilityCall, delegation events).
- Wire a Working Memory ingestor to publish selected events to stream/metrics.
- Add basic metrics (counters/histograms) for capability calls, delegation approvals, failures.
- Add logging integration points and a small dashboard spec for intent graph & capability usage.

Notes:
- Base branch: origin/main
- Keep changes minimal and well-tested; add integration tests for event emission.

