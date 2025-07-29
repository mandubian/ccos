//! Cognitive Computing Operating System (CCOS) Foundation
//!
//! This module implements the core architectural components of the CCOS, which
//! orchestrate the process of turning user intent into secure, auditable actions.

// CCOS Architectural Components
pub mod arbiter;
pub mod governance_kernel;
pub mod orchestrator;
pub mod delegating_arbiter;
pub mod arbiter_engine;

// CCOS Data Structures
pub mod causal_chain;
pub mod intent_graph;
pub mod intent_storage;
pub mod types;

// CCOS Delegation and Execution
pub mod delegation;
pub mod delegation_l4;
pub mod remote_models;
pub mod local_models;

// CCOS Infrastructure
pub mod caching;

// CCOS Advanced Components
pub mod context_horizon;
pub mod subconscious;
pub mod task_context;

pub mod loaders;

// --- Core CCOS System ---

use std::sync::{Arc, Mutex};
use std::rc::Rc;

use crate::runtime::capability_marketplace::CapabilityMarketplace;
use crate::runtime::{RTFSRuntime, Runtime, ModuleRegistry};
use crate::runtime::error::{RuntimeError, RuntimeResult};
use crate::runtime::security::RuntimeContext;

use self::arbiter::{Arbiter, ArbiterConfig};
use self::causal_chain::CausalChain;
use self::governance_kernel::GovernanceKernel;
use self::intent_graph::IntentGraph;
use self::orchestrator::Orchestrator;
use self::types::ExecutionResult;

/// The main CCOS system struct, which initializes and holds all core components.
/// This is the primary entry point for interacting with the CCOS.
pub struct CCOS {
    arbiter: Arc<Arbiter>,
    governance_kernel: Arc<GovernanceKernel>,
    // The following components are shared across the system
    intent_graph: Arc<Mutex<IntentGraph>>,
    causal_chain: Arc<Mutex<CausalChain>>,
    capability_marketplace: Arc<CapabilityMarketplace>,
    rtfs_runtime: Arc<Mutex<dyn RTFSRuntime>>,
}

impl CCOS {
    /// Creates and initializes a new CCOS instance.
    pub fn new() -> RuntimeResult<Self> {
        // 1. Initialize shared, stateful components
        let intent_graph = Arc::new(Mutex::new(IntentGraph::new()?));
        let causal_chain = Arc::new(Mutex::new(CausalChain::new()?));
        // TODO: The marketplace should be initialized with discovered capabilities.
        let capability_marketplace = Arc::new(CapabilityMarketplace::new(Default::default()));

        // 2. Initialize architectural components, injecting dependencies
        let orchestrator = Arc::new(Orchestrator::new(
            Arc::clone(&causal_chain),
            Arc::clone(&intent_graph),
            Arc::clone(&capability_marketplace),
        ));

        let governance_kernel = Arc::new(GovernanceKernel::new(orchestrator, Arc::clone(&intent_graph)));

        let arbiter = Arc::new(Arbiter::new(
            ArbiterConfig::default(),
            Arc::clone(&intent_graph),
        ));

        Ok(Self {
            arbiter,
            governance_kernel,
            intent_graph,
            causal_chain,
            capability_marketplace,
            rtfs_runtime: Arc::new(Mutex::new(Runtime::new_with_tree_walking_strategy(Rc::new(ModuleRegistry::new())))),
        })
    }

    /// The main entry point for processing a user request.
    /// This method follows the full CCOS architectural flow:
    /// 1. The Arbiter converts the request into a Plan.
    /// 2. The Governance Kernel validates the Plan.
    /// 3. The Orchestrator executes the validated Plan.
    pub async fn process_request(
        &self,
        natural_language_request: &str,
        security_context: &RuntimeContext,
    ) -> RuntimeResult<ExecutionResult> {
        // 1. Arbiter: Generate a plan from the natural language request.
        let proposed_plan = self.arbiter
            .process_natural_language(natural_language_request, None)
            .await?;

        // 2. Governance Kernel: Validate the plan and execute it via the Orchestrator.
        let result = self.governance_kernel
            .validate_and_execute(proposed_plan, security_context)
            .await?;

        Ok(result)
    }

    // --- Accessors for external analysis ---

    pub fn get_intent_graph(&self) -> Arc<Mutex<IntentGraph>> {
        Arc::clone(&self.intent_graph)
    }

    pub fn get_causal_chain(&self) -> Arc<Mutex<CausalChain>> {
        Arc::clone(&self.causal_chain)
    }
}

// --- Tests ---

#[cfg(test)]
mod tests {
    use super::*;
    use crate::runtime::security::SecurityLevel;

    #[tokio::test]
    async fn test_ccos_end_to_end_flow() {
        // This test demonstrates the full architectural flow from a request
        // to a final (simulated) execution result.

        // 1. Create the CCOS instance
        let ccos = CCOS::new().unwrap();

        // 2. Define a security context for the request
        let context = RuntimeContext {
            security_level: SecurityLevel::Controlled,
            allowed_capabilities: vec![
                ":data.fetch-user-interactions".to_string(),
                ":ml.analyze-sentiment".to_string(),
                ":reporting.generate-sentiment-report".to_string(),
            ].into_iter().collect(),
            ..RuntimeContext::pure()
        };

        // 3. Process a natural language request
        let request = "Could you please analyze the sentiment of our recent users?";
        let result = ccos.process_request(request, &context).await;

        // 4. Assert the outcome
        assert!(result.is_ok());
        let execution_result = result.unwrap();
        assert!(execution_result.success);

        // 5. Verify the Causal Chain for auditability
        let causal_chain_arc = ccos.get_causal_chain();
        let chain = causal_chain_arc.lock().unwrap();
        let actions = chain.get_all_actions();

        // We expect a chain of actions: PlanStarted -> StepStarted -> ... -> StepCompleted -> PlanCompleted
        assert!(actions.len() > 2);
        assert_eq!(actions.first().unwrap().action_type, types::ActionType::PlanStarted);
        assert_eq!(actions.last().unwrap().action_type, types::ActionType::PlanCompleted);
    }
}