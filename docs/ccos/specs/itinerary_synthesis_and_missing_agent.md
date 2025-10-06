# Itinerary Planning Synthesis & Missing-Agent Handling

Status: Draft
Author: Automated assistant
Date: 2025-10-07
Scope: Defines conventions and strategy for turning interactive itinerary refinement sessions into reusable RTFS capabilities while robustly handling references to external planning agents that may not yet exist.

---
## 1. Goals
- Preserve user-collected preference semantics across turns.
- Generate higher-quality synthesized capabilities (collector, planner, stub) from dialogue traces.
- Prevent premature termination when a plan designates a `:next/agent` that is unavailable.
- Standardize status taxonomy for itinerary planning flows.
- Provide graceful fallback paths, persistence, and resumption when missing agents are referenced.

## 2. Status Taxonomy (Proposed)
| Status | Meaning | Terminal? | Next Action |
|--------|---------|-----------|------------|
| collecting_info | Asking questions; not all required params known | No | Continue interrogation or synthesis preview |
| ready_for_planning | All required user inputs gathered; internal planner available | No (unless agent resolved) | Invoke planner OR escalate if external agent missing |
| requires_agent | External `:next/agent` not found | No | Offer fallback: stub, wait, generic plan, ask user |
| agent_unavailable_retry | Deferred; waiting for registration | No | Schedule recheck or subscription |
| itinerary_generating | Long-running build in progress | No | Poll / stream |
| itinerary_ready | Itinerary assembled successfully | Yes | Emit final result & synthesize capability |
| refinement_exhausted | No further refinement + final result emitted | Yes | Wrap up |

> Only `itinerary_ready` and `refinement_exhausted` are always terminal.
> `ready_for_planning` becomes **non-terminal if** an unresolved `:next/agent` is present.

## 3. Interaction Decision Tree (High-Level)
```
Result Map ->
  if status in {itinerary_ready, refinement_exhausted}: end
  else if next/agent present:
      if agent exists: auto invoke (or prompt user)
      else: emit requires_agent contract (no termination)
  else if missing required params: continue collecting_info
  else: choose synthesis mode (collector vs planner vs stub)
```

## 4. Handling Unknown External Agents
When a plan returns:
```
{:status "ready_for_planning" :next/agent "itinerary.builder.v1" ...context}
```
and registry lookup fails:
1. Transform status → `requires_agent`.
2. Emit a structured response contract:
```clojure
{:status "requires_agent"
 :next/agent "itinerary.builder.v1"
 :context {...}
 :capability/requirements ["itinerary.build" "reasoning.multi_step"]
 :fallback/options ["synthesize_stub" "generic_builder" "ask_user" "abort"]}
```
3. Create a persistence record (PendingExecution).
4. Offer fallback menu or auto-policy.
5. On later capability registration, resume with stored context.

## 5. Persistence Model (Conceptual)
```clojure
(pending-external
  :id "pending-itinerary-builder-<uuid>"
  :created "2025-10-07T12:34:00Z"
  :next/agent "itinerary.builder.v1"
  :context { ... normalized parameters ... }
  :requirements ["itinerary.build"]
  :policy {:fallback "generic_builder" :ttl "P2D"})
```
- TTL after which record is pruned.
- Policy may direct automatic fallback after expiration.

## 6. Synthesis Modes
| Mode | Artifact | Use Case | Pros | Cons |
|------|----------|----------|------|------|
| Generic Planner | Single high-level capability | Quick reusable block | Simple | Loses interrogation detail |
| Interactive Collector + Planner | Split: `collector` + `planner` | Structured multi-turn reuse | Composable, testable | Two artifacts |
| Stub (Missing Agent) | Scaffolding w/ TODOs | Future implementation placeholder | Keeps flow moving | Risk of stub clutter |
| Delta / Incremental | Diff from base template | Versioned improvements | Traceability | Needs baseline mgmt |
| Summary Only | Non-executable schema | Early checkpoint | Low risk | Not directly runnable |

### Recommendation
Adopt Dual (Collector + Planner) as default once sufficient parameters collected; else Summary Only early, upgrade later.

## 7. Parameter Extraction & Normalization Pipeline
1. Collect all `:ccos.user.ask` prompts & answers.
2. Map prompt → canonical param name using heuristics / regex (e.g. `walking tolerance` → `walking_tolerance`).
3. Detect enumerations: split prompt segments with `/` → candidate enum values.
4. Build schema object:
```clojure
{:walking_tolerance {:type :enum :values ["low" "medium" "high"] :source_turn 1 :required true}
 :art_preference {:type :enum :values ["classical" "modern" "contemporary"]}
 ...}
```
5. Mark required vs optional based on downstream usage in plan bodies.
6. Record provenance (first_turn, last_turn, questions_asked_count).

## 8. Variable / Reference Conventions
Current inconsistency: use of keywords in maps (`:trip/destination`) vs `$destination` in steps.
Options:
- (A) Document `$param` as alias to parameter binding symbol.
- (B) Replace `$param` with `destination` and rely on lexical binding.
- (C) Use keyword-based fetch: `(search.flights :destination (:destination $preferences))`.
Recommendation: Adopt (A) short-term for brevity; add linter to flag `$` symbols not declared in `:parameters`.

## 9. Stub Capability Template
```clojure
(capability "itinerary.builder.v1"
  :description "Build a structured multi-day itinerary from normalized preference context"
  :parameters {:context "map"}
  :expects {:context/keys [:destination :duration :art_preference :museum_priority :walking_tolerance]}
  :steps (do
    (validate.context $context)
    ;; TODO: integrate domain search capabilities
    (assemble.itinerary $context)
    {:status "itinerary_ready" :itinerary (plan.compute $context)})
)
```
Add header comment: `; AUTO-GENERATED STUB - DO NOT SHIP WITHOUT IMPLEMENTATION`

## 10. Fallback Strategies (Tiered)
| Tier | Strategy | Trigger | Outcome |
|------|----------|---------|---------|
| 1 | Passive Wait | User opts to wait | Pending record stored |
| 2 | Stub Generate | Agent missing & user accepts scaffold | Stub file created |
| 3 | Generic Replan | Stub rejected or time-sensitive | Simplified itinerary built |
| 4 | Proxy Planner | Generic planning agent available | Delegation rerouted |
| 5 | Abort | User chooses stop | Session gracefully closed |

## 11. Evaluation Metrics
After synthesis, compute:
- Coverage = collected_params / total_detected_questions.
- Redundancy = duplicate_prompts / total_prompts.
- Enum Specificity = enum_params / total_params.
- Missing Required = |required - provided|.
- Latency to readiness (turn count until `ready_for_planning`).

Expose as:
```clojure
(synthesis.metrics
  :coverage 0.82
  :redundancy 0.05
  :enum_specificity 0.6
  :missing_required [:dates])
```

## 12. When to Synthesize Which Artifact
| Condition | Artifact |
|-----------|----------|
| <4 distinct params | Summary Only |
| ≥4 params, missing critical (dates) | Collector Only |
| All required collected, no external agent needed | Collector + Planner |
| External agent missing | Stub + Collector + Pending Record |
| Terminal (itinerary_ready) without planner | Planner (retrofit) |

## 13. Auto-Invoke vs. User Prompt
Decision matrix:
| Context | Action |
|---------|--------|
| Agent exists & trusted | Auto invoke silently |
| Agent exists & untrusted / new | Prompt w/ confirmation |
| Agent missing | Branch to fallback (Section 10) |

## 14. Security & Governance Considerations
- Log every auto-invocation with {origin_plan_id, agent_id, context_hash}.
- Prevent silent invocation of newly registered agents until governance trust threshold met.
- Hash context map for idempotent replays.
- Redact PII in persisted pending records.

## 15. Implementation Roadmap (Phased)
1. (MVP) Adjust loop: treat `ready_for_planning` as non-terminal if unresolved `next/agent`.
2. Add `requires_agent` emission & contract.
3. Parameter extraction + schema builder utility.
4. Dual artifact synthesis (collector + planner).
5. Stub generator + persistence layer for pending executions.
6. Metrics emission & linter for `$param`.
7. Governance gating for auto-invocation.

## 16. Open Questions
- Should collected parameters be namespaced (`:itinerary/walking_tolerance`) uniformly? (Consistency vs verbosity)
- Should we version synthesized capabilities (e.g., `.v1`, `.v2`) automatically on re-synthesis? (Likely yes, with semantic diffing.)
- Where to persist pending executions (embedded KV, file ledger, or causal chain extension)?
- Do we allow partial itinerary output before final readiness? (Maybe via streaming status updates.)

## 17. Future Enhancements
- Semantic deduplication of overlapping prompts using embedding similarity.
- Confidence scoring for each parameter (exposed for downstream risk-aware planning).
- Adaptive questioning (skip low-impact params when user shows fatigue).
- Re-synthesis diff mode (emit only changed parameter set or steps).
- Capability marketplace quality scoring loop (usage success metrics feed back into synthesis ranking).

## 18. Summary
This design decouples interactive preference acquisition from itinerary generation, provides a structured fallback when requested planning agents are absent, and produces higher-fidelity reusable capabilities. By formalizing statuses, fallback strategies, and artifact types, we reduce brittleness and pave the way for governance-aware auto-invocation and richer synthesis metrics.

---
## 19. Quick Reference (Cheat Sheet)
```
Statuses: collecting_info → ready_for_planning → (requires_agent?) → itinerary_generating → itinerary_ready
Artifacts: summary | collector | planner | stub | collector+planner
Fallback: wait | stub | generic | proxy | abort
Key Map Keys: :status :next/agent :context :capability/requirements :fallback/options
```

End of document.
