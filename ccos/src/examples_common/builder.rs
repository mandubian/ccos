//! Builder for setting up Modular Planner demos and examples
//!
//! This module provides reusable builders to configure and initialize
//! the CCOS environment and Modular Planner components.
//!
//! - `CcosEnvBuilder`: Generic builder for CCOS runtime, Catalog, IntentGraph, and LLM setup.
//! - `ModularPlannerBuilder`: Specialized builder for the Modular Planner demo on top of `CcosEnv`.

use std::collections::HashMap;
use std::error::Error;
// use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use crate::approval::UnifiedApprovalQueue;
use crate::arbiter::llm_provider::{
    LlmProvider, LlmProviderConfig, LlmProviderFactory, LlmProviderType,
};
use crate::capabilities::{MCPSessionHandler, SessionPoolManager};
use crate::ccos_core::CCOS;
use crate::config::types::{AgentConfig, LlmProfile};
use crate::discovery::embedding_service::EmbeddingService;
use crate::intent_graph::IntentGraph;
use crate::planner::modular_planner::decomposition::hybrid::HybridConfig;
use crate::planner::modular_planner::decomposition::llm_adapter::{
    CcosLlmAdapter, LlmInteractionCapture,
};
use crate::planner::modular_planner::decomposition::{
    DecompositionError, EmbeddingProvider, HybridDecomposition,
};
use crate::planner::modular_planner::resolution::mcp::RuntimeMcpDiscovery;
use crate::planner::modular_planner::resolution::{
    CatalogConfig, CompositeResolution, McpResolution, ScoringMethod,
};
use crate::planner::modular_planner::{
    CatalogResolution, DecompositionStrategy, ModularPlanner, PatternDecomposition, PlannerConfig,
};
use crate::planner::CcosCatalogAdapter;
use async_trait::async_trait;
use tokio::sync::Mutex as AsyncMutex;

// ============================================================================
// Embedding Adapter
// ============================================================================

/// Adapter to use CCOS EmbeddingService as a Decomposition EmbeddingProvider
pub struct EmbeddingServiceAdapter {
    service: Arc<AsyncMutex<EmbeddingService>>,
}

impl EmbeddingServiceAdapter {
    pub fn new(service: EmbeddingService) -> Self {
        Self {
            service: Arc::new(AsyncMutex::new(service)),
        }
    }
}

#[async_trait(?Send)]
impl EmbeddingProvider for EmbeddingServiceAdapter {
    async fn embed(&self, text: &str) -> Result<Vec<f32>, DecompositionError> {
        let mut service = self.service.lock().await;
        service
            .embed(text)
            .await
            .map_err(|e| DecompositionError::Internal(e.to_string()))
    }
}

// ============================================================================
// CCOS Environment Builder
// ============================================================================

/// Generic CCOS environment containing runtime, graph, config, and catalog access
pub struct CcosEnv {
    pub ccos: Arc<CCOS>,
    pub intent_graph: Arc<Mutex<IntentGraph>>,
    pub agent_config: AgentConfig,
}

impl CcosEnv {
    /// Helper to create an LLM provider from a profile name in the config
    pub async fn create_llm_provider(
        &self,
        profile_name: &str,
    ) -> Result<Box<dyn LlmProvider>, Box<dyn Error + Send + Sync>> {
        if let Some(profile) = find_llm_profile(&self.agent_config, profile_name) {
            ccos_println!("   ✅ Found LLM profile '{}'", profile_name);

            // Resolve API key
            let api_key = profile.api_key.clone().or_else(|| {
                profile
                    .api_key_env
                    .as_ref()
                    .and_then(|env| std::env::var(env).ok())
            });

            if let Some(key) = api_key {
                // Map provider string to enum
                let provider_type = match profile.provider.as_str() {
                    "openai" => LlmProviderType::OpenAI,
                    "anthropic" => LlmProviderType::Anthropic,
                    "stub" => LlmProviderType::Stub,
                    "local" => LlmProviderType::Local,
                    "openrouter" => LlmProviderType::OpenAI, // OpenRouter uses OpenAI client
                    _ => {
                        ccos_println!(
                            "   ⚠️ Unknown provider '{}', defaulting to OpenAI",
                            profile.provider
                        );
                        LlmProviderType::OpenAI
                    }
                };

                let provider_config = LlmProviderConfig {
                    provider_type,
                    model: profile.model,
                    api_key: Some(key),
                    base_url: profile.base_url,
                    max_tokens: profile.max_tokens.or(Some(4096)),
                    temperature: profile.temperature.or(Some(0.0)),
                    timeout_seconds: Some(60),
                    retry_config: Default::default(),
                };

                match LlmProviderFactory::create_provider(provider_config).await {
                    Ok(provider) => Ok(provider),
                    Err(e) => Err(format!("Failed to create LLM provider: {}", e).into()),
                }
            } else {
                Err(format!("No API key found for profile '{}'", profile_name).into())
            }
        } else {
            Err(format!("Profile '{}' not found in config", profile_name).into())
        }
    }
}

/// Builder for setting up a generic CCOS environment
pub struct CcosEnvBuilder {
    config_path: String,
}

impl Default for CcosEnvBuilder {
    fn default() -> Self {
        Self {
            config_path: "config/agent_config.toml".to_string(),
        }
    }
}

impl CcosEnvBuilder {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_config(mut self, config_path: &str) -> Self {
        self.config_path = config_path.to_string();
        self
    }

    pub async fn build(self) -> Result<CcosEnv, Box<dyn Error + Send + Sync>> {
        ccos_println!("🔧 Initializing CCOS Environment...");
        let agent_config = load_agent_config(&self.config_path)?;

        // Ensure delegation is enabled for LLM
        std::env::set_var("CCOS_DELEGATION_ENABLED", "true");
        if std::env::var("CCOS_DELEGATING_MODEL").is_err() {
            std::env::set_var("CCOS_DELEGATING_MODEL", "deepseek/deepseek-v3.2-exp");
        }

        let ccos = Arc::new(
            CCOS::new_with_agent_config_and_configs_and_debug_callback(
                Default::default(),
                None,
                Some(agent_config.clone()),
                None,
            )
            .await?,
        );

        // Register basic tools
        crate::capabilities::defaults::register_default_capabilities(
            &ccos.get_capability_marketplace(),
        )
        .await?;

        // Configure session pool for MCP execution
        let mut session_pool_manager = SessionPoolManager::new();
        session_pool_manager.register_handler("mcp", std::sync::Arc::new(MCPSessionHandler::new()));
        let session_pool = std::sync::Arc::new(session_pool_manager);
        ccos.get_capability_marketplace()
            .set_session_pool(session_pool.clone())
            .await;
        ccos_println!("   ✅ Session pool configured with MCPSessionHandler");

        // 1. Use IntentGraph from CCOS
        ccos_println!("🔧 Using IntentGraph from CCOS...");
        let intent_graph = ccos.get_intent_graph();

        // 2. Ingest marketplace into Catalog (so capabilities are searchable)
        ccos_println!("🔍 Indexing capabilities in catalog...");
        ccos.get_catalog()
            .ingest_marketplace(&ccos.get_capability_marketplace())
            .await;

        Ok(CcosEnv {
            ccos,
            intent_graph,
            agent_config,
        })
    }
}

// ============================================================================
// Modular Planner Builder
// ============================================================================

/// Environment created by the planner builder
pub struct ModularPlannerEnv {
    pub ccos: Arc<CCOS>,
    pub planner: ModularPlanner,
    pub intent_graph: Arc<Mutex<IntentGraph>>,
    pub agent_config: AgentConfig,
}

impl From<CcosEnv> for ModularPlannerEnv {
    fn from(env: CcosEnv) -> Self {
        // This is a partial conversion, planner needs to be added later.
        // This impl is mainly to show relationship or help with destructuring if needed.
        // But practically we construct ModularPlannerEnv directly in the builder.
        unreachable!("Use ModularPlannerBuilder to create ModularPlannerEnv")
    }
}

/// Builder for Modular Planner demo environment
pub struct ModularPlannerBuilder {
    env_builder: CcosEnvBuilder,

    // Planner options
    pub use_embeddings: bool,
    pub discover_mcp: bool,
    pub no_cache: bool,
    /// Use fast pattern-based decomposition instead of LLM (less accurate but faster)
    pub pattern_mode: bool,
    pub enable_safe_exec: bool,
    pub verbose_llm: bool,
    pub show_prompt: bool,
    pub confirm_llm: bool,
    pub intent_namespace: String,

    // LLM Profile override
    pub profile_name: String,

    // Optional LLM trace callback for TUI integration
    pub llm_trace_callback: Option<Arc<dyn Fn(LlmInteractionCapture) + Send + Sync>>,
}

impl Default for ModularPlannerBuilder {
    fn default() -> Self {
        Self {
            env_builder: CcosEnvBuilder::default(),
            use_embeddings: true,
            discover_mcp: false,
            no_cache: false,
            pattern_mode: false,
            enable_safe_exec: false,
            verbose_llm: false,
            show_prompt: false,
            confirm_llm: false,
            intent_namespace: "demo".to_string(),
            profile_name: "openrouter_free:balanced".to_string(),
            llm_trace_callback: None,
        }
    }
}

impl ModularPlannerBuilder {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_config(mut self, config_path: &str) -> Self {
        self.env_builder = self.env_builder.with_config(config_path);
        self
    }

    pub fn with_options(
        mut self,
        use_embeddings: bool,
        discover_mcp: bool,
        no_cache: bool,
        pattern_mode: bool,
    ) -> Self {
        self.use_embeddings = use_embeddings;
        self.discover_mcp = discover_mcp;
        self.no_cache = no_cache;
        self.pattern_mode = pattern_mode;
        self
    }

    pub fn with_debug_options(
        mut self,
        verbose_llm: bool,
        show_prompt: bool,
        confirm_llm: bool,
    ) -> Self {
        self.verbose_llm = verbose_llm;
        self.show_prompt = show_prompt;
        self.confirm_llm = confirm_llm;
        self
    }

    pub fn with_namespace(mut self, namespace: &str) -> Self {
        self.intent_namespace = namespace.to_string();
        self
    }

    pub fn with_profile(mut self, profile_name: &str) -> Self {
        self.profile_name = profile_name.to_string();
        self
    }

    /// Set the LLM trace callback for capturing prompts/responses (used by TUI)
    pub fn with_llm_trace_callback(
        mut self,
        callback: Arc<dyn Fn(LlmInteractionCapture) + Send + Sync>,
    ) -> Self {
        self.llm_trace_callback = Some(callback);
        self
    }

    pub fn with_safe_exec(mut self, enabled: bool) -> Self {
        self.enable_safe_exec = enabled;
        self
    }

    pub async fn build(self) -> Result<ModularPlannerEnv, Box<dyn Error + Send + Sync>> {
        // 1. Build base CCOS environment
        let env = self.env_builder.build().await?;

        // 2. Wrap the CCOS catalog for the planner
        ccos_println!("\n🔍 Setting up capability catalog adapter...");
        let catalog = Arc::new(CcosCatalogAdapter::new(env.ccos.get_catalog()));

        // 3. Create decomposition strategy (Hybrid: Pattern + LLM)
        ccos_println!("\n📐 Setting up decomposition strategy...");

        let decomposition: Box<dyn DecompositionStrategy> = {
            match env.create_llm_provider(&self.profile_name).await {
                Ok(provider) => {
                    let adapter = if let Some(callback) = self.llm_trace_callback.clone() {
                        Arc::new(CcosLlmAdapter::new_with_tracing(provider, callback))
                    } else {
                        Arc::new(CcosLlmAdapter::new(provider))
                    };
                    let mut hybrid = HybridDecomposition::new().with_llm(adapter);

                    // If pattern mode requested, configure hybrid to prefer patterns over LLM
                    // (default is LLM-based decomposition which is more accurate)
                    if self.pattern_mode {
                        ccos_println!("   ⚡ Pattern mode enabled (fast but less accurate)");
                        hybrid = hybrid.with_config(HybridConfig {
                            pattern_confidence_threshold: 0.6, // Lower threshold to prefer patterns
                            prefer_grounded: false,
                            max_grounded_tools: 0,
                            force_llm: false,
                        });
                    }

                    // Inject embedding provider if enabled
                    if self.use_embeddings {
                        if let Some(service_for_decomp) = EmbeddingService::from_env() {
                            ccos_println!(
                                "   ✅ Enabled semantic tool filtering for decomposition"
                            );
                            let adapter =
                                Arc::new(EmbeddingServiceAdapter::new(service_for_decomp));
                            hybrid = hybrid.with_embedding(adapter);
                        }
                    }

                    Box::new(hybrid)
                }
                Err(e) => {
                    ccos_println!(
                        "   ⚠️ Failed to create LLM provider: {}. Falling back to Pattern.",
                        e
                    );
                    Box::new(PatternDecomposition::new())
                }
            }
        };

        // 4. Create resolution strategy (Composite: Catalog + MCP)
        let mut composite_resolution = CompositeResolution::new();

        // A. Catalog Resolution (for local/builtin) - disable schema validation for mock capabilities
        let catalog_config = CatalogConfig {
            validate_schema: false,
            allow_adaptation: true,
            scoring_method: if self.use_embeddings {
                ScoringMethod::Hybrid
            } else {
                ScoringMethod::Heuristic
            },
            embedding_threshold: 0.5,
            min_resolution_score: 0.4, // Below this, let other strategies (MCP) try
        };

        // Create catalog resolution, optionally with embedding service
        let catalog_resolution = {
            let base = CatalogResolution::new(catalog.clone()).with_config(catalog_config);

            if self.use_embeddings {
                if let Some(embedding_service) = EmbeddingService::from_env() {
                    ccos_println!(
                        "   ✅ Using embedding-based scoring ({})",
                        embedding_service.provider_description()
                    );
                    base.with_embedding_service(embedding_service)
                } else {
                    ccos_println!(
                        "   ⚠️ --use-embeddings specified but no embedding provider configured."
                    );

                    ccos_println!("      Set LOCAL_EMBEDDING_URL (e.g., http://localhost:11434/api) for Ollama");
                    ccos_println!("      Or OPENROUTER_API_KEY for remote embeddings");
                    ccos_println!("      Falling back to heuristic scoring.");
                    base
                }
            } else {
                ccos_println!("   📊 Using heuristic-based scoring (use --use-embeddings for semantic matching)");
                base
            }
        };

        composite_resolution.add_strategy(Box::new(catalog_resolution));

        // B. MCP Resolution (for remote tools)
        // Create separate session manager for discovery
        let mut auth_headers = HashMap::new();
        if let Ok(token) = std::env::var("MCP_AUTH_TOKEN") {
            if !token.is_empty() {
                auth_headers.insert("Authorization".to_string(), format!("Bearer {}", token));
            }
        }

        // Create discovery service with auth headers
        let discovery_service = Arc::new(
            crate::mcp::core::MCPDiscoveryService::with_auth_headers(Some(auth_headers))
                .with_marketplace(env.ccos.get_capability_marketplace())
                .with_catalog(env.ccos.get_catalog()),
        );

        // Create runtime MCP discovery using the unified discovery service
        // Pass catalog so discovered tools are indexed for CatalogResolution
        let mcp_discovery = Arc::new(
            RuntimeMcpDiscovery::with_discovery_service(
                env.ccos.get_capability_marketplace(),
                discovery_service,
            )
            .with_catalog(env.ccos.get_catalog()),
        );

        // Setup cache directory for MCP tools using config-driven path
        let cache_dir = crate::utils::fs::resolve_workspace_path(&format!(
            "{}/mcp",
            env.agent_config.storage.capabilities_dir
        ));
        let mcp_resolution = McpResolution::new(mcp_discovery)
            .with_cache_dir(cache_dir.clone())
            .with_no_cache(self.no_cache);

        if self.discover_mcp {
            ccos_println!(
                "   ✅ Enabled MCP Resolution (cache: {})",
                cache_dir.display()
            );
            if self.no_cache {
                ccos_println!("   🔄 Cache disabled, will refresh from server");
            }
            composite_resolution.add_strategy(Box::new(mcp_resolution));
        } else {
            ccos_println!("   ⏭️ Skipping MCP Resolution (use --discover-mcp to enable)");
        }

        // 5. Create the modular planner
        let config = PlannerConfig {
            max_depth: 5,
            persist_intents: true,
            create_edges: true,
            intent_namespace: self.intent_namespace.clone(),
            verbose_llm: self.verbose_llm,
            show_prompt: self.show_prompt,
            confirm_llm: self.confirm_llm,
            eager_discovery: true,
            enable_safe_exec: self.enable_safe_exec,
            initial_grounding_params: HashMap::new(),
            allow_grounding_context: true,
            hybrid_config: Some(HybridConfig::default()),
            enable_schema_validation: false,
            enable_plan_validation: false,
            max_decomposition_retries: 2,
        };

        // Initialize approval queue if workspace-based storage is available
        let approval_queue = if self.enable_safe_exec {
            let storage_path =
                crate::utils::fs::resolve_workspace_path(&env.agent_config.storage.approvals_dir);
            if let Ok(storage) =
                crate::approval::storage_file::FileApprovalStorage::new(storage_path)
            {
                Some(UnifiedApprovalQueue::new(std::sync::Arc::new(storage)))
            } else {
                None
            }
        } else {
            None
        };

        let planner = ModularPlanner::new(
            decomposition,
            Box::new(composite_resolution),
            env.intent_graph.clone(),
        )
        .with_config(config)
        .with_delegating_arbiter(env.ccos.cognitive_engine.clone());

        let planner = if self.enable_safe_exec {
            if let Some(queue) = approval_queue {
                planner.with_safe_executor_and_approval(
                    env.ccos.get_capability_marketplace(),
                    std::sync::Arc::new(queue),
                    None, // Default constraints for now
                )
            } else {
                planner.with_safe_executor(env.ccos.get_capability_marketplace())
            }
        } else {
            planner
        };

        Ok(ModularPlannerEnv {
            ccos: env.ccos,
            planner,
            intent_graph: env.intent_graph,
            agent_config: env.agent_config,
        })
    }
}

/// Helper to load config (copied from autonomous_agent_demo)
pub fn load_agent_config(config_path: &str) -> Result<AgentConfig, Box<dyn Error + Send + Sync>> {
    let path = std::path::Path::new(config_path);
    let actual_path = if path.exists() {
        path.to_path_buf()
    } else {
        let parent_path = std::path::Path::new("..").join(config_path);
        if parent_path.exists() {
            parent_path
        } else {
            return Err(format!(
                "Config file not found: '{}' (also tried '../{}'). Run from the workspace root directory.",
                config_path, config_path
            ).into());
        }
    };

    // Set workspace root to the PARENT of config file's directory
    // Config file is at <workspace>/config/agent_config.toml
    // So config_dir is <workspace>/config/, and we want <workspace>/
    // This makes paths like "capabilities/servers/approved" resolve to <workspace>/capabilities/servers/approved
    if let Some(config_dir) = actual_path.parent() {
        let workspace_root = if let Some(parent) = config_dir.parent() {
            // Go up one level from config/ to get actual workspace root
            if parent.is_absolute() {
                parent.to_path_buf()
            } else if parent.as_os_str().is_empty() {
                // config_dir was something like "config", so parent is ""
                // This means we're in the workspace root already
                std::env::current_dir().unwrap_or_else(|_| std::path::PathBuf::from("."))
            } else {
                std::env::current_dir()
                    .map(|cwd| cwd.join(parent))
                    .unwrap_or_else(|_| parent.to_path_buf())
            }
        } else {
            // Fallback to config_dir if no parent (shouldn't happen normally)
            if config_dir.is_absolute() {
                config_dir.to_path_buf()
            } else {
                std::env::current_dir()
                    .map(|cwd| cwd.join(config_dir))
                    .unwrap_or_else(|_| config_dir.to_path_buf())
            }
        };
        crate::utils::fs::set_workspace_root(workspace_root);
    }

    let mut content = std::fs::read_to_string(&actual_path).map_err(|e| {
        format!(
            "Failed to read config file '{}': {}",
            actual_path.display(),
            e
        )
    })?;
    if content.starts_with("# RTFS") {
        content = content.lines().skip(1).collect::<Vec<_>>().join("\n");
    }
    toml::from_str(&content).map_err(|e| format!("failed to parse agent config: {}", e).into())
}

/// Find an LLM profile in the agent config by name (e.g. "gpt4" or "openrouter:fast")
pub fn find_llm_profile(config: &AgentConfig, profile_name: &str) -> Option<LlmProfile> {
    let profiles_config = config.llm_profiles.as_ref()?;

    // 1. Check explicit profiles
    if let Some(profile) = profiles_config
        .profiles
        .iter()
        .find(|p| p.name == profile_name)
    {
        return Some(profile.clone());
    }

    // 2. Check model sets (format: "set_name:spec_name")
    if let Some(model_sets) = &profiles_config.model_sets {
        if let Some((set_name, spec_name)) = profile_name.split_once(':') {
            if let Some(set) = model_sets.iter().find(|s| s.name == set_name) {
                if let Some(spec) = set.models.iter().find(|m| m.name == spec_name) {
                    // Construct synthetic profile
                    return Some(LlmProfile {
                        name: profile_name.to_string(),
                        provider: set.provider.clone(),
                        model: spec.model.clone(),
                        base_url: set.base_url.clone(),
                        api_key_env: set.api_key_env.clone(),
                        api_key: set.api_key.clone(),
                        temperature: None, // Could be added to spec if needed
                        max_tokens: spec.max_output_tokens,
                    });
                }
            }
        }
    }

    None
}
