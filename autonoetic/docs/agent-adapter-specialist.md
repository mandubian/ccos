# Agent Adapter Specialist (`agent-adapter.default`)

This document explains how the adapter specialist works, with emphasis on the
Python scripts it uses to compare schemas and generate wrapper agents.

## Purpose

`agent-adapter.default` is an evolution-layer specialist that creates a
**wrapper agent** around an existing specialist when:

- the base specialist is close to the requested role, but
- I/O schema or behavior shape does not match the target contract.

The wrapper keeps the base behavior reusable while introducing deterministic
input/output remapping middleware where needed.

## High-Level Flow

1. Receive:
   - `base_agent_id`
   - `target_spec` (target accepts/returns schemas and adaptation intent)
   - optional `rationale`
2. Read base `SKILL.md` and extract base manifest/schema metadata.
3. Run `schema_diff.py` to detect compatibility and mapping needs.
4. Run `generate_wrapper.py` to produce wrapper artifacts.
5. Register wrapper via `agent.install`.
6. Return wrapper id and mapping summary.

## Files in Adapter Bundle

- `agents/evolution/agent-adapter.default/SKILL.md`
- `agents/evolution/agent-adapter.default/runtime.lock`
- `agents/evolution/agent-adapter.default/scripts/schema_diff.py`
- `agents/evolution/agent-adapter.default/scripts/generate_wrapper.py`

## Script: `schema_diff.py`

Compares base and target I/O schemas and emits mapping requirements.

### Input (stdin JSON)

```json
{
  "base_accepts": {},
  "base_returns": {},
  "target_accepts": {},
  "target_returns": {}
}
```

All fields may be `null`.

### Output (stdout JSON)

```json
{
  "accepts_compatible": false,
  "returns_compatible": false,
  "requires_input_mapping": true,
  "requires_output_mapping": true,
  "input_mappings": [
    {"from": "task", "to": "query"},
    {"from": "topic", "to": "domain"}
  ],
  "output_mappings": [
    {"from": "result", "to": "summary"},
    {"from": "score", "to": "confidence"}
  ],
  "notes": ["..."]
}
```

### Mapping heuristic

The script infers deterministic mappings when both sides are object schemas with
required fields:

- same-name fields are paired first (`x -> x`),
- remaining fields are paired by required-field order
  (`target_required[i] -> base_required[i]`).

In that case:

- input mappings are inferred as `target_required -> base_required`,
- output mappings are inferred similarly and reversed by generator in post-map.

If schemas are missing or types mismatch, mappings can be empty and notes explain
why manual refinement is required.

## Script: `generate_wrapper.py`

Generates wrapper agent files from base skill text + schema diff metadata.

### CLI arguments

- `--base-skill <path>`: path to base skill markdown.
- `--wrapper-id <id>`: generated wrapper agent id.
- `--target-spec-json <json>`: wrapper target I/O schema object.
- `--schema-diff-json <json>`: output from `schema_diff.py`.
- `--base-manifest-json <json>` (optional): used to inherit base capabilities.
- `--output-dir <path>` (optional): writes generated files to disk.

### Generated files

Always:

- `SKILL.md`
- `runtime.lock`

Conditionally (when mapping is needed):

- `scripts/pre_map.py`
- `scripts/post_map.py`

### Middleware behavior in generated scripts

- `pre_map.py`:
  - reads completion request JSON from stdin,
  - parses the last user message content as JSON,
  - applies all inferred input mappings (`from -> to`) when possible.
- `post_map.py`:
  - reads completion response JSON from stdin,
  - parses `response.text` as JSON,
  - applies all inferred output mappings in reverse (`to -> from`) so caller
    receives target shape.

If parsing fails, scripts are fail-soft and pass data through unchanged.

## Capability Inheritance

When `--base-manifest-json` includes `capabilities`, the generated wrapper
places those capabilities into wrapper frontmatter so wrapper policy remains
compatible with the base specialist security envelope.

## Runtime Notes

- Middleware runs relative to the wrapper agent directory.
- Wrapper generation is deterministic for the currently implemented schema diff
  and required-field mapping strategy.
- For complex schema transforms (nested objects, arrays, one-to-many mappings),
  manual refinement of generated scripts is expected.

## Validation

Current tests covering adapter script behavior:

- `autonoetic-gateway/tests/agent_adapter_scripts_integration.rs`
- `autonoetic-gateway/tests/agent_adapter_wrapper_integration.rs`
