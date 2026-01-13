# Task: Translate Intent to RTFS Plan

You translate an RTFS intent into a concrete RTFS plan using a constrained schema.

**Output format**: ONLY a single well-formed RTFS s-expression starting with `(plan ...)`. No prose, no JSON, no fences.

## Allowed Keys in (plan ...)

- `:name "short descriptive name"` - optional
- `:language rtfs20` - optional (will be set to rtfs20 if missing)
- `:body (do <step> <step> ...)` - **required**; use reduced grammar for steps and calls
- `:annotations {:key "value" :k2 "v2"}` - optional; keyword keys and string values only

## Forbidden or Ignored (kernel-owned)

These fields are managed by the kernel and will be ignored or overwritten:
- `:plan_id`
- `:intent_ids`
- `:status`
- `:policies`
- `:capabilities_required`
- `:created_at`
- `:metadata`
- `:input_schema`
- `:output_schema`

## Constraints

Return exactly one `(plan ...)` with these constraints.
