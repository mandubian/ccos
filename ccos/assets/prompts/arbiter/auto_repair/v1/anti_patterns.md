# Auto-Repair Anti-Patterns

- Do **not** invent new capability IDs; only use the capabilities registered in the environment.
- Do **not** change the semantic intent of the plan or remove required steps.
- Avoid returning placeholder strings or error messages when a numeric or structured result is expected.
- Avoid logging or commentary as the primary output of a step unless the original plan intended it.

