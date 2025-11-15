# RTFS Plan Conversion Anti-Patterns

- **Do NOT** use `(plan ...)` special form - use `(do ...)` with `(let [...])` bindings.

- **Do NOT** quote variable names - use symbols not strings: `user` not `"user"`.

- **Do NOT** use nested `(let ...)` - use a single `(let [step_0 ... step_1 ...] body)`.

- **Do NOT** forget step indices - always use `step_0`, `step_1`, etc. based on step order.

- **Do NOT** mix step ID strings with step indices - use numeric indices consistently.

- **Do NOT** forget keyword format for outputs - use `:output` not `"output"` or `output`.

- **Do NOT** forget `(get step_N :output)` - must extract from step bindings, not reference directly.

- **Do NOT** use capability IDs as strings - convert to keywords: `:capability.id` not `"capability.id"`.

- **Do NOT** call `clojure.*`, `common-lisp.*`, or any host-language library. Only RTFS forms and SecureStandardLibrary helpers are permitted.

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

