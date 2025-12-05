## CCOS Smart Assistant Autonomy Plan (Draft)

### Motivation
CCOS Smart Assistant must stop failing fast when the LLM invents a capability. The arbiter should be able to surface *what it needs*, and CCOS should autonomously discover, synthesize, or escalate the missing tool rather than aborting execution. This document records the autonomy-focused workflow built on the planner modules: goal signals, coverage, resolver, scaffolding, and governance.

### Key Concepts

- **Goal-Oriented Requirements** – Every plan step, even while still speculative, contributes to `GoalSignals`. Unknown capability IDs are treated as `GoalRequirementKind::MustCallCapability` with provenance metadata and a `RequirementReadiness` state (`Identified`, `Incomplete`, `PendingExternal`, `Available`).
- **Coverage as Signal** – The coverage analyzer classifies unmet requirements into missing/incomplete/pending buckets instead of throwing errors. This drives diagnostics, resolver actions, and menu annotations.
- **Resolver as Capability Fabricator** – The requirement resolver orchestrates the `MissingCapabilityResolver` (discovery, MCP introspection, synth) and reports structured outcomes (`CapabilitiesDiscovered`, `AwaitingExternal`, `Failed`). It updates requirement readiness and metadata so the planner loop can re-run with the new capabilities.
- **Synthetic Scaffolds & Readiness** – When synthesis/discovery cannot immediately produce a manifest, a synthetic capability (with `capability_status`) is injected into the menu and the requirement stays marked `Incomplete` or `PendingExternal`. This keeps the LLM aware of the conceptual tool and enables escalation through governance or human intervention.
- **Planner-as-Process** – These stages eventually compose into RTFS capabilities (`planner.extract_goal_signals`, `planner.build_capability_menu`, `planner.synthesize_plan_steps`, `planner.validate_plan`, `planner.resolve_capability_gaps`, `planner.materialize_plan`) so that the planner itself can be invoked, audited, and checkpointed in CCOS.

### Autonomy Workflow

1. **Signal extraction** – `GoalSignals` captures goal text, intent, constraints, preferences, and any new `MustCallCapability` requirements derived from explicit goal language or LLM-specified capability IDs that were not yet known.
2. **Menu build** – `planner.build_capability_menu` runs catalog searches, applies descriptors/overrides, and injects synthetic scaffolds for requirements whose readiness is not `Available`.
3. **Plan synthesis loop** – `planner.synthesize_plan_steps` prompts the LLM with the menu, receives JSON steps, and produces structured feedback when schema or coverage gaps emerge.
4. **Validation + coverage** – `planner.validate_plan` enforces schema rules and coverage, producing a `CoverageCheck` that lists missing/incomplete/pending capabilities instead of failing immediately.
5. **Capability resolution** – `planner.resolve_capability_gaps` feeds the coverage result into the resolver. The resolver:
   - Queries existing manifests and descriptors.
   - Invokes MCP/introspection or synthesizes RTFS capabilities.
   - Updates requirement readiness and metadata (`provision_source`, `pending_request_id`, etc.).
6. **Menu/materialization** – With new manifests or scaffolds registered, the menu rebuilds and the loop restarts. If readiness remains `PendingExternal`, the planner surfaces the pending ticket and awaits completion; if `Available`, the plan continues to finalization.

### Tracking & Work Items (for the AI Planner)

> These tasks are written *for the cognitive agent* (LLM+CCOS), not the human end-user. They describe how you, as the planner, should evolve your own tools and workflows while still interacting with a human partner.

1. **New statuses and metadata (GoalSignals + requirements)**
    - Add `RequirementReadiness` enum in Rust: `Unknown | Identified | Incomplete | PendingExternal | Available`.
    - Extend `GoalRequirement` / `GoalSignals` structs to carry:
       - `readiness: RequirementReadiness`
       - `provision_source: Option<ProvisionSource>` (e.g., `ExistingManifest | Synthesized | MCP | HumanProvided`).
       - `pending_request_id: Option<String>` to link governance or ticketing.
       - `scaffold_summary: Option<String>` with a short natural language description usable directly in LLM prompts ("conceptual tool I wish I had").
    - Ensure planner capabilities (`planner.extract_goal_signals`, `planner.validate_plan`) update these fields instead of throwing errors.
           - **Status:** Implemented in `smart_assistant_planner_viz` loop — unknown capabilities now create `MustCallCapability` requirements with `RequirementReadiness::Identified`, metadata (origin step, requested outputs), and scaffold summaries; resolver outcomes promote readiness to `PendingExternal` or `Available` while tracking `provision_source` and `pending_request_id`. First RTFS capability (`planner.extract_goal_signals`) now exposes this extraction step to the marketplace, returning enriched `GoalSignals` for reuse by metaplans.

2. **Coverage instrumentation and menu annotations**
    - Introduce a `CoverageCheck` data type that includes per-requirement readiness and rationale.
    - In `planner.validate_plan`, return `CoverageCheck` even on failure; never panic on missing capabilities.
    - Surface readiness information in:
       - Planner visualization (`smart_assistant_planner_viz`), e.g., color-code `Missing/Incomplete/PendingExternal/Available`.
       - Causal Chain actions (e.g., `CoverageEvaluated`, `RequirementUpdated`).
    - Provide a compact JSON/RTFS snippet summarizing coverage that can be embedded directly into LLM prompts as *"what you currently have vs what you still need"*.

3. **Resolver outcomes and control flow**
    - Extend `RequirementResolutionOutcome` to distinguish:
       - `CapabilitiesDiscovered { manifests: Vec<CapabilityManifest> }`
       - `Synthesized { manifests: Vec<CapabilityManifest>, tests_run: Vec<TestResult> }`
       - `AwaitingExternal { pending_request_id, suggested_human_action }`
       - `Failed { reason, recoverable: bool }`.
    - Update `planner.resolve_capability_gaps` to:
       - Map each requirement to one of these outcomes.
       - Update `RequirementReadiness` accordingly.
       - Emit a structured summary the LLM can consume to decide whether to retry planning, ask the human for input, or degrade gracefully.
    - **Status:** Implemented in `smart_assistant_planner_viz` — planner now differentiates discovered vs. synthesized capabilities (including test summaries), tracks external requests with suggested follow-ups, and halts with a pending-external summary instead of looping forever. Remaining: expose the same logic as a reusable RTFS capability and add governance hooks.

4. **Governance integration for synthesized and pending tools**
    - Implement a governance proposal flow for any `Synthesized` or `AwaitingExternal` outcome:
       - Create a `CapabilityProposal` record with fields: `manifest`, `origin_requirement_id`, `provision_source`, `author_agent`, `sample_tests`.
       - Submit proposal via `GovernanceKernel` and record the resulting `proposal_id` into `pending_request_id`.
    - Ensure planner prompts include a concise description of pending proposals when asking the human for help (e.g., *"I drafted a new capability; it awaits your approval under proposal #123"*).
    - Make sure rejection/approval events update `RequirementReadiness` from `PendingExternal` → `Available` or `Failed`.

5. **Autonomous capability synthesis (for planner and data primitives)**
    - Wire `MissingCapabilityResolver` to a synthesis pipeline that:
       - Prompts the LLM with: existing manifests, schemas, and a compact RTFS grammar primer.
       - Requests a combined `capability-manifest` 2b RTFS implementation 2b minimal tests.
       - Validates via the canonical RTFS loader and the restricted runtime used for synthesized code.
    - On success, auto-register the manifest into the capability catalog; on failure, store a scaffold with:
       - Human-readable description.
       - The failing RTFS snippet and analyzer diagnostics.
       - A suggested next action (retry synthesis, escalate to human, or adjust plan).
    - Treat planner modules themselves (e.g., `planner.validate_plan`) as candidates for synthesis/refinement over time, not just application-level capabilities.

6. **Negative tests, tracing, and LLM-facing diagnostics**
    - Add tests where the LLM proposes plans with:
       - Unknown capabilities.
       - Wrong schemas.
       - Conflicting requirements.
    - Ensure the system behavior is always:
       - Record a requirement.
       - Run resolver.
       - Produce structured diagnostics (no panics).
    - Structure logs and traces so that a future LLM prompt can include:
       - The last `CoverageCheck`.
       - A list of requirements with readiness and sources.
       - Short, human-and-LLM-friendly explanations ("I cannot proceed because X requires Y; I attempted Z").
    - Prefer short, machine-readable diagnostics over verbose prose so they can be re-used and transformed by other capabilities.

### Acceptance Criteria

- The demo can handle an LLM plan referencing an unknown capability without terminating: it records the requirement, runs the resolver, and either discovers/synthesizes the tool or escalates it, then retries.
- Capability readiness and pending requests are surfaced in the menu and logs, making the planner’s reasoning inspectable.
- The autonomy workflow can be expressed as RTFS capabilities so the planner itself becomes an executable plan (“planner.generate_plan”) per the generalization roadmap.


