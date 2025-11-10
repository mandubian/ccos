# Auto-Repair Strategy

- Study the compiler diagnostics to understand whether the fault is a parse error, type error, or runtime exception.
- Repair the RTFS structure with the smallest possible change that satisfies the diagnostics.
- Preserve the original intent, step names, and data flow of the plan.
- Prefer core forms such as `let`, `if`, `do`, arithmetic ops, predicates, and list helpers before reaching for capabilities.
- Available local capabilities in the demo environment: `:core.echo` (expects `{:message string}`), `:core.math.add` (integer addition), `:ccos.io.println`, `:ccos.io.log`.
- Keep the plan pure and deterministic; avoid side effects outside the existing capabilities.

