//! OpenRouter Integration Demo
//!
//! This example demonstrates how to use the LLM arbiter with OpenRouter,
//! which provides access to multiple LLM models through a unified API.

use rtfs_compiler::ccos::arbiter::{
    ArbiterConfig, ArbiterFactory, LlmConfig, LlmProviderType
};
use rtfs_compiler::ccos::intent_graph::IntentGraph;
use std::sync::{Arc, Mutex};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("🚀 OpenRouter LLM Arbiter Demo");
    println!("================================\n");

    // Create OpenRouter configuration
    let config = ArbiterConfig {
        engine_type: rtfs_compiler::ccos::arbiter::ArbiterEngineType::Llm,
    llm_config: Some(LlmConfig {
            provider_type: LlmProviderType::OpenAI, // OpenRouter uses OpenAI-compatible API
            // model: "anthropic/claude-3.5-sonnet".to_string(), // OpenRouter model
            model: "moonshotai/kimi-k2:free".to_string(),
            api_key: std::env::var("OPENROUTER_API_KEY").ok(), // Get from environment
            base_url: Some("https://openrouter.ai/api/v1".to_string()), // OpenRouter API
            max_tokens: Some(2000),
            temperature: Some(0.7),
            timeout_seconds: Some(60),
<<<<<<< HEAD
            prompts: None,
=======
            prompts: Some(rtfs_compiler::ccos::arbiter::prompt::PromptConfig::default()),
>>>>>>> d3d4c9a (Fix: treat unknown string escapes as parse errors; normalize :keys destructuring for AST and IR; update integration test expectation)
        }),
        delegation_config: None,
        capability_config: rtfs_compiler::ccos::arbiter::CapabilityConfig::default(),
        security_config: rtfs_compiler::ccos::arbiter::SecurityConfig::default(),
        template_config: None,
    };

    // Check if API key is available
    if config.llm_config.as_ref().unwrap().api_key.is_none() {
        println!("⚠️  No OpenRouter API key found!");
        println!("   Set OPENROUTER_API_KEY environment variable to use real LLM.");
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
<<<<<<< HEAD
                prompts: None,
=======
                prompts: Some(rtfs_compiler::ccos::arbiter::prompt::PromptConfig::default()),
>>>>>>> d3d4c9a (Fix: treat unknown string escapes as parse errors; normalize :keys destructuring for AST and IR; update integration test expectation)
            }),
            delegation_config: None,
            capability_config: rtfs_compiler::ccos::arbiter::CapabilityConfig::default(),
            security_config: rtfs_compiler::ccos::arbiter::SecurityConfig::default(),
            template_config: None,
        };
        
        run_demo(stub_config).await?;
    } else {
        println!("✅ OpenRouter API key found!");
        println!("   Using model: {}", config.llm_config.as_ref().unwrap().model);
        println!("   Base URL: {}", config.llm_config.as_ref().unwrap().base_url.as_ref().unwrap());
        println!();
        
        run_demo(config).await?;
    }

    Ok(())
}

async fn run_demo(config: ArbiterConfig) -> Result<(), Box<dyn std::error::Error>> {
    // Create intent graph
    let intent_graph = Arc::new(Mutex::new(IntentGraph::new()?));
    
    // Create arbiter
    let arbiter = ArbiterFactory::create_arbiter(config, intent_graph, None).await?;
    
    // Demo requests
    let requests = vec![
        "Analyze user sentiment from recent interactions",
        "Optimize system performance for high load",
        "Generate a weekly report on system metrics",
    ];
    
    for (i, request) in requests.iter().enumerate() {
        println!("📝 Request {}: {}", i + 1, request);
        println!("   Processing...");
        
        match arbiter.process_natural_language(request, None).await {
            Ok(result) => {
                println!("   ✅ Success!");
                println!("   Result: {}", result.value);
                if let Some(metadata) = result.metadata.get("plan_id") {
                    println!("   Plan ID: {}", metadata);
                }
            }
            Err(e) => {
                println!("   ❌ Error: {}", e);
            }
        }
        println!();
    }
    
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[tokio::test]
    async fn test_openrouter_config() {
        let config = ArbiterConfig {
            engine_type: rtfs_compiler::ccos::arbiter::ArbiterEngineType::Llm,
            llm_config: Some(LlmConfig {
                provider_type: LlmProviderType::OpenAI,
                // model: "anthropic/claude-3.5-sonnet".to_string(),
                model: "moonshotai/kimi-k2:free".to_string(),
                api_key: Some("test-key".to_string()),
                base_url: Some("https://openrouter.ai/api/v1".to_string()),
                max_tokens: Some(2000),
                temperature: Some(0.7),
                timeout_seconds: Some(60),
                prompts: None,
            }),
            delegation_config: None,
            capability_config: rtfs_compiler::ccos::arbiter::CapabilityConfig::default(),
            security_config: rtfs_compiler::ccos::arbiter::SecurityConfig::default(),
            template_config: None,
        };
        
        // assert_eq!(config.llm_config.as_ref().unwrap().model, "anthropic/claude-3.5-sonnet");
        assert_eq!(config.llm_config.as_ref().unwrap().model, "moonshotai/kimi-k2:free");
        assert_eq!(config.llm_config.as_ref().unwrap().base_url.as_ref().unwrap(), "https://openrouter.ai/api/v1");
    }
}

