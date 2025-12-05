# Iterative Planner: Refinement, Grounding, Synthesis, and Adapters

Status: draft  
Owners: CLI planning stream  
Scope: planner/runtime (no code changes yet)

## Goals
- Reduce reliance on `ccos.user.ask` for formatting/transform steps.
- Make planning iterative and grounded with real outputs.
- Auto-synthesize missing capabilities; queue when synthesis cannot produce runnable code.
- Optionally bridge incompatible tool I/O with lightweight adapters.
- Prefer using grounded data (safe-exec outputs + seeded params) over re-asking the user.

## Proposed Flow (happy path)
1) **Decompose + resolve** using current modular planner (catalog + MCP).
2) **Opportunistic execution (safe-only)**: execute resolved, read-only steps (e.g., `mcp.github/...search_issues`) immediately; capture output schema + sample rows.
3) **Refine next intents with real data**: pass executed outputs/schemas into the next decomposition prompt; keep rule: avoid `user_input` unless params are missing/ambiguous.
   - Include grounded params and safe-exec snippets directly in refinement prompts (`result_<intent_id>`).
   - Add prompt rule: if grounded data is available, prefer `data_transform` or `output` over `user_input` for formatting/summarization.
   - Emit a compact preview of safe-exec results (schema + 1–2 rows) into `pre_extracted_params` so prompts can reference actual data.
4) **Iterative refinement**: if an intent is unresolved, decompose that intent again (bounded depth/iterations).
5) **Synthesis-or-enqueue** on unresolved data_transform/output intents:
   - Try to synthesize RTFS (prefer) or sandboxed code + RTFS wrapper.
   - On success: register in marketplace/catalog; retry resolution.
   - On failure: enqueue “needs implementation” artifact (id, schema, description, example I/O).
   - When prompting synthesis, pass any grounded samples/schemas relevant to the intent.
6) **Execution + logging**: causal chain logs all executions; governance gates mutating steps.

## Safety / Modes
- Modes: (a) planning-only, (b) plan-with-safe-exec (read-only), (c) full autonomous (opt-in).
- Governance: only auto-run idempotent/read-only capabilities; require approval for writes.
- Budgets: max recursion depth, max synth attempts.

## Synthesis Queue (when synth fails)
- Stored artifact includes: suggested capability id, input/output schema, NL description, example input/output, source intent, status=`needs_impl`.
- Storage: dir-based queue (e.g., `capabilities/pending_synth/`) or approval-queue variant.
- Reifiers: human or LLM codegen (RTFS preferred; code in microvm if needed).

## Prompting Tweaks
- Provide fully-qualified tool ids in prompts.
- Include real data/schemas from executed steps in the prompt.
- Explicit rule: don’t use `user_input` for formatting/summarization; prefer `data_transform` + `output`.

## Adapter Idea (I/O shims)
- When tool A output ≠ tool B input but is convertible with a small transform, synthesize a tiny RTFS adapter capability (e.g., map/rename fields, reshape arrays).
- Use same synth-or-enqueue flow; register adapter so the planner can chain A → adapter → B.
- Worth doing: yes, as a low-cost way to reuse existing tools without full resynthesis.

## Open Decisions
- Where to hook opportunistic execution (planner orchestrator vs. resolver) and the “safe” capability policy.
- Queue location and format; whether to reuse approval queue with a new state.
- Allow codegen beyond RTFS? If yes, enforce sandbox (microvm) by default.
- Backtracking: intentionally deferred for now. If we see frequent “wrong tool chosen” failures, add a limited backtracking mode (one alternate per intent, capped breadth/depth) with a heuristic scorer.

## Suggested Tasks (high level)
- Add “safe-to-run” exec pass + context injection.
- Add iterative refinement loop on unresolved intents (bounded).
- Expose `synthesize_or_enqueue` helper in synthesis layer; retry resolution on success.
- Implement synth queue artifact writer + basic reifier hook.
- Add adapter synthesis path for mismatched I/O (optional but valuable).

