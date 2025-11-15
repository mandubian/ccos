# RTFS Plan Grammar Hints

- Generate a complete RTFS plan expression starting with `(do ...)` containing:
  - A `(let [bindings] body)` form with step bindings
  - Each step bound as `step_N (call :capability.id {...})`
  - A final map `{...}` with keyword outputs extracted from steps

- Step bindings format: `step_N (call :capability.id {arguments})`
  - Use `:capability.id` (keyword format) for capability IDs
  - Arguments are a map `{:param1 value1 :param2 value2}`
  - Values can be: literals, symbols (variables), or `(get step_N :output)` expressions

- Value types in arguments:
  - String literals: `"text"`
  - Number literals: `123`, `45.67`
  - Boolean literals: `true`, `false`
  - Variables: `symbol_name` (unquoted symbol)
  - Step outputs: `(get step_N :output_name)` where N is the step index and output_name is a keyword
  - RTFS code: Embed directly for function parameters (e.g., `(fn [item] ...)`)

- Final output map format: `{:output_name (get step_N :output_name) ...}`
  - Use keywords (`:output`) for map keys
  - Extract from steps using `(get step_N :output)` 
  - Sort outputs alphabetically by keyword name

- Example structure:
```rtfs
(do
  (let [
    step_0 (call :storage.fetch {
      :bucket "data-bucket"
      :key "users.csv"
    })
    step_1 (call :transform.parse {
      :data (get step_0 :content)
      :format "csv"
    })
    step_2 (call :mcp.core.filter {
      :items (get step_1 :records)
      :predicate (fn [record] (> (get record :age) 18))
    })
  ]
    {
      :adults (get step_2 :filtered_items)
      :all_records (get step_1 :records)
      :raw_data (get step_0 :content)
    })
)
```

- Example with string filtering (correct RTFS syntax):
```rtfs
step_1 (call :mcp.core.filter {
  :items (get step_0 :issues)
  :predicate (fn [issue] (string-contains (string-lower (str (get issue :title))) "rtfs"))
})
```

Note: The predicate uses:
- `string-contains` (not `string/includes?` or `clojure.string/includes?`)
- `string-lower` (not `string/lower` or `clojure.string/lowercase`)
- `(get issue :title)` (not `issue.title`)
- `str` to convert values to strings when needed

- Allowed RTFS constructs:
  - `do`, `let`, `call`, `get`, `assoc`, `dissoc`, `if`, `match`, `map`, `filter`, `reduce`, `contains?`, primitive arithmetic, comparisons, and helpers documented in SecureStandardLibrary.
  - Anonymous functions via `(fn [args] body)` when (and only when) the capability parameter expects a function.
  - `rtfs20` literal syntax only; **do not** use `clojure.string`, Common Lisp macros, or any host-language namespaces.

- **RTFS String Functions** (use these, not namespace syntax):
  - `string-contains` - Check if a string contains a substring: `(string-contains haystack needle)`
  - `string-lower` - Convert string to lowercase: `(string-lower str)`
  - `string-upper` - Convert string to uppercase: `(string-upper str)`
  - `string-length` - Get string length: `(string-length str)`
  - `string-trim` - Remove whitespace: `(string-trim str)`
  - `str` - Convert to string: `(str value)`
  - **DO NOT use**: `string/includes?`, `string/lower`, `clojure.string/includes?`, or any namespace syntax
  - **DO NOT use**: Property access like `issue.title` - use `(get issue :title)` instead

- **Map/Record Access**:
  - Always use `(get map :key)` to access map values, not dot notation
  - Example: `(get issue :title)` not `issue.title`
  - Example: `(get step_0 :issues)` not `step_0.issues`

- Never invent capability parameters:
  - Every `:param` used inside `{...}` must appear in the JSON step definition for that capability.
  - If a capability needs filtering but has no function parameter, emit a separate filtering step that uses a capability which does expose a predicate input.

- Variable naming:
  - Plan-level variables (from intent) are unquoted symbols: `user`, `project`, `filter`
  - Step outputs use `:keyword` format when accessing
  - Step indices are numeric: `step_0`, `step_1`, etc.

- Capability ID sanitization:
  - Convert dots to colons for keywords: `mcp.github.list_issues` → `:mcp.github.list_issues`
  - Keep capability ID format consistent with RTFS keyword syntax

