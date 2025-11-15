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

**Files Modified**:
- `ccos/examples/smart_assistant_planner_viz.rs`

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

**Files Modified**:
- `ccos/assets/prompts/arbiter/plan_rtfs_conversion/v1/grammar.md`
- `ccos/assets/prompts/arbiter/plan_rtfs_conversion/v1/anti_patterns.md`
- `ccos/assets/prompts/arbiter/plan_rtfs_conversion/v1/task.md`
- `ccos/assets/prompts/arbiter/plan_rtfs_conversion/v1/strategy.md`

## Technical Improvements

### Code Quality
- Fixed type mismatches and import paths
- Added comprehensive error messages
- Improved validation logic with specific feedback
- Enhanced menu display for better UX

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

⚠️ **Minor Issues**:
- LLM sometimes uses string keys `"title"` instead of keyword keys `:title` in map access
- Plan execution not yet integrated (visualization only)

## Next Steps

1. **Plan Execution Integration**
   - Add orchestrator execution to `smart_assistant_planner_viz.rs`
   - Test end-to-end: goal → plan → execution → results
   - Handle execution errors and provide feedback

2. **RTFS Syntax Refinement**
   - Update grammar hints to emphasize keyword usage (`:key`) over strings (`"key"`)
   - Add more examples of correct map access patterns
   - Strengthen validation for keyword vs string usage

3. **Error Handling and Feedback**
   - Improve error messages for execution failures
   - Add retry logic for plan generation
   - Better feedback loop for LLM corrections

4. **Testing**
   - Add unit tests for new prompt system
   - Test plan conversion with various capability types
   - Validate RTFS code generation correctness

5. **Documentation**
   - Document the new prompt system structure
   - Add examples of correct RTFS plan patterns
   - Update architecture docs with new conversion flow

## Statistics

- **Files Modified**: 18 files
- **Lines Added**: ~2,527
- **Lines Removed**: ~476
- **New Prompt Files**: 4 files
- **Major Features**: 6 functional improvements

