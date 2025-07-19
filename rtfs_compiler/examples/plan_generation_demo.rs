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
use rtfs_compiler::runtime::stdlib::StandardLibrary;
use rtfs_compiler::ast::TopLevel;
use rtfs_compiler::runtime::values::Value;
use rtfs_compiler::runtime::security::{RuntimeContext, SecurityPolicies};
use rtfs_compiler::runtime::host_interface::HostInterface;
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

/// Test the capability system with different security contexts
fn test_capability_system() -> Result<(), Box<dyn std::error::Error>> {
    println!("\n🧪 Testing Capability System");
    println!("=============================");
    
    // Create evaluator with different security contexts
    let delegation = Arc::new(StaticDelegationEngine::new(HashMap::new()));
    
    // Test 1: Pure security context (no capabilities allowed)
    println!("\n1️⃣ Testing Pure Security Context");
    let pure_context = RuntimeContext::pure();
    let stdlib_env = StandardLibrary::create_global_environment();
    let registry = std::sync::Arc::new(tokio::sync::RwLock::new(rtfs_compiler::runtime::capability_registry::CapabilityRegistry::new()));
    let capability_marketplace = std::sync::Arc::new(rtfs_compiler::runtime::capability_marketplace::CapabilityMarketplace::new(registry.clone()));
    let host = Rc::new(rtfs_compiler::runtime::host::RuntimeHost::new(
        capability_marketplace,
        Rc::new(std::cell::RefCell::new(rtfs_compiler::ccos::causal_chain::CausalChain::new().unwrap())),
        pure_context.clone(),
    ));
    let mut evaluator = Evaluator::with_environment(
        Rc::new(ModuleRegistry::new()),
        stdlib_env,
        delegation.clone(),
        pure_context,
        host.clone(),
    );
    
    // Try to call a capability - should fail
    let pure_expr = match &parser::parse("(call \"ccos.echo\" \"Hello World\")")?[0] {
        TopLevel::Expression(expr) => expr.clone(),
        _ => return Err("Expected an expression".into()),
    };
    
    // Set execution context for the host
    HostInterface::set_execution_context(&*host, "test-plan".to_string(), vec!["test-intent".to_string()]);
    
    let pure_result = evaluator.eval_expr(
        &pure_expr,
        &mut evaluator.env.clone(),
    );
    
    match pure_result {
        Ok(_) => println!("❌ Pure context incorrectly allowed capability call"),
        Err(e) => println!("✅ Pure context correctly blocked capability: {}", e),
    }
    
    // Test 2: Controlled security context (specific capabilities allowed)
    println!("\n2️⃣ Testing Controlled Security Context");
    let controlled_context = SecurityPolicies::data_processing();
    let stdlib_env = StandardLibrary::create_global_environment();
    let registry = std::sync::Arc::new(tokio::sync::RwLock::new(rtfs_compiler::runtime::capability_registry::CapabilityRegistry::new()));
    let capability_marketplace = std::sync::Arc::new(rtfs_compiler::runtime::capability_marketplace::CapabilityMarketplace::new(registry.clone()));
    let host = Rc::new(rtfs_compiler::runtime::host::RuntimeHost::new(
        capability_marketplace,
        Rc::new(std::cell::RefCell::new(rtfs_compiler::ccos::causal_chain::CausalChain::new().unwrap())),
        controlled_context.clone(),
    ));

    let mut evaluator = Evaluator::with_environment(
        Rc::new(ModuleRegistry::new()),
        stdlib_env,
        delegation.clone(),
        controlled_context,
        host.clone(),
    );
    
    // Try to call allowed capability
    let controlled_expr = match &parser::parse("(call \"ccos.echo\" \"Hello World\")")?[0] {
        TopLevel::Expression(expr) => expr.clone(),
        _ => return Err("Expected an expression".into()),
    };
    
    // Set execution context for the host
    HostInterface::set_execution_context(&*host, "test-plan".to_string(), vec!["test-intent".to_string()]);
    
    let controlled_result = evaluator.eval_expr(
        &controlled_expr,
        &mut evaluator.env.clone(),
    );
    
    match controlled_result {
        Ok(result) => println!("✅ Controlled context allowed capability call: {:?}", result),
        Err(e) => println!("❌ Controlled context incorrectly blocked capability: {}", e),
    }
    
    // Test 3: Full security context (all capabilities allowed)
    println!("\n3️⃣ Testing Full Security Context");
    let full_context = RuntimeContext::full();
    let stdlib_env = StandardLibrary::create_global_environment();
    let registry = std::sync::Arc::new(tokio::sync::RwLock::new(rtfs_compiler::runtime::capability_registry::CapabilityRegistry::new()));
    let capability_marketplace = std::sync::Arc::new(rtfs_compiler::runtime::capability_marketplace::CapabilityMarketplace::new(registry.clone()));
    let host = Rc::new(rtfs_compiler::runtime::host::RuntimeHost::new(
        capability_marketplace,
        Rc::new(std::cell::RefCell::new(rtfs_compiler::ccos::causal_chain::CausalChain::new().unwrap())),
        full_context.clone(),
    ));

    let mut evaluator = Evaluator::with_environment(
        Rc::new(ModuleRegistry::new()),
        stdlib_env,
        delegation.clone(),
        full_context,
        host.clone(),
    );
    
    // Try to call various capabilities
    let capabilities_to_test = [
        "ccos.echo",
        "ccos.math.add",
        "ccos.ask-human",
    ];
    
    for capability in &capabilities_to_test {
        let test_expr = format!("(call \"{}\" \"test input\")", capability);
        let expr = match &parser::parse(&test_expr)?[0] {
            TopLevel::Expression(expr) => expr.clone(),
            _ => return Err("Expected an expression".into()),
        };
        
        // Set execution context for each test
        HostInterface::set_execution_context(&*host, "test-plan".to_string(), vec!["test-intent".to_string()]);
        
        let result = evaluator.eval_expr(
            &expr,
            &mut evaluator.env.clone(),
        );
        match result {
            Ok(value) => println!("✅ Full context allowed {}: {:?}", capability, value),
            Err(e) => println!("❌ Full context failed for {}: {}", capability, e),
        }
    }
    
    // Test 4: Math capability with structured input
    println!("\n4️⃣ Testing Math Capability");
    let math_context = RuntimeContext::full();
    let stdlib_env = StandardLibrary::create_global_environment();
    let registry = std::sync::Arc::new(tokio::sync::RwLock::new(rtfs_compiler::runtime::capability_registry::CapabilityRegistry::new()));
    let capability_marketplace = std::sync::Arc::new(rtfs_compiler::runtime::capability_marketplace::CapabilityMarketplace::new(registry.clone()));
    let host = Rc::new(rtfs_compiler::runtime::host::RuntimeHost::new(
        capability_marketplace,
        Rc::new(std::cell::RefCell::new(rtfs_compiler::ccos::causal_chain::CausalChain::new().unwrap())),
        math_context.clone(),
    ));

    let mut evaluator = Evaluator::with_environment(
        Rc::new(ModuleRegistry::new()),
        stdlib_env,
        delegation.clone(),
        math_context,
        host.clone(),
    );
    
    let math_expr = match &parser::parse("(call \"ccos.math.add\" {:a 10 :b 20})")?[0] {
        TopLevel::Expression(expr) => expr.clone(),
        _ => return Err("Expected an expression".into()),
    };
    
    // Set execution context for math test
    HostInterface::set_execution_context(&*host, "test-plan".to_string(), vec!["test-intent".to_string()]);
    
    let math_result = evaluator.eval_expr(
        &math_expr,
        &mut evaluator.env.clone(),
    );
    
    match math_result {
        Ok(value) => println!("✅ Math capability result: {:?}", value),
        Err(e) => println!("❌ Math capability failed: {}", e),
    }
    
    // Test 5: Plan with capability calls
    println!("\n5️⃣ Testing Plan with Capability Calls");
    let plan_rtfs = r#"
    (plan test-capability-plan
      :description "Test plan that uses various capabilities"
      :intent-id "test-intent"
      :steps [
        (call "ccos.echo" "Step 1: Echo test")
        (let [result (call "ccos.math.add" {:a 5 :b 3})]
          (call "ccos.echo" (str "Step 2: Math result is " result)))
        (call "ccos.echo" "Step 3: Plan completed")
      ])
    "#;
    
    let plan_ast = parser::parse(plan_rtfs)?;
    let plan_context = RuntimeContext::full();
    let stdlib_env = StandardLibrary::create_global_environment();
    let registry = std::sync::Arc::new(tokio::sync::RwLock::new(rtfs_compiler::runtime::capability_registry::CapabilityRegistry::new()));
    let capability_marketplace = std::sync::Arc::new(rtfs_compiler::runtime::capability_marketplace::CapabilityMarketplace::new(registry.clone()));
    let host = Rc::new(rtfs_compiler::runtime::host::RuntimeHost::new(
        capability_marketplace,
        Rc::new(std::cell::RefCell::new(rtfs_compiler::ccos::causal_chain::CausalChain::new().unwrap())),
        plan_context.clone(),
    ));    
    let mut evaluator = Evaluator::with_environment(
        Rc::new(ModuleRegistry::new()),
        stdlib_env,
        delegation.clone(),
        plan_context,
        host.clone(),
    );
    
    // Evaluate the plan
    // Set execution context for plan evaluation
    HostInterface::set_execution_context(&*host, "test-capability-plan".to_string(), vec!["test-intent".to_string()]);
    
    let plan_result = evaluator.eval_toplevel(&plan_ast);
    match plan_result {
        Ok(metadata) => println!("✅ Plan evaluated successfully. Metadata: {:?}", metadata),
        Err(e) => println!("❌ Plan evaluation failed: {}", e),
    }
    
    // 6. Validate causal chain contents
    println!("\n6️⃣ Validating Causal Chain Contents");
    let causal_chain = host.causal_chain.borrow();
    let action_count = causal_chain.get_action_count();
    println!("📊 Causal Chain has {} actions recorded", action_count);
    
    // Check for specific actions that should have been recorded
    for i in 0..action_count {
        if let Some(action) = causal_chain.get_action_by_index(i) {
            println!("  🔗 Action {}: {} ({})", i, action.function_name, action.action_id);
            if let Some(result) = &action.result {
                println!("    ✅ Result: {:?}", result);
            }
        }
    }
    
    if action_count > 0 {
        println!("✅ Causal chain validation completed - {} actions recorded", action_count);
    } else {
        println!("⚠️  No actions were recorded in the causal chain");
    }
    
    drop(causal_chain);
    
    Ok(())
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("🧪 RTFS Plan Generation Demo\n===============================\n");

    // Test capability system first
    test_capability_system()?;

    // Verify API key
    let api_key = std::env::var("OPENROUTER_API_KEY").unwrap_or_default();
    if api_key.is_empty() {
        println!("❌ OPENROUTER_API_KEY not set – the demo will only print the prompt.\n");
    }

    // --- Optionally run a generated plan file ---
    // Example usage: run_plan_with_full_security_context("output/plan_20250718_102122_test_intent.rtfs")?;
    // Uncomment and set the correct path to run your generated plan:
    // run_plan_with_full_security_context("output/plan_20250718_102122_test_intent.rtfs")?;

    // --- Get input intent file ---
    let intent_file_path_str = std::env::args().nth(1).unwrap_or_else(|| {
        // Create a default test intent if no file is provided
        let test_intent = r#"
(intent test-sentiment-analysis
  :goal "Analyze the sentiment of a given text"
  :original-request "Analyze the sentiment of 'I love this product!'"
  :constraints { :text "I love this product!" }
  :intent-id "intent-test-sentiment")
"#;
        
        // Write test intent to a temp file
        let temp_file = "/tmp/test_intent.rtfs";
        std::fs::write(temp_file, test_intent).expect("Failed to write test intent");
        println!("ℹ️  No intent file provided, using default test intent: {}", temp_file);
        temp_file.to_string()
    });
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
// - (call :capability-id inputs) -> any : Call a capability with inputs
// - (call :capability-id inputs options) -> any : Call a capability with inputs and options
//
// COMMON CAPABILITIES:
// - :ccos.echo - Echo input back (for testing)
// - :ccos.math.add - Add two numbers {:a number :b number}
// - :ccos.ask-human - Ask human for input (returns resource handle)
// - :ccos.io.log - Log a message
// - :ccos.data.parse-json - Parse JSON string
// - :ccos.network.http-fetch - Make HTTP request
//
// RTFS STANDARD LIBRARY FUNCTIONS:
// - (get map key) -> any : Gets a value from a map or vector
// - (get map key default) -> any : Gets a value with default fallback
// - (get-in data path) -> any : Gets nested value from path vector
// - (str arg1 arg2 ...) -> string : Concatenates arguments as string
// - (+ a b ...) -> number : Adds numbers
// - (count collection) -> integer : Counts elements in collection
// - (map fn collection) -> vector : Maps function over collection
// - (filter fn collection) -> vector : Filters collection with predicate
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
    (call :ccos.io.log (str "Hello, " "Bob" "!"))
  ])

### Example 2: Math Calculation Plan
INPUT INTENT:
(intent calculate-sum
  :goal "Calculate the sum of two numbers"
  :original-request "What is 15 + 27?"
  :constraints { :a 15 :b 27 }
  :intent-id "intent-calc-sum-1")

GENERATED RTFS PLAN:
(plan calculate-sum-plan
  :description "Uses the math capability to add two numbers and logs the result."
  :intent-id "intent-calc-sum-1"
  :steps [
    (let [result (call :ccos.math.add {:a 15 :b 27})]
      (call :ccos.io.log (str "The sum is: " result)))
  ])

### Example 3: Data Fetch and Process Plan
INPUT INTENT:
(intent fetch-user-email
  :goal "Fetch user data for user ID 1 and extract their email"
  :original-request "Get the email for user 1"
  :constraints { :user-id 1 }
  :intent-id "intent-fetch-email-1")

GENERATED RTFS PLAN:
(plan fetch-and-extract-email-plan
  :description "Fetches user data from a public API, parses the JSON response, and extracts the email field."
  :intent-id "intent-fetch-email-1"
  :steps [
    (let [response (call :ccos.network.http-fetch "https://jsonplaceholder.typicode.com/users/1")]
      (let [user-data (call :ccos.data.parse-json (get response :body))]
        (let [email (get user-data "email")]
          (call :ccos.io.log (str "User email is: " email))
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
    let model_registry = ModelRegistry::new();
    // let model_id = "openrouter-hunyuan-a13b-instruct"; // previous model id
    let model_id = "moonshotai/kimi-k2:free";
    // let model_name = "tencent/hunyuan-a13b-instruct:free"; // previous model name
    let model_name = "moonshotai/kimi-k2:free";
    let model = CustomOpenRouterModel::new(
        model_id,
        model_name,
    );
    model_registry.register(model);
    
    let delegation = Arc::new(StaticDelegationEngine::new(HashMap::new()));
    let stdlib_env = StandardLibrary::create_global_environment();
    let registry = std::sync::Arc::new(tokio::sync::RwLock::new(rtfs_compiler::runtime::capability_registry::CapabilityRegistry::new()));
    let capability_marketplace = std::sync::Arc::new(rtfs_compiler::runtime::capability_marketplace::CapabilityMarketplace::new(registry.clone()));
    let host = Rc::new(rtfs_compiler::runtime::host::RuntimeHost::new(
        capability_marketplace,
        Rc::new(std::cell::RefCell::new(rtfs_compiler::ccos::causal_chain::CausalChain::new().unwrap())),
        rtfs_compiler::runtime::security::RuntimeContext::full(),
    ));
    
    let mut evaluator = Evaluator::with_environment(
        Rc::new(ModuleRegistry::new()),
        stdlib_env,
        delegation,
        RuntimeContext::full(),
        host,
    );
    evaluator.model_registry = Arc::new(model_registry);

    let provider = evaluator
        .model_registry
        .get(model_id)
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
                             println!("\nAST: {:#?}", ast);

                             // --- Execute plan steps ---
                             // Find the top-level plan expression
                             let plan_expr = ast.iter().find_map(|top| {
                                 if let TopLevel::Expression(expr) = top {
                                     Some(expr)
                                 } else {
                                     None
                                 }
                             });
                             // Helper to extract :steps property from plan expression
                             fn extract_steps(expr: &rtfs_compiler::Expression) -> Option<&Vec<rtfs_compiler::Expression>> {
                                 use rtfs_compiler::Expression::*;
                                 if let FunctionCall { callee, arguments } = expr {
                                     // Look for :steps keyword in arguments
                                     let mut args_iter = arguments.iter();
                                     while let Some(arg) = args_iter.next() {
                                         if let Literal(rtfs_compiler::ast::Literal::Keyword(k)) = arg {
                                             if k.0 == "steps" || k.0 == ":steps" {
                                                 // Next argument should be the vector
                                                 if let Some(Vector(vec)) = args_iter.next() {
                                                     return Some(vec);
                                                 }
                                             }
                                         }
                                     }
                                 }
                                 None
                             }
                             if let Some(plan_expr) = plan_expr {
                                 if let Some(steps_vec) = extract_steps(plan_expr) {
                                     println!("\n🚀 Executing plan steps:");
                                     for (i, step_expr) in steps_vec.iter().enumerate() {
                                         HostInterface::set_execution_context(&*evaluator.host, "generated-plan".to_string(), vec!["generated-intent".to_string()]);
                                         let result = evaluator.eval_expr(step_expr, &mut evaluator.env.clone());
                                         match result {
                                             Ok(val) => println!("  ✅ Step {} result: {:?}", i+1, val),
                                             Err(e) => println!("  ❌ Step {} error: {}", i+1, e),
                                         }
                                     }
                                 } else {
                                     println!("⚠️  No :steps property found in plan, cannot execute steps.");
                                 }
                             } else {
                                 println!("⚠️  No top-level plan expression found, cannot execute plan.");
                             }

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
