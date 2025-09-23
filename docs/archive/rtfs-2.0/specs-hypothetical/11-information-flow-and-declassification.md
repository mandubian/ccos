# RTFS 2.0 Incoming Spec: Information Flow, Labeling, and Declassification

Status: Proposed (specs-incoming)  
Audience: RTFS compiler/runtime, Governance Kernel, Orchestrator, Working Memory/Context Horizon  
Related: specs-incoming/07-effect-system.md, 08-concurrency-and-determinism.md, 09-capability-contracts.md, 10-contracts-and-compensations.md, docs/ccos/specs/009-context-horizon.md, 013-working-memory.md

## 1. Overview

This document specifies Information Flow Control (IFC) for RTFS/CCOS, introducing data labels, propagation rules, locality/privacy enforcement, and a governed declassification mechanism. The goal is end-to-end data provenance and policy enforcement from inputs to outputs with auditability in the Causal Chain.

Goals
- Label data with classifications (PII, confidential, eu_only, export_restricted, etc.) at ingestion and track taint through program execution.
- Enforce data-locality/sovereignty and privacy constraints at compile-time (shape) and runtime (policy).
- Provide an explicit, policy-gated declassification special form with full audit and rationale.
- Persist provenance/lineage for artifacts to enable post-hoc proof of compliance.

Non-Goals
- Cryptographic data protection (encryption at rest/in transit is orthogonal).
- Replacing access control/permissions; IFC complements IAM and effect/resource policies.

## 2. Terminology

- Label: A classification tag (keyword + optional parameters) applied to data fragments.
- Taint: The set of labels propagated through computations that derive from labeled inputs.
- Declassification: A policy-approved conversion that removes or transforms labels prior to a boundary (e.g., external call or result return).
- Boundary: A point where data leaves the current trust domain (capability calls, storage outside tenant scope, UI, network).

## 3. Label Vocabulary (Initial Set)

Labels are namespaced keywords with optional parameters:
- :ifc/pii {:fields [:email :ssn :phone] :subject "user-id-123"}
- :ifc/confidential {:project "alpha"}
- :ifc/export_restricted {:jurisdiction :us_itAR}
- :ifc/eu_only {}
- :ifc/hipaa {:patient-id "..."}
- :ifc/gdpr {:subject-id "..."}
- :ifc/public {}

Notes
- :ifc/public is the absence of restrictions; derived values from public-only inputs remain public unless combined with restricted inputs.
- Labels MAY be extended by policy; GK maintains the authoritative registry per tenant/org.

## 4. Label Attachment and Metadata

Labels can be attached at ingestion or computed:

A) Ingestion (Working Memory/Context Horizon)
```clojure
(with-resource ^{:labels [:ifc/pii {:fields [:email]} :ifc/eu_only {}]} user-record
  ...)
```

B) Derived labels at call boundaries
- Capabilities may declare the labels of their outputs (e.g., OCR from ID docs → :ifc/pii).
- Contracts (09) include label semantics for inputs/outputs.

C) Manual label assertions (rare; policy-gated)
```clojure
^{:labels [:ifc/confidential {:project "alpha"}]} data-fragment
```

## 5. Propagation Rules

- Pure computations propagate labels by union:
  labels(f(x,y)) = labels(x) ∪ labels(y) ∪ labels(constants used)
- Conditional paths do not erase labels; if any branch depends on labeled data, the result carries those labels.
- Aggregations (map/reduce/filter) union labels across elements.
- Parallel branches propagate labels independently; join unions label sets.

Label Narrowing
- A pure transformation that provably removes a sensitive attribute (e.g., hashing email) MAY allow label refinement by rule:
  - Example policy rule: hash(email) removes :ifc/pii[:email] but not :ifc/gdpr subject linkage unless salted/pepper per policy.
- Narrowing requires GK-approved rules; compiler/runtime consult GK for allowed transformations.

## 6. Enforcement at Boundaries

Before a boundary (e.g., network egress, capability call, storage outside permissible scope), ORCH enforces:

- Locality enforcement
  - :ifc/eu_only data cannot flow to non-EU providers/capabilities; GK policy maps provider compliance metadata (Marketplace) to allowed regions.
- Export restrictions
  - :ifc/export_restricted blocks egress to disallowed jurisdictions or requires declassification.
- PII controls
  - :ifc/pii may require masking/redaction before leaving tenant boundary or specific channels.

If a boundary is attempted with violating labels, runtime MUST:
- Block the operation
- Emit a policy violation to the Causal Chain
- Suggest declassification/redaction strategies (if configured) or require approval

## 7. Declassification

Provide an explicit special form that transforms a labeled value into a less-restricted one under policy:

Syntax
```clojure
(declassify
  ^{:policy {:purpose "send_notification"
             :rationale "only city + first_name allowed"
             :approvals 1}}
  value
  {:strategy :redact
   :rules [{:remove [:email :phone]}
           {:truncate [:address]  :to  "city"]
           {:mask     [:name]     :with :first_name}]})
```

Semantics
- declassify takes a value, a strategy/rules map, and optional policy hints.
- GK validates whether declassification is allowed for requested labels, purpose, and rules.
- ORCH applies transformation (pure), re-computes labels, and verifies the target boundary policy is satisfied.
- CC logs DeclassificationRequested/Approved/Applied with hashes of pre/post values, labels, policy references, and approver identity (human or automated policy decision).

Failure Modes
- If GK denies declassification, the operation fails with :ifc/declassification-denied; ORCH blocks downstream boundary.
- If transformation fails to meet policy (labels remain too restrictive), ORCH blocks boundary, logs failure with diagnostics.

## 8. Compiler and Type System Considerations

- The compiler need not fully model runtime labels; however, it can:
  - Validate declassify syntax and shape.
  - Emit warnings when effectful boundaries are present without adjacent declassify and inputs are likely labeled (based on capability contracts and common patterns).
  - Allow refinement-style annotations that describe label expectations to aid Arbiter synthesis:
    ^{:expects-labels [:ifc/public]} or ^{:produces-labels [:ifc/pii]}

- Capability contracts (09) SHOULD include label semantics:
  - :input-labels-constraints (e.g., forbids :ifc/pii unless masked)
  - :output-labels (e.g., returns :ifc/pii due to OCR -> text)

## 9. Governance Kernel Integration

- Policy registry defining:
  - Label definitions and relationships (hierarchies, aliases).
  - Allowed declassification strategies per label/class and purpose (purpose limitation).
  - Approval requirements (risk tier → M-of-N approvals).
  - Locality mappings (provider region attestations).
- Admission-time checks:
  - Plans referencing declassify must cite purposes; GK validates that purposes align with intents and tenant policies.
  - High-risk label flows require human-in-the-loop approvals with signed artifacts bound to plan ID.

- Runtime decisions:
  - GK provides allow/deny decisions for declassification requests.
  - Records decisions and justifications in the Causal Chain.

## 10. Orchestrator and WM/CH Integration

- Label storage: WM/CH must attach and persist labels alongside data artifacts; provenance edges recorded for derivations.
- Boundary enforcement:
  - Egress proxy consults labels to allow/deny and to apply DLP/redaction.
  - Storage backends enforce location/tenant boundaries; attempts to store labeled data in disallowed locations are blocked.
- Provenance:
  - ORCH logs per-step label summaries and any declassification to the Causal Chain with content hashes.

## 11. Examples

A) EU-only data with allowed egress after declassification
```clojure
(let [report ^{:labels [:ifc/eu_only {} :ifc/confidential {:project "alpha"}]}
             (call :com.analytics.eu:v1.generate {:topic "phoenix"})]
  ; Remove confidential parts and aggregate stats only
  (let [public-report (declassify
                        ^{:policy {:purpose "external_publish" :approvals 2}}
                        report
                        {:strategy :redact
                         :rules [{:keep [:summary :metrics]}
                                 {:remove [:raw_records :attachments]}]})]
    (step "Publish"
      ^{:effects [[:network {:domains ["press.example.com"] :methods [:POST]}]]}
      (call :com.press:v1.publish {:doc public-report}))))
```

B) PII masking for notifications
```clojure
(let [user ^{:labels [:ifc/pii {:fields [:email :phone]}]}
           (with-resource user-record)]
  (let [msg (declassify
              ^{:policy {:purpose "notify_user"}}
              user
              {:strategy :mask
               :rules [{:mask [:email] :with :first_letter_plus_domain}
                       {:remove [:phone]}]})]
    (call :notify.sms-or-email {:to msg :template "welcome"})))
```

C) Denied declassification (policy)
- Attempting to declassify :ifc/export_restricted → :ifc/public for :purpose "external_publish" without required approvals returns :ifc/declassification-denied and blocks egress.

## 12. Open Questions

- Canonical rule language for declassification strategies (keep/remove/mask/truncate/hash): do we standardize minimal set or allow provider plugins?
- Automated labeling: to what extent can WM/CH infer labels via classifiers (PII detectors)? How are false positives handled?
- Label explosion control: strategies to minimize combinatorial growth of label sets (normalization, dominance rules).

## 13. Acceptance Criteria

Compiler
- Parses and validates declassify forms; provides warnings for suspicious boundary flows without declassification.
- Supports optional label expectation annotations to guide synthesis.

Governance Kernel
- Maintains label registry and declassification policies; provides admission/runtime decisions; records them in CC.
- Enforces purpose limitation and approval workflows for high-risk declassifications.

Orchestrator and WM/CH
- Persist labels with artifacts; propagate through pure transforms and across calls.
- Enforce boundaries (egress/storage/UI) based on labels; apply DLP/redaction as required.
- Emit provenance to Causal Chain for label flows and declassifications.

---
This is an incoming specification intended for review and iteration. Once stabilized, it should be merged into the core RTFS 2.0 specs and cross-referenced from Effect System, Concurrency, and Capability Contracts documents, as well as Working Memory and Context Horizon specs.
