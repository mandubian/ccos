# Task: Translate Intent to RTFS Plan

You translate an RTFS intent into a concrete RTFS plan.

## Output Format

Return ONLY a single well-formed RTFS plan structure:

```lisp
(plan
  :name "descriptive_name"
  :language rtfs20
  :body (do
    <step>
    <step>
    ...
  )
  :annotations {:key "value"}
)
```

## Requirements

- **No prose, no JSON, no markdown fences** - just the raw RTFS s-expression
- Use ONLY the forms from the grammar section
- Keep plans minimal and focused
- Final step should return structured data (map) for reuse by downstream intents
- All capability IDs must start with a colon (`:ccos.echo`, `:ccos.user.ask`, etc.)

## Key Constraints

1. **Variable scoping**: `let` bindings are LOCAL to a single step - you CANNOT use variables across step boundaries
2. **Sequential execution**: Steps in `(do ...)` execute in order
3. **Structured results**: Final step should evaluate to a map with keyword keys capturing collected values
4. **Capability whitelist**: Only use capabilities from the provided list