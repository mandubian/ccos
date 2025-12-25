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

## Next Steps

1. **Consider executing pending plans** - The inline RTFS generated for pending capabilities is often valid and could execute. Add a `--force-execute` flag or smarter detection.

2. **Improve LLM prompt for owner/repo splitting** - The LLM should learn to split `owner/repo` input during decomposition, not rely on repair.

3. **Fix `html_url` nil values** - Check if GitHub API returns a different field name (e.g., `url` vs `html_url`).

4. **Add metrics/logging** - Track decomposition retry counts, repair success rates, and pending capability frequencies.
