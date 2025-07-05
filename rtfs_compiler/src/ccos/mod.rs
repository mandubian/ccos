//! Cognitive Computing Operating System (CCOS) Foundation
//!
//! This module implements the core components of the CCOS architecture:
//! - Intent Graph: Persistent storage and virtualization of user intents
//! - Causal Chain: Immutable ledger of all actions and decisions  
//! - Task Context: Context propagation across execution
//! - Context Horizon: Management of LLM context window constraints
//! - Subconscious: Background analysis and wisdom distillation

pub mod arbiter;
pub mod causal_chain;
pub mod context_horizon;
pub mod delegation;
pub mod intent_graph;
pub mod loaders;
pub mod subconscious;
pub mod task_context;
pub mod types;

use crate::runtime::error::RuntimeError;
use crate::runtime::values::Value;
use std::collections::HashMap;
use std::time::{SystemTime, UNIX_EPOCH};

/// The main CCOS runtime that provides cognitive infrastructure
pub struct CCOSRuntime {
    pub intent_graph: intent_graph::IntentGraph,
    pub causal_chain: causal_chain::CausalChain,
    pub task_context: task_context::TaskContext,
    pub context_horizon: context_horizon::ContextHorizonManager,
    pub subconscious: subconscious::SubconsciousV1,
}

impl CCOSRuntime {
    /// Create a new CCOS runtime with default configuration
    pub fn new() -> Result<Self, RuntimeError> {
        Ok(CCOSRuntime {
            intent_graph: intent_graph::IntentGraph::new()?,
            causal_chain: causal_chain::CausalChain::new()?,
            task_context: task_context::TaskContext::new()?,
            context_horizon: context_horizon::ContextHorizonManager::new()?,
            subconscious: subconscious::SubconsciousV1::new()?,
        })
    }

    /// Execute an RTFS plan with full cognitive context
    /// This is the main entry point for cognitive execution
    pub fn execute_with_cognitive_context(
        &mut self,
        plan: &types::Plan,
        user_intent: &str,
    ) -> Result<Value, RuntimeError> {
        // 1. Process user intent and store in Intent Graph
        let intent_id = self.process_user_intent(user_intent)?;

        // 2. Create execution context
        let context_id = self.create_execution_context(&intent_id)?;

        // 3. Load relevant cognitive context
        let context = self.load_cognitive_context(&context_id)?;

        // 4. Execute RTFS plan with context
        let result = self.execute_rtfs_plan(plan, context)?;

        // 5. Update cognitive state
        self.update_cognitive_state(&intent_id, &context_id, result.clone())?;

        Ok(result)
    }

    /// Process user intent and store in Intent Graph
    pub fn process_user_intent(
        &mut self,
        user_intent: &str,
    ) -> Result<types::IntentId, RuntimeError> {
        // Create or find existing intent
        let intent = types::Intent::new(user_intent.to_string())
            .with_metadata("source".to_string(), Value::String("user".to_string()));

        let intent_id = intent.intent_id.clone();
        self.intent_graph.store_intent(intent)?;

        // Find related intents
        let related = self.intent_graph.find_relevant_intents(user_intent);

        // Create relationships
        for related_intent in related {
            if related_intent.intent_id != intent_id {
                self.intent_graph.create_edge(
                    intent_id.clone(),
                    related_intent.intent_id.clone(),
                    types::EdgeType::RelatedTo,
                )?;
            }
        }

        Ok(intent_id)
    }

    /// Create execution context for an intent
    pub fn create_execution_context(
        &mut self,
        intent_id: &types::IntentId,
    ) -> Result<String, RuntimeError> {
        // Create task context
        let context_id = format!("execution_{}", intent_id);
        self.task_context.push_execution_context(context_id.clone());

        // Load intent information into context
        if let Some(intent) = self.intent_graph.get_intent(intent_id) {
            self.task_context
                .set_context("goal".to_string(), Value::String(intent.goal.clone()))?;

            // Add constraints and preferences
            for (key, value) in &intent.constraints {
                self.task_context
                    .set_context(format!("constraint_{}", key), value.clone())?;
            }

            for (key, value) in &intent.preferences {
                self.task_context
                    .set_context(format!("preference_{}", key), value.clone())?;
            }
        }

        Ok(context_id)
    }

    /// Load cognitive context for execution
    pub fn load_cognitive_context(&self, context_id: &str) -> Result<types::Context, RuntimeError> {
        // Load task context
        let default_goal = Value::String("".to_string());
        let goal = self
            .task_context
            .get_context(&"goal".to_string())
            .unwrap_or(&default_goal)
            .as_string()
            .unwrap_or_default();

        let related_intents = self.intent_graph.find_relevant_intents(&goal);

        // Create execution context
        let mut execution_context = types::Context::new();

        // Add related intents (virtualized)
        for intent in related_intents {
            execution_context.intents.push(intent);
        }

        // Apply context horizon constraints
        let task = types::Task {
            task_id: context_id.to_string(),
            description: goal.to_string(),
            metadata: HashMap::new(),
        };

        let horizon_context = self.context_horizon.load_relevant_context(&task)?;

        // Merge horizon context with execution context
        execution_context.intents.extend(horizon_context.intents);
        execution_context.wisdom = horizon_context.wisdom;
        execution_context.plan = horizon_context.plan;

        Ok(execution_context)
    }

    /// Execute RTFS plan with cognitive context
    pub fn execute_rtfs_plan(
        &mut self,
        plan: &types::Plan,
        context: types::Context,
    ) -> Result<Value, RuntimeError> {
        // Start causal chain tracking
        let action = self
            .causal_chain
            .create_action(types::Intent::new("RTFS Plan Execution".to_string()))?;

        // Execute each step with context
        let mut result = Value::Nil;

        // TODO: Integrate with existing RTFS runtime
        // For now, return a placeholder result
        // In the full implementation, this would:
        // 1. Parse the RTFS code in plan.rtfs_code
        // 2. Execute it with the provided context
        // 3. Track each function call in the causal chain
        // 4. Handle delegation decisions (self/local/agent/recursive)

        // Record the execution result
        let execution_result = types::ExecutionResult {
            success: true,
            value: result.clone(),
            metadata: HashMap::new(),
        };

        self.causal_chain.record_result(action, execution_result)?;

        Ok(result)
    }

    /// Update cognitive state after execution
    pub fn update_cognitive_state(
        &mut self,
        intent_id: &types::IntentId,
        context_id: &str,
        result: Value,
    ) -> Result<(), RuntimeError> {
        // Update intent with result
        if let Some(mut intent) = self.intent_graph.get_intent(intent_id).cloned() {
            let execution_result = types::ExecutionResult {
                success: true,
                value: result,
                metadata: HashMap::new(),
            };

            self.intent_graph.update_intent(intent, &execution_result)?;
        }

        // Persist context
        self.task_context.persist_context(context_id.to_string())?;

        Ok(())
    }

    /// Execute an intent with full CCOS cognitive infrastructure
    pub fn execute_intent(
        &mut self,
        intent: types::Intent,
    ) -> Result<types::ExecutionResult, RuntimeError> {
        // 1. Load relevant context (respecting context horizon)
        let task = types::Task {
            task_id: intent.intent_id.clone(),
            description: intent.goal.clone(),
            metadata: intent.metadata.clone(),
        };
        let context = self.context_horizon.load_relevant_context(&task)?;

        // 2. Create causal chain entry
        let action = self.causal_chain.create_action(intent.clone())?;

        // 3. Execute with context awareness
        let plan = types::Plan::new_rtfs("".to_string(), vec![intent.intent_id.clone()]);
        let result = self.execute_rtfs_plan(&plan, context)?;

        // 4. Create execution result
        let execution_result = types::ExecutionResult {
            success: true,
            value: result,
            metadata: HashMap::new(),
        };

        // 5. Record in causal chain
        self.causal_chain
            .record_result(action, execution_result.clone())?;

        // 6. Update intent graph
        self.intent_graph.update_intent(intent, &execution_result)?;

        Ok(execution_result)
    }

    /// Execute a plan with given context and action tracking
    fn execute_with_context(
        &self,
        plan: &types::Plan,
        context: &types::Context,
        action: &types::Action,
    ) -> Result<types::ExecutionResult, RuntimeError> {
        // TODO: Integrate with existing RTFS runtime
        // For now, return a placeholder result
        Ok(types::ExecutionResult {
            success: true,
            value: Value::Nil,
            metadata: HashMap::new(),
        })
    }

    /// Get execution statistics
    pub fn get_execution_stats(&self) -> HashMap<String, Value> {
        let mut stats = HashMap::new();

        // Intent Graph stats
        let intent_counts = self.intent_graph.get_intent_count_by_status();
        stats.insert(
            "active_intents".to_string(),
            Value::Integer(
                *intent_counts
                    .get(&types::IntentStatus::Active)
                    .unwrap_or(&0) as i64,
            ),
        );
        stats.insert(
            "completed_intents".to_string(),
            Value::Integer(
                *intent_counts
                    .get(&types::IntentStatus::Completed)
                    .unwrap_or(&0) as i64,
            ),
        );

        // Causal Chain stats
        stats.insert(
            "total_actions".to_string(),
            Value::Integer(self.causal_chain.get_all_actions().len() as i64),
        );
        stats.insert(
            "total_cost".to_string(),
            Value::Float(self.causal_chain.get_total_cost()),
        );

        // Task Context stats
        stats.insert(
            "context_keys".to_string(),
            Value::Integer(self.task_context.size() as i64),
        );

        stats
    }

    /// Create a context-aware execution frame
    pub fn create_execution_frame(&mut self, frame_id: String) -> Result<(), RuntimeError> {
        self.task_context.push_execution_context(frame_id);
        Ok(())
    }

    /// Set context key in current execution frame
    pub fn set_context_key(&mut self, key: String, value: Value) -> Result<(), RuntimeError> {
        let context_value = task_context::ContextValue::new(value);
        self.task_context
            .set_execution_context(key, context_value)?;
        Ok(())
    }

    /// Get context key from current execution frame
    pub fn get_context_key(&self, key: &str) -> Option<&Value> {
        self.task_context
            .resolve_context_key(&key.to_string())
            .map(|cv| &cv.value)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_ccos_runtime_creation() {
        let runtime = CCOSRuntime::new();
        assert!(runtime.is_ok());
    }

    #[test]
    fn test_intent_processing() {
        let mut runtime = CCOSRuntime::new().unwrap();
        let intent_id = runtime.process_user_intent("Analyze quarterly sales data");
        assert!(intent_id.is_ok());
    }

    #[test]
    fn test_context_creation() {
        let mut runtime = CCOSRuntime::new().unwrap();
        let intent_id = runtime.process_user_intent("Test intent").unwrap();
        let context_id = runtime.create_execution_context(&intent_id);
        assert!(context_id.is_ok());
    }

    #[test]
    fn test_execution_stats() {
        let runtime = CCOSRuntime::new().unwrap();
        let stats = runtime.get_execution_stats();
        assert!(stats.contains_key("active_intents"));
        assert!(stats.contains_key("total_actions"));
    }
}
