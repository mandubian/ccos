# Plan Generation Prompt Consolidation

**Date**: 2025-10-05  
**Status**: ✅ Complete

## Overview

Consolidated multiple plan generation prompt directories into a single, well-designed version in `assets/prompts/arbiter/plan_generation/v1/`.

## Problem

We had too many different plan generation prompt directories:
- `plan_generation/`
- `plan_generation_full/`
- `plan_generation_reduced/`
- `plan_generation_retry/`
- `plan_generation_simple/`

Each had slightly different approaches, leading to:
- Inconsistent LLM outputs
- Confusion about which to use
- Maintenance burden
- Quality issues

## Solution

Created a single, unified plan generation prompt set based on:
- Best practices from `plan_generation_retry/v1/grammar.md` (latest working version)
- Correct RTFS syntax (no `edge` forms in plans)
- Clear examples from `plan_generation_reduced` and `plan_generation_full`
- Explicit anti-patterns to guide LLM away from common mistakes

## New Unified Structure

### Files Created/Updated

1. **`task.md`** - Clear task definition
   - Output format requirements
   - Key constraints
   - Variable scoping rules

2. **`grammar.md`** - Complete RTFS grammar reference
   - Plan structure with `(plan ...)` wrapper
   - All allowed forms
   - Available capabilities with signatures
   - Critical rules with examples
   - Common mistakes highlighted

3. **`few_shots.md`** - Comprehensive examples
   - Simple to complex patterns
   - All control flow types (if, match)
   - Math operations with return values
   - Multi-step data collection
   - Anti-pattern examples (what NOT to do)

4. **`strategy.md`** - Strategic guidance
   - Core principles
   - Step-by-step approach
   - Common patterns
   - Data flow handling
   - Anti-patterns to avoid

5. **`anti_patterns.md`** - Explicit violations
   - Output format violations
   - Variable scoping violations
   - Let binding violations
   - Return value violations
   - Capability violations
   - Structure violations

## Key Features

### ✅ Correct RTFS Syntax
- Plans use `(plan ...)` wrapper with `:name`, `:language`, `:body`, `:annotations`
- No `(edge ...)` forms in plans (those are for intent graphs)
- Sequential execution within `(do ...)` blocks

### ✅ Variable Scoping
- **CRITICAL**: `let` bindings are LOCAL to a single step
- Variables CANNOT cross step boundaries
- All related operations must be in the same step

### ✅ Structured Results
- Final step returns a map with keyword keys
- Enables downstream intent reuse
- Uses namespaced keywords (`:trip/destination`, `:user/name`)

### ✅ Capability Signatures
- All capabilities documented with exact signatures
- `:ccos.echo` takes map with `:message`
- `:ccos.math.add` takes positional arguments
- `:ccos.user.ask` takes string prompt

### ✅ Examples for Every Pattern
- Single prompt
- Multiple prompts with summary
- Conditional branching (if)
- Multiple choice (match)
- Math operations
- Complex multi-step plans

### ✅ Anti-Pattern Prevention
- Explicit "WRONG" examples
- Clear explanations of why they fail
- Correct alternatives shown

## Usage

The delegating arbiter uses these prompts via:
```rust
self.prompt_manager.render("plan_generation", "v1", &vars)
```

This loads all files from `assets/prompts/arbiter/plan_generation/v1/` and combines them into a single prompt for the LLM.

## Next Steps

1. ✅ Consolidate prompts (DONE)
2. ⏭️ Test with real LLM interactions
3. ⏭️ Monitor plan generation quality
4. ⏭️ Archive/remove old prompt directories once validated

## Migration Notes

### Old Directories (to be archived after validation)
- `plan_generation_full/` - Had good examples but missing structured returns
- `plan_generation_reduced/` - Good variable scoping but incomplete
- `plan_generation_retry/` - Had best grammar but only single file
- `plan_generation_simple/` - Too minimal

### Current Active Directory
- `plan_generation/v1/` - **USE THIS ONE**

## Testing

Build test passed:
```bash
cd rtfs_compiler && cargo build --example user_interaction_progressive_graph
```

Runtime testing needed:
```bash
cd rtfs_compiler && cargo run --example user_interaction_progressive_graph -- --enable-delegation --verbose
```

## References

- CCOS Spec: `docs/ccos/specs/`
- RTFS Spec: `docs/rtfs-2.0/specs/`
- Delegating Arbiter: `rtfs_compiler/src/ccos/arbiter/delegating_arbiter.rs`
- Example: `rtfs_compiler/examples/user_interaction_progressive_graph.rs`
