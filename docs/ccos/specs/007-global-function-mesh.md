# CCOS Specification 007: Global Function Mesh (RTFS 2.0 Edition)

**Status:** Draft for Review  
**Version:** 1.0  
**Date:** 2025-09-20  
**Related:** [000: Architecture](./000-ccos-architecture-new.md), [004: Capabilities](./004-capabilities-and-marketplace-new.md), [008: Delegation Engine](./008-delegation-engine-new.md)  

## Introduction: The DNS for Capabilities

The Global Function Mesh (GFM) is CCOS's universal discovery layer for capabilities—acting as a 'DNS' that maps abstract RTFS yield names (e.g., `:image.sharpen`) to available providers in the Marketplace. In RTFS 2.0, GFM receives yield requests from the Orchestrator, queries for matches, and returns candidates for the Delegation Engine to select. This keeps plans portable: Write abstract yields; GFM handles resolution dynamically.

Why essential? Enables extensibility without hardcoding—RTFS stays pure, unaware of providers. Reentrancy: Consistent resolution on resume (e.g., same version pinned).

## Core Concepts

### 1. GFM Structure and Workflow
GFM is a queryable index over Marketplace manifests:
- **Query Input**: From yield `RequiresHost({:cap :image.sharpen, :version \"^1.0\", :constraints {:latency < 200ms}})`).
- **Resolution**: Search by name/version/schema; filter by availability (e.g., healthy providers).
- **Output**: List of provider candidates (e.g., AWS, Local, Cloudflare), with metadata (cost, SLA).

**Resolution Flow Diagram** (Integrated with Yield):
```mermaid
sequenceDiagram
    RTFS[RTFS Runtime] --> O[Orchestrator]
    RTFS->>O: Yield :image.sharpen (request)
    O->>GK[Governance Kernel]: Validate
    GK->>O: Approved
    O->>GFM: Query {:cap :image.sharpen :version ^1.0}
    GFM->>MP[Marketplace]: Search Manifests
    MP->>GFM: Candidates [AWS-v1.1, Local-v1.0]
    GFM->>DE[Delegation Engine]: Pass Candidates
    DE->>O: Selected AWS-v1.1
    O->>Provider: Execute
    Provider->>O: Result
    O->>RTFS: Resume
```

### 2. Key Features
- **Versioning and Compatibility**: Semantic matching (e.g., `^1.0` allows 1.x); schema validation ensures arg/output fit.
- **Dynamic Registration**: Providers publish to Marketplace; GFM subscribes for updates (e.g., new versions).
- **Fallbacks**: If no match, GFM yields error or suggests alternatives (e.g., `:image.blur` proxy).
- **Caching**: Resolved mappings cached per plan/context for reentrancy (resume uses same).

**Sample Yield Resolution** (in Plan):
```
(call :nlp.sentiment {:text \"Review text\" :lang :en})  ;; Abstract
```
- GFM Query: `:nlp.sentiment` → Candidates: OpenAI-v3 (cost 0.01), Local-v2 (cost 0).
- Ties to RTFS: Yield args validated against manifest schemas before execution.

### 3. Integration with RTFS 2.0 Reentrancy
- **Resume Handling**: Yield requests include `resume-id` (from chain); GFM re-resolves with same constraints to avoid drift.
- **Purity Preservation**: GFM is host-side; RTFS only sees the abstract symbol—no provider knowledge.

### 4. Security and Governance
Kernel intercepts GFM queries: Verify cap existence before Orchestrator proceeds. Logs resolutions to chain as `Action {:type :CapabilityResolution}`.

GFM makes CCOS's capabilities truly global: Abstract in plans, concrete at runtime, reentrant by design.

Next: Delegation Engine in 008.