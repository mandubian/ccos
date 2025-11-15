# Planner-as-Plan Design

## Goal

Transform the planner execution process into an executable RTFS plan where each planner step becomes a capability call. This enables:
1. **Full causal chain tracking** - Every planner step logged as a capability call
2. **Arbiter rewriting** - Arbiters can modify the planner meta-plan before execution
3. **Replay and audit** - Complete planner execution trace in causal chain
4. **Composability** - Planner steps become first-class capabilities

## Architecture

### Current State
- Planner steps executed imperatively in Rust (`smart_assistant_planner_viz.rs`)
- Steps: extract_goal_signals → build_menu → synthesize_plan → validate → resolve_gaps → refine
- Not tracked in causal chain (only final plan execution is tracked)

### Target State
- Planner steps become RTFS plan calling `planner.*` capabilities
- Plan executed through orchestrator → all steps logged to causal chain
- Arbiter can rewrite/modify planner plan before execution
- Full trace: `planner.extract_goal_signals` → `planner.build_menu` → etc.

## Implementation

### 1. Planner Capabilities
All planner steps exposed as callable capabilities:
- `planner.extract_goal_signals` ✅ (already exists)
- `planner.build_capability_menu` (to implement)
- `planner.synthesize_plan_steps` (to implement)
- `planner.validate_plan` (to implement)
- `planner.resolve_capability_gaps` (to implement)

### 2. Planner Meta-Plan Generation
Create mechanism to generate RTFS plan from planner execution:

```rust
pub struct PlannerMetaPlanGenerator {
    // Generates RTFS plan that orchestrates planner.* capabilities
}

impl PlannerMetaPlanGenerator {
    pub fn generate_planner_plan(
        &self,
        goal: &str,
        intent: Option<&Intent>,
        options: PlannerOptions,
    ) -> RuntimeResult<Plan> {
        // Generate RTFS plan calling planner.* capabilities
        // Similar to cognitive_flow.rtfs but dynamically generated
    }
}
```

### 3. Arbiter Rewriting Hook
Allow arbiters to modify planner meta-plan:

```rust
pub trait PlannerPlanRewriter {
    fn rewrite_planner_plan(&self, plan: Plan) -> RuntimeResult<Plan>;
}
```

### 4. Execution Flow
```
User Goal
  ↓
Generate Planner Meta-Plan (RTFS plan calling planner.* capabilities)
  ↓
Arbiter rewrites plan (optional)
  ↓
Execute through Orchestrator
  ↓
All steps tracked in causal chain as capability calls
  ↓
Final plan produced (also tracked)
```

## Benefits

1. **Causal Chain Completeness**: Every planner step logged
2. **Audit Trail**: Full planner execution trace for debugging/analysis
3. **Flexibility**: Arbiters can customize planner behavior
4. **Consistency**: Planner follows same execution model as regular plans
5. **Replay**: Can replay entire planner execution from causal chain

## Next Steps

1. Implement missing `planner.*` capabilities
2. Create `PlannerMetaPlanGenerator`
3. Add arbiter rewriting hook
4. Update `smart_assistant_planner_viz.rs` to use planner-as-plan
5. Verify all steps logged in causal chain

