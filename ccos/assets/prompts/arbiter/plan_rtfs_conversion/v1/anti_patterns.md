# RTFS Plan Conversion Anti-Patterns

- **Do NOT** use `(plan ...)` special form - use `(do ...)` with `(let [...])` bindings.

- **Do NOT** quote variable names - use symbols not strings: `user` not `"user"`.

- **Do NOT** use nested `(let ...)` - use a single `(let [step_0 ... step_1 ...] body)`.

- **Do NOT** forget step indices - always use `step_0`, `step_1`, etc. based on step order.

- **Do NOT** mix step ID strings with step indices - use numeric indices consistently.

- **Do NOT** forget keyword format for outputs - use `:output` not `"output"` or `output`.
- **Do NOT** keep spaces or camelCase in output names - convert to lowercase slug so the final keyword is valid (e.g., `"Filtered Items"` → `filtered-items` → `:filtered-items`).

- **Do NOT** forget `(get step_N :output)` - must extract from step bindings, not reference directly.

- **Do NOT** use capability IDs as strings - convert to keywords: `:capability.id` not `"capability.id"`.

- **Do NOT** use strings for map keys - always use keywords: `{:key value}` not `{"key" value}`.

- **Do NOT** use strings for capability IDs in calls - always use keywords: `(call :capability.id {...})` not `(call "capability.id" {...})`.

- **Do NOT** use strings for output keys in `(get ...)` - always use keywords: `(get step_0 :output)` not `(get step_0 "output")`.

---

## ⚠️ CRITICAL: RTFS is NOT Clojure!

**RTFS uses Clojure-like syntax but IS NOT Clojure.** The following WILL FAIL:

| ❌ Unsupported | ❌ Example | ✅ RTFS Alternative |
|----------------|-----------|---------------------|
| `clojure.*` namespaces | `clojure.string/join` | `(reduce str items)` |
| `map-indexed` | `(map-indexed ...)` | `(map (fn [x] ...) coll)` |
| `inc` / `dec` | `(inc n)` | `(+ n 1)` / `(- n 1)` |
| `some` | `(some pred coll)` | `(not (empty? (filter pred coll)))` |
| `sort-by` | `(sort-by :key coll)` | Use `call ccos.data.sort` |
| `apply str` | `(apply str xs)` | `(reduce str xs)` |

**RTFS stdlib:** `map`, `filter`, `reduce`, `first`, `rest`, `conj`, `get`, `assoc`, `count`, `empty?`, `nil?`, `str`, `+`, `-`, `*`, `/`, `=`, `not=`, `<`, `>`, `and`, `or`, `not`, `if`, `let`, `fn`, `call`

---

- **Do NOT** use namespace syntax for string functions - use RTFS standard library functions:
  - ❌ `string/includes?`, `string/lower`, `string/upper` (Clojure namespace syntax)
  - ❌ `clojure.string/includes?`, `clojure.string/lowercase` (Clojure library syntax)
  - ✅ `string-contains`, `string-lower`, `string-upper` (RTFS standard library)

- **Do NOT** use dot notation for map access:
  - ❌ `issue.title`, `step_0.issues` (property access syntax)
  - ✅ `(get issue :title)`, `(get step_0 :issues)` (RTFS map access)

- **Do NOT** invent new capability parameters (e.g., adding `:labels`, `:query`, `:filter` if they are absent from the JSON step definition).

- **Do NOT** embed `{"rtfs": "..."}` for inputs that are not declared as functions. Reserve RTFS embeddings exclusively for explicit function parameters.

- **Do NOT** try to "patch" missing behavior by overloading unrelated parameters. If a capability lacks filtering, add a separate filtering step instead.

