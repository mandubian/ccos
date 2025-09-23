# Plan Generation Modes: Direct RTFS (primary) + JSON IR (fallback)

Purpose: Define how the Arbiter generates multi-step RTFS plans from Intent while ensuring safety, determinism, and auditability. Prefer direct RTFS for expressiveness; use JSON IR as a safety net and for evaluation/compare.

---
## Rationale
- RTFS concisely expresses control flow and execution semantics; it’s the native format the Orchestrator executes.
- LLMs don’t inherently “know RTFS”; we provide a bounded subset and guardrails.
- JSON IR is easier for models to emit and for us to validate/repair; we can compile IR → RTFS deterministically and compare.

---
## Modes

### A. Direct RTFS Generation (primary)
- Input to Arbiter:
  - Intent (RTFS) with goal/constraints.
  - Capability inventory snapshot: `id`, version, brief description, argument schema/examples, safety notes.
- Prompt contract (enforced):
  - Output ONLY RTFS plan body wrapped in `(do ...)`.
  - Allowed forms: `do`, `step`, `call`, `let`, `if`, `step-parallel`, `step-loop`.
  - Map keys must be colon-prefixed keywords (e.g., `{:message "hi"}`), no comments, no prose.
  - Use only capabilities from the provided list (e.g., `:ccos.echo`, `:ccos.math.add`).
- Post-generation checks (deterministic):
  1) Parse RTFS → Expression.
  2) Preflight capability ids; arity/type validation against schemas.
  3) GovernanceKernel sanitize/scaffold/validate.
  4) If any step fails, return compact error hints for auto-repair.
- Auto-repair loop: Up to N attempts (e.g., 2). If still invalid, switch to Mode B.

### B. JSON Plan IR (fallback + evaluation harness)
- JSON schema (conceptual):
  - `steps`: Array of
    - `id`: string
    - `name`: string
    - `capability`: string (e.g., `:ccos.echo`)
    - `args`: array | object (validated against capability schema)
    - `deps`: string[] (step ids)
    - `options` (optional): `{ :deterministic bool, :isolation "inherit|isolated", :contracts {...} }`
- Pipeline:
  1) Model emits JSON only (function-call/JSON mode recommended).
  2) Validate ids/args/deps; topo-sort.
  3) Compile IR → RTFS using PlanBuilder.
  4) Run the same parse/preflight/governance checks as Mode A.

---
## Equivalence & Comparison
- Canonicalization: Convert the final RTFS Plan → normalized IR (extract steps, capability calls, deps).
- Compare JSON IR (if available) vs. canonical IR:
  - Same step multiset (id/name/capability)
  - Equivalent deps (ignoring trivial renames)
  - Compatible args per schema
- Record diffs in IntentGraph metadata for audit and continuous improvement; execute the validated RTFS.

---
## Prompt Scaffold (excerpt)
System:
"""
You generate RTFS 2.0 plans.
Rules:
- Output ONLY an RTFS plan body wrapped in (do ...). No prose.
- Allowed forms: do, step, call, let, if, step-parallel, step-loop.
- Map keys MUST be colon-keywords. Use ONLY listed capabilities.
"""

User (capabilities excerpt):
"""
Capabilities:
- :ccos.echo { :message string }
- :ccos.math.add [number number]
Intent:
{:goal "Greet and add numbers"}
"""

Expected output (example):
```
(do
  (step "Greet" (call :ccos.echo {:message "hi"}))
  (step "Add" (call :ccos.math.add 2 3)))
```

---
## Safety & Governance
- All generations (RTFS or IR) go through: parse → capability preflight → governance validation.
- Orchestrator executes only validated RTFS; all side effects via `(call ...)`.
- Prompts and generations can be logged for audit (see CausalChain policies).

---
## Implementation Notes (next steps)
- Introduce `PlanGenerationProvider` (trait): `generate(intent) -> PlanGenerationResult { rtfs_plan, optional_ir, diagnostics }`.
- Add deterministic stub model for tests; plug OpenRouter-backed provider under feature flag.
- Implement IR validator + compiler (reuse `builders/plan_builder.rs`).
- Store both raw outputs and canonicalized IR diffs in IntentGraph metadata.
- Integration test: NL → Intent → RTFS Plan (primary) with fallback path exercised; execute; assert CausalChain entries and IntentGraph transitions.
