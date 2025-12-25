# RTFS Step Grammar

## RTFS Step References

Use this syntax when a param needs output from a previous step:

- Reference step output variable: `step_0`, `step_1`, `step_2`... (0-indexed)
- Access map key: `(get step_0 :issues)`
- Access array element: `(nth step_0 0)`
- Chained access: `(get (nth (get step_0 :items) 0) :number)`

**Examples:**
- If step 1 filters issues from step 0, use: `"data": "step_0"`
- To get issue number from first item: `"issue_number": "(get (nth step_0 0) :number)"`

## RTFS Syntax Rules (CRITICAL - RTFS is NOT Clojure!)

**DO NOT USE:**
- Namespace prefixes: use `"split"` NOT `"clojure.string/split"`
- `clojure.anything` namespaced calls
- `map-indexed` → use `(map (fn [x] ...) coll)` instead
- `inc` / `dec` → use `(+ n 1)` / `(- n 1)` instead
- `some` → use `(not (empty? (filter pred coll)))` instead
- `sort-by` → use `call ccos.data.sort` instead
- `apply str` → use `(reduce str items)` instead
- Regex literals: use plain strings `"pattern"` NOT `#"pattern"`
- Anonymous function syntax: use `(fn [x] ...)` NOT `#(...)`

**ONLY USE these RTFS functions:**
`get`, `map`, `filter`, `reduce`, `first`, `rest`, `count`, `empty?`, `str`, `+`, `-`, `*`, `/`, `=`, `not=`, `<`, `>`, `and`, `or`, `not`, `if`, `let`, `fn`, `call`

**Keywords use colons:** `:name`, `:issues`, `:body` (not 'name or "name" for map keys)
