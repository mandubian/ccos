# CCOS Specification 011: Capability Attestation (RTFS 2.0 Edition)

**Status:** Draft for Review  
**Version:** 1.0  
**Date:** 2025-09-20  
**Related:** [000: Architecture](./000-ccos-architecture-new.md), [004: Capabilities](./004-capabilities-and-marketplace-new.md), [010: Ethical Governance](./010-ethical-governance-new.md)  

## Introduction: Verifiable Trust for Providers

Capability Attestation ensures providers in the Marketplace are authentic and untampered: Each manifest includes cryptographic signatures, verified by the Governance Kernel before resolution/execution. In RTFS 2.0, attestation happens on yield requests—Kernel checks proofs against a trust root, preventing malicious code in yields. Supports revocation and versioning for ongoing security.

Why vital? Marketplace is open; attestation builds zero-trust. Reentrancy: Verified on every resume yield, no stale trusts.

## Core Concepts

### 1. Attestation Structure
Providers sign manifests with keys from trusted CAs or DEC.

**Fields in Manifest** (Extended from 004):
- `:attestation`: {:signature (bytes), :public-key (pem), :timestamp, :ca-chain}.
- `:revocation`: {:status :active, :revoke-hash (if deprecated)}.
- `:proof`: Merkle-proof of code integrity (e.g., hash of impl binary).

**Verification Process**:
1. Kernel loads trust root (Constitution-signed CAs).
2. On yield: Extract sig from manifest → Verify against key → Check timestamp/revocation.
3. If valid, proceed to GFM/DE.

**Sample Attestation Check** (Kernel Pseudo):
```
;; Pure RTFS for Verification Logic
(let [sig (get manifest :attestation.signature)
      pubkey (get manifest :public-key)
      data-hash (hash manifest.data)]
  (if (verify-sig sig pubkey data-hash)
    :valid
    :invalid))  ;; Yield error if fail
```

### 2. Workflow
Integrated into yield path.

**Diagram: Attestation in Resolution**:
```mermaid
sequenceDiagram
    O[Orchestrator] --> Yield[RTFS Yield :cap-xyz]
    Yield --> GK[Governance Kernel]
    GK --> Manifest[Fetch Manifest from Marketplace]
    Manifest --> Sig[Extract Signature + Key]
    Sig --> Verify[Verify Sig + Revocation<br/>(Pure RTFS Check)]
    alt Valid
        Verify --> GFM[Proceed to GFM/DE]
    else Invalid
        Verify --> Chain[Log Denial to Chain]
        Chain --> Arbiter[Notify Arbiter for Adaptation]
    end
    GFM --> Exec[Execute Provider]
```

### 3. Integration with RTFS 2.0 Reentrancy
- **Per-Yield Check**: Every resume yield re-verifies (fresh manifest fetch).
- **Purity**: Verification as pure RTFS function in Kernel—no side effects.
- **Chain Logging**: `Action {:type :AttestationVerified, :cap :xyz, :sig-hash \"abc\"}`.

**Reentrant Example**:
- Initial yield: Attest valid → Execute.
- Resume after pause: Re-yield → Re-attest (checks for revocation) → Consistent trust.

### 4. Revocation and Evolution
- **Revocation List**: Kernel queries CRL (Certificate Revocation List) via yield.
- **Version Pinning**: Plans can pin attested versions; GFM warns on deprecations.

Attestation secures the ecosystem: Signed trust for every yield, reentrant and verifiable.

Next: Intent Sanitization in 012.