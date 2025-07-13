//! Plan Generation Demo using OpenRouter Hunyuan A13B
//!
//! This example asks a remote LLM (Hunyuan A13B served through OpenRouter) to
//! translate a validated RTFS `intent` into an RTFS `plan`.
//! The goal is to test whether a general-purpose model can generate a sequence
//! of executable steps based on a declarative goal.
//!
//! Usage:
//! `cargo run --example plan_generation_demo -- ./output/intent_20250711_153000_analyze_the_sentiment.rtfs`

use rtfs_compiler::ccos::delegation::{ExecTarget, ModelRegistry, StaticDelegationEngine, ModelProvider};
use rtfs_compiler::parser;
use rtfs_compiler::runtime::{Evaluator, ModuleRegistry};
use rtfs_compiler::ast::TopLevel;
use rtfs_compiler::runtime::values::Value;
use std::collections::HashMap;
use std::rc::Rc;
use std::sync::Arc;
use std::fs;

mod shared;
use shared::CustomOpenRouterModel;

/// Extracts the first top-level `(plan …)` s-expression from the given text.
fn extract_plan(text: &str) -> Option<String> {
    let start = text.find("(plan")?;
    let mut depth = 0usize;
    for (idx, ch) in text[start..].char_indices() {
        match ch {
            '(' => depth += 1,
            ')' => {
                depth = depth.saturating_sub(1);
                if depth == 0 {
                    let end = start + idx + 1;
                    return Some(text[start..end].to_string());
                }
            }
            _ => {}
        }
    }
    None
}

/// Write the validated RTFS plan to an output file
fn write_plan_to_file(plan_rtfs: &str, source_intent_filename: &str) -> Result<(), Box<dyn std::error::Error>> {
    let output_dir = std::path::Path::new("output");
    if !output_dir.exists() {
        std::fs::create_dir(output_dir)?;
    }
    
    let timestamp = chrono::Utc::now().format("%Y%m%d_%H%M%S");
    let filename = format!("plan_{}_{}.rtfs", timestamp, source_intent_filename);
    let filepath = output_dir.join(filename);
    
    std::fs::write(&filepath, plan_rtfs)?;
    println!("💾 Saved validated RTFS plan to: {}", filepath.display());
    Ok(())
}

/// Attempt to repair a malformed plan
fn attempt_plan_repair(
    malformed_rtfs: &str,
    source_intent_rtfs: &str,
    provider: &dyn ModelProvider,
) -> Result<(), Box<dyn std::error::Error>> {
    println!("\n🔧 Attempting Plan Repair");
    println!("==========================");

    let repair_prompt = format!(
        "The following RTFS plan is malformed and cannot be parsed. Please fix the syntax and structure based on the original intent.\n\nORIGINAL INTENT:\n{}\n\nMALFORMED PLAN:\n{}\n\nPlease provide a corrected, well-formed RTFS plan:",
        source_intent_rtfs,
        malformed_rtfs,
    );

    println!("📤 Sending repair prompt to model...");
    match provider.infer(&repair_prompt) {
        Ok(repaired) => {
            if let Some(repaired_plan) = extract_plan(&repaired) {
                println!("\n🔧 Repaired RTFS plan:\n{}", repaired_plan.trim());
                // Optionally, try to parse again and save if successful
                if parser::parse(&repaired_plan).is_ok() {
                    println!("✅ Repaired plan parsed successfully.");
                    // Consider saving the repaired plan
                } else {
                    eprintln!("Repaired plan still has parsing issues.");
                }
            } else {
                println!("⚠️  Could not extract plan from repair response. Raw response:\n{}", repaired);
            }
        }
        Err(e) => eprintln!("❌ Failed to repair plan: {}", e),
    }

    Ok(())
}

/// Generate a new plan from scratch when extraction fails
fn generate_plan_from_scratch(
    source_intent_rtfs: &str,
    provider: &dyn ModelProvider,
) -> Result<(), Box<dyn std::error::Error>> {
    println!("\n🔄 Generating Plan from Scratch");
    println!("=================================");

    // Re-use the main prompt generation logic, but simplified
    let scratch_prompt = format!(
        "The LLM failed to generate a proper RTFS plan. Please generate a complete, well-formed RTFS plan for this user intent:\n\nINPUT INTENT:\n{}\n\nPlease provide a complete RTFS plan definition:",
        source_intent_rtfs
    );

    match provider.infer(&scratch_prompt) {
        Ok(new_plan) => {
            if let Some(plan_block) = extract_plan(&new_plan) {
                println!("\n🆕 Generated RTFS plan:\n{}", plan_block.trim());
                if parser::parse(&plan_block).is_ok() {
                    println!("✅ Newly generated plan parsed successfully.");
                    // Consider saving the new plan
                } else {
                    eprintln!("Newly generated plan has parsing issues.");
                }
            }
            else {
                println!("⚠️ Could not extract plan from the new response. Raw response:\n{}", new_plan);
            }
        }
        Err(e) => eprintln!("Failed to generate new plan: {}", e),
    }

    Ok(())
}


fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("🧪 RTFS Plan Generation Demo\n===============================\n");

    // Verify API key
    let api_key = std::env::var("OPENROUTER_API_KEY").unwrap_or_default();
    if api_key.is_empty() {
        println!("❌ OPENROUTER_API_KEY not set – the demo will only print the prompt.\n");
    }

    // --- Get input intent file ---
    let intent_file_path_str = std::env::args().nth(1).ok_or("Please provide the path to an RTFS intent file.")?;
    let intent_file_path = std::path::Path::new(&intent_file_path_str);
    let intent_rtfs = fs::read_to_string(intent_file_path)?;
    
    let source_filename = intent_file_path.file_stem().unwrap_or_default().to_str().unwrap_or_default();

    // ---------------------------------------------------------------------
    // Build prompt: grammar snippet + few-shot examples + input intent
    // ---------------------------------------------------------------------

    const PLAN_GRAMMAR_SNIPPET: &str = r#"// RTFS Plan Grammar for AI Generation
// =====================================
// A plan is a list: (plan <name-symbol> :property value ...)
// It contains the concrete steps to fulfill an intent.
//
// REQUIRED PROPERTIES:
// - :intent-id - String ID of the intent this plan fulfills.
// - :steps - A vector of executable RTFS expressions.
//
// OPTIONAL PROPERTIES:
// - :description - A string explaining the plan's strategy.
//
// AVAILABLE CAPABILITIES (Functions you can use in :steps):
// - (http/get url) -> map : Makes an HTTP GET request.
// - (json/parse text) -> any : Parses a JSON string.
// - (map/get map key) -> any : Gets a value from a map.
// - (string/format "template" arg1) -> string : Formats a string.
// - (console/log message) : Prints a message.
// - (sentiment/analyze text) -> string : Returns 'positive', 'negative', or 'neutral'.
// - (validate/email email-string) -> bool : Validates an email format.
"#;

    const FEW_SHOTS: &str = r#"### Example 1: Simple Greeting Plan
INPUT INTENT:
(intent greet-user
  :goal "Generate a personalized greeting for 'Bob'"
  :original-request "Greet Bob"
  :constraints { :name "Bob" }
  :intent-id "intent-greet-bob")

GENERATED RTFS PLAN:
(plan greet-bob-plan
  :description "A simple plan to log a greeting to the console for a fixed name."
  :intent-id "intent-greet-bob"
  :steps [
    (console/log (string/format "Hello, {}!" "Bob"))
  ])

### Example 2: Sentiment Analysis Plan
INPUT INTENT:
(intent analyze-comment-sentiment
  :goal "Analyze the sentiment of a user's comment"
  :original-request "What is the sentiment of 'I love this product!'?"
  :constraints { :comment "I love this product!" }
  :intent-id "intent-sentiment-1")

GENERATED RTFS PLAN:
(plan analyze-sentiment-plan
  :description "Uses the sentiment/analyze capability to determine the sentiment of a given text and logs the result."
  :intent-id "intent-sentiment-1"
  :steps [
    (let [sentiment (sentiment/analyze "I love this product!")]
      (console/log (string/format "The sentiment is: {}" sentiment)))
  ])

### Example 3: Data Fetch and Process Plan
INPUT INTENT:
(intent fetch-user-email
  :goal "Fetch user data for user ID 1 and extract their email"
  :original-request "Get the email for user 1"
  :constraints { :user-id 1 }
  :success-criteria (fn [result] (validate/email result))
  :intent-id "intent-fetch-email-1")

GENERATED RTFS PLAN:
(plan fetch-and-extract-email-plan
  :description "Fetches user data from a public API, parses the JSON response, and extracts the email field."
  :intent-id "intent-fetch-email-1"
  :steps [
    (let [response (http/get "https://jsonplaceholder.typicode.com/users/1")]
      (let [user-data (json/parse (:body response))]
        (let [email (map/get user-data "email")]
          (console/log (string/format "User email is: {}" email))
          email))) ; Return the email as the final result
  ])
"#;

    let full_prompt = format!(
        "You are an expert RTFS developer. Your task is to translate a validated RTFS `intent` into a concrete, executable RTFS `plan`.\n\n{}\n\n{}\n\n### TASK\nINPUT INTENT:\n{}\n\nGenerate a complete RTFS `plan` that fulfills this intent. The `:intent-id` in the plan must match the one from the input intent.\n\nGENERATED RTFS PLAN:",
        PLAN_GRAMMAR_SNIPPET, FEW_SHOTS, intent_rtfs
    );

    println!("📜 Prompt sent to Hunyuan:\n{}\n---", full_prompt);

    if api_key.is_empty() {
        println!("(Set OPENROUTER_API_KEY to execute the call.)");
        return Ok(());
    }

    // --- Set up evaluator and model provider ---
    let registry = ModelRegistry::new();
    let hunyuan = CustomOpenRouterModel::new(
        "openrouter-hunyuan-a13b-instruct",
        "tencent/hunyuan-a13b-instruct:free",
    );
    registry.register(hunyuan);
    let delegation = Arc::new(StaticDelegationEngine::new(HashMap::new()));
    let mut evaluator = Evaluator::new(Rc::new(ModuleRegistry::new()), delegation);
    evaluator.model_registry = Arc::new(registry);

    let provider = evaluator
        .model_registry
        .get("openrouter-hunyuan-a13b-instruct")
        .expect("provider registered");

    // --- Call LLM and process response ---
    match provider.infer(&full_prompt) {
        Ok(r) => {
            match extract_plan(&r) {
                Some(plan_block) => {
                    println!("\n🎯 Extracted RTFS plan:\n{}", plan_block.trim());

                    match parser::parse(&plan_block) {
                        Ok(ast) => {
                             println!("\n✅ Plan parsed successfully.");
                             // For this demo, we just check for valid syntax.
                             // A real system would build a Plan struct.
                             println!("\nAST: {:#?}", ast);
                             write_plan_to_file(&plan_block, source_filename)?;
                        },
                        Err(e) => {
                            eprintln!("\n❌ Failed to parse extracted plan: {:?}", e);
                            attempt_plan_repair(&plan_block, &intent_rtfs, provider.as_ref())?;
                        }
                    }
                }
                None => {
                    println!("\n⚠️  Could not locate a complete (plan …) block. Raw response:\n{}", r.trim());
                    generate_plan_from_scratch(&intent_rtfs, provider.as_ref())?;
                }
            }
        },
        Err(e) => eprintln!("Error contacting model: {}", e),
    }

    Ok(())
}
