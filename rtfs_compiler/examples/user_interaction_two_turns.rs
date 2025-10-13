// Minimal example: parse `--config <path>` and `--profile <name>`,
// apply profile -> env, then initialize CCOS with AgentConfig when present.
use clap::Parser;
use std::sync::Arc;
use std::fs;
use std::path::Path;

use serde_json;
use toml;

use rtfs_compiler::ccos::{CCOS, IntentGraphConfig};
use rtfs_compiler::config::types::{AgentConfig, LlmProfile};

#[derive(Parser, Debug)]
/// Minimal demo with optional config loading (`--config <path>`)
struct Args {
    /// Path to AgentConfig (TOML or JSON)
    #[arg(long)]
    config: Option<String>,

    /// Optional LLM profile name to select from config
    #[arg(long)]
    profile: Option<String>,
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

fn apply_profile_env(p: &LlmProfile) {
    // Minimal mapping for demo purposes
    std::env::set_var("CCOS_DELEGATING_MODEL", &p.model);
    std::env::set_var("CCOS_LLM_PROVIDER_HINT", &p.provider);
    if let Some(url) = &p.base_url {
        std::env::set_var("CCOS_LLM_BASE_URL", url);
    }
    // Prefer inline api_key, otherwise try named env var from the profile
    if let Some(inline) = &p.api_key {
        std::env::set_var("OPENAI_API_KEY", inline);
    } else if let Some(env_key) = &p.api_key_env {
        if let Ok(v) = std::env::var(env_key) {
            std::env::set_var("OPENAI_API_KEY", v);
        }
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args = Args::parse();

    let mut agent_config: Option<AgentConfig> = None;
    if let Some(cfg_path) = &args.config {
        match load_agent_config(cfg_path) {
            Ok(cfg) => {
                if let Some(profile_name) = &args.profile {
                    if let Some(profiles) = &cfg.llm_profiles {
                        if let Some(p) = profiles.profiles.iter().find(|pp| pp.name == *profile_name) {
                            apply_profile_env(p);
                        }
                    }
                }
                agent_config = Some(cfg);
            }
            Err(e) => eprintln!("failed to load agent config {}: {}", cfg_path, e),
        }
    }

    let ccos = if let Some(cfg) = agent_config.clone() {
        Arc::new(
            CCOS::new_with_agent_config_and_configs_and_debug_callback(
                IntentGraphConfig::default(),
                None,
                Some(cfg),
                None,
            )
            .await?,
        )
    } else {
        Arc::new(CCOS::new_with_debug_callback(None).await?)
    };

    println!(
        "delegating arbiter available = {}",
        ccos.get_delegating_arbiter().is_some()
    );

    Ok(())
}
// Minimal example: parse `--config <path>` and `--profile <name>`,
// apply profile -> env, then initialize CCOS with AgentConfig when present.
use clap::Parser;
use std::sync::Arc;
use std::fs;
use std::path::Path;

use serde_json;
use toml;

use rtfs_compiler::ccos::{CCOS, IntentGraphConfig};
use rtfs_compiler::config::types::{AgentConfig, LlmProfile};

#[derive(Parser, Debug)]
/// Minimal demo with optional config loading (`--config <path>`)
struct Args {
    /// Path to AgentConfig (TOML or JSON)
    #[arg(long)]
    config: Option<String>,

    /// Optional LLM profile name to select from config
    #[arg(long)]
    profile: Option<String>,
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

fn apply_profile_env(p: &LlmProfile) {
    // Minimal mapping for demo purposes
    std::env::set_var("CCOS_DELEGATING_MODEL", &p.model);
    std::env::set_var("CCOS_LLM_PROVIDER_HINT", &p.provider);
    if let Some(url) = &p.base_url {
        std::env::set_var("CCOS_LLM_BASE_URL", url);
    }
    // Prefer inline api_key, otherwise try named env var from the profile
    if let Some(inline) = &p.api_key {
        std::env::set_var("OPENAI_API_KEY", inline);
    } else if let Some(env_key) = &p.api_key_env {
        if let Ok(v) = std::env::var(env_key) {
            std::env::set_var("OPENAI_API_KEY", v);
        }
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args = Args::parse();

    let mut agent_config: Option<AgentConfig> = None;
    if let Some(cfg_path) = &args.config {
        match load_agent_config(cfg_path) {
            Ok(cfg) => {
                if let Some(profile_name) = &args.profile {
                    if let Some(profiles) = &cfg.llm_profiles {
                        if let Some(p) = profiles.profiles.iter().find(|pp| pp.name == *profile_name) {
                            apply_profile_env(p);
                        }
                    }
                }
                agent_config = Some(cfg);
            }
            Err(e) => eprintln!("failed to load agent config {}: {}", cfg_path, e),
        }
    }

    let ccos = if let Some(cfg) = agent_config.clone() {
        Arc::new(
            CCOS::new_with_agent_config_and_configs_and_debug_callback(
                IntentGraphConfig::default(),
                None,
                Some(cfg),
                None,
            )
            .await?,
        )
    } else {
        Arc::new(CCOS::new_with_debug_callback(None).await?)
    };

    println!(
        "delegating arbiter available = {}",
        ccos.get_delegating_arbiter().is_some()
    );

    Ok(())
}
// Minimal example: parse `--config <path>` and `--profile <name>`,
// apply profile -> env, then initialize CCOS with AgentConfig when present.

use clap::Parser;
use std::sync::Arc;
use std::fs;
use std::path::Path;
use serde_json;
use toml;

use rtfs_compiler::ccos::{CCOS, IntentGraphConfig};
use rtfs_compiler::config::types::{AgentConfig, LlmProfile};

#[derive(Parser, Debug)]
/// Minimal demo with optional config loading (`--config <path>`)
struct Args {
    /// Path to AgentConfig (TOML or JSON)
    #[arg(long)]
    config: Option<String>,

    /// Optional LLM profile name to select from config
    #[arg(long)]
    profile: Option<String>,
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

fn apply_profile_env(p: &LlmProfile) {
    // Minimal mapping for demo purposes
    std::env::set_var("CCOS_DELEGATING_MODEL", &p.model);
    std::env::set_var("CCOS_LLM_PROVIDER_HINT", &p.provider);
    if let Some(url) = &p.base_url {
        std::env::set_var("CCOS_LLM_BASE_URL", url);
    }
    // Prefer inline api_key, otherwise try named env var from the profile
    if let Some(inline) = &p.api_key {
        std::env::set_var("OPENAI_API_KEY", inline);
    } else if let Some(env_key) = &p.api_key_env {
        if let Ok(v) = std::env::var(env_key) {
            std::env::set_var("OPENAI_API_KEY", v);
        }
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args = Args::parse();

    let mut agent_config: Option<AgentConfig> = None;
    if let Some(cfg_path) = &args.config {
        match load_agent_config(cfg_path) {
            Ok(cfg) => {
                if let Some(profile_name) = &args.profile {
                    if let Some(profiles) = &cfg.llm_profiles {
                        if let Some(p) = profiles.profiles.iter().find(|pp| pp.name == *profile_name) {
                            apply_profile_env(p);
                        }
                    }
                }
                agent_config = Some(cfg);
            Err(e) => eprintln!("failed to load agent config {}: {}", cfg_path, e),
        }
    }

    let ccos = if let Some(cfg) = agent_config.clone() {
        Arc::new(
            CCOS::new_with_agent_config_and_configs_and_debug_callback(
                IntentGraphConfig::default(),
                None,
                Some(cfg),
                None,
            )
            .await?,
        )
    } else {
        Arc::new(CCOS::new_with_debug_callback(None).await?)
    };

    println!(
        "delegating arbiter available = {}",
        ccos.get_delegating_arbiter().is_some()
    );

    Ok(())
}
// Minimal example: parse `--config <path>` and `--profile <name>`,
// apply profile -> env, then initialize CCOS with AgentConfig when present.
use clap::Parser;
use std::sync::Arc;
use std::fs;
use std::path::Path;

use serde_json;
use toml;

use rtfs_compiler::ccos::{CCOS, IntentGraphConfig};
use rtfs_compiler::config::types::{AgentConfig, LlmProfile};

#[derive(Parser, Debug)]
/// Minimal demo with optional config loading (`--config <path>`)
struct Args {
    /// Path to AgentConfig (TOML or JSON)
    #[arg(long)]
    config: Option<String>,

    /// Optional LLM profile name to select from config
    #[arg(long)]
    profile: Option<String>,
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

fn apply_profile_env(p: &LlmProfile) {
    // Minimal mapping for demo purposes
    std::env::set_var("CCOS_DELEGATING_MODEL", &p.model);
    std::env::set_var("CCOS_LLM_PROVIDER_HINT", &p.provider);
    if let Some(url) = &p.base_url {
        std::env::set_var("CCOS_LLM_BASE_URL", url);
    }
    // Prefer inline api_key, otherwise try named env var from the profile
    if let Some(inline) = &p.api_key {
        std::env::set_var("OPENAI_API_KEY", inline);
    } else if let Some(env_key) = &p.api_key_env {
        if let Ok(v) = std::env::var(env_key) {
            std::env::set_var("OPENAI_API_KEY", v);
        }
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args = Args::parse();

    let mut agent_config: Option<AgentConfig> = None;
    if let Some(cfg_path) = &args.config {
        match load_agent_config(cfg_path) {
            Ok(cfg) => {
                if let Some(profile_name) = &args.profile {
                    if let Some(profiles) = &cfg.llm_profiles {
                        if let Some(p) = profiles.profiles.iter().find(|pp| pp.name == *profile_name) {
                            apply_profile_env(p);
                        }
                    }
                }
                agent_config = Some(cfg);
            }
            Err(e) => eprintln!("failed to load agent config {}: {}", cfg_path, e),
        }
    }

    let ccos = if let Some(cfg) = agent_config.clone() {
        Arc::new(
            CCOS::new_with_agent_config_and_configs_and_debug_callback(
                IntentGraphConfig::default(),
                None,
                Some(cfg),
                None,
            )
            .await?,
        )
    } else {
        Arc::new(CCOS::new_with_debug_callback(None).await?)
    };

    println!(
        "delegating arbiter available = {}",
        ccos.get_delegating_arbiter().is_some()
    );

    Ok(())
}
// Minimal, clean example that parses `--config` and `--profile`, applies a profile,
// and initializes CCOS with an AgentConfig when provided.
use clap::Parser;
use std::sync::Arc;
use std::fs;
use std::path::Path;

use serde_json;
use toml;

use rtfs_compiler::ccos::{CCOS, IntentGraphConfig};
use rtfs_compiler::config::types::{AgentConfig, LlmProfile};

#[derive(Parser, Debug)]
struct Args {
    /// Path to AgentConfig (TOML or JSON)
    #[arg(long)]
    config: Option<String>,

    /// Optional LLM profile name to select from config
    #[arg(long)]
    profile: Option<String>,
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

fn apply_profile_env(p: &LlmProfile) {
    // Minimal mapping for demo; real projects may dispatch provider-specific keys
    std::env::set_var("CCOS_DELEGATING_MODEL", &p.model);
    std::env::set_var("CCOS_LLM_PROVIDER_HINT", &p.provider);
    if let Some(key) = &p.api_key {
        std::env::set_var("OPENAI_API_KEY", key);
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args = Args::parse();

    let mut agent_config: Option<AgentConfig> = None;
    if let Some(cfg_path) = &args.config {
        match load_agent_config(cfg_path) {
            Ok(cfg) => {
                if let Some(profile_name) = &args.profile {
                    if let Some(profiles) = &cfg.llm_profiles {
                        if let Some(p) = profiles.profiles.iter().find(|pp| pp.name == *profile_name) {
                            apply_profile_env(p);
                        }
                    }
                }
                agent_config = Some(cfg);
            }
            Err(e) => eprintln!("failed to load agent config {}: {}", cfg_path, e),
        }
    }

    let ccos = if let Some(cfg) = agent_config.clone() {
        Arc::new(
            CCOS::new_with_agent_config_and_configs_and_debug_callback(
                IntentGraphConfig::default(),
                None,
                Some(cfg),
                None,
            )
            .await?,
        )
    } else {
        Arc::new(CCOS::new_with_debug_callback(None).await?)
    };

    println!(
        "delegating arbiter available = {}",
        ccos.get_delegating_arbiter().is_some()
    );

    Ok(())
}
// Minimal example: demonstrate `--config <path>` and `--profile <name>`
use clap::Parser;
use std::sync::Arc;
use std::fs;
use std::path::Path;

use rtfs_compiler::ccos::{CCOS, IntentGraphConfig};
use rtfs_compiler::config::types::{AgentConfig, LlmProfile};

#[derive(Parser, Debug)]
/// Minimal demo with optional config loading (`--config <path>`)
struct Args {
    /// Path to AgentConfig (TOML or JSON)
    #[arg(long)]
    config: Option<String>,

    /// Optional LLM profile name to select from config
    #[arg(long)]
    profile: Option<String>,
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

fn apply_profile_env(p: &LlmProfile) {
    std::env::set_var("CCOS_DELEGATING_MODEL", &p.model);
    std::env::set_var("CCOS_LLM_PROVIDER_HINT", &p.provider);
    if let Some(url) = &p.base_url {
        std::env::set_var("CCOS_LLM_BASE_URL", url);
    }
    if let Some(inline) = &p.api_key {
        std::env::set_var("OPENAI_API_KEY", inline);
    } else if let Some(env_key) = &p.api_key_env {
        if let Ok(v) = std::env::var(env_key) {
            std::env::set_var("OPENAI_API_KEY", v);
        }
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args = Args::parse();

    let mut agent_config: Option<AgentConfig> = None;
    if let Some(cfg_path) = &args.config {
        match load_agent_config(cfg_path) {
            Ok(cfg) => {
                if let Some(profile_name) = &args.profile {
                    if let Some(profiles) = &cfg.llm_profiles {
                        if let Some(p) = profiles.profiles.iter().find(|pp| pp.name == *profile_name) {
                            apply_profile_env(p);
                        }
                    }
                }
                agent_config = Some(cfg);
            }
            Err(e) => eprintln!("failed to load agent config {}: {}", cfg_path, e),
        }
    }

    let ccos = if let Some(cfg) = agent_config.clone() {
        Arc::new(
            CCOS::new_with_agent_config_and_configs_and_debug_callback(
                IntentGraphConfig::default(),
                None,
                Some(cfg),
                None,
            )
            .await?,
        )
    } else {
        Arc::new(CCOS::new_with_debug_callback(None).await?)
    };

    if ccos.get_delegating_arbiter().is_some() {
        println!("Delegating arbiter is available (config applied).");
    } else {
        println!("Delegating arbiter is NOT available.");
    }

    Ok(())
}
// Compact example: only enough to demonstrate --config and --profile support
use clap::Parser;
use std::sync::Arc;
use std::fs;
use std::path::Path;

use rtfs_compiler::ccos::{CCOS, IntentGraphConfig};
use rtfs_compiler::config::types::{AgentConfig, LlmProfile};

#[derive(Parser, Debug)]
/// Minimal two-turn demo with optional config loading (`--config <path>`)
struct Args {
    /// Path to AgentConfig (TOML or JSON)
    #[arg(long)]
    config: Option<String>,

    /// Optional LLM profile name to select from config
    #[arg(long)]
    profile: Option<String>,
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

fn apply_profile_env(p: &LlmProfile) {
    // Map profile fields to environment variables used by CCOS
    std::env::set_var("CCOS_DELEGATING_MODEL", &p.model);
    std::env::set_var("CCOS_LLM_PROVIDER_HINT", &p.provider);
    if let Some(url) = &p.base_url {
        std::env::set_var("CCOS_LLM_BASE_URL", url);
    }
    if let Some(inline) = &p.api_key {
        // place inline key into OPENAI_API_KEY by default for demo; dispatch_key could be used
        std::env::set_var("OPENAI_API_KEY", inline);
    } else if let Some(env_key) = &p.api_key_env {
        if let Ok(v) = std::env::var(env_key) {
            std::env::set_var("OPENAI_API_KEY", v);
        }
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args = Args::parse();

    let mut agent_config: Option<AgentConfig> = None;
    if let Some(cfg_path) = &args.config {
        match load_agent_config(cfg_path) {
            Ok(cfg) => {
                if let Some(profile_name) = &args.profile {
                    if let Some(profiles) = &cfg.llm_profiles {
                        if let Some(p) = profiles.profiles.iter().find(|pp| pp.name == *profile_name) {
                            apply_profile_env(p);
                        }
                    }
                }
                agent_config = Some(cfg);
            }
            Err(e) => eprintln!("failed to load agent config {}: {}", cfg_path, e),
        }
    }

    let ccos = if let Some(cfg) = agent_config.clone() {
        Arc::new(
            CCOS::new_with_agent_config_and_configs_and_debug_callback(
                IntentGraphConfig::default(),
                None,
                Some(cfg),
                None,
            )
            .await?,
        )
    } else {
        Arc::new(CCOS::new_with_debug_callback(None).await?)
    };

    if ccos.get_delegating_arbiter().is_some() {
        println!("Delegating arbiter is available (config applied).");
    } else {
        println!("Delegating arbiter is NOT available.");
    }

    Ok(())
}
use std::sync::Arc;

use rtfs_compiler::ast::MapKey;
use rtfs_compiler::ccos::synthesis::{self, synthesize_capabilities, InteractionTurn};
use rtfs_compiler::ccos::types::{Plan, StorableIntent};
use rtfs_compiler::ccos::CCOS;
use rtfs_compiler::runtime::{RuntimeContext, RuntimeResult, Value};

const FIRST_PROMPT: &str = "Describe the calendar event you need help with";
const SECOND_PROMPT: &str = "List any constraints or preferences";

#[derive(Debug, Clone)]
struct ConversationRecord {
    prompt: String,
    answer: String,
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("CCOS two-turn interactive demo (set CCOS_INTERACTIVE_ASK=1 for live prompts).");

    let ccos = Arc::new(CCOS::new_with_debug_callback(None).await?);
    let runtime_context = RuntimeContext::controlled(vec!["ccos.user.ask".to_string()]);

    let plan_body = build_two_turn_plan(FIRST_PROMPT, SECOND_PROMPT);
    println!("\n--- Hardcoded Plan ---\n{}\n", plan_body);

    let intent_id = "intent.two_turn.demo".to_string();
    let mut storable_intent =
        StorableIntent::new("Collect calendar details via two prompts".to_string());
    storable_intent.intent_id = intent_id.clone();
    storable_intent.name = Some("two-turn-demo".to_string());

    if let Ok(mut graph) = ccos.get_intent_graph().lock() {
        graph.store_intent(storable_intent)?;
    } else {
        return Err("Failed to lock intent graph".into());
    }

    let plan = Plan::new_rtfs(plan_body.clone(), vec![intent_id]);

    let execution = ccos
        .validate_and_execute_plan(plan, &runtime_context)
        .await?;

    println!("Execution value: {}", execution.value);

    let conversation = extract_conversation(&execution.value);
    if conversation.is_empty() {
        println!("No conversation captured; confirm CCOS_INTERACTIVE_ASK=1 when running.");
        return Ok(());
    }

use clap::Parser;
use rtfs_compiler::config::parser::AgentConfigParser;
use rtfs_compiler::config::types::{AgentConfig, LlmProfile};
use std::fs;
use std::path::Path;
    println!("\nConversation transcript:");
    for (idx, turn) in conversation.iter().enumerate() {
        println!("- Turn {} prompt: {}", idx + 1, turn.prompt);
        println!("  Answer: {}", turn.answer);
#[derive(Parser, Debug)]
/// Simple two-turn demo with optional config loading (`--config <path>`)
struct Args {
    /// Path to AgentConfig (TOML or JSON)
    #[arg(long)]
    config: Option<String>,
// Re-create clean file body below
// (Overwritten earlier; now provide correct imports and functions)


// Clean single implementation for the example
use clap::Parser;
use std::sync::Arc;
use std::fs;
use std::path::Path;
use serde_json;
use toml;

use rtfs_compiler::ast::MapKey;
use rtfs_compiler::ccos::synthesis::{self, synthesize_capabilities, InteractionTurn};
use rtfs_compiler::ccos::types::{Plan, StorableIntent};
use rtfs_compiler::ccos::{CCOS, IntentGraphConfig};
use rtfs_compiler::config::types::{AgentConfig, LlmProfile};
use rtfs_compiler::runtime::{RuntimeContext, RuntimeResult, Value};

const FIRST_PROMPT: &str = "Describe the calendar event you need help with";
const SECOND_PROMPT: &str = "List any constraints or preferences";

#[derive(Debug, Clone)]
struct ConversationRecord {
    prompt: String,
    answer: String,
}

#[derive(Parser, Debug)]
/// Simple two-turn demo with optional config loading (`--config <path>`)
struct Args {
    /// Path to AgentConfig (TOML or JSON)
    #[arg(long)]
    config: Option<String>,

    /// Optional LLM profile name to select from config
    #[arg(long)]
    profile: Option<String>,
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("CCOS two-turn interactive demo (set CCOS_INTERACTIVE_ASK=1 for live prompts).\n");

    let args = Args::parse();

    // Load agent config if provided
    let mut loaded_agent_config: Option<AgentConfig> = None;
    if let Some(cfg_path) = &args.config {
        match load_agent_config(cfg_path) {
            Ok(cfg) => {
                // apply profile env if requested
                if let Some(profile_name) = &args.profile {
                    if let Some(pp) = &cfg.llm_profiles {
                        if let Some(p) = pp.profiles.iter().find(|pr| pr.name == *profile_name) {
                            apply_profile_env(p);
                        }
                    }
                }
                loaded_agent_config = Some(cfg);
            }
            Err(e) => eprintln!("Failed to load agent config {}: {}", cfg_path, e),
        }
    }

    // Initialize CCOS with optional agent config
    let ccos = if let Some(cfg) = loaded_agent_config.clone() {
        Arc::new(
            CCOS::new_with_agent_config_and_configs_and_debug_callback(
                IntentGraphConfig::default(),
                None,
                Some(cfg),
                None,
            )
            .await?,
        )
    } else {
        Arc::new(CCOS::new_with_debug_callback(None).await?)
    };

    let runtime_context = RuntimeContext::controlled(vec!["ccos.user.ask".to_string()]);

    let plan_body = build_two_turn_plan(FIRST_PROMPT, SECOND_PROMPT);
    println!("\n--- Hardcoded Plan ---\n{}\n", plan_body);

    let intent_id = "intent.two_turn.demo".to_string();
    let mut storable_intent = StorableIntent::new("Collect calendar details via two prompts".to_string());
    storable_intent.intent_id = intent_id.clone();
    storable_intent.name = Some("two-turn-demo".to_string());

    if let Ok(mut graph) = ccos.get_intent_graph().lock() {
        graph.store_intent(storable_intent)?;
    } else {
        return Err("Failed to lock intent graph".into());
    }

    let plan = Plan::new_rtfs(plan_body.clone(), vec![intent_id]);

    let execution = ccos
        .validate_and_execute_plan(plan, &runtime_context)
        .await?;

    println!("Execution value: {}", execution.value);

    let conversation = extract_conversation(&execution.value);
    if conversation.is_empty() {
        println!("No conversation captured; confirm CCOS_INTERACTIVE_ASK=1 when running.");
        return Ok(());
    }

    println!("\nConversation transcript:");
    for (idx, turn) in conversation.iter().enumerate() {
        println!("- Turn {} prompt: {}", idx + 1, turn.prompt);
        println!("  Answer: {}", turn.answer);
    }

    let synthesis_input: Vec<InteractionTurn> = conversation
        .iter()
        .enumerate()
        .map(|(idx, record)| InteractionTurn {
            turn_index: idx,
            prompt: record.prompt.clone(),
            answer: Some(record.answer.clone()),
        })
        .collect();

    let synthesis = synthesize_capabilities(&synthesis_input);
    println!(
        "\nSynthesis metrics: turns={} coverage={:.2}",
        synthesis.metrics.turns_total, synthesis.metrics.coverage
    );
    if !synthesis.metrics.missing_required.is_empty() {
        println!("Missing answers for: {:?}", synthesis.metrics.missing_required);
    }

    if let Some(collector_src) = synthesis.collector {
        println!("\n--- Synthesized Collector ---\n{}", collector_src);
        println!("Registration skipped in demo: synthesized capability not registered.");
    }

    if let Some(planner_src) = synthesis.planner {
        println!("\n--- Synthesized Planner ---\n{}", planner_src);
    }

    if let Some(stub_src) = synthesis.stub {
        println!("\n--- Synthesized Agent Stub ---\n{}", stub_src);
    }

    if let Some(arbiter) = ccos.get_delegating_arbiter() {
        let schema = synthesis::schema_builder::extract_param_schema(&synthesis_input);
        let domain = "two_turn.demo";
        match arbiter
            .synthesize_capability_from_collector(&schema, &synthesis_input, domain)
            .await
        {
            Ok(rtfs_block) => println!("\n--- Delegating Arbiter Capability & Plan ---\n{}\n", rtfs_block),
            Err(err) => println!("\nDelegating arbiter synthesis failed: {}", err),
        }
    } else {
        println!("\nDelegating arbiter not available in this configuration.");
    }

    Ok(())
}

fn build_two_turn_plan(first_prompt: &str, second_prompt: &str) -> String {
    let first = escape_quotes(first_prompt);
    let second = escape_quotes(second_prompt);
    format!(
        "(do\n  (let [first (call :ccos.user.ask \"{first}\")\n        second (call :ccos.user.ask \"{second}\")]\n    {{:status \"completed\"\n     :conversation [{{:prompt \"{first}\" :answer first}}\n                    {{:prompt \"{second}\" :answer second}}]}}))"
    )
}

fn escape_quotes(text: &str) -> String {
    text.replace('"', "\\\"")
}

fn extract_conversation(value: &Value) -> Vec<ConversationRecord> {
    match value {
        Value::Map(map) => conversation_entries(map),
        _ => Vec::new(),
    }
}

fn conversation_entries(map: &std::collections::HashMap<MapKey, Value>) -> Vec<ConversationRecord> {
    let mut records = Vec::new();
    if let Some(value) = map_get_keyword(map, "conversation") {
        match value {
            Value::Vector(items) | Value::List(items) => {
                for item in items {
                    if let Value::Map(entry_map) = item {
                        if let (Some(prompt), Some(answer)) = (
                            map_get_keyword(entry_map, "prompt"),
                            map_get_keyword(entry_map, "answer"),
                        ) {
                            if let (Value::String(p), Value::String(a)) = (prompt, answer) {
                                records.push(ConversationRecord {
                                    prompt: p.clone(),
                                    answer: a.clone(),
                                });
                            }
                        }
                    }
                }
            }
            _ => {}
        }
    }
    records
}

fn map_get_keyword<'a>(
    map: &'a std::collections::HashMap<MapKey, Value>,
    key: &str,
) -> Option<&'a Value> {
    map.iter().find_map(|(k, v)| match k {
        MapKey::Keyword(keyword) if keyword.0 == key => Some(v),
        MapKey::String(s) if s == key => Some(v),
        _ => None,
    })
}

fn extract_capability_id(rtfs_source: &str) -> Option<String> {
    rtfs_source.lines().find_map(|line| {
        let trimmed = line.trim();
        if trimmed.starts_with("(capability ") {
            trimmed
                .split_whitespace()
                .nth(1)
                .map(|token| token.trim_matches(|c| c == '(' || c == ')'))
                .map(str::to_string)
        } else {
            None
        }
    })
}

async fn register_placeholder_capability(
    ccos: &Arc<CCOS>,
    capability_id: &str,
) -> RuntimeResult<()> {
    println!(
        "Registering synthesized capability `{}` with placeholder handler",
        capability_id
    );

    let marketplace = ccos.get_capability_marketplace();
    let capability_id_owned = capability_id.to_string();
    let handler_id = capability_id_owned.clone();
    let handler = Arc::new(move |input: &Value| -> RuntimeResult<Value> {
        println!(
            "[synth] capability `{}` invoked with input: {}",
            handler_id, input
        );
        Ok(Value::String("synth placeholder response".into()))
    });

    marketplace
        .register_local_capability(
            capability_id_owned.clone(),
            format!("Synthesized capability {}", capability_id_owned),
            "Placeholder handler generated during demo".to_string(),
            handler,
        )
        .await?;

    println!(
        "Capability `{}` registered successfully.",
        capability_id_owned
    );
    Ok(())
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

fn apply_profile_env(p: &LlmProfile) {
    std::env::set_var("CCOS_DELEGATING_MODEL", &p.model);
    std::env::set_var("CCOS_LLM_PROVIDER_HINT", &p.provider);
    if let Some(url) = &p.base_url {
        std::env::set_var("CCOS_LLM_BASE_URL", url);
    } else if p.provider == "openrouter" {
        if std::env::var("CCOS_LLM_BASE_URL").is_err() {
            std::env::set_var("CCOS_LLM_BASE_URL", "https://openrouter.ai/api/v1");
        }
    }
    if let Some(inline) = &p.api_key {
        dispatch_key(&p.provider, inline);
    } else if let Some(env_key) = &p.api_key_env {
        if let Ok(v) = std::env::var(env_key) {
            dispatch_key(&p.provider, &v);
        }
    }
    match p.provider.as_str() {
        "openai" => std::env::set_var("CCOS_LLM_PROVIDER", "openai"),
        "claude" | "anthropic" => std::env::set_var("CCOS_LLM_PROVIDER", "anthropic"),
        "stub" => std::env::set_var("CCOS_LLM_PROVIDER", "stub"),
        "local" => std::env::set_var("CCOS_LLM_PROVIDER", "local"),
        _ => {}
    }
    std::env::set_var("CCOS_LLM_MODEL", &p.model);
}

fn dispatch_key(provider: &str, key: &str) {
    match provider {
        "openrouter" => std::env::set_var("OPENROUTER_API_KEY", key),
        "claude" | "anthropic" => std::env::set_var("ANTHROPIC_API_KEY", key),
        "gemini" => std::env::set_var("GEMINI_API_KEY", key),
        "stub" => {},
        _ => std::env::set_var("OPENAI_API_KEY", key),
    }
}


            Ok(())
        }

        fn build_two_turn_plan(first_prompt: &str, second_prompt: &str) -> String {
            let first = escape_quotes(first_prompt);
            let second = escape_quotes(second_prompt);
            format!(
            "(do\n  (let [first (call :ccos.user.ask \"{first}\")\n        second (call :ccos.user.ask \"{second}\")]\n    {{:status \"completed\"\n     :conversation [{{:prompt \"{first}\" :answer first}}\n                    {{:prompt \"{second}\" :answer second}}]}}))"
            )
        }

        fn escape_quotes(text: &str) -> String {
            text.replace('"', "\\\"")
        }

        fn extract_conversation(value: &Value) -> Vec<ConversationRecord> {
            match value {
                Value::Map(map) => conversation_entries(map),
                _ => Vec::new(),
            }
        }

        fn conversation_entries(map: &std::collections::HashMap<MapKey, Value>) -> Vec<ConversationRecord> {
            let mut records = Vec::new();
            if let Some(value) = map_get_keyword(map, "conversation") {
                match value {
                    Value::Vector(items) | Value::List(items) => {
                        for item in items {
                            if let Value::Map(entry_map) = item {
                                if let (Some(prompt), Some(answer)) = (
                                    map_get_keyword(entry_map, "prompt"),
                                    map_get_keyword(entry_map, "answer"),
                                ) {
                                    if let (Value::String(p), Value::String(a)) = (prompt, answer) {
                                        records.push(ConversationRecord {
                                            prompt: p.clone(),
                                            answer: a.clone(),
                                        });
                                    }
                                }
                            }
                        }
                    }
                    _ => {}
                }
            }
            records
        }

        fn map_get_keyword<'a>(
            map: &'a std::collections::HashMap<MapKey, Value>,
            key: &str,
        ) -> Option<&'a Value> {
            map.iter().find_map(|(k, v)| match k {
                MapKey::Keyword(keyword) if keyword.0 == key => Some(v),
                MapKey::String(s) if s == key => Some(v),
                _ => None,
            })
        }

        fn extract_capability_id(rtfs_source: &str) -> Option<String> {
            rtfs_source.lines().find_map(|line| {
                let trimmed = line.trim();
                if trimmed.starts_with("(capability ") {
                    trimmed
                        .split_whitespace()
                        .nth(1)
                        .map(|token| token.trim_matches(|c| c == '(' || c == ')'))
                        .map(str::to_string)
                } else {
                    None
                }
            })
        }

        async fn register_placeholder_capability(
            ccos: &Arc<CCOS>,
            capability_id: &str,
        ) -> RuntimeResult<()> {
            println!(
                "Registering synthesized capability `{}` with placeholder handler",
                capability_id
            );

            let marketplace = ccos.get_capability_marketplace();
            let capability_id_owned = capability_id.to_string();
            let handler_id = capability_id_owned.clone();
            let handler = Arc::new(move |input: &Value| -> RuntimeResult<Value> {
                println!(
                    "[synth] capability `{}` invoked with input: {}",
                    handler_id, input
                );
                Ok(Value::String("synth placeholder response".into()))
            });

            marketplace
                .register_local_capability(
                    capability_id_owned.clone(),
                    format!("Synthesized capability {}", capability_id_owned),
                    "Placeholder handler generated during demo".to_string(),
                    handler,
                )
                .await?;

            println!(
                "Capability `{}` registered successfully.",
                capability_id_owned
            );
            Ok(())
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

        fn apply_profile_env(p: &LlmProfile) {
            std::env::set_var("CCOS_DELEGATING_MODEL", &p.model);
            std::env::set_var("CCOS_LLM_PROVIDER_HINT", &p.provider);
            if let Some(url) = &p.base_url {
                std::env::set_var("CCOS_LLM_BASE_URL", url);
            } else if p.provider == "openrouter" {
                if std::env::var("CCOS_LLM_BASE_URL").is_err() {
                    std::env::set_var("CCOS_LLM_BASE_URL", "https://openrouter.ai/api/v1");
                }
            }
            if let Some(inline) = &p.api_key {
                dispatch_key(&p.provider, inline);
            } else if let Some(env_key) = &p.api_key_env {
                if let Ok(v) = std::env::var(env_key) {
                    dispatch_key(&p.provider, &v);
                }
            }
            match p.provider.as_str() {
                "openai" => std::env::set_var("CCOS_LLM_PROVIDER", "openai"),
                "claude" | "anthropic" => std::env::set_var("CCOS_LLM_PROVIDER", "anthropic"),
                "stub" => std::env::set_var("CCOS_LLM_PROVIDER", "stub"),
                "local" => std::env::set_var("CCOS_LLM_PROVIDER", "local"),
                _ => {}
            }
            std::env::set_var("CCOS_LLM_MODEL", &p.model);
        }

        fn dispatch_key(provider: &str, key: &str) {
            match provider {
                "openrouter" => std::env::set_var("OPENROUTER_API_KEY", key),
                "claude" | "anthropic" => std::env::set_var("ANTHROPIC_API_KEY", key),
                "gemini" => std::env::set_var("GEMINI_API_KEY", key),
                "stub" => {},
                _ => std::env::set_var("OPENAI_API_KEY", key),
            }
        }
