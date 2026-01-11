//! Plan operations - pure logic functions for planning
//!
//! This module implements the core logic for the `ccos plan` command family:
//! - `create_plan`: Generates a plan from a natural language goal using LLM
//! - `execute_plan`: Executes an RTFS plan using the CCOS runtime
//! - `validate_plan`: Validates an RTFS plan (syntax + capability availability)
//! - `repair_plan`: Attempts to fix a failing plan using LLM

use crate::arbiter::llm_provider::{LlmProviderConfig, LlmProviderFactory, LlmProviderType};
use crate::archivable_types::ArchivablePlan;
use crate::capabilities::{MCPSessionHandler, SessionPoolManager};
use crate::capability_marketplace::CapabilityMarketplace;
use crate::examples_common::builder::load_agent_config;
use crate::mcp::core::MCPDiscoveryService;
use crate::orchestrator::Orchestrator;
use crate::plan_archive::PlanArchive;
use crate::planner::modular_planner::decomposition::hybrid::HybridConfig;
use crate::planner::modular_planner::decomposition::llm_adapter::CcosLlmAdapter;
use crate::planner::modular_planner::decomposition::{HybridDecomposition, PatternDecomposition};
use crate::planner::modular_planner::resolution::mcp::RuntimeMcpDiscovery;
use crate::planner::modular_planner::resolution::{
    CatalogResolution, CompositeResolution, McpResolution,
};
use crate::planner::modular_planner::{DecompositionStrategy, ModularPlanner, PlannerConfig};
use crate::planner::CcosCatalogAdapter;
use crate::types::{Plan, PlanBody};
use crate::CCOS;
use rtfs::runtime::error::{RuntimeError, RuntimeResult};
use rtfs::runtime::security::RuntimeContext;
use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::Path;
use std::sync::Arc;

fn maybe_set_capability_storage_from_config(config_path: &str) {
    // Set workspace root to the directory containing the config file
    if let Some(parent) = Path::new(config_path).parent() {
        if !parent.as_os_str().is_empty() {
            crate::utils::fs::set_workspace_root(parent.to_path_buf());
        }
    }

    // Respect explicit env override
    if std::env::var("CCOS_CAPABILITY_STORAGE").is_ok() {
        return;
    }

    if let Ok(contents) = fs::read_to_string(config_path) {
        if let Ok(value) = toml::from_str::<toml::Value>(&contents) {
            if let Some(dir) = value
                .get("mcp_discovery")
                .and_then(|v| v.get("export_directory"))
                .and_then(|v| v.as_str())
            {
                unsafe { std::env::set_var("CCOS_CAPABILITY_STORAGE", dir) };
            }
        }
    }
}

/// Options for plan creation
#[derive(Debug, Clone)]
pub struct CreatePlanOptions {
    /// Don't execute, just show the plan
    pub dry_run: bool,
    /// Save plan to file
    pub save_to: Option<String>,
    /// Show verbose output (LLM prompts, etc.)
    pub verbose: bool,
    /// Skip capability validation
    pub skip_validation: bool,
    /// Enable safe execution during planning
    pub enable_safe_exec: bool,
    /// Allow grounding data to be pushed into runtime context
    pub allow_grounding_context: bool,
    /// Seed grounding params
    pub grounding_params: std::collections::HashMap<String, String>,
    /// Force LLM decomposition (skip pattern path)
    pub force_llm: bool,
}

impl Default for CreatePlanOptions {
    fn default() -> Self {
        Self {
            dry_run: false,
            save_to: None,
            verbose: false,
            skip_validation: false,
            enable_safe_exec: false,
            allow_grounding_context: true,
            grounding_params: std::collections::HashMap::new(),
            force_llm: false,
        }
    }
}

/// Options for plan execution
#[derive(Debug, Clone, Default)]
pub struct ExecutePlanOptions {
    /// Maximum repair attempts on failure
    pub max_repair_attempts: usize,
    /// Show verbose output
    pub verbose: bool,
}

/// Result of plan creation with metadata
#[derive(Debug)]
pub struct CreatePlanResult {
    /// Generated RTFS code
    pub rtfs_code: String,
    /// Validation issues (if any)
    pub validation_issues: Vec<String>,
    /// Whether all capabilities were resolved
    pub all_resolved: bool,
    /// Unresolved capability IDs
    pub unresolved_capabilities: Vec<String>,
    /// Plan ID if the planner archived the plan
    pub plan_id: Option<String>,
    /// Content-addressable hash returned by the archive (if persisted)
    pub archive_hash: Option<String>,
}

/// Create plan from goal using LLM
pub async fn create_plan(goal: String) -> RuntimeResult<String> {
    let options = CreatePlanOptions::default();
    let result = create_plan_with_options(goal, options).await?;
    Ok(result.rtfs_code)
}

/// Create plan from goal with options
pub async fn create_plan_with_options(
    goal: String,
    options: CreatePlanOptions,
) -> RuntimeResult<CreatePlanResult> {
    println!("🧠 Generating plan for goal: \"{}\"...", goal);

    // Load agent config from config file (if available)
    // Try local config/ first, then parent ../config/ (for when running from crate dir)
    let config_path = if std::path::Path::new("config/agent_config.toml").exists() {
        "config/agent_config.toml"
    } else if std::path::Path::new("../config/agent_config.toml").exists() {
        "../config/agent_config.toml"
    } else {
        "config/agent_config.toml" // default fallback
    };

    maybe_set_capability_storage_from_config(config_path);
    let mut agent_config = match load_agent_config(config_path) {
        Ok(cfg) => {
            println!("✅ Loaded agent configuration from {}", config_path);
            Some(cfg)
        }
        Err(e) => {
            println!(
                "ℹ️  Could not load agent config from {} (using defaults): {}",
                config_path, e
            );
            None
        }
    };

    // Force verbose logging for missing capability resolver if --verbose is set
    if options.verbose {
        if let Some(ref mut config) = agent_config {
            config.missing_capabilities.verbose_logging = Some(true);
        } else {
            // Create default config with verbose logging
            let mut config = crate::config::types::AgentConfig::default();
            config.missing_capabilities.verbose_logging = Some(true);
            agent_config = Some(config);
        }
    }

    let mut llm_config = get_llm_config_from_env()?;

    // Override LLM config from agent config if available
    if let Some(config) = &agent_config {
        if let Some(profiles) = &config.llm_profiles {
            if let Some(sets) = &profiles.model_sets {
                for set in sets {
                    if let Some(default_model) = &set.default {
                        if let Some(model_spec) =
                            set.models.iter().find(|m| &m.name == default_model)
                        {
                            println!(
                                "ℹ️  Using default LLM profile from config: {}/{}",
                                set.name, model_spec.name
                            );

                            let provider_type = match set.provider.as_str() {
                                "openai" => LlmProviderType::OpenAI,
                                "anthropic" => LlmProviderType::Anthropic,
                                "stub" => LlmProviderType::Stub,
                                "local" => LlmProviderType::Local,
                                "openrouter" => LlmProviderType::OpenAI,
                                _ => LlmProviderType::OpenAI,
                            };

                            let api_key = if let Some(env_var) = &set.api_key_env {
                                std::env::var(env_var).ok()
                            } else {
                                set.api_key.clone()
                            };

                            llm_config = LlmProviderConfig {
                                provider_type,
                                model: model_spec.model.clone(),
                                api_key,
                                base_url: set.base_url.clone(),
                                max_tokens: model_spec.max_output_tokens,
                                temperature: None,
                                timeout_seconds: None,
                                retry_config: Default::default(),
                            };
                            break;
                        }
                    }
                }
            }
        }
    }

    // Initialize full CCOS runtime so we have catalog, marketplace and governance wired.
    // Pass agent config if available so LLM provider is properly configured.
    let ccos = Arc::new(
        CCOS::new_with_agent_config_and_configs_and_debug_callback(
            Default::default(),
            None,
            agent_config.clone(),
            None,
        )
        .await?,
    );
    let marketplace = ccos.get_capability_marketplace();

    // Ensure native CLI capabilities plus approved MCP capabilities are registered.
    println!("🔍 Registering native capabilities...");
    crate::ops::native::register_native_capabilities(&marketplace).await?;
    crate::mcp::capabilities::register_mcp_capabilities(&marketplace).await?;
    println!("🔍 LOAD_APPROVED START");
    load_approved_capabilities(&marketplace).await?;
    println!("🔍 LOAD_GENERATED START");
    load_generated_capabilities(&marketplace).await?;

    // Keep catalog in sync so planner queries see the latest capabilities.
    ccos.get_catalog().ingest_marketplace(&marketplace).await;

    configure_mcp_session_pool(&marketplace).await?;

    // Build modular planner with hybrid decomposition + catalog/MCP resolution.
    let mut planner =
        build_cli_modular_planner(ccos.clone(), &llm_config, options.verbose, &options).await?;

    let plan_result = planner
        .plan(&goal)
        .await
        .map_err(|e| RuntimeError::Generic(format!("Planner failed: {}", e)))?;

    let rtfs_code = plan_result.rtfs_plan.clone();
    println!("{}", rtfs_code);

    if let Some(plan_id) = &plan_result.plan_id {
        if let Some(hash) = &plan_result.archive_hash {
            if let Some(path) = &plan_result.archive_path {
                println!(
                    "💾 Plan archived as {} (hash {}) at {}",
                    plan_id,
                    hash,
                    path.display()
                );
            } else {
                println!("💾 Plan archived as {} (hash {})", plan_id, hash);
            }
        } else {
            println!("💾 Plan archived as {}", plan_id);
        }
    }

    // Validate capability references unless explicitly skipped.
    let (validation_issues, all_resolved, unresolved_capabilities) = if !options.skip_validation {
        validate_capabilities_in_code(&rtfs_code, &marketplace).await
    } else {
        (vec![], true, vec![])
    };

    if !validation_issues.is_empty() {
        println!(
            "\n⚠️  {} capability(ies) not found:",
            validation_issues.len()
        );
        for issue in &validation_issues {
            println!("   • {}", issue.replace("Capability not found: ", ""));
        }
        println!();
    }

    if let Some(path) = &options.save_to {
        std::fs::write(path, &rtfs_code).map_err(|e| {
            RuntimeError::Generic(format!("Failed to save plan to {}: {}", path, e))
        })?;
        println!("💾 Saved plan to: {}", path);
    }

    if options.dry_run {
        println!("\n📋 Generated Plan (dry-run):\n");
        println!("{}", rtfs_code);
    }

    if options.verbose {
        println!(
            "\n🔍 Planner created {} intents and resolved {} capabilities.",
            plan_result.intent_ids.len(),
            plan_result.resolutions.len()
        );
    }

    Ok(CreatePlanResult {
        rtfs_code,
        validation_issues,
        all_resolved,
        unresolved_capabilities,
        plan_id: plan_result.plan_id,
        archive_hash: plan_result.archive_hash,
    })
}

async fn configure_mcp_session_pool(marketplace: &Arc<CapabilityMarketplace>) -> RuntimeResult<()> {
    let mut session_pool = SessionPoolManager::new();
    session_pool.register_handler("mcp", Arc::new(MCPSessionHandler::new()));
    let pool = Arc::new(session_pool);
    marketplace.set_session_pool(pool).await;
    Ok(())
}

async fn build_cli_modular_planner(
    ccos: Arc<CCOS>,
    llm_config: &LlmProviderConfig,
    verbose: bool,
    options: &CreatePlanOptions,
) -> RuntimeResult<ModularPlanner> {
    let intent_graph = ccos.get_intent_graph();

    // Resolution strategies (Catalog + MCP).
    let catalog_adapter = Arc::new(CcosCatalogAdapter::new(ccos.get_catalog()));
    let mut composite_resolution = CompositeResolution::new();
    composite_resolution.add_strategy(Box::new(CatalogResolution::new(catalog_adapter)));

    let discovery_service = Arc::new(
        MCPDiscoveryService::with_auth_headers(mcp_auth_headers())
            .with_marketplace(ccos.get_capability_marketplace())
            .with_catalog(ccos.get_catalog()),
    );

    let mcp_discovery = Arc::new(
        RuntimeMcpDiscovery::with_discovery_service(
            ccos.get_capability_marketplace(),
            discovery_service,
        )
        .with_catalog(ccos.get_catalog()),
    );
    composite_resolution.add_strategy(Box::new(McpResolution::new(mcp_discovery)));

    // Decomposition strategy (Hybrid with LLM fallback, pattern if LLM missing).
    let decomposition: Box<dyn DecompositionStrategy> = match LlmProviderFactory::create_provider(
        llm_config.clone(),
    )
    .await
    {
        Ok(provider) => {
            let adapter = Arc::new(CcosLlmAdapter::new(provider));
            let mut hybrid = HybridDecomposition::new().with_llm(adapter);
            if options.force_llm {
                hybrid = hybrid.with_config(HybridConfig {
                    force_llm: true,
                    ..HybridConfig::default()
                });
            }
            Box::new(hybrid)
        }
        Err(e) => {
            println!(
                    "⚠️  Failed to initialize planner LLM provider: {}. Falling back to pattern-only decomposition.",
                    e
                );
            Box::new(PatternDecomposition::new())
        }
    };

    let config = PlannerConfig {
        intent_namespace: "cli".to_string(),
        verbose_llm: verbose,
        show_prompt: verbose,
        eager_discovery: true,
        enable_safe_exec: options.enable_safe_exec,
        allow_grounding_context: options.allow_grounding_context,
        initial_grounding_params: options.grounding_params.clone(),
        hybrid_config: Some(HybridConfig {
            force_llm: options.force_llm,
            ..HybridConfig::default()
        }),
        ..PlannerConfig::default()
    };

    let mut planner =
        ModularPlanner::new(decomposition, Box::new(composite_resolution), intent_graph)
            .with_config(config)
            .with_delegating_arbiter(ccos.cognitive_engine.clone());

    if options.enable_safe_exec {
        planner = planner.with_safe_executor(ccos.get_capability_marketplace());
        println!("🛡️  Safe exec enabled: executor wired to marketplace");
    }

    if let Some(resolver) = ccos.get_missing_capability_resolver() {
        planner = planner.with_missing_capability_resolver(resolver);
        if verbose {
            println!("🔍 Missing capability resolver wired: unresolved data/output intents will trigger synthesis");
        }
    } else if verbose {
        println!(
            "⚠️ Missing capability resolver unavailable; unresolved intents will only enqueue placeholders"
        );
    }

    Ok(planner)
}

fn mcp_auth_headers() -> Option<HashMap<String, String>> {
    if let Ok(token) = std::env::var("MCP_AUTH_TOKEN") {
        if !token.is_empty() {
            let mut headers = HashMap::new();
            headers.insert("Authorization".to_string(), format!("Bearer {}", token));
            return Some(headers);
        }
    }
    None
}

/// Execute a plan (RTFS string or file path)
pub async fn execute_plan(plan_input: String) -> RuntimeResult<String> {
    let options = ExecutePlanOptions::default();
    execute_plan_with_options(plan_input, options).await
}

/// Execute a plan with options (including repair loop)
pub async fn execute_plan_with_options(
    plan_input: String,
    options: ExecutePlanOptions,
) -> RuntimeResult<String> {
    // 1. Resolve plan content
    let resolved = resolve_plan_content(&plan_input)?;
    let mut content = resolved.content.clone();

    // 2. Initialize CCOS runtime
    let config_path = if std::path::Path::new("config/agent_config.toml").exists() {
        "config/agent_config.toml"
    } else if std::path::Path::new("../config/agent_config.toml").exists() {
        "../config/agent_config.toml"
    } else {
        "config/agent_config.toml" // default fallback
    };

    maybe_set_capability_storage_from_config(config_path);
    let agent_config = match load_agent_config(config_path) {
        Ok(cfg) => Some(cfg),
        Err(_) => None,
    };

    println!("🚀 Initializing CCOS runtime...");
    let ccos = Arc::new(
        CCOS::new_with_agent_config_and_configs_and_debug_callback(
            Default::default(),
            None,
            agent_config.clone(),
            None,
        )
        .await?,
    );

    // 3. Create execution context
    let context = RuntimeContext::full();

    // Register native capabilities (ccos.cli.*) so they can be used in the plan
    let marketplace = ccos.get_capability_marketplace();
    crate::ops::native::register_native_capabilities(&marketplace).await?;
    crate::mcp::capabilities::register_mcp_capabilities(&marketplace).await?;
    load_approved_capabilities(&marketplace).await?;
    load_generated_capabilities(&marketplace).await?;

    // 4. Execute with repair loop
    let mut attempts = 0;
    let max_attempts = options.max_repair_attempts.max(1);
    let mut last_error: Option<String> = None;

    while attempts < max_attempts {
        attempts += 1;

        // Create Plan object
        let mut plan = if let Some(base) = resolved.plan.as_ref() {
            let mut plan = base.clone();
            plan.body = PlanBody::Rtfs(content.clone());
            plan
        } else {
            // Try to parse as (plan ...) expression to extract metadata
            if let Ok(expr) = rtfs::parser::parse_expression(content.trim()) {
                println!("[PlanOps] Successfully parsed RTFS expression");
                match crate::rtfs_bridge::extractors::extract_plan_from_rtfs(&expr) {
                    Ok(extracted) => {
                        println!("[PlanOps] Successfully extracted plan metadata");
                        extracted
                    }
                    Err(e) => {
                        println!("[PlanOps] Failed to extract plan from RTFS: {:?}", e);
                        Plan::new_rtfs(content.clone(), vec![])
                    }
                }
            } else {
                println!("[PlanOps] Failed to parse RTFS expression");
                // Print parse error if we could capture it, but parse_expression returns Result
                if let Err(e) = rtfs::parser::parse_expression(content.trim()) {
                    println!("[PlanOps] Parse error: {:?}", e);
                }
                Plan::new_rtfs(content.clone(), vec![])
            }
        };

        // If plan has a goal in metadata/annotations but no intent, create a transient intent
        if plan.intent_ids.is_empty() {
            if let Some(goal_val) = plan.annotations.get("goal") {
                if let rtfs::runtime::values::Value::String(goal) = goal_val {
                    let mut intent = crate::types::StorableIntent::new(goal.clone());
                    intent.name = Some(
                        plan.name
                            .clone()
                            .unwrap_or_else(|| "Transient Plan Intent".to_string()),
                    );
                    let intent_id = intent.intent_id.clone();

                    if let Ok(mut graph) = ccos.get_intent_graph().lock() {
                        if let Err(e) = graph.store_intent(intent.clone()) {
                            eprintln!("⚠️ Failed to store transient intent: {}", e);
                        } else {
                            println!(
                                "🎯 Created transient intent '{}' for goal: {}",
                                intent_id, goal
                            );
                            plan.intent_ids.push(intent_id);
                        }
                    }
                }
            }
        }

        // Execute
        if attempts == 1 {
            println!("▶️  Executing plan...");
        } else {
            println!("▶️  Executing plan (attempt {}/{})", attempts, max_attempts);
        }

        println!(
            "[PlanOps] Executing plan with intent_ids: {:?}",
            plan.intent_ids
        );

        let result = ccos.validate_and_execute_plan(plan, &context).await;

        match result {
            Ok(exec_result) if exec_result.success => {
                return Ok(format!(
                    "✅ Plan executed successfully.\nResult: {}",
                    exec_result.value
                ));
            }
            Ok(exec_result) => {
                let error = exec_result
                    .metadata
                    .get("error")
                    .map(|v| format!("{}", v))
                    .unwrap_or_else(|| "Unknown error".to_string());

                if attempts < max_attempts {
                    println!("❌ Execution failed: {}", error);
                    println!("🔧 Attempting to repair plan...");

                    match repair_plan(&content, &error).await {
                        Ok(repaired) => {
                            content = repaired;
                            continue;
                        }
                        Err(e) => {
                            last_error = Some(format!("Repair failed: {}", e));
                            break;
                        }
                    }
                } else {
                    last_error = Some(error);
                }
            }
            Err(e) => {
                let error = e.to_string();

                if attempts < max_attempts {
                    println!("❌ Execution error: {}", error);
                    println!("🔧 Attempting to repair plan...");

                    match repair_plan(&content, &error).await {
                        Ok(repaired) => {
                            content = repaired;
                            continue;
                        }
                        Err(repair_err) => {
                            last_error = Some(format!("Repair failed: {}", repair_err));
                            break;
                        }
                    }
                } else {
                    last_error = Some(error);
                }
            }
        }
    }

    Err(RuntimeError::Generic(format!(
        "❌ Plan execution failed after {} attempts: {}",
        attempts,
        last_error.unwrap_or_else(|| "Unknown error".to_string())
    )))
}

/// Validate plan syntax only (no CCOS initialization, Send-safe)
/// This is used by native capabilities which need Send futures.
pub async fn validate_plan(plan_input: String) -> RuntimeResult<bool> {
    let resolved = resolve_plan_content(&plan_input)?;
    let content = resolved.content;

    // Syntax validation only
    match rtfs::parser::parse(&content) {
        Ok(_) => Ok(true),
        Err(e) => {
            println!("❌ Syntax Error: {}", e);
            Ok(false)
        }
    }
}

/// Validate plan syntax and capability availability (full validation)
/// This creates CCOS and checks capabilities, so it's not Send-safe.
pub async fn validate_plan_full(plan_input: String) -> RuntimeResult<bool> {
    let resolved = resolve_plan_content(&plan_input)?;
    let content = resolved.content;

    // 1. Syntax validation
    println!("🔍 Validating syntax...");
    if let Err(e) = rtfs::parser::parse(&content) {
        println!("❌ Syntax Error: {}", e);
        return Ok(false);
    }
    println!("   ✅ Syntax valid");

    // 2. Capability validation
    println!("🔍 Validating capabilities...");
    let capabilities = extract_capabilities_from_rtfs(&content);

    if capabilities.is_empty() {
        println!("   ⚠️  No capabilities found in plan");
        return Ok(true);
    }

    // Initialize CCOS to check capabilities
    let config_path = if std::path::Path::new("config/agent_config.toml").exists() {
        "config/agent_config.toml"
    } else if std::path::Path::new("../config/agent_config.toml").exists() {
        "../config/agent_config.toml"
    } else {
        "config/agent_config.toml" // default fallback
    };

    maybe_set_capability_storage_from_config(config_path);
    let agent_config = match load_agent_config(config_path) {
        Ok(cfg) => Some(cfg),
        Err(_) => None,
    };

    let ccos = CCOS::new_with_agent_config_and_configs_and_debug_callback(
        Default::default(),
        None,
        agent_config.clone(),
        None,
    )
    .await?;
    let marketplace = ccos.get_capability_marketplace();
    crate::ops::native::register_native_capabilities(&marketplace).await?;
    load_approved_capabilities(&marketplace).await?;
    load_generated_capabilities(&marketplace).await?;

    let mut all_valid = true;
    for cap_id in &capabilities {
        let exists = marketplace.has_capability(cap_id).await;
        if exists {
            println!("   ✅ {} - available", cap_id);
        } else {
            println!("   ❌ {} - NOT FOUND", cap_id);
            all_valid = false;
        }
    }

    if all_valid {
        println!("\n✅ Plan is valid and all capabilities are available.");
    } else {
        println!("\n⚠️  Plan has syntax errors or missing capabilities.");
    }

    Ok(all_valid)
}

/// Repair a failing plan using LLM
pub async fn repair_plan(original_plan: &str, error: &str) -> RuntimeResult<String> {
    let config = get_llm_config_from_env()?;

    if matches!(config.provider_type, LlmProviderType::Stub) {
        return Err(RuntimeError::Generic(
            "Cannot repair plan without LLM API key".to_string(),
        ));
    }

    let provider = LlmProviderFactory::create_provider(config).await?;

    let prompt = format!(
        r#"The following RTFS plan failed with this error:

Error: {}

Original Plan:
```rtfs
{}
```

Please fix the plan to address this error. Return ONLY the corrected RTFS code, no explanations.

Rules:
- RTFS uses prefix notation with parentheses
- Maps use {{:key value}} syntax, NO commas, NO equals signs  
- Capability calls: (call :provider.capability {{:param value}})
- Strings must be in double quotes
"#,
        error, original_plan
    );

    let response = provider.generate_text(&prompt).await?;

    // Extract RTFS from response
    let repaired = extract_rtfs_from_response(&response).ok_or_else(|| {
        RuntimeError::Generic("Failed to extract repaired RTFS from LLM response".to_string())
    })?;

    // Validate syntax
    if let Err(e) = rtfs::parser::parse(&repaired) {
        return Err(RuntimeError::Generic(format!(
            "Repaired plan has syntax errors: {}",
            e
        )));
    }

    Ok(repaired)
}

// --- Helpers ---

#[derive(Debug)]
struct ResolvedPlan {
    content: String,
    plan: Option<Plan>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum MatchKind {
    ExactId,
    PrefixId,
    Name,
    Goal,
}

fn open_plan_archive() -> RuntimeResult<PlanArchive> {
    let archive_path = crate::utils::fs::default_plan_archive_path();
    std::fs::create_dir_all(&archive_path).map_err(|e| {
        RuntimeError::Generic(format!(
            "Failed to create plan archive dir {}: {}",
            archive_path.display(),
            e
        ))
    })?;

    PlanArchive::with_file_storage(archive_path.clone()).map_err(|e| {
        RuntimeError::Generic(format!(
            "Failed to open plan archive at {}: {}",
            archive_path.display(),
            e
        ))
    })
}

fn match_plan_hint(plan: &ArchivablePlan, hint: &str) -> Option<MatchKind> {
    let needle = hint.to_lowercase();

    if plan.plan_id == hint {
        return Some(MatchKind::ExactId);
    }
    if plan.plan_id.starts_with(hint) {
        return Some(MatchKind::PrefixId);
    }

    if let Some(name) = &plan.name {
        if name.to_lowercase().contains(&needle) {
            return Some(MatchKind::Name);
        }
    }

    if let Some(goal) = plan.metadata.get("goal") {
        if goal.to_lowercase().contains(&needle) {
            return Some(MatchKind::Goal);
        }
    }

    None
}

fn find_plan_by_hint(archive: &PlanArchive, hint: &str) -> RuntimeResult<Option<ArchivablePlan>> {
    let mut candidates: Vec<(MatchKind, ArchivablePlan)> = Vec::new();
    for plan_id in archive.list_plan_ids() {
        if let Some(plan) = archive.get_plan_by_id(&plan_id) {
            if let Some(kind) = match_plan_hint(&plan, hint) {
                candidates.push((kind, plan));
            }
        }
    }

    if candidates.is_empty() {
        return Ok(None);
    }

    candidates.sort_by(|a, b| a.0.cmp(&b.0));
    let best_kind = candidates.first().map(|c| c.0).unwrap();
    let best: Vec<_> = candidates
        .into_iter()
        .filter(|(k, _)| *k == best_kind)
        .collect();

    if best.len() > 1 {
        let summary: Vec<String> = best
            .iter()
            .map(|(_, p)| {
                if let Some(name) = &p.name {
                    format!("{} ({})", p.plan_id, name)
                } else {
                    p.plan_id.clone()
                }
            })
            .collect();
        return Err(RuntimeError::Generic(format!(
            "Ambiguous plan reference '{}'. Candidates: {}",
            hint,
            summary.join(", ")
        )));
    }

    Ok(best.into_iter().next().map(|(_, p)| p))
}

pub fn list_archived_plans(filter: Option<&str>) -> RuntimeResult<Vec<ArchivablePlan>> {
    let archive = open_plan_archive()?;
    let filter = filter.map(|f| f.to_lowercase());
    let mut out = Vec::new();

    for plan_id in archive.list_plan_ids() {
        if let Some(plan) = archive.get_plan_by_id(&plan_id) {
            if let Some(ref needle) = filter {
                let matches_id = plan.plan_id.to_lowercase().contains(needle);
                let matches_name = plan
                    .name
                    .as_ref()
                    .map(|n| n.to_lowercase().contains(needle))
                    .unwrap_or(false);
                let matches_goal = plan
                    .metadata
                    .get("goal")
                    .map(|g| g.to_lowercase().contains(needle))
                    .unwrap_or(false);

                if !(matches_id || matches_name || matches_goal) {
                    continue;
                }
            }
            out.push(plan);
        }
    }

    Ok(out)
}

pub fn delete_plan_by_hint(hint: &str) -> RuntimeResult<String> {
    let archive = open_plan_archive()?;
    let plan = find_plan_by_hint(&archive, hint).and_then(|opt| {
        opt.ok_or_else(|| RuntimeError::Generic(format!("Plan not found: {}", hint)))
    })?;

    let plan_id = plan.plan_id.clone();
    let deleted = archive
        .delete_plan(&plan_id)
        .map_err(|e| RuntimeError::Generic(format!("Failed to delete plan: {}", e)))?;

    if !deleted {
        return Err(RuntimeError::Generic(format!(
            "Plan not found: {}",
            plan_id
        )));
    }

    Ok(plan_id)
}

fn load_plan_from_archive(hint: &str) -> RuntimeResult<Option<Plan>> {
    let archive = open_plan_archive()?;
    let plan = find_plan_by_hint(&archive, hint)?;
    Ok(plan.map(|p| Orchestrator::archivable_plan_to_plan(&p)))
}

fn plan_body_to_string(plan: &Plan) -> RuntimeResult<String> {
    match &plan.body {
        PlanBody::Source(code) | PlanBody::Rtfs(code) => Ok(code.clone()),
        PlanBody::Binary(_) | PlanBody::Wasm(_) => Err(RuntimeError::Generic(
            "Binary/WASM plan execution from archive is not supported".to_string(),
        )),
    }
}

fn resolve_plan_content(input: &str) -> RuntimeResult<ResolvedPlan> {
    let path = Path::new(input);
    if path.exists() && path.is_file() {
        let content = std::fs::read_to_string(path)
            .map_err(|e| RuntimeError::Generic(format!("Failed to read plan file: {}", e)))?;
        return Ok(ResolvedPlan {
            content,
            plan: None,
        });
    }

    // Assume input is raw RTFS if it looks like it.
    let trimmed = input.trim();
    if trimmed.starts_with('(') || trimmed.contains("(do") || trimmed.contains("(plan") {
        return Ok(ResolvedPlan {
            content: input.to_string(),
            plan: None,
        });
    }

    // Fallback to plan archive lookup (id, prefix, name, goal)
    if let Some(plan) = load_plan_from_archive(input)? {
        let content = plan_body_to_string(&plan)?;
        println!("📦 Loaded plan '{}' from archive", plan.plan_id);
        return Ok(ResolvedPlan {
            content,
            plan: Some(plan),
        });
    }

    Err(RuntimeError::Generic(format!(
        "File or plan not found: {}",
        input
    )))
}

fn get_llm_config_from_env() -> RuntimeResult<LlmProviderConfig> {
    use crate::cognitive_engine::config::RetryConfig;

    // Check for API keys
    let (provider_type, api_key, model, base_url) =
        if let Ok(key) = std::env::var("OPENROUTER_API_KEY") {
            (
                LlmProviderType::OpenAI,
                Some(key),
                std::env::var("OPENROUTER_MODEL")
                    .unwrap_or_else(|_| "anthropic/claude-3.5-sonnet".to_string()),
                std::env::var("CCOS_LLM_BASE_URL")
                    .ok()
                    .or_else(|| Some("https://openrouter.ai/api/v1".to_string())),
            )
        } else if let Ok(key) = std::env::var("OPENAI_API_KEY") {
            (
                LlmProviderType::OpenAI,
                Some(key),
                std::env::var("OPENAI_MODEL").unwrap_or_else(|_| "gpt-4o".to_string()),
                std::env::var("CCOS_LLM_BASE_URL").ok(),
            )
        } else if let Ok(key) = std::env::var("ANTHROPIC_API_KEY") {
            (
                LlmProviderType::Anthropic,
                Some(key),
                std::env::var("ANTHROPIC_MODEL")
                    .unwrap_or_else(|_| "claude-3-5-sonnet-20240620".to_string()),
                None,
            )
        } else {
            println!(
            "⚠️  No LLM API key found (OPENAI_API_KEY, ANTHROPIC_API_KEY, or OPENROUTER_API_KEY)."
        );
            println!("   Using Stub provider (generates fake plans).");
            (LlmProviderType::Stub, None, "stub-model".to_string(), None)
        };

    Ok(LlmProviderConfig {
        provider_type,
        api_key,
        model,
        base_url,
        max_tokens: Some(4096),
        temperature: Some(0.0),
        timeout_seconds: Some(60),
        retry_config: RetryConfig::default(),
    })
}

/// Test the discovery step with a query
///
/// This function can be used to test the new `step_discover_new_tools`
/// functionality without requiring full plan creation.
///
/// # Arguments
/// * `query` - Search query (e.g., "GitHub issues", "weather api")
///
/// # Returns
/// A formatted string with discovery results
pub async fn test_discovery_step(query: &str) -> RuntimeResult<String> {
    use crate::discovery::registry_search::RegistrySearcher;

    let searcher = RegistrySearcher::new();
    let results = searcher.search(query).await?;

    let mut output = format!("Discovery results for '{}':\n", query);
    output.push_str(&format!(
        "Found {} potential servers/APIs\n\n",
        results.len()
    ));

    for (i, result) in results.iter().take(10).enumerate() {
        output.push_str(&format!(
            "{}. {} (source: {:?}, score: {:.2})\n",
            i + 1,
            result.server_info.name,
            result.source,
            result.match_score
        ));
        if let Some(ref desc) = result.server_info.description {
            let short_desc = if desc.len() > 80 {
                format!("{}...", &desc[..80])
            } else {
                desc.clone()
            };
            output.push_str(&format!("   {}\n", short_desc));
        }
    }

    Ok(output)
}

/// Validate capabilities in RTFS code against marketplace
async fn validate_capabilities_in_code(
    rtfs_code: &str,
    marketplace: &Arc<CapabilityMarketplace>,
) -> (Vec<String>, bool, Vec<String>) {
    let capabilities = extract_capabilities_from_rtfs(rtfs_code);

    let mut issues = Vec::new();
    let mut unresolved = Vec::new();
    let mut all_resolved = true;

    for cap_id in &capabilities {
        let exists = marketplace.has_capability(cap_id).await;
        if !exists {
            issues.push(format!("Capability not found: {}", cap_id));
            unresolved.push(cap_id.clone());
            all_resolved = false;
        }
    }

    (issues, all_resolved, unresolved)
}

/// Extract capability IDs from RTFS code
fn extract_capabilities_from_rtfs(rtfs_code: &str) -> HashSet<String> {
    let mut capabilities = HashSet::new();

    // Simple extraction for (call :capability.id ...) patterns
    for line in rtfs_code.lines() {
        let trimmed = line.trim();
        if let Some(call_idx) = trimmed.find("(call ") {
            let after_call = &trimmed[call_idx + 6..];
            // Extract the capability ID (starts with : or is a symbol)
            let raw_cap: String = after_call
                .chars()
                .take_while(|c| !c.is_whitespace() && *c != ')' && *c != '{')
                .collect();
            if !raw_cap.is_empty() {
                // Handle either symbols (:ccos.io.println) or quoted strings ("ccos.io.println")
                let cap_id = raw_cap
                    .trim_start_matches(':')
                    .trim_matches('"')
                    .to_string();
                capabilities.insert(cap_id);
            }
        }
    }

    capabilities
}

/// Extract RTFS code from LLM response
fn extract_rtfs_from_response(response: &str) -> Option<String> {
    let trimmed = response.trim();
    if trimmed.is_empty() {
        return None;
    }

    // Try to find fenced code blocks
    let mut cursor = trimmed;
    while let Some(start) = cursor.find("```") {
        let after_tick = &cursor[start + 3..];
        let mut block_start = after_tick;
        if let Some(idx) = after_tick.find('\n') {
            let first_line = after_tick[..idx].trim().to_ascii_lowercase();
            let rest = &after_tick[idx + 1..];
            if first_line == "rtfs"
                || first_line == "lisp"
                || first_line == "scheme"
                || first_line.is_empty()
            {
                block_start = rest;
            }
        }

        if let Some(end_idx) = block_start.find("```") {
            let code = block_start[..end_idx].trim();
            if !code.is_empty() && code.starts_with('(') {
                return Some(code.to_string());
            }
            cursor = &block_start[end_idx + 3..];
        } else {
            break;
        }
    }

    // Fallback: use the response directly if it looks like RTFS
    let stripped = trimmed.trim_matches('`').trim();
    if stripped.starts_with('(') {
        return Some(stripped.to_string());
    }

    None
}

/// Load capabilities from approved MCP servers
///
/// Uses the marketplace's built-in import_capabilities_from_rtfs_dir_recursive
/// to load capabilities from the approved servers directory.
async fn load_approved_capabilities(marketplace: &Arc<CapabilityMarketplace>) -> RuntimeResult<()> {
    // Use workspace-relative path for approved servers directory
    // Workspace root is the config dir, so ../capabilities goes to <workspace>/capabilities
    let approved_dir = crate::utils::fs::resolve_workspace_path("../capabilities/servers/approved");
    println!("🔍 Loading approved capabilities from: {:?}", approved_dir);

    if !approved_dir.exists() {
        println!(
            "⚠️  Approved servers directory not found at {:?}",
            approved_dir
        );
        return Ok(());
    }

    let approved_dir = &approved_dir;

    // Use the marketplace's built-in method to recursively import RTFS capabilities
    let loaded = marketplace
        .import_capabilities_from_rtfs_dir_recursive(approved_dir)
        .await?;

    if loaded > 0 {
        println!("📦 Loaded {} capabilities from approved servers", loaded);
    }

    Ok(())
}

/// Load generated capabilities
async fn load_generated_capabilities(
    marketplace: &Arc<CapabilityMarketplace>,
) -> RuntimeResult<()> {
    // Use workspace-relative path for generated capabilities directory
    // Workspace root is the config dir, so ../capabilities goes to <workspace>/capabilities
    let gen_dir = crate::utils::fs::resolve_workspace_path("../capabilities/generated");
    println!("🔍 Loading generated capabilities from: {:?}", gen_dir);

    if !gen_dir.exists() {
        println!(
            "⚠️  Generated capabilities directory not found at {:?}",
            gen_dir
        );
        return Ok(());
    }

    let gen_dir = &gen_dir;

    // Use the marketplace's built-in method to recursively import RTFS capabilities
    let loaded = marketplace
        .import_capabilities_from_rtfs_dir_recursive(gen_dir)
        .await?;

    if loaded > 0 {
        println!("✨ Loaded {} generated capabilities", loaded);
    }

    Ok(())
}

/// Step testing module for CLI debugging
pub mod step_testing {
    use super::*;
    use crate::planner::modular_planner::decomposition::DecompositionResult;
    use crate::planner::modular_planner::resolution::ResolvedCapability;
    use crate::planner::modular_planner::steps::ToolDiscoveryResult;

    /// Test tool discovery for a goal
    pub async fn test_discover(goal: &str, _verbose: bool) -> RuntimeResult<ToolDiscoveryResult> {
        let (_decomposition, resolution, _config) = build_planner_components(false).await?;

        crate::planner::modular_planner::steps::step_discover_tools(goal, resolution.as_ref())
            .await
            .map_err(|e| RuntimeError::Generic(e.to_string()))
    }

    /// Test decomposition for a goal
    pub async fn test_decompose(
        goal: &str,
        verbose: bool,
        force_llm: bool,
        show_prompt: bool,
    ) -> RuntimeResult<DecompositionResult> {
        let (decomposition, resolution, config) = build_planner_components(force_llm).await?;

        // First discover tools
        let tools_result =
            crate::planner::modular_planner::steps::step_discover_tools(goal, resolution.as_ref())
                .await
                .map_err(|e| RuntimeError::Generic(e.to_string()))?;

        let mut trace = crate::planner::modular_planner::PlanningTrace::default();
        let grounding_params = HashMap::new();

        let config = crate::planner::modular_planner::PlannerConfig {
            verbose_llm: verbose,
            show_prompt,
            ..config
        };

        crate::planner::modular_planner::steps::step_decompose(
            goal,
            Some(&tools_result.tools),
            decomposition.as_ref(),
            &config,
            &grounding_params,
            &mut trace,
        )
        .await
        .map_err(|e| RuntimeError::Generic(e.to_string()))
    }

    /// Test resolution for a goal (includes decomposition)
    pub async fn test_resolve(
        goal: &str,
        verbose: bool,
    ) -> RuntimeResult<(
        DecompositionResult,
        HashMap<String, ResolvedCapability>,
        Vec<String>,
    )> {
        let (decomposition, resolution, config) = build_planner_components(false).await?;

        // First discover tools
        let tools_result =
            crate::planner::modular_planner::steps::step_discover_tools(goal, resolution.as_ref())
                .await
                .map_err(|e| RuntimeError::Generic(e.to_string()))?;

        let mut trace = crate::planner::modular_planner::PlanningTrace::default();
        let grounding_params = HashMap::new();

        let config = crate::planner::modular_planner::PlannerConfig {
            verbose_llm: verbose,
            ..config
        };

        // Decompose
        let decomp_result = crate::planner::modular_planner::steps::step_decompose(
            goal,
            Some(&tools_result.tools),
            decomposition.as_ref(),
            &config,
            &grounding_params,
            &mut trace,
        )
        .await
        .map_err(|e| RuntimeError::Generic(e.to_string()))?;

        // Generate intent IDs
        let intent_ids: Vec<String> = decomp_result
            .sub_intents
            .iter()
            .enumerate()
            .map(|(i, _)| format!("test:step-{}", i))
            .collect();

        // Resolve
        let resolution_result = crate::planner::modular_planner::steps::step_resolve_intents(
            &decomp_result.sub_intents,
            &intent_ids,
            resolution.as_ref(),
            &mut trace,
        )
        .await
        .map_err(|e| RuntimeError::Generic(e.to_string()))?;

        Ok((
            decomp_result,
            resolution_result.resolutions,
            resolution_result.unresolved,
        ))
    }

    /// Test full pipeline with detailed output
    pub async fn test_full_pipeline(
        goal: &str,
        verbose: bool,
        force_llm: bool,
    ) -> RuntimeResult<()> {
        println!("\n📍 Step 1: Tool Discovery");
        let tools = test_discover(goal, verbose).await?;
        println!(
            "   Found {} tools, {} domains",
            tools.tools.len(),
            tools.domain_hints.len()
        );

        println!("\n📍 Step 2: Decomposition");
        let decomp = test_decompose(goal, verbose, force_llm, false).await?;
        println!(
            "   Decomposed into {} sub-intents",
            decomp.sub_intents.len()
        );
        for (i, intent) in decomp.sub_intents.iter().enumerate() {
            println!("   {}. {}", i + 1, intent.description);
        }

        println!("\n📍 Step 3: Resolution");
        let (_, resolutions, unresolved) = test_resolve(goal, verbose).await?;
        println!(
            "   Resolved: {}, Unresolved: {}",
            resolutions.len(),
            unresolved.len()
        );

        println!("\n✅ Pipeline complete!");
        Ok(())
    }

    /// Create CCOS instance and LLM configuration for step testing
    async fn create_ccos_and_llm_config() -> RuntimeResult<(Arc<CCOS>, LlmProviderConfig)> {
        // Get LLM config from environment
        let llm_config = get_llm_config_from_env()?;

        // Initialize CCOS runtime
        let ccos = Arc::new(CCOS::new().await?);
        let marketplace = ccos.get_capability_marketplace();

        // Ensure native capabilities are registered
        crate::ops::native::register_native_capabilities(&marketplace).await?;
        load_approved_capabilities(&marketplace).await?;
        load_generated_capabilities(&marketplace).await?;

        // Keep catalog in sync
        ccos.get_catalog().ingest_marketplace(&marketplace).await;

        // Configure MCP session pool
        configure_mcp_session_pool(&marketplace).await?;

        Ok((ccos, llm_config))
    }

    /// Build the planner components (reused across steps)
    async fn build_planner_components(
        force_llm: bool,
    ) -> RuntimeResult<(
        Box<dyn DecompositionStrategy>,
        Box<dyn crate::planner::modular_planner::ResolutionStrategy>,
        crate::planner::modular_planner::PlannerConfig,
    )> {
        let (ccos, llm_config) = create_ccos_and_llm_config().await?;

        // Build decomposition strategy based on LLM availability
        let decomposition: Box<dyn DecompositionStrategy> =
            match LlmProviderFactory::create_provider(llm_config).await {
                Ok(provider) => {
                    let adapter = Arc::new(CcosLlmAdapter::new(provider));
                    if force_llm {
                        use crate::planner::modular_planner::decomposition::IntentFirstDecomposition;
                        Box::new(IntentFirstDecomposition::new(adapter))
                    } else {
                        let hybrid = HybridDecomposition::new()
                            .with_llm(adapter)
                            .with_config(HybridConfig::default());
                        Box::new(hybrid)
                    }
                }
                Err(e) => {
                    println!(
                        "⚠️  LLM provider unavailable ({}). Using pattern-only decomposition.",
                        e
                    );
                    Box::new(PatternDecomposition::new())
                }
            };

        // Build resolution strategy
        let resolution = build_resolution_strategy(&ccos).await?;

        let config = crate::planner::modular_planner::PlannerConfig {
            max_depth: 5,
            persist_intents: false,
            create_edges: false,
            intent_namespace: "test".to_string(),
            ..Default::default()
        };

        Ok((decomposition, resolution, config))
    }

    /// Build resolution strategy (catalog + MCP)
    async fn build_resolution_strategy(
        ccos: &Arc<CCOS>,
    ) -> RuntimeResult<Box<dyn crate::planner::modular_planner::ResolutionStrategy>> {
        let catalog_adapter = Arc::new(CcosCatalogAdapter::new(ccos.get_catalog()));
        let mut composite = CompositeResolution::new();
        composite.add_strategy(Box::new(CatalogResolution::new(catalog_adapter)));

        // Try to set up MCP discovery
        let discovery_service = Arc::new(
            MCPDiscoveryService::with_auth_headers(mcp_auth_headers())
                .with_marketplace(ccos.get_capability_marketplace())
                .with_catalog(ccos.get_catalog()),
        );

        let mcp_discovery = Arc::new(
            RuntimeMcpDiscovery::with_discovery_service(
                ccos.get_capability_marketplace(),
                discovery_service,
            )
            .with_catalog(ccos.get_catalog()),
        );
        composite.add_strategy(Box::new(McpResolution::new(mcp_discovery)));

        Ok(Box::new(composite))
    }
}
