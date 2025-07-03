# Global Function Mesh

**Status:** Outline – v0.1 (placeholder)

---

## Purpose

Provide a universal, decentralized naming and discovery system for functions and capabilities across the CCOS ecosystem. Think of it as **DNS for functions**.

---

## Key Responsibilities (MVP)

1. **Universal Identifiers** – Map a canonical name like `image-processing/sharpen` to one or more providers.
2. **Decentralized Registry** – Pluggable back-end (Git repo, IPFS, blockchain, etc.)
3. **Versioning & Namespaces** – Allow multiple versions and vendor namespaces to coexist.
4. **Provider Metadata Stub** – Minimal pointer to Capability Marketplace listing (SLA lives there).

---

## Data Model (draft)

```rtfs
{:type :ccos.mesh:v0.func-record,
 :func-name "image-processing/sharpen",
 :latest-version "1.2.0",
 :providers [
   {:id "provider-123",
    :capability-ref "marketplace://offer/abc"},
   {:id "provider-456",
    :capability-ref "marketplace://offer/def"}
 ]}
```

---

## Open Questions

- Governance of name collisions?
- Recommended discovery transport (libp2p? https API?)
- Caching & TTL semantics.

---

## Roadmap Alignment

Phase 9 in `RTFS_MIGRATION_PLAN.md` – **Global Function Mesh V1**.

---

_This is a stub file – contributions welcome._
