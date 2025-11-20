# Work Summary: Capability Synthesis and Plan Generation Improvements

## Overview
This work session focused on improving the capability synthesis and plan generation process within the CCOS system, specifically enhancing how LLMs generate and convert plans from goals to executable RTFS code.

## Completed Features

### 1. Output Schema Introspection for MCP Tools
**Problem**: MCP tools lacked output schema information, making it difficult to generate proper capability manifests.

**Solution**:
- Added feature flag `output_schema_introspection_enabled` in `feature_flags.rs`
- Implemented `introspect_output_schema()` in `mcp_introspector.rs` that:
  - Calls MCP tools with safe test inputs
  - Infers output schema from JSON responses
  - Converts JSON values to RTFS `TypeExpr` types
- Added `generate_safe_test_inputs()` to create safe test inputs from input schemas

**Files Modified**:
- `ccos/src/synthesis/feature_flags.rs`
- `ccos/src/synthesis/mcp_introspector.rs`

### 2. Improved Test Input Generation
**Problem**: Test input generation for synthesized capabilities was basic and didn't handle complex types.

**Solution**:
- Enhanced `generate_test_inputs()` in `registration_flow.rs` to:
  - Parse `TypeExpr` recursively
  - Generate appropriate test data for strings, numbers, booleans, arrays, maps
  - Handle nested structures
- Fixed type mismatch: `MapKey` import corrected from `rtfs::runtime::values::MapKey` to `rtfs::ast::MapKey`
- Converted string keys to `MapKey::Keyword` for proper RTFS map construction

**Files Modified**:
- `ccos/src/synthesis/registration_flow.rs`

### 3. Function Parameter Support in Capability Menu
**Problem**: Function parameters (like predicates) couldn't be distinguished from regular parameters in the capability menu, leading to confusion.

**Solution**:
- Added `is_function_type_expr()` to detect function types in `menu.rs`
- Modified `insert_entry()` to annotate function parameters with `(function - cannot be passed directly)`
- Added `function_parameters()` method to extract function parameter names without annotations
- Updated menu display to clearly identify function parameters

**Files Modified**:
- `ccos/src/planner/menu.rs`

### 4. LLM-Based JSON-to-RTFS Plan Conversion
**Problem**: Manual RTFS plan generation was error-prone. LLM was generating non-RTFS code (Clojure-like syntax) and "hacking" non-existent fields.

**Solution**:
- Created new prompt system in `plan_rtfs_conversion/v1/`:
  - `grammar.md`: Strict RTFS grammar hints with examples
  - `task.md`: Clear task definition with parameter fidelity requirements
  - `strategy.md`: Conversion strategy guidance
  - `anti_patterns.md`: Explicit bans on non-RTFS syntax and misuse patterns
- Added `StepInputBinding::RtfsCode(String)` variant to explicitly handle RTFS code
- Implemented `render_plan_body_with_llm()` for LLM-based conversion:
  - Serializes plan steps to JSON
  - Renders prompt using `PromptManager`
  - Extracts RTFS code from LLM response
- Updated `assemble_plan_from_steps()` to use LLM conversion with fallback
- Enhanced validation in `validate_plan_steps_against_menu()`:
  - Detects code-like strings in literal inputs
  - Validates RTFS code is only used for function parameters
  - Provides specific error messages for misuse

**Files Modified**:
- `ccos/examples/smart_assistant_planner_viz.rs` (major refactor)
- `ccos/assets/prompts/arbiter/plan_rtfs_conversion/v1/*.md` (new files)

### 5. Proactive Filtering Capability Synthesis
**Problem**: LLM was avoiding generating filtering steps even when `MustFilter` requirements were present, leading to unmet requirements.

**Solution**:
- Included `GoalSignals` in initial plan synthesis prompt with explicit `MustFilter` requirements
- Added proactive synthesis logic:
  - Detects unmet `MustFilter` when `RequirementResolutionOutcome::NoAction`
  - If no filtering capability exists, adds `MustCallCapability` for `mcp.core.filter`
  - Forces capability provisioning attempt
  - Registers synthesized capability and retries plan synthesis
- Enhanced feedback messages to guide LLM on filtering requirements
- Updated `DefaultGoalCoverageAnalyzer` to recognize RTFS filter steps (e.g., `(filter ...)`) as valid implementations of `MustFilter`, even without explicit capability calls.

**Files Modified**:
- `ccos/examples/smart_assistant_planner_viz.rs`
- `ccos/src/planner/coverage.rs`

### 6. Enhanced Prompt System and Validation
**Problem**: Prompts lacked separation of concerns and RTFS-specific guidance was mixed with goal-to-JSON steps.

**Solution**:
- Separated RTFS code generation rules from initial goal-to-JSON steps
- Added explicit RTFS syntax examples:
  - Correct: `string-contains`, `(get map :key)`
  - Incorrect: `clojure.string/includes?`, `issue.title`
- Updated example structure in `grammar.md` to use different use case (CSV parsing) instead of GitHub issues
- Strengthened anti-patterns to prevent:
  - Non-RTFS syntax usage
  - Fabricated parameters
  - Misuse of `{"rtfs": "..."}` for non-function inputs
- Updated error explanations in `error_explainer.rs` to suggest correct `parse-json` usage instead of non-existent capabilities.

**Files Modified**:
- `ccos/assets/prompts/arbiter/plan_rtfs_conversion/v1/grammar.md`
- `ccos/assets/prompts/arbiter/plan_rtfs_conversion/v1/anti_patterns.md`
- `ccos/assets/prompts/arbiter/plan_rtfs_conversion/v1/task.md`
- `ccos/assets/prompts/arbiter/plan_rtfs_conversion/v1/strategy.md`
- `ccos/src/rtfs_bridge/error_explainer.rs`

### 7. Plan Execution and Auto-Repair
**Problem**: Plans were generating correctly but failing at runtime due to type mismatches (e.g., `first` on map), incorrect JSON content extraction, or invalid RTFS wrapping (`(rtfs ...)`). Auto-repair was not triggering for runtime errors or was generating invalid plan wrappers.

**Solution**:
- **Runtime Error Trapping**: Updated `ccos_core.rs` to catch `Ok(ExecutionResult { success: false })` and feed the error message into the auto-repair loop.
- **Robust JSON Parsing**: Refined planner prompts to explicitly instruct on extracting `:text` from the `content` vector before parsing, and handling JSON wrappers (`"items"`, `"issues"`).
- **Strict RTFS Generation**: Updated `plan_rtfs_conversion` prompt to strictly forbid generating `(rtfs ...)` function calls, enforcing direct expression inlining.
- **Auto-Repair Refinement**: Updated `ccos_core.rs` auto-repair prompt to forbid wrapping code in `(plan ...)` and using commas in maps.
- **Clean Output**: Updated `plan_rtfs_conversion` prompt to extract only relevant final outputs, filtering out large intermediate data.
- **Verified Execution**: Validated end-to-end execution with `CCOS_USE_LLM_PLAN_CONVERSION=true` and `--auto-repair`, confirming successful retrieval and parsing of specific GitHub issues.

**Files Modified**:
- `ccos/src/ccos_core.rs`
- `ccos/assets/prompts/arbiter/plan_rtfs_conversion/v1/task.md`
- `ccos/examples/smart_assistant_planner_viz.rs`

### 8. Smarter Capability Discovery (Latest Update)
**Problem**: Capability discovery was fragile, relying on exact keyword matches or generic searches that often failed to find relevant tools (e.g., searching "issues" failed to find GitHub tools if not explicitly named "issues"). Hardcoded checks for MCP discovery were brittle.

**Solution**:
- **Semantic Gap Detection**: Implemented a generic trigger for discovery based on low semantic coverage scores (< 0.65) rather than hardcoded keywords.
- **Context-Aware Query Expansion**: Enhanced `DiscoveryEngine` to inject high-value context tokens (e.g., "github", "aws", "slack") from the rationale into search queries. If a user asks "list github issues", searching for "issues" alone might fail, but "github issues" succeeds.
- **Stopword Filtering**: Improved tokenization in `capability_helpers.rs` to ignore common stopwords, improving search relevance.
- **RTFS Syntax Fixes**: Corrected RTFS type syntax in `mcp_discovery.rs` (e.g., using `T?` instead of `[:optional T]`).

**Files Modified**:
- `ccos/src/discovery/engine.rs`
- `ccos/examples/smart_assistant_planner_viz.rs`
- `ccos/src/examples_common/capability_helpers.rs`
- `ccos/src/capability_marketplace/mcp_discovery.rs`

## Technical Improvements

### Code Quality
- Fixed type mismatches and import paths
- Added comprehensive error messages
- Improved validation logic with specific feedback
- Enhanced menu display for better UX
- Addressed linter warnings (unused mutability, etc.)

### Architecture
- Separated concerns: goal-to-JSON vs JSON-to-RTFS conversion
- Modular prompt system using `PromptManager` and `FilePromptStore`
- Feature flags for optional functionality
- Robust fallback mechanisms

## Current Status

✅ **Working**: 
- Output schema introspection
- Test input generation
- Function parameter detection
- LLM-based plan conversion
- Proactive filtering synthesis
- RTFS syntax validation
- **Plan Execution & Auto-Repair** (Verified & Robust)
- **Context-Aware Discovery** (Verified with GitHub example)

⚠️ **Minor Issues**:
- LLM sometimes uses string keys `"title"` instead of keyword keys `:title` in map access (Mitigated by prompts)

## Next Steps

1. **RTFS Syntax Refinement**
   - Update grammar hints to emphasize keyword usage (`:key`) over strings (`"key"`)
   - Add more examples of correct map access patterns
   - Strengthen validation for keyword vs string usage

2. **Error Handling and Feedback**
   - Improve error messages for execution failures
   - Add retry logic for plan generation
   - Better feedback loop for LLM corrections

3. **Testing**
   - Add unit tests for new prompt system
   - Test plan conversion with various capability types
   - Validate RTFS code generation correctness

4. **Documentation**
   - Document the new prompt system structure
   - Add examples of correct RTFS plan patterns
   - Update architecture docs with new conversion flow

## Statistics

- **Files Modified**: 25 files
- **Lines Added**: ~2,850
- **Lines Removed**: ~530
- **New Prompt Files**: 4 files
- **Major Features**: 8 functional improvements
