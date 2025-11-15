# Criticality-Based Execution Design (CCOS Spec Alignment)

## Goal

Implement safe execution model that prevents automatic execution of critical actions (payments, deletions, etc.) and provides dry-run capability for validation, **leveraging existing CCOS specifications**.

## Problem Statement

When a planner generates a plan containing critical actions:
- **Payments** should not execute multiple times
- **Data deletion** should require explicit approval
- **Irreversible operations** should be validated before execution
- **Dry-run mode** should validate execution without side effects

## Alignment with CCOS Specifications

### Existing Mechanisms (Use These!)

Based on CCOS specs 001, 002, 005, 010:

1. **Intent Constraints** (Spec 001):
   - `intent.constraints` (Map): Already supports `{:max-cost 10.0, :privacy :pii-safe, :security-level :high}`
   - Use `:security-level` and `:privacy-level` from RTFS schemas
   - Add `:execution-mode` constraint (e.g., `:dry-run`, `:safe-only`, `:require-approval`)

2. **Plan Policies** (Spec 002):
   - `plan.policies` (HashMap<String, Value>): Already supports execution policies
   - Can declare: `{"execution_mode": "dry-run"}`, `{"require_approval": true}`, `{"max_cost": 50.0}`

3. **Runtime Context** (Spec 005):
   - `RuntimeContext.quota`: Resource limits (tokens, yields, budget)
   - `RuntimeContext.acl`: Allowed capabilities (deny-by-default)
   - `RuntimeContext.sandbox`: Isolation settings (network, filesystem, etc.)

4. **Governance Kernel** (Spec 005, 010):
   - Enforces Constitution rules on plans/yields
   - Validates against intent constraints and plan policies
   - Applies ACLs and resource limits
   - **This is where criticality enforcement should happen**

### Security Levels (From RTFS Schemas)

Already defined in RTFS object schemas:
- `:security-level`: `[:enum [:low :medium :high :critical]]`
- `:privacy-level`: `[:enum [:public :internal :confidential :secret]]`

These should be used instead of creating a new criticality enum.

## Architecture

### Leveraging Existing Structures

**Intent declares constraints**:
```rust
Intent {
    constraints: {
        "security-level": Value::String("critical"),
        "privacy-level": Value::String("confidential"),
        "execution-mode": Value::String("require-approval"),
        "max-cost": Value::Number(100.0),
    }
}
```

**Plan declares policies**:
```rust
Plan {
    policies: {
        "execution_mode": Value::String("dry-run"),
        "require_approval_for": Value::Array(vec![
            Value::String("payment.*"),
            Value::String("delete.*"),
        ]),
        "max_cost": Value::Number(50.0),
    }
}
```

**Runtime Context enforces limits**:
```rust
RuntimeContext {
    quota: {
        tokens: 8192,
        yields: 10,
        budget: 5.0,  // Max cost before requiring approval
    },
    acl: vec!["storage.read", "nlp.*"],  // Deny-by-default
    sandbox: {
        network: false,  // Block network access
        filesystem: true,  // Allow filesystem
    },
}
```

**Governance Kernel validates**:
- Checks intent constraints against plan policies
- Enforces resource limits (budget, tokens, yields)
- Applies ACL filtering (capabilities must be in ACL)
- Evaluates Constitution rules based on security/privacy levels

### Capability Manifest Enhancement

Capabilities should declare security levels in their effects/metadata:

```rust
pub struct CapabilityManifest {
    // ... existing fields ...
    
    /// Effects this capability can have (already exists)
    pub effects: Vec<String>,
    
    /// Security requirements (add to metadata)
    pub metadata: {
        "security-level": "high",  // low/medium/high/critical
        "irreversible": true,      // Whether operation is irreversible
        "requires-approval": true, // Whether explicit approval needed
        "dry-run-simulatable": true, // Whether can be simulated
    }
}
```

Or detect from capability ID patterns (fallback):
- `payment.*`, `billing.*`, `charge.*` → `security-level: critical`
- `delete.*`, `remove.*`, `destroy.*` → `security-level: critical, irreversible: true`
- `write.*`, `create.*`, `update.*` → `security-level: medium`
- `read.*`, `get.*`, `list.*` → `security-level: low`

### Execution Modes (Use Plan Policies + Intent Constraints)

Execution mode determined by combination of:
1. **Intent constraint**: `intent.constraints["execution-mode"]`
2. **Plan policy**: `plan.policies["execution_mode"]`
3. **Runtime Context**: Can override for testing

Execution modes (as string values in constraints/policies):
- `"full"`: Execute all actions (default for safe operations)
- `"dry-run"`: Validate plan without executing critical actions
- `"safe-only"`: Execute only safe actions, pause for critical ones
- `"require-approval"`: Pause and request approval for each critical action

**Precedence**: Plan policy > Intent constraint > Default (full)

### Governance Kernel Enforcement

The Governance Kernel (spec 005, 010) should:
1. Read `intent.constraints` and `plan.policies`
2. Check capability security levels (from manifest or pattern detection)
3. Apply execution mode:
   - If `dry-run`: Simulate critical capabilities
   - If `require-approval`: Pause before critical capabilities
   - If `safe-only`: Block critical capabilities unless approved
4. Enforce resource limits (`context.quota.budget`, etc.)
5. Apply ACL filtering (`context.acl`)

### Security Level Detection

#### Method 1: Capability Manifest Metadata (Preferred)
Capabilities declare their security level in `manifest.metadata["security-level"]`.

#### Method 2: Pattern Matching (Fallback)
Detect security level from capability ID patterns (in Governance Kernel):

```rust
fn detect_security_level(capability_id: &str) -> String {
    let id_lower = capability_id.to_lowercase();
    
    if id_lower.contains("payment") || id_lower.contains("billing") || 
       id_lower.contains("charge") || id_lower.contains("transfer") {
        return "critical".to_string();
    }
    
    if id_lower.contains("delete") || id_lower.contains("remove") || 
       id_lower.contains("destroy") || id_lower.contains("drop") {
        return "critical".to_string();
    }
    
    if id_lower.contains("write") || id_lower.contains("create") || 
       id_lower.contains("update") || id_lower.contains("modify") {
        return "medium".to_string();
    }
    
    // Default: read operations are safe
    "low".to_string()
}
```

#### Method 3: Governance Kernel Constitution Rules
Constitution rules can declare security levels for capability patterns:
```yaml
rules:
  - id: payment-critical
    condition: ":cap matches /payment.*/"
    action: require-approval
    security-level: critical
```

## Dry-Run Implementation

### Behavior (Governance Kernel Enforcement)

When `plan.policies["execution_mode"] == "dry-run"` or `intent.constraints["execution-mode"] == "dry-run"`:

1. **Low security-level actions**: Execute normally (log real results)
2. **Medium security-level actions**: Execute but log `dry_run: true` in action metadata
3. **High/Critical security-level actions**: 
   - Governance Kernel intercepts before capability execution
   - Log `CapabilityCall` action with `metadata: {"dry_run": true, "simulated": true}`
   - Skip actual capability execution
   - Generate simulated result based on capability output schema
   - Continue plan execution with simulated result

### Simulated Results

For critical capabilities in dry-run:
- Governance Kernel generates mock data matching capability output schema
- Include in action metadata: `{"dry_run": true, "simulated": true, "security_level": "critical"}`
- Log to causal chain with governance decision marker

### Example (Governance Kernel Integration)

```rust
// In GovernanceKernel::validate_and_execute
// Check execution mode from plan policies or intent constraints
let execution_mode = plan.policies.get("execution_mode")
    .or_else(|| intent.constraints.get("execution-mode"))
    .unwrap_or(&Value::String("full".to_string()));

if execution_mode == "dry-run" {
    // Governance Kernel intercepts capability calls
    // In Orchestrator, before executing capability:
    let capability_security_level = detect_security_level(&capability_id);
    
    if capability_security_level == "high" || capability_security_level == "critical" {
        // Generate simulated result
        let simulated_result = generate_simulated_result(capability_id, &args)?;
        
        // Log simulated action to causal chain
        let action = Action::new(/*...*/)
            .with_metadata_entry("dry_run", Value::Bool(true))
            .with_metadata_entry("simulated", Value::Bool(true))
            .with_metadata_entry("security_level", Value::String(capability_security_level));
        
        self.causal_chain.append(&action)?;
        return Ok(simulated_result);
    }
}
```

## Approval Gates (Governance Kernel + Orchestrator)

### Human-in-the-Loop

When `plan.policies["execution_mode"] == "require-approval"` or capability security-level is `critical`:

1. Governance Kernel detects critical capability in plan
2. Orchestrator pauses execution:
   - Log `PlanPaused` action with reason: `"awaiting_approval"`
   - Include capability details, args, security level in metadata
   - Yield `RequiresHost` with approval request

3. Approval handler (implemented at CCOS level):
   - Display action details to user
   - Wait for approval/rejection/modification

4. User response:
   - **Approve**: Resume plan execution (`PlanResumed`)
   - **Reject**: Abort plan with `PlanAborted` (reason: "user_rejected")
   - **Modify**: Update capability args, then resume

### Approval Interface (Extend RuntimeContext)

```rust
// Add to RuntimeContext or as separate ApprovalHandler
pub trait ApprovalHandler: Send + Sync {
    fn request_approval(
        &self,
        plan_id: &PlanId,
        intent_id: &IntentId,
        action: &Action,
        capability_id: &str,
        args: &[Value],
        security_level: &str,
    ) -> RuntimeResult<ApprovalDecision>;
}

pub enum ApprovalDecision {
    Approved,
    Rejected { reason: String },
    Modified { new_args: Vec<Value> },
}
```

Approval decisions are logged to causal chain with governance provenance.

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

## Implementation Plan (Aligned with CCOS Specs)

### Phase 1: Intent & Plan Constraint Support
1. ✅ Intent already has `constraints: HashMap<String, Value>` - document usage
2. ✅ Plan already has `policies: HashMap<String, Value>` - document usage
3. Add standard keys: `"execution-mode"`, `"security-level"`, `"privacy-level"` to constraints/policies
4. Document standard constraint/policy values in specs

### Phase 2: Governance Kernel Enhancement
1. Enhance `GovernanceKernel::validate_and_execute` to:
   - Read `plan.policies["execution_mode"]` and `intent.constraints["execution-mode"]`
   - Detect capability security levels (manifest metadata or pattern matching)
   - Apply execution mode rules (dry-run, require-approval, safe-only)
2. Add security level detection function (pattern-based fallback)
3. Implement simulated result generation for dry-run
4. Add governance decision logging to causal chain

### Phase 3: Runtime Context & ACL Enhancement
1. ✅ `RuntimeContext.quota.budget` already exists - use for cost limits
2. ✅ `RuntimeContext.acl` already exists - use for capability filtering
3. Enhance ACL with security level awareness
4. Add `approval_handler` to `RuntimeContext` for human-in-the-loop

### Phase 4: Orchestrator Integration
1. Modify `Orchestrator::execute_plan` to check execution mode before capability calls
2. Implement pause/resume mechanism for approval gates
3. Integrate Governance Kernel decisions into execution flow
4. Log all governance decisions to causal chain

### Phase 5: Planner Integration
1. Add `--dry-run` flag → sets `plan.policies["execution_mode"] = "dry-run"`
2. Add `--require-approval` flag → sets `plan.policies["execution_mode"] = "require-approval"`
3. Add `--safe-only` flag → sets `plan.policies["execution_mode"] = "safe-only"`
4. Display security level summary before execution
5. Request approval for critical actions (if mode requires)

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

