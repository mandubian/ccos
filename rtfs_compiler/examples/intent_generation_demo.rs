//! Intent Generation Demo using OpenRouter Hunyuan A13B
//!
//! This example asks a remote LLM (Hunyuan A13B served through OpenRouter) to
//! translate a natural-language user request into an RTFS `intent` definition.
//! The goal is to test whether a general-purpose model can "speak RTFS" with a
//! few-shot prompt plus a snippet of the grammar – no fine-tuning.

use rtfs_compiler::ccos::delegation::{ExecTarget, ModelRegistry, StaticDelegationEngine, ModelProvider};
use rtfs_compiler::parser;
use rtfs_compiler::runtime::{Evaluator, ModuleRegistry};
use rtfs_compiler::ast::TopLevel;
use rtfs_compiler::runtime::values::Value;
use std::collections::HashMap;
use std::rc::Rc;
use std::sync::Arc;
use regex::Regex; // Add dependency in Cargo.toml if not present
use rtfs_compiler::ccos::types::Intent;

mod shared;
use shared::CustomOpenRouterModel;

// ------------------------- NEW: extractor helper -------------------------
/// Extracts the first top-level `(intent …)` s-expression from the given text.
/// Returns `None` if no well-formed intent block is found.
fn extract_intent(text: &str) -> Option<String> {
    // Locate the starting position of the "(intent" keyword
    let start = text.find("(intent")?;

    // Scan forward and track parenthesis depth to find the matching ')'
    let mut depth = 0usize;
    for (idx, ch) in text[start..].char_indices() {
        match ch {
            '(' => depth += 1,
            ')' => {
                depth = depth.saturating_sub(1);
                // When we return to depth 0 we've closed the original "(intent"
                if depth == 0 {
                    let end = start + idx + 1; // inclusive of current ')'
                    return Some(text[start..end].to_string());
                }
            }
            _ => {}
        }
    }
    None
}
// -------------------------------------------------------------------------

/// Replace #rx"pattern" literals with plain "pattern" string literals so the current
/// grammar (which lacks regex literals) can parse the intent.
fn sanitize_regex_literals(text: &str) -> String {
    // Matches #rx"..." with minimal escaping (no nested quotes inside pattern)
    let re = Regex::new(r#"#rx\"([^\"]*)\""#).unwrap();
    re.replace_all(text, |caps: &regex::Captures| {
        format!("\"{}\"", &caps[1])
    }).into_owned()
}

// Helper: convert parser Literal to runtime Value (basic subset)
fn lit_to_val(lit: &rtfs_compiler::ast::Literal) -> Value {
    use rtfs_compiler::ast::Literal as Lit;
    match lit {
        Lit::String(s) => Value::String(s.clone()),
        Lit::Integer(i) => Value::Integer(*i),
        Lit::Float(f) => Value::Float(*f),
        Lit::Boolean(b) => Value::Boolean(*b),
        _ => Value::Nil,
    }
}

fn expr_to_value(expr: &rtfs_compiler::ast::Expression) -> Value {
    use rtfs_compiler::ast::{Expression as E};
    match expr {
        E::Literal(lit) => lit_to_val(lit),
        E::Map(m) => {
            let mut map = std::collections::HashMap::new();
            for (k, v) in m {
                map.insert(k.clone(), expr_to_value(v));
            }
            Value::Map(map)
        }
        E::Vector(vec) | E::List(vec) => {
            let vals = vec.iter().map(expr_to_value).collect();
            if matches!(expr, E::Vector(_)) { Value::Vector(vals) } else { Value::List(vals) }
        }
        E::Symbol(s) => Value::Symbol(rtfs_compiler::ast::Symbol(s.0.clone())),
        E::FunctionCall { callee, arguments } => {
            // Convert function calls to a list representation for storage
            let mut func_list = vec![expr_to_value(callee)];
            func_list.extend(arguments.iter().map(expr_to_value));
            Value::List(func_list)
        }
        E::Fn(fn_expr) => {
            // Convert fn expressions to a list representation: (fn params body...)
            let mut fn_list = vec![Value::Symbol(rtfs_compiler::ast::Symbol("fn".to_string()))];
            
            // Add parameters as a vector
            let mut params = Vec::new();
            for param in &fn_expr.params {
                params.push(Value::Symbol(rtfs_compiler::ast::Symbol(format!("{:?}", param.pattern))));
            }
            fn_list.push(Value::Vector(params));
            
            // Add body expressions
            for body_expr in &fn_expr.body {
                fn_list.push(expr_to_value(body_expr));
            }
            
            Value::List(fn_list)
        }
        _ => Value::Nil,
    }
}

fn map_expr_to_string_value(expr: &rtfs_compiler::ast::Expression) -> Option<std::collections::HashMap<String, Value>> {
    use rtfs_compiler::ast::{Expression as E, MapKey};
    if let E::Map(m) = expr {
        let mut out = std::collections::HashMap::new();
        for (k, v) in m {
            let key_str = match k {
                MapKey::Keyword(k) => k.0.clone(),
                MapKey::String(s) => s.clone(),
                MapKey::Integer(i) => i.to_string(),
            };
            out.insert(key_str, expr_to_value(v));
        }
        Some(out)
    } else {
        None
    }
}

fn intent_from_function_call(expr: &rtfs_compiler::ast::Expression) -> Option<Intent> {
    use rtfs_compiler::ast::{Expression as E, Literal, Symbol};

    let E::FunctionCall { callee, arguments } = expr else { return None; };
    let E::Symbol(Symbol(sym)) = &**callee else { return None; };
    if sym != "intent" { return None; }
    if arguments.is_empty() { return None; }

    // The first argument is the intent name/type, as per the demo's grammar snippet.
    let name = if let E::Symbol(Symbol(name_sym)) = &arguments[0] {
        name_sym.clone()
    } else {
        return None; // First argument must be a symbol
    };

    let mut properties = HashMap::new();
    let mut args_iter = arguments[1..].chunks_exact(2);
    while let Some([key_expr, val_expr]) = args_iter.next() {
        if let E::Literal(Literal::Keyword(k)) = key_expr {
            properties.insert(k.0.clone(), val_expr);
        }
    }

    let original_request = properties.get("original-request")
        .and_then(|expr| if let E::Literal(Literal::String(s)) = expr { Some(s.clone()) } else { None })
        .unwrap_or_default();
    
    let goal = properties.get("goal")
        .and_then(|expr| if let E::Literal(Literal::String(s)) = expr { Some(s.clone()) } else { None })
        .unwrap_or_else(|| original_request.clone());

    let mut intent = Intent::with_name(name, original_request.clone(), goal);
    
    if let Some(expr) = properties.get("constraints") {
        if let Some(m) = map_expr_to_string_value(expr) {
            intent.constraints = m;
        }
    }

    if let Some(expr) = properties.get("preferences") {
        if let Some(m) = map_expr_to_string_value(expr) {
            intent.preferences = m;
        }
    }

    if let Some(expr) = properties.get("success-criteria") {
        println!("🔍 Debug - Found success-criteria expression: {:?}", expr);
        let value = expr_to_value(expr);
        println!("🔍 Debug - Converted to value: {:?}", value);
        intent.success_criteria = Some(value);
    } else {
        println!("🔍 Debug - No success-criteria property found in properties: {:?}", properties.keys().collect::<Vec<_>>());
    }
    
    Some(intent)
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("🧪 RTFS Intent Generation Demo\n===============================\n");

    // Verify API key
    let api_key = std::env::var("OPENROUTER_API_KEY").unwrap_or_default();
    if api_key.is_empty() {
        println!("❌ OPENROUTER_API_KEY not set – the demo will only print the prompt.\n");
    }

    // ---------------------------------------------------------------------
    // Build prompt: grammar snippet + few-shot examples + user request
    // ---------------------------------------------------------------------

    const INTENT_GRAMMAR_SNIPPET: &str = r#"// RTFS Intent Grammar for AI Generation
// ============================================
//
// An intent is represented as a list: (intent <name-symbol> :property1 value1 ...)
// This is parsed as a function call but treated as declarative data by CCOS.
//
// REQUIRED PROPERTIES:
// - :goal - String describing what should be accomplished
// - :original-request - The user's natural language request
//
// OPTIONAL PROPERTIES:
// - :constraints - Map of constraints (e.g., { :input-type :string })
// - :preferences - Map of preferences (e.g., { :tone :friendly })
// - :success-criteria - Function that validates the result
// - :status - String (default: "active")
//
// SUCCESS CRITERIA FUNCTIONS:
// The :success-criteria must be a function: (fn [result] <validation-logic>)
// Available functions for validation:
//
// Type Checking: (string? x), (int? x), (float? x), (bool? x), (map? x), (vector? x)
// Map Operations: (get map key [default]), (contains? map key), (empty? coll)
// Logic: (and ...), (or ...), (not ...)
// Comparison: (= a b), (> a b), (< a b), (>= a b), (<= a b)
// String: (str/includes? str substr), (str/starts-with? str prefix)
//
// GENERATION GUIDELINES:
// 1. Always include :goal and :original-request
// 2. Use descriptive constraint keys (e.g., :input-type, :output-format)
// 3. Write success criteria that are specific and testable
// 4. Use meaningful intent names (e.g., "analyze-sentiment", "validate-email")
// 5. If user mentions validation requirements, translate them to success-criteria
// 6. If user mentions constraints, map them to :constraints
"#;

    const FEW_SHOTS: &str = r#"### Example 1: Simple Intent
User request: "Greet a user by name"
RTFS:
(intent greet-user
  :goal         "Generate a personalized greeting using the user's name"
  :original-request "Greet a user by name"
  :constraints  { :name-type :string :name-required true }
  :preferences  { :tone :friendly :formality :casual }
  :success-criteria (fn [result] 
    (and (string? result)
         (str/includes? result "Hello")
         (not (empty? result))))
  :status       "active")

### Example 2: Data Processing with Validation
User request: "Add two integers and return the sum"
RTFS:
(intent add-integers
  :goal         "Perform arithmetic addition of two integer inputs"
  :original-request "Add two integers and return the sum"
  :constraints  { :x-type :int :y-type :int :overflow-check true }
  :success-criteria (fn [result] 
    (and (int? result)
         (>= result 0)))  ; Assuming non-negative result is expected
  :status       "active")

### Example 3: Complex Analysis with Multiple Validation Rules
User request: "Analyze sales data for Q2 and ensure the report is a map containing a 'summary' string and a 'total_revenue' float greater than 50000."
RTFS:
(intent analyze-q2-sales
  :goal "Analyze Q2 sales data and generate a comprehensive report"
  :original-request "Analyze sales data for Q2 and ensure the report is a map containing a 'summary' string and a 'total_revenue' float greater than 50000."
  :constraints { 
    :quarter "Q2" 
    :data-source :sales-database
    :report-format :map
  }
  :preferences { 
    :detail-level :comprehensive
    :include-charts false
  }
  :success-criteria (fn [result]
    (and (map? result)
         (contains? result :summary)
         (string? (get result :summary))
         (not (empty? (get result :summary)))
         (contains? result :total_revenue)
         (float? (get result :total_revenue))
         (> (get result :total_revenue) 50000.0)
         (contains? result :quarter)
         (= (get result :quarter) "Q2")))
  :status "active")

### Example 4: Error Handling and Edge Cases
User request: "Validate an email address and return true if valid, false otherwise"
RTFS:
(intent validate-email
  :goal "Check if the provided string is a valid email address format"
  :original-request "Validate an email address and return true if valid, false otherwise"
  :constraints { 
    :input-type :string 
    :allow-empty false
    :max-length 254
  }
  :success-criteria (fn [result]
    (and (bool? result)
         (or (= result true) (= result false))))
  :status "active")

### Example 5: Multi-Step Process with Intermediate Validation
User request: "Process a JSON file, extract user data, and return a list of users with age > 18"
RTFS:
(intent process-user-data
  :goal "Parse JSON file, extract user records, and filter by age criteria"
  :original-request "Process a JSON file, extract user data, and return a list of users with age > 18"
  :constraints { 
    :file-format :json
    :required-fields ["name" "age" "email"]
    :min-age 18
  }
  :preferences { 
    :sort-by :name
    :include-metadata true
  }
  :success-criteria (fn [result]
    (and (vector? result)
         (not (empty? result))
         ; Ensure all users in result have age > 18
         (every? (fn [user] 
                   (and (map? user)
                        (contains? user :age)
                        (> (get user :age) 18))) result)))
  :status "active")
"#;

    const ANTI_PATTERNS: &str = r#"### ANTI-PATTERN 1: Mismatched Parentheses (Common Error!)
User request: "Validate a user's age is over 21"
INCORRECT RTFS (missing a closing parenthesis at the end):
(intent validate-age
  :goal "Check if age is over 21"
  :original-request "Validate a user's age is over 21"
  :success-criteria (fn [result]
    (and (int? result)
         (> result 21))
; <-- Missing final ')'

CORRECTED RTFS:
(intent validate-age
  :goal "Check if age is over 21"
  :original-request "Validate a user's age is over 21"
  :success-criteria (fn [result]
    (and (int? result)
         (> result 21))))
"#;

    const GENERATION_STRATEGY: &str = r#"AI GENERATION STRATEGY:
================================

STEP 1: ANALYZE USER REQUEST
- Identify the core action/operation
- Extract implicit constraints and preferences
- Determine expected output format
- Note any validation requirements

STEP 2: DESIGN INTENT STRUCTURE
- Choose a descriptive intent name (verb-noun format)
- Write a clear, specific :goal
- Map user constraints to :constraints
- Infer preferences and add to :preferences

STEP 3: CREATE SUCCESS CRITERIA
- Translate user validation requirements to RTFS functions
- Ensure criteria are specific and testable
- Handle edge cases (empty results, type mismatches)
- Use appropriate type checking and comparison functions

STEP 4: SYNTAX VALIDATION
- CRITICAL: Double-check that all opening parentheses '(' are matched with a closing parenthesis ')'.
- Ensure the final output is a single, complete `(intent ...)` block.
- Verify keyword-argument pairs are correct (e.g., `:goal "..."`).

STEP 5: VALIDATE GENERATED INTENT
- Ensure all required properties are present
- Verify success criteria syntax is correct
- Check that constraints are reasonable
- Confirm intent name follows naming conventions

COMMON PATTERNS:
- Data validation: (and (map? result) (contains? result :key) (type? (get result :key)))
- List processing: (and (vector? result) (not (empty? result)) (every? predicate result))
- Type conversion: (and (string? result) (not (empty? result)))
- Range checking: (and (number? result) (>= result min) (<= result max))
"#;

    // ---------------------------------------------------------------------
    // Build runtime registry / evaluator with Hunyuan provider
    // ---------------------------------------------------------------------
    let registry = ModelRegistry::new();
    let hunyuan = CustomOpenRouterModel::new(
        "openrouter-hunyuan-a13b-instruct",
        "tencent/hunyuan-a13b-instruct:free",
    );
    registry.register(hunyuan);

    // Delegation engine: always use remote model for our generator function
    let mut static_map = HashMap::new();
    static_map.insert(
        "nl->intent".to_string(),
        ExecTarget::RemoteModel("openrouter-hunyuan-a13b-instruct".to_string()),
    );
    let delegation = Arc::new(StaticDelegationEngine::new(static_map));

    // Evaluator (we won't actually evaluate the generated intent here, but set up for future)
    let mut evaluator = Evaluator::new(Rc::new(ModuleRegistry::new()), delegation);
    evaluator.model_registry = Arc::new(registry);

    // ---------------------------------------------------------------------
    // Ask user for a request (or use default)
    // ---------------------------------------------------------------------
    let user_request = std::env::args().nth(1).unwrap_or_else(|| {
        "Analyze the sentiment of a user's comment. The result must be a map containing a ':sentiment' key, and the value must be one of the strings 'positive', 'negative', or 'neutral'".to_string()
    });

    let full_prompt = format!(
        "You are an expert RTFS developer specializing in intent generation for AI systems. Your task is to translate natural language requests into precise, executable RTFS intent definitions that can be validated and executed by runtime systems.\n\n{}\n\n{}\n\n{}\n\n{}\n\n### TASK\nUser request: \"{}\"\n\nGenerate a complete RTFS intent definition that:\n1. Captures the user's intent accurately\n2. Includes appropriate constraints and preferences\n3. Has specific, testable success criteria\n4. Follows RTFS syntax and conventions\n\nRTFS:",
        INTENT_GRAMMAR_SNIPPET, GENERATION_STRATEGY, FEW_SHOTS, ANTI_PATTERNS, user_request
    );

    println!("📜 Prompt sent to Hunyuan:\n{}\n---", full_prompt);

    if api_key.is_empty() {
        println!("(Set OPENROUTER_API_KEY to execute the call.)");
        return Ok(());
    }

    // Directly call the provider for simplicity
    let provider = evaluator
        .model_registry
        .get("openrouter-hunyuan-a13b-instruct")
        .expect("provider registered");

    match provider.infer(&full_prompt) {
        Ok(r) => {
            match extract_intent(&r) {
                Some(intent_block) => {
                    println!("\n🎯 Extracted RTFS intent:\n{}", intent_block.trim());

                    // -------- Parse and enhance --------
                    let sanitized = sanitize_regex_literals(&intent_block);
                    match parser::parse(&sanitized) {
                        Ok(ast_items) => {
                            // DEBUG: print entire AST items
                            println!("\n🔍 Parsed AST items: {:#?}", ast_items);

                            // The parser now produces a generic expression. We find the first one
                            // and check if it matches our expected (intent ...) structure.
                            if let Some(TopLevel::Expression(expr)) = ast_items.get(0) {
                                if let Some(mut ccos_intent) = intent_from_function_call(expr) {
                                    // -------- Enrich the CCOS struct --------
                                    if ccos_intent.constraints.is_empty() {
                                        ccos_intent.constraints.insert(
                                            "note".into(),
                                            Value::String("no-constraints-specified".into()),
                                        );
                                    }
                                    if ccos_intent.preferences.is_empty() {
                                        ccos_intent.preferences.insert(
                                            "note".into(),
                                            Value::String("no-preferences-specified".into()),
                                        );
                                    }
                                    if ccos_intent.success_criteria.is_none() {
                                        ccos_intent.success_criteria = Some(Value::Nil);
                                    }
                                    // Print the enriched struct
                                    println!("\n🪄 Enriched CCOS Intent struct:\n{:#?}", ccos_intent);
                                    
                                    // -------- AI Validation and Repair Loop --------
                                    validate_and_repair_intent(&ccos_intent, &intent_block, &user_request, provider.as_ref())?;
                                } else {
                                     eprintln!("Parsed AST expression was not a valid intent definition.");
                                     // Try to repair the intent
                                     attempt_intent_repair(&intent_block, &user_request, provider.as_ref())?;
                                }
                            } else {
                                                                 eprintln!("Parsed AST did not contain a top-level expression for the intent.");
                                 // Try to repair the intent
                                 attempt_intent_repair(&intent_block, &user_request, provider.as_ref())?;
                            }
                        }
                        Err(e) => {
                                                         eprintln!("Failed to parse extracted intent: {:?}", e);
                             // Try to repair the intent
                             attempt_intent_repair(&intent_block, &user_request, provider.as_ref())?;
                        }
                    }
                }
                None => {
                                         println!("\n⚠️  Could not locate a complete (intent …) block. Raw response:\n{}", r.trim());
                     // Try to generate a new intent from scratch
                     generate_intent_from_scratch(&user_request, provider.as_ref())?;
                }
            }
        },
        Err(e) => eprintln!("Error contacting model: {}", e),
    }

    Ok(())
}

/// AI-powered validation and repair of generated intents
fn validate_and_repair_intent(
    intent: &Intent, 
    original_rtfs: &str, 
    user_request: &str, 
    provider: &dyn ModelProvider
) -> Result<(), Box<dyn std::error::Error>> {
    println!("\n🔍 AI Validation and Repair Loop");
    println!("=================================");
    
    // Debug: Print the actual success_criteria value
    println!("🔍 Debug - success_criteria: {:?}", intent.success_criteria);
    
    let mut issues = Vec::new();
    
    // Check for common issues
    if intent.goal.is_empty() {
        issues.push("Missing or empty :goal property");
    }
    if intent.original_request.is_empty() {
        issues.push("Missing or empty :original-request property");
    }
    if intent.constraints.is_empty() || intent.constraints.contains_key("note") {
        issues.push("No meaningful constraints specified");
    }
    if intent.success_criteria.is_none() || matches!(intent.success_criteria, Some(Value::Nil)) {
        issues.push("Missing or empty :success-criteria function");
    }
    
    if issues.is_empty() {
        println!("✅ Intent validation passed - no issues found!");
        write_intent_to_file(original_rtfs, user_request)?;
        return Ok(());
    }
    
    println!("⚠️  Found {} issues:", issues.len());
    for issue in &issues {
        println!("   - {}", issue);
    }
    
    // Generate repair prompt
    let repair_prompt = format!(
        "The following RTFS intent has validation issues. Please fix them:\n\n{}\n\nIssues to fix:\n{}\n\nUser's original request: \"{}\"\n\nPlease provide a corrected RTFS intent that addresses all issues:\n",
        original_rtfs,
        issues.join("\n"),
        user_request
    );
    
    match provider.infer(&repair_prompt) {
        Ok(repaired) => {
            if let Some(repaired_intent) = extract_intent(&repaired) {
                println!("\n🔧 Repaired RTFS intent:\n{}", repaired_intent.trim());
                
                // Validate the repaired intent
                let sanitized = sanitize_regex_literals(&repaired_intent);
                if let Ok(ast_items) = parser::parse(&sanitized) {
                    if let Some(TopLevel::Expression(expr)) = ast_items.get(0) {
                        if let Some(repaired_ccos) = intent_from_function_call(expr) {
                            println!("\n✅ Repaired CCOS Intent:\n{:#?}", repaired_ccos);
                        }
                    }
                }
            }
        }
        Err(e) => eprintln!("Failed to repair intent: {}", e),
    }
    
    Ok(())
}

/// Attempt to repair a malformed intent
fn attempt_intent_repair(
    malformed_rtfs: &str, 
    user_request: &str, 
    provider: &dyn ModelProvider
) -> Result<(), Box<dyn std::error::Error>> {
    println!("\n🔧 Attempting Intent Repair");
    println!("==========================");
    
    let repair_prompt = format!(
        "The following RTFS intent is malformed and cannot be parsed. Please fix the syntax and structure:\n\n{}\n\nUser's original request: \"{}\"\n\nPlease provide a corrected, well-formed RTFS intent:\n",
        malformed_rtfs,
        user_request
    );
    
    println!("📤 Sending repair prompt to model...");
    println!("📝 Repair prompt length: {} characters", repair_prompt.len());
    
    // Simple API call with debugging
    match provider.infer(&repair_prompt) {
        Ok(repaired) => {
            println!("📥 Received repair response ({} characters)", repaired.len());
            if let Some(repaired_intent) = extract_intent(&repaired) {
                println!("\n🔧 Repaired RTFS intent:\n{}", repaired_intent.trim());
                
                // Try to parse the repaired intent
                let sanitized = sanitize_regex_literals(&repaired_intent);
                match parser::parse(&sanitized) {
                    Ok(ast_items) => {
                        if let Some(TopLevel::Expression(expr)) = ast_items.get(0) {
                            if let Some(repaired_ccos) = intent_from_function_call(expr) {
                                println!("\n✅ Successfully repaired and parsed:\n{:#?}", repaired_ccos);
                            }
                        }
                    }
                    Err(e) => eprintln!("Repaired intent still has parsing issues: {:?}", e),
                }
            } else {
                println!("⚠️  Could not extract intent from repair response");
                println!("Raw repair response:\n{}", repaired);
            }
        }
        Err(e) => {
            eprintln!("❌ Failed to repair intent: {}", e);
        }
    }
    
    Ok(())
}

/// Generate a new intent from scratch when extraction fails
fn generate_intent_from_scratch(
    user_request: &str, 
    provider: &dyn ModelProvider
) -> Result<(), Box<dyn std::error::Error>> {
    println!("\n🔄 Generating Intent from Scratch");
    println!("=================================");
    
    let scratch_prompt = format!(
        "The LLM failed to generate a proper RTFS intent. Please generate a complete, well-formed RTFS intent for this user request:\n\nUser request: \"{}\"\n\nPlease provide a complete RTFS intent definition:\n",
        user_request
    );
    
    match provider.infer(&scratch_prompt) {
        Ok(new_intent) => {
            if let Some(intent_block) = extract_intent(&new_intent) {
                println!("\n🆕 Generated RTFS intent:\n{}", intent_block.trim());
                
                // Try to parse the new intent
                let sanitized = sanitize_regex_literals(&intent_block);
                match parser::parse(&sanitized) {
                    Ok(ast_items) => {
                        if let Some(TopLevel::Expression(expr)) = ast_items.get(0) {
                            if let Some(ccos_intent) = intent_from_function_call(expr) {
                                println!("\n✅ Successfully generated and parsed:\n{:#?}", ccos_intent);
                            }
                        }
                    }
                    Err(e) => eprintln!("Generated intent has parsing issues: {:?}", e),
                }
            }
        }
        Err(e) => eprintln!("Failed to generate new intent: {}", e),
    }
    
    Ok(())
}

/// Write the validated RTFS intent to an output file
fn write_intent_to_file(intent_rtfs: &str, user_request: &str) -> Result<(), Box<dyn std::error::Error>> {
    // Create output directory if it doesn't exist
    let output_dir = std::path::Path::new("output");
    if !output_dir.exists() {
        std::fs::create_dir(output_dir)?;
    }
    // Generate filename based on timestamp and sanitized user request
    let timestamp = chrono::Utc::now().format("%Y%m%d_%H%M%S");
    let sanitized_request = user_request
        .chars()
        .filter(|c| c.is_alphanumeric() || c.is_whitespace())
        .collect::<String>()
        .split_whitespace()
        .take(5)
        .collect::<Vec<_>>()
        .join("_");
    let filename = format!("intent_{}_{}.rtfs", timestamp, sanitized_request);
    let filepath = output_dir.join(filename);
    // Write the intent to file
    std::fs::write(&filepath, intent_rtfs)?;
    println!("💾 Saved validated RTFS intent to: {}", filepath.display());
    Ok(())
}