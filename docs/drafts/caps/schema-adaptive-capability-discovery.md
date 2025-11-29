# Schema-Adaptive Capability Discovery

## Problem Statement

Currently, capability discovery relies too heavily on exact ID matching:
- LLM generates `mcp.filter.filter`, exact match fails, partial match skipped for MCP, new capability synthesized → duplicates
- When output_schema doesn't match input_schema, discovery fails instead of generating an adapter

## Proposed Solution

### 1. Semantic-First Discovery (No ID Dependency)

**Priority Order:**
1. **Semantic matching by role/description** (what capability does)
2. **Schema compatibility** (input_schema/output_schema satisfaction)
3. **Exact/partial ID matching** (fallback only)

**Changes needed:**
- Move semantic matching BEFORE exact match in `discover_capability`
- Remove the `mcp.*` exclusion from partial matching
- Add role-based matching (filter, map, reduce, etc.)

### 2. Schema Adaptation with RTFS Transformers

When a capability's output_schema doesn't exactly match the next capability's input_schema, generate RTFS transformation code:

```rtfs
;; Example: Adapt {:issues [...]} output to {:items [...]} input
(let [result (call :mcp.github.list_issues {:owner "..." :repo "..."})]
  (call :mcp.core.filter {
    :items (get result :issues)
    :predicate (fn [issue] ...)
  }))
```

**Transformation patterns:**
- Key renaming: `:issues` → `:items`
- Type coercion: vector → map, map → vector
- Field extraction: `{:issues [...] :pageInfo {...}}` → `[...]`
- Structure unwrapping: nested maps → flat maps

### 3. Capability Role Detection

Capabilities should declare their **role** (what they do, not just provider):

```rtfs
:metadata {
  :role "filter"  ; or "map", "reduce", "list", "get", etc.
  :input-role "collection"  ; expected role of input
  :output-role "collection"  ; role of output
}
```

Then match by role:
- Need: "filter collection by keyword" → Find all capabilities with `:role "filter"`
- Match by semantic description AND schema compatibility
- Generate adapter if schemas don't match exactly

## Implementation Plan

### Phase 1: Semantic-First Discovery
- [ ] Move semantic matching before exact match
- [ ] Remove MCP exclusion from partial matching
- [ ] Add role-based capability matching

### Phase 2: Schema Adapter Generation
- [ ] Detect schema mismatches (output_schema vs input_schema)
- [ ] Generate RTFS transformation code:
  - Key mapping: `{:issues :items}`
  - Type coercion helpers
  - Structure unwrapping
- [ ] Insert adapter as intermediate step in plan

### Phase 3: Role-Based Matching
- [ ] Add role metadata to capabilities
- [ ] Match by role + description + schema compatibility
- [ ] Prioritize role matches over ID matches

## Benefits

1. **No duplicate capabilities**: Semantic matching finds existing ones
2. **Flexible composition**: Schemas adapt automatically via RTFS transformers
3. **Provider-agnostic**: `mcp.*`, `http.*`, `local.*` all treated equally
4. **Better plans**: Planner can use any capability that matches role/schema, not just exact IDs

