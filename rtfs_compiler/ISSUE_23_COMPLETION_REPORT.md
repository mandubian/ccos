# Issue #23 Progress Report: Arbiter V1 — LLM execution bridge and NL→intent/plan

- Issue: #23 — [CCOS] Arbiter V1: LLM execution bridge and NL-to-intent/plan conversion
- Status: In Progress
- Report Date: August 11, 2025

## Executive Summary

The Arbiter module exists with a minimal NL→Intent→Plan pipeline and unit tests. Core V1 features from the issue checklist (LLM execution bridge, LLM-driven intent/plan, dynamic capability resolution, agent registry, task delegation) are being implemented iteratively. The LLM bridge is complete and integrated; IntentGraph storage is wired with a runtime-safe sync wrapper. Remaining work focuses on DelegatingArbiter integration and end-to-end tests.

## Recent Changes (Aug 11, 2025)

- CCOS: async init with built-in capabilities registered at startup
- Orchestrator: trim RTFS plan body before parsing to avoid leading-whitespace parse errors
- RuntimeHost: bridge async capability calls via `futures::executor::block_on` to avoid nested Tokio runtime panics
- DelegatingArbiter: integrated behind env toggle; shares base Arbiter IntentGraph; model selected via `CCOS_DELEGATING_MODEL`
- ModelRegistry: added deterministic `stub-model` for CI and deterministic tests
- Tooling: VS Code test task fixed to run in `rtfs_compiler/` cwd

## Current Implementation

- Module: `rtfs_compiler/src/ccos/arbiter.rs`
  - Exposes `Arbiter::process_natural_language` → `natural_language_to_intent` → `intent_to_plan`
  - NL→Intent currently uses simple pattern matching (LLM path pending behind a flag)
  - Intent→Plan emits RTFS plan templates (string) based on intent name
  - Integrates an `IntentGraph` handle; storing intent is implemented and runtime-safe
- IntentGraph
  - Sync wrappers no longer panic in Tokio: when already inside a runtime, use `futures::executor::block_on` instead of `block_in_place`
  - Fix verified by passing `ccos::arbiter::tests::test_arbiter_proposes_plan`
- DelegatingArbiter (`rtfs_compiler/src/ccos/delegating_arbiter.rs`)
  - Stores generated intents into shared `IntentGraph`
  - LLM-driven JSON→Intent and RTFS plan generation code-paths prepared; integration tests pending
- Capability Marketplace
  - Refactored into modular registry/executors with schema-aware validation
- Host bridge fix
  - `RuntimeHost::execute_capability` now uses `futures::executor::block_on` to bridge async capabilities from a sync evaluator safely
  - E2E + integration tests pass with this approach

## Environment / Configuration

- Enable DelegatingArbiter: `CCOS_USE_DELEGATING_ARBITER=1`
- Select model for delegation: `CCOS_DELEGATING_MODEL=stub-model` (deterministic) or `echo-model`

## Testing Status

- E2E: `ccos::tests::test_ccos_end_to_end_flow` — passing
- Integration suite: passing on last run (52 passed; 0 failed; 2 ignored)
- Deterministic stub model available for DelegatingArbiter tests

## Checklist (mirrors issue)

- [x] LLM execution bridge (`(llm-execute)`) — M1
- [ ] Natural language to intent/plan conversion (LLM + templates) — M2
- [ ] Dynamic capability resolution via marketplace — M3
- [ ] Agent registry integration (for delegation) — M4
- [ ] Task delegation and RTFS Task Protocol — M5

## Milestones and Acceptance Criteria

- M1 LLM execution bridge
  - Add `LlmProvider` trait and providers (OpenAI/Anthropic/local stub)
  - RTFS intrinsic `(llm-execute prompt {:model …})` wired in evaluator and guarded by Governance Kernel
  - Arbiter configurable via `ArbiterConfig` (model/provider)
  - Tests: deterministic stub provider; evaluator integration

- M2 NL-to-Intent/Plan (LLM + templates)
  - `natural_language_to_intent` produces `Intent` via LLM schema (no pattern matching)
  - `IntentGraph::store_intent(intent)` implemented and invoked
  - `intent_to_plan` uses few-shot prompt/templates to emit RTFS (kept in `PlanBody::Rtfs`)
  - Tests: assert structured fields and expected RTFS steps

- M3 Dynamic capability resolution
  - Capability registry integration; validate `:module.fn` in generated plans
  - Annotate plan with resolved providers or emit deterministic errors
  - Tests: success path and missing-capability errors

- M4 Agent registry integration
  - Define `AgentRegistry` trait + stub; Arbiter proposes delegation candidates based on intent constraints
  - Tests: selection logic under simple constraints

- M5 Task delegation protocol (RTFS Task Protocol)
  - Define `TaskRequest`/`TaskResult`; orchestrator dispatch and result handling
  - Tests: local stub round-trip

## Risks and Dependencies

- Governance checks for `(llm-execute)` and delegation
- Environment configuration for provider API keys
- Determinism/testing of LLM paths (use stub for CI)
- Alignment with Capability Marketplace and Evaluator interfaces
- Docs: tracker is at `docs/archive/ccos/CCOS_MIGRATION_TRACKER.md` (section 3.1)

## Next Steps (immediate)

1) Switch NL→Intent to schema-driven LLM (behind a feature flag) and wire DelegatingArbiter via config toggle in CCOS
   - Toggle: `CCOS_USE_DELEGATING_ARBITER=1`, `CCOS_DELEGATING_MODEL=stub-model`
   - Register deterministic `stub-model` in `ModelRegistry::with_defaults()`
2) Add integration tests validating DelegatingArbiter with the deterministic stub provider (Intent JSON + RTFS plan)
   - Assert: Intent stored in IntentGraph; plan executes successfully using built-ins; CausalChain records success
3) Validate generated plans against Capability Marketplace (M3 pre-work) and surface deterministic errors
   - Preflight parse RTFS; verify `:module.fn` existence
4) Keep unrelated failing areas deferred per scope (parser complex types, streaming, microvm, e2e)

## References

- Issue: https://github.com/mandubian/ccos/issues/23
- Tracker: `docs/archive/ccos/CCOS_MIGRATION_TRACKER.md` (3.1 Arbiter V1)
- Arbiter: `rtfs_compiler/src/ccos/arbiter.rs`
- Delegating Arbiter: `rtfs_compiler/src/ccos/delegating_arbiter.rs`
- Host bridge: `rtfs_compiler/src/runtime/host.rs`
