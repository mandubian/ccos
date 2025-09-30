# Prompt Store Migration Analysis

## Executive Summary

**Recommendation: YES, migrate to file-based prompt storage**

The project already has:
- ✅ A well-designed `PromptManager` + `FilePromptStore` system (`src/ccos/arbiter/prompt.rs`)
- ✅ Existing prompt assets in `assets/prompts/arbiter/` with proper structure
- ✅ Partial adoption in `llm_arbiter.rs` (intent generation only)
- ❌ Hard-coded prompts still dominate in `delegating_arbiter.rs` and other arbiters

## Current State Analysis

### Existing Prompt Assets Structure
```
assets/prompts/arbiter/
├── intent_generation/v1/
│   ├── grammar.md
│   ├── strategy.md
│   ├── few_shots.md
│   ├── anti_patterns.md
│   └── task.md
├── plan_generation/v1/
│   ├── grammar.md
│   ├── strategy.md
│   ├── few_shots.md
│   ├── anti_patterns.md
│   └── task.md
└── delegation_analysis/v1/
    ├── task.md
    ├── few_shots.md
    └── anti_patterns.md
```

### Hard-Coded Prompts Inventory

#### DelegatingArbiter (`src/ccos/arbiter/delegating_arbiter.rs`)
1. ✅ **`create_intent_prompt`** - Has asset: `intent_generation/v1/`
   - Current: ~60 lines hard-coded
   - Asset: Complete with grammar, strategy, few_shots, anti_patterns, task
   
2. ✅ **`create_delegation_analysis_prompt`** - Has asset: `delegation_analysis/v1/`
   - Current: ~30 lines hard-coded
   - Asset: Complete with task, few_shots, anti_patterns
   
3. ✅ **`create_delegation_plan_prompt`** - Can use: `plan_generation/v1/` (with delegation context)
   - Current: ~35 lines hard-coded
   - Asset: Complete plan generation prompts available
   
4. ✅ **`create_direct_plan_prompt`** - Has asset: `plan_generation/v1/`
   - Current: ~50 lines hard-coded
   - Asset: Complete with grammar, strategy, few_shots, anti_patterns, task
   
5. ⚠️ **`natural_language_to_graph` inline prompt** - Missing asset
   - Current: ~30 lines inline in method
   - Asset: **NEEDS CREATION** - `graph_generation/v1/`

#### LlmArbiter (`src/ccos/arbiter/llm_arbiter.rs`)
1. ✅ **`generate_intent_prompt`** - **ALREADY USES PromptManager** ✨
   - Uses: `FilePromptStore` + `PromptManager`
   - Loads from: `intent_generation/v1/`
   
2. ⚠️ **`generate_plan_prompt`** - Has asset but not using it
   - Current: ~25 lines hard-coded
   - Asset: Available in `plan_generation/v1/`

#### HybridArbiter (`src/ccos/arbiter/hybrid_arbiter.rs`)
1. ⚠️ **`create_intent_prompt`** - Has asset: `intent_generation/v1/`
2. ⚠️ **`create_plan_prompt`** - Has asset: `plan_generation/v1/`

## Benefits of Migration

### 1. **Version Management** 🎯
- Prompts can be versioned without code changes
- A/B testing different prompt versions
- Rollback to previous versions easily
- Example: `v1/`, `v2/`, `experimental/`

### 2. **Non-Developer Collaboration** 👥
- Prompt engineers can edit without Rust knowledge
- Immediate changes without recompilation
- Easier review and iteration
- Git diffs show actual prompt changes clearly

### 3. **Modularity & Reusability** 🔄
- Sections (grammar, strategy, few_shots, etc.) can be mixed/matched
- Shared components across different prompts
- DRY principle for common patterns
- Example: Same grammar.md used across multiple prompt types

### 4. **Testability** 🧪
- Test different prompt versions independently
- Compare outputs from different prompt strategies
- Performance metrics per prompt version
- Easier to identify which prompt sections matter

### 5. **Documentation Co-location** 📚
- Prompts serve as documentation
- Clear examples in few_shots.md
- Anti-patterns documented explicitly
- Self-documenting system behavior

### 6. **Hot Reload Capability** 🔥
- Could add file watching for development
- No restart needed for prompt tweaks
- Faster iteration cycle

### 7. **Separation of Concerns** 🎨
- Prompt content ≠ prompt logic
- Rust code focuses on orchestration
- Content experts manage prompts
- Cleaner codebase

## Migration Plan

### Phase 1: DelegatingArbiter (High Priority)
1. ✅ Migrate `create_intent_prompt` to use existing `intent_generation/v1/`
2. ✅ Migrate `create_delegation_analysis_prompt` to use existing `delegation_analysis/v1/`
3. ✅ Migrate `create_delegation_plan_prompt` to use `plan_generation/v1/` with delegation context
4. ✅ Migrate `create_direct_plan_prompt` to use existing `plan_generation/v1/`
5. ❌ Create `graph_generation/v1/` prompts for `natural_language_to_graph`

### Phase 2: LlmArbiter (Medium Priority)
1. ✅ Already using PromptManager for intent generation
2. ⚠️ Migrate `generate_plan_prompt` to use existing `plan_generation/v1/`

### Phase 3: HybridArbiter (Low Priority)
1. ⚠️ Migrate both prompt methods to PromptManager

## Technical Implementation Guide

### Step 1: Add PromptManager to DelegatingArbiter

```rust
use super::prompt::{FilePromptStore, PromptConfig, PromptManager};

pub struct DelegatingArbiter {
    llm_config: LlmConfig,
    delegation_config: DelegationConfig,
    llm_provider: Box<dyn LlmProvider>,
    agent_registry: AgentRegistry,
    intent_graph: Arc<Mutex<IntentGraph>>,
    adaptive_threshold_calculator: Option<AdaptiveThresholdCalculator>,
    prompt_manager: PromptManager<FilePromptStore>, // ADD THIS
}
```

### Step 2: Initialize PromptManager in Constructor

```rust
impl DelegatingArbiter {
    pub async fn new(...) -> Result<Self, RuntimeError> {
        // ... existing code ...
        
        let prompt_store = FilePromptStore::new("assets/prompts/arbiter");
        let prompt_manager = PromptManager::new(prompt_store);
        
        Ok(Self {
            // ... existing fields ...
            prompt_manager,
        })
    }
}
```

### Step 3: Refactor Prompt Methods

**Before (Hard-coded):**
```rust
fn create_intent_prompt(
    &self,
    natural_language: &str,
    context: Option<HashMap<String, Value>>,
) -> String {
    format!(
        r#"CRITICAL: You must respond with RTFS syntax, NOT JSON.
        
Convert the following natural language request into a structured Intent...
        
Request: {natural_language}
Context: {context:?}
..."#
    )
}
```

**After (File-based):**
```rust
fn create_intent_prompt(
    &self,
    natural_language: &str,
    context: Option<HashMap<String, Value>>,
) -> Result<String, RuntimeError> {
    let mut vars = HashMap::new();
    vars.insert("natural_language".to_string(), natural_language.to_string());
    vars.insert("context".to_string(), format!("{:?}", context.unwrap_or_default()));
    vars.insert("available_capabilities".to_string(), 
                format!("{:?}", vec!["ccos.echo", "ccos.math.add"]));
    
    let prompt_config = self.delegation_config
        .prompts
        .clone()
        .unwrap_or_default();
    
    self.prompt_manager.render(
        &prompt_config.intent_prompt_id,
        &prompt_config.intent_prompt_version,
        &vars,
    )
}
```

### Step 4: Update Config to Include Prompt References

```rust
pub struct DelegationConfig {
    pub enabled: bool,
    pub threshold: f64,
    // ... existing fields ...
    pub prompts: Option<PromptConfig>, // ADD THIS
}
```

## Missing Prompt Assets to Create

### 1. Graph Generation Prompts
Create: `assets/prompts/arbiter/graph_generation/v1/`

Required sections:
- **grammar.md**: RTFS intent graph syntax (intent, edge forms)
- **strategy.md**: How to decompose goals into subgoals
- **few_shots.md**: Example graphs for common patterns
- **anti_patterns.md**: Common mistakes in graph structure
- **task.md**: Task description for LLM

### 2. Delegated Plan Generation (Variant)
Optionally create: `assets/prompts/arbiter/delegated_plan_generation/v1/`
- Could be a variant of plan_generation with agent context
- Or use variable substitution in existing plan_generation

## Risks & Mitigations

### Risk 1: File Not Found Errors
**Mitigation**: 
- Validate prompt assets exist at startup
- Provide clear error messages with file paths
- Include fallback to embedded prompts for critical paths

### Risk 2: Variable Substitution Complexity
**Mitigation**:
- Current system uses simple `{var}` replacement
- Consider upgrading to a template engine (handlebars, tera) if needed
- Document required variables per prompt type

### Risk 3: Backward Compatibility
**Mitigation**:
- Keep hard-coded prompts as fallback initially
- Gradual migration with feature flag
- Comprehensive testing before removal

### Risk 4: Prompt Asset Distribution
**Mitigation**:
- Include assets in releases
- Document asset location requirements
- Consider embedding critical prompts with `include_str!` macro

## Success Metrics

1. ✅ All arbiter prompt methods use PromptManager
2. ✅ Zero hard-coded prompts in arbiter implementations
3. ✅ All prompt types have v1 assets
4. ✅ Tests pass with file-based prompts
5. ✅ Documentation updated with prompt authoring guide
6. ✅ Prompt versioning strategy documented

## Timeline Estimate

- **Phase 1 (DelegatingArbiter)**: 2-3 hours
  - Create missing graph_generation prompts: 30min
  - Refactor 4 prompt methods: 1.5hrs
  - Update tests: 1hr
  
- **Phase 2 (LlmArbiter)**: 1 hour
  - Refactor plan generation: 30min
  - Update tests: 30min
  
- **Phase 3 (HybridArbiter)**: 1 hour
  - Similar to Phase 2

**Total Estimated Time**: 4-5 hours

## Conclusion

**Strong YES for migration** because:

1. ✅ Infrastructure already exists and proven (llm_arbiter uses it)
2. ✅ Most prompt assets already created (90% coverage)
3. ✅ Clear benefits for maintainability and collaboration
4. ✅ Low risk with existing fallback patterns
5. ✅ Modest time investment (4-5 hours) for significant long-term gains

**Immediate Next Step**: Start with Phase 1 - DelegatingArbiter migration, as it has the most hard-coded prompts and will demonstrate the pattern for others.

## References

- Prompt Store Implementation: `rtfs_compiler/src/ccos/arbiter/prompt.rs`
- Existing Prompt Assets: `rtfs_compiler/assets/prompts/arbiter/`
- LLM Arbiter Example: `rtfs_compiler/src/ccos/arbiter/llm_arbiter.rs:260-290`
- Delegating Arbiter: `rtfs_compiler/src/ccos/arbiter/delegating_arbiter.rs`
