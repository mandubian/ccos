# Adaptation and Composition Model

This document explains how adaptation works in Autonoetic, why it is composition-first, where adaptation data is stored, and how runtime instruction composition is performed.

## 1. Purpose

Adaptation exists to let the system refine an existing specialist for a narrow gap without installing a brand new agent.

Core goals:

- Prefer reuse over creation.
- Preserve base agent identity and base files.
- Keep adaptations auditable and reversible.
- Keep adaptation behavior deterministic at load time.

## 2. Key Principle: Composition-First, Non-Destructive

Autonoetic adaptation is designed as a composition layer over a base agent.

What this means:

- `agent.adapt` targets an existing `target_agent_id`.
- The base `SKILL.md` remains canonical for agent identity and baseline instructions.
- Adaptation data is stored as overlay metadata, not as in-place replacement of base files.
- Runtime instructions are composed by appending only planner-selected adaptation overlays.

What this explicitly avoids:

- Creating a new durable agent for small role gaps.
- Silent mutation of base agent instructions.
- Untracked patches that cannot be audited later.

## 3. Storage Model

Adaptations are gateway-owned metadata and stored centrally under the gateway control plane.

Path layout:

- `agents/.gateway/adaptations/<agent_id>/<adaptation_id>.json`

Rationale:

- Keeps adaptation governance in the same ownership model as other gateway state.
- Avoids scattering canonical adaptation state across per-agent private folders.
- Enables centralized retention, indexing, and future integrity controls.

## 4. Adaptation Record Shape

Each adaptation is persisted as one JSON record file.

Current fields include:

- `adaptation_id`: Stable identifier for this overlay.
- `metadata.compose_mode`: Currently `behavior_overlay`.
- `metadata.base_manifest_hash`: Hash of base manifest at adaptation time.
- `metadata.capability_delta`: Declared capability additions.
- `metadata.evidence_refs`: Session-oriented references for audit traceability.
- `metadata.adapted_at`: Adaptation timestamp.
- `metadata.adapter_agent_id`: Caller agent ID that authored adaptation.
- `behavior_overlay`: Instruction overlay text.
- `asset_changes`: Declared bounded changes for composition context.
- `asset_changes_mode`: `overlay_only`.
- `base_mutation`: `false`.
- `rationale`: Optional human/model rationale.
- `applied_at`: Persistence timestamp.

## 5. Runtime Composition Behavior

When an agent is loaded for a request:

1. Base instructions are parsed from `SKILL.md`.
2. The loader checks `agents/.gateway/adaptations/<agent_id>/`.
3. Valid adaptation JSON records are read.
4. The planner/execution path provides explicit `selected_adaptation_ids` in spawn metadata.
5. Only selected records with non-empty `behavior_overlay` are collected.
6. Overlays are sorted by `(adapted_at, adaptation_id)` for deterministic ordering.
7. Composed system instructions are produced by appending an `Adaptation Overlays` section.

Effectively:

- Base instructions stay intact.
- Adaptation overlays influence behavior only when explicitly selected by planner/execution.
- Overlay order is deterministic for reproducibility.

## 6. Approval and Capability Surface Changes

If an adaptation requests `capability_additions`, adaptation requires explicit promotion evidence.

Accepted approval evidence (`promotion_gate`):

- `evaluator_pass: true` and `auditor_pass: true`, or
- `override_approval_ref` (when operator override has been granted).

Without approval evidence, the tool returns a structured response with:

- `ok: false`
- `approval_required: true`
- Reason and repair guidance.

## 7. Input Guardrails and Limits

Current guardrails include:

- `target_agent_id` must be valid.
- `behavior_overlay` must be non-empty.
- `asset_changes` max length is 5.
- Asset paths are constrained to `skills/*` or `state/*`.
- Non-delete asset content size is capped at 100 KB.

These limits keep adaptation narrow, auditable, and predictable.

## 8. Observability and Auditability

Auditability is enabled by design:

- Every adaptation is a persisted JSON artifact.
- Metadata includes who adapted what, when, and against which base hash.
- Session evidence references can be linked back to causal traces.

This allows reviewers to answer:

- Which agent created the adaptation?
- What behavior was overlaid?
- What approval evidence was used?
- In what order did overlays become active?

## 9. Operational Guidance

Use adaptation when:

- A strong existing specialist already matches intent.
- The gap is small and role-local.
- You need bounded behavioral refinement rather than a new durable role.

Do not use adaptation when:

- The gap fundamentally changes role boundaries.
- You need broad new authority and capability surface expansion.
- A cleanly separated new specialist is preferable.

## 10. Example Tool Call

```json
{
  "target_agent_id": "researcher.default",
  "behavior_overlay": "Prioritize SEC filings and earnings-call transcripts.",
  "asset_changes": [
    {
      "path": "skills/financial_research.md",
      "action": "create",
      "content": "Checklist for 10-K and 10-Q analysis."
    }
  ],
  "promotion_gate": {
    "evaluator_pass": true,
    "auditor_pass": true
  },
  "rationale": "Narrow role refinement for financial investigations"
}
```

In composition-only mode:

- `asset_changes` are recorded as overlay intent.
- Base agent files are not mutated as canonical state by adaptation itself.
- Adaptation overlays are not applied implicitly; they are applied only when explicitly selected.

## 11. Deterministic Pipeline Hooks (Planned)

While behavior overlays are sufficient for basic instruction adjustments, relying entirely on LLM prompt adherence for rigorous data transformations (e.g. stripping PII, formatting strict API schemas) is brittle and token-expensive. 

To address this, adaptation will support **Deterministic Pipeline Hooks** (`adaptation_hooks`).

### Mechanics

- Adapters can supply `pre_process` and `post_process` hooks pointing to scripts deployed via `asset_changes`.
- These hooks execute directly in the Autonoetic Gateway outside of the LLM context.
- **Pre-process**: Takes the user's input/request, runs it through the script (e.g., standardizing a JSON payload, stripping secrets), and passes the *output of the script* to the LLM agent tick.
- **Post-process**: Takes the LLM agent's output, runs it through the script (e.g., enforcing an XML schema, logging to external monitoring), and passes the *output of the script* back to the user or downstream agent.

### How the Planner Uses It

When the planner identifies that a specialist agent struggles with rigid formatting or repetitive data scrubbing, it will:
1. **Generate Interceptors**: Draft small, deterministic scripts (e.g., Python or bash) that handle the exact transformation logic.
2. **Package via `asset_changes`**: Call `agent.adapt` and mount these scripts into the agent's `skills/` directory (e.g., `skills/format_input.py`, `skills/scrub_output.py`).
3. **Register Hooks**: Include the `adaptation_hooks` block in the adaptation payload:
   ```json
   "adaptation_hooks": {
     "pre_process": "sandbox.exec --script skills/format_input.py",
     "post_process": "sandbox.exec --script skills/scrub_output.py"
   }
   ```
4. **Spawn with Hooks**: The planner issues `agent.spawn` and provides the exact explicit `selected_adaptation_ids`. The Gateway mechanically intercepts all I/O for that session tick, completely bypassing LLM hallucination risk for the rote transformation step.

## 12. Known Constraints and Next Enhancements

Current constraints:

- Overlay composition is instruction-centric (behavior overlay text).
- Activation depends on explicit planner/execution selection via `selected_adaptation_ids`.
- Asset change overlays are declared and auditable, but not merged into live file trees as canonical base mutations.

Likely future enhancements:

- Optional gateway index for fast adaptation lookup across all agents.
- Optional signed adaptation records.
- Operator tooling for list/inspect/disable adaptation overlays.
- Policy modules for planner-side adaptation selection.

## 12. Summary

Adaptation in Autonoetic is a controlled composition mechanism:

- Reuse-first by default.
- Base-preserving and audit-friendly.
- Gateway-owned metadata storage.
- Deterministic runtime composition.

This provides a practical middle path between brittle one-shot edits and costly new-agent proliferation.
