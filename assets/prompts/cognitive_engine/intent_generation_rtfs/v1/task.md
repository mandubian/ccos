# Task: Generate a Single RTFS Intent

You convert a natural language user request into exactly one RTFS intent s-expression.

MUST output ONLY a single line or multi-line `(intent ...)` form with no explanations.

Available variables:
- {{natural_language}} : the raw user request
- {{available_capabilities}} : Rust Vec debug print of capability symbols available later in planning

