//! CCOS Orchestrator
//!
//! This module implements the Orchestrator, the component responsible for driving the
//! execution of a `Plan`. It interprets orchestration primitives like `(step ...)`
//! and ensures that all actions are securely executed and logged to the Causal Chain.
//!
//! The Orchestrator acts as the stateful engine for a plan, sitting between the
//! high-level cognitive reasoning of the Arbiter and the low-level execution of
//! the RTFS runtime and Capability Marketplace.

use std::sync::{Arc, Mutex};
use crate::runtime::capability_marketplace::CapabilityMarketplace;
use crate::runtime::security::RuntimeContext;
use crate::runtime::evaluator::Evaluator;
use crate::runtime::host::RuntimeHost;
use crate::runtime::error::{RuntimeResult, RuntimeError};
use crate::parser::parse_expression;

use super::causal_chain::CausalChain;
use super::intent_graph::IntentGraph;
use super::types::{Plan, Action, ActionType, ExecutionResult, PlanLanguage, PlanBody, IntentStatus};

use crate::runtime::module_runtime::ModuleRegistry;
use crate::ccos::delegation::{DelegationEngine, StaticDelegationEngine};
use crate::runtime::host_interface::HostInterface;
use std::rc::Rc;
use std::collections::HashMap;
use sha2::{Digest, Sha256};
use super::checkpoint_archive::{CheckpointArchive, CheckpointRecord};
use crate::runtime::values::Value as RtfsValue;

/// The Orchestrator is the stateful engine that drives plan execution.
pub struct Orchestrator {
    causal_chain: Arc<Mutex<CausalChain>>,
    intent_graph: Arc<Mutex<IntentGraph>>,
    capability_marketplace: Arc<CapabilityMarketplace>,
    checkpoint_archive: Arc<CheckpointArchive>,
}

impl Orchestrator {
    /// Creates a new Orchestrator.
    pub fn new(
        causal_chain: Arc<Mutex<CausalChain>>,
        intent_graph: Arc<Mutex<IntentGraph>>,
        capability_marketplace: Arc<CapabilityMarketplace>,
    ) -> Self {
        Self {
            causal_chain,
            intent_graph,
            capability_marketplace,
            checkpoint_archive: Arc::new(CheckpointArchive::new()),
        }
    }

    /// Executes a given `Plan` within a specified `RuntimeContext`.
    /// This is the main entry point for the Orchestrator.
    pub async fn execute_plan(
        &self,
        plan: &Plan,
        context: &RuntimeContext,
    ) -> RuntimeResult<ExecutionResult> {
        let plan_id = plan.plan_id.clone();
        let primary_intent_id = plan.intent_ids.first().cloned().unwrap_or_default();

        // --- 1. Log PlanStarted Action ---
        let plan_action_id = self.log_action(
            Action::new(
                ActionType::PlanStarted,
                plan_id.clone(),
                primary_intent_id.clone(),
            ).with_parent(None)
        )?;

        // Mark primary intent as Executing (transition Active -> Executing) before evaluation begins
        if !primary_intent_id.is_empty() {
            if let Ok(mut graph) = self.intent_graph.lock() {
                let _ = graph.set_intent_status_with_audit(
                    &primary_intent_id,
                    IntentStatus::Executing,
                    Some(&plan_id),
                    Some(&plan_action_id),
                );
            }
        }

        // --- 2. Set up the Host and Evaluator ---
        let host = Arc::new(RuntimeHost::new(
            self.causal_chain.clone(),
            self.capability_marketplace.clone(),
            context.clone(),
        ));
        host.set_execution_context(plan_id.clone(), plan.intent_ids.clone(), plan_action_id.clone());
        let module_registry = Rc::new(ModuleRegistry::new());
        let delegation_engine: Arc<dyn DelegationEngine> = Arc::new(StaticDelegationEngine::new(HashMap::new()));
        let host_iface: Arc<dyn HostInterface> = host.clone();
        let evaluator = Evaluator::new(module_registry, delegation_engine, context.clone(), host_iface);
        
        // Initialize context manager for the plan execution
        {
            let mut context_manager = evaluator.context_manager.borrow_mut();
            context_manager.initialize(Some(format!("plan-{}", plan_id)));
        }

        // --- 3. Parse and Execute the Plan Body ---
        let final_result = match &plan.language {
            PlanLanguage::Rtfs20 => {
                match &plan.body {
                    PlanBody::Rtfs(rtfs_code) => {
                        let code = rtfs_code.trim();
                        if code.is_empty() {
                            Err(RuntimeError::Generic("Empty RTFS plan body after trimming".to_string()))
                        } else {
                            match parse_expression(code) {
                                Ok(expr) => evaluator.evaluate(&expr),
                                Err(e) => Err(RuntimeError::Generic(format!("Failed to parse RTFS plan body: {:?}", e))),
                            }
                        }
                    }
                    PlanBody::Wasm(_) => Err(RuntimeError::Generic("RTFS plans must use Rtfs body format".to_string())),
                }
            }
            _ => Err(RuntimeError::Generic(format!("Unsupported plan language: {:?}", plan.language))),
        };

        host.clear_execution_context();

        // --- 4. Log Final Plan Status ---
        // Construct execution_result while ensuring we still update the IntentGraph on failure.
        let (execution_result, error_opt) = match final_result {
            Ok(value) => {
                let res = ExecutionResult { success: true, value, metadata: Default::default() };
                self.log_action(
                    Action::new(
                        ActionType::PlanCompleted,
                        plan_id.clone(),
                        primary_intent_id.clone(),
                    )
                    .with_parent(Some(plan_action_id.clone()))
                    .with_result(res.clone())
                )?;
                (res, None)
            },
            Err(e) => {
                // Log aborted action first
                self.log_action(
                    Action::new(
                        ActionType::PlanAborted,
                        plan_id.clone(),
                        primary_intent_id.clone(),
                    )
                    .with_parent(Some(plan_action_id.clone()))
                    .with_error(&e.to_string())
                )?;
                // Represent failure value explicitly (string) so ExecutionResult always has a Value
                let failure_value = RtfsValue::String(format!("error: {}", e));
                let res = ExecutionResult { success: false, value: failure_value, metadata: Default::default() };
                (res, Some(e))
            }
        };

        // --- 5. Update Intent Graph ---
        // Update the primary intent(s) associated with this plan so the IntentGraph
        // reflects the final outcome of plan execution. We attempt to lock the
        // IntentGraph, load the primary intent, and call the graph's
        // update_intent helper which will set status/updated_at and persist the
        // change via storage.
        {
            let mut graph = self
                .intent_graph
                .lock()
                .map_err(|_| RuntimeError::Generic("Failed to lock IntentGraph".to_string()))?;

            if !primary_intent_id.is_empty() {
                if let Some(pre_intent) = graph.get_intent(&primary_intent_id) {
                    graph
                        .update_intent_with_audit(
                            pre_intent,
                            &execution_result,
                            Some(&plan_id),
                            Some(&plan_action_id),
                        )
                        .map_err(|e| RuntimeError::Generic(format!(
                            "IntentGraph update failed for {}: {:?}",
                            primary_intent_id, e
                        )))?;
                }
            }
        }

    // Propagate original error after updating intent status
    if let Some(err) = error_opt { Err(err) } else { Ok(execution_result) }
    }

    /// Serialize the current execution context from an evaluator (checkpoint helper)
    pub fn serialize_context(&self, evaluator: &Evaluator) -> RuntimeResult<String> {
        evaluator
            .context_manager
            .borrow()
            .serialize()
    }

    /// Restore a serialized execution context into an evaluator (resume helper)
    pub fn deserialize_context(&self, evaluator: &Evaluator, data: &str) -> RuntimeResult<()> {
        evaluator
            .context_manager
            .borrow_mut()
            .deserialize(data)
    }

    /// Create a checkpoint: serialize context and log PlanPaused with checkpoint id
    pub fn checkpoint_plan(
        &self,
        plan_id: &str,
        intent_id: &str,
        evaluator: &Evaluator,
    ) -> RuntimeResult<(String, String)> {
        let serialized = self.serialize_context(evaluator)?;
        let mut hasher = Sha256::new();
        hasher.update(serialized.as_bytes());
        let checkpoint_id = format!("cp-{:x}", hasher.finalize());

        // Log lifecycle event with checkpoint metadata
        let mut chain = self.causal_chain.lock()
            .map_err(|_| RuntimeError::Generic("Failed to lock CausalChain".to_string()))?;
        let action = Action::new(super::types::ActionType::PlanPaused, plan_id.to_string(), intent_id.to_string())
            .with_name("checkpoint")
            .with_args(vec![RtfsValue::String(checkpoint_id.clone())]);
        let _ = chain.append(&action)?;

        // Persist checkpoint
        let record = CheckpointRecord {
            checkpoint_id: checkpoint_id.clone(),
            plan_id: plan_id.to_string(),
            intent_id: intent_id.to_string(),
            serialized_context: serialized.clone(),
            created_at: std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_secs(),
            metadata: HashMap::new(),
        };
        let _id = self.checkpoint_archive.store(record)
            .map_err(|e| RuntimeError::Generic(format!("Failed to store checkpoint: {}", e)))?;

        Ok((checkpoint_id, serialized))
    }

    /// Resume from a checkpoint: restore context and log PlanResumed with checkpoint id
    pub fn resume_plan(
        &self,
        plan_id: &str,
        intent_id: &str,
        evaluator: &Evaluator,
        serialized_context: &str,
    ) -> RuntimeResult<()> {
        // Restore
        self.deserialize_context(evaluator, serialized_context)?;

        // Compute checkpoint id for audit linkage
        let mut hasher = Sha256::new();
        hasher.update(serialized_context.as_bytes());
        let checkpoint_id = format!("cp-{:x}", hasher.finalize());

        // Log resume event
        let mut chain = self.causal_chain.lock()
            .map_err(|_| RuntimeError::Generic("Failed to lock CausalChain".to_string()))?;
        let action = Action::new(super::types::ActionType::PlanResumed, plan_id.to_string(), intent_id.to_string())
            .with_name("resume_from_checkpoint")
            .with_args(vec![RtfsValue::String(checkpoint_id)]);
        let _ = chain.append(&action)?;
        Ok(())
    }

    /// Load a checkpoint by id (if present) and resume
    pub fn resume_plan_from_checkpoint(
        &self,
        plan_id: &str,
        intent_id: &str,
        evaluator: &Evaluator,
        checkpoint_id: &str,
    ) -> RuntimeResult<()> {
        // Try in-memory first, then disk fallback
        let rec = if let Some(rec) = self.checkpoint_archive.get_by_id(checkpoint_id) {
            rec
        } else {
            self.checkpoint_archive
                .load_from_disk(checkpoint_id)
                .ok_or_else(|| RuntimeError::Generic("Checkpoint not found".to_string()))?
        };
        if rec.plan_id != plan_id || rec.intent_id != intent_id {
            return Err(RuntimeError::Generic("Checkpoint does not match plan/intent".to_string()));
        }
        self.resume_plan(plan_id, intent_id, evaluator, &rec.serialized_context)
    }


    /// Helper to log an action to the Causal Chain.
    fn log_action(&self, action: Action) -> RuntimeResult<String> {
        let mut chain = self.causal_chain.lock()
            .map_err(|_| RuntimeError::Generic("Failed to lock CausalChain".to_string()))?;
        chain.append(&action)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ccos::event_sink::CausalChainIntentEventSink;
    use crate::ccos::types::{PlanStatus, StorableIntent};
    use crate::runtime::security::SecurityLevel;

    fn test_context() -> RuntimeContext {
        RuntimeContext {
            security_level: SecurityLevel::Controlled,
            ..RuntimeContext::pure()
        }
    }

    fn make_graph_with_sink(chain: Arc<Mutex<CausalChain>>) -> Arc<Mutex<IntentGraph>> {
        let sink = Arc::new(CausalChainIntentEventSink::new(Arc::clone(&chain)));
        Arc::new(Mutex::new(IntentGraph::with_event_sink(sink).expect("intent graph")))
    }

    fn collect_status_changes(chain: &CausalChain, intent_id: &str) -> Vec<Action> {
        let mut out = Vec::new();
        for a in chain.get_actions_for_intent(&intent_id.to_string()) {
            if a.action_type == ActionType::IntentStatusChanged {
                out.push((*a).clone());
            }
        }
        out
    }

    #[tokio::test]
    async fn orchestrator_emits_executing_and_completed() {
        let chain = Arc::new(Mutex::new(CausalChain::new().expect("chain")));
        let graph = make_graph_with_sink(Arc::clone(&chain));
        let marketplace = Arc::new(CapabilityMarketplace::new(Default::default()));
        let orch = Orchestrator::new(Arc::clone(&chain), Arc::clone(&graph), Arc::clone(&marketplace));

        // Seed an Active intent
    let stored = StorableIntent::new("test goal".to_string());
    let intent_id = stored.intent_id.clone();
        {
            let mut g = graph.lock().unwrap();
            g.store_intent(stored.clone()).expect("store intent");
        }

        // Minimal RTFS plan body that evaluates successfully
        let mut plan = Plan::new_rtfs("42".to_string(), vec![intent_id.clone()]);
        plan.status = PlanStatus::Active;

        let ctx = test_context();
        let result = orch.execute_plan(&plan, &ctx).await.expect("exec ok");
        assert!(result.success);

        // Verify status changes were audited: Active->Executing, Executing->Completed
        let changes = {
            let guard = chain.lock().unwrap();
            collect_status_changes(&guard, &intent_id)
        };
        assert!(changes.len() >= 2, "expected at least 2 status change actions, got {}", changes.len());

        // Find specific transitions via metadata
        let mut saw_active_to_executing = false;
        let mut saw_executing_to_completed = false;
        let mut saw_triggering = false;
        let mut saw_reason_set = false;
        for a in &changes {
            let old_s = a.metadata.get("old_status").and_then(|v| v.as_string());
            let new_s = a.metadata.get("new_status").and_then(|v| v.as_string());
            if a.metadata.get("triggering_action_id").is_some() { saw_triggering = true; }
            if let Some(reason) = a.metadata.get("reason").and_then(|v| v.as_string()) {
                if reason == "IntentGraph: explicit status set" || reason == "IntentGraph: update_intent result" {
                    saw_reason_set = true;
                }
            }
            match (old_s.as_deref(), new_s.as_deref()) {
                (Some("Active"), Some("Executing")) => saw_active_to_executing = true,
                (Some("Executing"), Some("Completed")) => saw_executing_to_completed = true,
                _ => {}
            }
        }
        assert!(saw_active_to_executing, "missing Active->Executing status change");
        assert!(saw_executing_to_completed, "missing Executing->Completed status change");
        assert!(saw_triggering, "missing triggering_action_id metadata on status change");
        assert!(saw_reason_set, "missing expected reason metadata on status change");
    }

    #[tokio::test]
    async fn orchestrator_emits_failed_on_error() {
        let chain = Arc::new(Mutex::new(CausalChain::new().expect("chain")));
        let graph = make_graph_with_sink(Arc::clone(&chain));
        let marketplace = Arc::new(CapabilityMarketplace::new(Default::default()));
        let orch = Orchestrator::new(Arc::clone(&chain), Arc::clone(&graph), Arc::clone(&marketplace));

        // Seed an Active intent
        let stored = StorableIntent::new("test goal".to_string());
        let intent_id = stored.intent_id.clone();
        {
            let mut g = graph.lock().unwrap();
            g.store_intent(stored).expect("store intent");
        }

        // Invalid RTFS to trigger parse error
        let mut plan = Plan::new_rtfs("(this is not valid".to_string(), vec![intent_id.clone()]);
        plan.status = PlanStatus::Active;

        let ctx = test_context();
        let res = orch.execute_plan(&plan, &ctx).await;
        assert!(res.is_err(), "expected parse error");

        // Verify status changes were audited: Active->Executing, Executing->Failed
        let changes = {
            let guard = chain.lock().unwrap();
            collect_status_changes(&guard, &intent_id)
        };
        assert!(changes.len() >= 2, "expected at least 2 status change actions, got {}", changes.len());

        let mut saw_active_to_executing = false;
        let mut saw_executing_to_failed = false;
        let mut saw_triggering = false;
        let mut saw_reason_set = false;
        for a in &changes {
            let old_s = a.metadata.get("old_status").and_then(|v| v.as_string());
            let new_s = a.metadata.get("new_status").and_then(|v| v.as_string());
            if a.metadata.get("triggering_action_id").is_some() { saw_triggering = true; }
            if let Some(reason) = a.metadata.get("reason").and_then(|v| v.as_string()) {
                if reason == "IntentGraph: explicit status set" || reason == "IntentGraph: update_intent result" {
                    saw_reason_set = true;
                }
            }
            match (old_s.as_deref(), new_s.as_deref()) {
                (Some("Active"), Some("Executing")) => saw_active_to_executing = true,
                (Some("Executing"), Some("Failed")) => saw_executing_to_failed = true,
                _ => {}
            }
        }
        assert!(saw_active_to_executing, "missing Active->Executing status change");
        assert!(saw_executing_to_failed, "missing Executing->Failed status change");
        assert!(saw_triggering, "missing triggering_action_id metadata on status change");
        assert!(saw_reason_set, "missing expected reason metadata on status change");
    }
}
