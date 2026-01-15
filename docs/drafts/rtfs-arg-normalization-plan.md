# RTFS Argument Normalization Plan (Draft)

## Goal
Allow capabilities to be called with either:
- positional arguments (tuple/vector)
- named arguments (map with keyword keys)

…while keeping schemas precise and avoiding breaking changes.

## Current Situation
- Capabilities use strict `Arity` checks before type validation.
- Schemas can represent either tuple or map shapes, but not both without a union.
- There is no general RTFS rule to auto-map positional args to named fields.

## Design Options

### Option A: RTFS-Level Normalization (Language Semantics)
Add a runtime rule that, when a function expects a **map input schema**, positional args
can be normalized into a map before validation/execution.

**Proposed rule (example):**
- If the function has a map schema with required fields in a defined order,
  and the call provides a tuple/vector/list of matching length,
  then map positional args to those fields.
  Example: `{:handle Int :line String}` maps from `(handle line)`.

**Pros**
- Works for any capability/function with map schemas.
- Callers get flexible syntax automatically.

**Cons**
- Changes RTFS runtime semantics (wider impact).
- Needs clear rules for field order and optional fields.
- Potential ambiguity if schema includes wildcard or optional keys.

### Option B: Capability-Level Normalization (Boundary Adapter)
Normalize in the capability layer (registry/provider) and keep RTFS unchanged.

**Pros**
- Contained change, minimal global impact.
- Explicit control per capability.

**Cons**
- Requires per-capability implementation or shared helper.
- Not “pure RTFS” behavior.

### Option C: Hybrid
Add a **helper** in RTFS runtime (library function) but keep default semantics unchanged.
Capabilities can opt into normalization by calling the helper before validation.

## Detailed Proposal (Option A)

### 1) Schema Introspection
Add a helper to derive a **field order** from `TypeExpr::Map` entries:
- Only use non-optional entries
- Preserve declaration order
- Reject if wildcard exists or optional fields are interleaved (to avoid ambiguity)

### 2) Normalization Step
At call execution:
1. If args is a list/vector/tuple AND function input schema is map
2. If args length matches number of required fields
3. Convert to map `{ :field_i -> arg_i }`
4. Continue validation/execution with normalized input

### 3) Arity Handling
Adjust arity checks to allow:
- single map argument OR
- positional args matching required fields count

### 4) Error Reporting
If normalization fails, return explicit error:
- expected map with keys X/Y
- or positional args of length N

## Edge Cases
- Map with optional fields
- Map with wildcard entries
- Union schemas (map | tuple)
- Variadic functions

## Migration Plan
1. Add normalization helper and tests for map-only schema
2. Gate behavior behind a feature flag (e.g. `RTFS_ARG_NORMALIZE`)
3. Migrate selected capabilities (`ccos.io.write-line`, etc.)
4. Remove capability-level adapters once RTFS-level behavior is stable

## Testing Plan
- Unit tests for:
  - tuple → map normalization
  - map passthrough
  - errors on wrong length
  - optional field behavior
- Integration tests with representative capabilities
- Ensure no behavior change for tuple-only schemas

## Recommendation
Start with **Option B** (capability-level adapter) for immediate UX,
then implement **Option A** if we want consistent RTFS semantics.
