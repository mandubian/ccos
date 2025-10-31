# Smart Assistant Demo Plan (CCOS/RTFS)

A deep, actionable plan to evolve the smart assistant demo to fully leverage dynamic MCP/OpenAPI capability generation, DelegatingArbiter with human-in-the-loop refinement, partial execution outcomes for iterative planning, and synthetic capability generation/registration—showcasing advantages RTFS/CCOS offers beyond LLM/MCP-only stacks.

---

## 1) Purpose and Differentiators

- Goal: Turn a natural-language goal into a governed Intent, refine it with a DelegatingArbiter, discover/create capabilities (MCP/OpenAPI) on the fly, execute safely with partial outcomes, and synthesize reusable capabilities.
- Why CCOS/RTFS vs LLM/MCP-only:
  - Governance & audit: explicit, enforceable policies; every decision recorded in a causal chain.
  - Intent graph & composability: plans are typed graphs with dependencies, not opaque prompts.
  - Deterministic partial execution outcomes: structured, verifiable state for re-planning and replay.
  - Synthetic capabilities with provenance: distill reusable building blocks with contracts and tests.
  - Secure stdlib & consent: fine-grained, capability-scoped permissions beyond generic tool calls.

---

## 2) User Journey (Happy Path)

1. User states a goal in natural language.
2. Assistant extracts a governed Intent (constraints, success criteria, resources).
3. DelegatingArbiter asks targeted questions to refine ambiguity and collect missing inputs.
4. Orchestrator proposes a minimal viable plan (intent graph) with ordered steps/dependencies.
5. Capability discovery loads candidates (MCP/OpenAPI + existing synthetic) and ranks them.
6. User approves scopes/permissions; execution proceeds stepwise with partial outcomes.
7. On failures/unknowns, assistant re-plans or asks clarifying questions; all decisions logged.
8. Success: assistant proposes synthesizing a new capability; shows its contract and test; registers it.
9. Next time: the same goal is solved faster via the synthesized capability.

---

## 2.5) Current Progress Snapshot

- ✅ Clarifying-question parser fails closed on malformed data and normalizes varied list/map shapes (`extract_question_items`, `strip_code_fences` tests green).
- ✅ Plan-step parser now strips Markdown/```clojure fences before RTFS parsing; covered by `strips_markdown_code_fence_blocks`.
- 🧪 Latest validation: `cargo test --example smart_assistant_demo` (all six tests pass).
- 🔄 Next immediate action: rerun the demo with the real `openrouter_free:balanced` profile to confirm fenced-plan handling end-to-end.
- 🔄 Next implementation focus: wire dynamic capability discovery + partial outcome loop per Sections 3, 6, and 7.
- 🚧 DelegatingArbiter still calls a stubbed planner; replace with live LLM invocation (`openrouter_free:balanced`) via the RTFS mediator and capture prompt/response artifacts in the ledger.

---

## 3) Architecture & Flow (End-to-End)

- Status: clarifier parsing hardens the DelegatingArbiter intake (malformed responses abort with rationale) and plan-step parsing now sanitizes fenced RTFS before handing off to the runtime.

- Intake → Intent
  - Input: natural-language goal, optional constraints/preferences.
  - Output: Intent { id, user_goal, constraints, acceptance_criteria, privacy_scope, budgets }.

- Delegation loop (DelegatingArbiter)
  - Inputs: Intent, plan gaps, policy context, capability inventory.
  - Outputs: Questions[], updated Intent, consent deltas, risk flags.
  - Constraints: bounded-turn, no redundant questions, propose minimal executable shard.
  - Question targeting: derive from `metadata.needs_capabilities[*].required_inputs` minus keys already present in Intent/Clarifications; ask only what unblocks the next runnable steps.

- Plan construction (Intent Graph)
  - Inputs: Intent, capabilities, dependency analysis.
  - Outputs: Plan { steps[], edges, pre/postconditions, expected evidence }.
  - Planner also emits plan metadata for missing effects (per 018):
    - metadata.needs_capabilities: [{ capability_id? | class?, required_inputs, expected_outputs, policies?, notes? }]
    - Prefer using capability classes early (e.g., travel.flights.search, lodging.hotels.search); concrete IDs can be bound later by discovery/selection.
  - ID hygiene: normalize/trim capability IDs (avoid trailing newlines like "v1\n"); dedupe needs by (capability_id || class).

- Capability discovery/selection
  - MCP servers and OpenAPI schemas feed CapabilityDescriptors.
  - Ranking by semantic fit, policy/consent fit, cost/time, trust/reliability, reuse bonus.
  - Resolver priority: consume plan.metadata.needs_capabilities first → scan call sites for unknown caps → convert preflight/governance failures into new needs (feeds the continuous resolution loop).

- Execution & Partial outcomes
  - Step executor produces PartialExecutionOutcome at well-defined boundaries.
  - Re-planning consumes outcome deltas; triggers new questions/consents when needed.

- Synthesis pipeline
  - From trace: infer stable parameter set, outputs, validations; generate tests & provenance.
  - Register SyntheticCapability with versioning and policy tags.

- Governance & audit
  - Arbiter/kernel checks: consent boundaries, scopes, data egress, risk.
  - Causal chain with deterministic replay markers (seeds, schema/version tags, fingerprints).

---

### 3.a needs_capabilities contract (planner + preflight)

To keep the system deterministic and aligned with 018/020, planners should populate `metadata.needs_capabilities`. The runtime and preflight enrich/merge this list.

Each entry contains:

- capability_id (optional): concrete symbol if known; normalized/trimmed.
- class (optional): taxonomy hint to guide discovery (e.g., `travel.flights.search`).
- required_inputs: [keyword] → union of call-arg keys, manifest input schema (if known), and step preconditions; the DelegatingArbiter asks only for those missing from Intent/Clarifications.
- expected_outputs: [keyword] → from manifest, downstream consumption, or planner hints.
- policies (optional): risk tier, cost/latency preferences, data residency.
- notes/candidates (optional): suggested providers or IDs.

Merge strategy:
- Source order of truth: planner-declared → static RTFS scan of calls → preflight/governance failures → runtime unknown yields.
- Deduplicate by (capability_id || class); keep provenance for audit.

---

## 4) Core Data Contracts (Sketch)

- Intent
  - id, user_goal (string), context, constraints (list/enums), acceptance_criteria (checklist), privacy_scope (hosts/paths), budgets (cost/time).

- Question
  - id, about (intent path), rationale, suggested_options, sensitivity_level, requires_explicit_consent.

- PlanStep
  - id, name, inputs (typed), expected_outputs (typed), capability_candidates [CapabilityDescriptor], pre/postconditions, cost/time estimate, risk score.

- CapabilityDescriptor
  - id, provider_type (MCP/OpenAPI/Synthetic), signature (params→returns), auth requirements, trust score, reliability metrics, last_verified.

- PartialExecutionOutcome
  - step_id, status (success/partial/fail/retryable/blocked), outputs (typed), evidence (artifacts/paths), logs digest, policy_flags, replannable_fields.

- SyntheticCapability
  - id, name, description, signature, examples, testcases, provenance (trace refs + versions), policy_tags, versioning info.

---

## 5) DelegatingArbiter Loop (Deep Dive)

- Purpose: minimize uncertainty via targeted high-signal questions; keep clear rationale tied to policy.
- Inputs: intent coverage map, plan feasibility gaps, risk triggers, governance advisories.
- Outputs: ranked questions, proposed intent updates with confidence, scope/consent deltas.
- Constraints: max turns per topic; stop on diminishing returns; always propose a minimal executable shard.
- Prompting: few-shot patterns mapping missing constraints to questions, aligned with CCOS policy scaffolding.

---

## 6) Dynamic Capability Discovery & Ranking

- Discovery feeders:
  - MCP: list tools → infer signatures → health checks → domain/verb tagging.
  - OpenAPI: parse operations, auth, rate limits → create descriptors → embeddings for semantic match.
- Ranking features:
  - Similarity to intent/step semantics; policy/consent fit; cost/time estimates; trust/reliability; reuse bonus.
- Selection strategy:
  - Top-K per step with diversity across provider types; optional shadow evaluation for non-destructive steps.

---

## 7) Execution & Partial Outcomes with Iterative Planning

- Step execution emits PartialExecutionOutcome early and at completion for long-running tasks.
- Retry semantics: idempotent vs non-idempotent; backoff hints.
- Re-planning consumes deltas and recomputes minimal next plan fragment.
- Human-in-the-loop triggers on ambiguity or consent boundaries.
- Determinism & replay: record seeds, provider versions, response fingerprints; avoid storing sensitive data unless opted-in.

---

## 8) Synthetic Capability Generation Pipeline

- Contract inference: extract stable params/outputs from varied traces; confirm with hold-out example.
- Artifacts: signature & types, description, 2–3 examples, golden test, policy metadata, provenance links.
- Registration & versioning: registry entry with embedding/tags; version bump on signature/provider changes; periodic health checks.

---

## 9) Governance, Security, Privacy

- Consent model: capability-scoped approval; batch with granular overrides; default deny beyond declared scope.
- Data boundaries: restrict outbound domains and filesystem paths; annotate PII fields; redaction in logs.
- Policy hooks: deny early with rationale; no long-held locks waiting for LLM/human input; use secure stdlib; use `log` not `println`.

---

## 10) Demo Storyboard (Showcase)

- Scenario: “Prepare a summary report on a topic with live data: fetch from an API, clean it, chart it, and send me a PDF. Ask me to confirm sources and filters.”
- Beats:
  - Extract intent; ask 2–3 high-signal questions (source, date range, output format).
  - Discover OpenAPI ops + synthetic "build_timeseries_chart" capability.
  - Show minimal plan, seek consent for HTTP to domains and file output path.
  - Execute with partial outcomes; re-plan on schema mismatch; ask clarification; continue.
  - Offer to synthesize "generate_topic_pdf_report"; present signature + test; register.
  - Second run: fewer questions/steps; show governance and audit diffs.

> Inset example: "Plan a trip to Paris"
>
> - Intent: goal + constraints (dates, budget), preferences (airline tier, museums).
> - Planner steps: flights.search → hotels.search → museum.tickets → itinerary.compose (class-first decomposition).
> - metadata.needs_capabilities:
>   - {class travel.flights.search, required_inputs [:origin :dest :dates :party_size], expected_outputs [:flight_selection]}
>   - {class lodging.hotels.search, required_inputs [:dest :dates :tier :budget], expected_outputs [:hotel_reservation]}
>   - {class event.booking.museum, required_inputs [:city :museums :dates], expected_outputs [:tickets]}
> - DelegatingArbiter asks only the missing required_inputs (e.g., :origin, :dates) via :ccos.user.ask.
> - Resolver binds concrete providers (or synthesizes stubs) and registers them; execution proceeds stepwise; itinerary composed at the end.

---

## 11) Tests and Evaluation

- Unit: intent parsing; ranking contributions/tie-breaks; synthesis parameter extraction; postcondition validation.
- Integration: end-to-end with replay fixtures; simulated human answers; partial outcome branches.
- Golden: ledger hashes stable; capability registry snapshot diff before/after synthesis.
- Metrics: questions per goal (decrease after learning), plan depth/width vs success, time-to-success, token cost, failure categories, trust score evolution.

---

## 12) Roadmap (Phases with Acceptance Criteria)

- Progress to date:
  - ✅ Hardened clarifier parsing + failure handling (Section 5).
  - ✅ Added Markdown fence stripping + unit tests for plan-step parser (Section 3).
  - 🔄 Pending rerun against live profile to validate fenced-plan parsing.

- Phase 0: Live LLM wiring
  - Replace stubbed clarifier/planner responses with DelegatingArbiter calls to the configured OpenRouter profile using RTFS request payloads.
  - Establish prompt scaffolding (system + few-shot exemplar) and persist it alongside capability metadata for audit.
  - Acceptance: demo run executes end-to-end on live LLM without panics; ledger stores prompt/response digests; failure modes surface actionable errors instead of silent fallbacks.

- Phase 1: Foundations upgrade
  - Wire MCP/OpenAPI discovery into capability marketplace with ranking.
  - Integrate PartialExecutionOutcome in orchestrator loop.
  - Acceptance: e2e demo with at least one partial re-plan; registry displays candidates with trust scores.

- Phase 2: DelegatingArbiter refinement
  - High-signal questions, consent gating, turn limits.
  - Acceptance: ≤4 questions for storyboard; no redundant questions; rationales logged.

- Phase 3: Synthesis & registration
  - Extract contract + tests; register synthetic capability; reuse in second run.
  - Acceptance: second run faster/fewer steps; passes golden tests; provenance visible.

- Phase 4: Governance polish & replay
  - Replay fixtures; policy denials with human-friendly reasons; privacy redaction.
  - Acceptance: deterministic golden tests pass; audit trail surfaces consent & policy checks.

- Phase 5: Showcase polish
  - TUI/visual outputs for plan graph, partial outcomes, consent UI; demo scripts.
  - Acceptance: turnkey demo script reproduces storyboard trace and visuals.

---

## 13) Risks and Mitigations

- OpenAPI variability/flakiness → health checks, cached fixtures, provider diversity.
- Over-questioning → thresholds, "don’t ask again", track coverage of intent fields.
- Synthesis brittleness → hold-out test must pass; confidence thresholds; human approval gate.
- Replay complexity → start with deterministic fixtures; selective recording later.

---

## 14) What CCOS/RTFS Shows That Others Don’t

- Causal, governed audit with intent graph diffs, not just chat logs.
- Reusable, typed capability synthesized from execution traces with tests and provenance.
- Partial execution outcomes driving planning instead of ad-hoc retries.
- Policy-aware, consent-scoped operations with replayable execution.
- Multi-provider orchestration with trust scoring and optional shadow eval.

---

## 15) Minimal Contract for the Demo Loop

- Inputs: natural-language goal, preferences; user answers; allowed scopes/consents.
- Outputs: plan graph, partial outcomes, final artifacts; optional synthesized capability.
- Error modes: capability unavailable, schema drift, consent denial; lead to re-plan or informed abort.
- Success criteria: goal met; governance compliant; ledger & provenance complete; second run benefits from synthesis.

---

## 16) Repo Mapping & Next Steps (High-Level)

- Touchpoints (indicative):
  - `rtfs_compiler/src/ccos/mod.rs`: process_request pipeline wiring.
  - `rtfs_compiler/src/ccos/orchestrator.rs`: integrate partial outcomes + re-planning.
  - `rtfs_compiler/src/ccos/delegating_arbiter.rs`: question generation + consent gating loop.
  - `rtfs_compiler/src/runtime/capability_marketplace.rs`: dynamic MCP/OpenAPI discovery & ranking.
  - `rtfs_compiler/src/ccos/causal_chain.rs` and `intent_graph.rs`: deterministic evidence & plan diffs.
  - `examples/user_interaction_smart_assistant.rs`: demo scenario, scripted Q&A, showcase outputs.
  - Prefer capability classes in early plan emission; bind concrete IDs during discovery.
  - Normalize capability IDs in dependency extraction; merge needs from planner + static scan.
  - Continuous resolution loop prioritizes `metadata.needs_capabilities` before scanning.
  - Collector questions are driven by `required_inputs` from needs.
- Non-goals for MVP: full GUI; multi-user collaboration; advanced caching across users; these can follow.

---

## 17) Acceptance Checklist (Showcase)

- [x] Clarifier parsing fails closed on malformed LLM output and is covered by unit tests.
- [x] Plan-step parser strips Markdown fences; regression test exists.
- [ ] Real-profile (`openrouter_free:balanced`) run succeeds end-to-end with clarified inputs and plan parsing.
- [ ] Minimal plan graph rendered/printed with steps and dependencies.
- [ ] Questions ≤ 4 with rationales; consent prompts are scoped and clear.
- [ ] Partial outcome triggers re-plan at least once; ledger shows both branches.
- [ ] Synthesis proposes a contract + test; registration succeeds; provenance displayed.
- [ ] Second run reuses synthesized capability; faster and with fewer questions.
- [ ] Golden tests pass; replay stable with fixtures; privacy respected.

---

## 18) Agent vs Capability (when to promote)

- Keep decomposition as composite capabilities when providers are known and orchestration is simple.
- Promote to an agent capability (with `:metadata {:kind :agent :planning true}`) when autonomy is required: multi-provider selection, retries with adaptation, human-in-the-loop gating.
- Governance tightens automatically for `:agent` (see 010/015).

---

## 19) Minimal implementation deltas (low-risk)

- Planner: emit `metadata.needs_capabilities` alongside current RTFS body.
- Dependency extractor: normalize/trim IDs; synthesize needs by scanning call sites; merge with planner-declared.
- Continuous resolution: prioritize metadata needs; then call-site scan; convert preflight/governance failures into needs.
- DelegatingArbiter: drive questions from `required_inputs` minus known intent/clarifications.
- Add an integration test demonstrating: plan with needs → resolver binds one need via stub → collector asks only missing inputs → execution pauses/resumes correctly.

---

## 20) Live LLM Delegation Implementation Guide

- **Configuration**: use `config/agent_config.toml` (or environment override) with a profile targeting `openrouter_free:balanced`, and set the CCOS runtime to load it under `:delegating-arbiter/profile-id` so requests stay governed.
- **Prompt contract**: compose the DelegatingArbiter request as RTFS data (`{:intent intent-map :coverage coverage-map :needs needs-capabilities}`) and wrap it with a system scaffold that enumerates governance rules, turn limits, and the expected response schema.
- **Invocation path**: issue `(:call :ccos.delegating-arbiter.invoke-llm {:profile profile-id :payload arbiter-input})` from the arbiter module, capture the LLM response as structured RTFS, and validate it against the clarifier schema before progressing.
- **Ledger & observability**: hash prompts/responses, record token usage, provider latency, and any redactions in the causal chain for audit without leaking raw content.
- **Fallbacks**: on schema violations or provider errors, emit a PartialExecutionOutcome with `:status :blocked`, enqueue a governance alert, and surface a clear remediation path (retry with adjusted prompt, switch provider, or ask the user for confirmation).
- **Testing loop**: add a recorded run (with redacted prompt bodies) under `tests/replays/` to guard against regressions; exercise both success and schema-failure branches using the live profile when tokens are available and stub fixtures when offline.
