# RTFS 2.0 Incoming Spec: Capability Contracts

Status: Proposed (specs-incoming)  
Audience: Capability authors, RTFS compiler/runtime, Governance Kernel, Orchestrator, Marketplace  
Related: specs-incoming/07-effect-system.md, specs-incoming/08-concurrency-and-determinism.md, docs/rtfs-2.0/specs/01-language-features.md, 05-native-type-system.md, docs/ccos/specs/004-capabilities-and-marketplace.md, 002-plans-and-orchestration.md

## 1. Overview

A Capability Contract is a typed, attested interface that describes a capability’s inputs, outputs, effects, resource bounds, determinism/idempotency properties, and versioning semantics. Contracts are the source of truth the compiler and Governance Kernel rely on to validate calls and the Orchestrator relies on to enforce least privilege and runtime safety.

Goals
- Provide a precise, machine-validated interface for capabilities discoverable in the Marketplace.
- Enable compile-time/type-level validation of call sites and result handling in RTFS.
- Bind capability privileges (effects/resources) for GK admission and ORCH enforcement.
- Support versioning, compatibility guarantees, and deprecation flows.
- Integrate security attestation (signatures, SBOM, provenance, revocation).

Non-Goals
- Define transport protocols or wire formats (handled by specific providers/runtimes).
- Replace organization policy (GK) or runtime isolation (ORCH) — this spec feeds both.

## 2. Contract Schema (Conceptual)

A contract document (stored in Marketplace and embedded as metadata in builds) has the following conceptual fields:

- :capability-id keyword (namespaced), e.g., :com.vendor.billing:v2
- :version semver string, e.g., "2.1.0"
- :input RTFS type schema (structural, supports refinements)
- :output RTFS type schema
- :effects effect row (see 07-effect-system)
- :resources resource constraints (complementary to effects)
- :determinism {:deterministic? bool :modes [:seeded|:best_effort] :requirements {...}}
- :idempotency {:supports bool :key_schema RTFS type :scope [:plan|:intent|:global]}
- :errors vector of typed error variants with shapes and codes
- :security {:signing {:sigstore {...}|:tuf {...}} :sbom uri/hash :provenance slsa/attestation}
- :reputation {:sla {:p95_ms int :availability "99.9%"} :history ref (Causal Chain anchor)}
- :deprecation {:replaced_by :capability-id :sunset_at iso-datetime}
- :notes documentation links and human-readable hints

Contracts MAY be represented as RTFS data or a companion JSON/EDN/Protobuf document; the Marketplace provides canonical storage plus hashes.

## 3. RTFS Type Schemas

Use the RTFS native type system (05-native-type-system.md):
- Base types: string, number, boolean, null, keyword, vector, map, union, optional
- Refinements: [:and number [:> 0]], [:matches-regex "..."], etc.
- Structural: maps with required keys; optional keys via [:optional ...]
- Example:
  {:amount [:and number [:> 0]]
   :currency [:enum "USD" "EUR"]
   :meta [:optional {:note string :tags [:vector string]}]}

Compiler obligations
- Validate call-site inputs against :input schema at compile-time where known and at runtime otherwise.
- Validate returned values against :output schema at runtime; surface typed diagnostics.

## 4. Effects and Resources

Effects (see 07-effect-system.md)
- Contracts MUST declare the minimal effect row necessary to operate.
- Example: [[:network {:domains ["billing.vendor.com"] :methods [:POST]}]]

Resources
- Contracts MAY declare resource bounds or expectations:
  {:max_time_ms 15000 :token_budget 500000 :data_locality :eu_only}

Governance Kernel (GK)
- Uses contract-declared effects/resources for admission checks. Plans cannot exceed or broaden the contract’s declared effect privileges.

Orchestrator (ORCH)
- Maps effect/resource declarations to runtime sandbox/egress/DLP/enforcement.

## 5. Determinism and Idempotency

Determinism
- Fields:
  {:deterministic? bool
   :modes [:seeded|:best_effort]
   :requirements {:temperature 0 :seeded true :model_ver "local-7b-v3"}}
- If deterministic? is true only under conditions, document them in :requirements.

Idempotency
- Fields:
  {:supports true
   :key_schema {:idempotency_key string}
   :scope :plan}
- ORCH deduplicates using the provided key and scope; required by GK policy for certain effect classes (e.g., financial operations).

## 6. Errors and Typed Catch

Errors are typed to enable precise (try/catch) handling in RTFS:
- Example variants:
  [{:code :error/network :shape {:status [:and number [:>= 400]] :body string}}
   {:code :error/timeout :shape {:elapsed_ms number}}
   {:code :policy/forbidden :shape {:reason keyword}}]

Compiler
- Enables match/catch on :code values and validates shapes where possible.

GK/ORCH
- Map policy violations to :policy/* errors; ensure Causal Chain records structured error data.

## 7. Versioning and Compatibility (SemVer)

Semver compliance:
- Patch (x.y.z): bug fixes; schemas unchanged; no effect/resource broadening.
- Minor (x.y): backward-compatible additions:
  - Inputs: adding optional fields allowed; narrowing refinements NOT allowed (breaking).
  - Outputs: adding optional fields or union extensions allowed.
  - Effects/resources: MAY narrow (safer), MUST NOT broaden (privilege escalation → breaking).
- Major (x): breaking changes — input schema tightening; output structure changes; effect/resource broadening; error variant changes.

Marketplace
- Enforces semver rules on publish.
- Provides negotiation (preferred version range) for consumers (Arbiter/ORCH).
- Deprecation workflows with sunset and replacement.

## 8. Discovery and Negotiation

Discovery via Marketplace query:
- Filters: :category, :capability-id prefix, version ranges, effect kind, data locality, SLA, reputation.
- Arbiter composes plans picking versions satisfying both intent constraints and GK policy.
- GK may override selection based on policy (e.g., forbid non-EU providers).

## 9. Security and Attestation

- Signatures (Sigstore/TUF); SBOM links; SLSA provenance required for listing.
- Revocation lists maintained by Marketplace; ORCH refuses revoked capabilities.
- Optional runtime attestation for runners (enforced by ORCH); contract MAY link to required runtime attestation policy.

Causal Chain
- Publish contract digests (hash) into CC for calls to support reproducible replay and audit.

## 10. Testing and Conformance

- Contract Test Suite: provider supplies deterministic fixtures/golden tests and property-based tests.
- Marketplace CI runs conformance on publish; badges exposed in discovery.
- Optional “self-test” endpoint for liveness and minimum behavior.

## 11. Examples

A) Billing capability (HTTP)

```clojure
(capability :com.vendor.billing:v2.1.0
  {:input  {:amount [:and number [:> 0]]
            :currency [:enum "USD" "EUR"]
            :customer_id string
            :memo [:optional string]}
   :output {:invoice_id string
            :status [:enum "ok" "pending" "error"]
            :eta_ms [:optional number]}
   :effects  [[:network {:domains ["billing.vendor.com"] :methods [:POST]}]]
   :resources {:max_time_ms 15000 :data_locality :eu_only}
   :determinism {:deterministic? true
                 :modes [:seeded]
                 :requirements {:seeded true}}
   :idempotency {:supports true
                 :key_schema {:idempotency_key string}
                 :scope :plan}
   :errors [{:code :error/network :shape {:status number :body string}}
            {:code :error/timeout :shape {:elapsed_ms number}}
            {:code :policy/forbidden :shape {:reason keyword}}]
   :security {:signing {:sigstore {:certificate "pem" :bundle "dsse"}}
              :sbom "sha256:..."
              :provenance {:slsa_level 3 :builder "..."}}})
```

B) LLM synthesis (local deterministic mode)

```clojure
(capability :com.local-llm:v1.3.0
  {:input  {:docs [:vector string] :format [:enum :summary :press-release]}
   :output {:text string :tokens_used number}
   :effects  [[:llm {:models ["local-7b-v3"] :max_tokens 200000}]]
   :resources {:token_budget 200000 :max_time_ms 30000}
   :determinism {:deterministic? true
                 :modes [:seeded]
                 :requirements {:temperature 0 :seeded true :model_ver "local-7b-v3"}}
   :idempotency {:supports false}
   :errors [{:code :error/model :shape {:reason string}}
            {:code :error/quota :shape {:remaining number}}]
   :security {:signing {:sigstore {...}} :sbom "sha256:..."}})
```

Call site validation example

```clojure
(defn create-invoice
  ^{:effects [[:network {:domains ["billing.vendor.com"] :methods [:POST]}]]}
  [req]
  (call :com.vendor.billing:v2.1.0
        (merge req {:idempotency_key (hash req)})))
; Compiler checks input shape; GK validates effects/resources; ORCH enforces at runtime.
```

## 12. Compiler Obligations

- Import capability contracts and expose them to the type checker.
- Validate call-site :input statically where available; emit runtime checks otherwise.
- Validate :output dynamically; enable typed catch/match on contract-defined error variants.
- Integrate effect/resource rows into program-level effect/resource analysis.

## 13. Governance Kernel Obligations

- Admission checks on plan envelope against capability-declared effects/resources/determinism/idempotency.
- Enforce policy forbidding privilege broadening beyond contract.
- Require approvals for high-risk contracts (e.g., :fs :write outside /tmp).
- Record admitted contract digests and policy decisions in the Causal Chain.

## 14. Orchestrator Obligations

- Select sandbox profile and egress/DLP configuration from effects.
- Enforce resource ceilings (time, tokens, cost) and idempotency dedup.
- Record capability version, contract digest, inputs/outputs hashes, and seeds in the Causal Chain.
- Map contract error variants to runtime errors and retry/compensation policy.

## 15. Marketplace Obligations

- Verify attestation (signatures, SBOM, provenance); run conformance tests.
- Enforce semver compatibility rules (deny effect/resource broadening in non-major releases).
- Maintain revocation lists; expose reputation and SLA history.
- Provide discovery filters over effects/resources/determinism/data-locality.

## 16. Open Questions

- Optional effects: clean expression of conditional effects without proliferating variants.
- Cross-capability transactions: contract-level grouping with atomicity semantics?
- Multi-provider fallback: expressing equivalent contract families for Arbiter decisioning.

## 17. Acceptance Criteria

- Compiler loads and validates against capability contracts; integrates effect/resource rows.
- GK admission fails plans that exceed contract privileges or violate policy.
- ORCH enforces least privilege and records audit trail with contract digests.
- Marketplace rejects unsigned/unattested or semver-noncompliant updates and maintains revocations.

---
This is an incoming specification intended for review and iteration. Once stabilized, it should be merged into the core RTFS 2.0 specs and cross-referenced from the Effect System and Concurrency specs.
