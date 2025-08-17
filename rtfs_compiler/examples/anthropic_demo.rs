use rtfs_compiler::ccos::arbiter::{
    ArbiterConfig, LlmConfig, LlmProviderType, ArbiterFactory,
};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("🚀 Anthropic Claude LLM Arbiter Demo");
    println!("====================================\n");

    // Create Anthropic configuration
    let config = ArbiterConfig {
        engine_type: rtfs_compiler::ccos::arbiter::ArbiterEngineType::Llm,
        llm_config: Some(LlmConfig {
            provider_type: LlmProviderType::Anthropic,
            model: "claude-3-sonnet-20240229".to_string(), // Anthropic Claude model
            api_key: std::env::var("ANTHROPIC_API_KEY").ok(), // Get from environment
            base_url: None, // Use default Anthropic API
            max_tokens: Some(2000),
            temperature: Some(0.7),
            timeout_seconds: Some(60),
        }),
        delegation_config: None,
        capability_config: rtfs_compiler::ccos::arbiter::CapabilityConfig::default(),
        security_config: rtfs_compiler::ccos::arbiter::SecurityConfig::default(),
        template_config: None,
    };

    // Check if API key is available
    if config.llm_config.as_ref().unwrap().api_key.is_none() {
        println!("⚠️  No Anthropic API key found!");
        println!("   Set ANTHROPIC_API_KEY environment variable to use real LLM.");
        println!("   Falling back to stub provider for demo...\n");
        
        // Fall back to stub provider
        let stub_config = ArbiterConfig {
            engine_type: rtfs_compiler::ccos::arbiter::ArbiterEngineType::Llm,
            llm_config: Some(LlmConfig {
                provider_type: LlmProviderType::Stub,
                model: "stub-model".to_string(),
                api_key: None,
                base_url: None,
                max_tokens: Some(1000),
                temperature: Some(0.7),
                timeout_seconds: Some(30),
            }),
            delegation_config: None,
            capability_config: rtfs_compiler::ccos::arbiter::CapabilityConfig::default(),
            security_config: rtfs_compiler::ccos::arbiter::SecurityConfig::default(),
            template_config: None,
        };
        
        run_demo(stub_config).await?;
    } else {
        println!("✅ Anthropic API key found!");
        println!("   Using model: {}", config.llm_config.as_ref().unwrap().model);
        println!("   Provider: Anthropic Claude");
        println!();
        
        run_demo(config).await?;
    }

    Ok(())
}

async fn run_demo(config: ArbiterConfig) -> Result<(), Box<dyn std::error::Error>> {
    // Extract provider type before moving config
    let provider_type = config.llm_config.as_ref().unwrap().provider_type.clone();
    
    // Create intent graph
    let intent_graph = std::sync::Arc::new(std::sync::Mutex::new(
        rtfs_compiler::ccos::intent_graph::IntentGraph::new()?
    ));
    
    // Create arbiter
    let arbiter = ArbiterFactory::create_arbiter(config, intent_graph, None).await?;
    println!("✅ Arbiter created successfully\n");

    // Demo requests
    let demo_requests = vec![
        "analyze user sentiment from recent interactions",
        "optimize database performance for high traffic",
        "create a backup strategy for critical data",
        "implement error handling for the payment system",
    ];

    for (i, request) in demo_requests.iter().enumerate() {
        println!("📝 Demo Request {}: {}", i + 1, request);
        println!("{}", "-".repeat(50));

        match arbiter.process_natural_language(request, None).await {
            Ok(result) => {
                println!("✅ Success!");
                println!("   Result: {}", result.value);
                if let Some(metadata) = result.metadata.get("plan_id") {
                    println!("   Plan ID: {}", metadata);
                }
                if let Some(metadata) = result.metadata.get("intent_id") {
                    println!("   Intent ID: {}", metadata);
                }
            }
            Err(e) => {
                println!("❌ Error: {}", e);
            }
        }
        println!();
    }

    // Summary
    println!("🎉 Demo completed!");
    println!("   Total requests processed: {}", demo_requests.len());
    println!("   Provider: {}", match provider_type {
        LlmProviderType::Anthropic => "Anthropic Claude",
        LlmProviderType::Stub => "Stub (for testing)",
        _ => "Unknown",
    });

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_anthropic_config_creation() {
        let config = ArbiterConfig {
            engine_type: rtfs_compiler::ccos::arbiter::ArbiterEngineType::Llm,
            llm_config: Some(LlmConfig {
                provider_type: LlmProviderType::Anthropic,
                model: "claude-3-sonnet-20240229".to_string(),
                api_key: Some("test-key".to_string()),
                base_url: None,
                max_tokens: Some(2000),
                temperature: Some(0.7),
                timeout_seconds: Some(60),
            }),
            delegation_config: None,
            capability_config: rtfs_compiler::ccos::arbiter::CapabilityConfig::default(),
            security_config: rtfs_compiler::ccos::arbiter::SecurityConfig::default(),
            template_config: None,
        };

        assert_eq!(config.llm_config.as_ref().unwrap().provider_type, LlmProviderType::Anthropic);
        assert_eq!(config.llm_config.as_ref().unwrap().model, "claude-3-sonnet-20240229");
    }
}
