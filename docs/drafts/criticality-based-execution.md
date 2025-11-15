# Criticality-Based Execution Design

## Goal

Implement safe execution model that prevents automatic execution of critical actions (payments, deletions, etc.) and provides dry-run capability for validation.

## Problem Statement

When a planner generates a plan containing critical actions:
- **Payments** should not execute multiple times
- **Data deletion** should require explicit approval
- **Irreversible operations** should be validated before execution
- **Dry-run mode** should validate execution without side effects

## Architecture

### Action Criticality Levels

```rust
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ActionCriticality {
    /// Safe operations (read-only, can auto-execute)
    Safe,
    /// Moderate operations (writes, requires validation)
    Moderate,
    /// Critical operations (payments, deletions, requires explicit approval)
    Critical,
    /// Dangerous operations (system-level changes, requires human approval)
    Dangerous,
}
```

### Capability Manifest Enhancement

```rust
pub struct CapabilityManifest {
    // ... existing fields ...
    
    /// Criticality level of this capability
    pub criticality: Option<ActionCriticality>,
    
    /// Whether this capability requires explicit human approval
    pub requires_approval: bool,
    
    /// Whether this operation is irreversible
    pub irreversible: bool,
    
    /// Whether this operation can be simulated in dry-run
    pub dry_run_simulatable: bool,
}
```

### Execution Modes

```rust
#[derive(Debug, Clone)]
pub enum ExecutionMode {
    /// Execute all actions including critical ones
    FullExecution,
    
    /// Validate plan without executing critical actions
    /// - Safe actions execute normally
    /// - Critical actions are simulated
    DryRun,
    
    /// Execute only safe actions, pause for approval on critical ones
    SafeOnly { 
        /// Whether to continue after approval
        continue_on_approval: bool 
    },
    
    /// Execute with approval gates - pause and ask for each critical action
    ApprovalGated,
}
```

### Criticality Detection

#### Method 1: Capability Metadata (Preferred)
Capabilities declare their criticality in manifest.

#### Method 2: Pattern Matching (Fallback)
Detect criticality from capability ID patterns:

```rust
fn detect_criticality(capability_id: &str) -> ActionCriticality {
    let id_lower = capability_id.to_lowercase();
    
    if id_lower.contains("payment") || id_lower.contains("billing") || 
       id_lower.contains("charge") || id_lower.contains("transfer") {
        return ActionCriticality::Critical;
    }
    
    if id_lower.contains("delete") || id_lower.contains("remove") || 
       id_lower.contains("destroy") || id_lower.contains("drop") {
        return ActionCriticality::Critical;
    }
    
    if id_lower.contains("write") || id_lower.contains("create") || 
       id_lower.contains("update") || id_lower.contains("modify") {
        return ActionCriticality::Moderate;
    }
    
    // Default: read operations are safe
    ActionCriticality::Safe
}
```

## Dry-Run Implementation

### Behavior

In dry-run mode:
1. **Safe actions**: Execute normally (log real results)
2. **Moderate actions**: Execute but log `dry_run: true` in metadata
3. **Critical actions**: 
   - Log `CapabilityCall` with `dry_run: true`
   - Skip actual execution
   - Generate simulated result based on capability output schema
   - Continue plan execution with simulated result

### Simulated Results

For critical capabilities in dry-run:
- Return mock data matching output schema
- Include metadata: `{"dry_run": true, "simulated": true}`
- Log to causal chain with special marker

### Example

```rust
// In RuntimeHost::execute_capability
if execution_mode == ExecutionMode::DryRun && criticality >= ActionCriticality::Critical {
    // Log simulated call
    let action = Action::new(/*...*/)
        .with_metadata_entry("dry_run", Value::Bool(true))
        .with_metadata_entry("simulated", Value::Bool(true));
    
    // Generate simulated result
    let simulated_result = generate_simulated_result(capability_id, &args)?;
    
    // Log and return simulated result
    self.log_action(action)?;
    return Ok(simulated_result);
}
```

## Approval Gates

### Human-in-the-Loop

For `ExecutionMode::ApprovalGated` or `SafeOnly { continue_on_approval: true }`:

1. When encountering critical action:
   - Pause execution
   - Log `PlanPaused` with reason: "awaiting_approval"
   - Display action details to user
   - Wait for approval/rejection

2. User response:
   - **Approve**: Continue execution
   - **Reject**: Abort plan with `PlanAborted`
   - **Modify**: Allow plan modification, then continue

### Approval Interface

```rust
pub trait ApprovalHandler {
    fn request_approval(
        &self,
        action: &Action,
        capability_id: &str,
        args: &[Value],
    ) -> RuntimeResult<ApprovalDecision>;
}

pub enum ApprovalDecision {
    Approved,
    Rejected { reason: String },
    Modified { new_args: Vec<Value> },
}
```

## Plan Execution Flow

```
Plan Generated
  ↓
Detect Critical Actions
  ↓
Choose Execution Mode:
  - FullExecution → Execute all
  - DryRun → Execute safe, simulate critical
  - SafeOnly → Execute safe, pause for critical
  - ApprovalGated → Pause for each critical
  ↓
Execute Plan Through Orchestrator
  ↓
For each capability:
  - Check criticality
  - Check execution mode
  - Execute / Simulate / Pause
  ↓
All actions logged to Causal Chain
  ↓
Return Results
```

## Implementation Plan

### Phase 1: Criticality Detection
1. Add `criticality` field to `CapabilityManifest`
2. Implement pattern-based detection as fallback
3. Tag capabilities with criticality in marketplace

### Phase 2: Dry-Run Mode
1. Add `ExecutionMode` to `RuntimeContext`
2. Modify `RuntimeHost::execute_capability` to check mode
3. Implement simulated result generation
4. Add dry-run logging to causal chain

### Phase 3: Approval Gates
1. Implement `ApprovalHandler` trait
2. Add pause/resume mechanism in orchestrator
3. Add approval UI/handler for CLI
4. Integrate with plan execution flow

### Phase 4: Planner Integration
1. Add `--dry-run` flag to planner
2. Add `--safe-only` flag to planner
3. Display critical actions summary before execution
4. Request approval for critical actions

## Usage Examples

### Dry-Run
```bash
# Validate plan without executing critical actions
cargo run --example smart_assistant_planner_viz -- \
  --goal "Charge user $100" \
  --dry-run
```

### Safe-Only with Approval
```bash
# Execute safe actions, pause for critical ones
cargo run --example smart_assistant_planner_viz -- \
  --goal "Delete old files and create backup" \
  --safe-only \
  --execute-plan
```

### Full Execution (Explicit)
```bash
# Execute everything including critical actions
cargo run --example smart_assistant_planner_viz -- \
  --goal "Process payment" \
  --execute-plan \
  --force-critical
```

## Benefits

1. **Safety**: Prevents accidental execution of critical actions
2. **Validation**: Dry-run allows testing without side effects
3. **Audit**: All critical actions require explicit approval logged
4. **Flexibility**: Multiple execution modes for different use cases
5. **Transparency**: Clear visibility into plan criticality before execution

