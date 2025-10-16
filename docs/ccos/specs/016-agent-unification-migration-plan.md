# 016 — Agent Unification Migration Plan

Goal: unify Agents into the CapabilityMarketplace by treating agents as capabilities with metadata (:kind :agent) and policy gates, deprecating AgentRegistry/AgentConfig/AgentDescriptor.

## Scope
- One artifact shape (capability spec) for all executable units
- Marketplace as the single registry for capabilities and agents
- Arbiter/Delegation reads from marketplace with filters
- Governance applies stricter policies to agent-kind artifacts

## Tasks

1) Define agent-as-capability metadata and schema update [in_progress]
- Add metadata flags to capability manifests: :kind, :planning, :stateful, :interactive
- Update manifest validation and attestation to include these fields
- Acceptance: manifests with :kind :agent parse, store, and surface via marketplace APIs

2) Extend marketplace discovery to filter :kind :agent and flags [pending]
- Add query filters: kind, planning, stateful, interactive
- Update listing/search APIs and index
- Acceptance: `list(kind=:agent, planning=true)` returns agent artifacts

3) Migrate Arbiter/Delegation to query marketplace, not AgentRegistry [pending]
- Update `DelegatingArbiter` discovery/selection paths to use marketplace APIs
- Maintain a compatibility shim during migration
- Acceptance: demo flows run with AgentRegistry disabled (behind feature flag)

4) Create adapters: AgentRegistry -> marketplace-backed shim [pending]
- Implement `AgentRegistryShim` delegating to marketplace
- Keep AgentRegistry types to avoid breaking callers; mark deprecated
- Acceptance: existing callers run unchanged using shim-backed data

5) Deprecate AgentRegistry/AgentConfig/AgentDescriptor APIs [pending]
- Add deprecation annotations and warnings
- Document sunset timeline (one release cycle)
- Acceptance: no new code uses deprecated APIs; CI warns on new references

6) Update governance: stricter policies when :kind :agent [pending]
- Enforce delegation rules, human-in-loop checks, and long-running limits
- Add audit requirements for planning/selection decisions
- Acceptance: policy engine differentiates capability vs agent at runtime

7) Update docs/specs; add migration notes and examples [pending]
- Reference 015—Capabilities vs Agents; this plan (016)
- Add examples for primitive/composite/agent and marketplace queries
- Acceptance: docs reflect unified model and migration steps

8) Remove legacy agent types after release cycle [pending]
- Delete AgentRegistry, AgentConfig, AgentDescriptor
- Remove shim and feature flags
- Acceptance: codebase compiles/tests without legacy agent types

## Milestones

M1 — Metadata & Discovery (Tasks 1-2)
- Manifests accept agent metadata; marketplace filters by :kind and flags

M2 — Arbiter Migration (Tasks 3-4)
- Arbiter uses marketplace; AgentRegistry shim in place

M3 — Governance Tightening (Task 6)
- Policies differentiate agents; audits extended

M4 — Documentation & Adoption (Task 7)
- Specs updated; examples and migration guidance published

M5 — Decommission Legacy (Task 8)
- Legacy agent types removed post-release cycle

## Risks & Mitigations
- Risk: Hidden dependencies on AgentRegistry
  - Mitigation: Introduce shim; CI deprecation warnings; phased rollout
- Risk: Policy regressions for agents
  - Mitigation: Add targeted tests for delegation/human-in-loop/stateful policies
- Risk: Marketplace index performance under new filters
  - Mitigation: Add indexes; measure and optimize queries

## Acceptance Criteria
- Single registry (marketplace) provides both capabilities and agents through one API
- Arbiter/Delegation exclusively query marketplace
- Agent governance enforced via metadata
- Documentation reflects unified model
- Legacy agent types removed after deprecation window
