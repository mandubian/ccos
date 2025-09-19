# RTFS 2.0 (specs-incoming) Glossary

Concise glossary of acronyms and recurring terms appearing across `docs/rtfs-2.0/specs-incoming`. References in parentheses point to the main documents where terms are introduced or emphasized.

## Acronyms

- AI — Artificial Intelligence. RTFS is designed for AI-first authorship and governance. (00, 02, 18)
- AST — Abstract Syntax Tree. Parsing representation used for normalization and IR conversion. (00, 02, 12)
- ATSA — Admission-Time Static Analysis. Compiler/GK validation phase before execution. (12)
- CC — Causal Chain. Immutable audit ledger of actions/events. (00, 02; CCOS 003)
- CCOS — Cognitive Computing Operating System. The broader system RTFS integrates with. (00; CCOS specs)
- CIDR — Classless Inter-Domain Routing. Network effect parameter for ip_ranges. (07)
- DCE — Dead Code Elimination. IR optimization pass. (00, 02)
- DLP — Data Loss Prevention. ORCH egress filtering and policy enforcement. (07, 11, 19)
- DP — Differential Privacy. Privacy budget (epsilon) and training constraints. (13, 15)
- EWMA — Exponentially Weighted Moving Average. Telemetry estimator blending. (13)
- FS — Filesystem. Effect kind and policy for paths/modes, ephemeral roots. (07, 08, 19)
- GK — Governance Kernel. Policy admission, envelope signing, approvals/quorum. (00, 02, 12)
- gRPC — Remote procedure call protocol. Included in :network effect protocols. (07)
- GFM — Global Function Mesh. CCOS discovery/routing layer. (CCOS 000)
- HIPAA — Health Insurance Portability and Accountability Act. Regulated analytics constraints. (15)
- HMAC — Hash-based Message Authentication Code. Deterministic branch seed derivation. (08)
- HTTP — HyperText Transfer Protocol. Network capability and effect protocol. (07, 11)
- IAM — Identity and Access Management. IFC complements IAM; not a replacement. (11)
- IFC — Information Flow Control. Labels, taint propagation, declassification. (11)
- IR — Intermediate Representation. Canonical form for optimization/execution. (00, 02, 12)
- LLM — Large Language Model. Effect kind, determinism modes, token budgets. (07, 09, 15)
- mTLS — Mutual TLS. Network effect parameter and proxy enforcement control. (07, 19)
- OS — Operating System. MicroVM guest OS in deployment profile. (19)
- PII — Personally Identifiable Information. IFC label type; DLP focus. (11, 15)
- REPL — Read-Eval-Print Loop. Dev tooling for RTFS. (00, 02)
- RO/RW — Read-Only / Read-Write. Rootfs/mount modes and FS policy. (19)
- RPO/RTO — Recovery Point/Time Objective. Disaster recovery scenario metrics. (15)
- RTFS — Reason about The Fucking Spec. Kernel language and ecosystem. (00, 02)
- SaaS/SLA — Software as a Service / Service Level Agreement. Contracts/reputation context. (09, 15)
- SBOM — Software Bill of Materials. Supply-chain attestation in marketplace. (09, 15)
- S-expr — S-expression. Homoiconic code/data form of RTFS surface language. (00, 02)
- SLSA — Supply-chain Levels for Software Artifacts. Provenance/attestation. (09, 15)
- SLO — Service Level Objective. Ops gates and rollout checks. (15)
- TUF — The Update Framework. Signing/attestation mechanism. (09, 15)
- UI — User Interface. :ui effect kind. (07)
- UUID — Universally Unique Identifier. Literal type in RTFS. (00, 02)
- WASM — WebAssembly. Preferred per-step sandbox for isolation. (17, 19)

## Effect System (07-effect-system.md)

- Effect / Effect Item / Effect Row — Typed side-effect descriptors that compose and are enforced at compile-time/admission/runtime.
- Normalization — Merge duplicate effect kinds by intersecting parameters to most restrictive sets; fail-closed on conflicts.
- Subtyping (containment) — Narrower scopes (e.g., fewer domains) are subtypes of broader scopes.
- Row Polymorphism — Open effect rows with variables unified at call sites under bounds/policy.
- Purity — Empty effect row; only `(call ...)` can perform effects.
- Inference — Compiler propagation of effects from capability contracts and language constructs.
- Resources (envelope) — Complementary constraints (time, cost, tokens, locality) tracked alongside effects.

## Concurrency & Determinism (08-concurrency-and-determinism.md)

- step.parallel — Concurrent branches with deterministic join semantics; result ordered by branch lexical position.
- Branch Seeds — Deterministic per-branch seeds derived via HMAC(root_seed, plan/step ids, branch index).
- Fail-fast — Default on any branch failure; cancel siblings; execute compensations as needed.
- Idempotency (key/scope) — Dedup semantics for retries/replays; common scopes: :plan, :intent, :global.
- Compensations — Saga-style reverse-order undo steps on failure/abort.
- Timeout / Cancellation — Policy-controlled abort with compensations and auditable failure.

## Capability Contracts (09-capability-contracts.md)

- Contract Schema — Input/output types, effect/resource rows, determinism/idempotency, typed errors, security/attestation, semver, reputation.
- Determinism Modes — `:seeded` vs `:best_effort`; requirements pinned (e.g., temperature 0, model version).
- Typed Errors — Error variants enabling precise try/catch handling and shape validation.
- SemVer Rules — No privilege broadening in minor/patch; broadenings require major.

## Behavioral Contracts & Compensations (10-contracts-and-compensations.md)

- Pre/Post/Invariant Contracts — Pure predicates attached to steps/functions/plans to guard inputs/outputs/state.
- step.with-compensation — Pair primary effectful step with compensating step; idempotency recommended.
- Policy Gates — GK may require contracts and compensations for specific risk tiers/effect classes.

## Information Flow & Declassification (11-information-flow-and-declassification.md)

- Labels / Taint — Namespaced labels (e.g., `:ifc/pii`, `:ifc/eu_only`, `:ifc/confidential`) tracking data classifications.
- Boundary — Egress/storage/UI boundaries where enforcement occurs.
- Declassify — Explicit, policy-approved label reduction with recorded purpose/rationale and audit.
- Label Narrowing — GK-approved pure transforms (e.g., hashing) that reduce label scope under policy.

## Admission-Time Compilation & Caching (12-admission-time-compilation-and-caching.md)

- Admission-Time Compile — GK+compiler checks (types, effects, contracts, IFC, concurrency) before any effects.
- Plan Envelope — Signed admitted set: effects/resources, determinism metadata, contracts/versions, policy decisions.
- Normalization/Cache Keys — Canonical AST + metadata hashing for cache reuse; IR caching.
- Parametric Plan Templates — Reusable plan skeletons with typed parameter bounds and invariant envelopes.

## Resource Estimation & Envelopes (13-resource-estimation-and-envelopes.md)

- Resource Vector — cost/time/tokens/egress/compute/memory/privacy per call and aggregated across control-flow.
- Safety Margins — GK-defined buffers and ceilings with confidence and assumptions recorded.
- Guards — Runtime checks enforce bounds when admission had unknown inputs/sizes.

## Compiler Enhancement Plan (14-compiler-enhancement-plan-effects-and-types.md)

- Effect Row Model — Kinds/params; normalization and subtyping utilities.
- Contract Loader — Marketplace-driven binding with digests/semver enforcement.
- Admission API — `admit_plan` returns Type/Effect/Contract reports, envelope draft, IR or cache hit.

## Showcase Scenarios (15) & Intent→Plan (16)

- End-to-end examples — SRE self-healing, regulated analytics (HIPAA/GDPR), trading with quorum, supply-chain patch, legal with DLP, DR drill, generative capability.
- Typed Intents — Constraints like `:max_cost_usd`, `:data_locality`, `:privacy_budget {:epsilon ...}`, determinism, approvals/quorum.
- Diagnostics-driven Refinement — Arbiter iterates plans using compiler/GK hints to admission success.

## Agent Configuration (17) & AI Perspective (18)

- RTFS-native agent.config — Features, capabilities, governance keys/policies, orchestrator isolation, causal_chain, marketplace.
- Profiles/Macros — Reusable configuration generators in RTFS; AI-friendly AST rewrites.
- AI Ergonomics — Homoiconicity, gradual/refined typing, explicit effects, and admission diagnostics enable closed-loop synthesis.

## MicroVM Deployment Profile (19)

- MicroVM — Firecracker-style isolation: RO rootfs, proxy egress, vsock control plane, measured images/attestation.
- Egress Proxy — Domain allowlists, TLS pinning/mTLS, DLP filters, rate limiting.
- Supervisor — Synthesizes VM spec from agent.config; programs hypervisor and proxy ACLs.

---

This glossary is generated from the incoming RTFS 2.0 spec set and aligns with the referenced documents:
- 00 Overview, 02 Language Guide, 07 Effect System, 08 Concurrency/Determinism, 09 Capability Contracts, 10 Behavioral Contracts/Compensations, 11 IFC/Declassification, 12 Admission-Time & Caching, 13 Resource Estimation, 14 Compiler Plan, 15 Scenarios, 16 Intent→Plan, 17 Agent Config, 18 AI Perspective, 19 MicroVM Profile.
