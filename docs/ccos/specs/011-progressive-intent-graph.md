# CCOS Specification 011: Progressive Intent Graph & Semantic Relationship Inference

**Status: Not Yet Implemented** <!-- not-yet-impl -->
Status: Draft (Phase 0 — Spec Committed)
Owner: TBD
Last Updated: 2025-10-02

## 0. Executive Summary
We introduce a progressive, arbiter-managed Intent Graph that evolves as a user iteratively states, refines, or pivots goals. The system automatically infers semantic relationships between intents (refinement, decomposition, alternative, follow-up, exploratory, pivot) to reduce cognitive load and enable analytical + replay capabilities. Explicit user commands (e.g. manual branching) become optional overrides rather than mandatory controls.

Primary deliverables:
- Automatic intent relationship classifier (heuristics + optional LLM fallback).
- Iterative session example showcasing dynamic graph growth.
- Metadata + storage extensions enabling replay, audit, and structural analytics.

---
## 1. Goals & Non‑Goals
### Goals
- Build a dynamic Intent Graph without requiring explicit structural commands from the user.
- Persist intents, relationships, plans, execution outcomes for later inspection & deterministic replay (where possible).
- Provide semantic querying (e.g., “list alternatives”, “refinement chain”).
- Support exporting/importing graph snapshots with integrity verification.

### Non‑Goals (initial phases)
- Full multi-user collaborative editing (future extension).
- Advanced probabilistic plan ranking (only metadata hooks initially).
- Automatic pruning/garbage collection of obsolete branches (manual for now).

---
## 2. Relationship Taxonomy
Relationship | Meaning | Trigger Signals (Indicative) | Execution Semantics
-------------|---------|------------------------------|-------------------
refinement_of | Narrows or augments constraints while preserving core objective | High semantic similarity + added modifiers / constraints | **REPLACES**: Only the refined intent executes; original becomes historical context
decomposes | Breaks a goal into sub-goals collectively achieving parent | Enumerations, list formatting, task verbs | **DELEGATES**: Parent coordinates child execution
alternative_to | Presents different strategy for same objective | Contrast markers (instead, rather, alternative) | **CHOICE**: User selects which alternative to execute
follow_up | Sequential action after completion of another | Temporal progression markers (next, now that) | **SEQUENCE**: Executes after parent completion
exploratory_branch | Hypothetical / experimental investigation | Hedging verbs (explore, try out, experiment) | **OPTIONAL**: May execute based on user interest
pivot_new_root | Shifts to a new unrelated objective | Low similarity & change-intent phrases | **NEW ROOT**: Independent execution path

`deepening` is modeled as a refinement with `metadata.scope = subcomponent`.

## 2.1 Execution Semantics by Relationship Type

### Refinement Chain Execution
When intents form a `refinement_of` chain, **only the latest (most refined) intent executes**. Previous versions provide historical context but their plans are superseded:

```
Root Intent v1: "find restaurant" (superseded)
└── Refinement v2: "ask cuisine preference, then recommend" (EXECUTES)
```

**Rationale**: Refinements represent improved understanding. Executing both the simple and complex versions would be redundant and potentially conflicting.

### Decomposition Execution  
Parent intent coordinates execution of child sub-goals:

```
Parent: "plan party" (coordinates)
├── Child: "book venue"
├── Child: "order food" 
└── Child: "send invites"
```

### Alternative Execution
User selects which alternative path to execute:

```
Common Goal: "solve problem"
├── Alternative A: "use method X" (user choice)
└── Alternative B: "use method Y" (user choice)
```

---

## 3. Lifecycle Pipeline
1. User utterance captured.
2. Preprocessing: normalize text, extract quoted constraints.
3. Candidate Parents: select prior intents (recent window + root + focused intent).
4. Feature Extraction: embeddings, lexical diff, discourse markers, structural cues.
5. Classification (heuristics first; escalate to LLM if ambiguous).
6. Relationship Decision + (optional) sub-intent generation (decomposition).
7. Intent persistence (store + metadata + event emission).
8. Plan generation & execution (immediate or deferred).
9. Graph & UI update; telemetry logging.

---
## 4. Metadata Extensions (StorableIntent)
Field | Description
------|------------
relationship_kind | Enum string (see taxonomy)
base_intent_id | ID of parent/reference intent (if applicable)
goal_version | Monotonic integer for root refinements (higher = more recent, should execute)
delta_summary | Short natural language diff summary
scope | Optional scope indicator (“subcomponent”, etc.)
ambiguity_score | 0 (confident) .. 1 (high ambiguity)
rationale | Classifier rationale snippet
classifier_method | `heuristic_only` | `heuristic+llm`
ranked_candidates | JSON array of (intent_id, score)
extracted_deltas | JSON array of constraint fragments
replay_hash | Integrity hash of (plan + input context)

All stored as stringified or JSON-encoded values inside metadata map initially (backward compatible). Later: consider a typed struct.

---
## 5. Heuristic Classification Rules (Phase 1)
Priority order conflict resolution:
1. pivot_new_root (if similarity < 0.40 & no resolved pronoun link)
2. decomposition (clear list structure & enumerations)
3. alternative_to (contrast markers)
4. refinement_of (high similarity + additive constraints)
5. follow_up (temporal markers referencing completed intent)
6. exploratory_branch (hedging verbs)
7. deepening (refinement + explicit subcomponent noun overlap)

Similarity thresholds (initial):
- High similarity: cosine >= 0.80
- Low similarity pivot threshold: < 0.40

Ambiguity detection: top two candidate scores difference < 0.10.

---
## 6. LLM Fallback Prompt (Outline)
Few-shot JSON classification restricting output to:
```
{"relationship": "refinement_of", "base_intent_id": "intent-123", "rationale": "..."}
```
Heuristics preface inserted:
```
Heuristics suggest candidates: refinement_of (0.78), alternative_to (0.72). Decide.
```
Fallback only when ambiguity_score > 0.5 and arbitration allowed.

---
## 7. Snapshot & Replay
Snapshot includes:
- intents[] (serialized storable intents + metadata)
- edges[] (derived from parent/base references + relationship_kind)
- plans[] (serialized plan archive entries)
- execution_results[] (subset of causal chain or separate structure)
- integrity: Merkle root hash of ordered items

Replay modes:
- structure: load graph only
- simulate: re-drive plans stubbing impure capabilities
- full: re-run plans (warn nondeterminism)

---
## 8. Telemetry & Metrics
Metric | Purpose
-------|--------
relationship_inference_latency_ms | Performance profiling
heuristic_vs_llm_ratio | Cost control
ambiguity_rate | Classifier quality baseline
user_override_rate | UX & accuracy indicator
misclassification_flags | QA feedback collection

---
## 9. Overrides & Corrections
User command (future):
```
/reclass <intent_id> relationship=alternative_to parent=<other_id>
```
Effect:
- Update metadata
- Emit IntentRelationshipModified event
- Optionally recompute affected subtrees

---
## 10. Governance & Constraints
Policy Examples:
- Max decomposition breadth per intent (e.g. 12 children).
- Max refinement depth (version cap).
- Disallow pivot_new_root unless session policy allows multi-root.
- Require confirmation if ambiguity_score > 0.8 before storing.

---
## 11. Risks & Mitigations
Risk | Mitigation
-----|-----------
Early misclassification cascades | Override + subtree re-evaluation tool
High LLM latency | Heuristic-first; only escalate on ambiguity
Graph bloat from decomposition | Governance cap + user confirm
Nondeterministic replay | Capability purity tagging + simulation mode

---
## 12. Phased Roadmap & Tasks
### Phase 1: Foundations / Example
- (T3) Progressive example (diff-based new intent discovery)
- (T10) Spec file (this document)
- Extend no code yet for classifier internals

### Phase 2: Core Classification
- (T6) Heuristic feature extraction & rule engine
- Add metadata population in intent creation path (arbiter hook)

### Phase 3: LLM Backstop & Decomposition
- LLM fallback integration (config gate)
- Enumeration parser -> automatic child intents

### Phase 4: Visualization & Query
- Query API helpers (refinements, alternatives, chain)
- Enhanced ASCII/TUI graph with color-coded relationships
- Snapshot export (structure + plans)

### Phase 5: Replay & Metrics
- Replay harness (structure/simulate/full)
- Metrics instrumentation events
- Override command / correction flow

### Phase 6: Hardening & Governance
- Policy enforcement (breadth/depth caps)
- Integrity hashing & audit diff tool
- Documentation + final example polish

---
## 13. Task Tracker Mapping (IDs reference project todo list)
ID | Title | Phase | Status
---|-------|-------|-------
3 | Implement new progressive example | 1 | pending
4 | Summarize approach & next steps | 1 | pending
5 | Design semantic relation inference | 2 | done (spec) 
6 | Prototype relation classifier | 2 | pending
7 | Integrate classifier into arbiter | 2 | pending
8 | Evaluation & metrics | 5 | pending
9 | Fallback & override UX | 5/6 | pending
10 | Persist spec file | 1 | in-progress

---
## 14. Open Questions
- Should decomposition children inherit *all* constraints immediately or selectively? (Default: inherit all.)
- Represent alternatives as siblings sharing same parent or linked via alternative_to edges to original? (Current: siblings, plus metadata.)
- Multi-root sessions default policy? (Allow, mark each root with `session_root=true`.)

---
## 15. Acceptance Criteria (Initial Pilot)
- Progressive example can create >= 5 linked intents automatically classified (heuristic-only) with >= 70% manual agreement.
- Snapshot export includes all referenced plan IDs and statuses.
- Reclassification command updates edge & emits audit event.

---
## 16. Next Immediate Action
Implement Phase 1 progressive example (`user_interaction_progressive_graph.rs`), capturing per-iteration graph diff and preparing placeholder metadata fields.

---
## 17. Appendix: Classification Output Schema (Draft)
```json
{
  "relationship_kind": "refinement_of",
  "base_intent_id": "intent-123",
  "confidence": 0.83,
  "ambiguity_score": 0.17,
  "rationale": "High overlap; new constraint 'use streaming cache'",
  "classifier_method": "heuristic_only",
  "ranked_candidates": [
    {"intent_id": "intent-123", "score": 0.83},
    {"intent_id": "intent-045", "score": 0.62}
  ],
  "extracted_deltas": ["add streaming cache"],
  "timestamp": 1759459200
}
```
