You must output a single RTFS 2.0 capability form:

- Use `(capability "<id>" ...)`
- Include fields: `:name`, `:description`, `:version`, `:language "rtfs20"`, `:permissions`, `:effects`, `:input-schema`, `:output-schema`, `:implementation`
- `:effects` should be `[:pure]` for data transforms/output formatting (no side effects).
- `:input-schema` / `:output-schema` must be precise RTFS schema forms (e.g., `[:map [:data [:vector :any]]]`). AVOID generic types like `:any` or `:map` without fields if the structure is known.
- `:metadata` map MUST be present and include:
    - `:sample-input`: an RTFS-formatted string of a representative input (e.g., `"{:data [...]}"`).
    - `:sample-output`: an RTFS-formatted string of the expected output (e.g., `"{:count 1}"`).
- Implementation must be a pure `(fn [input] ...)` using standard RTFS functions (`map`, `filter`, `reduce`, `group-by`, `sort`, `get`, `assoc`, etc.).
- Do not call host capabilities inside the implementation (no `(call ...)`).
- Prefer safe defaults: if input is missing or of wrong type, return `{}` or `[]` instead of failing.










