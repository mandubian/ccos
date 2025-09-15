//! LLM RTFS Plan Demo (Reduced Grammar)
//!
//! This example generates a multi-step RTFS plan directly as a `(do ...)` body
//! using the reduced-grammar prompt, then executes it through the Orchestrator.
//! It falls back to a deterministic stub provider when no API key is present.
//!
//! Try:
//!   cargo run --example llm_rtfs_plan_demo -- --goal "Greet the user and add 2 + 3"
//!
//! To use OpenRouter:
//!   export OPENROUTER_API_KEY=... && cargo run --example llm_rtfs_plan_demo -- --goal "..."
//!

use clap::Parser;
// use std::collections::HashMap; // no longer needed here
use std::sync::{Arc, Mutex};
use tokio::sync::RwLock;

use rtfs_compiler::ccos::types::{Plan, ActionType};
use rtfs_compiler::ccos::intent_graph::IntentGraph;
use rtfs_compiler::ccos::causal_chain::CausalChain;
use rtfs_compiler::ccos::orchestrator::Orchestrator;
use rtfs_compiler::ccos::plan_archive::PlanArchive;
use rtfs_compiler::runtime::capability_marketplace::CapabilityMarketplace;
use rtfs_compiler::runtime::capabilities::registry::CapabilityRegistry;
use rtfs_compiler::runtime::security::RuntimeContext;

use rtfs_compiler::ccos::arbiter::plan_generation::{
    PlanGenerationProvider,
    StubPlanGenerationProvider,
    LlmRtfsPlanGenerationProvider,
};
use rtfs_compiler::ccos::arbiter::llm_provider::{LlmProviderConfig, LlmProviderType};
use rtfs_compiler::ccos::arbiter::{
    arbiter_factory::ArbiterFactory,
    arbiter_config::{ArbiterConfig, ArbiterEngineType, LlmConfig},
};
use rtfs_compiler::runtime::values::Value;
use rtfs_compiler::ast::MapKey;

// Compact pretty-printer to render Value maps with friendly keys (e.g., {:message "hi"})
fn render_value_compact(v: &Value) -> String {
    match v {
        Value::Nil => "nil".to_string(),
        Value::Boolean(b) => b.to_string(),
        Value::Integer(i) => i.to_string(),
        Value::Float(fl) => fl.to_string(),
        Value::String(s) => format!("\"{}\"", s),
        Value::Timestamp(t) => format!("#timestamp(\"{}\")", t),
        Value::Uuid(u) => format!("#uuid(\"{}\")", u),
        Value::ResourceHandle(rh) => format!("#resource-handle(\"{}\")", rh),
        Value::Atom(_) => "#<atom>".to_string(),
        Value::Symbol(s) => s.0.clone(),
        Value::Keyword(k) => format!(":{}", k.0),
        Value::Vector(vec) => {
            let items: Vec<String> = vec.iter().map(render_value_compact).collect();
            format!("[{}]", items.join(", "))
        }
        Value::List(list) => {
            let items: Vec<String> = list.iter().map(render_value_compact).collect();
            format!("({})", items.join(", "))
        }
        Value::Map(map) => {
            // Convert keys to friendly strings and sort for readability
            let mut items: Vec<(String, &Value)> = map
                .iter()
                .map(|(k, v)| {
                    let key_str = match k {
                        MapKey::String(s) => format!("\"{}\"", s),
                        MapKey::Keyword(kw) => format!(":{}", kw.0),
                        MapKey::Integer(i) => i.to_string(),
                    };
                    (key_str, v)
                })
                .collect();
            items.sort_by(|a, b| a.0.cmp(&b.0));
            let body: Vec<String> = items
                .into_iter()
                .map(|(k, v)| format!("{} {}", k, render_value_compact(v)))
                .collect();
            format!("{{{}}}", body.join(", "))
        }
        Value::Function(_) => "#<function>".to_string(),
        Value::FunctionPlaceholder(_) => "#<function-placeholder>".to_string(),
        Value::Error(e) => format!("#<error: {}>", e.message),
    }
}

#[derive(Parser, Debug)]
#[command(name = "llm_rtfs_plan_demo")] 
#[command(about = "Generate and execute RTFS plans via reduced-grammar LLM prompt")] 
struct Args {
    /// Natural language goal
    #[arg(long)]
    goal: Option<String>,

    /// Force stub (ignore API keys)
    #[arg(long, default_value_t = false)]
    stub: bool,

    /// Verbose output
    #[arg(long, default_value_t = false)]
    verbose: bool,

    /// Ask the model for a full (plan ...) wrapper (will still extract :body)
    #[arg(long, default_value_t = false)]
    full_plan: bool,

    /// Debug: print intent prompt, raw LLM responses, and parsed intent
    #[arg(long, default_value_t = false)]
    debug: bool,
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args = Args::parse();

    let goal = args.goal.unwrap_or_else(|| {
        "Greet the user and then add 2 and 3 to show a simple calculation".to_string()
    });

    println!("🧪 LLM RTFS Plan Demo (Reduced Grammar)\n======================================\n");

    if args.debug {
        // Enable arbiter intent generation debug + LLM prompt visibility
        std::env::set_var("RTFS_ARBITER_DEBUG", "1");
        std::env::set_var("RTFS_SHOW_PROMPTS", "1");
        println!("🔧 Debug mode enabled (intent prompts/responses will be shown)");
    }

    // --- CCOS subsystems ---
    let causal_chain = Arc::new(Mutex::new(CausalChain::new()?));
    let intent_graph = Arc::new(Mutex::new(IntentGraph::new()?));

    let capability_registry = Arc::new(RwLock::new(CapabilityRegistry::new()));
    let marketplace = CapabilityMarketplace::with_causal_chain(
        Arc::clone(&capability_registry),
        Some(Arc::clone(&causal_chain)),
    );
    marketplace.bootstrap().await?;
    let marketplace = Arc::new(marketplace);
    let plan_archive = Arc::new(PlanArchive::new());

    let orchestrator = Arc::new(Orchestrator::new(
        Arc::clone(&causal_chain),
        Arc::clone(&intent_graph),
        Arc::clone(&marketplace),
        Arc::clone(&plan_archive),
    ));

    // --- Generate Intent via Arbiter (LLM or Stub) ---
    let use_stub = args.stub || std::env::var("OPENROUTER_API_KEY").is_err() && std::env::var("OPENAI_API_KEY").is_err();

    let arbiter_config = if use_stub {
        ArbiterConfig {
            engine_type: ArbiterEngineType::Llm,
            llm_config: Some(LlmConfig {
                provider_type: rtfs_compiler::ccos::arbiter::arbiter_config::LlmProviderType::Stub,
                model: "stub-model".to_string(),
                api_key: None,
                base_url: None,
                max_tokens: Some(512),
                temperature: Some(0.2),
                timeout_seconds: Some(30),
                prompts: None,
            }),
            delegation_config: None,
            capability_config: rtfs_compiler::ccos::arbiter::arbiter_config::CapabilityConfig::default(),
            security_config: rtfs_compiler::ccos::arbiter::arbiter_config::SecurityConfig::default(),
            template_config: None,
        }
    } else {
        let (api_key, base_url, model) = if let Ok(key) = std::env::var("OPENROUTER_API_KEY") {
            let model = std::env::var("LLM_MODEL").unwrap_or_else(|_| "moonshotai/kimi-k2:free".to_string());
            (Some(key), Some("https://openrouter.ai/api/v1".to_string()), model)
        } else {
            let key = std::env::var("OPENAI_API_KEY").ok();
            let model = std::env::var("LLM_MODEL").unwrap_or_else(|_| "gpt-4o-mini".to_string());
            (key, None, model)
        };

        ArbiterConfig {
            engine_type: ArbiterEngineType::Llm,
            llm_config: Some(LlmConfig {
                provider_type: rtfs_compiler::ccos::arbiter::arbiter_config::LlmProviderType::OpenAI,
                model,
                api_key,
                base_url,
                max_tokens: Some(512),
                temperature: Some(0.2),
                timeout_seconds: Some(45),
                prompts: None,
            }),
            delegation_config: None,
            capability_config: rtfs_compiler::ccos::arbiter::arbiter_config::CapabilityConfig::default(),
            security_config: rtfs_compiler::ccos::arbiter::arbiter_config::SecurityConfig::default(),
            template_config: None,
        }
    };

    let arbiter = ArbiterFactory::create_arbiter(arbiter_config, Arc::clone(&intent_graph), None)
        .await
        .map_err(|e| format!("Failed to create arbiter: {}", e))?;
    let intent = arbiter
        .natural_language_to_intent(&goal, None)
        .await
        .map_err(|e| format!("Failed to generate intent: {}", e))?;

    // --- Choose provider: LLM reduced-grammar or stub for PLAN generation ---

    // If verbose, enable prompt display for LLM path
    if args.verbose {
        std::env::set_var("RTFS_SHOW_PROMPTS", "1");
    }

    // If requested, enable full-plan mode for LLM generation
    if args.full_plan {
        std::env::set_var("RTFS_FULL_PLAN", "1");
    }

    let plan_result = if use_stub {
        println!("⚠️  No API key detected or --stub set. Using deterministic StubPlanGenerationProvider.\n");
        if args.verbose {
            println!("ℹ️ Advisory capability whitelist (demo): :ccos.echo, :ccos.math.add");
        }
    let provider = StubPlanGenerationProvider;
        provider.generate_plan(&intent, Arc::clone(&marketplace)).await?
    } else {
        let (api_key, base_url, model) = if let Ok(key) = std::env::var("OPENROUTER_API_KEY") {
            let model = std::env::var("LLM_MODEL").unwrap_or_else(|_| "moonshotai/kimi-k2:free".to_string());
            (Some(key), Some("https://openrouter.ai/api/v1".to_string()), model)
        } else {
            // OpenAI native
            let key = std::env::var("OPENAI_API_KEY").ok();
            let model = std::env::var("LLM_MODEL").unwrap_or_else(|_| "gpt-4o-mini".to_string());
            (key, None, model)
        };

        let config = LlmProviderConfig {
            provider_type: LlmProviderType::OpenAI,
            model,
            api_key,
            base_url,
            max_tokens: Some(512),
            temperature: Some(0.2),
            timeout_seconds: Some(45),
        };
        let provider = LlmRtfsPlanGenerationProvider::new(config);
        let result = provider.generate_plan(&intent, Arc::clone(&marketplace)).await?;
        if args.verbose {
            println!("ℹ️ Advisory capability whitelist (demo): :ccos.echo, :ccos.math.add");
        }
        if let Some(diag) = &result.diagnostics { println!("🩺 Diagnostics: {}", diag); }
        result
    };

    let Plan { plan_id, body, .. } = plan_result.plan.clone();
    if args.verbose {
        if let rtfs_compiler::ccos::types::PlanBody::Rtfs(code) = &body {
            println!("\n📄 Generated RTFS (do ...) body:\n{}\n", code);
        }
    }

    // --- Execute plan ---
    if args.verbose {
        let gen_mode = if use_stub {
            "reduced-grammar (stub)"
        } else if args.full_plan {
            "full-plan"
        } else {
            "reduced-grammar"
        };
        println!("ℹ️ Generation mode: {}", gen_mode);
    }

    // Log intent lifecycle: created and transition to Executing (ledger indexes by plan, so we log after plan exists)
    if let Ok(mut chain) = causal_chain.lock() {
        let _ = chain.log_intent_created(&plan_id, &intent.intent_id, &intent.goal, None);
        let _ = chain.log_intent_status_change(&plan_id, &intent.intent_id, "Active", "Executing", "demo:start", None);
    }
    if args.verbose {
        println!("\n🧠 Intent lifecycle:");
        println!("  • Created intent {} (goal: \"{}\")", intent.intent_id, intent.goal);
        println!("  • Status: Active → Executing");
    }
    println!("🚀 Executing plan {}", plan_id);
    let context = RuntimeContext::full();
    let result = orchestrator.execute_plan(&plan_result.plan, &context).await;
    match result {
        Ok(exec) => {
            println!(
                "✅ Execution success: {}",
                render_value_compact(&exec.value)
            );
            if let Ok(mut chain) = causal_chain.lock() {
                let _ = chain.log_intent_status_change(&plan_id, &intent.intent_id, "Executing", "Completed", "demo:completed", None);
            }
            if args.verbose {
                println!("  • Status: Executing → Completed");
            }
        }
        Err(e) => {
            println!("❌ Execution failed: {}", e);
            if let Ok(mut chain) = causal_chain.lock() {
                let _ = chain.log_intent_status_change(&plan_id, &intent.intent_id, "Executing", "Failed", &format!("demo:error: {}", e), None);
            }
            if args.verbose {
                println!("  • Status: Executing → Failed");
            }
        }
    }

    // --- Show per-step capability calls from the CausalChain ---
    // Note: Capability calls are logged twice: once at call time (no result yet), then again with the result.
    // We display only entries that include a result to avoid duplicate "<no result>" lines.
    if let Ok(chain) = causal_chain.lock() {
    let actions = chain.get_actions_for_plan(&plan_id);
    let mut idx = 1;
    let mut printed_header = false;
    let mut seen_ids = std::collections::HashSet::new();
        for a in actions {
            if a.action_type == ActionType::CapabilityResult {
                // Only show actions that have a recorded result
        if let Some(exec) = &a.result {
            // Deduplicate by action_id in case both pre and post entries exist
            if !seen_ids.insert(a.action_id.clone()) { continue; }
                    if !printed_header {
                        println!("\n🔎 Step outputs (CapabilityCall actions):");
                        printed_header = true;
                    }
                    let cap = a.function_name.as_deref().unwrap_or("<unknown>");
                    // Look up inputs from the parent CapabilityCall action (arguments field)
                    let inputs_str = chain
                        .get_parent(&a.action_id)
                        .and_then(|parent| parent.arguments.as_ref())
                        .map(|args| {
                            if args.is_empty() { "".to_string() } else {
                                let rendered: Vec<String> = args.iter().map(|v| render_value_compact(v)).collect();
                                format!("({})", rendered.join(", "))
                            }
                        })
                        .unwrap_or_else(|| "".to_string());
                    // Pretty-print result values for readability using Value's Display
                    let result_str = render_value_compact(&exec.value);
                    if inputs_str.is_empty() {
                        println!("  {}. {} -> {}", idx, cap, result_str);
                    } else {
                        println!("  {}. {}{} -> {}", idx, cap, inputs_str, result_str);
                    }
                    idx += 1;
                }
            }
        }
        if !printed_header {
            println!("\n🔎 No capability results recorded for this plan.");
        }
        // Optional: verbose debug of all actions for this plan
        if args.verbose {
            println!("\n🧭 Debug: all actions for this plan (type, name, has_result)");
            let mut i = 1;
            for a in chain.get_actions_for_plan(&plan_id) {
                println!(
                    "  {:>2}. {:?} name={} result={}",
                    i,
                    a.action_type,
                    a.function_name.as_deref().unwrap_or("<none>"),
                    a.result.as_ref().map(|_| "yes").unwrap_or("no")
                );
                i += 1;
            }
        }
    }

    Ok(())
}

// No extra pretty-printer needed: Value already implements Display.
