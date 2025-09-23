# Simple Implementation Plan: Intent Graph Orchestration

**Status:** Ready to Implement  
**Date:** 2025-01-02  
**Approach:** Extend existing infrastructure (simpler is better)

## 🎯 **What We're Building**

Transform the CCOS Orchestrator from a **single-plan executor** into a **full intent graph orchestrator** by extending existing infrastructure.

## 🏗️ **Architecture: Extend, Don't Replace**

### **Current State**
- ✅ `RuntimeContext` - security and permissions
- ✅ `ContextManager` - step-by-step context within plans
- ✅ `Orchestrator` - executes individual plans

### **What We Add**
- ➕ `CrossPlanContext` - shared context across multiple plans
- ➕ `execute_intent_graph()` - orchestrates entire intent graphs
- ➕ `:ccos.context.*` capabilities - access cross-plan context

## 📋 **Implementation Steps**

### **Phase 1: Core Infrastructure (Week 1-2)**

#### **Step 1.1: Extend RuntimeContext**
```rust
// File: src/runtime/security.rs
pub struct RuntimeContext {
    // ... existing fields remain unchanged ...
    
    // NEW: Cross-plan parameters (merged by orchestrator across plans)
    pub cross_plan_params: HashMap<String, Value>,
}
```

#### **Step 1.2: Extend ContextManager**
```rust
// File: src/ccos/execution_context.rs
impl ContextManager {
    // ... existing methods remain unchanged ...
    
    /// NEW: get with cross-plan fallback
    pub fn get_with_cross_plan_fallback(&self, key: &str) -> Option<Value> {
        if let Some(local) = self.get(key) { return Some(local); }
        self.runtime_context.cross_plan_params.get(key).cloned()
    }
}
```

### **Phase 2: Intent Graph Orchestration (Week 3-4)**

#### **Step 2.1: Add execute_intent_graph() Method**
```rust
// File: src/ccos/orchestrator.rs
impl Orchestrator {
    /// Execute an entire intent graph with cross-plan parameter merging
    pub async fn execute_intent_graph(
        &self,
        root_intent_id: &IntentId,
        initial_context: &RuntimeContext,
    ) -> RuntimeResult<ExecutionResult> {
        // 1. Start with an empty cross-plan param bag
        let mut enhanced_context = initial_context.clone();
        enhanced_context.cross_plan_params.clear();
        
        // 2. Execute children and merge exported vars
        for child_id in self.get_children_order(root_intent_id)? {
            if let Some(child_plan) = self.get_plan_for_intent(child_id)? {
                let child_result = self.execute_plan(&child_plan, &enhanced_context).await?;
                let exported = self.extract_exported_variables(&child_result);
                enhanced_context.cross_plan_params.extend(exported);
            }
        }
        
        // 3. Optionally execute root plan (if any)
        if let Some(root_plan) = self.get_plan_for_intent(root_intent_id)? {
            self.execute_plan(&root_plan, &enhanced_context).await?;
        }
        
        Ok(ExecutionResult::success())
    }
    
    /// Simple method to get children order
    fn get_children_order(&self, root_id: &IntentId) -> RuntimeResult<Vec<String>> {
        let graph = self.intent_graph.lock()?;
        Ok(match graph.get_intent(root_id) { Some(i) => i.child_intents.clone(), None => Vec::new() })
    }
}
```

### **Phase 3: Demo Integration (Week 5)**

#### **Step 3.1: Update Demo**
```rust
// File: examples/arbiter_rtfs_graph_demo_live.rs
// Keep leaf plans separate; orchestrator manages order & data sharing

if node.children.is_empty() {
    execute_plan(plan_info).await?;
} else {
    execute_intent_graph(intent_id).await?;
}
```

#### **Step 3.2: Authoring Guidance (for LLM)**
- Root has no plan; generate one plan per leaf.
- Use `set!` to publish values to siblings/parent (merged by orchestrator).
- Use `get` to consume; runtime falls back to cross-plan params.
- Do not use deprecated forms (no `:step_1.result`, no custom context calls).

### **Phase 4: Tests**
- Unit: cross_plan_params lifecycle; get-with-fallback behavior.
- Integration: data produced by one plan is visible to another via `get`.
- Demo: sequential intents share data without a root plan.

## 🧪 **Testing Strategy**

### **Unit Tests**
- Test `CrossPlanContext` creation and management
- Test `ContextManager` cross-plan methods
- Test context capability implementations

### **Integration Tests**
- Test intent graph execution with cross-plan context
- Test data flow between child and root plans
- Test context persistence across plan executions

### **Demo Tests**
- Test the enhanced demo with intent graph orchestration
- Verify no redundant plan generation for root intents
- Verify proper data flow between intents

## 🔄 **Migration Path**

### **Backward Compatibility**
- ✅ Existing single-plan execution continues to work unchanged
- ✅ New cross-plan context fields are optional in RuntimeContext
- ✅ Context capabilities are additive, not breaking

### **Gradual Migration**
1. Deploy new capabilities alongside existing ones
2. Update orchestrator with new methods (old methods remain)
3. Gradually migrate demos and examples to use new orchestration
4. Remove deprecated single-plan orchestration methods (future)

## 🎯 **Success Criteria**

### **Phase 1 Complete**
- [ ] RuntimeContext extended with cross-plan context
- [ ] ContextManager has cross-plan methods
- [ ] Context capabilities registered and working

### **Phase 2 Complete**
- [ ] `execute_intent_graph()` method implemented
- [ ] Children execute first, then root with access to results
- [ ] Cross-plan context flows between plans

### **Phase 3 Complete**
- [ ] Demo updated to use intent graph orchestration
- [ ] Root intents no longer generate redundant plans
- [ ] Data flows seamlessly between child and root intents

## 🚀 **Why This Approach is Better**

### **1. Simpler Implementation**
- Extends existing infrastructure instead of creating parallel systems
- Reuses proven security and context management patterns
- Less code to write, test, and maintain

### **2. Better Integration**
- Works seamlessly with existing step context system
- Maintains backward compatibility
- Natural extension of current architecture

### **3. Faster Delivery**
- Phase 1: 2 weeks (core infrastructure)
- Phase 2: 2 weeks (orchestration logic)
- Phase 3: 1 week (demo integration)
- **Total: 5 weeks** instead of 8+ weeks

### **4. Lower Risk**
- Builds on proven components
- Gradual migration path
- Easy to rollback if issues arise

## 📚 **References**

- [027 - Intent Graph Orchestration Specification](../specs/027-intent-graph-orchestration.md)
- [015 - Execution Contexts](../specs/015-execution-contexts.md)
- [002 - Plans and Orchestration](../specs/002-plans-and-orchestration.md)

---

**Remember: The simpler approach is better. We build on what works rather than reinventing the wheel.**
