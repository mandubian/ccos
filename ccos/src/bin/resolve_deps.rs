//! CLI tool for resolving missing capability dependencies
//!
//! This tool implements advanced CLI commands for managing missing capability resolution,
//! monitoring, and observability as part of Phase 8 enhancements.

use ccos::arbiter::arbiter_config::{
    AgentRegistryConfig as ArbiterAgentRegistryConfig, DelegationConfig as ArbiterDelegationConfig,
    LlmConfig as ArbiterLlmConfig, LlmProviderType, RetryConfig,
};
use ccos::arbiter::delegating_arbiter::DelegatingArbiter;
use ccos::capability_marketplace::types::{CapabilityManifest, LocalCapability, ProviderType};
use ccos::capability_marketplace::CapabilityMarketplace;
use ccos::checkpoint_archive::CheckpointArchive;
use ccos::intent_graph::IntentGraph;
use ccos::synthesis::feature_flags::MissingCapabilityConfig;
use ccos::synthesis::missing_capability_resolver::{MissingCapabilityResolver, ResolverConfig};
use clap::{Parser, Subcommand};
use rtfs::ast::{Keyword, MapKey};
use rtfs::config::profile_selection::expand_profiles;
use rtfs::config::types::{AgentConfig, LlmProfile};
use rtfs::runtime::values::Value;
use serde_json;
use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::Path;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::sync::RwLock;
use tokio::time::sleep;

#[derive(Parser, Debug)]
#[clap(author, version, about, long_about = None)]
#[clap(name = "resolve-deps")]
#[clap(about = "CCOS Missing Capability Resolution Tool")]
struct Args {
    /// Path to agent configuration (TOML or JSON)
    #[clap(short, long, default_value = "config/agent_config.toml")]
    config: String,

    /// Override the LLM profile declared in the agent configuration
    #[clap(short, long)]
    profile: Option<String>,

    /// Command to execute
    #[clap(subcommand)]
    command: Command,

    /// Enable verbose logging
    #[clap(short, long, global = true)]
    verbose: bool,
}

#[derive(Subcommand, Debug)]
enum Command {
    /// Resolve missing capability dependencies
    Resolve {
        /// Capability ID to resolve
        #[clap(short, long)]
        capability_id: String,
        /// Force resolution even if capability exists
        #[clap(short, long)]
        force: bool,
    },
    /// Resume execution after capability resolution
    Resume {
        /// Checkpoint ID to resume from
        #[clap(short = 'k', long)]
        checkpoint_id: String,

        /// Capability ID that was resolved
        #[clap(short = 'c', long)]
        capability_id: String,
    },
    /// List pending capabilities awaiting resolution
    ListPending {
        /// Filter by status (pending/in_progress/failed)
        #[clap(short, long)]
        filter: Option<String>,
    },
    /// Show resolution statistics and metrics
    Stats {
        /// Time range for statistics (e.g., '1h', '1d', '7d')
        #[clap(short, long)]
        time_range: Option<String>,
    },
    /// Monitor resolution queue in real-time
    Monitor {
        /// Update interval in seconds
        #[clap(short, long, default_value = "5")]
        interval: u64,
        /// Run continuously until interrupted
        #[clap(short, long)]
        continuous: bool,
    },
    /// Validate a capability before registration
    Validate {
        /// Capability ID to validate
        #[clap(short, long)]
        capability_id: String,
        /// Security validation level (low/medium/high)
        #[clap(short, long)]
        security_level: Option<String>,
    },
    /// Search for capabilities across all sources
    Search {
        /// Search query
        #[clap(short, long)]
        query: String,
        /// Search source (mcp/registry/local/all)
        #[clap(short, long, default_value = "all")]
        source: String,
        /// Maximum number of results
        #[clap(short, long, default_value = "10")]
        limit: usize,
    },
    /// Export resolution data for analysis
    Export {
        /// Output format (json/csv/yaml)
        #[clap(short, long, default_value = "json")]
        format: String,
        /// Output file path
        #[clap(short, long)]
        output: Option<String>,
    },
    /// Show detailed capability information
    Info {
        /// Capability ID to inspect
        #[clap(short, long)]
        capability_id: String,
    },
    /// Clean up old resolution data
    Cleanup {
        /// Days of data to keep (default: 30)
        #[clap(short, long, default_value = "30")]
        days: u32,
        /// Dry run (don't actually delete)
        #[clap(long)]
        dry_run: bool,
    },
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args = Args::parse();

    // Load agent configuration + select profile so downstream components share CCOS defaults
    let agent_config = load_agent_config(&args.config)?;
    apply_llm_profile(&agent_config, args.profile.as_deref())?;

    // Initialize core runtime structures shared with delegation + synthesis
    let registry = Arc::new(RwLock::new(
        rtfs::runtime::capabilities::registry::CapabilityRegistry::new(),
    ));
    let marketplace = Arc::new(CapabilityMarketplace::new(registry));
    let intent_graph = Arc::new(Mutex::new(IntentGraph::new()?));

    // Bootstrap the marketplace with some test capabilities
    bootstrap_test_capabilities(&marketplace).await?;

    let feature_config = MissingCapabilityConfig::from_agent_config(Some(&agent_config));

    let resolver_feature_config = match feature_config.validate() {
        Ok(_) => feature_config,
        Err(err) => {
            eprintln!(
                "⚠️  Missing capability configuration invalid: {}. Falling back to env defaults.",
                err
            );
            MissingCapabilityConfig::from_env()
        }
    };

    let resolver_config = ResolverConfig {
        max_attempts: resolver_feature_config.max_resolution_attempts,
        auto_resolve: resolver_feature_config.feature_flags.auto_resolution,
        verbose_logging: agent_config
            .missing_capabilities
            .verbose_logging
            .unwrap_or(args.verbose),
    };

    let checkpoint_archive = Arc::new(CheckpointArchive::new());

    let resolver = Arc::new(MissingCapabilityResolver::new(
        marketplace.clone(),
        checkpoint_archive,
        resolver_config,
        resolver_feature_config,
    ));

    if let Some(delegating) =
        maybe_create_delegating_arbiter(Arc::clone(&marketplace), Arc::clone(&intent_graph)).await
    {
        resolver.set_delegating_arbiter(Some(Arc::clone(&delegating)));
        println!("✅ Delegating arbiter configured for LLM synthesis.");
    }

    match args.command {
        Command::Resolve {
            capability_id,
            force,
        } => {
            handle_resolve(&resolver, &marketplace, &capability_id, force).await?;
        }
        Command::Resume {
            checkpoint_id,
            capability_id,
        } => {
            handle_resume(&resolver, &checkpoint_id, &capability_id).await?;
        }
        Command::ListPending { filter } => {
            handle_list_pending(&resolver, filter.as_deref()).await?;
        }
        Command::Stats { time_range } => {
            handle_stats(&resolver, time_range.as_deref()).await?;
        }
        Command::Monitor {
            interval,
            continuous,
        } => {
            handle_monitor(&resolver, interval, continuous).await?;
        }
        Command::Validate {
            capability_id,
            security_level,
        } => {
            handle_validate(&resolver, &capability_id, security_level.as_deref()).await?;
        }
        Command::Search {
            query,
            source,
            limit,
        } => {
            handle_search(&resolver, &query, &source, limit).await?;
        }
        Command::Export { format, output } => {
            handle_export(&resolver, &format, output.as_deref()).await?;
        }
        Command::Info { capability_id } => {
            handle_info(&marketplace, &capability_id).await?;
        }
        Command::Cleanup { days, dry_run } => {
            handle_cleanup(&resolver, days, dry_run).await?;
        }
    }

    Ok(())
}

fn load_agent_config(path: &str) -> Result<AgentConfig, Box<dyn std::error::Error>> {
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
) -> Result<(), Box<dyn std::error::Error>> {
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

async fn maybe_create_delegating_arbiter(
    capability_marketplace: Arc<CapabilityMarketplace>,
    intent_graph: Arc<Mutex<IntentGraph>>,
) -> Option<Arc<DelegatingArbiter>> {
    if !is_delegation_enabled() {
        return None;
    }

    let model = match std::env::var("CCOS_DELEGATING_MODEL") {
        Ok(value) if !value.trim().is_empty() => value,
        _ => {
            eprintln!(
                "⚠️  Delegation enabled but CCOS_DELEGATING_MODEL is not set. Skipping delegating arbiter."
            );
            return None;
        }
    };

    let (api_key, base_url) = if let Ok(key) = std::env::var("OPENROUTER_API_KEY") {
        let base = std::env::var("CCOS_LLM_BASE_URL")
            .ok()
            .or_else(|| Some("https://openrouter.ai/api/v1".to_string()));
        (Some(key), base)
    } else if let Ok(key) = std::env::var("OPENAI_API_KEY") {
        (Some(key), None)
    } else if let Ok(key) = std::env::var("ANTHROPIC_API_KEY") {
        (Some(key), std::env::var("CCOS_LLM_BASE_URL").ok())
    } else {
        (None, std::env::var("CCOS_LLM_BASE_URL").ok())
    };

    let provider_hint = std::env::var("CCOS_LLM_PROVIDER_HINT").unwrap_or_default();
    let provider_type = if provider_hint.eq_ignore_ascii_case("stub")
        || matches!(
            model.as_str(),
            "stub-model" | "deterministic-stub-model" | "stub"
        ) {
        LlmProviderType::Stub
    } else if provider_hint.eq_ignore_ascii_case("anthropic") {
        LlmProviderType::Anthropic
    } else if provider_hint.eq_ignore_ascii_case("local") {
        LlmProviderType::Local
    } else {
        LlmProviderType::OpenAI
    };

    let mut retry_config = RetryConfig::default();
    if let Ok(v) = std::env::var("CCOS_LLM_RETRY_MAX_RETRIES") {
        if let Ok(n) = v.parse::<u32>() {
            retry_config.max_retries = n;
        }
    }
    if let Ok(v) = std::env::var("CCOS_LLM_RETRY_SEND_FEEDBACK") {
        retry_config.send_error_feedback = matches!(v.as_str(), "1" | "true" | "yes" | "on");
    }
    if let Ok(v) = std::env::var("CCOS_LLM_RETRY_SIMPLIFY_FINAL") {
        retry_config.simplify_on_final_attempt = matches!(v.as_str(), "1" | "true" | "yes" | "on");
    }
    if let Ok(v) = std::env::var("CCOS_LLM_RETRY_USE_STUB_FALLBACK") {
        retry_config.use_stub_fallback = matches!(v.as_str(), "1" | "true" | "yes" | "on");
    }

    let llm_config = ArbiterLlmConfig {
        provider_type,
        model,
        api_key,
        base_url,
        max_tokens: Some(1000),
        temperature: Some(0.7),
        timeout_seconds: Some(30),
        prompts: None,
        retry_config,
    };

    let delegation_config = ArbiterDelegationConfig {
        enabled: true,
        threshold: 0.65,
        max_candidates: 3,
        min_skill_hits: None,
        agent_registry: ArbiterAgentRegistryConfig::default(),
        adaptive_threshold: None,
        print_extracted_intent: Some(false),
        print_extracted_plan: Some(false),
    };

    match DelegatingArbiter::new(
        llm_config,
        delegation_config,
        capability_marketplace,
        intent_graph,
    )
    .await
    {
        Ok(arbiter) => Some(Arc::new(arbiter)),
        Err(err) => {
            eprintln!(
                "⚠️  Failed to initialise delegating arbiter (LLM synthesis will be skipped): {}",
                err
            );
            None
        }
    }
}

fn is_delegation_enabled() -> bool {
    std::env::var("CCOS_ENABLE_DELEGATION")
        .ok()
        .or_else(|| std::env::var("CCOS_USE_DELEGATING_ARBITER").ok())
        .map(|value| matches!(value.as_str(), "1" | "true" | "yes" | "on"))
        .unwrap_or(false)
}

async fn handle_resolve(
    resolver: &Arc<MissingCapabilityResolver>,
    marketplace: &Arc<CapabilityMarketplace>,
    capability_id: &str,
    force: bool,
) -> Result<(), Box<dyn std::error::Error>> {
    println!(
        "🔍 Resolving dependencies for capability: {}",
        capability_id
    );

    if force {
        println!("⚡ Force mode enabled - will attempt resolution even if capability exists");
    }

    let (arguments, context) = build_sample_invocation(capability_id, force);

    println!("📋 Adding missing capability to resolution queue...");
    println!(
        "🔍 DEBUG: Attempting to resolve capability '{}'",
        capability_id
    );

    if !arguments.is_empty() {
        println!("🧪 Sample invocation payload:");
        for (idx, value) in arguments.iter().enumerate() {
            println!("   Arg {} => {}", idx, value);
        }
    } else {
        println!("🧪 No sample arguments provided for this capability.");
    }

    // Check if capability already exists in marketplace and track initial state
    let existing_capabilities = marketplace.list_capabilities().await;
    let capability_ids: Vec<String> = existing_capabilities.iter().map(|c| c.id.clone()).collect();
    if capability_ids.contains(&capability_id.to_string()) {
        println!(
            "❌ ERROR: Capability '{}' already exists in marketplace!",
            capability_id
        );
        println!("📋 Available capabilities: {:?}", capability_ids);
        return Ok(());
    } else {
        println!(
            "✅ Capability '{}' is missing from marketplace - proceeding with resolution",
            capability_id
        );
    }

    let before_capability_ids: HashSet<String> = capability_ids.iter().cloned().collect();

    resolver.handle_missing_capability(capability_id.to_string(), arguments, context)?;

    println!("⚙️ Processing resolution queue...");
    resolver.process_queue().await?;

    // Get statistics
    let stats = resolver.get_stats();
    println!("📊 Resolution Statistics:");
    println!("   Pending: {}", stats.pending_count);
    println!("   In Progress: {}", stats.in_progress_count);
    println!("   Failed: {}", stats.failed_count);
    println!("   Success Rate: {:.1}%", calculate_success_rate(&stats));

    let post_resolution_capabilities = marketplace.list_capabilities().await;
    let mut reported_path = false;
    for capability in &post_resolution_capabilities {
        if !before_capability_ids.contains(&capability.id) {
            if let Some(storage_path) = capability.metadata.get("storage_path") {
                println!(
                    "📁 Capability '{}' stored at: {}",
                    capability.id, storage_path
                );
                reported_path = true;
            }
        }
    }

    if !reported_path {
        if let Some(resolved_capability) = post_resolution_capabilities
            .iter()
            .find(|cap| cap.id == capability_id)
        {
            if let Some(storage_path) = resolved_capability.metadata.get("storage_path") {
                println!("📁 Capability stored at: {}", storage_path);
            }
        }
    }

    println!("✅ Dependency resolution completed!");
    Ok(())
}

fn build_sample_invocation(
    capability_id: &str,
    force: bool,
) -> (Vec<Value>, HashMap<String, String>) {
    let mut context = HashMap::new();
    context.insert("plan_id".to_string(), "test_plan".to_string());
    context.insert("intent_id".to_string(), "test_intent".to_string());
    context.insert("force_resolution".to_string(), force.to_string());

    let mut arguments = Vec::new();

    match capability_id {
        "core.safe-div" => {
            context.insert(
                "scenario".to_string(),
                "Guard division by zero and return either {:value <number>} or {:error {:message string}}".to_string(),
            );

            let mut payload = HashMap::new();
            payload.insert(
                MapKey::Keyword(Keyword::new("numerator")),
                Value::Integer(42),
            );
            payload.insert(
                MapKey::Keyword(Keyword::new("denominator")),
                Value::Integer(0),
            );
            arguments.push(Value::Map(payload));
        }
        "core.filter-by-topic" => {
            context.insert(
                "scenario".to_string(),
                "Filter articles by :topic while preserving original fields and returning both matches and match count."
                    .to_string(),
            );

            let articles = vec![
                make_article(
                    "Understanding Async Rust",
                    "rust",
                    "Guide to async/await patterns in Rust.",
                ),
                make_article(
                    "Macro Systems in Clojure",
                    "clojure",
                    "Explores macro capabilities in Clojure.",
                ),
                make_article(
                    "Rust Ownership Deep Dive",
                    "rust",
                    "Ownership and borrowing rules with examples.",
                ),
            ];

            let mut payload = HashMap::new();
            payload.insert(
                MapKey::Keyword(Keyword::new("articles")),
                Value::Vector(articles),
            );
            payload.insert(
                MapKey::Keyword(Keyword::new("topic")),
                Value::String("rust".to_string()),
            );
            arguments.push(Value::Map(payload));

            context.insert(
                "expected_output".to_string(),
                "Return {:matches [...] :count int} where :matches only includes entries whose :topic equals the requested topic."
                    .to_string(),
            );
        }
        _ => {
            arguments.push(Value::String("test_arg".to_string()));
        }
    }

    (arguments, context)
}

fn make_article(title: &str, topic: &str, summary: &str) -> Value {
    let mut article = HashMap::new();
    article.insert(
        MapKey::Keyword(Keyword::new("title")),
        Value::String(title.to_string()),
    );
    article.insert(
        MapKey::Keyword(Keyword::new("topic")),
        Value::String(topic.to_string()),
    );
    article.insert(
        MapKey::Keyword(Keyword::new("summary")),
        Value::String(summary.to_string()),
    );
    Value::Map(article)
}

async fn handle_resume(
    resolver: &Arc<MissingCapabilityResolver>,
    checkpoint_id: &str,
    capability_id: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    println!("🔄 Resuming execution from checkpoint: {}", checkpoint_id);
    println!("📦 Resolved capability: {}", capability_id);

    // Check if checkpoint exists and capability is available
    let checkpoint_archive = resolver.get_checkpoint_archive();
    let checkpoints = checkpoint_archive.get_pending_auto_resume_checkpoints();

    if let Some(checkpoint) = checkpoints.iter().find(|c| c.plan_id == checkpoint_id) {
        println!("✅ Checkpoint found: {}", checkpoint.plan_id);
        println!(
            "   Missing capabilities: {:?}",
            checkpoint.missing_capabilities
        );
        println!("   Auto-resume enabled: {}", checkpoint.auto_resume_enabled);

        if checkpoint
            .missing_capabilities
            .contains(&capability_id.to_string())
        {
            println!(
                "🎯 Capability {} is required for this checkpoint",
                capability_id
            );
            println!("⚠️ Checkpoint resume logic not yet implemented - this is a placeholder");
        } else {
            println!(
                "⚠️ Capability {} not required for this checkpoint",
                capability_id
            );
        }
    } else {
        println!(
            "❌ Checkpoint {} not found or not pending resume",
            checkpoint_id
        );
    }

    Ok(())
}

async fn handle_list_pending(
    resolver: &Arc<MissingCapabilityResolver>,
    filter: Option<&str>,
) -> Result<(), Box<dyn std::error::Error>> {
    println!("📋 Listing pending capabilities awaiting resolution...");

    let stats = resolver.get_stats();

    if let Some(filter) = filter {
        println!("🔍 Filter: {}", filter);
    }

    println!("📊 Pending Capabilities:");
    println!("   Pending: {}", stats.pending_count);
    println!("   In Progress: {}", stats.in_progress_count);
    println!("   Failed: {}", stats.failed_count);
    println!(
        "   Total: {}",
        stats.pending_count + stats.in_progress_count + stats.failed_count
    );

    // Show detailed breakdown if verbose
    if stats.pending_count > 0 {
        println!("\n📝 Pending Details:");
        println!("   • Capabilities awaiting initial resolution");
        println!("   • Checkpoint status: waiting for dependencies");
    }

    if stats.in_progress_count > 0 {
        println!("\n⚙️ In Progress Details:");
        println!("   • Capabilities currently being resolved");
        println!("   • Discovery and validation in progress");
    }

    if stats.failed_count > 0 {
        println!("\n❌ Failed Details:");
        println!("   • Capabilities that failed resolution");
        println!("   • May require manual intervention");
    }

    Ok(())
}

async fn handle_stats(
    resolver: &Arc<MissingCapabilityResolver>,
    time_range: Option<&str>,
) -> Result<(), Box<dyn std::error::Error>> {
    println!("📊 Resolution Statistics and Metrics");

    if let Some(range) = time_range {
        println!("📅 Time Range: {}", range);
    } else {
        println!("📅 Time Range: All time");
    }

    let stats = resolver.get_stats();

    println!("\n🎯 Resolution Metrics:");
    println!("   Pending: {}", stats.pending_count);
    println!("   In Progress: {}", stats.in_progress_count);
    println!("   Failed: {}", stats.failed_count);
    println!("   Success Rate: {:.1}%", calculate_success_rate(&stats));

    let total = stats.pending_count + stats.in_progress_count + stats.failed_count;
    if total > 0 {
        println!("\n📈 Breakdown:");
        println!(
            "   Pending: {:.1}%",
            (stats.pending_count as f64 / total as f64) * 100.0
        );
        println!(
            "   In Progress: {:.1}%",
            (stats.in_progress_count as f64 / total as f64) * 100.0
        );
        println!(
            "   Failed: {:.1}%",
            (stats.failed_count as f64 / total as f64) * 100.0
        );
    }

    println!("\n⏱️ Performance Metrics:");
    println!("   Average Resolution Time: N/A (not yet implemented)");
    println!("   Peak Resolution Rate: N/A (not yet implemented)");
    println!("   Discovery Success Rate: N/A (not yet implemented)");

    Ok(())
}

async fn handle_monitor(
    resolver: &Arc<MissingCapabilityResolver>,
    interval: u64,
    continuous: bool,
) -> Result<(), Box<dyn std::error::Error>> {
    println!("📡 Monitoring resolution queue (interval: {}s)", interval);

    if continuous {
        println!("🔄 Running continuously - press Ctrl+C to stop");
    }

    loop {
        let stats = resolver.get_stats();
        let timestamp = chrono::Utc::now().format("%H:%M:%S");

        println!("\n[{}] 📊 Queue Status:", timestamp);
        println!(
            "   Pending: {} | In Progress: {} | Failed: {}",
            stats.pending_count, stats.in_progress_count, stats.failed_count
        );

        if stats.pending_count > 0 || stats.in_progress_count > 0 {
            println!("   Status: Active");
        } else {
            println!("   Status: Idle");
        }

        if !continuous {
            break;
        }

        sleep(Duration::from_secs(interval)).await;
    }

    Ok(())
}

async fn handle_validate(
    resolver: &Arc<MissingCapabilityResolver>,
    capability_id: &str,
    security_level: Option<&str>,
) -> Result<(), Box<dyn std::error::Error>> {
    println!("🔍 Validating capability: {}", capability_id);

    let security_level = security_level.unwrap_or("medium");
    println!("🛡️ Security Level: {}", security_level);

    // TODO: Implement actual validation logic
    println!("⚠️ Validation logic not yet implemented - this is a placeholder");
    println!("   Capability ID: {}", capability_id);
    println!("   Security Level: {}", security_level);
    println!("   Would perform:");
    println!("     • Static code analysis");
    println!("     • Security vulnerability scan");
    println!("     • Governance policy compliance check");
    println!("     • Performance impact assessment");

    Ok(())
}

async fn handle_search(
    resolver: &Arc<MissingCapabilityResolver>,
    query: &str,
    source: &str,
    limit: usize,
) -> Result<(), Box<dyn std::error::Error>> {
    println!("🔍 Searching for capabilities: '{}'", query);
    println!("📍 Source: {}", source);
    println!("📊 Limit: {}", limit);

    // TODO: Implement actual search logic across different sources
    println!("⚠️ Search functionality not yet implemented - this is a placeholder");

    match source {
        "mcp" => {
            println!("   Would search MCP Registry for: {}", query);
        }
        "registry" => {
            println!("   Would search local registry for: {}", query);
        }
        "local" => {
            println!("   Would search local capabilities for: {}", query);
        }
        "all" => {
            println!("   Would search all sources for: {}", query);
        }
        _ => {
            println!("   Unknown source: {}", source);
        }
    }

    Ok(())
}

async fn handle_export(
    resolver: &Arc<MissingCapabilityResolver>,
    format: &str,
    output: Option<&str>,
) -> Result<(), Box<dyn std::error::Error>> {
    println!("📤 Exporting resolution data");
    println!("📋 Format: {}", format);

    if let Some(output_path) = output {
        println!("📁 Output: {}", output_path);
    } else {
        println!("📁 Output: stdout");
    }

    // TODO: Implement actual export logic
    println!("⚠️ Export functionality not yet implemented - this is a placeholder");

    let stats = resolver.get_stats();
    let export_data = serde_json::json!({
        "timestamp": chrono::Utc::now().to_rfc3339(),
        "statistics": {
            "pending": stats.pending_count,
            "in_progress": stats.in_progress_count,
            "failed": stats.failed_count
        },
        "format": format
    });

    match format {
        "json" => {
            if let Some(output_path) = output {
                std::fs::write(output_path, serde_json::to_string_pretty(&export_data)?)?;
                println!("✅ Data exported to {}", output_path);
            } else {
                println!("{}", serde_json::to_string_pretty(&export_data)?);
            }
        }
        "csv" => {
            println!("   CSV export not yet implemented");
        }
        "yaml" => {
            println!("   YAML export not yet implemented");
        }
        _ => {
            println!("   Unknown format: {}", format);
        }
    }

    Ok(())
}

async fn handle_info(
    marketplace: &CapabilityMarketplace,
    capability_id: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    println!("ℹ️ Capability Information: {}", capability_id);

    // Check if capability exists in marketplace
    if let Some(capability) = marketplace.get_capability(capability_id).await {
        println!("✅ Capability found in marketplace");
        println!("   Name: {}", capability.name);
        println!("   Description: {}", capability.description);
        println!("   Version: {}", capability.version);
        println!("   Provider: {:?}", capability.provider);

        if !capability.permissions.is_empty() {
            println!("   Permissions: {:?}", capability.permissions);
        }

        if !capability.effects.is_empty() {
            println!("   Effects: {:?}", capability.effects);
        }

        if !capability.metadata.is_empty() {
            println!("   Metadata: {:?}", capability.metadata);
        }

        if let Some(agent_metadata) = &capability.agent_metadata {
            println!("   Agent Metadata: {:?}", agent_metadata);
        }
    } else {
        println!("❌ Capability not found in marketplace");
        println!("   This capability may need to be resolved");
    }

    Ok(())
}

async fn handle_cleanup(
    resolver: &Arc<MissingCapabilityResolver>,
    days: u32,
    dry_run: bool,
) -> Result<(), Box<dyn std::error::Error>> {
    println!("🧹 Cleaning up old resolution data");
    println!("📅 Keeping data from last {} days", days);

    if dry_run {
        println!("🔍 DRY RUN - no data will be deleted");
    }

    // TODO: Implement actual cleanup logic
    println!("⚠️ Cleanup functionality not yet implemented - this is a placeholder");
    println!("   Would clean up:");
    println!("     • Old resolution attempts");
    println!("     • Expired checkpoints");
    println!("     • Stale audit logs");
    println!("     • Temporary discovery data");

    if dry_run {
        println!("   (DRY RUN - no actual cleanup performed)");
    } else {
        println!("   (Cleanup would be performed)");
    }

    Ok(())
}

fn calculate_success_rate(stats: &ccos::synthesis::missing_capability_resolver::QueueStats) -> f64 {
    let total = stats.pending_count + stats.in_progress_count + stats.failed_count;
    if total == 0 {
        100.0
    } else {
        let successful = total - stats.failed_count;
        (successful as f64 / total as f64) * 100.0
    }
}

/// Bootstrap the marketplace with some test capabilities for demonstration
async fn bootstrap_test_capabilities(
    marketplace: &CapabilityMarketplace,
) -> Result<(), Box<dyn std::error::Error>> {
    // Add some test primitive capabilities
    let test_capabilities = vec![
        ("travel.hotels", "Hotel booking capability"),
        ("travel.attractions", "Tourist attractions capability"),
        ("food.recommendations", "Restaurant recommendations"),
        ("weather.current", "Current weather information"),
        ("data.json.parse", "JSON parsing utility"),
    ];

    for (capability_id, description) in &test_capabilities {
        let capability_id = capability_id.to_string();
        let description = description.to_string();
        let manifest = CapabilityManifest {
            id: capability_id.clone(),
            name: description.clone(),
            description: format!("Test capability: {}", description),
            version: "1.0.0".to_string(),
            provider: ProviderType::Local(LocalCapability {
                handler: Arc::new(move |_args| {
                    Ok(Value::String(format!("Test result from {}", capability_id)))
                }),
            }),
            input_schema: None,
            output_schema: None,
            attestation: None,
            provenance: None,
            permissions: vec![],
            effects: vec![],
            metadata: HashMap::new(),
            agent_metadata: None,
        };

        marketplace.register_capability_manifest(manifest).await?;
    }

    println!(
        "🚀 Bootstrapped marketplace with {} test capabilities",
        test_capabilities.len()
    );
    Ok(())
}
