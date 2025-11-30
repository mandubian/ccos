# Capability Synthesis Strategy

- Mirror the requested capability id and arguments. Preserve the same semantic intent and return shape.
- Scan provided arguments/context to infer required validations (e.g. guard zero divisors, null strings, empty lists).
- Prefer simple arithmetic, comparisons, and collection helpers from SecureStandardLibrary (`+`, `-`, `*`, `/`, `zero?`, `map`, `filter`, `reduce`, `get`).
- For defensive programming use `(if ...)` or `(match ...)` to short-circuit invalid inputs and emit explanatory maps.
- Keep the implementation referentially transparent: no host calls, global state, or randomness.
- Document the capability through `:name`, `:description`, and `:effects` (default `:pure` unless side effects exist).
- Schemas should describe actual inputs/outputs so downstream validation succeeds.



