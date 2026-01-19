# Auto-Repair RTFS Grammar Hints

- RTFS uses prefix notation; every form is wrapped in parentheses with the operator first (e.g. `(call :foo {:bar 1})`).
- A minimal plan looks like `(plan "plan-name" :body (do (step "name" <expr>) ...))`; `:language rtfs20` is recommended.
- A step is `(step "Description" <expression>)`; the description must be a quoted string.
- `let` uses binding pairs inside a vector: `(let [name expr other expr2] <body>)`; bindings do not escape the current step.
- Maps use keyword keys with colons and no `=`: `{:key value}`; you can nest maps for structured arguments.
- Always double-quote string literals and capability IDs requiring strings; numbers remain bare (no quotes).
- Keywords (map keys) must start with `:` and typically use kebab-case (`:issues`, `:max-element`, `:sort-order`).
- Capability calls use `(call :provider.capability {:param value})` and must supply all required parameters declared in the schema.
- Positional arguments follow the capability name directly: `(call :ccos.math.add 2 3)`; do not wrap them in a map.
- Lambdas require a parameter vector and body: `(fn [acc item] (+ acc item))`; never omit the square brackets.
- Collection helpers such as `map`, `filter`, `reduce`, `sort`, `group-by` expect functions of the correct arity—verify the per-item keyword names.
- Conditionals use `(if <test> <then> <else>)`; multi-way branching uses `(match value pattern result ...)` with even argument count.
- Vectors use square brackets `[elem1 elem2]`; use `(vector ...)` only when constructing dynamically.
- Ensure every `(do ...)`, `(step ...)`, `(let ...)`, and `(call ...)` has balanced parentheses; count manually if needed.

