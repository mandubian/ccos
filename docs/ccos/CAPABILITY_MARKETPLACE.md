# Capability Marketplace

**Status:** Outline – v0.1 (placeholder)

---

## Purpose

Serve as the economic layer on top of the Global Function Mesh, allowing providers to publish **offers** with detailed SLA metadata and letting arbiters/agents broker the best deal for a task.

---

## Core Concepts

| Concept          | Description                                                                                                                                     |
| ---------------- | ----------------------------------------------------------------------------------------------------------------------------------------------- |
| **Offer**        | A structured RTFS object advertising a capability implementation, including cost, speed, confidence metrics, ethical alignment, and provenance. |
| **SLA Metadata** | Rich metadata the Arbiter can use to select the best provider under constraints (e.g., `:max-cost 0.01`, `:data-locality :EU-only`).            |
| **Broker**       | Logic (often the Arbiter) that compares offers and chooses the optimal provider.                                                                |
| **Reputation**   | Historical performance stats attached to offers or providers, recorded in Causal Chain.                                                         |

---

## Offer Object (draft schema)

```rtfs
{:type :ccos.marketplace:v0.offer,
 :capability "image-processing/sharpen",
 :version "1.2.0",
 :provider-id "agent-xyz",
 :cost-per-call 0.0003,
 :latency-ms 15,
 :confidence 0.98,
 :ethical-alignment :green,
 :data-provenance :verified,
 :signature "sha256:..."}
```

---

## Interaction Flow (happy path)

1. Provider registers an **Offer** → Marketplace.
2. Function name automatically appears in the **Global Function Mesh** with a pointer back to the Offer.
3. Arbiter needs `(image-processing/sharpen)` → queries Mesh → receives list of Offer refs.
4. Arbiter downloads Offer objects, filters by constraints in Intent / Task Context.
5. Winning provider receives RTFS task payload, executes, returns result + signature.
6. Result and SLA metrics recorded in **Causal Chain** for auditing.

---

## Roadmap Alignment

Phase 9 – "Capability marketplace integration" checkbox.

---

_Stub – expand with detailed APIs and settlement mechanisms._
