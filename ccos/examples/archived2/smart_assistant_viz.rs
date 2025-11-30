use std::borrow::Cow;
use std::collections::{HashMap, HashSet};
use std::error::Error;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::SystemTime;

use clap::{ArgAction, Parser};
use crossterm::style::Stylize;
use rtfs::ast::{Keyword, MapKey};
use rtfs::config::profile_selection::expand_profiles;
use rtfs::config::types::{AgentConfig, LlmProfile};
use rtfs::runtime::error::RuntimeError;
use rtfs::runtime::values::Value;

use ccos::capabilities::{MCPSessionHandler, SessionPoolManager};
use ccos::intent_graph::config::IntentGraphConfig;
use ccos::synthesis::missing_capability_resolver::{
    MissingCapabilityRequest, ResolutionEvent, ResolutionObserver, ResolutionResult,
};
use ccos::CCOS;

#[derive(Parser, Debug)]
#[command(author, version, about = "Structured capability discovery timeline for CCOS smart assistant", long_about = None)]
struct Args {
    /// Path to agent configuration (TOML/JSON)
    #[arg(long, default_value = "config/agent_config.toml")]
    config: String,

    /// Optional LLM profile name defined in agent_config
    #[arg(long)]
    profile: Option<String>,

    /// Optional natural language goal for context
    #[arg(long)]
    goal: Option<String>,

    /// Capability identifier(s) to resolve (repeat flag for multiple)
    #[arg(long = "capability", action = ArgAction::Append)]
    capabilities: Vec<String>,

    /// Expand all events or specific stages (e.g. --show mcp --show result)
    #[arg(long = "show", action = ArgAction::Append)]
    show_filters: Vec<String>,

    /// Print raw prompts/responses during LLM interactions
    #[arg(long, default_value_t = false)]
    debug_prompts: bool,

    /// Stream resolver's own logs (noisy). Off by default; use --trace to see them.
    #[arg(long, default_value_t = false)]
    trace: bool,
}

const DEFAULT_CAPABILITIES: &[&str] = &[
    "github.users.list_repos",
    "github.users.projects.list",
    "github.issues.search",
];

#[derive(Default)]
struct TimelineObserver {
    events: Mutex<Vec<ResolutionEvent>>,
}

impl TimelineObserver {
    fn drain(&self) -> Vec<ResolutionEvent> {
        let mut guard = self.events.lock().unwrap();
        std::mem::take(&mut *guard)
    }
}

impl ResolutionObserver for TimelineObserver {
    fn on_event(&self, event: ResolutionEvent) {
        if let Ok(mut guard) = self.events.lock() {
            guard.push(event);
        }
    }
}

#[derive(Debug, Default)]
struct DisplayFilter {
    show_all: bool,
    tokens: HashSet<String>,
}

#[derive(Clone)]
struct StageDescriptor {
    label: Cow<'static, str>,
    depth: usize,
}

impl DisplayFilter {
    fn from_args(args: &[String]) -> Self {
        if args.iter().any(|value| value.eq_ignore_ascii_case("all")) {
            return Self {
                show_all: true,
                tokens: HashSet::new(),
            };
        }

        let tokens = args
            .iter()
            .map(|value| value.to_lowercase())
            .collect::<HashSet<_>>();

        Self {
            show_all: false,
            tokens,
        }
    }

    fn should_expand(&self, stage: &str) -> bool {
        if self.show_all {
            return true;
        }

        let stage_lower = stage.to_lowercase();
        if self.tokens.contains(&stage_lower) {
            return true;
        }

        self.tokens.iter().any(|token| stage_lower.contains(token))
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    let args = Args::parse();

    if args.debug_prompts {
        std::env::set_var("CCOS_DEBUG_PROMPTS", "1");
    }
    if !args.trace {
        std::env::set_var("CCOS_QUIET_RESOLVER", "1");
    }

    let agent_config = load_agent_config(&args.config)?;
    apply_llm_profile(&agent_config, args.profile.as_deref())?;

    if std::env::var("CCOS_DELEGATING_MODEL").is_err() {
        std::env::set_var("CCOS_DELEGATING_MODEL", "stub");
        std::env::set_var("CCOS_LLM_MODEL", "stub");
        std::env::set_var("CCOS_LLM_PROVIDER", "stub");
        std::env::set_var("CCOS_ALLOW_STUB_PROVIDER", "1");
    }

    let plan_archive_path = plan_archive_dir();
    ensure_directory(&plan_archive_path)?;

    let ccos = Arc::new(
        CCOS::new_with_agent_config_and_configs_and_debug_callback(
            IntentGraphConfig::default(),
            Some(plan_archive_path),
            Some(agent_config.clone()),
            None,
        )
        .await
        .map_err(runtime_error)?,
    );

    configure_session_pool(&ccos).await?;

    let resolver = match ccos.get_missing_capability_resolver() {
        Some(resolver) => resolver,
        None => {
            eprintln!(
                "⚠️  Missing capability resolver is disabled in this configuration; nothing to visualize."
            );
            return Ok(());
        }
    };

    if let Some(delegating) = ccos.get_delegating_arbiter() {
        resolver.set_delegating_arbiter(Some(delegating));
    }

    let display_filter = DisplayFilter::from_args(&args.show_filters);

    let capabilities = if args.capabilities.is_empty() {
        DEFAULT_CAPABILITIES
            .iter()
            .map(|value| value.to_string())
            .collect()
    } else {
        args.capabilities.clone()
    };

    for capability in capabilities {
        println!("\n{}", "═".repeat(80));
        println!(
            "{} {}",
            "Capability".bold().cyan(),
            capability.as_str().bold()
        );
        if let Some(goal) = &args.goal {
            println!("{} {}", "Goal".bold().cyan(), goal);
        }

        let (arguments, mut context) = build_sample_invocation(&capability, false);
        if let Some(goal) = &args.goal {
            context.insert("goal".to_string(), goal.clone());
        }

        let request = MissingCapabilityRequest {
            capability_id: capability.clone(),
            arguments,
            context,
            requested_at: SystemTime::now(),
            attempt_count: 0,
        };

        let observer = Arc::new(TimelineObserver::default());
        resolver.set_event_observer(Some(observer.clone()));

        let resolution = resolver.resolve_capability(&request).await;
        let events = observer.drain();
        resolver.set_event_observer(None);
        let resolution = resolution?;

        render_timeline(&events, &display_filter, &resolution);
    }

    Ok(())
}

fn plan_archive_dir() -> PathBuf {
    std::env::var("CCOS_PLAN_ARCHIVE_PATH")
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from("demo_storage/plans"))
}

fn ensure_directory(path: &PathBuf) -> Result<(), Box<dyn Error>> {
    if let Err(err) = std::fs::create_dir_all(path) {
        if !path.exists() {
            return Err(Box::new(err));
        }
    }
    Ok(())
}

fn render_timeline(events: &[ResolutionEvent], filter: &DisplayFilter, result: &ResolutionResult) {
    println!("  {}", "Timeline".bold());
    if events.is_empty() {
        println!("    - No timeline events recorded.");
    } else {
        for event in events {
            let descriptor = stage_descriptor(event.stage);
            let indent = "    ".repeat(descriptor.depth);
            println!(
                "  {}- {}: {}",
                indent,
                descriptor.label.as_ref().bold(),
                event.summary
            );
            if filter.should_expand(event.stage) {
                if let Some(detail) = &event.detail {
                    for line in detail.lines() {
                        let trimmed = line.trim_end();
                        if trimmed.is_empty() {
                            continue;
                        }
                        println!("  {}    {}", indent, trimmed);
                    }
                }
            }
        }
    }

    println!("\n  {}", "Outcome".bold());
    match result {
        ResolutionResult::Resolved {
            capability_id: resolved_id,
            resolution_method,
            provider_info,
        } => {
            println!("    - Resolved capability: {}", resolved_id.as_str().cyan());
            println!("    - Resolution method: {}", resolution_method);
            if let Some(info) = provider_info {
                println!("    - Provider: {}", info);
            }
        }
        ResolutionResult::Failed { reason, .. } => {
            println!("    - Resolution failed: {}", reason);
        }
        ResolutionResult::PermanentlyFailed { reason, .. } => {
            println!("    - Permanently failed: {}", reason);
        }
    }
}

fn stage_descriptor(stage: &str) -> StageDescriptor {
    match stage {
        "start" => StageDescriptor {
            label: Cow::Borrowed("Start"),
            depth: 0,
        },
        "alias_lookup" => StageDescriptor {
            label: Cow::Borrowed("Alias cache"),
            depth: 1,
        },
        "discovery" => StageDescriptor {
            label: Cow::Borrowed("Discovery"),
            depth: 1,
        },
        "marketplace" | "marketplace_search" => StageDescriptor {
            label: Cow::Borrowed("Marketplace"),
            depth: 2,
        },
        "local_scan" => StageDescriptor {
            label: Cow::Borrowed("Local manifests"),
            depth: 2,
        },
        "mcp_registry" | "mcp_search" => StageDescriptor {
            label: Cow::Borrowed("MCP registry"),
            depth: 2,
        },
        "mcp_introspection" => StageDescriptor {
            label: Cow::Borrowed("MCP introspection"),
            depth: 3,
        },
        "heuristic_match" => StageDescriptor {
            label: Cow::Borrowed("Heuristic match"),
            depth: 2,
        },
        "tool_selector" => StageDescriptor {
            label: Cow::Borrowed("Tool selector"),
            depth: 3,
        },
        "llm_selection" => StageDescriptor {
            label: Cow::Borrowed("LLM selection"),
            depth: 3,
        },
        "llm_synthesis" => StageDescriptor {
            label: Cow::Borrowed("LLM synthesis"),
            depth: 3,
        },
        "result" => StageDescriptor {
            label: Cow::Borrowed("Result"),
            depth: 1,
        },
        other => StageDescriptor {
            label: Cow::Owned(other.replace('_', " ")),
            depth: 1,
        },
    }
}

async fn configure_session_pool(ccos: &Arc<CCOS>) -> Result<(), Box<dyn Error>> {
    let mut session_pool = SessionPoolManager::new();
    session_pool.register_handler("mcp", Arc::new(MCPSessionHandler::new()));
    let session_pool = Arc::new(session_pool);

    let marketplace = ccos.get_capability_marketplace();
    marketplace.set_session_pool(session_pool).await;

    Ok(())
}

fn runtime_error(err: RuntimeError) -> Box<dyn Error> {
    Box::new(err)
}

fn load_agent_config(path: &str) -> Result<AgentConfig, Box<dyn Error>> {
    let data = std::fs::read_to_string(path)?;
    let config = if path.ends_with(".json") {
        serde_json::from_str(&data)?
    } else {
        toml::from_str(&data)?
    };
    Ok(config)
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
            }
        } else if let Some(first) = profiles.first() {
            apply_profile_env(first);
        }
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
            eprintln!("⚠️  WARNING: Using stub LLM provider (testing only - not realistic)");
            eprintln!(
                "   Set a real provider in agent_config.toml or use --profile with a real provider"
            );
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

fn build_sample_invocation(
    capability_id: &str,
    force: bool,
) -> (Vec<Value>, HashMap<String, String>) {
    let mut context = HashMap::new();
    context.insert("plan_id".to_string(), "viz_plan".to_string());
    context.insert("intent_id".to_string(), "viz_intent".to_string());
    context.insert("force_resolution".to_string(), force.to_string());

    let mut arguments = Vec::new();

    match capability_id {
        "core.safe-div" => {
            context.insert(
                "scenario".to_string(),
                "Guard division by zero and return either {:value number} or {:error {:message string}}".to_string(),
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
                "Filter articles by :topic while preserving original fields and returning {:matches [:vector :map]} and {:count int}."
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
                    "Advanced Rate Limiting",
                    "architecture",
                    "Design patterns for resilient distributed systems.",
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
                "Return {:matches [...] :count int} where :matches only includes entries whose :topic equals the requested topic.".to_string(),
            );
        }
        _ => {
            arguments.push(Value::String("sample".to_string()));
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
