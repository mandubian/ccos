# CCOS Specification 005: Security and Context (RTFS 2.0 Edition)

**Status:** Draft for Review  
**Version:** 1.0  
**Date:** 2025-09-20  
**Related:** [000: Architecture](./000-ccos-architecture-new.md), [004: Capabilities](./004-capabilities-and-marketplace-new.md), [003: Causal Chain](./003-causal-chain-new.md)  

## Introduction: Zero-Trust Governance for Pure Execution

Security in CCOS is proactive: The Governance Kernel enforces a human-signed Constitution on every yield/plan, leveraging RTFS 2.0's purity (no hidden effects). Runtime Context provides per-execution constraints (e.g., token limits). Together, they ensure alignment, preventing misuse while enabling reentrant safety.

Why vital? AI plans can be adversarial. Purity reduces attack surface; Kernel gates the rest. Reentrancy: Contexts persist across resumes, ACLs prevent escalation.

## Core Concepts

### 1. Governance Kernel: The Enforcer
High-privilege component loading the Constitution (rules like 'no PII writes without consent'). Validates before execution.

**Workflow**:
- **Plan Validation**: Scan IR for yields → Check against rules (e.g., disallow :exec.shell).
- **Yield Gate**: On `RequiresHost`, verify args/schema, provenance (intent/step), apply ACLs.
- **Scaffolding**: Wrap plans with handlers (e.g., timeout → abort).

**Sample Constitution Rule** (YAML-like, signed):
```
rules:
  - id: budget-limit
    condition: \":cost < 10.0\"
    action: allow
    scope: all-yields
  - id: data-privacy
    condition: \":cap in [:storage.read] and :privacy = :high\"
    action: deny-else-log
    scope: capabilities
```

**Validation Example**:
- Plan yield :storage.write {:data :user-pii} → Kernel: Matches privacy rule? No → Deny, log `Action {:type :GovernanceDenial}` → Arbiter adapts.

### 2. Runtime Context: Scoped Constraints
Per-execution env: Injected into RTFS, enforced by Orchestrator. Immutable Map, updated on resume.

**Fields**:
- `:intent-id`: Links to graph.
- `:quota`: {:tokens 8192, :yields 10, :budget 5.0}.
- `:acl`: Allowed caps (e.g., [:storage.read, :nlp.*]).
- `:sandbox`: Isolation (e.g., no net access).
- `:checkpoint`: Last chain ID for reentrancy.

**Sample Context**:
```
{:intent-id :intent-123
 :quota {:tokens 4096 :yields 3}
 :acl [:storage.fetch :nlp.sentiment]
 :sandbox {:network false}
 :checkpoint :act-456}
```

Injected: RTFS sees as env binding; yields include it for Kernel check.

Practical note: For plan execution, initialize the context ACL from `plan.capabilities_required` (as in the demo), then intersect with role/tenant policies. Deny-by-default remains the baseline—capabilities not present in ACL must be rejected by the Kernel even if included in the plan’s declaration.

### 3. Integration with RTFS 2.0 Purity
- **Compile-Time**: IR verify: Static disallow bad yields (e.g., regex on symbols).
- **Runtime**: Yields carry context → Kernel dynamic check (e.g., quota deduct on call).
- **Reentrancy**: Resume loads context from chain → Enforce same ACLs, preventing drift (e.g., no privilege gain post-pause).

**Reentrant Security Example**:
1. Start with context {:quota :yields 2}.
2. Yield 1: :storage.fetch → Success, deduct → Chain logs updated quota.
3. Pause → Resume: Load context from chain → Yield 2 allowed; Yield 3 denied → Safe, no escalation.

### 4. Auditing and Recovery
All denials/decisions logged to chain with rule ID. Recovery: Arbiter queries chain for context, proposes compliant plan.

RTFS purity + Kernel = Bulletproof: Effects explicit, governed; reentrancy contained.

### 5. Configuration (Consolidated Guidance)
Security posture is largely configuration-driven. Keep it simple, auditable, and versioned.

- **Constitution**: Path to signed rules (e.g., `config/constitution.rtfsir`). Kernel verifies on boot. Use semantic versions and change logs.
- **Delegation Policies**: Define DE scorer weights, thresholds, and fallbacks (e.g., `config/delegation.toml`). Hash incorporated into chain for reproducibility.
- **ACLs**: Capability allow-lists per role/context (`config/acl/*.toml`). Deny-by-default; explicit wildcards require DEC approval.
- **Quotas**: Runtime Context defaults (tokens/yields/budget); overridable per intent class.
- **Feature Flags**: Enable/disable experimental caps or legacy modes (e.g., no-legacy-atoms=true).

**Config Provenance**: Kernel logs config hashes at start and on reload to the Causal Chain to ensure future audits can reproduce decisions.

Next: Arbiter in 006.