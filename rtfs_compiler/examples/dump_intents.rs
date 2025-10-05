use clap::Parser;
use rtfs_compiler::ccos::CCOS;
use rtfs_compiler::ccos::arbiter::ArbiterEngine;
use rtfs_compiler::config::expand_profiles;
use rtfs_compiler::config::types::AgentConfig;
use rtfs_compiler::config::types::LlmProfile;
use rtfs_compiler::runtime::security::{RuntimeContext, SecurityLevel};
use serde_json::json;
use std::fs;
use std::path::Path;
use std::sync::Arc;

#[derive(Parser, Debug)]
struct Args {
    /// One-shot prompt (intent text or RTFS intent)
    #[arg(long)]
    prompt: Option<String>,

    /// Load agent config (JSON or TOML)
    #[arg(long)]
    config: Option<String>,

    /// Override LLM provider (e.g. stub)
    #[arg(long)]
    llm_provider: Option<String>,

    /// Override LLM model
    #[arg(long)]
    llm_model: Option<String>,

    /// Override api key
    #[arg(long)]
    llm_api_key: Option<String>,
}

fn load_agent_config(path: &str) -> Result<AgentConfig, Box<dyn std::error::Error>> {
    let raw = fs::read_to_string(path)?;
    let ext = Path::new(path)
        .extension()
        .and_then(|s| s.to_str())
        .unwrap_or("")
        .to_lowercase();
    if ext == "toml" || ext == "tml" {
        Ok(toml::from_str(&raw)?)
    } else {
        Ok(serde_json::from_str(&raw)?)
    }
}

fn apply_profile_env(p: &LlmProfile, announce: bool) {
    std::env::set_var("CCOS_DELEGATING_MODEL", &p.model);
    std::env::set_var("CCOS_LLM_PROVIDER_HINT", &p.provider);
    if let Some(url) = &p.base_url {
        std::env::set_var("CCOS_LLM_BASE_URL", url);
    }
    if let Some(inline) = &p.api_key {
        match p.provider.as_str() {
            "openrouter" => std::env::set_var("OPENROUTER_API_KEY", inline),
            "claude" | "anthropic" => std::env::set_var("ANTHROPIC_API_KEY", inline),
            "gemini" => std::env::set_var("GEMINI_API_KEY", inline),
            _ => std::env::set_var("OPENAI_API_KEY", inline),
        }
    }
    match p.provider.as_str() {
        "openai" => std::env::set_var("CCOS_LLM_PROVIDER", "openai"),
        "claude" | "anthropic" => std::env::set_var("CCOS_LLM_PROVIDER", "anthropic"),
        "stub" => std::env::set_var("CCOS_LLM_PROVIDER", "stub"),
        _ => {}
    }
    std::env::set_var("CCOS_LLM_MODEL", &p.model);
    if announce {
        println!("[config] applied profile '{}' provider={} model={}", p.name, p.provider, p.model);
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args = Args::parse();

    let mut loaded_config: Option<AgentConfig> = None;
    if let Some(cfg_path) = &args.config {
        match load_agent_config(cfg_path) {
            Ok(cfg) => loaded_config = Some(cfg),
            Err(e) => eprintln!("[config] failed to load {}: {}", cfg_path, e),
        }
    }

    let (expanded_profiles, _profile_meta, expansion_rationale) = if let Some(cfg) = &loaded_config {
        expand_profiles(cfg)
    } else {
        (Vec::new(), std::collections::HashMap::new(), String::new())
    };
    if !expansion_rationale.is_empty() {
        println!("[config] profiles expanded:\n{}", expansion_rationale);
    }

    // Apply default profile if config present
    if args.llm_model.is_none() && args.llm_provider.is_none() {
        if let Some(cfg) = &loaded_config {
            if let Some(llm_cfg) = &cfg.llm_profiles {
                if let Some(default_name) = &llm_cfg.default {
                    if let Some(p) = expanded_profiles.iter().find(|p| &p.name == default_name) {
                        apply_profile_env(p, true);
                    }
                } else if !expanded_profiles.is_empty() {
                    apply_profile_env(&expanded_profiles[0], true);
                }
            }
        }
    }

    // CLI overrides
    if let Some(m) = &args.llm_model { std::env::set_var("CCOS_DELEGATING_MODEL", m); }
    if let Some(provider) = &args.llm_provider {
        std::env::set_var("CCOS_LLM_PROVIDER_HINT", provider);
        if provider == "stub" { std::env::set_var("CCOS_LLM_PROVIDER", "stub"); }
    }
    if let Some(key) = &args.llm_api_key {
        let hint = args.llm_provider.as_deref().unwrap_or("openai");
        match hint {
            "openrouter" => std::env::set_var("OPENROUTER_API_KEY", key),
            "claude" => std::env::set_var("ANTHROPIC_API_KEY", key),
            "gemini" => std::env::set_var("GEMINI_API_KEY", key),
            _ => std::env::set_var("OPENAI_API_KEY", key),
        }
    }

    // Ensure delegation enabled for parsing
    std::env::set_var("CCOS_ENABLE_DELEGATION", "1");

    let ctx = RuntimeContext {
        security_level: SecurityLevel::Controlled,
        allowed_capabilities: vec!["ccos.echo".to_string(), "ccos.user.ask".to_string()].into_iter().collect(),
        ..RuntimeContext::pure()
    };

    let ccos = Arc::new(CCOS::new().await?);

    if let Some(prompt) = &args.prompt {
        // If a delegating arbiter is available, call it directly to see the raw Intent it produces
        if let Some(da) = ccos.get_delegating_arbiter() {
            match (&*da).natural_language_to_intent_with_raw(prompt, None).await {
                Ok((intent, raw)) => {
                    println!("--- ARBITER RAW LLM RESPONSE ---\n{}\n--- END RAW RESPONSE ---", raw);
                    println!("--- ARBITER PARSED INTENT ---\n{:#?}\n--- END PARSED INTENT ---", intent);
                }
                Err(e) => {
                    eprintln!("failed to produce intent via delegating arbiter: {}", e);
                }
            }
        } else {
            eprintln!("No delegating arbiter available on this CCOS instance (check CCOS_ENABLE_DELEGATION)");
        }

        // Use process_request_with_plan to materialize intent and print the synthesized plan
        match ccos.process_request_with_plan(prompt, &ctx).await {
            Ok((plan, res)) => {
                println!("--- GENERATED PLAN (RTFS) ---");
                match &plan.body {
                    rtfs_compiler::ccos::types::PlanBody::Rtfs(src) => println!("{}", src),
                    rtfs_compiler::ccos::types::PlanBody::Wasm{..} => println!("<wasm module plan>"),
                }
                println!("--- END PLAN ---");
                println!("--- EXECUTION RESULT ---\nsuccess={} value={}\n--- END RESULT ---", res.success, res.value);
            }
            Err(e) => {
                eprintln!("process_request_with_plan error: {}", e);
            }
        }
    }

    // Dump intents
    let intents = ccos.list_intents_snapshot();
    let out = json!({ "intents": intents });
    println!("{}", serde_json::to_string_pretty(&out)?);

    Ok(())
}
