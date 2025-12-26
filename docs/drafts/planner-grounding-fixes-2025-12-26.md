# Planner Grounding Fixes - 2025-12-26

## Completed Tasks

### ✅ Schema Visibility Fix
- **File**: `ccos/src/mcp/core.rs`
- **Issue**: When loading MCP tools from RTFS cache, `input_schema_json` was set to `None`, preventing `format_tool_for_prompt` from extracting required parameter names.
- **Fix**: Convert `TypeExpr` to JSON Schema via `to_json()` when creating `DiscoveredMCPTool` from RTFS cache.
- **Result**: Tool prompts now include `required_params="owner, repo"`, giving the LLM clear visibility of required parameters.

### ✅ Prompt Template Hardening
- **File**: `ccos/assets/prompts/arbiter/plan_rtfs_conversion/v1/task.md`
- **Issue**: LLM was truncating capability IDs (e.g., `mcp.github/github-mcp.list_issues` → `github-mcp.list_issues`).
- **Fix**: Added explicit GOOD/BAD examples with generic domain names (`mcp.domain/provider.tool_name`).

### ✅ Domain-Specific Code Cleanup
- **File**: `ccos/src/planner/modular_planner/safe_executor.rs`
- **Action**: Removed all domain-specific parameter normalization (owner/repo splitting, q→query aliasing).
- **Rationale**: Generic infrastructure should not contain domain-specific hints.

### ✅ Pending Capability Mechanism Verified
- Tested with goal: "fetch issues and group them by author"
- Correctly triggered:
  - Decomposition retry loop (`🔄 Decomposition attempt 1 produced pending capabilities, retrying...`)
  - `_grounded_no_tool: "true"` for missing capabilities
  - LLM-generated inline RTFS for complex transformations
  - Plan status `PendingSynthesis`

## Current Status

The "top 5 issues with formatted summary" goal now works correctly:
1. Initial plan may fail safe execution (missing owner/repo split)
2. GovernanceKernel LLM repair successfully fixes the RTFS plan
3. Execution completes with correct output

## Investigation: RTFS `group` / `group_by` Function

**Status**: ❌ **No native `group` or `group_by` function exists in RTFS stdlib**

## ✅ COMPLETED: Added `group-by` to RTFS stdlib

**Date**: 2025-12-26

### Implementation
- **File**: [rtfs/src/runtime/secure_stdlib.rs](../../rtfs/src/runtime/secure_stdlib.rs)
- **Function**: `group-by` - groups collection items by key function result
- **Signature**: `(group-by key-fn collection)` → map of key → [items]

### Features:
1. Accepts anonymous functions: `(group-by (fn [x] (get x :state)) issues)`
2. Accepts keywords as shorthand: `(group-by :author issues)` (equivalent to `(fn [x] (get x :author))`)
3. Returns a map where keys are the grouping values, values are vectors of items
4. Handles all key types: strings, keywords, integers, symbols, booleans, nil

### Example:
```lisp
(group-by :state [{:state "open" :id 1} {:state "closed" :id 2} {:state "open" :id 3}])
;; => {"open" [{:state "open" :id 1} {:state "open" :id 3}]
;;     "closed" [{:state "closed" :id 2}]}
```

### Prompt Updates
- **File**: [ccos/assets/prompts/arbiter/plan_rtfs_conversion/v1/grammar.md](../../ccos/assets/prompts/arbiter/plan_rtfs_conversion/v1/grammar.md)
- Added `group-by` to allowed RTFS constructs
- Added "Collection Transformation Functions" section with `map`, `filter`, `reduce`, `group-by`

### Tests Added
- **File**: [rtfs/tests/stdlib_e2e_tests.rs](../../rtfs/tests/stdlib_e2e_tests.rs)
- Test with keyword key-fn
- Test with anonymous function key-fn
- Test with empty collection

---

## ✅ COMPLETED: Synthesized Capability Persistence

**Date**: 2025-12-26

### Problem
When the planner generates inline RTFS code (via `ResolvedCapability::Synthesized`) for capabilities that don't exist as real tools, the synthesized code is used once and discarded. This means:
- Same capability synthesis happens repeatedly for similar requests
- No learning/reuse between sessions
- Wasted LLM calls for repeat syntheses

### Solution
Implemented automatic persistence of synthesized inline RTFS as reusable capabilities.

### Implementation

#### New Module: `SynthesizedCapabilityStorage`
- **File**: [ccos/src/synthesis/core/synthesized_capability_storage.rs](../../ccos/src/synthesis/core/synthesized_capability_storage.rs)
- **Struct**: `SynthesizedCapability` with id, description, implementation, input/output schemas, metadata
- **Storage Location**: `capabilities/synthesized/` directory
- **Format**: TOML files with RTFS capability definition

#### Orchestrator Integration
- **File**: [ccos/src/planner/modular_planner/orchestrator.rs](../../ccos/src/planner/modular_planner/orchestrator.rs)
- When `PlanStatus::PendingSynthesis` is detected, `save_synthesized_capabilities()` is called
- Extracts synthesized RTFS from `ResolvedCapability::Synthesized` entries
- Generates unique capability ID from description via slugification
- Persists to TOML file for future reuse

#### Marketplace Loading
- **File**: [ccos/src/capability_marketplace/marketplace.rs](../../ccos/src/capability_marketplace/marketplace.rs)
- Added `load_synthesized_capabilities()` method
- Integrated with `load_discovered_capabilities()` - loads both MCP-discovered and synthesized capabilities
- Synthesized capabilities are available for future planner decomposition

### Example Synthesized Capability File
```toml
# capabilities/synthesized/group-issues-by-author.toml
[capability]
id = "synth-group-issues-by-author-abc123"
description = "Groups issues by their author field"
input_schema = "list of issues"
output_schema = "map of author -> [issues]"
implementation = "(group-by :author issues)"
created_at = "2025-12-26T12:00:00Z"
```

### Tests
- `test_slugify` - ID generation from descriptions
- `test_generate_capability_id` - Unique ID format
- `test_capability_to_rtfs` - RTFS capability definition generation
- `conclude_and_learn_registers_synthesized_capabilities` - Full integration test

---

## Next Steps

1. **Consider executing pending plans** - The inline RTFS generated for pending capabilities is often valid and could execute. Add a `--force-execute` flag or smarter detection.

2. **Improve LLM prompt for owner/repo splitting** - The LLM should learn to split `owner/repo` input during decomposition, not rely on repair.

3. **Fix `html_url` nil values** - Check if GitHub API returns a different field name (e.g., `url` vs `html_url`).

4. **Add metrics/logging** - Track decomposition retry counts, repair success rates, and pending capability frequencies.

5. **Synthesized capability quality feedback** - Add mechanism to mark synthesized capabilities as working/not-working so learning can improve over time.
