//! Plan operations - pure logic functions for planning
//!
//! This module implements the core logic for the `ccos plan` command family:
//! - `create_plan`: Generates a plan from a natural language goal using LLM
//! - `execute_plan`: Executes an RTFS plan using the CCOS runtime
//! - `validate_plan`: Validates an RTFS plan (syntax + capability availability)
//! - `repair_plan`: Attempts to fix a failing plan using LLM

use crate::arbiter::llm_provider::{LlmProviderConfig, LlmProviderFactory, LlmProviderType};
use crate::capabilities::{MCPSessionHandler, SessionPoolManager};
use crate::capability_marketplace::CapabilityMarketplace;
use crate::examples_common::builder::load_agent_config;
use crate::mcp::core::MCPDiscoveryService;
use crate::planner::modular_planner::decomposition::hybrid::HybridConfig;
use crate::planner::modular_planner::decomposition::llm_adapter::CcosLlmAdapter;
use crate::planner::modular_planner::decomposition::{HybridDecomposition, PatternDecomposition};
use crate::planner::modular_planner::resolution::mcp::RuntimeMcpDiscovery;
use crate::planner::modular_planner::resolution::{
    CatalogResolution, CompositeResolution, McpResolution,
};
use crate::planner::modular_planner::{DecompositionStrategy, ModularPlanner, PlannerConfig};
use crate::planner::CcosCatalogAdapter;
use crate::types::Plan;
use crate::CCOS;
use rtfs::runtime::error::{RuntimeError, RuntimeResult};
use rtfs::runtime::security::RuntimeContext;
use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::Path;
use std::sync::Arc;

fn maybe_set_capability_storage_from_config(config_path: &str) {
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
            let mut config = rtfs::config::types::AgentConfig::default();
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
    crate::ops::native::register_native_capabilities(&marketplace).await?;
    load_approved_capabilities(&marketplace).await?;
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
            .with_config(config);

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
    let mut content = resolve_plan_content(&plan_input)?;

    // 2. Initialize CCOS runtime
    println!("🚀 Initializing CCOS runtime...");
    let ccos = CCOS::new().await?;

    // 3. Create execution context
    let context = RuntimeContext::full();

    // Register native capabilities (ccos.cli.*) so they can be used in the plan
    let marketplace = ccos.get_capability_marketplace();
    crate::ops::native::register_native_capabilities(&marketplace).await?;

    // 4. Execute with repair loop
    let mut attempts = 0;
    let max_attempts = options.max_repair_attempts.max(1);
    let mut last_error: Option<String> = None;

    while attempts < max_attempts {
        attempts += 1;

        // Create Plan object
        let plan = Plan::new_rtfs(content.clone(), vec![]);

        // Execute
        if attempts == 1 {
            println!("▶️  Executing plan...");
        } else {
            println!("🔄 Retry attempt {} of {}...", attempts, max_attempts);
        }

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
    let content = resolve_plan_content(&plan_input)?;

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
    let content = resolve_plan_content(&plan_input)?;

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
    let ccos = CCOS::new().await?;
    let marketplace = ccos.get_capability_marketplace();
    crate::ops::native::register_native_capabilities(&marketplace).await?;

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

fn resolve_plan_content(input: &str) -> RuntimeResult<String> {
    let path = Path::new(input);
    if path.exists() && path.is_file() {
        std::fs::read_to_string(path)
            .map_err(|e| RuntimeError::Generic(format!("Failed to read plan file: {}", e)))
    } else {
        // Assume input is raw RTFS if it looks like it, otherwise treat as file not found
        let trimmed = input.trim();
        if trimmed.starts_with('(') || trimmed.contains("(do") || trimmed.contains("(plan") {
            Ok(input.to_string())
        } else {
            Err(RuntimeError::Generic(format!("File not found: {}", input)))
        }
    }
}

fn get_llm_config_from_env() -> RuntimeResult<LlmProviderConfig> {
    use crate::arbiter::arbiter_config::RetryConfig;

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

    if !approved_dir.exists() {
        log::debug!("No approved servers directory found at {:?}", approved_dir);
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

    if !gen_dir.exists() {
        log::debug!("No generated capabilities directory found at {:?}", gen_dir);
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
