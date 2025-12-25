# RTFS Plan Conversion Task

Convert JSON plan steps into a complete RTFS plan expression.

## Input: JSON Plan Steps
{json_steps}

## Plan Context
- Intent ID: {intent_id}
- Intent Name: {intent_name}
- Plan-level variables: {plan_variables}
- Step dependencies: {step_dependencies}

## Requirements

1. Generate a complete RTFS expression starting with `(do ...)`.

2. Structure the plan as:
   - `(do` wrapper
   - `(let [` bindings block with all steps
   - Each capability step as `step_N (call :capability.id {arguments})`
   - Each logic step (capability_id="rtfs") as `step_N (expression)`
   - Final output map `{...}` extracting outputs from steps

3. Handle step dependencies:
   - Steps reference earlier steps via `(get step_N :output)`
   - Ensure step order respects dependencies
   - Variable references from intent use unquoted symbols

4. SPECIAL: Logic Steps (capability_id="rtfs")
   - If a step has `capability_id: "rtfs"`, do NOT generate a `(call ...)` expression.
   - **CRITICAL**: Do NOT generate a `(rtfs ...)` function call. `rtfs` is NOT a function.
   - Instead, extract the RTFS code from the inputs (key "expression" or "code") and use it DIRECTLY in the let binding.
   - Example JSON: `{ "capability_id": "rtfs", "inputs": { "expression": "(first (get step_0 :content))" } }`
   - Example RTFS: `step_1 (first (get step_0 :content))`
   - BAD Example: `step_1 (rtfs (first (get step_0 :content)))`  <-- NEVER DO THIS
   - Do not verify capabilities for these steps.
   - The result of a logic step is the value itself. When mapping it to the final output, use `step_N` directly, NOT `(get step_N :key)`.

5. Capability Step Conversion (capability_id != "rtfs"):
   - For the capability ID, **ALWAYS PRIORITIZE** the value found in `_suggested_tool` within the `inputs` metadata if it exists.
   - Use the format `(call "id" {arguments})`.
   - **CRITICAL**: Do NOT use a keyword literal like `:capability.id` for the first argument of `call`. Use a **STRING** literal for the capability ID.
   - **NEVER TRUNCATE THE ID**: If the `_suggested_tool` is `mcp.domain/provider.tool_name`, you MUST use that exact string. Do NOT shorten it by removing the namespace prefix.
   - ✅ GOOD: `(call "mcp.domain/provider.tool_name" {...})`
   - ❌ BAD:  `(call "provider.tool_name" {...})`  <-- This will cause "Unknown capability" errors!


6. Argument conversion:
   - String/number/boolean literals: Keep as-is
   - Variables: Convert `{"var": "name"}` → `name` (unquoted symbol)
   - Step outputs: Convert `{"step": "step_1", "output": "issues"}` → `(get step_1 :issues)`
   - RTFS code: Keep `{"rtfs": "..."}` → embed code directly (convert `var::name` to `name`)
   - **CRITICAL**: When converting the `inputs` JSON object to an RTFS map, ensure a direct and exact correspondence: the JSON object's *key* becomes the RTFS map's *keyword*, and the JSON object's *value* (after conversion, e.g., `{"var": "name"}` becomes `name`) becomes the RTFS map's *value*. Maintain this one-to-one mapping without altering the association between a specific key and its original value.
   - **Grounded Parameters**: If a parameter was provided by the grounding pass (check `_suggested_tool` context), use the parameter names that match the tool's expected schema.

7. Parameter fidelity:
   - Use only the parameter names present in the JSON for that step; never invent new inputs (no ad-hoc `:labels`, `:query`, etc.).
   - Preserve the exact spelling/casing of each parameter name.
   - Apply RTFS code only to parameters explicitly marked as functions in the JSON. For all other parameters, use literals, variables, or prior step outputs.
   - If a capability lacks a filter parameter, add a separate filtering step that consumes previous outputs instead of hacking additional parameters into the capability call.
   - Ensure every output or map key you reference can become a keyword: lowercase letters, digits, `_` or `-` only (e.g., `filtered_items` → `:filtered_items`). Rename outputs in the JSON before conversion if needed.

6. Output extraction:
   - Construct a final output map `{ :key value ... }`.
   - Include the output of the FINAL step in the plan.
   - You may omit intermediate outputs (e.g., large raw content from `step_0`) IF they are consumed and transformed by a subsequent step (e.g., `step_1` extracts data from `step_0`).
   - If the plan has only one step, return its output.
   - Sort keys alphabetically.
   - Use `:keyword` format for map keys.

## Output Format

Return ONLY the RTFS expression, wrapped in ```rtfs``` code fence. No prose, no explanations.

Example output:
```rtfs
(do
  (let [
    step_0 (call "mcp.domain/mcp.tool_a" {:query "example"})
    step_1 (call "ccos.data.sort" {:data (get step_0 :items) :key "timestamp"})
  ]
    {
      :results (get step_1 :rows)
    })
)
```

## Additional Rules

1. **ID Precision**: NEVER truncate or modify the capability ID. Use the EXACT string provided in the `_suggested_tool` or `capability_id` fields.
2. **1:1 Mapping**: Every JSON step MUST correspond to exactly ONE `let` binding in the RTFS plan. Do NOT invent intermediate steps unless absolutely necessary for complex data transformation that the JSON already implies.
3. **Keyword Hygiene**: Ensure all field names in `(get ... :field)` and the final output map are in `snake_case`. If you see `camelCase` in the suggested parameters, grounding failed and you should prioritize `snake_case`.
