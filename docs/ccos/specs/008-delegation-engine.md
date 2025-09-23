# CCOS Specification 008: Delegation Engine (RTFS 2.0 Edition)

**Status:** Draft for Review  
**Version:** 1.0  
**Date:** 2025-09-20  
**Related:** [000: Architecture](./000-ccos-architecture-new.md), [007: Global Function Mesh](./007-global-function-mesh-new.md), [005: Security](./005-security-and-context-new.md)  

## Introduction: Policy-Driven Provider Selection

The Delegation Engine (DE) is CCOS's decision layer: After GFM provides capability candidates, DE selects the optimal provider based on runtime policies (e.g., cost, privacy, latency). Pluggable and configurable, it ensures yields align with context (e.g., intent constraints). In RTFS 2.0, DE operates on yield requests, keeping plans abstract and pure—selection is host-side, transparent via chain logs.

Why critical? GFM finds 'what's available'; DE decides 'which one'—balancing tradeoffs. Reentrancy: Deterministic selection on resume (policy-seeded randomness or fixed).

## Core Concepts

### 1. DE Structure and Scoring
DE uses pluggable scorers (e.g., CostScorer, PrivacyScorer) to rank GFM candidates.

**Input**: Yield request + candidates from GFM.
**Policies**: From Runtime Context (e.g., {:prefer-local true, :max-latency 100ms, :budget-per-call 0.05}).
**Output**: Selected provider (e.g., {:impl :local-nlp, :score 0.95}).

**Scoring Example** (Pseudo-RTFS for config):
```
;; Policy as RTFS Map (compiled for verification)
{:scorers [cost-weight 0.4, latency-weight 0.3, privacy-weight 0.3]
 :threshold 0.8
 :fallback :cheapest}
```

### 2. Workflow
1. GFM returns candidates.
2. DE applies scorers: e.g., Cost = 1 - (price / budget); Privacy = match-level (0-1).
3. Weighted sum → Rank; select top if > threshold, else fallback.
4. Log decision to chain.

**Diagram: Selection Process**:
```mermaid
graph LR
    Yield[Yield Request<br/>:nlp.sentiment + Context]
    GFM[GFM: Candidates<br/>(OpenAI, Local, HuggingFace)]
    DE[Delegation Engine<br/>Apply Policies]
    S1[Cost Scorer: OpenAI=0.2, Local=0.9]
    S2[Latency: OpenAI=0.8, Local=0.95]
    S3[Privacy: OpenAI=0.6, Local=1.0]
    Rank[Ranked: Local (0.93) > OpenAI (0.53)]
    Select[Select Local]
    Exec[Execute + Resume RTFS]

    Yield --> GFM
    GFM --> DE
    DE --> S1 & S2 & S3
    S1 & S2 & S3 --> Rank
    Rank --> Select
    Select --> Exec
```

### 3. Integration with RTFS 2.0
- **Yield Context**: Requests include policies from env (e.g., intent constraints) → DE uses for scoring.
- **Reentrancy**: Selections logged in chain (`Action {:type :DelegationDecision, :policy-hash \"xyz\"}`); resume re-applies same policy for consistency.
- **Purity**: DE is opaque to RTFS—plans yield abstractly; host decides.

**Sample in Reentrant Resume**:
- Initial yield: DE selects Local (low latency).
- Pause → Chain logs decision.
- Resume: Re-yield with same context → DE re-selects Local (no drift).

### 4. Pluggability and Configuration
- **Custom Scorers**: Implement as host modules (e.g., GeoScorer for location).
- **Governance Tie-In**: Kernel vetoes selections (e.g., deny high-risk providers).

DE turns availability into alignment: Policies ensure yields serve intents safely, reentrantly.

### Future: Extending Selection to Agents
The Delegation Engine’s policy/scorer framework can later include agent selection alongside provider selection, without API breaks:
- Reuse the same candidate → scorer → rank → select pipeline.
- Add an `:agent` candidate type with additional scorers (skills match, trust tier).
- Keep policy files compatible by adding optional sections; old configs remain valid.

This ensures multi-agent delegation can be layered in incrementally.

Next: Context Horizon in 009.