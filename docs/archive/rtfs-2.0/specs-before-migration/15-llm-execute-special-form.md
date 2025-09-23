# RTFS 2.0 — Special Form: llm-execute

Status: Draft
Owner: Arbiter V1 (Issue #23)
Last-Updated: 2025-08-09

## Overview
Adds a new special form `(llm-execute ...)` to the RTFS evaluator to request language model inference via the CCOS Model Registry. This bridges RTFS programs with LLM providers while preserving determinism, security, and auditability.

## Syntax
- Positional
```
(llm-execute "model-id" "prompt")
```
- Keyword
```
(llm-execute :model "model-id" :prompt "prompt text" [:system "system prompt"]) 
```

## Arguments
- `model-id` (String): Identifier of a registered `ModelProvider` (e.g., `echo-model`, `arbiter-remote`, `openai`, `claude`).
- `prompt` (String): User/content prompt text.
- `:system` (String, optional): System instruction prelude; if present, final prompt is "System + User" composed.

## Evaluation Rules
1. Security check: requires capability `ccos.ai.llm-execute` in the current `RuntimeContext`. In Pure contexts, this fails.
2. Host notification: start → complete/failed for causal logging.
3. Provider resolution: `Evaluator.model_registry.get(model-id)`.
4. Inference: `ModelProvider::infer(final_prompt) -> String`.
5. Result: returns a `String` value to the program.

## Errors
- `(SecurityViolation ...)` if not allowed.
- `(UnknownCapability ...)` when provider missing.
- `(InvalidArguments ...)` on invalid shapes.
- `(Generic ...)` wrapping provider-specific failure.

## Notes
- Deterministic stub available via `echo-model` for CI.
- Future enhancements: structured outputs, streaming, temperature control, tool-use.

## Cross-References
- Implementation: `rtfs_compiler/src/runtime/evaluator.rs`
- Security: `rtfs_compiler/src/runtime/security.rs`
- Delegation/Providers: `rtfs_compiler/src/ccos/delegation.rs`, `rtfs_compiler/src/ccos/remote_models.rs`
