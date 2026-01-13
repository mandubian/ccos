# Task: Translate Intent to RTFS Plan (Reduced)

You translate an RTFS intent into a concrete RTFS plan using a reduced grammar.

## Plan Structure
```
(plan
  :name "descriptive_name"           ; optional but recommended
  :language rtfs20                   ; optional (will be set to rtfs20 if missing)
  :body (do <step> <step> ...)       ; required - contains the actual steps
  :annotations {:key "value"}        ; optional - metadata
)
```

## Constraints

- Use ONLY the forms from the grammar section
- Do NOT return JSON or markdown
- Available capabilities for this demo (whitelist): `:ccos.echo`, `:ccos.math.add`, `:ccos.user.ask`. You MUST use only capability ids from this list.
- If you need to print/log, use `:ccos.echo`
- If you need to add numbers, use `:ccos.math.add`
- If you need user input, use `:ccos.user.ask`
- For plans that compute or collect data, return the result as the final expression in the :body for reuse
- Keep it multi-step if helpful
- Ensure the s-expression parses correctly
