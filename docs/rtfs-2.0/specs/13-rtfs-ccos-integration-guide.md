# RTFS 2.0 and CCOS Integration Guide

**Status:** Stable  
**Version:** 2.0.0  
**Date:** July 2025  
**Purpose:** Quick reference for understanding RTFS 2.0 and CCOS integration

## TL;DR - Key Differences

| Aspect | RTFS 2.0 Runtime | CCOS Runtime |
|--------|------------------|--------------|
| **Purpose** | Execute RTFS language code | Provide cognitive infrastructure |
| **Scope** | Language semantics & evaluation | Intent tracking, capability management |
| **Security** | Pure by design, no external access | Controls all external interactions |
| **Components** | AST Evaluator, IR Runtime, Step Form | Intent Graph, Causal Chain, etc. |
| **Data Flow** | RTFS expressions → Values | Plans → Actions → Results |
| **Integration** | `(step ...)` special form | Automatic action logging |

## What is RTFS 2.0 Runtime?

**RTFS 2.0 Runtime** = The engine that executes RTFS programming language code

### Two Implementation Strategies:

#### 1. **AST Evaluator** (`evaluator.rs`)
- **What**: Tree-walking interpreter
- **When**: Development, debugging, full feature support needed
- **Performance**: Baseline (1x speed)
- **Features**: 100% language support
- **Maturity**: Production ready

#### 2. **IR Runtime** (`ir_runtime.rs`)  
- **What**: Optimized intermediate representation execution
- **When**: Production, performance-critical applications
- **Performance**: 2-26x faster than AST
- **Features**: 95%+ language support (growing)
- **Maturity**: Operational, actively optimized

### RTFS 2.0 Runtime Responsibilities:
```clojure
;; RTFS Runtime handles:
(let [x 42]                    ; Variable binding
  (map inc [1 2 3]))           ; Function application
                               ; Collection processing
(if condition then-expr else)  ; Control flow
(call "capability" args)       ; ← Only external access point
```

## What is CCOS Runtime?

**CCOS Runtime** = The cognitive infrastructure that surrounds RTFS execution

### Core Components:

#### 1. **Intent Graph**
- **What**: Persistent storage of user goals and intents
- **Why**: Context continuity across sessions

#### 2. **Causal Chain** 
- **What**: Immutable ledger of all actions and decisions
- **Why**: Full auditability and reasoning about causation

#### 3. **Task Context**
- **What**: Context propagation across execution boundaries  
- **Why**: Maintain state across different execution phases

#### 4. **Context Horizon**
- **What**: LLM context window management
- **Why**: Handle large contexts efficiently

#### 5. **Capability Marketplace**
- **What**: Registry and execution engine for external capabilities
- **Why**: Controlled access to system resources

### CCOS Runtime Responsibilities:
```rust
// CCOS Runtime manages:
- User intent: "Analyze quarterly sales data"
- Plan execution: Steps to accomplish the intent  
- Capability calls: http-get, file-read, database-query
- Action tracking: What was done, when, why
- Context management: Maintaining state across calls
- Security enforcement: What's allowed to run
```

## Integration Architecture

### The Bridge: HostInterface

RTFS communicates with external systems through a clean interface that allows for different host implementations:

```rust
// RTFS Runtime communicates through this minimal interface:
pub trait HostInterface {
    fn set_execution_context(&self, plan_id: String, intent_ids: Vec<String>);
    fn execute_capability(&self, name: &str, args: &[Value]) -> RuntimeResult<Value>;
    fn clear_execution_context(&self);
    fn notify_step_started(&self, step_id: &str) -> RuntimeResult<()>;
    fn notify_step_completed(&self, step_id: &str, result: &ExecutionResult) -> RuntimeResult<()>;
    fn notify_step_failed(&self, step_id: &str, error: &str) -> RuntimeResult<()>;
}
```

### Host Implementations

#### PureHost (RTFS-Only Testing)
```rust
pub struct PureHost {
    // Minimal implementation for pure RTFS testing
    // No external dependencies, no CCOS components
}

impl HostInterface for PureHost {
    fn execute_capability(&self, _name: &str, _args: &[Value]) -> RuntimeResult<Value> {
        Err(RuntimeError::CapabilityNotFound("PureHost: No external capabilities available".to_string()))
    }
    // ... other methods return appropriate defaults or errors
}
```

#### RuntimeHost (CCOS Integration)
```rust
pub struct RuntimeHost {
    pub capability_marketplace: Arc<CapabilityMarketplace>,
    pub causal_chain: Arc<Mutex<CausalChain>>,
    pub security_context: RuntimeContext,
    execution_context: RefCell<Option<ExecutionContext>>,
}

#[derive(Clone, Debug)]
struct ExecutionContext {
    plan_id: String,
    intent_ids: Vec<String>,
}
```

#### RuntimeHost Async Bridge (Notes)

- To safely call async capabilities from the sync evaluator, the host bridges with `futures::executor::block_on(...)` when invoking the Capability Marketplace.
- This avoids creating a nested Tokio runtime and prevents the "Cannot start a runtime from within a runtime" panic.

### RTFS Delegation System

RTFS has its own delegation system that determines where function calls should be executed:

```rust
// RTFS-local delegation types
pub enum ExecTarget {
    LocalPure,           // Execute in RTFS evaluator
    LocalModel(String),  // Execute via local model
    RemoteModel(String), // Delegate to remote provider
    CacheHit { storage_pointer: String, signature: String },
}

pub trait DelegationEngine {
    fn decide(&self, ctx: &CallContext) -> ExecTarget;
}

pub struct CallContext<'a> {
    pub fn_symbol: &'a str,
    pub arg_type_fingerprint: u64,
    pub runtime_context_hash: u64,
    pub semantic_hash: Option<Vec<f32>>,
    pub metadata: Option<DelegationMetadata>,
}
```

#### StaticDelegationEngine
```rust
// Simple static mapping for basic delegation
pub struct StaticDelegationEngine {
    static_map: HashMap<String, ExecTarget>,
}

impl DelegationEngine for StaticDelegationEngine {
    fn decide(&self, ctx: &CallContext) -> ExecTarget {
        self.static_map.get(ctx.fn_symbol)
            .cloned()
            .unwrap_or(ExecTarget::LocalPure)
    }
}
```

### CCOS Delegation Integration

CCOS can provide sophisticated delegation implementations while RTFS remains pure:

```rust
// CCOS can implement DelegationEngine for advanced delegation
pub struct CCOSDelegationEngine {
    intent_graph: Arc<IntentGraph>,
    model_registry: Arc<ModelRegistry>,
    l4_cache: Arc<L4CacheClient>,
}

impl DelegationEngine for CCOSDelegationEngine {
    fn decide(&self, ctx: &CallContext) -> ExecTarget {
        // CCOS-specific delegation logic
        // - Check intent graph for preferences
        // - Query model registry for available models
        // - Check L4 cache for cached results
        // - Return appropriate execution target
    }
}
```

### Delegating Arbiter and Model Providers (CCOS)

CCOS can delegate NL→Intent and Intent→Plan to a model provider via the DelegatingArbiter. It reuses the base Arbiter's `IntentGraph` so intents are stored in a single place.

Environment toggles:

```bash
# Enable LLM-driven delegation
export CCOS_USE_DELEGATING_ARBITER=1

# Select model provider (deterministic options for CI)
export CCOS_DELEGATING_MODEL=stub-model   # deterministic
# or
export CCOS_DELEGATING_MODEL=echo-model   # simple echo
```

Model registry defaults include a deterministic `stub-model` suitable for CI and repeatable tests. Plans generated by the delegating path should be validated against the Capability Marketplace before execution.

Parsing note: The Orchestrator trims leading/trailing whitespace on RTFS plan bodies before parsing to avoid position-0 parse errors.

### Streaming Integration

RTFS streaming is handled through CCOS capabilities with proper effect system integration:

```clojure
;; RTFS streaming syntax (handled by CCOS)
(stream-consume 
  :data.source {:url "https://api.example.com/stream"}
  {:buffer-size 1024}
  (fn [chunk] 
    (process-chunk chunk)))

;; This is lowered to:
(call :streaming.consume 
  {:source :data.source 
   :config {:url "https://api.example.com/stream" :buffer-size 1024}
   :callback (fn [chunk] (process-chunk chunk))})
```

**Effect System Integration**: Streaming operations imply specific effects (e.g., `:network`, `:ipc`) that are validated by the Governance Kernel and enforced by CCOS. For detailed effect system specifications, see **[07-effect-system.md](../specs-incoming/07-effect-system.md)**.

**CCOS Execution**: RTFS defines streaming types and macros, while CCOS executes the actual streaming via capabilities in the marketplace.

### Integration Testing Quickstart

- From `rtfs_compiler/`:

```bash
cargo test --test integration_tests -- --nocapture --test-threads 1
```

- End-to-end CCOS flow test ensures: NL→Plan, governance validation, execution, and causal chain logging.

## RTFS 2.0 Object Integration

### Intent Object in CCOS

```clojure
;; RTFS 2.0 Intent Object
(intent
  :type :rtfs.core:v2.0:intent,
  :intent-id "intent-uuid-12345",
  :goal "Analyze quarterly sales performance and create executive summary",
  :created-at "2025-06-23T10:30:00Z",
  :created-by "user:alice@company.com",
  :priority :high,
  :constraints {
    :max-cost 25.00,
    :deadline "2025-06-25T17:00:00Z",
    :data-locality [:US, :EU],
    :security-clearance :confidential
  },
  :success-criteria (fn [result] 
    (and (contains? result :executive-summary)
         (contains? result :key-metrics)
         (> (:confidence result) 0.85)))
)
```

**CCOS Integration:**
- Stored in Intent Graph for persistence
- Referenced by Plans for execution
- Tracked in Causal Chain for auditability

### Plan Object in CCOS

```clojure
;; RTFS 2.0 Plan Object with Step Logging
(plan
  :type :rtfs.core:v2.0:plan,
  :plan-id "plan-uuid-67890",
  :intent-ids ["intent-uuid-12345"],
  :program (do
    ;; Step 1: Fetch data with automatic action logging
    (step "fetch-sales-data"
      (let [sales_data (call :com.acme.db:v1.0:sales-query 
                             {:query "SELECT * FROM sales WHERE quarter = 'Q2-2025'",
                              :format :csv})]
        sales_data))
    
    ;; Step 2: Analyze data with automatic action logging
    (step "analyze-sales-data"
      (let [summary_document (call :com.openai:v1.0:data-analysis
                                   {:data sales_data,
                                    :analysis-type :quarterly-summary,
                                    :output-format :executive-brief})]
        summary_document))
  )
)
```

**Step Integration:**
- The `(step ...)` special form automatically logs `PlanStepStarted` and `PlanStepCompleted` actions
- Each step provides granular audit trail in the Causal Chain
- Step names enable better monitoring and debugging
- Execution context is maintained across step boundaries

**CCOS Integration:**
- Executed by RTFS Runtime
- Actions recorded in Causal Chain
- Results stored as Resources

### Action Object in CCOS

```clojure
;; RTFS 2.0 Action Object
(action
  :type :rtfs.core:v2.0:action,
  :action-id "action-uuid-54321",
  :timestamp "2025-06-23T10:37:15Z",
  :plan-id "plan-uuid-67890",
  :intent-id "intent-uuid-12345",
  :capability-used :com.acme.db:v1.0:sales-query,
  :executor {
    :type :agent,
    :id "agent-db-cluster-1",
    :node "node.us-west.acme.com"
  },
  :input {
    :query "SELECT * FROM sales WHERE quarter = 'Q2-2025'",
    :format :csv
  },
  :output {
    :type :resource,
    :handle "resource://sales-data-q2-2025.csv"
  },
  :execution {
    :started-at "2025-06-23T10:37:15Z",
    :completed-at "2025-06-23T10:37:18Z",
    :duration 3.2,
    :cost 0.45,
    :status :success
  }
)
```

**CCOS Integration:**
- Immutable record in Causal Chain
- Links to Intent, Plan, and Capability
- Provides complete audit trail

## Step Special Form Integration

### The `(step ...)` Primitive

The `(step ...)` special form is a cornerstone of CCOS plan execution, providing automatic action logging to the Causal Chain:

```clojure
(step "step-name" body-expression)
```

**Execution Flow:**
1. **Before**: Logs `PlanStepStarted` action to the Causal Chain
2. **During**: Evaluates the body expression in the current environment
3. **After**: Logs `PlanStepCompleted` or `PlanStepFailed` action with the result

**Implementation:**
```rust
impl Evaluator {
    fn eval_step(&self, step_name: &str, body: &Expression, env: &Environment) -> RuntimeResult<Value> {
        // 1. Log step started
        self.host.log_action(Action::PlanStepStarted {
            step_name: step_name.to_string(),
            plan_id: self.current_plan_id.clone(),
            intent_id: self.current_intent_id.clone(),
            timestamp: Utc::now(),
        })?;
        
        // 2. Evaluate body
        let result = self.eval(body, env)?;
        
        // 3. Log step completed
        self.host.log_action(Action::PlanStepCompleted {
            step_name: step_name.to_string(),
            plan_id: self.current_plan_id.clone(),
            intent_id: self.current_intent_id.clone(),
            result: result.clone(),
            timestamp: Utc::now(),
        })?;
        
        Ok(result)
    }
}
```

**Benefits:**
- **Auditability**: Complete step-level audit trail
- **Debugging**: Step names enable better error tracking
- **Monitoring**: Granular execution monitoring
- **Security**: Step boundaries for security enforcement

### Step vs Function

The `(step ...)` form must be a special form, not a regular function, because:

1. **Evaluation Order**: Functions evaluate arguments first, but `step` needs to log before evaluating the body
2. **Security**: Special forms are part of the trusted language core
3. **Predictability**: Ensures consistent execution order for auditability

### Complete Specification

For the complete specification of the `(step ...)` special form, including detailed semantics, implementation details, usage examples, and testing guidelines, see **[14-step-special-form.md](14-step-special-form.md)**.

## Security Integration

### RTFS Security System

RTFS has its own security system that is independent of CCOS:

```rust
// RTFS-local security types
pub enum IsolationLevel {
    Inherit,    // Inherit from parent context
    Isolated,   // Isolated execution with limited capabilities
    Sandboxed,  // Sandboxed execution with strict security boundaries
}

pub struct RuntimeContext {
    pub isolation_level: IsolationLevel,
    pub allowed_capabilities: HashSet<String>,
    pub allow_isolated_isolation: bool,
    pub allow_sandboxed_isolation: bool,
}

impl RuntimeContext {
    pub fn pure() -> Self {
        Self {
            isolation_level: IsolationLevel::Inherit,
            allowed_capabilities: HashSet::new(),
            allow_isolated_isolation: false,
            allow_sandboxed_isolation: false,
        }
    }
    
    pub fn is_isolation_allowed(&self, level: &IsolationLevel) -> bool {
        match level {
            IsolationLevel::Inherit => true,
            IsolationLevel::Isolated => self.allow_isolated_isolation,
            IsolationLevel::Sandboxed => self.allow_sandboxed_isolation,
        }
    }
}
```

### CCOS Security Integration

CCOS can provide security implementations while RTFS remains pure:

```rust
// CCOS can convert between its security types and RTFS types
impl IsolationLevel {
    pub fn from_ccos(ccos_level: &crate::ccos::execution_context::IsolationLevel) -> Self {
        match ccos_level {
            crate::ccos::execution_context::IsolationLevel::Inherit => IsolationLevel::Inherit,
            crate::ccos::execution_context::IsolationLevel::Isolated => IsolationLevel::Isolated,
            crate::ccos::execution_context::IsolationLevel::Sandboxed => IsolationLevel::Sandboxed,
        }
    }
}
```

### Security Enforcement

```rust
impl RuntimeHost {
    fn execute_capability(&self, capability_name: &str, args: &[Value]) -> RuntimeResult<Value> {
        // 1. Security validation using RTFS security context
        if !self.security_context.is_capability_allowed(capability_name) {
            return Err(RuntimeError::SecurityViolation {
                operation: "call".to_string(),
                capability: capability_name.to_string(),
                context: format!("{:?}", self.security_context),
            });
        }
        
        // 2. Isolation level validation
        if !self.security_context.is_isolation_allowed(&IsolationLevel::Isolated) {
            return Err(RuntimeError::SecurityViolation {
                operation: "isolated_execution".to_string(),
                capability: capability_name.to_string(),
                context: "Isolated execution not allowed".to_string(),
            });
        }
        
        // 3. Prepare arguments
        let capability_args = Value::List(args.to_vec());
        
        // 4. Create action for tracking
        let context = self.execution_context.borrow();
        let (plan_id, intent_id) = match &*context {
            Some(ctx) => (ctx.plan_id.clone(), ctx.intent_ids.get(0).cloned().unwrap_or_default()),
            None => panic!("FATAL: execute_capability called without valid execution context"),
        };
        
        // 5. Execute and record
        let result = self.capability_marketplace.execute_capability(capability_name, &capability_args).await?;
        
        // 6. Record action in causal chain
        self.record_action(plan_id, intent_id, capability_name, args, &result).await?;
        
        Ok(result)
    }
}
```

## Data Flow Example

### Complete Workflow

```rust
// 1. User creates intent
let intent = Intent::new(
    "Analyze quarterly sales data",
    user_id,
    constraints
);
intent_graph.store(intent).await?;

// 2. Arbiter generates plan
let plan = Plan::new(
    intent.id,
    rtfs_program,
    execution_strategy
);
plan_archive.store(plan).await?;

// 3. RTFS Runtime executes plan
let host = RuntimeHost::new(capability_marketplace, causal_chain);
host.set_execution_context(plan.id, vec![intent.id]);

let result = rtfs_runtime.execute(&plan.program, &host).await?;

host.clear_execution_context();

// 4. Results stored as resources
let resource = Resource::new(result, metadata);
resource_store.store(resource).await?;

// 5. Causal chain contains complete audit trail
let actions = causal_chain.get_actions_for_plan(plan.id).await?;
// Contains: intent creation, plan generation, capability calls, result storage
```

## Step-Level Profile Derivation and Enforcement

The RTFS-CCOS integration now includes automatic derivation of security profiles for individual execution steps, providing fine-grained security controls based on the operations being performed.

### Step Profile System

Each execution step gets a comprehensive security profile that includes:

- **Isolation Level**: Inherit, Isolated, or Sandboxed based on operation risk
- **Network Policy**: AllowList, DenyList, Denied, or Full network access  
- **File System Policy**: None, ReadOnly, ReadWrite, or Full access
- **Resource Limits**: CPU, memory, time, and I/O constraints
- **Security Flags**: Syscall filtering, memory protection, monitoring settings

### Automatic Profile Derivation

The `StepProfileDeriver` analyzes RTFS expressions to automatically determine appropriate security settings:

```rust
use rtfs_compiler::ccos::orchestrator::StepProfileDeriver;

let step_expr = parse_expression("(call :http.fetch {:url \"https://api.example.com\"})");
let profile = StepProfileDeriver::derive_profile("fetch-data", &step_expr, &runtime_context)?;

// Automatically derives:
// - IsolationLevel::Isolated (network operation)
// - NetworkPolicy::AllowList(["api.example.com"])
// - SecurityFlags::enable_network_acl = true
// - Resource limits appropriate for network operations
```

### Integration with Orchestrator

The step profile system integrates with the CCOS Orchestrator:

```rust
// Derive profile for a step before execution
orchestrator.derive_step_profile(step_name, &step_expr, &runtime_context)?;

// Get current step profile during execution
let profile = orchestrator.get_current_step_profile();

// Clear profile after step completion
orchestrator.clear_step_profile();
```

### RTFS Step Special Form Integration

The `(step ...)` special form now automatically derives and applies security profiles:

```clojure
; This step will automatically get appropriate security profiles
(step "Fetch external data"
  (call :http.fetch {:url "https://api.example.com/data"}))

; The step special form:
; 1. Derives security profile from the expression
; 2. Logs StepProfileDerived action to Causal Chain
; 3. Applies profile to execution context
; 4. Executes with appropriate isolation level
```

### Causal Chain Integration

All step profile decisions are logged to the Causal Chain:

```rust
// StepProfileDerived action logged to Causal Chain
let action = Action::new(
    ActionType::StepProfileDerived,
    step_name,
    profile.clone(),
    metadata
);
causal_chain.append(action).await?;
```

### Example Integration Flow

```rust
// 1. RTFS step expression parsed
let step_expr = parse_expression("(call :http.fetch {:url \"https://api.com\"})");

// 2. Profile automatically derived
let profile = StepProfileDeriver::derive_profile("fetch-data", &step_expr, &runtime_context)?;

// 3. Profile logged to Causal Chain
let action = Action::new(ActionType::StepProfileDerived, "fetch-data", profile.clone(), metadata);
causal_chain.append(action).await?;

// 4. Profile applied to execution context
host.set_step_profile(profile);

// 5. RTFS execution with security controls
let result = rtfs_runtime.execute(&step_expr, &host).await?;

// 6. Profile cleared after execution
host.clear_step_profile();
```

## Performance Considerations

### Runtime Selection

```rust
// Choose runtime based on requirements
let runtime = if is_production && performance_critical {
    IRRuntime::new()
} else {
    ASTEvaluator::new()
};

// Execute with appropriate runtime
let result = runtime.execute(&plan.program, &host).await?;
```

### Caching Strategy

```rust
// CCOS provides caching for repeated operations
let cached_result = capability_marketplace
    .execute_capability_cached(capability_id, inputs, context)
    .await?;
```

## Testing Integration

### Pure RTFS Testing

RTFS can be tested independently using PureHost:

```rust
use crate::tests::pure::pure_test_utils::*;

#[test]
fn test_pure_rtfs_functionality() {
    // Test pure RTFS language features without CCOS dependencies
    let result = parse_and_evaluate_pure("(+ 1 2)");
    assert_eq!(result.unwrap(), Value::Integer(3));
    
    // Test that capability calls fail appropriately in pure mode
    let result = parse_and_evaluate_pure("(call :http.get {:url \"http://example.com\"})");
    assert!(matches!(result, Err(RuntimeError::CapabilityNotFound(_))));
}

#[test]
fn test_pure_rtfs_with_stdlib() {
    // Test RTFS with standard library loaded
    let result = parse_and_evaluate_pure_with_stdlib("(map inc [1 2 3])");
    assert_eq!(result.unwrap(), Value::List(vec![
        Value::Integer(2), Value::Integer(3), Value::Integer(4)
    ]));
}
```

### CCOS Integration Testing

CCOS integration can be tested using RuntimeHost:

```rust
use crate::tests::ccos::ccos_test_utils::*;

#[tokio::test]
async fn test_rtfs_ccos_integration() {
    // Setup CCOS test environment
    let intent_graph = IntentGraph::new_in_memory();
    let causal_chain = create_ccos_causal_chain();
    let capability_marketplace = create_ccos_capability_marketplace();
    
    let host = create_ccos_runtime_host();
    let evaluator = create_ccos_evaluator();
    
    // Create test intent
    let intent = Intent::new("test goal", "test-user", HashMap::new());
    intent_graph.store(intent.clone()).await?;
    
    // Create test plan
    let plan = Plan::new(
        intent.id,
        "(call :test:capability {:input \"test\"})",
        "scripted-execution"
    );
    
    // Execute
    host.set_execution_context(plan.id, vec![intent.id]);
    let result = evaluator.evaluate(&plan.program)?;
    host.clear_execution_context();
    
    // Verify
    assert!(result.is_ok());
    
    let actions = causal_chain.get_actions_for_plan(plan.id).await?;
    assert_eq!(actions.len(), 1);
}
```

### Test Organization

Tests are organized into two categories:

```
rtfs_compiler/src/tests/
├── pure/                    # Pure RTFS tests (no CCOS dependencies)
│   ├── pure_test_utils.rs   # PureHost-based test utilities
│   ├── primitives_tests.rs  # Basic language feature tests
│   ├── function_tests.rs    # Function definition and call tests
│   └── ...
└── ccos/                    # CCOS integration tests
    ├── ccos_test_utils.rs   # RuntimeHost-based test utilities
    ├── delegation_integration_tests.rs
    └── ...
```

### Running Tests

```bash
# Run pure RTFS tests only
cargo test --lib --test-threads 1

# Run CCOS integration tests
cargo test --test integration_tests -- --nocapture --test-threads 1

# Run all tests
cargo test --all
```

## Conclusion

RTFS 2.0 and CCOS work together to provide a complete cognitive computing solution with clean separation of concerns:

### **RTFS 2.0 (Pure Language Runtime)**
- **Pure Language Execution**: Independent RTFS language runtime with no external dependencies
- **RTFS-Local Systems**: Own delegation system, security system, and isolation levels
- **Host Injection**: Uses injected hosts (PureHost for testing, RuntimeHost for CCOS integration)
- **Special Forms**: `(step ...)` special form for automatic action logging
- **Testing**: Can be tested independently using PureHost

### **CCOS (Cognitive Infrastructure)**
- **Orchestration**: Intent tracking, plan execution, and capability management
- **External Integration**: Capability marketplace, causal chain, and security enforcement
- **Advanced Features**: LLM integration, delegation engines, and context management
- **CCOS-Specific Systems**: Intent graph, plan archive, and execution context

### **Clean Integration**
- **Interface-Based**: RTFS and CCOS communicate through well-defined interfaces
- **Dependency Injection**: RTFS accepts injected hosts and delegation engines
- **Type Conversion**: CCOS can convert between its types and RTFS types
- **Independent Testing**: Each system can be tested independently
- **Extensibility**: CCOS can provide sophisticated implementations while RTFS remains pure

### **Architectural Benefits**
- **Independence**: RTFS can run completely independently of CCOS
- **Testability**: Pure RTFS tests run without CCOS dependencies
- **Maintainability**: Clear boundaries between language runtime and orchestration
- **Extensibility**: CCOS can evolve without affecting RTFS core
- **Security**: RTFS has its own security model independent of CCOS

This decoupled architecture ensures that RTFS 2.0 remains a pure, testable language runtime while CCOS provides the cognitive infrastructure needed for real-world applications. The `(step ...)` special form and host injection pattern provide the crucial links between RTFS execution and CCOS orchestration.

---

**Note**: This guide provides a quick reference for understanding how RTFS 2.0 integrates with the CCOS cognitive architecture. For detailed specifications, see the individual RTFS 2.0 specification documents and the migration plan in `99-rtfs-ccos-decoupling-migration.md`.