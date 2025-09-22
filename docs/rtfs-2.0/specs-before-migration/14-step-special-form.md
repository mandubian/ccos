# RTFS 2.0 Step Special Form Specification

**Status:** Stable  
**Version:** 2.0.0  
**Date:** July 2025  
**Purpose:** Specification for the `(step ...)` special form for CCOS plan execution

## Overview

The `(step ...)` special form is a cornerstone of RTFS 2.0's integration with the CCOS cognitive architecture. It provides automatic action logging to the Causal Chain, enabling complete audit trails and execution monitoring for plan-based workflows.

## Syntax

```clojure
(step step-name body-expression)
```

### Parameters

- **`step-name`**: String identifier for the step (required)
- **`body-expression`**: RTFS expression to evaluate (required)

### Return Value

Returns the result of evaluating `body-expression`.

## Semantics

### Execution Flow

1. **Pre-execution**: Logs `PlanStepStarted` action to the Causal Chain
2. **Execution**: Evaluates `body-expression` in the current environment
3. **Post-execution**: Logs `PlanStepCompleted` or `PlanStepFailed` action with the result

### Automatic Action Logging

The `(step ...)` form automatically logs the following actions to the CCOS Causal Chain:

#### PlanStepStarted Action
```clojure
(action
  :type :rtfs.core:v2.0:action,
  :action-id "auto-generated-uuid",
  :timestamp "2025-06-23T10:37:15Z",
  :plan-id "current-plan-id",
  :intent-id "current-intent-id",
  :action-type :PlanStepStarted,
  :step-name "step-name",
  :executor {
    :type :rtfs-runtime,
    :version "2.0.0"
  }
)
```

#### PlanStepCompleted Action
```clojure
(action
  :type :rtfs.core:v2.0:action,
  :action-id "auto-generated-uuid",
  :timestamp "2025-06-23T10:37:18Z",
  :plan-id "current-plan-id",
  :intent-id "current-intent-id",
  :action-type :PlanStepCompleted,
  :step-name "step-name",
  :result {
    :type :value,
    :content result-value
  },
  :execution {
    :started-at "2025-06-23T10:37:15Z",
    :completed-at "2025-06-23T10:37:18Z",
    :duration 3.2,
    :status :success
  }
)
```

#### PlanStepFailed Action
```clojure
(action
  :type :rtfs.core:v2.0:action,
  :action-id "auto-generated-uuid",
  :timestamp "2025-06-23T10:37:18Z",
  :plan-id "current-plan-id",
  :intent-id "current-intent-id",
  :action-type :PlanStepFailed,
  :step-name "step-name",
  :error {
    :type :runtime-error,
    :message "error-message",
    :details error-details
  },
  :execution {
    :started-at "2025-06-23T10:37:15Z",
    :completed-at "2025-06-23T10:37:18Z",
    :duration 3.2,
    :status :failed
  }
)
```

## Implementation

### Evaluator Implementation

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
        
        // 2. Evaluate body with timing
        let start_time = Utc::now();
        let result = self.eval(body, env);
        let end_time = Utc::now();
        let duration = (end_time - start_time).num_milliseconds() as f64 / 1000.0;
        
        // 3. Log step completed or failed
        match &result {
            Ok(value) => {
                self.host.log_action(Action::PlanStepCompleted {
                    step_name: step_name.to_string(),
                    plan_id: self.current_plan_id.clone(),
                    intent_id: self.current_intent_id.clone(),
                    result: value.clone(),
                    start_time,
                    end_time,
                    duration,
                })?;
            }
            Err(error) => {
                self.host.log_action(Action::PlanStepFailed {
                    step_name: step_name.to_string(),
                    plan_id: self.current_plan_id.clone(),
                    intent_id: self.current_intent_id.clone(),
                    error: error.clone(),
                    start_time,
                    end_time,
                    duration,
                })?;
            }
        }
        
        result
    }
}
```

### Host Interface Integration

```rust
pub trait HostInterface {
    fn log_action(&self, action: Action) -> RuntimeResult<()>;
    // ... other methods
}

#[derive(Clone, Debug)]
pub enum Action {
    PlanStepStarted {
        step_name: String,
        plan_id: String,
        intent_id: String,
        timestamp: DateTime<Utc>,
    },
    PlanStepCompleted {
        step_name: String,
        plan_id: String,
        intent_id: String,
        result: Value,
        start_time: DateTime<Utc>,
        end_time: DateTime<Utc>,
        duration: f64,
    },
    PlanStepFailed {
        step_name: String,
        plan_id: String,
        intent_id: String,
        error: RuntimeError,
        start_time: DateTime<Utc>,
        end_time: DateTime<Utc>,
        duration: f64,
    },
}
```

## Usage Examples

### Basic Step Usage

```clojure
(plan
  :plan-id "plan-123",
  :intent-ids ["intent-456"],
  :program (do
    ;; Step 1: Fetch data
    (step "fetch-sales-data"
      (call :com.acme.db:v1.0:sales-query 
            {:query "SELECT * FROM sales WHERE quarter = 'Q2-2025'",
             :format :csv}))
    
    ;; Step 2: Process data
    (step "process-sales-data"
      (let [raw_data (call :com.acme.db:v1.0:sales-query 
                           {:query "SELECT * FROM sales WHERE quarter = 'Q2-2025'",
                            :format :csv})]
        (call :com.openai:v1.0:data-analysis
              {:data raw_data,
               :analysis-type :quarterly-summary,
               :output-format :executive-brief})))
  )
)
```

### Nested Steps

```clojure
(step "complex-analysis"
  (do
    (step "data-validation"
      (validate-sales-data sales_data))
    
    (step "statistical-analysis"
      (perform-statistical-analysis validated_data))
    
    (step "report-generation"
      (generate-executive-report analysis_results))))
```

### Error Handling

```clojure
(step "robust-data-processing"
  (try
    (call :com.acme.db:v1.0:sales-query 
          {:query "SELECT * FROM sales WHERE quarter = 'Q2-2025'",
           :format :csv})
    (catch error
      (log-error "Database query failed" error)
      (call :com.acme.db:v1.0:backup-query 
            {:query "SELECT * FROM sales_backup WHERE quarter = 'Q2-2025'",
             :format :csv}))))
```

## Benefits

### 1. **Complete Audit Trail**
- Every step execution is automatically logged
- Full traceability from intent to result
- Immutable record in Causal Chain

### 2. **Enhanced Debugging**
- Step names provide clear execution boundaries
- Detailed timing information for performance analysis
- Error context preservation

### 3. **Security Enforcement**
- Step boundaries for security policy enforcement
- Granular access control at step level
- Complete action history for compliance

### 4. **Monitoring and Observability**
- Real-time execution monitoring
- Performance metrics collection
- Progress tracking for long-running plans

## Integration with CCOS

### Causal Chain Integration

The `(step ...)` form integrates seamlessly with the CCOS Causal Chain:

```rust
impl RuntimeHost {
    fn log_action(&self, action: Action) -> RuntimeResult<()> {
        let action_record = match action {
            Action::PlanStepStarted { step_name, plan_id, intent_id, timestamp } => {
                ActionRecord::new(
                    format!("PlanStepStarted: {}", step_name),
                    plan_id,
                    intent_id,
                    timestamp,
                    None,
                    None
                )
            }
            Action::PlanStepCompleted { step_name, plan_id, intent_id, result, start_time, end_time, duration } => {
                ActionRecord::new(
                    format!("PlanStepCompleted: {}", step_name),
                    plan_id,
                    intent_id,
                    end_time,
                    Some(result),
                    Some(duration)
                )
            }
            Action::PlanStepFailed { step_name, plan_id, intent_id, error, start_time, end_time, duration } => {
                ActionRecord::new(
                    format!("PlanStepFailed: {}", step_name),
                    plan_id,
                    intent_id,
                    end_time,
                    Some(Value::String(error.to_string())),
                    Some(duration)
                )
            }
        };
        
        self.causal_chain.record_action(action_record).await?;
        Ok(())
    }
}
```

### Execution Context Management

The step form maintains execution context across boundaries:

```rust
impl Evaluator {
    fn eval_step(&self, step_name: &str, body: &Expression, env: &Environment) -> RuntimeResult<Value> {
        // Preserve current execution context
        let current_context = self.execution_context.clone();
        
        // Update context with step information
        self.execution_context.step_name = Some(step_name.to_string());
        self.execution_context.step_start_time = Some(Utc::now());
        
        // Execute step
        let result = self.eval(body, env);
        
        // Restore original context
        self.execution_context = current_context;
        
        result
    }
}
```

## Security Considerations

### 1. **Step Name Validation**
- Step names must be valid identifiers
- No sensitive information in step names
- Sanitization for logging and display

### 2. **Execution Context Security**
- Execution context is immutable during step execution
- No unauthorized context modification
- Secure context propagation

### 3. **Action Logging Security**
- All actions are cryptographically signed
- Immutable logging to prevent tampering
- Access control for action retrieval

## Performance Considerations

### 1. **Minimal Overhead**
- Action logging is asynchronous where possible
- Efficient serialization of action data
- Optimized Causal Chain storage

### 2. **Batching**
- Multiple actions can be batched for efficiency
- Configurable batching policies
- Graceful degradation under load

### 3. **Caching**
- Step results can be cached for repeated execution
- Intelligent cache invalidation
- Performance monitoring and optimization

## Testing

### Unit Tests

```rust
#[tokio::test]
async fn test_step_special_form() {
    let host = TestHost::new();
    let evaluator = ASTEvaluator::new();
    
    // Test successful step execution
    let result = evaluator.eval_step("test-step", &test_expression, &env).await?;
    assert!(result.is_ok());
    
    // Verify action logging
    let actions = host.get_logged_actions().await?;
    assert_eq!(actions.len(), 2); // Started + Completed
    assert_eq!(actions[0].action_type, "PlanStepStarted");
    assert_eq!(actions[1].action_type, "PlanStepCompleted");
}

#[tokio::test]
async fn test_step_error_handling() {
    let host = TestHost::new();
    let evaluator = ASTEvaluator::new();
    
    // Test step with error
    let result = evaluator.eval_step("error-step", &error_expression, &env).await?;
    assert!(result.is_err());
    
    // Verify error action logging
    let actions = host.get_logged_actions().await?;
    assert_eq!(actions.len(), 2); // Started + Failed
    assert_eq!(actions[1].action_type, "PlanStepFailed");
}
```

### Integration Tests

```rust
#[tokio::test]
async fn test_step_ccos_integration() {
    let intent_graph = IntentGraph::new_in_memory();
    let causal_chain = CausalChain::new_in_memory();
    let host = RuntimeHost::new(capability_marketplace, causal_chain);
    
    // Execute plan with steps
    let plan = Plan::new(
        "test-plan",
        "(step \"test-step\" (call :test:capability {:input \"test\"}))",
        "scripted-execution"
    );
    
    let result = rtfs_runtime.execute(&plan.program, &host).await?;
    assert!(result.is_ok());
    
    // Verify Causal Chain integration
    let actions = causal_chain.get_actions_for_plan("test-plan").await?;
    assert!(actions.iter().any(|a| a.description.contains("PlanStepStarted")));
    assert!(actions.iter().any(|a| a.description.contains("PlanStepCompleted")));
}
```

## Migration from RTFS 1.0

### Breaking Changes
- The `(step ...)` form is new in RTFS 2.0
- No direct migration required from RTFS 1.0
- Existing code continues to work without steps

### Adoption Strategy
1. **Gradual Adoption**: Add steps to new plans first
2. **Refactoring**: Convert existing plans to use steps
3. **Monitoring**: Leverage step logging for better observability
4. **Optimization**: Use step boundaries for performance tuning

## Conclusion

The `(step ...)` special form provides a powerful mechanism for integrating RTFS 2.0 with the CCOS cognitive architecture. It enables:

- **Complete audit trails** for all plan executions
- **Enhanced debugging** and monitoring capabilities
- **Security enforcement** at step boundaries
- **Performance optimization** through detailed metrics

This integration ensures that RTFS 2.0 plans can be executed with full transparency and accountability within the CCOS cognitive infrastructure.

---

**Note**: This specification defines the complete behavior of the `(step ...)` special form in RTFS 2.0. For implementation details, see the integration guide and runtime documentation. 