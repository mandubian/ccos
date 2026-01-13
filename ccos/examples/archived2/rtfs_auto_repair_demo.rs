use std::collections::HashMap;
use std::error::Error;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use ccos::arbiter::prompt::{FilePromptStore, PromptManager};
use ccos::intent_graph::config::IntentGraphConfig;
use ccos::rtfs_bridge::RtfsErrorExplainer;
use ccos::types::Plan;
use ccos::{PlanAutoRepairOptions, CCOS};
use clap::{Parser, ValueEnum};
use once_cell::sync::Lazy;
use regex::Regex;
use rtfs::ast::{Keyword, MapKey};
use rtfs::config::profile_selection::expand_profiles;
use rtfs::config::types::{AgentConfig, LlmProfile};
use rtfs::parser::parse;
use rtfs::runtime::error::RuntimeError;
use rtfs::runtime::security::RuntimeContext;
use rtfs::runtime::values::Value;

#[derive(Parser, Debug)]
#[command(
    name = "rtfs-auto-repair-demo",
    about = "Demonstrate LLM-assisted repair of broken RTFS plans"
)]
struct Args {
    /// Path to AgentConfig (TOML or JSON) with delegating arbiter configuration
    #[arg(long)]
    config: String,

    /// Optional LLM profile to activate
    #[arg(long)]
    profile: Option<String>,

    /// Only run a specific fixture (simple | complex | fn-params | type-check | runtime-fault)
    #[arg(long, value_enum)]
    fixture: Option<Fixture>,

    /// Print prompt/response payloads
    #[arg(long, default_value_t = false)]
    debug_prompts: bool,
}

#[derive(Copy, Clone, Debug, ValueEnum, Eq, PartialEq)]
enum Fixture {
    Simple,
    Complex,
    FnParams,
    TypeCheck,
    RuntimeFault,
}

struct BrokenSample {
    name: &'static str,
    description: &'static str,
    source: String,
    kind: SampleKind,
    execution_body: Option<String>,
    repair_guidance: Option<&'static str>,
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
enum SampleKind {
    ParseError,
    TypeError,
    RuntimeError,
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    let args = Args::parse();

    let agent_config = load_agent_config(&args.config)?;
    apply_llm_profile(&agent_config, args.profile.as_deref())?;

    let ccos = Arc::new(
        CCOS::new_with_agent_config_and_configs_and_debug_callback(
            IntentGraphConfig::default(),
            None,
            Some(agent_config),
            None,
        )
        .await
        .map_err(|e| format!("failed to initialize CCOS: {}", e))?,
    );

    ensure_demo_capabilities(&ccos).await?;

    let delegating = ccos
        .get_delegating_arbiter()
        .ok_or("delegating arbiter unavailable (check config/profile)")?;

    let fixtures = select_fixtures(args.fixture);
    let prompt_base_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("assets/prompts/cognitive_engine");
    let prompt_manager = PromptManager::new(FilePromptStore::new(&prompt_base_dir));
    let repair_options = PlanAutoRepairOptions::default();
    println!("Running {} RTFS repair fixture(s)...\n", fixtures.len());

    for sample in fixtures {
        println!("════════════════════════════════════════════════════════════════════════");
        println!("🧪 Fixture: {} — {}", sample.name, sample.description);
        println!("────────────────────────────────────────────────────────────────────────");
        println!("Original RTFS (broken):\n{}\n", sample.source.trim());

        let (diagnostics, mut hints) = match sample.kind {
            SampleKind::ParseError => match parse(&sample.source) {
                Ok(_) => {
                    println!("⚠️  Parser accepted this fixture unexpectedly; skipping.\n");
                    continue;
                }
                Err(parse_err) => {
                    let runtime_error =
                        RuntimeError::Generic(format!("Failed to parse plan: {:?}", parse_err));
                    let diagnostics = RtfsErrorExplainer::explain(&runtime_error)
                        .map(|diag| RtfsErrorExplainer::format_for_llm(&diag))
                        .unwrap_or_else(|| format!("{}", runtime_error));
                    (diagnostics, repair_options.grammar_hints.clone())
                }
            },
            SampleKind::TypeError | SampleKind::RuntimeError => {
                if let Err(parse_err) = parse(&sample.source) {
                    println!(
                        "⚠️  Expected syntactically valid plan, but parser reported an error: {:?}\n",
                        parse_err
                    );
                    continue;
                }

                let plan_body = sample
                    .execution_body
                    .clone()
                    .unwrap_or_else(|| sample.source.clone());
                let plan = Plan::new_rtfs(plan_body, Vec::new());
                let context = RuntimeContext::full();
                let error = match ccos.validate_and_execute_plan(plan, &context).await {
                    Ok(_) => {
                        println!("⚠️  Plan executed without error; skipping.\n");
                        continue;
                    }
                    Err(err) => err,
                };

                let diag_opt = RtfsErrorExplainer::explain(&error);
                let diagnostics = diag_opt
                    .as_ref()
                    .map(RtfsErrorExplainer::format_for_llm)
                    .unwrap_or_else(|| format!("{}", error));

                let mut hints = repair_options.grammar_hints.clone();
                if let Some(diag) = diag_opt {
                    for hint in diag.hints {
                        if !hints.iter().any(|existing| existing == &hint) {
                            hints.push(hint);
                        }
                    }
                }

                (diagnostics, hints)
            }
        };

        println!("Compiler diagnostics:\n{}\n", diagnostics.trim());

        if hints.is_empty() {
            hints = repair_options.grammar_hints.clone();
        }

        let prompt = build_repair_prompt(
            &prompt_manager,
            sample.name,
            sample.repair_guidance,
            &sample.source,
            &diagnostics,
            &hints,
        )
        .map_err(|e| format!("failed to render auto-repair prompt: {}", e))?;

        if args.debug_prompts {
            println!("┌─ Prompt ──────────────────────────────────────────────────────────────");
            println!("{}", prompt);
            println!("└───────────────────────────────────────────────────────────────────────\n");
        }

        let response = delegating
            .generate_raw_text(&prompt)
            .await
            .map_err(|e| format!("LLM repair request failed: {}", e))?;

        if args.debug_prompts {
            println!("┌─ LLM Response ────────────────────────────────────────────────────────");
            println!("{}", response.trim());
            println!("└───────────────────────────────────────────────────────────────────────\n");
        }

        match extract_plan_rtfs_from_response(&response) {
            Some(repaired) => match parse(&repaired) {
                Ok(_) => {
                    println!(
                        "✅ Repaired RTFS plan (compiles successfully):\n{}\n",
                        repaired
                    );
                }
                Err(err) => {
                    println!(
                        "❌ Repaired plan still fails to parse: {:?}\n{}",
                        err, repaired
                    );
                }
            },
            None => {
                println!("❌ LLM response did not contain a parsable `(plan ...)` form.\n")
            }
        }
    }

    Ok(())
}

fn select_fixtures(filter: Option<Fixture>) -> Vec<BrokenSample> {
    let mut samples = Vec::new();
    if filter.is_none() || filter == Some(Fixture::Simple) {
        samples.push(BrokenSample {
            name: "simple-map-syntax",
            description: "Incorrect `=` usage inside capability map",
            source: build_simple_broken_plan(),
            kind: SampleKind::ParseError,
            execution_body: None,
            repair_guidance: None,
        });
    }
    if filter.is_none() || filter == Some(Fixture::Complex) {
        samples.push(BrokenSample {
            name: "complex-structure",
            description: "Broken step body (bad map syntax + missing closing paren)",
            source: build_complex_broken_plan(),
            kind: SampleKind::ParseError,
            execution_body: None,
            repair_guidance: None,
        });
    }
    if filter.is_none() || filter == Some(Fixture::FnParams) {
        samples.push(BrokenSample {
            name: "lambda-parameters",
            description: "Lambda parameter list missing closing bracket",
            source: build_lambda_params_plan(),
            kind: SampleKind::ParseError,
            execution_body: None,
            repair_guidance: Some(
                "- Lambdas must declare their parameters in [vec] form, e.g. (fn [acc item] ...).",
            ),
        });
    }
    if filter.is_none() || filter == Some(Fixture::TypeCheck) {
        samples.push(BrokenSample {
            name: "type-error",
            description: "Capability invoked with incorrect argument shape",
            source: build_type_error_plan(),
            kind: SampleKind::TypeError,
            execution_body: Some(build_type_error_body()),
            repair_guidance: Some(
                "- Available local capabilities: :core.echo (expects {:message string}), :core.math.add (integers), :ccos.io.println / :ccos.io.log (println-style output).\n\
                 - Use map syntax {:message \"text\"}; do not pass bare strings to capability calls.",
            ),
        });
    }
    if filter.is_none() || filter == Some(Fixture::RuntimeFault) {
        samples.push(BrokenSample {
            name: "runtime-fault",
            description: "Valid syntax that fails at runtime (division by zero)",
            source: build_runtime_error_plan(),
            kind: SampleKind::RuntimeError,
            execution_body: Some(build_runtime_error_body()),
            repair_guidance: Some(
                "- Core forms available: let, if, do, zero?, =, +, -, *, /, str, vector/list helpers.\n\
                 - Local capabilities: :core.echo {:message ...}, :core.math.add, :ccos.io.println / :ccos.io.log.\n\
                 - Prevent the runtime error while preserving the step’s intended result (e.g. guard invalid inputs and return a safe numeric value). Rely only on existing primitives/capabilities; do not invent new IDs.",
            ),
        });
    }
    samples
}

fn build_simple_broken_plan() -> String {
    r#"
(plan "demo-simple"
  :language rtfs20
  :body
    (do
      (step "Announce result"
        (call :core.echo {:message = hello})))
)"#
    .to_string()
}

fn build_complex_broken_plan() -> String {
    let plan = r#"
(plan "demo-complex"
  :language rtfs20
  :body
    (do
      (step "Find max repository"
        (let [repos (call :core.list.normalize {:items projects})
              best-repo (call :core.list.find_max_by_property {:list repos :property = "stargazers_count"})]
          (call :core.echo {:message (str "Top repo: " (get best-repo :name))}))))"#.to_string();
    // Deliberately drop the final closing parenthesis to trigger an EOF error
    plan
}

fn build_lambda_params_plan() -> String {
    r#"
(plan "demo-lambda"
  :language rtfs20
  :body
    (do
      (step "Sum values"
        (let [values [1 2 3 4]
              total (reduce values (fn [acc item (+ acc item)) 0)]
          (call :core.echo {:message (str "Total: " total)})))))"#
        .to_string()
}

fn build_type_error_plan() -> String {
    r#"
(plan "demo-type-error"
  :language rtfs20
  :body
    (do
      (step "Echo with wrong args"
        (call :core.echo "Hello world"))))"#
        .to_string()
}

fn build_runtime_error_plan() -> String {
    r#"
(plan "demo-runtime-error"
  :language rtfs20
  :body
    (do
      (step "Divide by zero"
        (/ 42 0))))"#
        .to_string()
}

fn build_type_error_body() -> String {
    r#"
(do
  (step "Echo with wrong args"
    (call :core.echo "Hello world")))"#
        .trim()
        .to_string()
}

fn build_runtime_error_body() -> String {
    r#"
(do
  (step "Divide by zero"
    (/ 42 0)))"#
        .trim()
        .to_string()
}

async fn ensure_demo_capabilities(ccos: &Arc<CCOS>) -> Result<(), Box<dyn Error>> {
    let marketplace = ccos.get_capability_marketplace();

    if marketplace.get_capability("core.echo").await.is_none() {
        marketplace
            .register_local_capability(
                "core.echo".to_string(),
                "Core Echo (demo)".to_string(),
                "Echoes the provided :message text".to_string(),
                Arc::new(|input| match input {
                    Value::Map(map) => {
                        let key = MapKey::Keyword(Keyword("message".to_string()));
                        match map.get(&key) {
                            Some(Value::String(text)) => Ok(Value::String(text.clone())),
                            Some(other) => Err(RuntimeError::TypeError {
                                expected: "string".to_string(),
                                actual: other.type_name().to_string(),
                                operation: "core.echo".to_string(),
                            }),
                            None => Err(RuntimeError::Generic(
                                "core.echo requires :message".to_string(),
                            )),
                        }
                    }
                    other => Err(RuntimeError::TypeError {
                        expected: "map".to_string(),
                        actual: other.type_name().to_string(),
                        operation: "core.echo".to_string(),
                    }),
                }),
            )
            .await
            .map_err(|e| format!("failed to register core.echo demo capability: {:?}", e))?;
    }

    Ok(())
}

fn build_repair_prompt(
    prompt_manager: &PromptManager<FilePromptStore>,
    fixture_name: &str,
    guidance: Option<&str>,
    broken_plan: &str,
    diagnostics: &str,
    hints: &[String],
) -> Result<String, RuntimeError> {
    let grammar_block = if hints.is_empty() {
        String::from("Remember these RTFS rules:\n- (no additional dynamic hints)")
    } else {
        let mut block = String::from("Remember these RTFS rules:\n");
        for hint in hints {
            block.push_str("- ");
            block.push_str(hint);
            if !block.ends_with('\n') {
                block.push('\n');
            }
        }
        if !block.ends_with('\n') {
            block.push('\n');
        }
        block.trim_end().to_string()
    };

    let guidance_block = guidance
        .map(|g| g.trim().to_string())
        .filter(|g| !g.is_empty())
        .map(|g| format!("Fixture-specific guidance:\n{}", g))
        .unwrap_or_default();

    let mut vars = HashMap::new();
    vars.insert("fixture_name".to_string(), fixture_name.to_string());
    vars.insert("diagnostics".to_string(), diagnostics.trim().to_string());
    vars.insert("grammar_hint_block".to_string(), grammar_block);
    vars.insert("fixture_guidance_block".to_string(), guidance_block);
    vars.insert("broken_plan".to_string(), broken_plan.trim().to_string());

    prompt_manager.render("auto_repair", "v1", &vars)
}

fn extract_plan_rtfs_from_response(response: &str) -> Option<String> {
    static CODE_BLOCK_RE: Lazy<Regex> = Lazy::new(|| {
        Regex::new(r"```(?:rtfs|lisp|scheme)?\s*([\s\S]*?)```").expect("invalid regex")
    });
    if let Some(caps) = CODE_BLOCK_RE.captures(response) {
        let code = caps.get(1).unwrap().as_str().trim();
        if code.starts_with("(plan") {
            return Some(code.to_string());
        }
    }
    let trimmed = response.trim().trim_matches('`').trim();
    if trimmed.starts_with("(plan") {
        return Some(trimmed.to_string());
    }
    None
}

fn load_agent_config(path: &str) -> Result<AgentConfig, Box<dyn Error>> {
    let raw = fs::read_to_string(path)?;
    let ext = Path::new(path)
        .extension()
        .and_then(|s| s.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();
    if ext == "json" {
        Ok(serde_json::from_str(&raw)?)
    } else {
        Ok(toml::from_str(&raw)?)
    }
}

fn apply_llm_profile(
    config: &AgentConfig,
    profile_name: Option<&str>,
) -> Result<(), Box<dyn Error>> {
    std::env::set_var("CCOS_ENABLE_DELEGATION", "1");

    if let Some(llm_profiles) = &config.llm_profiles {
        let (profiles, _meta, _why) = expand_profiles(config);
        let chosen = profile_name
            .map(|s| s.to_string())
            .or_else(|| llm_profiles.default.clone())
            .or_else(|| profiles.first().map(|p| p.name.clone()));

        if let Some(name) = chosen {
            if let Some(profile) = profiles.iter().find(|p| p.name == name) {
                apply_profile_env(profile);
                println!("Activated LLM profile '{}'.", name);
            } else {
                return Err(format!("profile '{}' not found in AgentConfig", name).into());
            }
        } else if let Some(first) = profiles.first() {
            apply_profile_env(first);
            println!("Activated default LLM profile '{}'.", first.name);
        }
    } else if let Some(requested) = profile_name {
        return Err(format!(
            "profile '{}' requested but no llm_profiles configured",
            requested
        )
        .into());
    }

    Ok(())
}

fn apply_profile_env(profile: &LlmProfile) {
    std::env::set_var("CCOS_DELEGATING_MODEL", &profile.model);
    std::env::set_var("CCOS_LLM_MODEL", &profile.model);
    std::env::set_var("CCOS_LLM_PROVIDER_HINT", &profile.provider);

    if let Some(url) = &profile.base_url {
        std::env::set_var("CCOS_LLM_BASE_URL", url);
    } else if profile.provider == "openrouter" {
        if std::env::var("CCOS_LLM_BASE_URL").is_err() {
            std::env::set_var("CCOS_LLM_BASE_URL", "https://openrouter.ai/api/v1");
        }
    }

    if let Some(api_key) = profile.api_key.as_ref() {
        set_api_key(&profile.provider, api_key);
    } else if let Some(env) = &profile.api_key_env {
        if let Ok(value) = std::env::var(env) {
            set_api_key(&profile.provider, &value);
        }
    }

    match profile.provider.as_str() {
        "openai" => std::env::set_var("CCOS_LLM_PROVIDER", "openai"),
        "claude" | "anthropic" => std::env::set_var("CCOS_LLM_PROVIDER", "anthropic"),
        "openrouter" => {
            std::env::set_var("CCOS_LLM_PROVIDER", "openrouter");
            if std::env::var("CCOS_LLM_BASE_URL").is_err() {
                std::env::set_var("CCOS_LLM_BASE_URL", "https://openrouter.ai/api/v1");
            }
        }
        "local" => std::env::set_var("CCOS_LLM_PROVIDER", "local"),
        "stub" => {
            eprintln!("⚠️  WARNING: Using stub LLM provider (testing only)");
            std::env::set_var("CCOS_LLM_PROVIDER", "stub");
            std::env::set_var("CCOS_ALLOW_STUB_PROVIDER", "1");
        }
        other => std::env::set_var("CCOS_LLM_PROVIDER", other),
    }
}

fn set_api_key(provider: &str, key: &str) {
    match provider {
        "openrouter" => std::env::set_var("OPENROUTER_API_KEY", key),
        "claude" | "anthropic" => std::env::set_var("ANTHROPIC_API_KEY", key),
        "gemini" => std::env::set_var("GEMINI_API_KEY", key),
        "stub" => {}
        _ => std::env::set_var("OPENAI_API_KEY", key),
    }
}
