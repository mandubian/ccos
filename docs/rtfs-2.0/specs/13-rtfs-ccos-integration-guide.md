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

```rust
// RTFS Runtime communicates with CCOS through this interface:
pub trait HostInterface {
    fn set_execution_context(&self, plan_id: String, intent_ids: Vec<String>);
    fn execute_capability(&self, name: &str, args: &[Value]) -> RuntimeResult<Value>;
    fn clear_execution_context(&self);
}
```

### RuntimeHost Implementation

```rust
pub struct RuntimeHost {
    pub capability_marketplace: Arc<CapabilityMarketplace>,
    pub causal_chain: Rc<RefCell<CausalChain>>,
    pub security_context: RuntimeContext,
    execution_context: RefCell<Option<ExecutionContext>>,
}

#[derive(Clone, Debug)]
struct ExecutionContext {
    plan_id: String,
    intent_ids: Vec<String>,
}
```

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

### Security Enforcement

```rust
impl RuntimeHost {
    fn execute_capability(&self, capability_name: &str, args: &[Value]) -> RuntimeResult<Value> {
        // 1. Security validation
        if !self.security_context.is_capability_allowed(capability_name) {
            return Err(RuntimeError::SecurityViolation {
                operation: "call".to_string(),
                capability: capability_name.to_string(),
                context: format!("{:?}", self.security_context),
            });
        }
        
        // 2. Prepare arguments
        let capability_args = Value::List(args.to_vec());
        
        // 3. Create action for tracking
        let context = self.execution_context.borrow();
        let (plan_id, intent_id) = match &*context {
            Some(ctx) => (ctx.plan_id.clone(), ctx.intent_ids.get(0).cloned().unwrap_or_default()),
            None => panic!("FATAL: execute_capability called without valid execution context"),
        };
        
        // 4. Execute and record
        let result = self.capability_marketplace.execute_capability(capability_name, &capability_args).await?;
        
        // 5. Record action in causal chain
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

### Unit Testing

```rust
#[tokio::test]
async fn test_rtfs_ccos_integration() {
    // Setup test environment
    let intent_graph = IntentGraph::new_in_memory();
    let causal_chain = CausalChain::new_in_memory();
    let capability_marketplace = CapabilityMarketplace::new_test();
    
    let host = RuntimeHost::new(capability_marketplace, causal_chain);
    let runtime = ASTEvaluator::new();
    
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
    let result = runtime.execute(&plan.program, &host).await?;
    host.clear_execution_context();
    
    // Verify
    assert!(result.is_ok());
    
    let actions = causal_chain.get_actions_for_plan(plan.id).await?;
    assert_eq!(actions.len(), 1);
}
```

## Conclusion

RTFS 2.0 and CCOS work together to provide a complete cognitive computing solution:

- **RTFS 2.0**: Pure, secure language execution with `(step ...)` special form
- **CCOS**: Cognitive infrastructure and external integration
- **Integration**: Seamless bridge through HostInterface and automatic action logging
- **Security**: Multi-layer validation and audit trail
- **Performance**: Optimized runtime selection and caching

This integration ensures that RTFS 2.0 code can be executed safely within the CCOS cognitive architecture while maintaining the purity and security of the RTFS language. The `(step ...)` special form provides the crucial link between RTFS execution and CCOS audit trails.

---

**Note**: This guide provides a quick reference for understanding how RTFS 2.0 integrates with the CCOS cognitive architecture. For detailed specifications, see the individual RTFS 2.0 specification documents. 