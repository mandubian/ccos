# LLM Provider Update - Consolidated Plan Generation

**Date**: 2025-10-05  
**Status**: ✅ Complete

## Changes Made

Updated `rtfs_compiler/src/ccos/arbiter/llm_provider.rs` to use the consolidated `plan_generation` prompts by default.

### Before

The OpenAI LLM provider used environment variable `RTFS_FULL_PLAN` to choose between:
- `plan_generation_full` (when `RTFS_FULL_PLAN=1`)
- `plan_generation_reduced` (default)

### After

The OpenAI LLM provider now:
- **Uses `plan_generation` by default** (consolidated unified prompts)
- Supports legacy modes via explicit environment variables:
  - `RTFS_LEGACY_PLAN_FULL=1` → use `plan_generation_full`
  - `RTFS_LEGACY_PLAN_REDUCED=1` → use `plan_generation_reduced`

## Code Changes

### 1. Prompt Selection Logic

**Before:**
```rust
let full_plan_mode = std::env::var("RTFS_FULL_PLAN")
    .map(|v| v == "1")
    .unwrap_or(false);

let prompt_id = if full_plan_mode {
    "plan_generation_full"
} else {
    "plan_generation_reduced"
};
```

**After:**
```rust
let use_legacy_full = std::env::var("RTFS_LEGACY_PLAN_FULL")
    .map(|v| v == "1")
    .unwrap_or(false);
let use_legacy_reduced = std::env::var("RTFS_LEGACY_PLAN_REDUCED")
    .map(|v| v == "1")
    .unwrap_or(false);

let prompt_id = if use_legacy_full {
    "plan_generation_full"
} else if use_legacy_reduced {
    "plan_generation_reduced"
} else {
    "plan_generation"  // Consolidated unified prompts
};
```

### 2. Fallback Prompt

Updated the fallback prompt (when asset loading fails) to match the consolidated format:

```rust
r#"You translate an RTFS intent into a concrete RTFS plan.

Output format: ONLY a single well-formed RTFS s-expression starting with (plan ...). No prose, no JSON, no fences.

Plan structure:
(plan
  :name "descriptive_name"
  :language rtfs20
  :body (do
    (step "Step Name" <expr>)
    ...
  )
  :annotations {:key "value"}
)

CRITICAL: let bindings are LOCAL to a single step. Variables CANNOT cross step boundaries.
Final step should return a structured map with keyword keys for downstream reuse."#
```

### 3. User Message

Simplified to a single format (no more conditional based on mode):

```rust
let user_message = format!(
    "Intent goal: {}\nConstraints: {:?}\nPreferences: {:?}\n\nGenerate the (plan ...) now, following the grammar and constraints:",
    intent.goal, intent.constraints, intent.preferences
);
```

### 4. Plan Extraction Logic

Updated to expect `(plan ...)` wrapper by default:

```rust
let expect_plan_wrapper = !use_legacy_reduced;

if expect_plan_wrapper {
    // Extract plan with wrapper
    ...
}
```

## Environment Variables

### New (Recommended)
- **Default**: No env var needed - uses consolidated `plan_generation`
- **Legacy Full**: `RTFS_LEGACY_PLAN_FULL=1` - use old `plan_generation_full`
- **Legacy Reduced**: `RTFS_LEGACY_PLAN_REDUCED=1` - use old `plan_generation_reduced`

### Deprecated
- ~~`RTFS_FULL_PLAN=1`~~ - No longer used (replaced by `RTFS_LEGACY_PLAN_FULL`)

## Testing

### Default Behavior (Consolidated Prompts)
```bash
cd rtfs_compiler
cargo run --example user_interaction_progressive_graph -- --enable-delegation --verbose
```

### Legacy Full Mode
```bash
cd rtfs_compiler
RTFS_LEGACY_PLAN_FULL=1 cargo run --example user_interaction_progressive_graph -- --enable-delegation --verbose
```

### Legacy Reduced Mode
```bash
cd rtfs_compiler
RTFS_LEGACY_PLAN_REDUCED=1 cargo run --example user_interaction_progressive_graph -- --enable-delegation --verbose
```

### Debug Prompts
```bash
cd rtfs_compiler
RTFS_SHOW_PROMPTS=1 cargo run --example user_interaction_progressive_graph -- --enable-delegation --verbose
```

## Benefits

1. **Single Source of Truth**: One well-designed prompt set instead of multiple inconsistent versions
2. **Better Quality**: Consolidated prompts include:
   - Comprehensive grammar reference
   - Extensive few-shot examples
   - Explicit anti-patterns
   - Critical rules (variable scoping, structured returns)
3. **Backward Compatible**: Legacy modes still available via explicit env vars
4. **Cleaner Code**: Simplified logic, better fallback prompt
5. **Better Defaults**: New users get the best prompts by default

## Affected Components

- ✅ `OpenAILlmProvider::generate_plan()` - Updated to use consolidated prompts
- ⏭️ `AnthropicLlmProvider::generate_plan()` - Uses different format (JSON-based), not affected
- ⏭️ `StubLlmProvider::generate_plan()` - Generates stub plans, not affected

## Migration Path

### For Existing Users

If you were using the default behavior (no `RTFS_FULL_PLAN` set):
- ✅ **No action needed** - you'll automatically get the improved consolidated prompts

If you were using `RTFS_FULL_PLAN=1`:
- 🔄 **Optional migration**: Remove the env var to use consolidated prompts (recommended)
- 🔄 **Keep old behavior**: Change to `RTFS_LEGACY_PLAN_FULL=1`

### For New Users

- ✅ **Just use the default** - no env vars needed
- ✅ **Best prompts out of the box**

## Next Steps

1. ✅ Update LLM provider (DONE)
2. ⏭️ Test with real LLM interactions
3. ⏭️ Monitor plan generation quality
4. ⏭️ Deprecate legacy prompt directories after validation period
5. ⏭️ Consider updating Anthropic provider to use RTFS format

## References

- **Consolidated Prompts**: `assets/prompts/arbiter/plan_generation/v1/`
- **Quick Reference**: `docs/prompts/PLAN_GENERATION_QUICK_REF.md`
- **Consolidation Details**: `docs/prompts/PLAN_GENERATION_CONSOLIDATION.md`
- **LLM Provider Code**: `rtfs_compiler/src/ccos/arbiter/llm_provider.rs`
