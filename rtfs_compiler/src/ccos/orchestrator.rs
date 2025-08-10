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
use crate::runtime::values::Value;
use crate::runtime::error::{RuntimeResult, RuntimeError};
use crate::parser::parse_expression;
use crate::ast::Expression;

use super::causal_chain::CausalChain;
use super::intent_graph::IntentGraph;
use super::types::{Plan, Action, ActionType, ExecutionResult, PlanLanguage, PlanBody};
use super::execution_context::ContextManager;

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

        // --- 2. Set up the Host and Evaluator ---
        let host = Rc::new(RuntimeHost::new(
            self.causal_chain.clone(),
            self.capability_marketplace.clone(),
            context.clone(),
        ));
        host.set_execution_context(plan_id.clone(), plan.intent_ids.clone(), plan_action_id.clone());
        let module_registry = Rc::new(ModuleRegistry::new());
        let delegation_engine: Arc<dyn DelegationEngine> = Arc::new(StaticDelegationEngine::new(HashMap::new()));
        let host_iface: Rc<dyn HostInterface> = host.clone();
        let mut evaluator = Evaluator::new(module_registry, delegation_engine, context.clone(), host_iface);
        
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
        let execution_result = match final_result {
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
                Ok(res)
            },
            Err(e) => {
                self.log_action(
                    Action::new(
                        ActionType::PlanAborted,
                        plan_id.clone(),
                        primary_intent_id.clone(),
                    )
                    .with_parent(Some(plan_action_id.clone()))
                    .with_error(&e.to_string())
                )?;
                Err(e)
            }
        }?; // Note: This '?' will propagate the error from the Err case

        // --- 5. Update Intent Graph ---
        // TODO: Add logic to update the status of the associated intents in the IntentGraph.

        Ok(execution_result)
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
        let rec = self
            .checkpoint_archive
            .get_by_id(checkpoint_id)
            .ok_or_else(|| RuntimeError::Generic("Checkpoint not found".to_string()))?;
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
