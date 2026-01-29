use async_trait::async_trait;
use ccos::arbiter::llm_provider::{LlmProvider, LlmProviderInfo, ValidationResult};
use ccos::arbiter::DelegatingCognitiveEngine;
use ccos::capabilities::registry::CapabilityRegistry;
use ccos::capability_marketplace::CapabilityMarketplace;
use ccos::causal_chain::CausalChain;
use ccos::governance_judge::Judgment;
use ccos::governance_kernel::{GovernanceKernel, SemanticJudgePolicy};
use ccos::intent_graph::IntentGraph;
use ccos::orchestrator::Orchestrator;
use ccos::plan_archive::PlanArchive;
use ccos::types::{Plan, StorableIntent};
use rtfs::runtime::error::RuntimeResult;
use rtfs::runtime::security::RuntimeContext;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use tokio::sync::RwLock;

struct MockLlmProvider {
    judgment: Judgment,
}

impl MockLlmProvider {
    fn new(judgment: Judgment) -> Self {
        Self { judgment }
    }
}

#[async_trait]
impl LlmProvider for MockLlmProvider {
    async fn generate_intent(
        &self,
        _: &str,
        _: Option<HashMap<String, String>>,
    ) -> RuntimeResult<StorableIntent> {
        Err(rtfs::runtime::RuntimeError::Generic(
            "Not implemented".to_string(),
        ))
    }
    async fn generate_plan(
        &self,
        _: &StorableIntent,
        _: Option<HashMap<String, String>>,
    ) -> RuntimeResult<Plan> {
        Err(rtfs::runtime::RuntimeError::Generic(
            "Not implemented".to_string(),
        ))
    }
    async fn validate_plan(&self, _: &str) -> RuntimeResult<ValidationResult> {
        Err(rtfs::runtime::RuntimeError::Generic(
            "Not implemented".to_string(),
        ))
    }
    async fn generate_text(&self, _: &str) -> RuntimeResult<String> {
        Ok(serde_json::to_string(&self.judgment).unwrap())
    }
    fn get_info(&self) -> LlmProviderInfo {
        LlmProviderInfo {
            name: "Mock".to_string(),
            version: "1.0".to_string(),
            model: "mock".to_string(),
            capabilities: vec![],
        }
    }
}

#[tokio::test]
async fn test_semantic_judge_blocks_bad_plan() {
    let causal_chain = Arc::new(Mutex::new(CausalChain::new().unwrap()));
    let intent_graph = Arc::new(Mutex::new(IntentGraph::new().unwrap()));
    let registry = Arc::new(RwLock::new(CapabilityRegistry::new()));
    let marketplace = Arc::new(CapabilityMarketplace::new(registry));
    let plan_archive = Arc::new(PlanArchive::new());

    let orchestrator = Arc::new(Orchestrator::for_test(
        causal_chain,
        intent_graph.clone(),
        marketplace.clone(),
        plan_archive,
    ));

    let kernel = GovernanceKernel::new(
        orchestrator,
        intent_graph.clone(),
        std::collections::HashMap::new(),
    );

    // Add policy to block unsafe plans
    kernel.set_semantic_judge_policy(SemanticJudgePolicy {
        enabled: true,
        fail_open: false,
        risk_threshold: 0.5,
    });

    let mock_llm = Box::new(MockLlmProvider::new(Judgment {
        allowed: false,
        reasoning: "Goal is to delete, but plan only reads".to_string(),
        risk_score: 0.9,
    }));

    let arbiter = Arc::new(DelegatingCognitiveEngine::for_test(
        mock_llm,
        marketplace,
        intent_graph.clone(),
    ));

    kernel.set_arbiter(arbiter);

    let mut plan = Plan::new_rtfs(
        "call :system.read_file { path: \"/etc/passwd\" }".to_string(),
        vec![],
    );

    let intent = StorableIntent::new("Delete all user files".to_string());
    let intent_id = intent.intent_id.clone();
    plan.intent_ids = vec![intent_id.clone()];

    {
        let mut graph = intent_graph.lock().unwrap();
        graph.store_intent(intent).unwrap();
    }

    let context = RuntimeContext::full();

    // Test through validate_and_execute which calls judge_plan_semantically
    let result = kernel.validate_and_execute(plan, &context).await;

    assert!(result.is_err());
    let err_msg = format!("{:?}", result.err().unwrap());
    assert!(err_msg.contains("Plan rejected by semantic judge"));
    assert!(err_msg.contains("Goal is to delete, but plan only reads"));
}

#[tokio::test]
async fn test_judge_plan_allowed() {
    let causal_chain = Arc::new(Mutex::new(CausalChain::new().unwrap()));
    let intent_graph = Arc::new(Mutex::new(IntentGraph::new().unwrap()));
    let registry = Arc::new(RwLock::new(CapabilityRegistry::new()));
    let marketplace = Arc::new(CapabilityMarketplace::new(registry));
    let plan_archive = Arc::new(PlanArchive::new());

    let orchestrator = Arc::new(Orchestrator::for_test(
        causal_chain,
        intent_graph.clone(),
        marketplace.clone(),
        plan_archive,
    ));

    let kernel = GovernanceKernel::new(
        orchestrator,
        intent_graph.clone(),
        std::collections::HashMap::new(),
    );

    kernel.set_semantic_judge_policy(SemanticJudgePolicy {
        enabled: true,
        fail_open: false,
        risk_threshold: 0.5,
    });

    let mock_llm = Box::new(MockLlmProvider::new(Judgment {
        allowed: true,
        reasoning: "Plan correctly implements the goal".to_string(),
        risk_score: 0.1,
    }));

    let arbiter = Arc::new(DelegatingCognitiveEngine::for_test(
        mock_llm,
        marketplace,
        intent_graph.clone(),
    ));

    kernel.set_arbiter(arbiter);

    let mut plan = Plan::new_rtfs(
        "call :system.print { message: \"Hello\" }".to_string(),
        vec![],
    );

    let intent = StorableIntent::new("Say hello to the user".to_string());
    let intent_id = intent.intent_id.clone();
    plan.intent_ids = vec![intent_id.clone()];

    {
        let mut graph = intent_graph.lock().unwrap();
        graph.store_intent(intent).unwrap();
    }

    let context = RuntimeContext::full();

    let result = kernel.validate_and_execute(plan, &context).await;

    // It might fail later because :system.print is not registered, but it should pass the semantic judge
    if let Err(e) = result {
        let err_msg = format!("{:?}", e);
        assert!(!err_msg.contains("Plan rejected by semantic judge"));
    }
}
