# LLM Provider Remaining Prompt Migration

## Status: PARTIAL MIGRATION COMPLETE

### What Was Migrated ✅

1. **`generate_intent` method** - Now uses `intent_generation/v1/` prompts
2. **`generate_plan` method** - Now uses `plan_generation_reduced/v1/` or `plan_generation_full/v1/` based on RTFS_FULL_PLAN env var
3. **Created prompt assets** for:
   - `intent_generation/v1/` (5 files)
   - `plan_generation_reduced/v1/` (5 files)
   - `plan_generation_full/v1/` (5 files)
   - `plan_generation_simple/v1/` (5 files)

### What Remains ❌

**`generate_plan_with_retry` method** (lines 706-1119 in llm_provider.rs)

This method still contains ~414 lines of hard-coded prompts in 3 different places:

1. **Initial attempt prompt** (lines ~717-786)
   - Should use: `plan_generation_reduced/v1/`
   
2. **Retry with error feedback** (lines ~788-885)
   - Regular retry: Should use `plan_generation_reduced/v1/`
   - Final attempt (simplified): Should use `plan_generation_simple/v1/`
   
3. **Simple retry without feedback** (lines ~895-1000)
   - Should use: `plan_generation_reduced/v1/`

## Recommended Migration Approach

### Option 1: Direct Replacement (Recommended)

Replace the entire `generate_plan_with_retry` method with a version that:

```rust
async fn generate_plan_with_retry(
    &self,
    intent: &StorableIntent,
    _context: Option<HashMap<String, String>>,
) -> Result<Plan, RuntimeError> {
    let mut last_error = None;
    let mut last_plan_text = None;
    
    // Prepare variables for prompt rendering
    let vars = HashMap::from([
        ("goal".to_string(), intent.goal.clone()),
        ("constraints".to_string(), format!("{:?}", intent.constraints)),
        ("preferences".to_string(), format!("{:?}", intent.preferences)),
    ]);
    
    for attempt in 1..=self.config.retry_config.max_retries {
        // Determine which prompt to use based on attempt and configuration
        let (prompt_id, user_message) = if attempt == 1 {
            // Initial attempt - use plan_generation_reduced
            let user_msg = format!(...);
            ("plan_generation_reduced", user_msg)
        } else if self.config.retry_config.send_error_feedback {
            if attempt == self.config.retry_config.max_retries && self.config.retry_config.simplify_on_final_attempt {
                // Final attempt - use plan_generation_simple
                let user_msg = format!(...);
                ("plan_generation_simple", user_msg)
            } else {
                // Regular retry - use plan_generation_reduced
                let user_msg = format!(...);
                ("plan_generation_reduced", user_msg)
            }
        } else {
            // Simple retry - use plan_generation_reduced
            let user_msg = format!(...);
            ("plan_generation_reduced", user_msg)
        };
        
        // Load prompt from assets with fallback
        let system_message = self.prompt_manager
            .render(prompt_id, "v1", &vars)
            .unwrap_or_else(|e| {
                eprintln!("Warning: Failed to load {} prompt. Using fallback.", prompt_id);
                "You translate an RTFS intent into a concrete RTFS execution body. Output ONLY a (do ...) s-expression.".to_string()
            });
        
        // ... rest of method unchanged ...
    }
}
```

### Option 2: Keep As-Is (Not Recommended)

The retry method could remain with hard-coded prompts because:
- It's used less frequently than `generate_plan`
- The prompts are contextual (include error feedback)
- Migration is complex due to method length

**Downsides**:
- Inconsistency with rest of codebase
- Prompts can't be edited without recompilation
- Duplication of prompt content

## Files Involved

- **Source**: `rtfs_compiler/src/ccos/arbiter/llm_provider.rs`
- **Prompts**: `assets/prompts/arbiter/plan_generation_{reduced,simple}/v1/*.md`

## Estimated Effort

- **Time**: 30-45 minutes
- **Complexity**: Medium (long method, multiple conditional branches)
- **Risk**: Low (method has extensive error handling and fallbacks)
- **Testing**: Use existing retry tests

## Benefits of Completing Migration

1. **Consistency**: All LLM prompts managed uniformly
2. **Maintainability**: Prompt engineers can iterate without Rust knowledge
3. **Version Control**: Easier to track prompt changes in git diffs
4. **A/B Testing**: Can test different retry strategies
5. **Completeness**: Closes the loop on prompt externalization

## Current Test Status

All existing tests pass (13/13 llm_provider tests) even with partial migration.

