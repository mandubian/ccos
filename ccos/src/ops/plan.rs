//! Plan operations - pure logic functions for planning
//!
//! This module implements the core logic for the `ccos plan` command family:
//! - `create_plan`: Generates a plan from a natural language goal using LLM
//! - `execute_plan`: Executes an RTFS plan using the CCOS runtime
//! - `validate_plan`: Validates an RTFS plan (syntax + capability availability)
//! - `repair_plan`: Attempts to fix a failing plan using LLM

use crate::arbiter::llm_provider::{LlmProviderConfig, LlmProviderFactory, LlmProviderType};
use crate::capability_marketplace::CapabilityMarketplace;
use crate::capabilities::registry::CapabilityRegistry;
use crate::types::Plan;
use crate::CCOS;
use rtfs::runtime::error::{RuntimeError, RuntimeResult};
use rtfs::runtime::security::RuntimeContext;
use std::collections::HashSet;
use std::path::Path;
use std::sync::{Arc, Mutex};
use tokio::sync::RwLock;

/// Options for plan creation
#[derive(Debug, Clone, Default)]
pub struct CreatePlanOptions {
    /// Don't execute, just show the plan
    pub dry_run: bool,
    /// Save plan to file
    pub save_to: Option<String>,
    /// Show verbose output (LLM prompts, etc.)
    pub verbose: bool,
    /// Skip capability validation
    pub skip_validation: bool,
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
    // 1. Configure LLM Provider from environment
    let config = get_llm_config_from_env()?;
    
    // 2. Create marketplace with native capabilities
    let registry = Arc::new(RwLock::new(CapabilityRegistry::new()));
    let causal_chain = Arc::new(Mutex::new(crate::causal_chain::CausalChain::new()?));
    let marketplace = Arc::new(CapabilityMarketplace::with_causal_chain_and_debug_callback(
        registry,
        Some(causal_chain),
        None,
    ));
    
    // Register native capabilities
    crate::ops::native::register_native_capabilities(&marketplace).await?;
    
    // 3. Load approved MCP server capabilities
    load_approved_capabilities(&marketplace).await?;
    
    // 4. Get available capabilities for the prompt
    let capabilities = marketplace.list_capabilities().await;
    let capability_descriptions = format_capabilities_for_prompt(&capabilities);
    
    // 5. Generate plan using capability-aware prompt
    println!("🧠 Generating plan for goal: \"{}\"...", goal);
    let rtfs_code = generate_capability_aware_plan(&goal, &capability_descriptions, &config).await?;
    
    // 6. Print the generated plan
    println!("{}", rtfs_code);
    
    // 7. Validate capabilities (unless skipped)
    let (validation_issues, all_resolved, unresolved) = if !options.skip_validation {
        validate_capabilities_in_code(&rtfs_code, &marketplace).await
    } else {
        (vec![], true, vec![])
    };
    
    // 7. Display validation issues
    if !validation_issues.is_empty() {
        println!("\n⚠️  {} capability(ies) not found:", validation_issues.len());
        for issue in &validation_issues {
            println!("   • {}", issue.replace("Capability not found: ", ""));
        }
        println!();
    }
    
    // 8. Save to file if requested
    if let Some(path) = &options.save_to {
        std::fs::write(path, &rtfs_code).map_err(|e| {
            RuntimeError::Generic(format!("Failed to save plan to {}: {}", path, e))
        })?;
        println!("💾 Saved plan to: {}", path);
    }
    
    // 9. Display result for dry-run
    if options.dry_run {
        println!("\n📋 Generated Plan (dry-run):\n");
        println!("{}", rtfs_code);
    }
    
    Ok(CreatePlanResult {
        rtfs_code,
        validation_issues,
        all_resolved,
        unresolved_capabilities: unresolved,
    })
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
    let (provider_type, api_key, model, base_url) = if let Ok(key) = std::env::var("OPENROUTER_API_KEY") {
        (
            LlmProviderType::OpenAI, 
            Some(key), 
            std::env::var("OPENROUTER_MODEL").unwrap_or_else(|_| "anthropic/claude-3.5-sonnet".to_string()),
            std::env::var("CCOS_LLM_BASE_URL").ok().or_else(|| Some("https://openrouter.ai/api/v1".to_string()))
        )
    } else if let Ok(key) = std::env::var("OPENAI_API_KEY") {
        (
            LlmProviderType::OpenAI, 
            Some(key), 
            std::env::var("OPENAI_MODEL").unwrap_or_else(|_| "gpt-4o".to_string()),
            std::env::var("CCOS_LLM_BASE_URL").ok()
        )
    } else if let Ok(key) = std::env::var("ANTHROPIC_API_KEY") {
        (
            LlmProviderType::Anthropic, 
            Some(key), 
            std::env::var("ANTHROPIC_MODEL").unwrap_or_else(|_| "claude-3-5-sonnet-20240620".to_string()),
            None
        )
    } else {
        println!("⚠️  No LLM API key found (OPENAI_API_KEY, ANTHROPIC_API_KEY, or OPENROUTER_API_KEY).");
        println!("   Using Stub provider (generates fake plans).");
        (
            LlmProviderType::Stub,
            None,
            "stub-model".to_string(),
            None
        )
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
            let cap_id: String = after_call
                .chars()
                .take_while(|c| !c.is_whitespace() && *c != ')' && *c != '{')
                .collect();
            if !cap_id.is_empty() {
                capabilities.insert(cap_id.trim_start_matches(':').to_string());
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

/// Format capabilities for inclusion in LLM prompt
fn format_capabilities_for_prompt(capabilities: &[crate::capability_marketplace::types::CapabilityManifest]) -> String {
    if capabilities.is_empty() {
        return "No capabilities are currently registered. You may need to use basic RTFS operations only.".to_string();
    }
    
    let mut output = String::new();
    output.push_str("Available capabilities:\n");
    
    for cap in capabilities {
        output.push_str(&format!("- {} : {}\n", cap.id, cap.description));
    }
    
    output
}

/// Generate a plan using capability-aware LLM prompt
async fn generate_capability_aware_plan(
    goal: &str,
    capability_descriptions: &str,
    config: &LlmProviderConfig,
) -> RuntimeResult<String> {
    let provider = LlmProviderFactory::create_provider(config.clone()).await?;
    
    let system_prompt = format!(r#"You are an RTFS plan generator. Generate executable RTFS code for the user's goal.

RTFS Syntax Rules:
- Use prefix notation with parentheses: (function arg1 arg2)
- Maps use curly braces with keyword keys: {{:key1 value1 :key2 value2}}
- NO commas in maps or lists
- Strings use double quotes: "string"
- Keywords start with colon: :keyword
- Call capabilities using: (call :capability.id {{:param value}})

{}

IMPORTANT:
- ONLY use capabilities from the list above
- If a capability doesn't exist, explain what's missing in a comment
- For GitHub operations, use MCP server capabilities if available (e.g., github.list_issues)
- Return ONLY the RTFS code, no explanations

Output format: A single RTFS expression starting with (do ...) or (plan ...)"#, capability_descriptions);

    let user_prompt = format!("Generate an RTFS plan for: {}", goal);
    
    let response = provider.generate_text(&format!("{}\n\n{}", system_prompt, user_prompt)).await?;
    
    // Extract RTFS from response
    extract_rtfs_from_response(&response).ok_or_else(|| {
        RuntimeError::Generic(format!("Failed to extract RTFS from LLM response: {}", response))
    })
}

/// Load capabilities from approved MCP servers
/// 
/// Uses the marketplace's built-in import_capabilities_from_rtfs_dir_recursive
/// to load capabilities from the approved servers directory.
async fn load_approved_capabilities(
    marketplace: &Arc<CapabilityMarketplace>,
) -> RuntimeResult<()> {
    // Try multiple potential locations for the approved servers directory
    let potential_paths = [
        std::path::PathBuf::from("capabilities/servers/approved"),
        std::path::PathBuf::from("../capabilities/servers/approved"),
    ];
    
    let approved_dir = potential_paths.iter().find(|p| p.exists());
    
    let approved_dir = match approved_dir {
        Some(path) => path,
        None => {
            log::debug!("No approved servers directory found");
            return Ok(());
        }
    };
    
    // Use the marketplace's built-in method to recursively import RTFS capabilities
    let loaded = marketplace.import_capabilities_from_rtfs_dir_recursive(approved_dir).await?;
    
    if loaded > 0 {
        println!("📦 Loaded {} capabilities from approved servers", loaded);
    }
    
    Ok(())
}

