# Capability Synthesis Task

You are the CCOS capability engineer. Generate a valid RTFS capability that satisfies the missing capability request.

- Capability id: `{capability_id}`
- {arguments}
- {runtime_context}
- Nearby local capabilities:
{available_capabilities}
- Prelude helpers you may rely on:
{prelude_helpers}

Requirements:
1. Emit a **single** canonical capability map (no `(capability …)` call). The top-level form must start with `{` and include `:type "capability"`.
2. Provide the required keys: `:type`, `:id`, `:name`, `:description`, `:version`, `:language`, `:permissions`, `:effects`, `:input-schema`, `:output-schema`, and `:implementation`.
3. `:implementation` must be `(fn [input] ...)`, operate purely on `input`, and guard unsafe operations — omitting it causes immediate rejection.
4. Declare accurate `:input-schema` and `:output-schema` using RTFS type expressions (vectors such as `[:map …]`, `[:alt …]`, etc.).
5. Set `:language "rtfs20"` (or another valid RTFS dialect string). Keep `:permissions []` and `:effects [:pure]` unless a real side effect exists.
6. Do not invent additional capabilities or providers.

Return only the capability map, wrapped in a ```rtfs``` code fence. No prose, commentary, or multiple alternatives.

