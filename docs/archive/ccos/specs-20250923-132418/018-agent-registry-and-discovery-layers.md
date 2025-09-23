# 018 - Agent Registry & Discovery Layers (Capabilities vs Agents)

Status: Draft (Issue #23, Milestone M4)

## 1. Purpose
Clarify the architectural separation between primitive capability discovery/execution and higher-order agent delegation. Prevents conflation of tool onboarding (capabilities) with cognitive service selection (agents) as CCOS evolves.

Related Specs:
- 004-capabilities-and-marketplace.md (primitive execution layer)
- 006-arbiter-and-cognitive-control.md (cognitive role of Arbiter)
- 008-delegation-engine.md (delegation decision mechanics)
- 016-llm-execution-bridge.md (LLM integration)

## 2. Layer Comparison

| Layer | Purpose | Granularity | Lifecycle Object | Selection Inputs | Security / Governance Focus |
|-------|---------|-------------|------------------|------------------|-----------------------------|
| CapabilityRegistry + Marketplace | Execute atomic operations (IO, math, network, MCP tool, HTTP API) | Primitive function / tool | `CapabilityManifest` | ID, schemas, provider type, provenance/attestation, permissions | Fine‑grained allow list, schema/type validation, attestation, resource isolation (microVM) |
| CapabilityDiscovery Agents | Populate marketplace from external sources (network registry, local JSON manifests, file system) | N/A (ingest metadata) | Discovered manifests | Registry endpoints, file paths, auth tokens | Provenance stamping, content hash, freshness interval |
| AgentRegistry (Planned) | Delegate higher-level tasks / intents to autonomous agents (planners, specialist models, remote arbiters) | Composite cognitive service | `AgentDescriptor` (planned) | Skills/competencies, cost, latency, trust score, historical success, constraints | Delegation policy (ownership of intent), data sharing rules, privilege escalation prevention |

## 3. CapabilityRegistry / Marketplace
Holds executable primitives. Each capability = deterministic interface + (optional) type schemas + provenance + provider variant (Local, HTTP, MCP, A2A, Stream). Execution path: Evaluator → Host → Marketplace → Provider / Executor. Focus: low-level determinism, safety, attestation, sandboxing (e.g. microVM for higher risk operations).

## 4. CapabilityDiscovery
Pluggable discovery agents (e.g., `NetworkDiscoveryAgent`, `LocalFileDiscoveryAgent`) harvest manifests and feed them into the marketplace—no execution logic, only ingestion + normalization + provenance hashing. They schedule refresh, enforce content hashes, and annotate provenance (source URI, retrieval timestamp, signature status).

## 5. AgentRegistry (Upcoming)
Directory of autonomous agents able to accept an `Intent` (or sub-intent) and return a `Plan` and/or direct `Result`. The DelegatingArbiter consults it to decide: transform locally vs delegate to specialist.

Proposed `AgentDescriptor` (initial fields):
- `agent_id`
- `kind` (planner | analyzer | synthesizer | remote-arbiter | composite)
- `skills` (semantic tags, e.g. `:competitive-analysis`, `:data-synthesis`)
- `supported-constraints` (budget, data locality, compliance labels)
- Cost model (tokens/sec, $/call), latency distribution summary (p50/p95)
- Trust / governance tier (e.g. T0 sandbox, T1 trusted, T2 privileged)
- Isolation requirements (network, data domains)
- Historical performance (success rate, mean latency, calibration metrics)
- Provenance & attestation (signature, build hash)
- Quotas & rate limits

## 6. Why The Separation Matters
- Scaling: Thousands of capabilities; dozens (or fewer) higher-order agents.
- Governance Surface: Capabilities need type/permission enforcement; agents need delegation & data boundary policies (who may see which intent segments / constraint sets).
- Caching Semantics: Capability execution cached at *call* granularity; agent delegation decisions cached at *intent pattern / semantic embedding* granularity (DelegationEngine L1 + future vector index).
- Learning Hooks: Agent layer yields strategic adaptation (plan quality, delegation success curves); capability layer yields reliability / availability / cost metrics.
- Risk Stratification: A mis-scoped capability call = localized side-effect; a mis-delegated intent = strategic drift or privacy boundary breach → stronger pre-delegation checks.

## 7. Delegation Flow (Planned Extension)
1. User (or upstream process) submits natural language goal.
2. DelegatingArbiter performs lightweight intent draft + constraint extraction.
3. Queries AgentRegistry for candidate agents ranked by (skill coverage, constraint compatibility, cost/latency budget, trust tier).
4. Produces `DelegationMetadata` proposal: { selected_agent?, rationale, expected-plan-shape, fallback_strategy }.
5. Governance Kernel evaluates delegation policy (e.g., budget, trust tier vs intent sensitivity).
6. If approved: intent ownership temporarily transferred / shared; agent produces plan; plan returns for normal validation & execution.
7. Telemetry + outcome metrics recorded to Causal Chain; performance metrics update AgentRegistry scores.

## 8. Governance & Policy Considerations
- Delegation Approval Rules: (intent sensitivity × agent trust tier) matrix.
- Data Boundary Controls: Redact intent fields before delegation if agent tier lower than required clearance.
- Economic Constraints: Enforce max projected cost; perform pre-flight cost simulation if agent supplies cost model.
- Revocation: Active delegated intent can be revoked if agent trust score drops or new constraint injected.
- Audit: Each delegation yields `DelegationProposed`, `DelegationApproved|Rejected`, `DelegationCompleted` actions with rationale & hashes.

## 9. Security & Attestation
Agents must provide signed descriptors. High-tier agents may require reproducible build provenance. Runtime isolates delegated execution context (separate memory, capability allow list subset). Outputs must pass validation (type, constraint compliance) before integration into primary Intent Graph.

## 10. Planned M4 Work (Snapshot)
1. Define `AgentRegistry` trait + in-memory implementation.
2. Extend DelegatingArbiter to query AgentRegistry pre LLM inference (produce `DelegationMetadata`).
3. Governance rule: approve/deny delegation based on trust tier vs intent sensitivity & constraints.
4. Telemetry: record agent selection rationale into Causal Chain.
5. Basic scoring model (success rate decay-weighted, latency penalty, cost efficiency).

## 11. Future Enhancements (Post-M4)
- Embedding-based semantic retrieval of agents by skill vector.
- Adaptive delegation (multi-agent negotiation / arbitration, see potential future spec).
- Federated agent trust recalibration using cross-intent performance cohorts.
- Auto-promotion / demotion of agents across trust tiers with human override gating.

## 12. Interactions With Existing Components
| Component | Interaction |
|-----------|------------|
| DelegationEngine | Supplies ranking inputs & caches results (intent pattern → selected agent). |
| Governance Kernel | Enforces delegation policies & constraints pre-approval. |
| Capability Marketplace | Still executes plan steps emitted by delegated agents. |
| Causal Chain | Records delegation lifecycle & metrics for audit and learning. |
| Intent Graph | Tracks ownership / provenance of sub-intents and delegated derivations. |

## 13. Open Questions
- Partial Plan Delegation: Do we allow hybrid local + delegated co-generation within a single intent cycle?
- Multi-Agent Consensus: Minimum quorum for high-risk intents?
- Privacy Gradient: Formal model for redaction tiers prior to delegation.

## 14. Migration Notes
README section removed; this spec is now the canonical source. Future PRs adding AgentRegistry code MUST reference this document in summary.

---
End of Spec 018.
