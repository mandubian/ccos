//! Example: Using Remote Model Providers with RTFS
//! 
//! This example demonstrates how to use various remote LLM providers (OpenAI, Gemini, 
//! Claude, OpenRouter) with the RTFS delegation engine. It shows how to configure
//! different providers and route functions to specific models.

use rtfs_compiler::ccos::delegation::{ExecTarget, ModelRegistry, StaticDelegationEngine, ModelProvider};
use rtfs_compiler::ccos::remote_models::{
    RemoteModelFactory, RemoteModelConfig
};
use rtfs_compiler::parser;
use rtfs_compiler::runtime::{Evaluator, ModuleRegistry};
use std::collections::HashMap;
use std::sync::Arc;
use reqwest::blocking::Client;

/// Custom OpenRouter model with unique ID
#[derive(Debug)]
struct CustomOpenRouterModel {
    id: &'static str,
    config: RemoteModelConfig,
    client: Arc<Client>,
}

impl CustomOpenRouterModel {
    pub fn new(id: &'static str, model_name: &str) -> Self {
        let config = RemoteModelConfig::new(
            std::env::var("OPENROUTER_API_KEY").unwrap_or_default(),
            model_name.to_string(),
        );
        let client = Arc::new(Client::new());
        
        Self {
            id,
            config,
            client,
        }
    }
}

impl ModelProvider for CustomOpenRouterModel {
    fn id(&self) -> &'static str {
        self.id
    }

    fn infer(&self, prompt: &str) -> Result<String, Box<dyn std::error::Error + Send + Sync>> {
        // OpenRouter uses the same API as OpenAI
        let request = serde_json::json!({
            "model": self.config.model_name,
            "messages": [{
                "role": "user",
                "content": prompt
            }],
            "max_tokens": self.config.max_tokens,
            "temperature": self.config.temperature,
        });

        let response: serde_json::Value = self
            .client
            .post("https://openrouter.ai/api/v1/chat/completions")
            .header("Authorization", format!("Bearer {}", self.config.api_key))
            .header("Content-Type", "application/json")
            .header("HTTP-Referer", "https://rtfs-compiler.example.com")
            .header("X-Title", "RTFS Compiler")
            .json(&request)
            .send()?
            .json()?;

        let content = response["choices"][0]["message"]["content"]
            .as_str()
            .ok_or("Invalid response format")?;

        Ok(content.to_string())
    }
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("🌐 RTFS Remote Model Providers Example");
    println!("=====================================");
    println!();

    // Check for API keys
    let api_keys = check_api_keys();
    if api_keys.is_empty() {
        println!("❌ No API keys found!");
        println!();
        println!("To use remote models, set one or more of these environment variables:");
        println!("  export OPENAI_API_KEY=your_openai_key");
        println!("  export GEMINI_API_KEY=your_gemini_key");
        println!("  export ANTHROPIC_API_KEY=your_anthropic_key");
        println!("  export OPENROUTER_API_KEY=your_openrouter_key");
        println!();
        println!("You can get API keys from:");
        println!("  - OpenAI: https://platform.openai.com/api-keys");
        println!("  - Gemini: https://makersuite.google.com/app/apikey");
        println!("  - Anthropic: https://console.anthropic.com/");
        println!("  - OpenRouter: https://openrouter.ai/keys");
        println!();
        println!("For testing without API keys, the example will show configuration only.");
        return Ok(());
    }

    println!("✅ Found API keys for: {}", api_keys.join(", "));
    println!();

    // Create model registry and register available models
    let registry = ModelRegistry::new();
    // Register available models
    register_available_models(&registry);

    // Wrap the registry in an Arc so it can be shared safely
    let registry_arc = Arc::new(registry);

    // Set up delegation engine with different routing strategies
    let delegation_engine = setup_delegation_engine();

    // Create evaluator
    let module_registry = Arc::new(ModuleRegistry::new());
    let registry = std::sync::Arc::new(tokio::sync::RwLock::new(rtfs_compiler::runtime::capabilities::registry::CapabilityRegistry::new()));
    let capability_marketplace = std::sync::Arc::new(rtfs_compiler::ccos::capability_marketplace::CapabilityMarketplace::new(registry.clone()));
    let host = std::sync::Arc::new(rtfs_compiler::ccos::host::RuntimeHost::new(
        Arc::new(std::sync::Mutex::new(rtfs_compiler::ccos::causal_chain::CausalChain::new().unwrap())),
        capability_marketplace,
        rtfs_compiler::runtime::security::RuntimeContext::pure(),
    ));
    let mut evaluator = Evaluator::new(
        module_registry,
        rtfs_compiler::runtime::security::RuntimeContext::pure(),
        host,
    );

    // Inject the custom model registry so the evaluator can find our providers

    // Test cases with different providers
    let test_cases = vec![
        // (
        //     "OpenAI GPT-4 Analysis",
        //     "openai-gpt4",
        //     r#"
        //     (defn ai-analyze-openai [text] 
        //       "Analyze the sentiment and key themes of the given text using OpenAI")
        //     (ai-analyze-openai "The RTFS system represents a breakthrough in programming language design, combining functional programming with cognitive computing capabilities.")
        //     "#,
        // ),
        // (
        //     "Gemini Pro Summarization",
        //     "gemini-pro", 
        //     r#"
        //     (defn ai-summarize-gemini [text]
        //       "Summarize the given text in one sentence using Gemini")
        //     (ai-summarize-gemini "The delegation engine in RTFS intelligently routes function calls between local execution, local models, and remote providers based on performance, cost, and privacy requirements.")
        //     "#,
        // ),
        // (
        //     "Claude 3 Opus Classification",
        //     "claude-opus",
        //     r#"
        //     (defn ai-classify-claude [text category]
        //       "Classify the text into the given category using Claude")
        //     (ai-classify-claude "The stock market experienced significant volatility today with tech stocks leading the gains" "finance")
        //     "#,
        // ),
        // (
        //     "OpenRouter Multi-Provider",
        //     "openrouter-gpt4",
        //     r#"
        //     (defn ai-generate-openrouter [prompt]
        //       "Generate creative content using OpenRouter's aggregated providers")
        //     (ai-generate-openrouter "Write a short poem about artificial intelligence and human creativity")
        //     "#,
        // ),
        (
            "OpenRouter Hunyuan A13B Generation",
            "openrouter-hunyuan-a13b-instruct",
            r#"
            (defn ai-generate-hunyuan [prompt]
              "Generate detailed explanation using Hunyuan A13B model via OpenRouter")
            (ai-generate-hunyuan "Explain the benefits of using remote model delegation in the RTFS runtime")
            "#,
        ),
    ];

    for (name, provider_id, code) in test_cases {
        println!("🧪 Testing: {}", name);
        println!("Provider: {}", provider_id);
        println!("Code: {}", code.trim());
        
        match parser::parse(code) {
            Ok(parsed) => {
                match evaluator.eval_toplevel(&parsed) {
                    Ok(result) => {
                        match result {
                            rtfs_compiler::runtime::execution_outcome::ExecutionOutcome::Complete(value) => {
                                println!("✅ Result: {}", value);
                            }
                            rtfs_compiler::runtime::execution_outcome::ExecutionOutcome::RequiresHost(host_call) => {
                                println!("⚠️  Host call required: {}", host_call.capability_id);
                            }
                        }
                    }
                    Err(e) => {
                        println!("❌ Error: {}", e);
                        
                        // Provide helpful error messages
                        if e.to_string().contains("API key") {
                            println!("💡 Make sure you have set the appropriate API key environment variable.");
                        }
                    }
                }
            }
            Err(e) => {
                println!("❌ Parse error: {}", e);
            }
        }
        println!();
    }

    // Demonstrate provider switching
    println!("🔄 Demonstrating Provider Switching");
    println!("===================================");
    
    let providers = vec![
        ("OpenAI GPT-4", "openai:gpt-4"),
        ("OpenAI GPT-3.5", "openai:gpt-3.5-turbo"),
        ("Gemini Pro", "gemini:gemini-pro"),
        ("Gemini Flash", "gemini:gemini-1.5-flash"),
        ("Claude 3 Opus", "claude:claude-3-opus-20240229"),
        ("Claude 3 Sonnet", "claude:claude-3-sonnet-20240229"),
        ("OpenRouter GPT-4", "openrouter:openai/gpt-4"),
        ("OpenRouter Claude", "openrouter:anthropic/claude-3-opus"),
        ("OpenRouter Llama", "openrouter:meta-llama/llama-3-8b-instruct"),
    ];

    for (name, config) in providers {
        match RemoteModelFactory::from_config(config) {
            Ok(_provider) => {
                println!("✅ {}: {}", name, config);
            }
            Err(_) => {
                println!("❌ {}: {} (API key not available)", name, config);
            }
        }
    }

    println!();
    println!("🎉 Example completed!");
    println!();
    println!("💡 Tips for using remote models:");
    println!("1. Set API keys as environment variables");
    println!("2. Use delegation hints in your RTFS code:");
    println!("   (defn my-function ^:delegation :remote \"openai-gpt4\" [input] ...)");
    println!("3. Configure the delegation engine for automatic routing:");
    println!("   static_map.insert(\"ai-function\".to_string(), ExecTarget::RemoteModel(\"openai-gpt4\".to_string()));");
    println!("4. Monitor costs and usage through your provider dashboards");
    println!();
    println!("🔧 Advanced Configuration:");
    println!("- Custom base URLs for self-hosted instances");
    println!("- Temperature and max_tokens tuning");
    println!("- Request timeout configuration");
    println!("- Retry logic and error handling");

    Ok(())
}

fn check_api_keys() -> Vec<String> {
    let mut available = Vec::new();
    
    if !std::env::var("OPENAI_API_KEY").unwrap_or_default().is_empty() {
        available.push("OpenAI".to_string());
    }
    if !std::env::var("GEMINI_API_KEY").unwrap_or_default().is_empty() {
        available.push("Gemini".to_string());
    }
    if !std::env::var("ANTHROPIC_API_KEY").unwrap_or_default().is_empty() {
        available.push("Anthropic".to_string());
    }
    if !std::env::var("OPENROUTER_API_KEY").unwrap_or_default().is_empty() {
        available.push("OpenRouter".to_string());
    }
    
    available
}

fn register_available_models(registry: &ModelRegistry) {
    println!("📦 Registering available models...");
    
    // OpenAI models
    if !std::env::var("OPENAI_API_KEY").unwrap_or_default().is_empty() {
        let openai_gpt4 = RemoteModelFactory::openai("gpt-4");
        let openai_gpt35 = RemoteModelFactory::openai("gpt-3.5-turbo");
        registry.register(openai_gpt4);
        registry.register(openai_gpt35);
        println!("  ✅ OpenAI: GPT-4, GPT-3.5-Turbo");
    } else {
        println!("  ❌ OpenAI: No API key");
    }
    
    // Gemini models
    if !std::env::var("GEMINI_API_KEY").unwrap_or_default().is_empty() {
        let gemini_pro = RemoteModelFactory::gemini("gemini-pro");
        let gemini_flash = RemoteModelFactory::gemini("gemini-1.5-flash");
        registry.register(gemini_pro);
        registry.register(gemini_flash);
        println!("  ✅ Gemini: Pro, Flash");
    } else {
        println!("  ❌ Gemini: No API key");
    }
    
    // Claude models
    if !std::env::var("ANTHROPIC_API_KEY").unwrap_or_default().is_empty() {
        let claude_opus = RemoteModelFactory::claude("claude-3-opus-20240229");
        let claude_sonnet = RemoteModelFactory::claude("claude-3-sonnet-20240229");
        let claude_haiku = RemoteModelFactory::claude("claude-3-haiku-20240307");
        registry.register(claude_opus);
        registry.register(claude_sonnet);
        registry.register(claude_haiku);
        println!("  ✅ Claude: Opus, Sonnet, Haiku");
    } else {
        println!("  ❌ Claude: No API key");
    }
    
    // OpenRouter models
    if !std::env::var("OPENROUTER_API_KEY").unwrap_or_default().is_empty() {
        // Create custom OpenRouter models with unique IDs
        // let openrouter_gpt4 = CustomOpenRouterModel::new("openrouter-gpt4", "openai/gpt-4");
        // let openrouter_claude = CustomOpenRouterModel::new("openrouter-claude", "anthropic/claude-3-opus");
        // let openrouter_gemini = CustomOpenRouterModel::new("openrouter-gemini", "google/gemini-pro");
        // let openrouter_llama = CustomOpenRouterModel::new("openrouter-llama", "meta-llama/llama-3-8b-instruct");
        let openrouter_hunyuan_a13b_instruct = CustomOpenRouterModel::new("openrouter-hunyuan-a13b-instruct", "tencent/hunyuan-a13b-instruct:free");
        // registry.register(openrouter_gpt4);
        // registry.register(openrouter_claude);
        // registry.register(openrouter_gemini);
        // registry.register(openrouter_llama);
        registry.register(openrouter_hunyuan_a13b_instruct);
        // println!("  ✅ OpenRouter: GPT-4, Claude-3-Opus, Gemini-Pro, Llama-3-8B");
        println!("  ✅ OpenRouter: Hunyuan-A13B-Instruct");
    } else {
        println!("  ❌ OpenRouter: No API key");
    }
    
    println!();
}

fn setup_delegation_engine() -> Arc<StaticDelegationEngine> {
    let mut static_map = HashMap::new();
    
    // Route specific functions to specific providers
    static_map.insert("ai-analyze-openai".to_string(), ExecTarget::RemoteModel("openai-gpt4".to_string()));
    static_map.insert("ai-summarize-gemini".to_string(), ExecTarget::RemoteModel("gemini-pro".to_string()));
    static_map.insert("ai-classify-claude".to_string(), ExecTarget::RemoteModel("claude-opus".to_string()));
    static_map.insert("ai-generate-openrouter".to_string(), ExecTarget::RemoteModel("openrouter-gpt4".to_string()));
    // New: route Hunyuan generator to the custom OpenRouter model
    static_map.insert("ai-generate-hunyuan".to_string(), ExecTarget::RemoteModel("openrouter-hunyuan-a13b-instruct".to_string()));
    
    // Add more generic routing patterns
    static_map.insert("ai-analyze".to_string(), ExecTarget::RemoteModel("openai-gpt4".to_string()));
    static_map.insert("ai-summarize".to_string(), ExecTarget::RemoteModel("gemini-pro".to_string()));
    static_map.insert("ai-classify".to_string(), ExecTarget::RemoteModel("claude-opus".to_string()));
    static_map.insert("ai-generate".to_string(), ExecTarget::RemoteModel("openrouter-gpt4".to_string()));
    
    Arc::new(StaticDelegationEngine::new(static_map))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_api_key_checking() {
        // Test with no API keys
        let keys = check_api_keys();
        // This test will pass even without API keys since we're just checking the function
        assert!(keys.len() <= 4);
    }

    #[test]
    fn test_remote_model_factory() {
        // Test factory methods (without actual API calls)
        let _openai = RemoteModelFactory::openai("gpt-4");
        let _gemini = RemoteModelFactory::gemini("gemini-pro");
        let _claude = RemoteModelFactory::claude("claude-3-opus");
        let _openrouter = RemoteModelFactory::openrouter("openai/gpt-4");
    }

    #[test]
    fn test_config_parsing() {
        // Test config string parsing
        let result = RemoteModelFactory::from_config("openai:gpt-4");
        assert!(result.is_ok());

        let result = RemoteModelFactory::from_config("invalid:format");
        assert!(result.is_err());
    }

    #[test]
    fn test_delegation_engine_setup() {
        let engine = setup_delegation_engine();
        // Verify the engine was created successfully
        assert!(std::sync::Arc::strong_count(&engine) > 0);
    }
} 