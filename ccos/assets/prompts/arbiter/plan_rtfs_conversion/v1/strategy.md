# RTFS Plan Conversion Strategy

- **Parse JSON structure**: Extract steps, their capability IDs, inputs, outputs, and dependencies.

- **Build step bindings**: Create `step_N (call :capability.id {...})` for each step in order, respecting dependencies.

- **Convert arguments**:
  - Literals: Preserve strings, numbers, booleans
  - Variables: Convert `{"var": "name"}` → unquoted symbol `name`
  - Step references: Convert `{"step": "step_1", "output": "output"}` → `(get step_1 :output)`
  - RTFS code: Embed directly, convert `var::name` → `name`

- **Respect capability schemas**:
  - Use only the keys present in the JSON `inputs` for each capability; if the JSON lacks a parameter, do not create it.
  - If a capability does not expose the needed behavior (e.g., filtering), emit an additional step that uses a capability designed for that action.
  - Only embed RTFS code for parameters explicitly identified as function inputs.

- **RTFS-only syntax**:
  - Emit pure RTFS constructs (`do`, `let`, `call`, `get`, `if`, `fn`, etc.).
  - Never call `clojure.*`, Common Lisp macros, or host language helpers.

- **Extract outputs**: Collect all outputs from all steps, sort alphabetically, create final map.

- **Validate structure**: Ensure all step references are valid, all outputs are accessible, all variables are declared.

