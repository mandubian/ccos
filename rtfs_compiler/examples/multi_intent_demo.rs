//! Multi-Intent Orchestration Demo
//!
//! Demonstrates running multiple intents sequentially with multi-step RTFS plans.
//! Uses Arbiter for intent generation and either a deterministic/stub or LLM plan generator.
//! Prints per-intent lifecycle, step outputs, and a final summary.
//!
//! Try:
//!   cargo run --example multi_intent_demo -- --scenario greet-and-sum --deterministic
//!   cargo run --example multi_intent_demo -- --scenario greet-and-sum --llm-plans --debug --verbose
//!

use clap::Parser;
use std::sync::{Arc, Mutex};
use tokio::sync::RwLock;

use rtfs_compiler::ccos::capability_marketplace::CapabilityMarketplace;
use rtfs_compiler::ccos::causal_chain::CausalChain;
use rtfs_compiler::ccos::intent_graph::IntentGraph;
use rtfs_compiler::ccos::orchestrator::Orchestrator;
use rtfs_compiler::ccos::plan_archive::PlanArchive;
use rtfs_compiler::ccos::types::{ActionType, Intent, Plan};
use rtfs_compiler::runtime::capabilities::registry::CapabilityRegistry;
use rtfs_compiler::runtime::security::RuntimeContext;

use rtfs_compiler::ast::MapKey;
use rtfs_compiler::ccos::arbiter::llm_provider::{LlmProviderConfig, LlmProviderType};
use rtfs_compiler::ccos::arbiter::plan_generation::{
    LlmRtfsPlanGenerationProvider, PlanGenerationProvider, StubPlanGenerationProvider,
};
use rtfs_compiler::ccos::arbiter::{
    arbiter_config::{ArbiterConfig, ArbiterEngineType, LlmConfig},
    arbiter_factory::ArbiterFactory,
};
use rtfs_compiler::runtime::values::Value;

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
        // Value::Atom removed - use host capabilities for state instead
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
#[command(name = "multi_intent_demo")]
#[command(about = "Run a multi-intent, multi-step RTFS demo with Arbiter + Orchestrator")]
struct Args {
    /// Scenario preset
    #[arg(long, default_value = "greet-and-sum")]
    scenario: String,

    /// Force stub (ignore API keys)
    #[arg(long, default_value_t = false)]
    stub: bool,

    /// Use LLM for plan generation (instead of deterministic/stub)
    #[arg(long, default_value_t = false)]
    llm_plans: bool,

    /// Deterministic plans (bypass LLM for plans, still use LLM for intents unless --stub)
    #[arg(long, default_value_t = false)]
    deterministic: bool,

    /// Verbose output
    #[arg(long, default_value_t = false)]
    verbose: bool,

    /// Ask the model for a full (plan ...) wrapper (will still extract :body)
    #[arg(long, default_value_t = false)]
    full_plan: bool,

    /// Debug: print LLM prompts/responses and parsed intent summaries
    #[arg(long, default_value_t = false)]
    debug: bool,
}

struct IntentRunResult {
    intent: Intent,
    plan_id: String,
    success: bool,
    value: Option<Value>,
    error: Option<String>,
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args = Args::parse();

    println!("🧪 Multi-Intent Orchestration Demo\n===============================\n");

    if args.debug {
        std::env::set_var("RTFS_ARBITER_DEBUG", "1");
        std::env::set_var("RTFS_SHOW_PROMPTS", "1");
        println!("🔧 Debug mode enabled (intent/plan prompts will be shown)");
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

    // --- Arbiter config for intent generation ---
    let use_stub_intent = args.stub
        || (std::env::var("OPENROUTER_API_KEY").is_err()
            && std::env::var("OPENAI_API_KEY").is_err());
    let arbiter_config = if use_stub_intent {
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
            capability_config:
                rtfs_compiler::ccos::arbiter::arbiter_config::CapabilityConfig::default(),
            security_config: rtfs_compiler::ccos::arbiter::arbiter_config::SecurityConfig::default(
            ),
            template_config: None,
        }
    } else {
        let (api_key, base_url, model) = if let Ok(key) = std::env::var("OPENROUTER_API_KEY") {
            let model = std::env::var("LLM_MODEL")
                .unwrap_or_else(|_| "moonshotai/kimi-k2:free".to_string());
            (
                Some(key),
                Some("https://openrouter.ai/api/v1".to_string()),
                model,
            )
        } else {
            let key = std::env::var("OPENAI_API_KEY").ok();
            let model = std::env::var("LLM_MODEL").unwrap_or_else(|_| "gpt-4o-mini".to_string());
            (key, None, model)
        };

        ArbiterConfig {
            engine_type: ArbiterEngineType::Llm,
            llm_config: Some(LlmConfig {
                provider_type:
                    rtfs_compiler::ccos::arbiter::arbiter_config::LlmProviderType::OpenAI,
                model,
                api_key,
                base_url,
                max_tokens: Some(512),
                temperature: Some(0.2),
                timeout_seconds: Some(45),
                prompts: None,
            }),
            delegation_config: None,
            capability_config:
                rtfs_compiler::ccos::arbiter::arbiter_config::CapabilityConfig::default(),
            security_config: rtfs_compiler::ccos::arbiter::arbiter_config::SecurityConfig::default(
            ),
            template_config: None,
        }
    };
    let arbiter = ArbiterFactory::create_arbiter(arbiter_config, Arc::clone(&intent_graph), None)
        .await
        .map_err(|e| format!("Failed to create arbiter: {}", e))?;

    // Verbose: show prompts
    if args.verbose {
        std::env::set_var("RTFS_SHOW_PROMPTS", "1");
    }
    if args.full_plan {
        std::env::set_var("RTFS_FULL_PLAN", "1");
    }

    // Build the scenario
    let mut goals: Vec<String> = vec![];
    match args.scenario.as_str() {
        "greet-and-sum" => {
            goals.push("Say Hi using echo".to_string());
            goals.push("Add the integers 2 and 3 and return only the sum".to_string());
            // The 3rd intent will be filled after we get the sum value
        }
        other => {
            goals.push(format!("{}", other));
        }
    }

    // Helper: run a single intent end-to-end and print diagnostics
    async fn run_intent(
        goal: &str,
        llm_plans: bool,
        deterministic: bool,
        marketplace: Arc<CapabilityMarketplace>,
        orchestrator: Arc<Orchestrator>,
        causal_chain: Arc<Mutex<CausalChain>>,
        arbiter: &dyn rtfs_compiler::ccos::arbiter::ArbiterEngine,
        verbose: bool,
    ) -> Result<IntentRunResult, String> {
        // 1) Generate intent
        let intent = arbiter
            .natural_language_to_intent(goal, None)
            .await
            .map_err(|e| format!("Intent generation failed: {}", e))?;

        // 2) Generate plan (deterministic/stub vs LLM)
        let plan_result = if deterministic || !llm_plans {
            if verbose {
                println!("⚙️  Using deterministic StubPlanGenerationProvider\n");
            }
            let provider = StubPlanGenerationProvider;
            provider
                .generate_plan(&intent, Arc::clone(&marketplace))
                .await
                .map_err(|e| format!("Plan generation failed: {}", e))?
        } else {
            let (api_key, base_url, model) = if let Ok(key) = std::env::var("OPENROUTER_API_KEY") {
                let model = std::env::var("LLM_MODEL")
                    .unwrap_or_else(|_| "moonshotai/kimi-k2:free".to_string());
                (
                    Some(key),
                    Some("https://openrouter.ai/api/v1".to_string()),
                    model,
                )
            } else {
                let key = std::env::var("OPENAI_API_KEY").ok();
                let model =
                    std::env::var("LLM_MODEL").unwrap_or_else(|_| "gpt-4o-mini".to_string());
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
            let result = provider
                .generate_plan(&intent, Arc::clone(&marketplace))
                .await
                .map_err(|e| format!("Plan generation failed: {}", e))?;
            if verbose {
                println!("ℹ️ Advisory capability whitelist (demo): :ccos.echo, :ccos.math.add");
            }
            if let Some(diag) = &result.diagnostics {
                if verbose {
                    println!("🩺 Diagnostics: {}", diag);
                }
            }
            result
        };

        let Plan { plan_id, body, .. } = plan_result.plan.clone();
        if verbose {
            if let rtfs_compiler::ccos::types::PlanBody::Rtfs(code) = &body {
                println!("\n📄 Generated RTFS (do ...) body:\n{}\n", code);
            }
        }

        // 3) Lifecycle logs
        if let Ok(mut chain) = causal_chain.lock() {
            let _ = chain.log_intent_created(&plan_id, &intent.intent_id, &intent.goal, None);
            let _ = chain.log_intent_status_change(
                &plan_id,
                &intent.intent_id,
                "Active",
                "Executing",
                "demo:start",
                None,
            );
        }
        if verbose {
            println!("🧠 Intent lifecycle:");
            println!(
                "  • Created intent {} (goal: \"{}\")",
                intent.intent_id, intent.goal
            );
            println!("  • Status: Active → Executing");
        }

        // 4) Execute plan
        println!("🚀 Executing plan {}", plan_id);
        let context = RuntimeContext::full();
        let exec_res = orchestrator.execute_plan(&plan_result.plan, &context).await;

        // 5) Print step outputs
        if let Ok(chain) = causal_chain.lock() {
            let actions = chain.get_actions_for_plan(&plan_id);
            let mut idx = 1;
            let mut printed_header = false;
            let mut seen_ids = std::collections::HashSet::new();
            for a in actions {
                if a.action_type == ActionType::CapabilityResult {
                    if let Some(exec) = &a.result {
                        if !seen_ids.insert(a.action_id.clone()) {
                            continue;
                        }
                        if !printed_header {
                            println!("\n🔎 Step outputs:");
                            printed_header = true;
                        }
                        let cap = a.function_name.as_deref().unwrap_or("<unknown>");
                        let inputs_str = chain
                            .get_parent(&a.action_id)
                            .and_then(|parent| parent.arguments.as_ref())
                            .map(|args| {
                                if args.is_empty() {
                                    "".to_string()
                                } else {
                                    let rendered: Vec<String> =
                                        args.iter().map(|v| render_value_compact(v)).collect();
                                    format!("({})", rendered.join(", "))
                                }
                            })
                            .unwrap_or_else(|| "".to_string());
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
            if verbose {
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

        // 6) Wrap up and return
        match exec_res {
            Ok(exec) => {
                println!(
                    "✅ Execution success: {}",
                    render_value_compact(&exec.value)
                );
                if let Ok(mut chain) = causal_chain.lock() {
                    let _ = chain.log_intent_status_change(
                        &plan_id,
                        &intent.intent_id,
                        "Executing",
                        "Completed",
                        "demo:completed",
                        None,
                    );
                }
                if verbose {
                    println!("  • Status: Executing → Completed");
                }
                Ok(IntentRunResult {
                    intent,
                    plan_id,
                    success: true,
                    value: Some(exec.value),
                    error: None,
                })
            }
            Err(e) => {
                println!("❌ Execution failed: {}", e);
                if let Ok(mut chain) = causal_chain.lock() {
                    let _ = chain.log_intent_status_change(
                        &plan_id,
                        &intent.intent_id,
                        "Executing",
                        "Failed",
                        &format!("demo:error: {}", e),
                        None,
                    );
                }
                if verbose {
                    println!("  • Status: Executing → Failed");
                }
                Ok(IntentRunResult {
                    intent,
                    plan_id,
                    success: false,
                    value: None,
                    error: Some(e.to_string()),
                })
            }
        }
    }

    let mut results: Vec<IntentRunResult> = Vec::new();

    // Intent 1
    if let Some(goal1) = goals.get(0) {
        let res = run_intent(
            goal1,
            args.llm_plans,
            args.deterministic,
            Arc::clone(&marketplace),
            Arc::clone(&orchestrator),
            Arc::clone(&causal_chain),
            arbiter.as_ref(),
            args.verbose,
        )
        .await?;
        results.push(res);
    }

    // Intent 2 (compute sum)
    let mut sum_value: Option<i64> = None;
    if let Some(goal2) = goals.get(1) {
        let res = run_intent(
            goal2,
            args.llm_plans,
            args.deterministic,
            Arc::clone(&marketplace),
            Arc::clone(&orchestrator),
            Arc::clone(&causal_chain),
            arbiter.as_ref(),
            args.verbose,
        )
        .await?;
        // Try to interpret result as integer
        if let Some(Value::Integer(n)) = res.value.clone() {
            sum_value = Some(n);
        }
        results.push(res);
    }

    // Optional Intent 3 (summarize)
    if args.scenario == "greet-and-sum" {
        let summary_goal = if let Some(n) = sum_value {
            format!("Announce the computed sum {} using echo", n)
        } else {
            "Announce that the sum could not be computed".to_string()
        };
        let res = run_intent(
            &summary_goal,
            args.llm_plans,
            args.deterministic,
            Arc::clone(&marketplace),
            Arc::clone(&orchestrator),
            Arc::clone(&causal_chain),
            arbiter.as_ref(),
            args.verbose,
        )
        .await?;
        results.push(res);
    }

    // Final summary
    println!("\n📊 Summary:");
    for (i, r) in results.iter().enumerate() {
        let status = if r.success { "✅" } else { "❌" };
        let goal = &r.intent.goal;
        let val = r
            .value
            .as_ref()
            .map(render_value_compact)
            .unwrap_or_else(|| "nil".to_string());
        println!(
            "  {} Intent {}: {}\n     • goal: {}\n     • plan: {}\n     • output: {}",
            status,
            i + 1,
            r.intent.intent_id,
            goal,
            r.plan_id,
            val
        );
    }

    println!("\n✨ Done.");
    Ok(())
}
