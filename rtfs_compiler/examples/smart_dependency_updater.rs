//! Example: Smart Dependency Updater (Intent-Driven)
//!
//! This example demonstrates a more realistic, multi-stage CCOS workflow.
//! Instead of using pre-written RTFS scripts, it shows how CCOS would
//! generate RTFS plans dynamically from high-level `Intents`.
//!
//! Scenario:
//! 1. A user provides a natural language prompt: "Check my project for outdated dependencies."
//! 2. A (simulated) NLU module converts this into a structured `Intent`.
//! 3. A (simulated) `Planner` component receives the `Intent` and **generates an RTFS plan** to analyze the file.
//! 4. The plan is executed, delegating the core analysis to an LLM.
//! 5. The LLM's structured output is used to create a **new, secondary Intent** to apply the updates.
//! 6. The `Planner` is invoked again on this new `Intent`, generating a second RTFS plan with conditional logic.
//! 7. The final plan is executed, demonstrating the full, dynamic, intent-driven loop.

use rtfs_compiler::ccos::delegation::{ExecTarget, ModelRegistry, StaticDelegationEngine};
use rtfs_compiler::ccos::local_models::LocalLlamaModel;
use rtfs_compiler::parser;
use rtfs_compiler::runtime::{Evaluator, ModuleRegistry, Value};
use std::collections::HashMap;
use std::rc::Rc;
use std::sync::Arc;

// --- CCOS Simulation Components ---

/// Represents a high-level goal. In a real CCOS, this would be the output
/// of a Natural Language Understanding (NLU) module.
#[derive(Debug)]
struct Intent<'a> {
    name: &'a str,
    parameters: HashMap<&'a str, Value>,
}

/// Represents the component that translates an `Intent` into an executable RTFS `Plan`.
/// In a real CCOS, this would be a sophisticated AI-driven module.
struct Planner;

impl Planner {
    /// Generates an RTFS code string (a Plan) from an Intent.
    fn generate_plan(&self, intent: &Intent) -> Result<String, String> {
        match intent.name {
            "analyze_dependencies" => {
                let file_path = match intent.parameters.get("file_path") {
                    Some(Value::String(s)) => s,
                    _ => return Err("Missing or invalid 'file_path' parameter.".to_string()),
                };

                Ok(format!(r#"
(do
    (println "--- Phase 1: Analyzing Dependencies ---")
    (let cargo_content (read-file "{}"))
    (let analysis_result 
        (analyze-dependencies 
            (str-join 
                "Analyze the following Cargo.toml file content. Identify outdated dependencies. For each, provide its name, current version, and the latest stable version. Determine if the update is 'major', 'minor', or 'patch'. Respond ONLY with a JSON list of objects with keys 'name', 'current', 'latest', 'type'. Here is the content: " 
                cargo_content)))
    analysis_result
)
"#, file_path))
            }
            "apply_dependency_updates" => {
                let updates_json = match intent.parameters.get("updates_json") {
                    Some(Value::String(s)) => s,
                    _ => return Err("Missing or invalid 'updates_json' parameter.".to_string()),
                };

                Ok(format!(r#"
(do
    (println "--- Phase 2: Planning & Executing Updates ---")
    (let updates (json-parse {}))
    (println (str-join "Found " (len updates) " potential updates."))
    (for-each update updates
        (do
            (println (str-join "  -> Checking: " (get update "name") " (" (get update "current") " -> " (get update "latest") ")"))
            (if (== (get update "type") "major")
                (do
                    (println "    -> MAJOR update detected. Creating branch and running tests...")
                    (let branch_name (str-join "feature/update-" (get update "name")))
                    (println (str-join "      (mock) git checkout -b " branch_name))
                    (println "      (mock) cargo test")
                    (println "    -> Task complete for major update.")
                )
                (do
                    (println "    -> Minor/patch update. Manual review suggested.")
                )
            )
        )
    )
)
"#, updates_json))
            }
            _ => Err(format!("Unknown intent: {}", intent.name)),
        }
    }
}


fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("🤖 CCOS Smart Dependency Updater Demo (Intent-Driven)");
    println!("====================================================");
    println!();

    // --- 1. Boilerplate: Model and Delegation Engine Setup ---
    let model_path = std::env::var("RTFS_LOCAL_MODEL_PATH")
        .unwrap_or_else(|_| "models/phi-2.Q4_K_M.gguf".to_string());
    
    if !std::path::Path::new(&model_path).exists() {
        println!("❌ Model not found. Please run ./scripts/download_model.sh or set RTFS_LOCAL_MODEL_PATH.");
        return Ok(());
    }
    println!("✅ Using model: {}", model_path);

    let registry = ModelRegistry::new();
    let realistic_model = LocalLlamaModel::new("local-analyzer", &model_path, None);
    registry.register(realistic_model);
    
    let mut static_map = HashMap::new();
    static_map.insert("analyze-dependencies".to_string(), ExecTarget::LocalModel("local-analyzer".to_string()));
    let de = Arc::new(StaticDelegationEngine::new(static_map));
    
    let module_registry = Rc::new(ModuleRegistry::new());
    let mut evaluator = Evaluator::new(module_registry, de);
    let planner = Planner;

    // --- 2. Create a dummy Cargo.toml for the demo ---
    let dummy_cargo_toml = r#"
[package]
name = "my-cool-project"
version = "0.1.0"

[dependencies]
serde = "1.0.1"
tokio = "0.2.5" # This is very old!
rand = "0.8.0"
"#;
    std::fs::write("dummy_Cargo.toml", dummy_cargo_toml)?;
    println!("✅ Created dummy_Cargo.toml for analysis.");
    println!();

    // --- 3. The User's Initial Request & Intent Creation ---
    let user_prompt = "Please analyze the dependencies in my Cargo.toml file.";
    println!("🗣️ User Prompt: \"{}\"", user_prompt);

    // (Simulated NLU step)
    let mut initial_params = HashMap::new();
    initial_params.insert("file_path", Value::String("dummy_Cargo.toml".to_string()));
    let initial_intent = Intent {
        name: "analyze_dependencies",
        parameters: initial_params,
    };
    println!("🧠 Generated Intent: {:?}
", initial_intent);

    // --- 4. Generate and Execute the First Plan ---
    let initial_plan_rtfs = planner.generate_plan(&initial_intent)?;
    println!("📝 Generated Plan (Phase 1):
{}
", initial_plan_rtfs.trim());

    let parsed_plan1 = parser::parse(&initial_plan_rtfs)?;
    let analysis_result = evaluator.eval_toplevel(&parsed_plan1)?;

    let llm_output = match analysis_result {
        Value::String(s) => s,
        _ => return Err("LLM did not return a string.".into()),
    };

    println!("✅ LLM Analysis Complete. Raw Output:
{}
", llm_output);

    // --- 5. The Recursive Intent & Plan ---
    println!("🔄 CCOS is now forming a new Intent from the LLM's output...");
    
    // (Simulated Intent creation from structured data)
    let mut update_params = HashMap::new();
    update_params.insert("updates_json", Value::String(llm_output.clone()));
    let update_intent = Intent {
        name: "apply_dependency_updates",
        parameters: update_params,
    };
    println!("🧠 Generated Intent: {:?}
", update_intent);

    // --- 6. Generate and Execute the Second Plan ---
    let update_plan_rtfs = planner.generate_plan(&update_intent)?;
    println!("📝 Generated Plan (Phase 2):
{}
", update_plan_rtfs.trim());

    let parsed_plan2 = parser::parse(&update_plan_rtfs)?;
    evaluator.eval_toplevel(&parsed_plan2)?;

    // --- 7. Cleanup ---
    std::fs::remove_file("dummy_Cargo.toml")?;
    println!("
✅ Demo complete. Cleaned up dummy file.");

    Ok(())
}
