//! Synthesis Test Script
//!
//! Tests LLM-based capability synthesis with real config and validates output.
//!
//! This script uses the CCOS environment builder to set up LLM access,
//! then synthesizes RTFS code for a given capability description.
//!
//! Usage:
//!   cargo run --example synthesis_test -- --capability "add two numbers"
//!   cargo run --example synthesis_test -- --capability "reverse a string" --verbose

use ccos::examples_common::builder::CcosEnvBuilder;
use ccos::governance_kernel::SynthesisRiskAssessment;
use clap::Parser;
use std::error::Error;

#[derive(Parser, Debug)]
struct Args {
    /// Capability description to synthesize
    #[arg(long, default_value = "add two numbers and return the sum")]
    capability: String,

    /// Path to agent config file
    #[arg(long, default_value = "config/agent_config.toml")]
    config: String,

    /// LLM profile to use (e.g., "openrouter_free:balanced")
    #[arg(long, default_value = "openrouter_free:balanced")]
    profile: String,

    /// Show verbose output
    #[arg(long)]
    verbose: bool,
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error + Send + Sync>> {
    let args = Args::parse();

    println!("\n╔══════════════════════════════════════════════════════════════╗");
    println!("║           🧪 LLM Synthesis Test                              ║");
    println!("╚══════════════════════════════════════════════════════════════╝\n");

    println!("📋 Capability to synthesize: \"{}\"\n", args.capability);

    // =========================================================================
    // Step 1: Check governance authorization
    // =========================================================================
    println!("🔒 Step 1: Governance Check");
    println!("─────────────────────────────────────────────────────────────");

    let capability_id = format!("synthesized.{}", sanitize_id(&args.capability));
    let risk_assessment = SynthesisRiskAssessment::assess(&capability_id);

    println!("   Capability ID: {}", capability_id);
    println!("   Risk Level: {:?}", risk_assessment.risk);
    println!("   Risk Factors: {:?}", risk_assessment.risk_factors);
    println!(
        "   Requires Human Approval: {}",
        risk_assessment.requires_human_approval
    );

    // =========================================================================
    // Step 2: Build CCOS environment and create LLM provider
    // =========================================================================
    println!("\n⚙️  Step 2: Loading Environment");
    println!("─────────────────────────────────────────────────────────────");

    let env = CcosEnvBuilder::new()
        .with_config(&args.config)
        .build()
        .await?;

    println!("   ✓ CCOS environment initialized");
    println!("   ✓ Config loaded from: {}", args.config);

    // Create LLM provider from the profile
    println!("\n   Creating LLM provider from profile: {}", args.profile);
    let llm_provider = env.create_llm_provider(&args.profile).await?;
    println!("   ✓ LLM provider created");

    // =========================================================================
    // Step 3: Synthesize the capability using LLM
    // =========================================================================
    println!("\n🔧 Step 3: Synthesizing Capability via LLM");
    println!("─────────────────────────────────────────────────────────────");

    // Build a synthesis prompt
    let synthesis_prompt = build_synthesis_prompt(&args.capability);

    if args.verbose {
        println!("   📝 Prompt:");
        for line in synthesis_prompt.lines() {
            println!("   {}", line);
        }
        println!();
    }

    println!("   📤 Sending to LLM...");

    let llm_response = llm_provider.generate_text(&synthesis_prompt).await?;

    println!("   📥 LLM Response received ({} chars)", llm_response.len());

    if args.verbose {
        println!("\n   Raw LLM Response:\n   ──────────────────");
        for line in llm_response.lines().take(40) {
            println!("   {}", line);
        }
        if llm_response.lines().count() > 40 {
            println!("   ... (truncated)");
        }
    }

    // =========================================================================
    // Step 4: Extract and validate RTFS
    // =========================================================================
    println!("\n✅ Step 4: Validating Synthesized RTFS");
    println!("─────────────────────────────────────────────────────────────");

    // Extract RTFS code block from response
    let rtfs_code = extract_rtfs_code(&llm_response);

    match rtfs_code {
        Some(code) => {
            println!("   ✓ Extracted RTFS code ({} chars)", code.len());
            println!("\n   Synthesized RTFS:\n   ──────────────────");
            for line in code.lines() {
                println!("   {}", line);
            }

            // Try to parse the RTFS
            match rtfs::parser::parse_expression(&code) {
                Ok(_ast) => {
                    println!("\n   ✓ RTFS parsed successfully!");
                    println!("   The synthesized capability is syntactically valid.");

                    // =========================================================================
                    // Step 5: Execute the synthesized code
                    // =========================================================================
                    println!("\n🚀 Step 5: Executing Synthesized Code");
                    println!("─────────────────────────────────────────────────────────────");

                    // Create a simple test execution
                    execute_synthesized_code(&code, &args.capability);
                }
                Err(e) => {
                    println!("\n   ✗ RTFS parse error: {:?}", e);
                }
            }
        }
        None => {
            println!("   ✗ Could not extract RTFS code from LLM response");
            println!("   Try running with --verbose to see the full response");

            // Show the raw response for debugging
            println!("\n   Raw response (first 500 chars):");
            println!("   {}", &llm_response[..llm_response.len().min(500)]);
        }
    }

    println!("\n✅ Synthesis test complete!");
    Ok(())
}

/// Build synthesis prompt for capability generation
fn build_synthesis_prompt(description: &str) -> String {
    format!(
        r#"Generate a pure RTFS capability function for the following need:

**Need:** {}

**RTFS Language Rules (IMPORTANT - NOT Clojure!):**
RTFS is a pure functional language. It looks like Clojure but has key differences:

SUPPORTED:
- Anonymous functions: (fn [a b] (+ a b))
- Vectors: [1 2 3]
- Maps: {{"key" value}}
- Keywords: :keyword
- let, if, do, match, reduce, map, filter

NOT SUPPORTED (will cause parse errors):
- Quote syntax: '() or 'expr - DO NOT USE
- Atoms/mutation: atom, deref, reset!, swap!, @atom - DO NOT USE  
- Set literals: #{{}} - DO NOT USE
- Regex literals: #"pattern" - DO NOT USE
- cons function - use conj instead

**Available Primitives:**
+, -, *, /, str, first, rest, count, map, filter, reduce, get, assoc, conj, concat, reverse, nth, empty?

**Examples of valid RTFS functions:**

For "add two numbers":
```rtfs
(fn [a b] (+ a b))
```

For "reverse a vector":
```rtfs
(fn [items] (reduce (fn [acc x] (conj acc x)) [] (reverse items)))
```

For "sum a list":
```rtfs
(fn [numbers] (reduce + 0 numbers))
```

**Now generate the RTFS function for: {}**

Wrap your answer in ```rtfs code blocks. Keep it simple and pure:
"#,
        description, description
    )
}

/// Extract RTFS code from LLM response (looks for ```rtfs blocks)
fn extract_rtfs_code(response: &str) -> Option<String> {
    // Look for ```rtfs ... ``` blocks
    if let Some(start) = response.find("```rtfs") {
        let after_marker = &response[start + 7..];
        if let Some(end) = after_marker.find("```") {
            let code = after_marker[..end].trim();
            if !code.is_empty() {
                return Some(code.to_string());
            }
        }
    }

    // Fallback: look for ``` ... ``` with (fn
    if let Some(start) = response.find("```") {
        let after_marker = &response[start + 3..];
        let code_start = after_marker.find('\n').map(|i| i + 1).unwrap_or(0);
        let after_lang = &after_marker[code_start..];
        if let Some(end) = after_lang.find("```") {
            let code = after_lang[..end].trim();
            if !code.is_empty() && code.starts_with('(') {
                return Some(code.to_string());
            }
        }
    }

    // Fallback: look for bare (fn ...) expressions
    if let Some(start) = response.find("(fn ") {
        let mut depth = 0;
        let mut end = start;
        for (i, c) in response[start..].char_indices() {
            match c {
                '(' => depth += 1,
                ')' => {
                    depth -= 1;
                    if depth == 0 {
                        end = start + i + 1;
                        break;
                    }
                }
                _ => {}
            }
        }
        if end > start {
            return Some(response[start..end].to_string());
        }
    }

    None
}

/// Sanitize capability ID from description
fn sanitize_id(description: &str) -> String {
    description
        .to_lowercase()
        .chars()
        .map(|c| if c.is_alphanumeric() { c } else { '_' })
        .take(40)
        .collect()
}

/// Execute synthesized code with test inputs
fn execute_synthesized_code(code: &str, capability_desc: &str) {
    use rtfs::runtime::ModuleRegistry;
    use rtfs::runtime::Runtime;
    use std::sync::Arc;

    // Create test expression based on capability description
    let test_expr = infer_test_expression(code, capability_desc);

    println!("   Test expression: {}", test_expr);

    // Create runtime and evaluate
    let module_registry = Arc::new(ModuleRegistry::new());
    let mut runtime = Runtime::new_with_tree_walking_strategy(module_registry);

    match runtime.run(&rtfs::parser::parse_expression(&test_expr).unwrap()) {
        Ok(result) => {
            println!("   ✓ Execution result: {:?}", result);
        }
        Err(e) => {
            println!("   ✗ Execution error: {:?}", e);
        }
    }
}

/// Infer a test expression for the synthesized function
fn infer_test_expression(code: &str, capability_desc: &str) -> String {
    let desc_lower = capability_desc.to_lowercase();

    // Try to infer appropriate test inputs based on the capability description
    if desc_lower.contains("add") && desc_lower.contains("number") {
        format!("({} 2 3)", code)
    } else if desc_lower.contains("multiply") && desc_lower.contains("three") {
        format!("({} 2 3 4)", code)
    } else if desc_lower.contains("multiply") && desc_lower.contains("number") {
        format!("({} 5 7)", code)
    } else if desc_lower.contains("sum") || desc_lower.contains("total") {
        format!("({} [1 2 3 4 5])", code)
    } else if desc_lower.contains("max") || desc_lower.contains("maximum") {
        format!("({} [3 1 4 1 5 9 2 6])", code)
    } else if desc_lower.contains("min") || desc_lower.contains("minimum") {
        format!("({} [3 1 4 1 5 9 2 6])", code)
    } else if desc_lower.contains("reverse") && desc_lower.contains("string") {
        format!("({} \"hello\")", code)
    } else if desc_lower.contains("reverse") {
        format!("({} [1 2 3])", code)
    } else if desc_lower.contains("even") {
        format!("({} 4)", code)
    } else if desc_lower.contains("odd") {
        format!("({} 3)", code)
    } else if desc_lower.contains("first") {
        format!("({} [1 2 3])", code)
    } else if desc_lower.contains("last") {
        format!("({} [1 2 3])", code)
    } else if desc_lower.contains("count") || desc_lower.contains("length") {
        format!("({} [1 2 3 4 5])", code)
    } else if desc_lower.contains("concat") || desc_lower.contains("join") {
        format!("({} \"hello\" \" \" \"world\")", code)
    } else {
        // Default: try with simple number args
        format!("({} 1 2)", code)
    }
}
