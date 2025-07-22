use std::sync::Arc;

use rtfs_compiler::ccos::arbiter_engine::ArbiterEngine;
use rtfs_compiler::ccos::delegation::{ModelProvider, ModelRegistry};
use rtfs_compiler::ccos::delegating_arbiter::DelegatingArbiter;
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

    // 2. Create the arbiter using our stub
    let arbiter = DelegatingArbiter::new(registry.clone(), "stub-rtfs").unwrap();

    // 3. Create a dummy Intent manually (skip NL phase for simplicity)
    let intent = Intent::new("test goal".to_string()).with_name("stub_intent".to_string());

    // 4. Ask the arbiter to convert Intent → Plan
    let plan = arbiter.intent_to_plan(&intent).await.unwrap();

    // 5. Ensure the plan body is valid RTFS that the parser accepts
    if let PlanBody::Text(code) = &plan.body {
        assert!(rtfs_compiler::parser::parse_expression(code).is_ok(), "Generated RTFS failed to parse");
    } else {
        panic!("Plan body is not textual RTFS");
    }
} 