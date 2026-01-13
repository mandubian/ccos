# CCOS Specification 026: Generic Planner Synthesis and Agent Learning

**Status: Not Yet Implemented** <!-- not-yet-impl -->
Status: Draft (breaking changes allowed)
Owner: CCOS Core
Last updated: 2025-10-11
Scope: Replace prior doc with a practical, staged plan to synthesize a generic Planner and (when warranted) executable Agents from interaction history. This doc is implementation-first and may break existing APIs.

---
## Why this rewrite

The existing generator’s planner was hardcoded to `ccos.<domain>.fetch-events` and didn’t adapt to collected inputs, the registry, or security. We’re rebuilding the feature to be:
- Registry-first: reuse existing capabilities/agents whenever possible
- Adaptive: map collected context to candidate inputs via light adapters
- Safe fallback: emit and register a “requires-agent” stub with clear needs
- Auditable and deterministic: produce explainable decisions and valid RTFS
- Security-aware: never propose calls disallowed by the current RuntimeContext

We do not preserve backward compatibility; we will change function names, inputs, and outputs as needed.

---
## North-star outcomes
- Given interaction history and a parameter schema, return an RTFS planner that:
  - Calls an existing, allowed capability/agent if a good match exists
  - Otherwise, emits a stub capability and clearly signals “requires-agent”
- Optionally synthesize an Agent (RTFS plan) when orchestration complexity is detected
- Produce telemetry to explain decisions, missing inputs, and next steps

---
## Core contracts (new)

We introduce new types in `ccos::synthesis` (names may change during implementation):

```rust
/// Inputs for planner generation.
pub struct PlannerGenContext {
    pub schema: ParamSchema,                 // from schema_builder
    pub history: Vec<InteractionTurn>,      // from causal chain
    pub skills: Vec<ExtractedSkill>,        // from skill_extractor
    pub security: RuntimeContext,           // allowed capabilities
    pub registry_caps: Vec<CapabilityCard>, // snapshot of marketplace
    pub registry_agents: Vec<AgentCard>,    // snapshot of agent registry
    pub preferences: PlannerPreferences,    // speed vs completeness, locality, etc
}

pub struct PlannerPreferences {
    pub prefer_local: bool,
    pub prefer_trusted: bool,
    pub optimize_for_speed: bool,
}

pub struct CapabilityCard {
    pub id: String,
    pub expects_context_keys: Vec<String>,
    pub trust_tier: String,
    pub local: bool,
    pub doc: Option<String>,
}

pub struct AgentCard {
    pub agent_id: String,
    pub skills: Vec<String>,
    pub supported_context_keys: Vec<String>,
    pub trust_tier: String,
}

/// Planner output and explainability.
pub struct PlannerSynthesis {
    pub rtfs: String,                    // planner body
    pub required_capabilities: Vec<String>,
    pub decision_trace: PlannerDecisionTrace,
}

pub struct PlannerDecisionTrace {
    pub candidates: Vec<CandidateScore>,
    pub selected: Option<CandidateScore>,
    pub adapters: Vec<AdapterStep>,
    pub fallback: Option<FallbackInfo>,
}

pub struct CandidateScore {
    pub id: String,             // capability or agent id
    pub kind: String,           // "capability" | "agent"
    pub coverage: f32,          // context key coverage ratio
    pub compatibility: f32,     // type/coercion feasibility
    pub trust_bias: f32,        // trusted/local bonus
    pub total: f32,
}

pub struct AdapterStep {
    pub from: String,           // context key
    pub to: String,             // candidate key
    pub coercion: Option<String>,// e.g., string->int, parse-date
    pub default_used: bool,
}

pub struct FallbackInfo {
    pub stub_id: String,
    pub missing_required: Vec<String>,
    pub reason: String,
}
```

New API surface:
- `generate_planner_generic(ctx: &PlannerGenContext) -> PlannerSynthesis`
- `discover_registry_snapshots(ccos: &CCOS) -> (Vec<CapabilityCard>, Vec<AgentCard>)`
- `score_candidates(...) -> Vec<CandidateScore>`
- `emit_planner_rtfs_direct(...)` and `emit_planner_rtfs_adapter(...)` and `emit_planner_rtfs_requires_agent(...)`

---
## Matching and adaptation

Candidate discovery (registry-first):
- Filter by security: only capabilities allowed by `ctx.security`
- Capability candidates: rank by overlap between `schema.required_keys()` and `expects_context_keys`
- Agent candidates: rank by skills overlap and supported context keys

Scoring (weights configurable later):
- coverage (0.45): required key coverage
- compatibility (0.35): can we adapt types with simple coercions?
- trust_bias (0.20): local + trusted preferred

Adaptation (v0.2):
- Key renames (synonyms and skill-derived hints)
- Simple coercions (string<->int/float/bool; parse date/time)
- Defaults for optional keys; if required still missing → fallback

---
## Planner RTFS templates

Direct call (perfect/near-perfect match):
```clojure
(do
  (let [; optionally: adapted-context context
        result (call :<candidate.id> {:context context})]
    {:status "processing"
     :result result
     :context context}))
```

Adapter + call (light mapping):
```clojure
(do
  (let [context-adapted (let [c context]
                           {; renames / coercions applied here
                            :k2 (:k1 c)
                            :n  (parse-int (:s c))})
        missing (filter required-key? (keys-not-in context-adapted))]
    (if (empty? missing)
      (let [result (call :<candidate.id> {:context context-adapted})]
        {:status "processing" :result result :context context-adapted})
      {:status "requires-agent"
       :missing missing
       :context context})))
```

Requires-agent fallback (no viable candidate):
```clojure
(do
  {:status "requires-agent"
   :required_capability "synth.domain.agent.stub"
   :missing <required-keys-missing>
   :context context})
```

Note: We will keep `:expects` accurate to the keys used in each template.

---
## When to synthesize an Agent (RTFS plan)

Trigger (configurable):
- >3 turns OR >2 refinements OR multi-step orchestration keywords ("then", "after", sequencing) → switch to Agent synthesis.

Agent generation path:
- Use interaction history + skills to build a multi-step RTFS plan
- Create and register an `AgentDescriptor::RTFS { plan }`
- Planner can then delegate to this newly synthesized agent

This replaces and generalizes the prior doc’s agent synthesis guidance.

---
## Security & policy
- Always enforce `RuntimeContext::is_capability_allowed` during discovery
- Prefer local and trusted capabilities unless preferences say otherwise
- Planner never executes during synthesis; runtime preflight still validates

---
## Telemetry & metrics
- Emit a `planner_generation` event with:
  - candidates + scores, selected candidate
  - adapters used, fallback reasons
  - coverage metrics of provided vs. required parameters
- Feed back execution success into agent/capability scoring (future work)

---
## Milestones (breaking work)

v0.1 Registry-first + stub (target: small PR)
- [ ] Add snapshot APIs for registry and marketplace (read-only)
- [ ] Implement `generate_planner_generic` registry-first path
- [ ] Emit direct call template when perfect match; else stub planner
- [ ] Telemetry event + metrics

v0.2 Adapters & ranking
- [ ] Introduce key-rename/coercion/default adapters and scoring weights
- [ ] Emit adapter+call planner when feasible
- [ ] Expand telemetry with adapter decisions

v0.3 Optional cognitive engine assist
- [ ] If delegating cognitive engine configured, ask it for a proposal with structured context
- [ ] Validate returned plan; fallback on validation failure

v0.4 Agent synthesis integration
- [ ] Trigger agent generation on complex interactions
- [ ] Register agent and route planner to it
- [ ] Feedback loop to improve agent trust-tier over time

---
## Testing strategy
- Unit tests for scoring and adapter selection (deterministic inputs)
- Golden RTFS tests for each template
- Integration test: end-to-end from two-turn collector → generic planner → preflight validate
- Security tests ensuring disallowed caps are never selected

---
## Risks & open questions
- How to extract reliable capability `:expects` when RTFS defs aren’t standardized?
- Balancing adapter complexity vs. correctness; keep v0.2 small
- When to auto-register stubs vs. only suggest (start: only suggest; optional register behind a flag)
- LLM proposal validation boundaries (strict parse + capability preflight)

---
## Quick start for devs
- To produce conversation data quickly, run the two-turn demo:
  - `CCOS_INTERACTIVE_ASK=1 cargo run --example user_interaction_two_turns`
- Feed the resulting `InteractionTurn`s + registry snapshot into `generate_planner_generic`
- Inspect `PlannerSynthesis` for RTFS and decision trace

---
## Definition of Done per milestone
- v0.1: Direct-call or stub planner generated deterministically; security respected; tests passing
- v0.2: Adapters applied where safe; higher match rates; expanded telemetry
- v0.3: LLM assist behind a feature flag; robust validation; safe fallback
- v0.4: Agent synthesis wired; planner delegates to agent; registry updated

End of document.
