//! LLM Arbiter Example
//!
//! This example demonstrates the LLM arbiter functionality using the stub provider
//! to show how the LLM integration works without the async trait complications.

use rtfs_compiler::ccos::arbiter::llm_provider::{LlmProvider, LlmProviderConfig, StubLlmProvider};
use rtfs_compiler::ccos::arbiter::{ArbiterConfig, LlmProviderType};
use rtfs_compiler::ccos::types::StorableIntent;
use std::collections::HashMap;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("🤖 CCOS LLM Arbiter Example");
    println!("============================\n");

    // Create LLM provider configuration
    let llm_config = LlmProviderConfig {
        provider_type: LlmProviderType::Stub,
        model: "stub-model-v1".to_string(),
        api_key: None,
        base_url: None,
        max_tokens: Some(1000),
        temperature: Some(0.7),
        timeout_seconds: Some(30),
        retry_config: rtfs_compiler::ccos::arbiter::arbiter_config::RetryConfig::default(),
    };

    // Create stub LLM provider
    let provider = StubLlmProvider::new(llm_config);

    println!("✅ Created LLM Provider: {}", provider.get_info().name);
    println!("   Model: {}", provider.get_info().model);
    println!("   Version: {}", provider.get_info().version);
    println!("   Capabilities: {:?}", provider.get_info().capabilities);
    println!();

    // Test intent generation
    println!("🧠 Testing Intent Generation");
    println!("----------------------------");

    let test_requests = vec![
        "analyze user sentiment from chat logs",
        "optimize database performance",
        "generate a weekly report",
        "help me with a general question",
    ];

    for request in test_requests {
        println!("\n📝 Request: \"{}\"", request);

        let intent = provider.generate_intent(request, None).await?;

        println!("   ✅ Generated Intent:");
        println!("      ID: {}", intent.intent_id);
        println!("      Name: {:?}", intent.name);
        println!("      Goal: {}", intent.goal);
        println!("      Constraints: {:?}", intent.constraints);
        println!("      Preferences: {:?}", intent.preferences);
        println!("      Success Criteria: {:?}", intent.success_criteria);
    }

    // Test plan generation
    println!("\n📋 Testing Plan Generation");
    println!("--------------------------");

    let mut constraints = HashMap::new();
    constraints.insert("accuracy".to_string(), "high".to_string());

    let mut preferences = HashMap::new();
    preferences.insert("speed".to_string(), "medium".to_string());

    let test_intent = StorableIntent {
        intent_id: "test_intent_123".to_string(),
        name: Some("analyze_user_sentiment".to_string()),
        original_request: "analyze sentiment".to_string(),
        rtfs_intent_source: "".to_string(),
        goal: "Analyze user sentiment from interactions".to_string(),
        constraints,
        preferences,
        success_criteria: Some("sentiment_analyzed".to_string()),
        parent_intent: None,
        child_intents: vec![],
        triggered_by: rtfs_compiler::ccos::types::TriggerSource::HumanRequest,
        generation_context: rtfs_compiler::ccos::types::GenerationContext {
            arbiter_version: "example".to_string(),
            generation_timestamp: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_secs(),
            input_context: HashMap::new(),
            reasoning_trace: None,
        },
        status: rtfs_compiler::ccos::types::IntentStatus::Active,
        priority: 0,
        created_at: std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs(),
        updated_at: std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs(),
        metadata: HashMap::new(),
    };

    println!("📝 Intent: {:?}", test_intent.name);
    println!("🎯 Goal: {}", test_intent.goal);

    let plan = provider.generate_plan(&test_intent, None).await?;

    println!("   ✅ Generated Plan:");
    println!("      ID: {}", plan.plan_id);
    // Description field may be absent on some Plan shapes; show metadata keys instead
    println!("      Name: {:?}", plan.name);
    println!(
        "      Metadata keys: {:?}",
        plan.metadata.keys().collect::<Vec<_>>()
    );
    println!("      Language: {:?}", plan.language);
    println!("      Status: {:?}", plan.status);

    if let rtfs_compiler::ccos::types::PlanBody::Rtfs(rtfs_code) = &plan.body {
        println!("      RTFS Code:");
        println!("      {}", rtfs_code);

        // Validate using the RTFS string when available
        let validation = provider.validate_plan(rtfs_code).await?;
        println!("   ✅ Validation Result:");
        println!("      Valid: {}", validation.is_valid);
        println!("      Confidence: {:.2}", validation.confidence);
        println!("      Reasoning: {}", validation.reasoning);
        println!("      Suggestions: {:?}", validation.suggestions);
        println!("      Errors: {:?}", validation.errors);
    } else {
        println!("   ⚠️  Cannot validate non-RTFS plan bodies in this example");
    }

    // Show configuration example
    println!("\n⚙️  Configuration Example");
    println!("------------------------");

    let arbiter_config = ArbiterConfig {
        engine_type: rtfs_compiler::ccos::arbiter::arbiter_config::ArbiterEngineType::Llm,
        llm_config: Some(rtfs_compiler::ccos::arbiter::arbiter_config::LlmConfig {
            provider_type: LlmProviderType::Stub,
            model: "stub-model-v1".to_string(),
            api_key: None,
            base_url: None,
            max_tokens: Some(1000),
            temperature: Some(0.7),
            timeout_seconds: Some(30),
            retry_config: rtfs_compiler::ccos::arbiter::arbiter_config::RetryConfig::default(),
            prompts: Some(rtfs_compiler::ccos::arbiter::prompt::PromptConfig::default()),
        }),
        delegation_config: None,
        capability_config: rtfs_compiler::ccos::arbiter::arbiter_config::CapabilityConfig::default(
        ),
        security_config: rtfs_compiler::ccos::arbiter::arbiter_config::SecurityConfig::default(),
        template_config: None,
    };

    println!("   Engine Type: {:?}", arbiter_config.engine_type);
    if let Some(llm_cfg) = &arbiter_config.llm_config {
        println!("   LLM Provider: {:?}", llm_cfg.provider_type);
        println!("   LLM Model: {}", llm_cfg.model);
        println!("   Max Tokens: {:?}", llm_cfg.max_tokens);
        println!("   Temperature: {:?}", llm_cfg.temperature);
    }

    println!("\n🎉 LLM Arbiter Example Completed Successfully!");
    println!("\n💡 Next Steps:");
    println!("   - Implement real LLM providers (OpenAI, Anthropic)");
    println!("   - Add capability marketplace integration");
    println!("   - Implement prompt templates and fine-tuning");
    println!("   - Add plan validation and optimization");

    Ok(())
}
