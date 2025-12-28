# RTFS Plan Grammar Hints

- Generate a complete RTFS plan expression starting with `(do ...)` containing:
  - A `(let [bindings] body)` form with step bindings
  - Each step bound as `step_N (call :capability.id {...})`
  - A final map `{...}` with keyword outputs extracted from steps

- Step bindings format: `step_N (call :capability.id {arguments})`
  - Use `:capability.id` (keyword format) for capability IDs - **NEVER use strings like `"capability.id"`**
  - Arguments are a map `{:param1 value1 :param2 value2}` - **ALL map keys must be keywords, not strings**
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
  - Output names must be lowercase/digit/`-`/`_` slugs so they convert cleanly to RTFS keywords
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
  - `do`, `let`, `call`, `get`, `assoc`, `dissoc`, `if`, `match`, `map`, `filter`, `reduce`, `group-by`, `contains?`, primitive arithmetic, comparisons, and helpers documented in SecureStandardLibrary.
  - Anonymous functions via `(fn [args] body)` when (and only when) the capability parameter expects a function.
  - `rtfs20` literal syntax only; **do not** use `clojure.string`, Common Lisp macros, or any host-language namespaces.

- **Collection Transformation Functions**:
  - `map` - Apply function to each item: `(map (fn [x] ...) collection)`
  - `filter` - Keep items matching predicate: `(filter (fn [x] ...) collection)`
  - `reduce` - Fold collection into single value: `(reduce (fn [acc x] ...) initial collection)`
  - `group-by` - Group items by key function result: `(group-by (fn [x] (get x :field)) collection)`
    - Returns a map where keys are the results of applying key-fn, values are vectors of items
    - Also accepts keywords as key-fn: `(group-by :state issues)` is equivalent to `(group-by (fn [x] (get x :state)) issues)`
    - Example: `(group-by :author issues)` → `{"alice" [issue1 issue3] "bob" [issue2]}`
  - `sort-by` - Sort collection by key function: `(sort-by (fn [x] (get x :date)) items)`

- **Collection Access Functions**:
  - `first` - Get first element: `(first collection)` → first item or `nil`
  - `rest` - Get all but first: `(rest collection)` → remaining items
  - `nth` - Get item by index: `(nth collection 2)` → third item (0-indexed)
  - `last` - Get last element: `(last collection)` → last item or `nil`
  - `count` - Get collection size: `(count collection)` → integer

- **Collection Building Functions**:
  - `conj` - Add item to collection: `(conj items new-item)` → updated collection
  - `concat` - Join collections: `(concat list1 list2)` → combined list
  - `flatten` - Flatten nested lists: `(flatten [[1 2] [3 4]])` → `[1 2 3 4]`
  - `distinct` - Remove duplicates: `(distinct [1 2 1 3])` → `[1 2 3]`
  - `take` - Take first N items: `(take 5 collection)` → first 5 items
  - `drop` - Drop first N items: `(drop 5 collection)` → items after first 5
  - `reverse` - Reverse collection: `(reverse collection)` → reversed
  - `keys` - Get map keys: `(keys {:a 1 :b 2})` → `[:a :b]`
  - `vals` - Get map values: `(vals {:a 1 :b 2})` → `[1 2]`

- **Collection Predicates**:
  - `empty?` - Check if empty: `(empty? collection)` → `true/false`
  - `contains?` - Check if key exists: `(contains? map :key)` → `true/false`
  - `nil?` - Check if nil: `(nil? value)` → `true/false`

- **Math Functions**:
  - `+`, `-`, `*`, `/` - Basic arithmetic
  - `max` - Maximum value: `(max 1 5 3)` → `5`
  - `min` - Minimum value: `(min 1 5 3)` → `1`
  - `abs` - Absolute value: `(abs -5)` → `5`
  - `mod` - Modulo: `(mod 10 3)` → `1`
  - `inc` - Increment by 1: `(inc 5)` → `6`
  - `dec` - Decrement by 1: `(dec 5)` → `4`
  - `sqrt` - Square root: `(sqrt 16)` → `4.0`
  - `pow` - Power: `(pow 2 3)` → `8`

- **Type Conversion Functions**:
  - `parse-int` - Parse string to integer: `(parse-int "123")` → `123`. Returns `nil` for invalid input.
  - `parse-float` - Parse string to float: `(parse-float "3.14")` → `3.14`. Returns `nil` for invalid input.
  - `int` - Coerce to integer: `(int "42")` → `42`, `(int 3.7)` → `3`, `(int true)` → `1`. Returns `0` for invalid input.
  - `float` - Coerce to float: `(float "3.14")` → `3.14`, `(float 42)` → `42.0`. Returns `0.0` for invalid input.
  - **Use these when user input comes as strings**: `(parse-int (get step_1 :perPage))` or `(int step_1)`

- **RTFS String Functions** (use these, not namespace syntax):
  - `string-contains` - Check if a string contains a substring: `(string-contains haystack needle)`
  - `string-lower` - Convert string to lowercase: `(string-lower str)`
  - `string-upper` - Convert string to uppercase: `(string-upper str)`
  - `string-length` - Get string length: `(string-length str)`
  - `string-trim` - Remove whitespace: `(string-trim str)`
  - `str` - Convert to string: `(str value)`
  - `split` - Split string by separator: `(split "a/b/c" "/")` → `["a" "b" "c"]`
  - `join` - Join collection with separator: `(join ", " ["a" "b"])` → `"a, b"`
  - `substring` - Extract substring: `(substring str start end)`
  - `starts-with?` - Check prefix: `(starts-with? "hello" "hel")` → `true`
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

- **LLM Capabilities** (for text generation, summarization, analysis):
  - `ccos.llm.generate` - Generate text using LLM with prompt sanitization
    - Use for: summarization, analysis, extraction, translation, explanation
    - Parameters:
      - `:prompt` (required) - The instruction/question for the LLM
      - `:context` (optional) - Data to include (serialize with `str` if needed)
      - `:max_tokens` (optional, default 4096) - Maximum tokens to generate
      - `:temperature` (optional, default 0.3) - Sampling temperature
    - Returns: Generated text string, or map with `:approval_required true` if prompt needs human approval
    - Example:
    ```rtfs
    step_2 (call :ccos.llm.generate {
      :prompt "Summarize these issues into a list of action items"
      :context (str step_1)
    })
    ```
    - **USE THIS** when the goal requires summarization, analysis, or generating human-readable text
    - **DO NOT** try to summarize with inline RTFS - use this capability instead

