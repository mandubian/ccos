//! LLM Provider Demo
//!
//! This example demonstrates the LLM provider abstraction and individual components.

use rtfs_compiler::ccos::arbiter::{
    LlmProvider, LlmProviderConfig, LlmProviderType, StubLlmProvider
};
use rtfs_compiler::ccos::arbiter::llm_provider::ValidationResult;
use rtfs_compiler::ccos::types::{Plan, PlanBody, PlanLanguage, StorableIntent, IntentStatus, TriggerSource, GenerationContext};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("🔧 LLM Provider Demo");
    println!("===================\n");

    // Create LLM provider configuration
    let config = LlmProviderConfig {
        provider_type: LlmProviderType::Stub,
        model: "stub-model".to_string(),
        api_key: None,
        base_url: None,
        max_tokens: Some(1000),
        temperature: Some(0.7),
        timeout_seconds: Some(30),
    };

    // Create stub LLM provider
    let provider = StubLlmProvider::new(config);
    println!("✅ Stub LLM Provider initialized\n");

    // Test intent generation
    println!("📝 Testing Intent Generation...");
    let intent = provider.generate_intent("analyze user sentiment", None).await?;
    print_intent(&intent);
    println!();

    // Test plan generation
    println!("📋 Testing Plan Generation...");
    let plan = provider.generate_plan(&intent, None).await?;
    print_plan(&plan);
    println!();

    // Test plan validation
    println!("✅ Testing Plan Validation...");
    let plan_content = match &plan.body {
        PlanBody::Rtfs(content) => content,
        PlanBody::Wasm(_) => "(wasm plan)",
    };
    let validation = provider.validate_plan(plan_content).await?;
    print_validation(&validation);
    println!();

    println!("🎉 All tests passed!");
    Ok(())
}

fn print_intent(intent: &StorableIntent) {
    println!("   Generated Intent:");
    println!("      Name: {:?}", intent.name);
    println!("      Goal: {}", intent.goal);
    println!("      Constraints: {:?}", intent.constraints);
    println!("      Preferences: {:?}", intent.preferences);
    println!("      Status: {:?}", intent.status);
    println!("      Original Request: {}", intent.original_request);
}

fn print_plan(plan: &Plan) {
    println!("   Generated Plan:");
    println!("      Plan ID: {}", plan.plan_id);
    println!("      Name: {:?}", plan.name);
    println!("      Language: {:?}", plan.language);
    println!("      Status: {:?}", plan.status);
    println!("      Intent IDs: {:?}", plan.intent_ids);
    println!("      Body: {:?}", plan.body);
}

fn print_validation(validation: &ValidationResult) {
    println!("   Validation Result:");
    println!("      Is Valid: {}", validation.is_valid);
    println!("      Confidence: {:.2}", validation.confidence);
    println!("      Reasoning: {}", validation.reasoning);
    println!("      Suggestions: {:?}", validation.suggestions);
    println!("      Errors: {:?}", validation.errors);
}
