# CCOS Specification 027: Intent Graph Orchestration with Shared Context

**Status:** Proposed
**Version:** 1.0
**Date:** 2025-01-02
**Related:**
- [002 - Plans and Orchestration](./002-plans-and-orchestration.md)
- [001 - Intent Graph](./001-intent-graph.md)
- [015 - Execution Contexts](./015-execution-contexts.md)
- [000 - System Architecture](./000-ccos-architecture.md)

## 1. Abstract

This specification defines how the CCOS Orchestrator extends beyond single-plan execution to coordinate entire intent graphs with shared context and data flow between related intents. It addresses the architectural need for hierarchical execution where parent intents orchestrate children without duplicating work, and where data flows seamlessly between related plan executions.

## 2. Intent Graph Execution Model

### 2.1. Hierarchical Execution Principles

When orchestrating an intent graph, the system follows these principles:

1. **Root Intent**: Represents the overall goal and orchestrates children
2. **Child Intents**: Execute specific sub-tasks with their own plans
3. **Shared Context**: Maintains state and results across all intent executions
4. **Dependency Management**: Ensures proper execution order based on intent relationships

### 2.2. Root Intent Behavior

**Root intents should NOT have execution plans** because:
- They **orchestrate children**, not execute work
- Children's plans **already cover the work needed**
- Root plans create **redundancy and confusion**
- The **orchestrator coordinates execution**, not duplicates it

```rust
// Root intent: Orchestration only
struct Intent {
    intent_id: "root-123",
    goal: "say hi and add 2 and 3",
    plan: None,  // No plan needed!
    children: ["child-1", "child-2"],
    // Purpose: Orchestrate children, not execute work
}
```

### 2.3. Child Intent Behavior

**Child intents execute specific work** and produce results:

```rust
// Child 1: Has a plan to execute
struct Intent {
    intent_id: "child-1", 
    goal: "say hi",
    plan: Some(Plan { body: "(call :ccos.echo \"hi\")" }),
    parent: "root-123",
}

// Child 2: Has a plan to execute  
struct Intent {
    intent_id: "child-2",
    goal: "add 2 and 3", 
    plan: Some(Plan { body: "(call :ccos.math.add 2 3)" }),
    parent: "root-123",
}
```

## 3. Shared Context Architecture

### 3.1. Extend Existing RuntimeContext

Instead of creating a new context system or new capabilities, we **extend the existing RuntimeContext** with cross-plan parameters that can be merged across plan executions by the orchestrator:

```rust
// In src/runtime/security.rs - extend existing RuntimeContext
pub struct RuntimeContext {
    // ... existing security fields ...
    
    // NEW: Cross-plan parameters (extends existing per-plan variable model)
    pub cross_plan_params: std::collections::HashMap<String, Value>,
}
```

### 3.2. Extend Existing ContextManager

The existing `ContextManager` already handles step isolation and variable bindings (`get`/`set!`). We add **read access to cross-plan parameters** as fallback:

```rust
// In src/ccos/execution_context.rs - extend existing ContextManager
impl ContextManager {
    // get(key) continues to read from current/parent contexts
    // and may fall back to cross-plan params when not found locally
    pub fn get_with_cross_plan_fallback(&self, key: &str) -> Option<Value> {
        if let Some(local) = self.get(key) { return Some(local); }
        self.runtime_context
            .cross_plan_params
            .get(key)
            .cloned()
    }
}
```

### 3.3. Context Flow Between Plans

#### 3.3.1. Child Plan (Producer)
```lisp
; Child plan produces result and writes it with existing set! semantics
(do
  (step "compute-sum"
    (set! "sum" (+ 2 3)))) ; Orchestrator will merge this into cross-plan params
```

#### 3.3.2. Sibling/Root Plan (Consumer)
```lisp
; Plan consumes previously produced value using existing get
(do
  (step "say"
    (let [s (get "sum")] ; falls back to cross-plan params if not local
      (call :ccos.echo (str "Hi!" (if s (str " The sum is " s) ""))))))
```

## 4. Orchestration Execution Flow

### 4.1. Main Orchestration Method

```rust
impl Orchestrator {
    /// Execute an entire intent graph with shared context via cross-plan params
    async fn execute_intent_graph(
        &self,
        root_intent_id: &IntentId,
        initial_context: &RuntimeContext,
    ) -> RuntimeResult<ExecutionResult> {
        // 1. Create RuntimeContext with empty cross-plan params
        let mut enhanced_context = initial_context.clone();
        enhanced_context.cross_plan_params.clear();
        
        // 2. Execute children sequentially (or by dependency), merging outputs
        for child_id in self.get_children_order(root_intent_id)? {
            if let Some(child_plan) = self.get_plan_for_intent(child_id)? {
                let child_result = self.execute_plan(&child_plan, &enhanced_context).await?;
                // Merge child's exported variables into cross-plan params
                let exported = self.extract_exported_variables(&child_result);
                enhanced_context.cross_plan_params.extend(exported);
            }
        }
        
        // 3. Execute root (if any) with access to cross-plan params via get fallback
        if let Some(root_plan) = self.get_plan_for_intent(root_intent_id)? {
            self.execute_plan(&root_plan, &enhanced_context).await?;
        }
        
        Ok(ExecutionResult::success())
    }
}
```

### 4.2. Simple Execution Order

The orchestrator uses the existing intent graph structure to determine execution order:

```rust
impl Orchestrator {
    fn get_children_order(&self, root_id: &IntentId) -> RuntimeResult<Vec<String>> {
        let graph = self.intent_graph.lock()?;
        if let Some(intent) = graph.get_intent(root_id) { Ok(intent.child_intents.clone()) } else { Ok(Vec::new()) }
    }
}
```

## 5. Guidance for Plan Generation (LLM)

- Generate one plan per leaf intent; the root intent has no plan.
- Use existing `set!` to publish values you want siblings/parent to see.
- Use existing `get` to read values; it falls back to cross-plan params when not set locally.
- Do not invent new capabilities; only use registered ones (e.g., `:ccos.echo`, `:ccos.math.add`).
- Avoid deprecated patterns (no `:step_1.result`, no custom context calls).

Examples:
```lisp
; Producer
(do (step "compute-sum" (set! "sum" (+ 2 3))))

; Consumer
(do (step "say" (let [s (get "sum")] (call :ccos.echo (str "Hi!" (if s (str " The sum is " s) ""))))))
```

## 6. Removed: Context Capabilities Section

This spec intentionally removes the `:ccos.context.*` capability design in favor of the simpler, existing `get`/`set!` plus orchestrator-managed cross-plan parameter merging.

## 7. Benefits of Intent Graph Orchestration

### 7.1. No Redundancy
- Root intents don't duplicate children's work
- Each intent has a single, clear responsibility
- LLM doesn't waste time generating overlapping plans

### 7.2. Data Flow Between Plans
- Child plan produces result: `sum = 5`
- Root plan can access: `get_intent_result("child-2")` → `5`
- No need to duplicate work or recalculate

### 7.3. Shared Configuration
- Set `max_retries = 3` in shared context
- All plans can access this configuration
- Consistent behavior across execution

### 7.4. Stateful Orchestration
- Track progress across multiple plans
- Handle dependencies and coordination
- Maintain execution state and metadata

## 8. Implementation Plan

### 8.1. Phase 1: Core Infrastructure (Immediate - Week 1-2)

#### **8.1.1. Extend RuntimeContext**
```rust
// In src/runtime/security.rs - add to existing RuntimeContext
pub struct RuntimeContext {
    // ... existing security fields remain unchanged ...
    
    // NEW: Cross-plan parameters
    pub cross_plan_params: std::collections::HashMap<String, Value>,
}
```

#### **8.1.2. Extend ContextManager**
```rust
// In src/ccos/execution_context.rs - add to existing ContextManager
impl ContextManager {
    // ... existing methods remain unchanged ...
    
    /// NEW: Get/set cross-plan context values
    pub fn get_with_cross_plan_fallback(&self, key: &str) -> Option<Value>
}
```

#### **8.1.3. Add Context Capabilities**
```rust
// In src/runtime/stdlib.rs - integrate with existing context system
pub async fn register_context_capabilities(marketplace: &CapabilityMarketplace) -> RuntimeResult<()> {
    // :ccos.context.get and :ccos.context.set
}
```

### 8.2. Phase 2: Intent Graph Orchestration (Short-term - Week 3-4)

#### **8.2.1. Implement execute_intent_graph()**
```rust
// In src/ccos/orchestrator.rs - add new method
impl Orchestrator {
    pub async fn execute_intent_graph(
        &self,
        root_intent_id: &IntentId,
        context: &RuntimeContext,
    ) -> RuntimeResult<ExecutionResult> {
        // Simple implementation using enhanced RuntimeContext
    }
}
```

#### **8.2.2. Add Simple Execution Order**
```rust
// Simple method to get children order
fn get_children_order(&self, root_id: &IntentId) -> RuntimeResult<Vec<String>>
```

### 8.3. Phase 3: Demo Integration (Week 5)

#### **8.3.1. Update Demo**
```rust
// In examples/arbiter_rtfs_graph_demo_live.rs
if node.children.is_empty() {
    execute_plan(plan_info).await?;
} else {
    execute_intent_graph(intent_id).await?;  // NEW
}
```

#### **8.3.2. Test Cross-Plan Context**
- Verify child plans can set cross-plan variables
- Verify root plan can access child results
- Test the complete workflow

## 9. Integration with Existing Components

### 9.1. RuntimeContext Extension
```rust
// Extend existing RuntimeContext with cross-plan context
impl RuntimeContext {
    pub fn with_cross_plan_context(mut self) -> Self {
        self.cross_plan_params.clear(); // Clear existing params
        self
    }
}
```

### 9.2. ContextManager Enhancement
```rust
// Extend existing ContextManager with cross-plan methods
impl ContextManager {
    /// Get a value from cross-plan context
    pub fn get_with_cross_plan_fallback(&self, key: &str) -> Option<Value> {
        // Access cross-plan context from RuntimeContext
        self.runtime_context.cross_plan_params
            .get(key)
            .cloned()
    }
    
    /// Set a value in cross-plan context
    pub fn set_cross_plan(&mut self, key: String, value: Value) -> RuntimeResult<()> {
        // Set in cross-plan context
        self.runtime_context.cross_plan_params.insert(key, value);
        Ok(())
    }
}
```

### 9.3. Demo Integration
```rust
// Modify demo to use execute_intent_graph instead of individual plans
// In examples/arbiter_rtfs_graph_demo_live.rs

if node.children.is_empty() {
    // Execute leaf intent plan
    execute_plan(plan_info).await?;
} else {
    // Execute entire intent graph with cross-plan context
    execute_intent_graph(intent_id).await?;
}
```

## 10. Testing Strategy

### 10.1. Unit Tests
- Test `SharedExecutionContext` creation and management
- Test context capability implementations
- Test execution order analysis algorithms

### 10.2. Integration Tests
- Test intent graph execution with shared context
- Test data flow between child and root plans
- Test context persistence and recovery

### 10.3. Demo Tests
- Test the enhanced demo with intent graph orchestration
- Verify no redundant plan generation for root intents
- Verify proper data flow between intents

## 11. Migration and Backward Compatibility

### 11.1. Backward Compatibility
- Existing single-plan execution continues to work
- New shared context fields are optional in RuntimeContext
- Context capabilities are additive, not breaking

### 11.2. Migration Path
1. Deploy new capabilities alongside existing ones
2. Update orchestrator with new methods (old methods remain)
3. Gradually migrate demos and examples to use new orchestration
4. Remove deprecated single-plan orchestration methods

## 12. Conclusion

This specification transforms the CCOS Orchestrator from a **single-plan executor** into a **full intent graph orchestrator** by **extending existing infrastructure** rather than creating parallel systems.

The key architectural improvements are:
1. **Elimination of redundant root plans** - root intents orchestrate, not execute
2. **Cross-plan context** - extends existing RuntimeContext and ContextManager
3. **Simple execution flow** - children execute first, then root with access to results
4. **Integrated capabilities** - work with existing step context system

This approach maintains the CCOS philosophy of **orchestration over execution** while **leveraging proven infrastructure**:
- ✅ **Extends RuntimeContext** - adds cross-plan capabilities to existing security model
- ✅ **Extends ContextManager** - adds cross-plan methods to existing step context
- ✅ **Integrates capabilities** - works with existing context system
- ✅ **Maintains compatibility** - existing plans continue to work unchanged

**The simpler approach is better** - we build on what works rather than reinventing the wheel.
