# 017 - Checkpoint and Resume Design (Draft)

This document outlines the design for checkpointing and resuming plan execution in CCOS.

## Goals
- Persist hierarchical execution context snapshots at safe points
- Allow orchestrator to resume execution from a checkpoint deterministically
- Maintain immutable auditability via Causal Chain

## Scope
- ExecutionContext persistence via `ContextManager.serialize()`
- Orchestrator API to create and load checkpoints
- Causal Chain linkage: PlanCheckpointCreated, PlanResumedFromCheckpoint

## Checkpoint Lifecycle
- When: typically at `(step ...)` boundaries or explicit `(checkpoint ...)`
- What: entire `ContextStack` serialized (JSON) + plan cursor metadata
- Where: Plan Archive (content-addressable storage)

## Resume Semantics
- Input: checkpoint blob + plan identifier + resume cursor (step ID)
- Process: restore context stack, validate security (`RuntimeContext`), continue
- Conflicts: if external world changed, resume policies and governance may require re-validation or human approval

## Security & Governance
- Checkpoint blobs are signed and content-hashed
- Resume requires policy checks (e.g., time bounds, constitution constraints)

## Open Questions
- Diff-based vs. full snapshot cadence
- Partial subtree restore for sub-plan resume
- Integration with Working Memory for large state

## Next Steps
- Add orchestrator API: `create_checkpoint(plan_id, context_manager)` and `resume_from(plan_id, checkpoint_id)`
- Emit Causal Chain events with checkpoint IDs
- Provide examples and tests


