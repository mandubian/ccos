# Auto-Repair RTFS Grammar Hints

- RTFS uses prefix notation; every form is wrapped in parentheses with the operator first (e.g. `(call :foo {:bar 1})`).
- A minimal plan looks like `(plan :body (do (step "name" <expr>) ...))`; include `:language rtfs20` when possible.
- Steps have the shape `(step "Description" <expression>)`; the description must be a quoted string.
- `let` bindings live in the current step only and use pairs in a vector: `(let [name expr other expr2] <body>)`.
- Maps require keyword keys and colons: `{:key value}`; never use `=`, `=>`, or bare identifiers for map entries.
- Double-quote every string literal and capability parameter that expects text; keep numbers unquoted.
- Keywords begin with `:` and typically use kebab-case such as `:issues`, `:max-element`, or `:sort-order`.
- Capability calls use `(call :provider.capability {:param value})` and must match the schema’s required parameters.
- Positional capability arguments follow the capability name directly, e.g. `(call :ccos.math.add 2 3)`; do not wrap positional args in a map.
- Lambdas require a parameter vector and body: `(fn [acc item] (+ acc item))`; never omit the surrounding brackets.
- Collection helpers (`map`, `filter`, `reduce`, `sort`, `group-by`, etc.) expect functions with the correct arity—double-check per-item parameter names.
- `if` forms take three expressions `(if test then else)`; multi-way branching uses `(match value pattern result ...)` with an even argument count.
- Use square brackets for vectors `[elem1 elem2]`; prefer `(vector ...)` only for dynamic construction.
- Ensure every `(do ...)`, `(step ...)`, `(let ...)`, and `(call ...)` has balanced parentheses; count manually if unsure.


