# Capability Synthesis Anti-Patterns

- Do **not** invent new external providers, host calls, HTTP requests, or MCP usage.
- Do **not** invoke `(call …)` or any `tool/…` helper — implementations must be pure RTFS.
- Avoid returning strings like `"TODO"` or `"error"`; return structured maps or numbers that match the intended schema.
- Do **not** change the requested capability id or rename expected outputs.
- Avoid uncontrolled recursion, unbounded loops, or large intermediate vectors.
- Never omit `:implementation`; the capability will be rejected without it.

