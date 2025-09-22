# CCOS Specification 004: Capabilities and Marketplace (RTFS 2.0 Edition)

**Status:** Draft for Review  
**Version:** 1.0  
**Date:** 2025-09-20  
**Related:** [000: Architecture](./000-ccos-architecture-new.md), [002: Plans](./002-plans-and-orchestration-new.md), [005: Security](./005-security-and-context-new.md)  

## Introduction: Extensible Effects Without Bloat

Capabilities are CCOS's way to provide RTFS plans with real-world powers: Functions like `:storage.fetch` that the pure RTFS engine yields to. The Marketplace is a registry for discovering signed providers. In RTFS 2.0, capabilities are the *only* effects—plans stay pure, yields explicit. This keeps the language minimal while enabling infinite extension.

Why key? RTFS doesn't know 'files' or 'NLP'; CCOS does, via governed calls. Reentrancy: Capabilities are idempotent, supporting resume without re-side-effects.

## Core Concepts

### 1. Capability Structure
A Capability is a host function: Abstract name + implementation(s).

**Fields** (Signed Manifest, RTFS Map):
- `:id` (Symbol): e.g., :storage.fetch.
- `:version` (String): Semantic (1.0.0).
- `:description` (String): Purpose.
- `:input-schema` (Map): Expected args (e.g., {:bucket String, :key String}).
- `:output-schema` (Map): Returned Value.
- `:providers` (List<Map>): Marketplace entries (e.g., {:impl :aws-s3, :cost 0.01, :latency 100ms, :acl :public}).
- `:attestation` (String): Crypto signature (proves trust/source).
- `:idempotent?` (Bool): Safe to retry.

**Sample Capability Call in Plan**:
```
(call :nlp.sentiment
      {:text \"Great product!\"
       :model :v3
       :intent-id :intent-123})  ;; Yield with context
```
Yields: `RequiresHost({:cap :nlp.sentiment, :args {...}, :idempotent-key \"uuid-789\"})`.

### 2. The Marketplace: Discovery and Selection
Content-addressable store of manifests. Providers register (signed); consumers discover.

- **Registration**: Impl (e.g., AWS Lambda) publishes manifest + attestation.
- **Discovery**: GFM queries by ID/version/constraints (e.g., low-latency).
- **Selection**: Delegation Engine scores (cost, trust, policy) → Picks best.

**Resolution Flow Diagram**:
```mermaid
graph LR
    Plan[RTFS Plan Yield<br/>:nlp.sentiment]
    GFM[Global Function Mesh<br/>Queries Marketplace]
    MP[Marketplace<br/>Signed Manifests]
    DE[Delegation Engine<br/>Filters: Cost < 0.05, ACL:ok]
    Prov1[AWS Provider<br/>Version 1.0, Cost 0.02]
    Prov2[Local Provider<br/>Version 1.1, Cost 0.01]
    
    Plan --> GFM
    GFM --> MP
    MP --> Prov1 & Prov2
    DE --> Prov2  ;; Selects best
    Prov2 --> Exec[Execute + Return Result]
    Exec --> Resume[Resume RTFS]
```

### 3. Integration with RTFS 2.0
Plans reference by symbol; runtime yields on unknown. CCOS host:
1. Receives yield → Kernel validates schema/ACL.
2. GFM finds matches → DE selects (e.g., prefer local for reentrancy).
3. Execute → Log to chain → Resume with result (injected as Value).

**Reentrant Example** (Retry on Failure):
- Yield :storage.fetch → Fails (network).
- Chain: `Action {:type :CapabilityCall, :success false, :retry-key \"idemp-123\"}`.
- Resume: Re-yield with same key → Provider skips dupes (CAS or check) → Success, continue pure map.

Idempotency ensures reentrancy: Resumes don't duplicate effects.

### 4. Security and Governance
- **Attestation**: Kernel verifies signature before resolution.
- **Versioning**: Plans pin versions; marketplace deprecates old.
- **Quotas**: Per-cap, tracked in chain.

### 4.a Attestation (Summary)
All provider manifests must be cryptographically signed. The Governance Kernel verifies signatures and revocation status before any execution. This guards against tampering and supply-chain attacks.

- **What is Verified**: Manifest data hash, issuer chain, timestamp/freshness, optional binary hash proofs.
- **When**: On resolution and at each yield (re-verified on resume).
- **Where Logged**: Causal Chain as `:AttestationVerified` actions.

For deeper details (revocation, trust roots, proofs), see [011-capability-attestation-new.md](./011-capability-attestation-new.md).

Capabilities make RTFS 'complete': Pure core + governed extensions, reentrant by design.

Next: Security in 005.