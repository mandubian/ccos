
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
use crate::parser::Parser;
use crate::ast::Expression;

use super::causal_chain::CausalChain;
use super::intent_graph::IntentGraph;
use super::types::{Plan, Action, ActionType, ExecutionResult};

/// The Orchestrator is the stateful engine that drives plan execution.
pub struct Orchestrator {
    causal_chain: Arc<Mutex<CausalChain>>,
    intent_graph: Arc<Mutex<IntentGraph>>,
    capability_marketplace: Arc<CapabilityMarketplace>,
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
        let host = Arc::new(RuntimeHost::new(
            self.causal_chain.clone(),
            self.capability_marketplace.clone(),
            context.clone(),
        ));

        host.set_execution_context(plan_id.clone(), plan.intent_ids.clone(), plan_action_id.clone());

        let mut evaluator = Evaluator::new(host.clone());

        // --- 3. Parse and Execute the Plan Body ---
        let final_result = match Parser::new(&plan.body).parse() {
            Ok(expr) => evaluator.eval(&mut evaluator.env.clone(), &expr),
            Err(e) => Err(RuntimeError::Generic(format!("Failed to parse plan body: {}", e))),
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

    

    /// Helper to log an action to the Causal Chain.
    fn log_action(&self, action: Action) -> RuntimeResult<String> {
        let mut chain = self.causal_chain.lock()
            .map_err(|_| RuntimeError::Generic("Failed to lock CausalChain".to_string()))?;
        chain.append(action)
    }
}
