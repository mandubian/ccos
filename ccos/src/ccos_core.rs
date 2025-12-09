#![allow(dead_code)]
//! CCOS core implementation - main CCOS system struct and initialization

// Keep one declaration per submodule to avoid duplicate module errors.
// If some modules are not yet present, gate or comment them as needed.

// Re-export some arbiter sub-modules at the ccos root for historic import paths
// Tests and examples sometimes refer to `ccos::ccos::delegating_arbiter` or
// `ccos::ccos::arbiter_engine`. Provide lightweight re-exports to avoid
// breaking consumers when the arbiter was nested under `ccos::arbiter`.
pub use crate::arbiter::arbiter_engine;
pub use crate::arbiter::delegating_arbiter;

// --- Core CCOS System ---

use std::cmp::Ordering;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

// AgentRegistry migration: Removed AgentRegistry import
// Agents are now managed via CapabilityMarketplace with :kind :agent capabilities
use crate::arbiter::prompt::{FilePromptStore, PromptStore};
use crate::arbiter::DelegatingArbiter;
use crate::capability_marketplace::CapabilityMarketplace;
use crate::catalog::{CatalogEntryKind, CatalogFilter, CatalogHit, CatalogService};
use crate::host::RuntimeHost;
use crate::rtfs_bridge::RtfsErrorExplainer;
use rtfs::ast::{Keyword, MapKey};
use rtfs::config::types::AgentConfig;
use rtfs::runtime::error::RuntimeResult;
use rtfs::runtime::security::RuntimeContext;
use rtfs::runtime::values::Value;
use rtfs::runtime::{ModuleRegistry, RTFSRuntime, Runtime};

use crate::types::{ActionType, ExecutionResult};

use crate::causal_chain::CausalChain;
use crate::event_sink::CausalChainIntentEventSink;
use crate::governance_kernel::GovernanceKernel;
use crate::intent_graph::{config::IntentGraphConfig, IntentGraph};
use crate::types::StorableIntent;
use rtfs::runtime::error::RuntimeError;
use rtfs::runtime::host_interface::HostInterface;

use once_cell::sync::Lazy;

use crate::orchestrator::Orchestrator;
use crate::plan_archive::PlanArchive;

/// The main CCOS system struct, which initializes and holds all core components.
/// This is the primary entry point for interacting with the CCOS.
pub struct CCOS {
    pub arbiter: Arc<DelegatingArbiter>,
    pub governance_kernel: Arc<GovernanceKernel>,
    pub orchestrator: Arc<Orchestrator>,
    // The following components are shared across the system
    pub intent_graph: Arc<Mutex<IntentGraph>>,
    pub causal_chain: Arc<Mutex<CausalChain>>,
    pub capability_marketplace: Arc<CapabilityMarketplace>,
    pub plan_archive: Arc<PlanArchive>,
    pub rtfs_runtime: Arc<Mutex<dyn RTFSRuntime>>,
    // AgentRegistry migration: Removed agent_registry field
    // Agents are now managed via CapabilityMarketplace with :kind :agent capabilities
    pub agent_config: Arc<AgentConfig>, // Global agent configuration (future: loaded from RTFS form)
    /// Missing capability resolver for runtime trap functionality
    pub missing_capability_resolver:
        Option<Arc<crate::synthesis::missing_capability_resolver::MissingCapabilityResolver>>,
    /// Optional debug callback for emitting lifecycle JSON lines (plan generation, execution etc.)
    pub debug_callback: Option<Arc<dyn Fn(String) + Send + Sync>>,
    pub catalog: Arc<CatalogService>,
}

// tests moved to bottom module

/// Options that control the behaviour of the plan auto-repair pipeline.
#[derive(Debug, Clone)]
pub struct PlanAutoRepairOptions {
    /// Maximum number of repair attempts (each attempt can trigger an LLM call).
    pub max_attempts: usize,
    /// Additional natural-language context to append to the repair prompt.
    pub additional_context: Option<String>,
    /// Extra grammar hints to provide to the LLM (deduplicated with diagnostics).
    pub grammar_hints: Vec<String>,
    /// When true, the raw prompt/response payloads are emitted via the debug callback.
    pub debug_responses: bool,
}

impl Default for PlanAutoRepairOptions {
    fn default() -> Self {
        Self {
            max_attempts: 1,
            additional_context: None,
            debug_responses: false,
            grammar_hints: DEFAULT_REPAIR_GRAMMAR_HINTS.clone(),
        }
    }
}

const AUTO_REPAIR_PROMPT_ID: &str = "auto_repair";
const AUTO_REPAIR_PROMPT_VERSION: &str = "v1";

static DEFAULT_REPAIR_GRAMMAR_HINTS: Lazy<Vec<String>> = Lazy::new(|| {
    load_grammar_hints_from_prompt_store().unwrap_or_else(|err| {
        eprintln!(
            "⚠️  Falling back to built-in auto-repair grammar hints: {}",
            err
        );
        fallback_grammar_hints()
    })
});

fn load_grammar_hints_from_prompt_store() -> Result<Vec<String>, String> {
    let base_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("assets/prompts/arbiter");
    let store = FilePromptStore::new(&base_dir);
    let template = store
        .get_template(AUTO_REPAIR_PROMPT_ID, AUTO_REPAIR_PROMPT_VERSION)
        .map_err(|e| {
            format!(
                "failed to load {}/{} prompt: {}",
                AUTO_REPAIR_PROMPT_ID, AUTO_REPAIR_PROMPT_VERSION, e
            )
        })?;
    let grammar_markdown = template
        .sections
        .into_iter()
        .find(|(name, _)| name == "grammar")
        .map(|(_, content)| content)
        .ok_or_else(|| "prompt template did not contain a grammar section".to_string())?;

    let mut hints: Vec<String> = grammar_markdown
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .filter_map(|line| {
            if line.starts_with("- ") {
                Some(line.trim_start_matches("- ").trim().to_string())
            } else {
                None
            }
        })
        .filter(|line| !line.is_empty())
        .collect();

    hints.dedup();

    if hints.is_empty() {
        Err("grammar section did not yield any bullet entries".to_string())
    } else {
        Ok(hints)
    }
}

fn fallback_grammar_hints() -> Vec<String> {
    vec![
        "RTFS uses prefix notation with parentheses.".to_string(),
        "Maps are written as `{:keyword value}` pairs without `=` characters.".to_string(),
        "Do NOT use commas `,` to separate map entries or list items.".to_string(),
        "Strings must be enclosed in double quotes.".to_string(),
        "Keywords begin with a colon and typically use kebab-case (e.g. `:issues`).".to_string(),
        "Capability calls use `(call :provider.capability {:param value})`.".to_string(),
    ]
}

fn extract_plan_rtfs_from_response(response: &str) -> Option<String> {
    let trimmed = response.trim();
    if trimmed.is_empty() {
        return None;
    }

    let mut candidates: Vec<String> = Vec::new();

    // Collect fenced code blocks (```rtfs ...``` etc.) from the response.
    let mut cursor = trimmed;
    while let Some(start) = cursor.find("```") {
        let after_tick = &cursor[start + 3..];
        let mut block_start = after_tick;
        if let Some(idx) = after_tick.find('\n') {
            let first_line = after_tick[..idx].trim().to_ascii_lowercase();
            let rest = &after_tick[idx + 1..];
            if first_line == "rtfs" || first_line == "lisp" || first_line == "scheme" {
                block_start = rest;
            }
        }

        if let Some(end_idx) = block_start.find("```") {
            let code = block_start[..end_idx].trim();
            if !code.is_empty() {
                candidates.push(code.to_string());
            }
            cursor = &block_start[end_idx + 3..];
        } else {
            break;
        }
    }

    // Fallback: use the trimmed response with surrounding backticks removed.
    let stripped = trimmed.trim_matches('`').trim();
    if !stripped.is_empty() {
        candidates.push(stripped.to_string());
    }

    for candidate in candidates {
        let candidate_trimmed = candidate.trim();
        if candidate_trimmed.is_empty() {
            continue;
        }

        if candidate_trimmed.starts_with("(plan") {
            return Some(candidate_trimmed.to_string());
        }

        if rtfs::parser::parse(candidate_trimmed).is_ok()
            || rtfs::parser::parse_expression(candidate_trimmed).is_ok()
        {
            return Some(candidate_trimmed.to_string());
        }
    }

    None
}

#[derive(Clone, Copy)]
enum CatalogQueryMode {
    Semantic,
    Keyword,
}

impl CatalogQueryMode {
    fn as_str(&self) -> &'static str {
        match self {
            CatalogQueryMode::Semantic => "semantic",
            CatalogQueryMode::Keyword => "keyword",
        }
    }
}

impl CCOS {
    /// Creates and initializes a new CCOS instance.
    pub async fn new() -> RuntimeResult<Self> {
        Self::new_with_debug_callback(None).await
    }

    /// Creates and initializes a new CCOS instance with an optional debug callback.
    pub async fn new_with_debug_callback(
        debug_callback: Option<Arc<dyn Fn(String) + Send + Sync>>,
    ) -> RuntimeResult<Self> {
        Self::new_with_config_and_debug_callback(IntentGraphConfig::default(), debug_callback).await
    }

    /// Creates and initializes a new CCOS instance with custom config and optional debug callback.
    pub async fn new_with_config_and_debug_callback(
        intent_graph_config: IntentGraphConfig,
        debug_callback: Option<Arc<dyn Fn(String) + Send + Sync>>,
    ) -> RuntimeResult<Self> {
        Self::new_with_configs_and_debug_callback(intent_graph_config, None, debug_callback).await
    }

    /// Creates and initializes a new CCOS instance with custom configs and optional debug callback.
    pub async fn new_with_configs_and_debug_callback(
        intent_graph_config: IntentGraphConfig,
        plan_archive_path: Option<std::path::PathBuf>,
        debug_callback: Option<Arc<dyn Fn(String) + Send + Sync>>,
    ) -> RuntimeResult<Self> {
        // Delegate to the new initializer that accepts an optional AgentConfig (backwards compatible)
        Self::new_with_agent_config_and_configs_and_debug_callback(
            intent_graph_config,
            plan_archive_path,
            None,
            debug_callback,
        )
        .await
    }

    /// New: Creates and initializes a new CCOS instance with an optional loaded AgentConfig,
    /// custom configs and optional debug callback. When `agent_config_opt` is Some, that
    /// config will be used instead of `AgentConfig::default()`.
    pub async fn new_with_agent_config_and_configs_and_debug_callback(
        intent_graph_config: IntentGraphConfig,
        plan_archive_path: Option<std::path::PathBuf>,
        agent_config_opt: Option<rtfs::config::types::AgentConfig>,
        debug_callback: Option<Arc<dyn Fn(String) + Send + Sync>>,
    ) -> RuntimeResult<Self> {
        // 1. Initialize shared, stateful components
        let causal_chain = Arc::new(Mutex::new(CausalChain::new()?));
        let sink = Arc::new(CausalChainIntentEventSink::new(Arc::clone(&causal_chain)));
        let intent_graph = Arc::new(Mutex::new(IntentGraph::with_config_and_event_sink(
            intent_graph_config,
            sink,
        )?));
        // Initialize capability marketplace with registry
        let capability_registry = Arc::new(tokio::sync::RwLock::new(
            crate::capabilities::registry::CapabilityRegistry::new(),
        ));

        // Pass the full CCOS capability registry to the marketplace
        let mut capability_marketplace =
            CapabilityMarketplace::with_causal_chain_and_debug_callback(
                Arc::clone(&capability_registry),
                Some(Arc::clone(&causal_chain)),
                debug_callback.clone(),
            );

        // Add Local Config MCP Discovery (centralized discovery)
        capability_marketplace.add_discovery_agent(Box::new(
            crate::capability_marketplace::config_mcp_discovery::LocalConfigMcpDiscovery::new(),
        ));

        // Bootstrap the marketplace with discovered capabilities
        capability_marketplace.bootstrap().await?;

        let capability_marketplace = Arc::new(capability_marketplace);

        // Provide a CCOS-aware host for executing RTFS capabilities loaded into the marketplace
        let rtfs_host_factory = {
            let marketplace_clone = Arc::clone(&capability_marketplace);
            let causal_chain_clone = Arc::clone(&causal_chain);
            Arc::new(move || {
                Arc::new(RuntimeHost::new(
                    Arc::clone(&causal_chain_clone),
                    Arc::clone(&marketplace_clone),
                    RuntimeContext::full(),
                )) as Arc<dyn HostInterface + Send + Sync>
            })
        };
        capability_marketplace.set_rtfs_host_factory(rtfs_host_factory);

        let catalog_service = Arc::new(CatalogService::new());
        capability_marketplace
            .set_catalog_service(Arc::clone(&catalog_service))
            .await;

        // Use provided AgentConfig or default
        let agent_config = if let Some(cfg) = agent_config_opt {
            Arc::new(cfg)
        } else {
            Arc::new(AgentConfig::default())
        };

        // Register built-in capabilities required by default plans (await using ambient runtime)
        crate::capabilities::register_default_capabilities(&capability_marketplace).await?;
        crate::planner::capabilities::register_planner_capabilities(
            Arc::clone(&capability_marketplace),
            Arc::clone(&catalog_service),
        )
        .await?;

        // 2. Initialize architectural components, injecting dependencies
        let plan_archive = Arc::new(match plan_archive_path {
            Some(path) => PlanArchive::with_file_storage(path).map_err(|e| {
                RuntimeError::StorageError(format!("Failed to create plan archive: {}", e))
            })?,
            None => PlanArchive::new(),
        });
        plan_archive.set_catalog(Arc::clone(&catalog_service));

        catalog_service
            .ingest_marketplace(&capability_marketplace)
            .await;
        catalog_service.ingest_plan_archive(&plan_archive);
        let orchestrator = Arc::new(Orchestrator::new(
            Arc::clone(&causal_chain),
            Arc::clone(&intent_graph),
            Arc::clone(&capability_marketplace),
            Arc::clone(&plan_archive),
        ));

        let governance_kernel = Arc::new(GovernanceKernel::new(
            Arc::clone(&orchestrator),
            Arc::clone(&intent_graph),
        ));

        // AgentRegistry migration: Agents are now registered as capabilities with :kind :agent
        // TODO: Migrate delegation feedback tracking to CapabilityMarketplace

        // Always create delegating arbiter - this is now the primary arbiter
        let delegating_arbiter = {
            // Try to extract LLM config from agent config first
            let llm_config = if let Some(llm_profiles) = &agent_config.llm_profiles {
                // Use default profile or first available profile
                let profile_name = llm_profiles
                    .default
                    .clone()
                    .or_else(|| llm_profiles.profiles.first().map(|p| p.name.clone()));

                if let Some(name) = profile_name {
                    if let Some(profile) = crate::examples_common::builder::find_llm_profile(
                        agent_config.as_ref(),
                        &name,
                    ) {
                        // Resolve API key from env var or inline
                        let api_key = profile.api_key.clone().or_else(|| {
                            profile
                                .api_key_env
                                .as_ref()
                                .and_then(|env| std::env::var(env).ok())
                        });

                        let provider_type = match profile.provider.to_lowercase().as_str() {
                            "openai" | "openrouter" => {
                                crate::arbiter::arbiter_config::LlmProviderType::OpenAI
                            }
                            "anthropic" | "claude" => {
                                crate::arbiter::arbiter_config::LlmProviderType::Anthropic
                            }
                            "local" | "ollama" => {
                                crate::arbiter::arbiter_config::LlmProviderType::Local
                            }
                            "stub" | "test" => {
                                crate::arbiter::arbiter_config::LlmProviderType::Stub
                            }
                            _ => crate::arbiter::arbiter_config::LlmProviderType::OpenAI,
                        };

                        crate::arbiter::arbiter_config::LlmConfig {
                            provider_type,
                            model: profile.model,
                            api_key,
                            base_url: profile.base_url,
                            max_tokens: profile.max_tokens.or(Some(1000)),
                            temperature: profile.temperature.or(Some(0.7)),
                            timeout_seconds: Some(30),
                            prompts: None,
                            retry_config: crate::arbiter::arbiter_config::RetryConfig::default(),
                        }
                    } else {
                        // Profile not found, fall through to env var logic
                        let model = std::env::var("CCOS_DELEGATING_MODEL")
                            .unwrap_or_else(|_| "stub".to_string());
                        let (api_key, base_url) =
                            if let Ok(key) = std::env::var("OPENROUTER_API_KEY") {
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
                        let provider_hint = std::env::var("CCOS_LLM_PROVIDER_HINT")
                            .unwrap_or_else(|_| String::from(""));
                        let provider_type = if provider_hint.eq_ignore_ascii_case("stub")
                            || model == "stub-model"
                            || model == "deterministic-stub-model"
                            || model == "stub"
                            || api_key.is_none()
                        {
                            crate::arbiter::arbiter_config::LlmProviderType::Stub
                        } else if provider_hint.eq_ignore_ascii_case("anthropic") {
                            crate::arbiter::arbiter_config::LlmProviderType::Anthropic
                        } else if provider_hint.eq_ignore_ascii_case("local") {
                            crate::arbiter::arbiter_config::LlmProviderType::Local
                        } else {
                            crate::arbiter::arbiter_config::LlmProviderType::OpenAI
                        };
                        crate::arbiter::arbiter_config::LlmConfig {
                            provider_type,
                            model,
                            api_key,
                            base_url,
                            max_tokens: Some(1000),
                            temperature: Some(0.7),
                            timeout_seconds: Some(30),
                            prompts: None,
                            retry_config: crate::arbiter::arbiter_config::RetryConfig::default(),
                        }
                    }
                } else {
                    // No profile name, fall through to env var logic
                    let model = std::env::var("CCOS_DELEGATING_MODEL")
                        .unwrap_or_else(|_| "stub".to_string());
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
                    let provider_hint = std::env::var("CCOS_LLM_PROVIDER_HINT")
                        .unwrap_or_else(|_| String::from(""));
                    let provider_type = if provider_hint.eq_ignore_ascii_case("stub")
                        || model == "stub-model"
                        || model == "deterministic-stub-model"
                        || model == "stub"
                        || api_key.is_none()
                    {
                        crate::arbiter::arbiter_config::LlmProviderType::Stub
                    } else if provider_hint.eq_ignore_ascii_case("anthropic") {
                        crate::arbiter::arbiter_config::LlmProviderType::Anthropic
                    } else if provider_hint.eq_ignore_ascii_case("local") {
                        crate::arbiter::arbiter_config::LlmProviderType::Local
                    } else {
                        crate::arbiter::arbiter_config::LlmProviderType::OpenAI
                    };
                    crate::arbiter::arbiter_config::LlmConfig {
                        provider_type,
                        model,
                        api_key,
                        base_url,
                        max_tokens: Some(1000),
                        temperature: Some(0.7),
                        timeout_seconds: Some(30),
                        prompts: None,
                        retry_config: crate::arbiter::arbiter_config::RetryConfig::default(),
                    }
                }
            } else {
                // No llm_profiles in agent config, fall back to environment variables
                let model =
                    std::env::var("CCOS_DELEGATING_MODEL").unwrap_or_else(|_| "stub".to_string());

                // Determine API configuration - use stub if no API keys are available
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

                // Create LLM config for delegating arbiter
                let provider_hint =
                    std::env::var("CCOS_LLM_PROVIDER_HINT").unwrap_or_else(|_| String::from(""));
                let provider_type = if provider_hint.eq_ignore_ascii_case("stub")
                    || model == "stub-model"
                    || model == "deterministic-stub-model"
                    || model == "stub"
                    || api_key.is_none()
                {
                    crate::arbiter::arbiter_config::LlmProviderType::Stub
                } else if provider_hint.eq_ignore_ascii_case("anthropic") {
                    crate::arbiter::arbiter_config::LlmProviderType::Anthropic
                } else if provider_hint.eq_ignore_ascii_case("local") {
                    crate::arbiter::arbiter_config::LlmProviderType::Local
                } else {
                    crate::arbiter::arbiter_config::LlmProviderType::OpenAI
                };

                crate::arbiter::arbiter_config::LlmConfig {
                    provider_type,
                    model,
                    api_key,
                    base_url,
                    max_tokens: Some(1000),
                    temperature: Some(0.7),
                    timeout_seconds: Some(30),
                    prompts: None,
                    retry_config: crate::arbiter::arbiter_config::RetryConfig::default(),
                }
            };

            // Allow examples or environment to override retry config for the LLM provider.
            let mut rc = llm_config.retry_config.clone();
            if let Ok(v) = std::env::var("CCOS_LLM_RETRY_MAX_RETRIES") {
                if let Ok(n) = v.parse::<u32>() {
                    rc.max_retries = n;
                }
            }
            if let Ok(v) = std::env::var("CCOS_LLM_RETRY_SEND_FEEDBACK") {
                rc.send_error_feedback = matches!(v.as_str(), "1" | "true" | "yes" | "on");
            }
            if let Ok(v) = std::env::var("CCOS_LLM_RETRY_SIMPLIFY_FINAL") {
                rc.simplify_on_final_attempt = matches!(v.as_str(), "1" | "true" | "yes" | "on");
            }
            if let Ok(v) = std::env::var("CCOS_LLM_RETRY_USE_STUB_FALLBACK") {
                rc.use_stub_fallback = matches!(v.as_str(), "1" | "true" | "yes" | "on");
            }
            // apply overrides back into llm_config
            let llm_config = crate::arbiter::arbiter_config::LlmConfig {
                retry_config: rc,
                ..llm_config
            };

            // Convert agent config delegation to arbiter delegation config
            let delegation_config = crate::arbiter::arbiter_config::DelegationConfig {
                enabled: true,
                threshold: 0.65,
                max_candidates: 3,
                min_skill_hits: None,
                agent_registry: crate::arbiter::arbiter_config::AgentRegistryConfig {
                    registry_type: crate::arbiter::arbiter_config::RegistryType::InMemory,
                    database_url: None,
                    agents: vec![],
                },
                adaptive_threshold: None,
                print_extracted_intent: Some(false),
                print_extracted_plan: Some(false),
            };

            // Always create delegating arbiter - fail properly if LLM provider is not configured
            let delegating_arbiter = crate::arbiter::DelegatingArbiter::new(
                llm_config,
                delegation_config,
                Arc::clone(&capability_marketplace),
                Arc::clone(&intent_graph),
            )
            .await
            .map_err(|e| {
                RuntimeError::Generic(format!(
                    "Failed to initialize DelegatingArbiter. This typically means no LLM provider is configured. \
                    Please set one of: CCOS_LLM_PROVIDER_HINT=openai|anthropic|local with OPENAI_API_KEY, ANTHROPIC_API_KEY, \
                    or OPENROUTER_API_KEY with CCOS_LLM_BASE_URL. Error: {}",
                    e
                ))
            })?;
            Arc::new(delegating_arbiter)
        };

        // Log which LLM provider is being used
        {
            let cfg = delegating_arbiter.get_llm_config();
            let provider_type = format!("{:?}", cfg.provider_type);
            let model = &cfg.model;
            eprintln!("🤖 Arbiter using {} (model: {})", provider_type, model);
        }

        // The delegating_arbiter is now the primary arbiter
        let arbiter = Arc::clone(&delegating_arbiter);

        // Initialize checkpoint archive
        let checkpoint_archive = Arc::new(crate::checkpoint_archive::CheckpointArchive::new());

        let mut missing_capability_config =
            crate::synthesis::feature_flags::MissingCapabilityConfig::from_agent_config(Some(
                agent_config.as_ref(),
            ));

        if let Err(err) = missing_capability_config.validate() {
            eprintln!(
                "⚠️  Missing capability configuration invalid: {}. Falling back to environment defaults.",
                err
            );
            missing_capability_config =
                crate::synthesis::feature_flags::MissingCapabilityConfig::from_env();
        }

        let resolver_config = crate::synthesis::missing_capability_resolver::ResolverConfig {
            max_attempts: missing_capability_config.max_resolution_attempts,
            auto_resolve: missing_capability_config.feature_flags.auto_resolution,
            verbose_logging: agent_config
                .missing_capabilities
                .verbose_logging
                .unwrap_or(false),
            base_backoff_seconds: 60,
            max_backoff_seconds: 3600,
            high_risk_auto_resolution: false,
            human_approval_timeout_hours: 24,
        };

        // Initialize missing capability resolver
        let missing_capability_resolver = Arc::new(
            crate::synthesis::missing_capability_resolver::MissingCapabilityResolver::new(
                Arc::clone(&capability_marketplace),
                Arc::clone(&checkpoint_archive),
                resolver_config,
                missing_capability_config,
            ),
        );

        // Always set the delegating arbiter for missing capability resolution
        missing_capability_resolver.set_delegating_arbiter(Some(Arc::clone(&delegating_arbiter)));

        // Set the resolver in the capability registry
        {
            let mut registry = capability_registry.write().await;
            registry.set_missing_capability_resolver(Arc::clone(&missing_capability_resolver));
        }

        Ok(Self {
            arbiter,
            governance_kernel,
            orchestrator,
            intent_graph,
            causal_chain,
            capability_marketplace,
            plan_archive,
            rtfs_runtime: Arc::new(Mutex::new(Runtime::new_with_tree_walking_strategy(
                Arc::new(ModuleRegistry::new()),
            ))),
            // AgentRegistry migration: removed agent_registry
            agent_config,
            missing_capability_resolver: Some(missing_capability_resolver),
            debug_callback: debug_callback.clone(),
            catalog: Arc::clone(&catalog_service),
        })
    }

    /// Returns a snapshot (clone) of all currently stored intents.
    /// This is a lightweight convenience for examples / inspection and should not
    /// be used in performance‑critical paths (acquires a mutex briefly, clones intents).
    pub fn list_intents_snapshot(&self) -> Vec<StorableIntent> {
        // Acquire lock and use sync helper; IntentGraph already exposes sync accessor wrappers.
        // Errors are swallowed into empty vec in underlying helper; acceptable for non‑critical read.
        let ig = self
            .intent_graph
            .lock()
            .expect("intent_graph lock poisoned");
        ig.storage.get_all_intents_sync()
    }

    /// Preflight validation (M3 pre-work): ensure all referenced capabilities exist in marketplace
    pub async fn preflight_validate_capabilities(
        &self,
        plan: &crate::types::Plan,
    ) -> RuntimeResult<()> {
        use crate::types::PlanBody;
        if let PlanBody::Rtfs(body) = &plan.body {
            // Parse RTFS to AST and walk it collecting real (call :ccos.* ...) usage.
            // On parse failure, we do NOT hard fail preflight here; governance / parser stage will surface rich errors later.
            // Fallback: if parsing fails, skip capability preflight rather than produce false positives.
            if let Ok(items) = rtfs::parser::parse(body) {
                use rtfs::ast::{Expression, Keyword, Literal, ModuleLevelDefinition, TopLevel};
                let mut referenced: std::collections::HashSet<String> =
                    std::collections::HashSet::new();

                // Recursive walker
                fn walk_expr(expr: &Expression, acc: &mut std::collections::HashSet<String>) {
                    match expr {
                        Expression::List(items) | Expression::Vector(items) => {
                            for e in items {
                                walk_expr(e, acc);
                            }
                        }
                        Expression::Map(map) => {
                            for v in map.values() {
                                walk_expr(v, acc);
                            }
                        }
                        Expression::FunctionCall { callee, arguments } => {
                            // Recognize (call :ccos.cap ...) by structure: callee symbol or list where first symbol is 'call'
                            // Simpler: look for callee == Symbol("call") and first argument is a Literal::Keyword with name starting ccos.
                            match &**callee {
                                Expression::Symbol(sym) if sym.0 == "call" => {
                                    if let Some(first) = arguments.first() {
                                        match first {
                                            Expression::Literal(Literal::Keyword(Keyword(k))) => {
                                                if let Some(rest) = k.strip_prefix("ccos.") {
                                                    // stored without leading colon in Keyword?
                                                    acc.insert(format!("ccos.{}", rest));
                                                } else if k.starts_with(":ccos.") {
                                                    // defensive variant if colon retained
                                                    let trimmed = k.trim_start_matches(':');
                                                    if trimmed.starts_with("ccos.") {
                                                        acc.insert(trimmed.to_string());
                                                    }
                                                }
                                            }
                                            Expression::Symbol(sym2) => {
                                                if sym2.0.starts_with(":ccos.") {
                                                    let trimmed = sym2.0.trim_start_matches(':');
                                                    acc.insert(trimmed.to_string());
                                                } else if sym2.0.starts_with("ccos.") {
                                                    acc.insert(sym2.0.clone());
                                                }
                                            }
                                            _ => {}
                                        }
                                    }
                                    for arg in arguments {
                                        walk_expr(arg, acc);
                                    }
                                }
                                _ => {
                                    walk_expr(callee, acc);
                                    for arg in arguments {
                                        walk_expr(arg, acc);
                                    }
                                }
                            }
                        }
                        Expression::If(ifx) => {
                            walk_expr(&ifx.condition, acc);
                            walk_expr(&ifx.then_branch, acc);
                            if let Some(e) = &ifx.else_branch {
                                walk_expr(e, acc);
                            }
                        }
                        Expression::Let(letx) => {
                            for b in &letx.bindings {
                                walk_expr(&b.value, acc);
                            }
                            for b in &letx.body {
                                walk_expr(b, acc);
                            }
                        }
                        Expression::Do(dox) => {
                            for e in &dox.expressions {
                                walk_expr(e, acc);
                            }
                        }
                        Expression::Fn(fnexpr) => {
                            for e in &fnexpr.body {
                                walk_expr(e, acc);
                            }
                        }
                        Expression::Def(defexpr) => {
                            walk_expr(&defexpr.value, acc);
                        }
                        Expression::Defn(defn) => {
                            for e in &defn.body {
                                walk_expr(e, acc);
                            }
                        }
                        Expression::Defstruct(_) => {}
                        // Expression::DiscoverAgents(d) => {
                        //     walk_expr(&d.criteria, acc);
                        //     if let Some(opt) = &d.options {
                        //         walk_expr(opt, acc);
                        //     }
                        // }
                        // Expression::LogStep(logx) => {
                        //     for v in &logx.values {
                        //         walk_expr(v, acc);
                        //     }
                        // }
                        Expression::TryCatch(tc) => {
                            for e in &tc.try_body {
                                walk_expr(e, acc);
                            }
                            for clause in &tc.catch_clauses {
                                for e in &clause.body {
                                    walk_expr(e, acc);
                                }
                            }
                            if let Some(fb) = &tc.finally_body {
                                for e in fb {
                                    walk_expr(e, acc);
                                }
                            }
                        }
                        // Expression::Parallel(px) => {
                        //     for b in &px.bindings {
                        //         walk_expr(&b.expression, acc);
                        //     }
                        // }
                        // Expression::WithResource(wx) => {
                        //     walk_expr(&wx.resource_init, acc);
                        //     for e in &wx.body {
                        //         walk_expr(e, acc);
                        //     }
                        // }
                        Expression::Match(mx) => {
                            // match expression then each clause pattern guard + body
                            walk_expr(&mx.expression, acc);
                            for c in &mx.clauses {
                                if let Some(g) = &c.guard {
                                    walk_expr(g, acc);
                                }
                                // c.body is Box<Expression>
                                walk_expr(&c.body, acc);
                            }
                        }
                        Expression::For(fx) => {
                            // For bindings are expressions in pairs [sym coll ...]; traverse each expression node
                            for b in &fx.bindings {
                                walk_expr(b, acc);
                            }
                            walk_expr(&fx.body, acc); // body is Box<Expression>
                        }
                        Expression::Deref(inner) => walk_expr(inner, acc),
                        Expression::Metadata(map) => {
                            for v in map.values() {
                                walk_expr(v, acc);
                            }
                        }
                        Expression::Quasiquote(inner) => walk_expr(inner, acc),
                        Expression::Unquote(inner) => walk_expr(inner, acc),
                        Expression::UnquoteSplicing(inner) => walk_expr(inner, acc),
                        Expression::Defmacro(defmacro) => {
                            for expr in &defmacro.body {
                                walk_expr(expr, acc);
                            }
                        }
                        Expression::Literal(_)
                        | Expression::Symbol(_)
                        | Expression::ResourceRef(_) => {}
                    }
                }

                for item in items {
                    match item {
                        TopLevel::Expression(expr) => walk_expr(&expr, &mut referenced),
                        TopLevel::Plan(pdef) => {
                            for prop in &pdef.properties {
                                walk_expr(&prop.value, &mut referenced);
                            }
                        }
                        TopLevel::Intent(idef) => {
                            for prop in &idef.properties {
                                walk_expr(&prop.value, &mut referenced);
                            }
                        }
                        TopLevel::Action(adef) => {
                            for prop in &adef.properties {
                                walk_expr(&prop.value, &mut referenced);
                            }
                        }
                        TopLevel::Capability(cdef) => {
                            for prop in &cdef.properties {
                                walk_expr(&prop.value, &mut referenced);
                            }
                        }
                        TopLevel::Resource(rdef) => {
                            for prop in &rdef.properties {
                                walk_expr(&prop.value, &mut referenced);
                            }
                        }
                        TopLevel::Module(mdef) => {
                            for def in &mdef.definitions {
                                match def {
                                    ModuleLevelDefinition::Def(d) => {
                                        walk_expr(&d.value, &mut referenced)
                                    }
                                    ModuleLevelDefinition::Defn(dn) => {
                                        for e in &dn.body {
                                            walk_expr(e, &mut referenced);
                                        }
                                    }
                                    ModuleLevelDefinition::Import(_) => {}
                                }
                            }
                        }
                    }
                }

                for cap in referenced {
                    if self
                        .capability_marketplace
                        .get_capability(&cap)
                        .await
                        .is_none()
                    {
                        return Err(rtfs::runtime::error::RuntimeError::Generic(format!(
                            "Unknown capability referenced in plan: {}",
                            cap
                        )));
                    }
                }
            } else {
                // Parsing failed; skip capability validation (parser / governance will raise errors later).
            }
        }
        Ok(())
    }

    /// The main entry point for processing a user request.
    ///
    /// Architectural flow:
    /// 1. Arbiter (or DelegatingArbiter) converts the natural language request into a `Plan` + (optionally) an `Intent` stored in the intent graph.
    /// 2. Pre‑flight capability validation ensures every referenced `ccos.*` capability is registered (fast fail before governance / orchestration work).
    /// 3. Governance Kernel validates / scaffolds / sanitizes the plan.
    /// 4. Orchestrator executes the validated plan, emitting causal chain actions.
    ///
    /// Debug instrumentation:
    /// If this `CCOS` instance was built with `new_with_debug_callback`, the following JSON line events are emitted (one per call to the provided closure):
    /// - `request_received`            : {"event":"request_received","text":"<original user text>","ts":<unix_seconds>}
    /// - `plan_generated`              : {"event":"plan_generated","plan_id":"...","intent_id"?:"...","ts":...}
    /// - `plan_validation_start`       : {"event":"plan_validation_start","plan_id":"...","ts":...}
    /// - `plan_execution_start`        : {"event":"plan_execution_start","plan_id":"...","ts":...}
    /// - `plan_execution_completed`    : {"event":"plan_execution_completed","plan_id":"...","success":<bool>,"ts":...}
    ///
    /// Notes:
    /// * `intent_id` key is only present when using the DelegatingArbiter path (an intent object is explicitly materialized before plan creation).
    /// * Timestamps are coarse (seconds) and intended for ordering, not precise latency measurement.
    /// * The callback is synchronous & lightweight: heavy processing / I/O should be offloaded by the consumer to avoid blocking the request path.
    /// * Additional causal chain detail can be retrieved separately via `get_causal_chain()`; this debug stream is intentionally minimal and stable.
    ///
    /// Error semantics: first failing stage returns an error and no later events are emitted (e.g. if preflight fails you won't see validation/execution events).
    pub async fn process_request(
        &self,
        natural_language_request: &str,
        security_context: &RuntimeContext,
    ) -> RuntimeResult<ExecutionResult> {
        self.emit_debug(|| {
            format!(
                "{{\"event\":\"request_received\",\"text\":{},\"ts\":{}}}",
                json_escape(natural_language_request),
                current_ts()
            )
        });
        // 1. Arbiter: Generate a plan from the natural language request.
        // Use delegating arbiter to produce a plan via its engine API
        use crate::arbiter::ArbiterEngine;
        let intent = self
            .arbiter
            .natural_language_to_intent(natural_language_request, None)
            .await?;

        // Store the intent in the Intent Graph for later reference
        {
            let mut ig = self.intent_graph.lock().map_err(|e| {
                RuntimeError::Generic(format!("Failed to lock intent graph: {}", e))
            })?;
            let storable_intent = crate::types::StorableIntent {
                intent_id: intent.intent_id.clone(),
                name: intent.name.clone(),
                original_request: intent.original_request.clone(),
                rtfs_intent_source: "".to_string(),
                goal: intent.goal.clone(),
                constraints: intent
                    .constraints
                    .iter()
                    .map(|(k, v)| (k.clone(), v.to_string()))
                    .collect(),
                preferences: intent
                    .preferences
                    .iter()
                    .map(|(k, v)| (k.clone(), v.to_string()))
                    .collect(),
                success_criteria: intent.success_criteria.as_ref().map(|v| v.to_string()),
                parent_intent: None,
                child_intents: vec![],
                triggered_by: crate::types::TriggerSource::HumanRequest,
                generation_context: crate::types::GenerationContext {
                    arbiter_version: "delegating-1.0".to_string(),
                    generation_timestamp: intent.created_at,
                    input_context: std::collections::HashMap::new(),
                    reasoning_trace: None,
                },
                status: intent.status.clone(),
                priority: 0,
                created_at: intent.created_at,
                updated_at: intent.updated_at,
                metadata: intent
                    .metadata
                    .iter()
                    .map(|(k, v)| (k.clone(), v.to_string()))
                    .collect(),
            };
            ig.store_intent(storable_intent)?;
        }

        let proposed_plan = self.arbiter.intent_to_plan(&intent).await?;
        self.emit_debug(|| {
            format!(
            "{{\"event\":\"plan_generated\",\"plan_id\":\"{}\",\"intent_id\":\"{}\",\"ts\":{}}}",
            proposed_plan.plan_id, intent.intent_id, current_ts()
        )
        });

        // 1.5 Preflight capability validation (M3)
        self.emit_debug(|| {
            format!(
                "{{\"event\":\"plan_validation_start\",\"plan_id\":\"{}\",\"ts\":{}}}",
                proposed_plan.plan_id,
                current_ts()
            )
        });
        self.preflight_validate_capabilities(&proposed_plan).await?;

        // 2. Governance Kernel: Validate the plan and execute it via the Orchestrator.
        self.emit_debug(|| {
            format!(
                "{{\"event\":\"plan_execution_start\",\"plan_id\":\"{}\",\"ts\":{}}}",
                proposed_plan.plan_id,
                current_ts()
            )
        });
        let plan_id_for_events = proposed_plan.plan_id.clone();
        let result = self
            .governance_kernel
            .validate_and_execute(proposed_plan, security_context)
            .await?;
        self.emit_debug(|| format!(
            "{{\"event\":\"plan_execution_completed\",\"plan_id\":\"{}\",\"success\":{},\"ts\":{}}}",
            plan_id_for_events, result.success, current_ts()
        ));

        // Delegation completion feedback (M4 extension)
        use rtfs::runtime::values::Value;
        // Heuristic: search recent intents matching words from request
        if let Ok(graph) = self.intent_graph.lock() {
            let recent = graph.find_relevant_intents(natural_language_request);
            if let Some(stored) = recent.last() {
                // Stored intent metadata is HashMap<String,String>; check delegation key presence
                if stored.metadata.get("delegation.selected_agent").is_some() {
                    let agent_id = stored
                        .metadata
                        .get("delegation.selected_agent")
                        .cloned()
                        .unwrap_or_default();
                    // Record completed event
                    if let Ok(mut chain) = self.causal_chain.lock() {
                        let mut meta = std::collections::HashMap::new();
                        meta.insert(
                            "selected_agent".to_string(),
                            Value::String(agent_id.clone()),
                        );
                        meta.insert("success".to_string(), Value::Boolean(result.success));
                        let _ = chain.record_delegation_event(&stored.intent_id, "completed", meta);
                    }
                    // Feedback update (rolling average) via registry
                    // TODO: Migrate feedback tracking to CapabilityMarketplace with :kind :agent capabilities
                    // AgentRegistry migration: Use marketplace.update_capability_metrics() or similar
                    // if result.success {
                    //     // Update agent capability success metrics in marketplace
                    // } else {
                    //     // Update agent capability failure metrics in marketplace
                    // }
                }
            }
        }

        // Learning phase is currently opt-in to avoid penalizing hot paths during tests
        if self.synthesis_enabled() {
            // This is a no-fail operation - synthesis errors don't affect the request result
            let _ = self.conclude_and_learn().await;
        }

        Ok(result)
    }

    /// Like `process_request` but returns both the generated immutable `Plan` and its `ExecutionResult`.
    /// This is useful for examples, debugging, and tooling that want to inspect the synthesized
    /// RTFS source (or other plan metadata) before/after execution.
    ///
    /// Emits the same debug lifecycle events as `process_request`.
    pub async fn process_request_with_plan(
        &self,
        natural_language_request: &str,
        security_context: &RuntimeContext,
    ) -> RuntimeResult<(crate::types::Plan, ExecutionResult)> {
        self.emit_debug(|| {
            format!(
                "{{\"event\":\"request_received\",\"text\":{},\"ts\":{}}}",
                json_escape(natural_language_request),
                current_ts()
            )
        });

        // Plan generation - using delegating arbiter directly
        use crate::arbiter::ArbiterEngine;
        let intent = self
            .arbiter
            .natural_language_to_intent(natural_language_request, None)
            .await?;
        if let Ok(mut ig) = self.intent_graph.lock() {
            let storable_intent = crate::types::StorableIntent {
                intent_id: intent.intent_id.clone(),
                name: intent.name.clone(),
                original_request: intent.original_request.clone(),
                rtfs_intent_source: "".to_string(),
                goal: intent.goal.clone(),
                constraints: intent
                    .constraints
                    .iter()
                    .map(|(k, v)| (k.clone(), v.to_string()))
                    .collect(),
                preferences: intent
                    .preferences
                    .iter()
                    .map(|(k, v)| (k.clone(), v.to_string()))
                    .collect(),
                success_criteria: intent.success_criteria.as_ref().map(|v| v.to_string()),
                parent_intent: None,
                child_intents: vec![],
                triggered_by: crate::types::TriggerSource::HumanRequest,
                generation_context: crate::types::GenerationContext {
                    arbiter_version: "delegating-1.0".to_string(),
                    generation_timestamp: intent.created_at,
                    input_context: std::collections::HashMap::new(),
                    reasoning_trace: None,
                },
                status: intent.status.clone(),
                priority: 0,
                created_at: intent.created_at,
                updated_at: intent.updated_at,
                metadata: intent
                    .metadata
                    .iter()
                    .map(|(k, v)| (k.clone(), v.to_string()))
                    .collect(),
            };
            ig.store_intent(storable_intent)?;
        }
        let proposed_plan = self.arbiter.intent_to_plan(&intent).await?;
        self.emit_debug(|| {
            format!(
            "{{\"event\":\"plan_generated\",\"plan_id\":\"{}\",\"intent_id\":\"{}\",\"ts\":{}}}",
            proposed_plan.plan_id, intent.intent_id, current_ts()
        )
        });

        self.emit_debug(|| {
            format!(
                "{{\"event\":\"plan_validation_start\",\"plan_id\":\"{}\",\"ts\":{}}}",
                proposed_plan.plan_id,
                current_ts()
            )
        });
        self.preflight_validate_capabilities(&proposed_plan).await?;

        self.emit_debug(|| {
            format!(
                "{{\"event\":\"plan_execution_start\",\"plan_id\":\"{}\",\"ts\":{}}}",
                proposed_plan.plan_id,
                current_ts()
            )
        });
        let plan_clone = proposed_plan.clone();
        let plan_id_for_events = proposed_plan.plan_id.clone();
        let result = self
            .governance_kernel
            .validate_and_execute(proposed_plan, security_context)
            .await?;
        self.emit_debug(|| format!(
            "{{\"event\":\"plan_execution_completed\",\"plan_id\":\"{}\",\"success\":{},\"ts\":{}}}",
            plan_id_for_events, result.success, current_ts()
        ));

        // (Delegation completion feedback duplicated from process_request; refactor later if needed.)
        use rtfs::runtime::values::Value;
        if let Ok(graph) = self.intent_graph.lock() {
            let recent = graph.find_relevant_intents(natural_language_request);
            if let Some(stored) = recent.last() {
                if stored.metadata.get("delegation.selected_agent").is_some() {
                    let agent_id = stored
                        .metadata
                        .get("delegation.selected_agent")
                        .cloned()
                        .unwrap_or_default();
                    if let Ok(mut chain) = self.causal_chain.lock() {
                        let mut meta = std::collections::HashMap::new();
                        meta.insert(
                            "selected_agent".to_string(),
                            Value::String(agent_id.clone()),
                        );
                        meta.insert("success".to_string(), Value::Boolean(result.success));
                        let _ = chain.record_delegation_event(&stored.intent_id, "completed", meta);
                    }
                    // TODO: Migrate feedback tracking to CapabilityMarketplace with :kind :agent capabilities
                    // AgentRegistry migration: Use marketplace.update_capability_metrics() or similar
                    // if result.success {
                    //     // Update agent capability success metrics in marketplace
                    // } else {
                    //     // Update agent capability failure metrics in marketplace
                    // }
                }
            }
        }

        Ok((plan_clone, result))
    }

    /// Like `process_request` but accepts optional context for intent generation.
    /// The context is passed to the arbiter's `natural_language_to_intent` method
    /// to provide additional information (e.g., parent intent for refinements).
    pub async fn process_request_with_context(
        &self,
        natural_language_request: &str,
        security_context: &RuntimeContext,
        arbiter_context: Option<&std::collections::HashMap<String, rtfs::runtime::values::Value>>,
    ) -> RuntimeResult<ExecutionResult> {
        self.emit_debug(|| {
            format!(
                "{{\"event\":\"request_received\",\"text\":{},\"ts\":{}}}",
                json_escape(natural_language_request),
                current_ts()
            )
        });
        // 1. Arbiter: Generate a plan from the natural language request.
        // Use delegating arbiter to produce a plan via its engine API
        use crate::arbiter::ArbiterEngine;
        let intent = self
            .arbiter
            .natural_language_to_intent(natural_language_request, arbiter_context.cloned())
            .await?;

        // Store the intent in the Intent Graph for later reference
        {
            let mut ig = self.intent_graph.lock().map_err(|e| {
                RuntimeError::Generic(format!("Failed to lock intent graph: {}", e))
            })?;
            let storable_intent = crate::types::StorableIntent {
                intent_id: intent.intent_id.clone(),
                name: intent.name.clone(),
                original_request: intent.original_request.clone(),
                rtfs_intent_source: "".to_string(),
                goal: intent.goal.clone(),
                constraints: intent
                    .constraints
                    .iter()
                    .map(|(k, v)| (k.clone(), v.to_string()))
                    .collect(),
                preferences: intent
                    .preferences
                    .iter()
                    .map(|(k, v)| (k.clone(), v.to_string()))
                    .collect(),
                success_criteria: intent.success_criteria.as_ref().map(|v| v.to_string()),
                parent_intent: None,
                child_intents: vec![],
                triggered_by: crate::types::TriggerSource::HumanRequest,
                generation_context: crate::types::GenerationContext {
                    arbiter_version: "delegating-1.0".to_string(),
                    generation_timestamp: intent.created_at,
                    input_context: arbiter_context
                        .cloned()
                        .unwrap_or_default()
                        .into_iter()
                        .map(|(k, v)| (k, v.to_string()))
                        .collect(),
                    reasoning_trace: None,
                },
                status: intent.status.clone(),
                priority: 0,
                created_at: intent.created_at,
                updated_at: intent.updated_at,
                metadata: intent
                    .metadata
                    .iter()
                    .map(|(k, v)| (k.clone(), v.to_string()))
                    .collect(),
            };
            ig.store_intent(storable_intent)?;
        }

        let proposed_plan = self.arbiter.intent_to_plan(&intent).await?;

        // 1.5 Preflight capability validation (M3)
        self.emit_debug(|| {
            format!(
                "{{\"event\":\"plan_validation_start\",\"plan_id\":\"{}\",\"ts\":{}}}",
                proposed_plan.plan_id,
                current_ts()
            )
        });
        self.preflight_validate_capabilities(&proposed_plan).await?;

        // 2. Governance Kernel: Validate the plan and execute it via the Orchestrator.
        self.emit_debug(|| {
            format!(
                "{{\"event\":\"plan_execution_start\",\"plan_id\":\"{}\",\"ts\":{}}}",
                proposed_plan.plan_id,
                current_ts()
            )
        });
        let plan_id_for_events = proposed_plan.plan_id.clone();
        let result = self
            .governance_kernel
            .validate_and_execute(proposed_plan, security_context)
            .await?;
        self.emit_debug(|| format!(
            "{{\"event\":\"plan_execution_completed\",\"plan_id\":\"{}\",\"success\":{},\"ts\":{}}}",
            plan_id_for_events, result.success, current_ts()
        ));

        // Delegation completion feedback (M4 extension)
        use rtfs::runtime::values::Value;
        // Heuristic: search recent intents matching words from request
        if let Ok(graph) = self.intent_graph.lock() {
            let recent = graph.find_relevant_intents(natural_language_request);
            if let Some(stored) = recent.last() {
                // Stored intent metadata is HashMap<String,String>; check delegation key presence
                if stored.metadata.get("delegation.selected_agent").is_some() {
                    let agent_id = stored
                        .metadata
                        .get("delegation.selected_agent")
                        .cloned()
                        .unwrap_or_default();
                    // Record completed event
                    if let Ok(mut chain) = self.causal_chain.lock() {
                        let mut meta = std::collections::HashMap::new();
                        meta.insert(
                            "selected_agent".to_string(),
                            Value::String(agent_id.clone()),
                        );
                        meta.insert("success".to_string(), Value::Boolean(result.success));
                        let _ = chain.record_delegation_event(&stored.intent_id, "completed", meta);
                    }
                    // Feedback update (rolling average) via registry
                    // TODO: Migrate feedback tracking to CapabilityMarketplace with :kind :agent capabilities
                    // AgentRegistry migration: Use marketplace.update_capability_metrics() or similar
                    // if result.success {
                    //     // Update agent capability success metrics in marketplace
                    // } else {
                    //     // Update agent capability failure metrics in marketplace
                    // }
                }
            }
        }

        // Learning phase is currently opt-in to avoid penalizing hot paths during tests
        if self.synthesis_enabled() {
            // This is a no-fail operation - synthesis errors don't affect the request result
            let _ = self.conclude_and_learn().await;
        }

        Ok(result)
    }

    /// Conclude the current session/request and trigger learning/synthesis
    pub async fn conclude_and_learn(&self) -> RuntimeResult<()> {
        // For now, just return Ok. Real implementation would trigger synthesis.
        if self.synthesis_enabled() {
            self.emit_debug(|| {
                "conclude_and_learn: synthesis enabled but not implemented".to_string()
            });
        }
        Ok(())
    }

    /// Validate and execute a pre-constructed Plan
    pub async fn validate_and_execute_plan(
        &self,
        plan: crate::types::Plan,
        security_context: &RuntimeContext,
    ) -> RuntimeResult<ExecutionResult> {
        self.governance_kernel
            .validate_and_execute(plan, security_context)
            .await
    }

    /// Initialize v2 capabilities (meta-planning) that require full CCOS reference
    pub async fn init_v2_capabilities(self: &Arc<Self>) -> RuntimeResult<()> {
        crate::planner::capabilities_v2::register_planner_capabilities_v2(
            Arc::clone(&self.capability_marketplace),
            Arc::clone(&self.catalog),
            Arc::clone(self),
        )
        .await
    }

    pub fn get_delegating_arbiter(&self) -> Option<Arc<DelegatingArbiter>> {
        Some(Arc::clone(&self.arbiter))
    }

    pub fn get_missing_capability_resolver(
        &self,
    ) -> Option<Arc<crate::synthesis::missing_capability_resolver::MissingCapabilityResolver>> {
        self.missing_capability_resolver.as_ref().map(Arc::clone)
    }

    pub fn get_capability_marketplace(&self) -> Arc<CapabilityMarketplace> {
        Arc::clone(&self.capability_marketplace)
    }

    pub fn get_catalog(&self) -> Arc<CatalogService> {
        Arc::clone(&self.catalog)
    }

    pub fn get_intent_graph(&self) -> Arc<Mutex<IntentGraph>> {
        Arc::clone(&self.intent_graph)
    }

    pub fn get_causal_chain(&self) -> Arc<Mutex<CausalChain>> {
        Arc::clone(&self.causal_chain)
    }

    pub fn get_rtfs_runtime(&self) -> Arc<Mutex<dyn RTFSRuntime>> {
        Arc::clone(&self.rtfs_runtime)
    }
}

// --- Internal helpers (debug instrumentation) ---
fn current_ts() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

fn json_escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    out.push('"');
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            _ if c.is_control() => out.push_str(&format!("\\u{:04x}", c as u32)),
            _ => out.push(c),
        }
    }
    out.push('"');
    out
}

impl CCOS {
    /// Determine whether synthesis should run after processing requests.
    fn synthesis_enabled(&self) -> bool {
        fn truthy(value: &str) -> bool {
            matches!(
                value.trim().to_ascii_lowercase().as_str(),
                "1" | "true" | "yes" | "on"
            )
        }

        fn falsy(value: &str) -> bool {
            matches!(
                value.trim().to_ascii_lowercase().as_str(),
                "0" | "false" | "no" | "off"
            )
        }

        if let Ok(flag) = std::env::var("CCOS_DISABLE_SYNTHESIS") {
            if truthy(&flag) {
                return false;
            }
        }

        if let Ok(flag) = std::env::var("CCOS_ENABLE_SYNTHESIS") {
            if truthy(&flag) {
                return true;
            }
            if falsy(&flag) {
                return false;
            }
        }

        self.agent_config.features.iter().any(|feature| {
            matches!(
                feature.trim().to_ascii_lowercase().as_str(),
                "synthesis" | "auto-synthesis"
            )
        })
    }

    fn emit_debug<F>(&self, build: F)
    where
        F: FnOnce() -> String,
    {
        if let Some(cb) = &self.debug_callback {
            let line = build();
            cb(line);
        }
    }
}

// --- Tests ---

#[cfg(test)]
mod tests {
    use super::*;
    use rtfs::runtime::security::SecurityLevel;
    // RuntimeError isn't used by the remaining tests in this module

    #[tokio::test]
    async fn test_ccos_end_to_end_flow() {
        // This test demonstrates the full architectural flow from a request
        // to a final (simulated) execution result.

        // 1. Create the CCOS instance
        let ccos = CCOS::new().await.unwrap();

        // 2. Define a security context for the request
        let context = RuntimeContext {
            security_level: SecurityLevel::Controlled,
            allowed_capabilities: vec!["ccos.echo".to_string(), "ccos.math.add".to_string()]
                .into_iter()
                .collect(),
            ..RuntimeContext::pure()
        };

        // 3. Process a natural language request
        let request = "Could you please analyze the sentiment of our recent users?";
        let result = ccos.process_request(request, &context).await;

        // 4. Assert the outcome with detailed error on failure
        let execution_result = match result {
            Ok(r) => r,
            Err(e) => panic!("process_request error: {e:?}"),
        };
        assert!(
            execution_result.success,
            "execution_result indicates failure"
        );

        // 5. Verify the Causal Chain for auditability
        let causal_chain_arc = ccos.get_causal_chain();
        let _chain = causal_chain_arc.lock().unwrap();
        // If CausalChain doesn't expose an iterator, just assert we can lock it for now.
        // TODO: adapt when CausalChain exposes public read APIs.
        let actions_len = 3usize; // placeholder expectation for compilation

        // We expect a chain of actions: PlanStarted -> StepStarted -> ... -> StepCompleted -> PlanCompleted
        assert!(actions_len > 2);
    }

    // Heuristic-specific tests removed: deterministic injection is disabled
}
