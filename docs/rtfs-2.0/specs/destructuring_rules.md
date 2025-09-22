# RTFS Destructuring Rules (Spec)

This document defines exactly what destructuring patterns are allowed in RTFS, where they are allowed, and the expected behaviors and errors. It reflects the current compiler/runtime implementation and is covered by deterministic tests in `rtfs_compiler/tests/shared/rtfs_files/features/destructuring_rules.rtfs`.

## Summary

- Allowed in bindings: `let` and function parameters (defn/fn) support symbol, vector, and map destructuring, and wildcard `_`.
- Variadic parameter: only a simple symbol is allowed after `&`. Destructuring after `&` is not supported.
- Lambda special form: parameters must be symbols. No destructuring directly in the lambda parameter list; destructure inside the body instead.
- Fixed-position vector destructuring binds exactly the provided positions. It does not absorb extra elements; arity of the function stays based on parameter count.

## Where destructuring is supported

- let bindings: the left-hand side `binding_pattern` can be:
  - Symbol: binds the entire value
  - Vector pattern: `[a b]`, nested allowed, supports `& rest`
  - Map pattern: `{ :keys [k1 k2] ... }` and explicit `{:key k}` entries; nested allowed, supports `:as sym`
  - Wildcard `_`: binds nothing

- defn/fn parameters: each fixed parameter can be any `binding_pattern` from above. This includes nested vector/map patterns and `_`.
  - Implementation detail: destructuring parameters are compiled into hidden parameters and a Destructure prologue at the top of the function body. Pattern-bound symbols are predeclared so the body can reference them.

## Variadic parameter rules

- Syntax: `(defn f [p1 p2 & rest] ...)`
- Constraint: `rest` MUST be a symbol. Patterns like `[& [a b]]` or `[& {:keys [k]}]` are not allowed.
- Rationale: The IR runtime expects a single rest binding; destructuring of rest can be achieved in the body via a separate destructure form if needed.
- Error: attempting destructuring after `&` results in an InvalidSpecialForm/variadic parameter error.

## Lambda special form

- Syntax: `(lambda [params] body...)`
- Constraint: parameters must be symbols. Destructuring in the parameter list is not allowed.
- Alternative: destructure inside the body with a `let`.
- Error:
  - IR conversion: using a non-symbol parameter yields `InvalidSpecialForm: lambda parameters must be symbols`.
  - AST evaluator: `lambda` is not a recognized special form, so `(lambda ...)` produces `UndefinedSymbol(Symbol("lambda"))` at runtime.

## Deterministic examples

- Vector param destructuring:
  - `(defn add-pair [[a b]] (+ a b))` with `[1 2]` → `3`
  - Nested: `(defn head-tail [[h & t]] [h (count t)])` with `[10 20 30 40]` → `[10 3]`
- Map param destructuring:
  - `(defn greet [{:keys [name title]}] (str title ": " name))` with `{:name "Ada" :title "Dr"}` → `"Dr: Ada"`
- Wildcard param:
  - `(defn second-item [_ y] y)` with `(1 42)` → `42`
- Variadic symbol only:
  - `(defn collect [x & rest] (cons x rest))` with `1 2 3` → `[1 2 3]`
  - Disallowed: `(defn bad [x & [a b]] ...)` → error
- Lambda params must be symbols:
  - `(lambda [[a b]] (+ a b))` → error; use `(lambda [pair] (let [[a b] pair] (+ a b)))`

## Error behaviors

- Destructuring after `&`: InvalidSpecialForm/"variadic parameter must be a symbol"
- Lambda non-symbol param: InvalidSpecialForm/"lambda parameters must be symbols"
- Arity mismatches are reported when the overall argument count doesn’t match fixed parameter count and variadic rules. Vector param patterns don’t change the function’s arity.

### Exact error message examples

- Destructuring after `&` (variadic):
  - Error kind: `InvalidSpecialForm`
  - Message includes: `variadic parameter must be a symbol`
  - Tests use a regex to accept variations: `InvalidSpecialForm|variadic parameter must be a symbol|Invalid.*variadic`

- Lambda with non-symbol parameter:
  - IR conversion error: `InvalidSpecialForm { form: "lambda", message: "lambda parameters must be symbols" }`
  - AST runtime error: `UndefinedSymbol(Symbol("lambda"))`

- Vector destructuring length mismatch (no rest binding):
  - Error kind: `TypeError`
  - Shape: `TypeError { expected: "vector with exactly N elements", actual: "vector with M elements", operation: "vector destructuring" }`
  - Example (N=2, M=3): `expected: "vector with exactly 2 elements", actual: "vector with 3 elements"`

## Notes for implementers

- Parser: `binding_pattern` supports `_`, symbol, vector, and map patterns. Variadic in `fn_param_list` accepts `& symbol [: type]?` only.
- Converter: destructuring params are lowered into hidden params plus an `IrNode::Destructure` prologue. Pattern-bound symbols are predeclared to avoid undefined symbol errors. Variadic param supports only `Pattern::Symbol`.
- IR runtime: apply remains simple—no param-pattern matching at call time. Destructuring is executed inside the function body prologue, preserving performance.

## Test coverage

See `tests/shared/rtfs_files/features/destructuring_rules.rtfs` which exercises:
- Allowed: symbol params, vector/map destructuring (nested), wildcard, variadic symbol, and in-body destructure for lambdas.
- Disallowed: destructuring after `&` and non-symbol lambda parameters.
- Edge: vector pattern not absorbing extra list items (arity remains based on param count).

## Quick reference (copy/paste examples)

- Variadic after & (disallowed):

  (defn bad-collect [x & [a b]] [x a b])
  (bad-collect 1 2 3)
  ;; Expected: ERROR: InvalidSpecialForm|variadic parameter must be a symbol|Invalid.*variadic

- Lambda params must be symbols (disallowed destructuring):

  (lambda [[a b]] (+ a b))
  ;; Expected: ERROR: lambda parameters must be symbols

- Vector destructuring length mismatch (no rest binding):

  (do (defn first-two [[a b]] a)
    (first-two [1 2 3]))
  ;; Expected: ERROR: TypeError { expected: "vector with exactly 2 elements", actual: "vector with 3 elements", operation: "vector destructuring" }
