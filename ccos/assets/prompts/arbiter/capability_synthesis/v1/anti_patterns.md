# Capability Synthesis Anti-Patterns

## ⚠️ CRITICAL: RTFS is NOT Clojure!

**RTFS uses a Clojure-like syntax but IS NOT Clojure.** Do NOT use:
- Any `clojure.*` namespace (e.g., `clojure.string/join`, `clojure.core/...`)
- Clojure-specific functions not in RTFS stdlib
- Clojure reader macros or special forms

**If you write Clojure code, it WILL FAIL with "Undefined symbol" errors.**

---

## Unsupported Clojure Syntax

| Unsupported | Clojure Example | RTFS Alternative |
|-------------|-----------------|------------------|
| Namespaced calls | `clojure.string/join` | Use `(reduce str ...)` or manual string building |
| Quote syntax | `'()`, `(quote ...)` | Use `[]` for empty list |
| Atoms/mutation | `atom`, `deref`, `@var` | Not supported - RTFS is pure functional |
| Set literals | `#{1 2 3}` | Use vectors with deduplication |
| Regex literals | `#"pattern"` | Not supported |
| Reader macros | `#(...)`, `#'var` | Not supported |

---

## Clojure Functions NOT in RTFS

**These WILL cause `Undefined symbol` errors:**

| Unsupported | Purpose | RTFS Alternative |
|-------------|---------|------------------|
| `clojure.string/join` | Join strings | `(reduce str items)` or use `call` with println directly |
| `map-indexed` | Map with index | Use `(map (fn [x] ...) coll)` |
| `some` | Check if any match | `(not (empty? (filter pred coll)))` |
| `sort-by` | Sort by key | Use `sort` or `call ccos.data.sort` |
| `inc` | Increment | `(+ n 1)` |
| `dec` | Decrement | `(- n 1)` |
| `str` | Concat strings | Available - this works |
| `apply` | Apply args | Available - this works |
| `partial` | Partial apply | Use inline `(fn [...] ...)` |
| `comp` | Compose fns | Chain function calls |
| `take-while` | Take while pred | Use `filter` |
| `drop-while` | Drop while pred | Use `filter` |

---

## RTFS Stdlib Reference

**Use ONLY these functions:**

```
map, filter, reduce, first, rest, conj, get, assoc, dissoc
count, empty?, nil?, contains?
+, -, *, /, =, not=, <, >, <=, >=
and, or, not
if, let, fn, def, defn
str, format, println
call (for MCP/capability invocation)
```

---

## Other Anti-Patterns

- Do **not** invent external providers, host calls, or HTTP requests
- Do **not** use `(call ...)` in capability implementations - they must be pure RTFS
- Avoid uncontrolled recursion or unbounded loops
- Never omit `:implementation`; the capability will be rejected
