# Auto-Repair Anti-Patterns (avoid)

- Do not wrap the output in markdown fences (no ``` blocks).
- Do not change unrelated parts of the plan.
- Do not rename symbols unless required to fix a concrete error.
- Do not add “helper” steps or extra logging unless explicitly requested.
- Do not invent capability IDs or arguments.
- Do not change RTFS object kinds (e.g., turning a plan into an intent).
- Do not output partial snippets: always return the full repaired source.

