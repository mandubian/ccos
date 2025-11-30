# Capability Synthesis RTFS Grammar Hints

- Emit the canonical capability map, not a function call:  
  `{:type "capability" :id "{capability_id}" :name "..." :description "..." :version "..." ...}`  
  Every string value must be quoted; every key is a keyword (e.g. `:name`, `:description`).
- Always include the required keys: `:type`, `:id`, `:name`, `:description`, `:version`, `:language`, `:permissions`, `:effects`, `:input-schema`, `:output-schema`, and `:implementation`.
- Local capabilities should set `:language "rtfs20"` (or another valid RTFS dialect string).
- Keep `:permissions []` and `:effects [:pure]` unless a genuine side effect is necessary.
- `:implementation` must be an `(fn [input] ...)` that returns structured data using only pure RTFS + SecureStandardLibrary helpers.
- Use maps for structured fields (`{:key value}`) and vectors for ordered collections (`[item1 item2]`).
- Schemas must use valid RTFS type expressions. Common forms:
  - scalars: `int`, `float`, `string`, `bool`, `:number` (any numeric) — do not append `?`; wrap with `[:optional …]` when optional
  - optionals: append `?` to the type (e.g. `string?`, `[:vector string]?`) or use `[:union <type> :nil]`
  - vectors/lists: `[:vector <type>]`
  - maps/records: `[:map [:field <type>] [:other <type>] ...]`
  - unions: `[:union <type-a> <type-b> ...]`
  - tuples: `[:tuple <type-a> <type-b> ...]`
- Use `(let [bindings] body)` for local bindings. Example: `(let [x (get input :field) y (+ x 1)] y)` binds `x` and `y`, then returns `y`.
- Arithmetic and predicates stay prefix: `(+ a b)`, `(/ numerator denominator)`, `(zero? value)`.
- Guard unsafe operations (`/`, `get`, etc.) with `(if ...)` or `(match ...)`, returning error maps when necessary.
- Avoid `(call ...)`, `tool/...`, host I/O, or side-effectful helpers in the implementation.
- Example implementation pattern:
  ```rtfs
  (fn [input]
    (let [field1 (get input :field1)
          field2 (get input :field2)]
      (if (and (string? field1) (number? field2))
        {:result (concat field1 (str field2))}
        {:error "Invalid input types"})))
  ```

