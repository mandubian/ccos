# Grounded Decomposition Anti-Patterns

## ⚠️ CRITICAL: RTFS is NOT Clojure!

**If you use Clojure functions, the plan WILL FAIL with "Undefined symbol" errors.**

| ❌ DO NOT USE | ❌ Example | ✅ USE INSTEAD |
|---------------|-----------|----------------|
| `clojure.*` namespaces | `clojure.string/join` | `(reduce str items)` |
| `map-indexed` | `(map-indexed (fn [i x] ...))` | `(map (fn [x] ...) coll)` |
| `inc` / `dec` | `(inc n)` | `(+ n 1)` |
| `some` | `(some pred coll)` | `(not (empty? (filter pred coll)))` |
| `sort-by` | `(sort-by :key coll)` | `(call "ccos.data.sort" {...})` |
| `partial` | `(partial f arg)` | `(fn [x] (f arg x))` |
| `apply` | `(apply str xs)` | `(reduce str xs)` |

## Other Anti-Patterns

- **Do NOT** force-fit a tool when none matches - use `"tool": null` instead
- **Do NOT** duplicate API calls - fetch once, transform with data_transform steps
- **Do NOT** invent parameter values - ask the user with user_input step
- **Do NOT** use string step descriptions in "depends_on" - use numeric indices only
