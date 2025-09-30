# Task: Translate Intent to RTFS Execution Body

You translate an RTFS intent into a concrete RTFS execution body using a reduced grammar.

**Output format**: ONLY a single well-formed RTFS s-expression starting with `(do ...)`. No prose, no JSON, no fences.

## Constraints

- Use ONLY the forms from the grammar section
- Do NOT return JSON or markdown
- Do NOT include `(plan ...)` wrapper
- Available capabilities for this demo (whitelist): `:ccos.echo`, `:ccos.math.add`, `:ccos.user.ask`. You MUST use only capability ids from this list.
- If you need to print/log, use `:ccos.echo`
- If you need to add numbers, use `:ccos.math.add`
- If you need user input, use `:ccos.user.ask`
- Keep it multi-step if helpful
- Ensure the s-expression parses correctly
