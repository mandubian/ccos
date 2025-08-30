# RTFS Demos Guide

This guide walks you through the built-in RTFS demos, what they showcase, how to run them, and how to enable deep observability for prompts, plans, and step outputs.

- Demo A: Single-intent LLM plan demo (`llm_rtfs_plan_demo.rs`)
- Demo B: Multi-intent orchestration demo (`multi_intent_demo.rs`)

Both demos run entirely on CCOS components: Arbiter (intent/plan), GovernanceKernel validation, Orchestrator execution, CausalChain audit, and CapabilityMarketplace.

---
## Prerequisites
- Rust toolchain installed.
- Linux/macOS shell. Examples assume zsh.
- Optional: an LLM API key for live plan generation.
  - OpenRouter: set `OPENROUTER_API_KEY` and optional `LLM_MODEL`.
  - or OpenAI: set `OPENAI_API_KEY` and optional `LLM_MODEL` (defaults to `gpt-4o-mini`).
- No key? Use the deterministic stub paths shown below.

Environment flags you can toggle:
- `RTFS_SHOW_PROMPTS=1` prints the plan-generation prompt and raw response.
- `RTFS_ARBITER_DEBUG=1` prints intent-generation prompt/response and a parsed summary.
- `RTFS_FULL_PLAN=1` asks the LLM to emit a `(plan ...)` wrapper; the runner still extracts `:body`.

---
## Capability signatures used in demos (strict)
To reduce ambiguity and avoid type errors, the prompts and validators assume these signatures:
- `:ccos.echo` must be called with a single map argument containing `:message` (string)
  - Example: `(call :ccos.echo {:message "hi"})`
- `:ccos.math.add` must be called with exactly two positional number arguments (no map form)
  - Example: `(call :ccos.math.add 2 3)`

The governance layer verifies capability ids and basic arity/types; the prompt scaffolds steer models to produce the right form.

---
## Demo A: Single-intent LLM plan (`examples/llm_rtfs_plan_demo.rs`)
Purpose
- Generate an intent from a natural-language goal using Arbiter.
- Generate a multi-step RTFS plan via deterministic stub or LLM.
- Execute and show per-step outputs from the CausalChain (CapabilityCall → CapabilityResult).

Common flags
- `--goal "..."` natural-language goal (required)
- `--stub` force deterministic stub instead of hitting an LLM
- `--verbose` show advisory whitelist and extra logs
- `--debug` enable prompt/response printing for intent and plan
- `--full_plan` ask LLM for a `(plan ...)` wrapper (runner still extracts `:body`)

What you’ll see
- Advisory capability whitelist
- Intent lifecycle: Created → Executing → Completed/Failed
- Step outputs printed from the audit ledger
- With `--debug`, the exact prompts and raw LLM responses

Troubleshooting
- If a plan fails due to argument shapes (e.g., using a map with `math.add`), enable `--debug` and check the raw plan. The prompt scaffolding enforces positional integers for `math.add`.
- If you lack an API key, add `--stub` to use the deterministic provider.

---
## Demo B: Multi-intent orchestration (`examples/multi_intent_demo.rs`)
Purpose
- Demonstrates multiple intents run sequentially with per-intent lifecycle logs, multi-step plans, and a final summary.
- Shows how a later intent can depend on earlier outputs (e.g., compute a sum, then announce it).

Scenarios
- `--scenario greet-and-sum` (default):
  1) Say Hi using echo
  2) Add integers 2 and 3 and return only the sum
  3) Announce the computed sum via echo

Common flags
- `--llm-plans` use the LLM for plan generation (still uses Arbiter for intent)
- `--deterministic` use the deterministic stub for plan generation
- `--stub` also forces stub for intent generation if no API keys are present
- `--verbose` show advisory whitelist, full action list, and extra logs
- `--debug` enable intent/plan prompt + raw responses
- `--full_plan` ask LLM for a `(plan ...)` wrapper (runner still extracts `:body`)

What you’ll see
- For each intent: an intent id, lifecycle transitions, a 2-step plan (echo, add), and step outputs read from the CausalChain.
- Final summary with each intent’s id, goal, plan id, and output value.

Troubleshooting
- Enable `--debug` to view both intent prompts and plan prompts/responses.
- If the LLM emits the wrong `math.add` argument shape, the tightened prompt should correct it; otherwise you can switch to `--deterministic` to proceed.

---
## Observability tips
- CausalChain emits CapabilityCall and CapabilityResult for each step; demos read these back to print compact step outputs.
- Use `--verbose` to dump every action entry for a plan (type, function name, has_result).
- Duration accounting in microVM providers is clamped to avoid negative/overlong reports when timeouts occur.

---
## Quick reference
- Single-intent demo file: `rtfs_compiler/examples/llm_rtfs_plan_demo.rs`
- Multi-intent demo file: `rtfs_compiler/examples/multi_intent_demo.rs`
- Capability whitelist in prompts: `:ccos.echo`, `:ccos.math.add`
- Strict signature rules are embedded in the plan-generation prompt and enforced by governance validators.

*** End of guide ***
