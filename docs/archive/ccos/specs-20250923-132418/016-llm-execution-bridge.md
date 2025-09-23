# CCOS Spec 016 — LLM Execution Bridge (Arbiter V1 / M1)

Status: Draft
Owner: Arbiter V1 (Issue #23)
Last-Updated: 2025-08-09

## Summary
Introduce a governed LLM execution bridge as a first-class special form `(llm-execute ...)` in the RTFS evaluator, wired to CCOS Delegation Engine and Model Registry. This enables the Arbiter and RTFS programs to request LLM inference in a deterministic, auditable way, with capability-based security (`ccos.ai.llm-execute`).

## Motivation
- Provide a minimal, deterministic path to call LLMs from plans.
- Enforce Separation of Powers: Arbiter proposes; Governance authorizes; Orchestrator executes; Causal Chain records.
- Support both local and remote models via `ModelProvider` abstraction.

## Design
- New RTFS special form: `(llm-execute ...)` handled in `Evaluator`.
- Security: gated by `RuntimeContext` capability `ccos.ai.llm-execute` (Controlled level).
- Providers resolved via `ModelRegistry` (defaults: `echo-model`, `arbiter-remote`; others can be registered at boot).
- Host notifications: `notify_step_started|completed|failed` to record causal events.

### Forms
1) Positional
```
(llm-execute "model-id" "prompt")
```
2) Keyword
```
(llm-execute :model "model-id" :prompt "prompt text" [:system "system prompt"]) 
```

### Semantics
- Validates security context; denies in Pure mode.
- Builds final prompt: if `:system` provided, prepends "System:\n...\n\nUser:\n...".
- Resolves provider by `model-id` from `ModelRegistry`.
- Calls `ModelProvider::infer(prompt) -> String`.
- Returns `String` value to RTFS program.
- Emits causal notifications through `HostInterface`.

### Errors
- `SecurityViolation` if capability not allowed.
- `UnknownCapability` if provider not found.
- `InvalidArguments` for malformed usage.
- `Generic` wrapping provider errors.

## Governance & Auditing
- All invocations create a step action in the Causal Chain.
- Recommended policy: enable in Controlled contexts; optionally require microVM for remote networking providers.

## Extensibility
- Providers: local GPU (`LocalLlamaModel`), remote (`OpenAI`, `Gemini`, `Claude`, `OpenRouter`).
- Future: streaming responses, JSON mode, tool-use, content filters.

## References
- Code: `rtfs_compiler/src/runtime/evaluator.rs` (eval_llm_execute_form)
- Security: `rtfs_compiler/src/runtime/security.rs`
- Delegation: `rtfs_compiler/src/ccos/delegation.rs`
- Issue: #23 (Arbiter V1)
