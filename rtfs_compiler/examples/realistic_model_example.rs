//! Example: Using Realistic Local Model with RTFS
//! 
//! This example demonstrates how to use a real local LLM (like Phi-2) 
//! with the RTFS delegation engine.

use rtfs_compiler::ccos::delegation::{ExecTarget, ModelRegistry, StaticDelegationEngine};
use rtfs_compiler::ccos::local_models::LocalLlamaModel;
use rtfs_compiler::parser;
use rtfs_compiler::runtime::{Evaluator, ModuleRegistry};
use std::collections::HashMap;
use std::sync::Arc;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("🤖 RTFS Realistic Local Model Example");
    println!("=====================================");
    println!();

    // Check if model path is set
    let model_path = std::env::var("RTFS_LOCAL_MODEL_PATH")
        .unwrap_or_else(|_| "models/phi-2.Q4_K_M.gguf".to_string());
    
    if !std::path::Path::new(&model_path).exists() {
        println!("❌ Model not found at: {}", model_path);
        println!();
        println!("To download a model, run:");
        println!("  ./scripts/download_model.sh");
        println!();
        println!("Or set the RTFS_LOCAL_MODEL_PATH environment variable:");
        println!("  export RTFS_LOCAL_MODEL_PATH=/path/to/your/model.gguf");
        println!();
        println!("Recommended models:");
        println!("  - Microsoft Phi-2 (efficient, ~1.5GB)");
        println!("  - Llama-2-7B-Chat (good balance, ~4GB)");
        println!("  - Mistral-7B-Instruct (excellent, ~4GB)");
        return Ok(());
    }

    println!("✅ Using model: {}", model_path);
    println!();

    // Create model registry with realistic model
    let registry = ModelRegistry::new();
    let realistic_model = LocalLlamaModel::new("realistic-llama", &model_path, None);
    registry.register(realistic_model);
    
    // Set up delegation engine to use realistic model for specific functions
    let mut static_map = HashMap::new();
    static_map.insert("ai-analyze".to_string(), ExecTarget::LocalModel("realistic-llama".to_string()));
    static_map.insert("ai-summarize".to_string(), ExecTarget::LocalModel("realistic-llama".to_string()));
    static_map.insert("ai-classify".to_string(), ExecTarget::LocalModel("realistic-llama".to_string()));
    
    let de = Arc::new(StaticDelegationEngine::new(static_map));
    
    // Create evaluator
    let module_registry = Arc::new(ModuleRegistry::new());
    let registry = std::sync::Arc::new(tokio::sync::RwLock::new(rtfs_compiler::runtime::capabilities::registry::CapabilityRegistry::new()));
    let capability_marketplace = std::sync::Arc::new(rtfs_compiler::runtime::capability_marketplace::CapabilityMarketplace::new(registry.clone()));
    let host = std::sync::Arc::new(rtfs_compiler::runtime::host::RuntimeHost::new(
        Arc::new(std::sync::Mutex::new(rtfs_compiler::ccos::causal_chain::CausalChain::new().unwrap())),
        capability_marketplace,
        rtfs_compiler::runtime::security::RuntimeContext::pure(),
    ));

    let mut evaluator = Evaluator::new(
    module_registry,
        de,
        rtfs_compiler::runtime::security::RuntimeContext::pure(),
        host,
    );
    
    // Test cases
    let test_cases = vec![
        (
            "AI Analysis",
            r#"
            (defn ai-analyze [text] 
              "Analyze the sentiment of the given text")
            (ai-analyze "I love this new RTFS system!")
            "#,
        ),
        (
            "AI Summarization", 
            r#"
            (defn ai-summarize [text]
              "Summarize the given text in one sentence")
            (ai-summarize "The RTFS system is a powerful programming language that combines functional programming with cognitive computing capabilities. It features delegation engines, model providers, and advanced type systems.")
            "#,
        ),
        (
            "AI Classification",
            r#"
            (defn ai-classify [text category]
              "Classify the text into the given category")
            (ai-classify "The stock market is performing well today" "finance")
            "#,
        ),
    ];

    for (name, code) in test_cases {
        println!("🧪 Testing: {}", name);
        println!("Code: {}", code.trim());
        
        match parser::parse(code) {
            Ok(parsed) => {
                match evaluator.eval_toplevel(&parsed) {
                    Ok(result) => {
                        println!("✅ Result: {:?}", result);
                    }
                    Err(e) => {
                        println!("❌ Error: {}", e);
                    }
                }
            }
            Err(e) => {
                println!("❌ Parse error: {}", e);
            }
        }
        println!();
    }

    println!("🎉 Example completed!");
    println!();
    println!("To try your own functions:");
    println!("1. Define a function with delegation hint:");
    println!("   (defn my-ai-function ^:delegation :local-model \"realistic-llama\" [input] ...)");
    println!();
    println!("2. Or configure the delegation engine to route specific functions:");
    println!("   static_map.insert(\"my-function\".to_string(), ExecTarget::LocalModel(\"realistic-llama\".to_string()));");
    println!();
    println!("3. Call your function and watch it execute through the local LLM!");

    Ok(())
} 