use std::sync::Arc;

use rtfs_compiler::ccos::arbiter::{ArbiterEngine, DelegatingArbiter};
use rtfs_compiler::ccos::delegation::{ModelProvider, ModelRegistry};
use rtfs_compiler::ccos::types::{Intent, PlanBody};

/// A stub LLM that returns hard-coded JSON or RTFS snippets so that we can unit-test
/// the DelegatingArbiter logic without relying on an actual model.
#[derive(Debug)]
struct StubRTFSModel;

impl ModelProvider for StubRTFSModel {
    fn id(&self) -> &'static str {
        "stub-rtfs"
    }

    fn infer(&self, prompt: &str) -> Result<String, Box<dyn std::error::Error + Send + Sync>> {
        // Very naive routing: if the prompt asks for intent JSON we return that,
        // otherwise we assume it asks for RTFS code.
        if prompt.contains("USER_REQUEST") {
            Ok("{\"name\": \"stub_intent\", \"goal\": \"test goal\"}".to_string())
        } else {
            // Minimal RTFS program that the parser should accept.
            Ok("(do (println \"hello\"))".to_string())
        }
    }
}

#[tokio::test]
async fn delegating_arbiter_generates_parsable_rtfs_plan() {
    // 1. Register the stub model
    let registry = Arc::new(ModelRegistry::new());
    registry.register(StubRTFSModel);
    // 2. Create the arbiter using our stub provider configuration
    let llm_config = rtfs_compiler::ccos::arbiter::arbiter_config::LlmConfig {
        provider_type: rtfs_compiler::ccos::arbiter::LlmProviderType::Stub,
        model: "stub-model".to_string(),
        api_key: None,
        base_url: None,
        max_tokens: Some(1000),
        temperature: Some(0.7),
        timeout_seconds: Some(30),
        prompts: None,
    };

    // Keep stub model registration from local edits; use upstream delegation config shape
    let delegation_config = rtfs_compiler::ccos::arbiter::arbiter_config::DelegationConfig {
        enabled: false,
        threshold: 0.65,
        max_candidates: 3,
        min_skill_hits: None,
        agent_registry: rtfs_compiler::ccos::arbiter::arbiter_config::AgentRegistryConfig::default(),
        adaptive_threshold: None,
    };

    let intent_graph = Arc::new(std::sync::Mutex::new(rtfs_compiler::ccos::intent_graph::IntentGraph::new().unwrap()));

    // Construct the delegating arbiter (async constructor)
    let arbiter = DelegatingArbiter::new(llm_config, delegation_config, intent_graph).await.unwrap();

    // 3. Create a dummy Intent manually (skip NL phase for simplicity)
    let intent = Intent::new("test goal".to_string()).with_name("stub_intent".to_string());

    // 4. Ask the arbiter to convert Intent → Plan
    let plan = arbiter.intent_to_plan(&intent).await.unwrap();

    // 5. Ensure the plan body is valid RTFS that the parser accepts
    if let PlanBody::Rtfs(code) = &plan.body {
    assert!(rtfs_compiler::parser::parse_expression(&code).is_ok(), "Generated RTFS failed to parse");
    } else {
        panic!("Plan body is not textual RTFS");
    }
} 