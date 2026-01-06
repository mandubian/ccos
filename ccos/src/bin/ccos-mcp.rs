//! CCOS MCP Server
//!
//! Exposes CCOS capabilities as MCP tools.
//! Supports two transports:
//!   - HTTP (default): Streamable HTTP for persistent long-running daemon
//!   - stdio: JSON-RPC over stdin/stdout for subprocess mode
//!
//! Usage:
//!   cargo run --bin ccos-mcp                    # HTTP on localhost:3000
//!   cargo run --bin ccos-mcp -- --port 8080     # HTTP on custom port
//!   cargo run --bin ccos-mcp -- --transport stdio  # stdio mode
//!
//! The HTTP transport enables state persistence across client sessions.

use chrono;
use std::collections::HashMap;
use std::sync::Arc;

use async_trait::async_trait;
use clap::{Parser, ValueEnum};
use serde_json::json;
use tokio::sync::{Mutex, RwLock};

use ccos::arbiter::llm_provider::LlmProvider;
use ccos::approval::{queue::{RiskAssessment, RiskLevel}, types::ApprovalCategory, UnifiedApprovalQueue};
use ccos::capabilities::registry::CapabilityRegistry;
use ccos::capability_marketplace::mcp_discovery::{MCPDiscoveryProvider, MCPServerConfig};
use ccos::capability_marketplace::types::CapabilityQuery;
use ccos::capability_marketplace::CapabilityMarketplace;
use ccos::causal_chain::CausalChain;
use ccos::utils::value_conversion::{json_to_rtfs_value, rtfs_value_to_json};
use ccos::discovery::llm_discovery::LlmDiscoveryService;
use ccos::mcp::http_transport::{run_http_transport_with_approvals, HttpTransportConfig};
use ccos::mcp::server::MCPServer;
use ccos::secrets::SecretStore;
use ccos::synthesis::dialogue::capability_synthesizer::{CapabilitySynthesizer, SynthesisRequest};
use ccos::utils::fs::get_workspace_root;
use rtfs::runtime::error::{RuntimeError, RuntimeResult};

/// Stub LLM provider for fallback mode when no real LLM is available
struct StubLlmProvider;

#[async_trait]
impl LlmProvider for StubLlmProvider {
    fn get_info(&self) -> ccos::arbiter::llm_provider::LlmProviderInfo {
        ccos::arbiter::llm_provider::LlmProviderInfo {
            name: "stub".to_string(),
            version: "0.0.0".to_string(),
            model: "none".to_string(),
            capabilities: vec![],
        }
    }

    async fn generate_text(&self, _prompt: &str) -> RuntimeResult<String> {
        Err(RuntimeError::Generic(
            "No LLM provider configured".to_string(),
        ))
    }

    async fn generate_intent(
        &self,
        _prompt: &str,
        _context: Option<std::collections::HashMap<String, String>>,
    ) -> RuntimeResult<ccos::types::StorableIntent> {
        Err(RuntimeError::Generic(
            "No LLM provider configured".to_string(),
        ))
    }

    async fn generate_plan(
        &self,
        _intent: &ccos::types::StorableIntent,
        _context: Option<std::collections::HashMap<String, String>>,
    ) -> RuntimeResult<ccos::types::Plan> {
        Err(RuntimeError::Generic(
            "No LLM provider configured".to_string(),
        ))
    }

    async fn validate_plan(
        &self,
        _plan_content: &str,
    ) -> RuntimeResult<ccos::arbiter::llm_provider::ValidationResult> {
        Err(RuntimeError::Generic(
            "No LLM provider configured".to_string(),
        ))
    }
}

// ============================================================================
// Session Management Types
// ============================================================================

/// A single step in an execution session
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ExecutionStep {
    pub step_number: usize,
    pub capability_id: String,
    pub inputs: serde_json::Value,
    pub result: serde_json::Value,
    pub rtfs_code: String,
    pub success: bool,
    pub executed_at: String,
}

/// An execution session that tracks steps toward a goal
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct Session {
    pub id: String,
    pub goal: String,
    pub steps: Vec<ExecutionStep>,
    pub context: std::collections::HashMap<String, serde_json::Value>,
    pub created_at: String,
}

impl Session {
    pub fn new(goal: &str) -> Self {
        let id = format!(
            "session_{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_millis())
                .unwrap_or(0)
        );

        Self {
            id,
            goal: goal.to_string(),
            steps: Vec::new(),
            context: std::collections::HashMap::new(),
            created_at: chrono::Utc::now().to_rfc3339(),
        }
    }

    pub fn add_step(
        &mut self,
        capability_id: &str,
        inputs: serde_json::Value,
        result: serde_json::Value,
        success: bool,
    ) -> ExecutionStep {
        let rtfs_code = Self::inputs_to_rtfs(capability_id, &inputs);
        let step = ExecutionStep {
            step_number: self.steps.len() + 1,
            capability_id: capability_id.to_string(),
            inputs,
            result,
            rtfs_code,
            success,
            executed_at: chrono::Utc::now().to_rfc3339(),
        };
        self.steps.push(step.clone());
        step
    }

    /// Convert JSON inputs to RTFS call syntax
    pub fn inputs_to_rtfs(capability_id: &str, inputs: &serde_json::Value) -> String {
        let mut parts = vec![format!("(call \"{}\"", capability_id)];

        if let Some(obj) = inputs.as_object() {
            for (key, value) in obj {
                let value_str = match value {
                    serde_json::Value::String(s) => format!("\"{}\"", s),
                    serde_json::Value::Number(n) => n.to_string(),
                    serde_json::Value::Bool(b) => b.to_string(),
                    serde_json::Value::Null => "nil".to_string(),
                    _ => format!("{}", value),
                };
                parts.push(format!(":{} {}", key, value_str));
            }
        }

        parts.push(")".to_string());
        parts.join(" ")
    }

    /// Generate the complete RTFS plan from all steps
    pub fn to_rtfs_plan(&self) -> String {
        if self.steps.is_empty() {
            return format!(
                ";; Session: {} - No steps executed\n;; Goal: {}",
                self.id, self.goal
            );
        }

        if self.steps.len() == 1 {
            return format!(
                ";; Goal: {}\n;; Session: {}\n\n{}",
                self.goal, self.id, self.steps[0].rtfs_code
            );
        }

        // Multiple steps - wrap in (do ...)
        let mut lines = vec![
            format!(";; Goal: {}", self.goal),
            format!(";; Session: {}", self.id),
            "".to_string(),
            "(do".to_string(),
        ];

        for step in &self.steps {
            lines.push(format!("  {}", step.rtfs_code));
        }
        lines.push(")".to_string());

        lines.join("\n")
    }

    /// Generate a complete RTFS session file with metadata, causal chain, and replay plan
    pub fn to_rtfs_session(&self) -> String {
        let timestamp = chrono::Utc::now().timestamp();
        
        let mut lines = vec![
            format!(";; CCOS Session: {}", self.id),
            format!(";; Goal: {}", self.goal),
            format!(";; Created: {}", self.created_at),
            "".to_string(),
            ";; === SESSION METADATA ===".to_string(),
            "(def :session-meta".to_string(),
            "  {".to_string(),
            format!("    :session-id \"{}\"", self.id),
            format!("    :goal \"{}\"", self.goal.replace("\"", "\\\"")),
            format!("    :created-at {}", timestamp),
            format!("    :step-count {}", self.steps.len()),
            "  })".to_string(),
            "".to_string(),
        ];

        // Add causal chain
        lines.push(";; === CAUSAL CHAIN ===".to_string());
        lines.push("(def :causal-chain".to_string());
        lines.push("  [".to_string());
        
        for (i, step) in self.steps.iter().enumerate() {
            lines.push("    {".to_string());
            lines.push(format!("      :step-number {}", step.step_number));
            lines.push(format!("      :capability-id \"{}\"", step.capability_id));
            lines.push(format!("      :inputs {}", Self::json_to_rtfs(&step.inputs)));
            lines.push(format!("      :success {}", step.success));
            lines.push(format!("      :executed-at \"{}\"", step.executed_at));
            if i < self.steps.len() - 1 {
                lines.push("    }".to_string());
            } else {
                lines.push("    }])".to_string());
            }
        }

        if self.steps.is_empty() {
            lines.push("  ])".to_string());
        }

        lines.push("".to_string());

        // Add replay plan as a function (not executed on load)
        lines.push(";; === REPLAY PLAN (call (replay-session) to execute) ===".to_string());
        lines.push("(defn replay-session []".to_string());
        
        if self.steps.is_empty() {
            lines.push("  nil)".to_string());
        } else if self.steps.len() == 1 {
            lines.push(format!("  {})", self.steps[0].rtfs_code));
        } else {
            lines.push("  (do".to_string());
            for step in &self.steps {
                lines.push(format!("    {}", step.rtfs_code));
            }
            lines.push("  ))".to_string());
        }

        lines.join("\n")
    }

    /// Convert JSON value to RTFS representation
    fn json_to_rtfs(value: &serde_json::Value) -> String {
        match value {
            serde_json::Value::Null => "nil".to_string(),
            serde_json::Value::Bool(b) => b.to_string(),
            serde_json::Value::Number(n) => n.to_string(),
            serde_json::Value::String(s) => format!("\"{}\"", s.replace("\"", "\\\"")),
            serde_json::Value::Array(arr) => {
                let items: Vec<String> = arr.iter().map(Self::json_to_rtfs).collect();
                format!("[{}]", items.join(" "))
            }
            serde_json::Value::Object(obj) => {
                let pairs: Vec<String> = obj
                    .iter()
                    .map(|(k, v)| format!(":{} {}", k, Self::json_to_rtfs(v)))
                    .collect();
                format!("{{{}}}", pairs.join(" "))
            }
        }
    }
}

/// Session store - thread-safe storage for active sessions
pub type SessionStore = Arc<RwLock<std::collections::HashMap<String, Session>>>;

pub fn create_session_store() -> SessionStore {
    Arc::new(RwLock::new(std::collections::HashMap::new()))
}

/// Generate a URL-safe slug from a goal string
fn slugify_goal(goal: &str) -> String {
    goal.to_lowercase()
        .chars()
        .map(|c| if c.is_alphanumeric() { c } else { '_' })
        .collect::<String>()
        .split('_')
        .filter(|s| !s.is_empty())
        .take(5) // Limit to 5 words
        .collect::<Vec<_>>()
        .join("_")
        .chars()
        .take(50) // Limit total length
        .collect()
}

/// Save a session to an RTFS file in the sessions directory
pub fn save_session(session: &Session, sessions_dir: Option<&std::path::Path>) -> std::io::Result<std::path::PathBuf> {
    let default_dir = std::path::PathBuf::from(".ccos/sessions");
    let dir = sessions_dir.unwrap_or(&default_dir);
    
    // Create sessions directory if it doesn't exist
    std::fs::create_dir_all(dir)?;
    
    // Generate human-readable filename
    let goal_slug = slugify_goal(&session.goal);
    let filename = format!("{}_{}.rtfs", session.id, goal_slug);
    let filepath = dir.join(&filename);
    
    // Generate and write RTFS content
    let content = session.to_rtfs_session();
    std::fs::write(&filepath, content)?;
    
    eprintln!("[ccos-mcp] Session saved to: {}", filepath.display());
    Ok(filepath)
}

#[derive(Debug, Clone, Copy, ValueEnum)]
enum Transport {
    /// Streamable HTTP transport (persistent daemon)
    Http,
    /// Stdio transport (subprocess mode)
    Stdio,
}

#[derive(Parser, Debug)]
#[command(version, about = "CCOS MCP Server - Exposes CCOS as MCP tools", author)]
struct Args {
    /// Transport to use
    #[arg(short, long, value_enum, default_value = "http")]
    transport: Transport,

    /// Host to bind for HTTP transport
    #[arg(long, default_value = "0.0.0.0")]
    host: String,

    /// Port for HTTP transport
    #[arg(short, long, default_value = "3000")]
    port: u16,

    /// Enable verbose logging to stderr
    #[arg(short, long)]
    verbose: bool,

    /// Path to agent_config.toml
    #[arg(short, long, default_value = "config/agent_config.toml")]
    config: String,

    /// LLM Profile to use for discovery
    #[arg(long, default_value = "openrouter_free:balanced")]
    profile: String,
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let args = Args::parse();

    if args.verbose {
        eprintln!("[ccos-mcp] Starting CCOS MCP Server...");
        eprintln!("[ccos-mcp] Transport: {:?}", args.transport);
    }

    // Initialize CCOS components
    let capability_registry = Arc::new(RwLock::new(CapabilityRegistry::new()));
    let marketplace = Arc::new(CapabilityMarketplace::new(capability_registry));
    let causal_chain = Arc::new(Mutex::new(CausalChain::new()?));

    // Load agent config
    let agent_config = match ccos::examples_common::builder::load_agent_config(&args.config) {
        Ok(cfg) => {
            if args.verbose {
                eprintln!("[ccos-mcp] Loaded config from {}", args.config);
            }
            Some(cfg)
        }
        Err(e) => {
            if args.verbose {
                eprintln!(
                    "[ccos-mcp] Warning: Could not load config {}: {}",
                    args.config, e
                );
            }
            None
        }
    };
    if let Some(ref config) = agent_config {
        ccos::utils::fs::set_workspace_root(std::path::PathBuf::from(
            &config.storage.capabilities_dir,
        ));
    }

    // Populate marketplace with approved capabilities
    load_capabilities_from_dir(&marketplace, "capabilities/servers/approved", args.verbose).await;
    // Load core capabilities
    load_capabilities_from_dir(&marketplace, "capabilities/core", args.verbose).await;

    // Create discovery service
    let discovery_service = if let Some(config) = &agent_config {
        // Try to find the requested profile
        if let Some(profile) =
            ccos::examples_common::builder::find_llm_profile(config, &args.profile)
        {
            if args.verbose {
                eprintln!("[ccos-mcp] Using LLM profile: {}", args.profile);
            }

            // Resolve API key
            let api_key = profile.api_key.clone().or_else(|| {
                profile
                    .api_key_env
                    .as_ref()
                    .and_then(|env| std::env::var(env).ok())
            });

            if let Some(key) = api_key {
                let provider_type = match profile.provider.as_str() {
                    "openai" => ccos::arbiter::llm_provider::LlmProviderType::OpenAI,
                    "anthropic" => ccos::arbiter::llm_provider::LlmProviderType::Anthropic,
                    "local" => ccos::arbiter::llm_provider::LlmProviderType::Local,
                    "openrouter" => ccos::arbiter::llm_provider::LlmProviderType::OpenAI,
                    _ => ccos::arbiter::llm_provider::LlmProviderType::OpenAI,
                };

                let provider_config = ccos::arbiter::llm_provider::LlmProviderConfig {
                    provider_type,
                    model: profile.model,
                    api_key: Some(key),
                    base_url: profile.base_url,
                    max_tokens: profile.max_tokens.or(Some(4096)),
                    temperature: profile.temperature.or(Some(0.0)),
                    timeout_seconds: Some(60),
                    retry_config: Default::default(),
                };

                match ccos::arbiter::llm_provider::LlmProviderFactory::create_provider(
                    provider_config,
                )
                .await
                {
                    Ok(provider) => Arc::new(LlmDiscoveryService::with_provider(provider)),
                    Err(e) => {
                        if args.verbose {
                            eprintln!(
                                "[ccos-mcp] Error creating LLM provider: {}, using fallback",
                                e
                            );
                        }
                        Arc::new(LlmDiscoveryService::with_provider(Box::new(
                            StubLlmProvider,
                        )))
                    }
                }
            } else {
                if args.verbose {
                    eprintln!(
                        "[ccos-mcp] No API key found for profile {}, using fallback",
                        args.profile
                    );
                }
                Arc::new(LlmDiscoveryService::with_provider(Box::new(
                    StubLlmProvider,
                )))
            }
        } else {
            if args.verbose {
                eprintln!(
                    "[ccos-mcp] Profile {} not found in config, using fallback",
                    args.profile
                );
            }
            Arc::new(LlmDiscoveryService::with_provider(Box::new(
                StubLlmProvider,
            )))
        }
    } else {
        // No config, try default env-based discovery or fallback
        match LlmDiscoveryService::new().await {
            Ok(svc) => Arc::new(svc),
            Err(_) => Arc::new(LlmDiscoveryService::with_provider(Box::new(
                StubLlmProvider,
            ))),
        }
    };

    // Create shared approval queue for both executor and HTTP UI
    let approval_queue = {
        let storage = ccos::approval::storage_file::FileApprovalStorage::new(
            std::path::PathBuf::from(".ccos/approvals")
        ).expect("Failed to create approval storage");
        UnifiedApprovalQueue::new(std::sync::Arc::new(storage))
    };

    // Create MCP server with tool handlers
    let mut server = MCPServer::new("ccos", env!("CARGO_PKG_VERSION"));

    // Register core CCOS tools
    let session_store = create_session_store();
    register_ccos_tools(
        &mut server,
        marketplace,
        causal_chain,
        discovery_service,
        session_store,
        approval_queue.clone(),
    );

    if args.verbose {
        eprintln!("[ccos-mcp] Registered {} tools", server.tool_count());
    }

    // Run with selected transport
    match args.transport {
        Transport::Http => {
            let config = HttpTransportConfig {
                host: args.host,
                port: args.port,
                keep_alive_secs: 30,
            };
            run_http_transport_with_approvals(server, config, Some(approval_queue)).await?;
        }
        Transport::Stdio => {
            if args.verbose {
                eprintln!("[ccos-mcp] Listening on stdio...");
            }
            server
                .run_stdio()
                .await
                .map_err(|e| -> Box<dyn std::error::Error + Send + Sync> { Box::new(e) })?;
        }
    }

    Ok(())
}

fn register_ccos_tools(
    server: &mut MCPServer,
    marketplace: Arc<CapabilityMarketplace>,
    causal_chain: Arc<Mutex<CausalChain>>,
    discovery_service: Arc<LlmDiscoveryService>,
    session_store: SessionStore,
    approval_queue: UnifiedApprovalQueue<ccos::approval::storage_file::FileApprovalStorage>,
) {
    // ccos/discover_capabilities
    let mp = marketplace.clone();
    server.register_tool(
        "ccos_discover_capabilities",
        "Search for tools matching a query or domain",
        json!({
            "type": "object",
            "properties": {
                "query": { "type": "string", "description": "Search query for capabilities" },
                "domains": {
                    "type": "array",
                    "items": { "type": "string" },
                    "description": "Optional domain filters"
                },
                "include_external": {
                    "type": "boolean",
                    "default": true,
                    "description": "Include external MCP servers"
                }
            },
            "required": ["query"]
        }),
        Box::new(move |params| {
            let mp = mp.clone();
            Box::pin(async move {
                let query = params.get("query").and_then(|v| v.as_str()).unwrap_or("");
                let domains = params.get("domains").and_then(|v| v.as_array());

                let mut q = CapabilityQuery::new();
                if !query.is_empty() {
                    q = q.with_id_pattern(format!("*{}*", query));
                }

                let mut caps = mp.list_capabilities_with_query(&q).await;

                // Manual domain filtering if domains provided
                if let Some(domains) = domains {
                    let domain_strs: Vec<&str> =
                        domains.iter().filter_map(|v| v.as_str()).collect();
                    if !domain_strs.is_empty() {
                        caps = caps
                            .into_iter()
                            .filter(|c| {
                                domain_strs.iter().any(|&d| {
                                    c.id.contains(d) || c.domains.iter().any(|cd| cd.contains(d))
                                })
                            })
                            .collect();
                    }
                }

                Ok(json!({
                    "capabilities": caps.iter().map(|c| json!({
                        "id": c.id,
                        "name": c.name,
                        "description": c.description,
                        "input_schema": c.input_schema
                    })).collect::<Vec<_>>()
                }))
            })
        }),
    );

    // ccos/search_tools
    let mp2 = marketplace.clone();
    server.register_tool(
        "ccos_search_tools",
        "Full-text search over capability descriptions and schemas",
        json!({
            "type": "object",
            "properties": {
                "query": { "type": "string", "description": "Search query" },
                "limit": {
                    "type": "integer",
                    "default": 10,
                    "description": "Maximum results to return"
                }
            },
            "required": ["query"]
        }),
        Box::new(move |params| {
            let mp = mp2.clone();
            Box::pin(async move {
                let query = params.get("query").and_then(|v| v.as_str()).unwrap_or("");
                let limit = params.get("limit").and_then(|v| v.as_u64()).unwrap_or(10) as usize;

                // Full text search across all capabilities
                let all_caps = mp.list_capabilities().await;
                let mut scored_results: Vec<_> = all_caps
                    .into_iter()
                    .map(|c| {
                        let score = calculate_match_score(query, &c.description, &c.id);
                        (c, score)
                    })
                    .filter(|(_, score)| *score > 0.0)
                    .collect();

                // Sort by score descending
                scored_results
                    .sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));

                let total = scored_results.len();
                let limited: Vec<_> = scored_results.into_iter().take(limit).collect();

                Ok(json!({
                    "results": limited.iter().map(|(c, score)| json!({
                        "id": c.id,
                        "name": c.name,
                        "description": c.description,
                        "match_score": score
                    })).collect::<Vec<_>>(),
                    "total": total
                }))
            })
        }),
    );

    // ccos/log_thought
    // Note: Direct CausalChain log_plan_event requires ActionType which is private.
    // For now, we log to stderr and return success. In Phase 2, we'll expose a public
    // logging API or make ActionType public.
    let _cc = causal_chain.clone();
    server.register_tool(
        "ccos_log_thought",
        "Record the agent's reasoning process into the CausalChain for learning",
        json!({
            "type": "object",
            "properties": {
                "plan_id": {
                    "type": "string",
                    "description": "Optional plan ID to associate with"
                },
                "thought": {
                    "type": "string",
                    "description": "The agent's reasoning or thought"
                },
                "context": {
                    "type": "object",
                    "description": "Optional context metadata (e.g., model, confidence)"
                }
            },
            "required": ["thought"]
        }),
        Box::new(move |params| {
            Box::pin(async move {
                let thought = params.get("thought").and_then(|v| v.as_str()).unwrap_or("");
                let plan_id = params
                    .get("plan_id")
                    .and_then(|v| v.as_str())
                    .unwrap_or("agent-thought");

                // TODO: Integrate with CausalChain once we expose a public logging API
                // For now, log to stderr for observability
                eprintln!("[ccos/log_thought] plan={} thought={}", plan_id, thought);

                Ok(json!({
                    "success": true,
                    "logged": true,
                    "note": "Logged to stderr; CausalChain integration pending"
                }))
            })
        }),
    );

    // ccos/list_capabilities - List internal CCOS capabilities from the marketplace
    let mp3 = marketplace.clone();
    server.register_tool(
        "ccos_list_capabilities",
        "List all registered CCOS capabilities from the internal capability marketplace (not MCP tools - use tools/list for that)",
        json!({
            "type": "object",
            "properties": {},
            "required": []
        }),
        Box::new(move |_params| {
            let mp = mp3.clone();
            Box::pin(async move {
                let caps = mp.list_capabilities().await;

                Ok(json!({
                    "capabilities": caps.iter().map(|c| json!({
                        "id": c.id,
                        "name": c.name,
                        "description": c.description
                    })).collect::<Vec<_>>(),
                    "count": caps.len(),
                    "note": "These are internal CCOS capabilities. For MCP tools exposed by this server, use the tools/list MCP method."
                }))
            })
        }),
    );

    // =========================================
    // Phase 2: Static Tools
    // =========================================

    // ccos/analyze_goal - Analyze a user goal to extract intents
    let ds = discovery_service.clone();
    server.register_tool(
        "ccos_analyze_goal",
        "Analyze a user's goal to extract intents, required capabilities, and suggested next steps",
        json!({
            "type": "object",
            "properties": {
                "goal": {
                    "type": "string",
                    "description": "The user's high-level goal to analyze"
                },
                "context": {
                    "type": "object",
                    "description": "Optional context about current state, constraints, etc."
                }
            },
            "required": ["goal"]
        }),
        Box::new(move |params| {
            let ds = ds.clone();
            Box::pin(async move {
                let goal = params.get("goal").and_then(|v| v.as_str()).unwrap_or("");

                if goal.is_empty() {
                    return Ok(json!({
                        "error": "Goal cannot be empty"
                    }));
                }

                // Try to analyze goal using LLM discovery service
                match ds.analyze_goal(goal).await {
                    Ok(analysis) => {
                        // LLM analysis succeeded
                        Ok(json!({
                            "goal": goal,
                            "analysis": {
                                "primary_action": analysis.primary_action,
                                "target_object": analysis.target_object,
                                "domain_keywords": analysis.domain_keywords,
                                "synonyms": analysis.synonyms,
                                "implied_concepts": analysis.implied_concepts,
                                "expanded_queries": analysis.expanded_queries,
                                "confidence": analysis.confidence
                            },
                            "next_steps": analysis.expanded_queries.iter()
                                .map(|q| format!("ccos_search_tools {{ query: '{}' }}", q))
                                .collect::<Vec<_>>()
                        }))
                    }
                    Err(_e) => {
                        // Fallback to keyword-based analysis
                        let fallback = ds.fallback_intent_analysis(goal);
                        Ok(json!({
                            "goal": goal,
                            "analysis": {
                                "primary_action": fallback.primary_action,
                                "target_object": fallback.target_object,
                                "domain_keywords": fallback.domain_keywords,
                                "synonyms": fallback.synonyms,
                                "implied_concepts": fallback.implied_concepts,
                                "expanded_queries": fallback.expanded_queries,
                                "confidence": fallback.confidence
                            },
                            "mode": "fallback",
                            "note": "LLM analysis unavailable, using keyword-based fallback",
                            "next_steps": fallback.expanded_queries.iter()
                                .map(|q| format!("ccos_search_tools {{ query: '{}' }}", q))
                                .collect::<Vec<_>>()
                        }))
                    }
                }
            })
        }),
    );

    // ccos/execute_plan - Execute an RTFS plan
    server.register_tool(
        "ccos_execute_plan",
        "Execute an RTFS plan and return the result. Note: Full execution requires the CCOS CLI.",
        json!({
            "type": "object",
            "properties": {
                "plan": {
                    "type": "string",
                    "description": "The RTFS plan to execute (as a string or file path)"
                },
                "dry_run": {
                    "type": "boolean",
                    "default": false,
                    "description": "If true, validate but don't execute"
                }
            },
            "required": ["plan"]
        }),
        Box::new(move |params| {
            Box::pin(async move {
                let plan = params.get("plan").and_then(|v| v.as_str()).unwrap_or("");
                let dry_run = params
                    .get("dry_run")
                    .and_then(|v| v.as_bool())
                    .unwrap_or(false);

                if plan.is_empty() {
                    return Ok(json!({
                        "success": false,
                        "error": "Plan cannot be empty"
                    }));
                }

                // Parse the plan to validate syntax
                let parse_result = rtfs::parser::parse(plan);
                let is_valid = parse_result.is_ok();
                let parse_error = parse_result.err().map(|e| format!("{:?}", e));

                if dry_run || !is_valid {
                    return Ok(json!({
                        "success": is_valid,
                        "dry_run": true,
                        "valid": is_valid,
                        "parse_error": parse_error,
                        "plan_preview": plan.chars().take(200).collect::<String>(),
                        "message": if is_valid {
                            "Plan syntax is valid"
                        } else {
                            "Plan has syntax errors"
                        }
                    }));
                }

                // For actual execution, use spawn_blocking to run in a thread pool
                // Note: Full execution is complex due to RTFS runtime thread-local state
                // For now, provide parsed plan info and suggest using ccos CLI for execution
                Ok(json!({
                    "success": true,
                    "validated": true,
                    "plan_preview": plan.chars().take(200).collect::<String>(),
                    "note": "Plan validated. For full execution, use: ccos plan execute <plan-file>",
                    "execution_hint": "The MCP server can validate plans but full execution requires the CCOS CLI due to runtime threading constraints."
                }))
            })
        }),
    );

    // ccos/resolve_intent - Find capabilities matching an intent
    let mp4 = marketplace.clone();
    let ds4 = discovery_service.clone();
    server.register_tool(
        "ccos_resolve_intent",
        "Find capabilities that can fulfill a given intent description",
        json!({
            "type": "object",
            "properties": {
                "intent": {
                    "type": "string",
                    "description": "Natural language description of what you want to accomplish"
                },
                "domain": {
                    "type": "string",
                    "description": "Optional domain to filter by (e.g., 'github', 'file', 'http')"
                },
                "min_score": {
                    "type": "number",
                    "default": 0.5,
                    "description": "Minimum relevance score (0-1)"
                }
            },
            "required": ["intent"]
        }),
        Box::new(move |params| {
            let mp = mp4.clone();
            let ds = ds4.clone();
            Box::pin(async move {
                let intent_str = params.get("intent").and_then(|v| v.as_str()).unwrap_or("");
                let domain = params.get("domain").and_then(|v| v.as_str());
                let min_score = params
                    .get("min_score")
                    .and_then(|v| v.as_f64())
                    .unwrap_or(0.5);

                if intent_str.is_empty() {
                    return Ok(json!({
                        "capabilities": [],
                        "error": "Intent cannot be empty"
                    }));
                }

                // 1. Get all capabilities from marketplace
                let all_caps = mp.list_capabilities().await;

                // 2. Filter by domain if specified
                let mut filtered_caps = all_caps;
                if let Some(d) = domain {
                    filtered_caps = filtered_caps.into_iter().filter(|c| c.id.contains(d)).collect();
                }

                if filtered_caps.is_empty() {
                    return Ok(json!({
                        "intent": intent_str,
                        "capabilities": [],
                        "total": 0,
                        "note": "No capabilities found matching the criteria"
                    }));
                }

                // 3. Analyze intent to get context
                let analysis = ds.analyze_goal(intent_str).await.ok();

                // 4. Map capabilities to RegistrySearchResult for ranking
                use ccos::approval::queue::{DiscoverySource, ServerInfo};
                use ccos::discovery::registry_search::RegistrySearchResult;

                let search_results: Vec<RegistrySearchResult> = filtered_caps
                    .iter()
                    .map(|c| RegistrySearchResult {
                        source: DiscoverySource::Manual { user: "marketplace".to_string() },
                        server_info: ServerInfo {
                            name: c.id.clone(),
                            endpoint: String::new(),
                            description: Some(c.description.clone()),
                            auth_env_var: None,
                            capabilities_path: None,
                            alternative_endpoints: Vec::new(),
                        },
                        match_score: calculate_match_score(intent_str, &c.description, &c.id) as f32,
                        alternative_endpoints: Vec::new(),
                    })
                    .collect();

                // 5. Rank results using LLM
                let ranked = match ds.rank_results(intent_str, analysis.as_ref(), search_results).await {
                    Ok(r) => r,
                    Err(_) => {
                        // Fallback to basic scoring if ranking fails
                        return Ok(json!({
                            "intent": intent_str,
                            "capabilities": filtered_caps.iter().map(|c| {
                                let score = calculate_match_score(intent_str, &c.description, &c.id);
                                json!({
                                    "id": c.id,
                                    "name": c.name,
                                    "description": c.description,
                                    "score": score
                                })
                            }).filter(|v| v["score"].as_f64().unwrap_or(0.0) >= min_score as f64).collect::<Vec<_>>(),
                            "mode": "basic_search"
                        }));
                    }
                };

                // 6. Return top matches
                let final_results: Vec<_> = ranked.into_iter()
                    .filter(|r| r.llm_score >= min_score)
                    .map(|r| json!({
                        "id": r.result.server_info.name,
                        "description": r.result.server_info.description,
                        "score": r.llm_score,
                        "reasoning": r.reasoning
                    }))
                    .collect();

                Ok(json!({
                    "intent": intent_str,
                    "capabilities": final_results,
                    "total": final_results.len(),
                    "analysis": analysis
                }))
            })
        }),
    );


    // ============================================================================
    // Planning Tool - ccos_plan_goal
    // ============================================================================

    // ccos_plan_goal - Generate a structured execution plan for a goal
    // This is the PRIMARY tool for agents to get actionable steps
    let mp_plan = marketplace.clone();
    let ds_plan = discovery_service.clone();
    server.register_tool(
        "ccos_plan_goal",
        "PRIMARY: Generate a structured execution plan with ordered steps. Returns capability IDs with parameter wiring ($step_N.field). Use this instead of manually sequencing capabilities.",
        json!({
            "type": "object",
            "properties": {
                "goal": {
                    "type": "string",
                    "description": "The goal to accomplish (e.g., 'get weather in paris tomorrow')"
                },
                "preferences": {
                    "type": "object",
                    "description": "Optional preferences (units, language, etc.)"
                }
            },
            "required": ["goal"]
        }),
        Box::new(move |params| {
            let mp = mp_plan.clone();
            let ds = ds_plan.clone();
            Box::pin(async move {
                let goal = params.get("goal").and_then(|v| v.as_str()).unwrap_or("");
                let _preferences = params.get("preferences").cloned().unwrap_or(json!({}));

                if goal.is_empty() {
                    return Ok(json!({
                        "success": false,
                        "error": "Goal is required"
                    }));
                }

                // Step 1: Analyze the goal to get domain keywords and concepts
                let analysis = match ds.analyze_goal(goal).await {
                    Ok(a) => a,
                    Err(_) => ds.fallback_intent_analysis(goal),
                };

                // Step 2: Get all capabilities from marketplace
                let all_caps = mp.list_capabilities().await;

                // Step 3: Find relevant capabilities using domain keywords
                let mut scored_caps: Vec<_> = all_caps
                    .iter()
                    .map(|c| {
                        let score = calculate_match_score(goal, &c.description, &c.id);
                        (c, score)
                    })
                    .filter(|(_, score)| *score > 0.2)
                    .collect();

                scored_caps.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
                let top_caps: Vec<_> = scored_caps.into_iter().take(10).collect();

                if top_caps.is_empty() {
                    return Ok(json!({
                        "success": false,
                        "goal": goal,
                        "error": "No matching capabilities found",
                        "analysis": {
                            "domain_keywords": analysis.domain_keywords,
                            "implied_concepts": analysis.implied_concepts
                        },
                        "suggestions": [
                            "Try 'ccos_discover_capabilities' with different keywords",
                            "Check if required MCP servers are connected"
                        ]
                    }));
                }

                // Step 4: Determine execution order based on capability types
                let mut execution_steps = Vec::new();
                let mut step_number = 1;

                // Build a simple sequential plan based on top matches
                // In a more advanced version, this would use a proper planner
                for (cap, score) in top_caps.iter().take(3) {
                    let mut inputs = json!({});
                    
                    // Simple heuristic: if not the first step, try to pipe the previous step's output
                    if step_number > 1 {
                        inputs = json!({
                            "context": format!("$step_{}", step_number - 1)
                        });
                    }

                    execution_steps.push(json!({
                        "step": step_number,
                        "capability_id": cap.id,
                        "capability_name": cap.name,
                        "description": cap.description,
                        "inputs": inputs,
                        "outputs": [],
                        "match_score": *score
                    }));
                    step_number += 1;
                }

                // Step 6: Calculate feasibility
                let feasibility = if execution_steps.is_empty() {
                    0.0
                } else {
                    let avg_score: f64 = execution_steps.iter()
                        .filter_map(|s| s.get("match_score").and_then(|v| v.as_f64()))
                        .sum::<f64>() / execution_steps.len() as f64;
                    (avg_score * 100.0).round() / 100.0
                };

                let ready_to_execute = feasibility >= 0.5 && !execution_steps.is_empty();

                // Step 7: Build final output expression
                let final_output = if step_number > 1 {
                    format!("$step_{}", step_number - 1)
                } else {
                    "$result".to_string()
                };

                Ok(json!({
                    "success": true,
                    "goal": goal,
                    "feasibility": feasibility,
                    "execution_plan": {
                        "steps": execution_steps,
                        "final_output": final_output,
                        "step_count": execution_steps.len()
                    },
                    "analysis": {
                        "primary_action": analysis.primary_action,
                        "target_object": analysis.target_object,
                        "domain_keywords": analysis.domain_keywords,
                        "confidence": analysis.confidence
                    },
                    "ready_to_execute": ready_to_execute,
                    "usage_hint": if ready_to_execute {
                        "Use ccos_start_session, then call ccos_execute_capability for each step in order"
                    } else {
                        "Some capabilities may be missing. Use ccos_discover_capabilities to find alternatives"
                    }
                }))
            })
        }),
    );

    // ============================================================================
    // Session Management Tools
    // ============================================================================


    // ccos_start_session - Start a new planning/execution session
    let ss1 = session_store.clone();
    server.register_tool(
        "ccos_start_session",
        "Start a new planning/execution session. Returns a session_id for tracking steps.",
        json!({
            "type": "object",
            "properties": {
                "goal": {
                    "type": "string",
                    "description": "The goal you're trying to accomplish"
                },
                "context": {
                    "type": "object",
                    "description": "Optional context (user preferences, constraints, etc.)"
                }
            },
            "required": ["goal"]
        }),
        Box::new(move |params| {
            let ss = ss1.clone();
            Box::pin(async move {
                let goal = params.get("goal").and_then(|v| v.as_str()).unwrap_or("");
                let context = params.get("context").cloned();

                if goal.is_empty() {
                    return Ok(json!({
                        "success": false,
                        "error": "Goal is required"
                    }));
                }

                let mut session = Session::new(goal);
                if let Some(ctx) = context {
                    if let Some(obj) = ctx.as_object() {
                        for (k, v) in obj {
                            session.context.insert(k.clone(), v.clone());
                        }
                    }
                }

                let session_id = session.id.clone();
                let created_at = session.created_at.clone();

                let mut store = ss.write().await;
                store.insert(session_id.clone(), session);

                Ok(json!({
                    "success": true,
                    "session_id": session_id,
                    "goal": goal,
                    "created_at": created_at
                }))
            })
        }),
    );

    // ccos_execute_capability - Execute a capability with JSON inputs
    let ss2 = session_store.clone();
    let mp_exec = marketplace.clone();
    let cc_exec = causal_chain.clone();
    let _aq_exec = approval_queue.clone();
    let aq_check = approval_queue.clone();
    server.register_tool(
        "ccos_execute_capability",
        "PRIMARY WAY TO CALL CAPABILITIES. Execute a capability with JSON inputs - no RTFS syntax knowledge needed. Use this instead of trying to write RTFS code.",
        json!({
            "type": "object",
            "properties": {
                "capability_id": {
                    "type": "string",
                    "description": "The capability ID to execute (e.g., 'geocoding_api.direct_geocoding_by_location_name')"
                },
                "inputs": {
                    "type": "object",
                    "description": "Input parameters as JSON object"
                },
                "session_id": {
                    "type": "string",
                    "description": "Optional session ID to track this execution"
                }
            },
            "required": ["capability_id", "inputs"]
        }),
        Box::new(move |params| {
            let ss = ss2.clone();
            let mp = mp_exec.clone();
            let cc = cc_exec.clone();
            Box::pin(async move {
                let capability_id = params.get("capability_id").and_then(|v| v.as_str()).unwrap_or("");
                let inputs = params.get("inputs").cloned().unwrap_or(json!({}));
                let session_id = params.get("session_id").and_then(|v| v.as_str());

                if capability_id.is_empty() {
                    return Ok(json!({
                        "success": false,
                        "error": "capability_id is required"
                    }));
                }

                // Generate RTFS equivalent for teaching
                let rtfs_equivalent = Session::inputs_to_rtfs(capability_id, &inputs);

                // Execute the capability via CCOS CapabilityMarketplace
                // Convert serde_json::Value to rtfs::runtime::Value
                let rtfs_inputs = match json_to_rtfs_value(&inputs) {
                    Ok(v) => v,
                    Err(e) => {
                        return Ok(json!({
                            "success": false,
                            "error": format!("Failed to convert inputs: {}", e)
                        }));
                    }
                };

                // Convert inputs to RTFS args for causal chain logging
                let rtfs_args: Vec<rtfs::runtime::values::Value> = if let rtfs::runtime::values::Value::Map(m) = &rtfs_inputs {
                    m.values().cloned().collect()
                } else {
                    vec![rtfs_inputs.clone()]
                };

                // Use session_id as intent_id, or generate one
                let intent_id = session_id.map(|s| s.to_string()).unwrap_or_else(|| format!("mcp-intent-{}", chrono::Utc::now().timestamp_millis()));
                let plan_id = format!("mcp-plan-{}", chrono::Utc::now().timestamp_millis());

                // Log capability call to causal chain BEFORE execution
                let action = match cc.try_lock() {
                    Ok(mut chain) => {
                        match chain.log_capability_call(
                            &plan_id,
                            &intent_id,
                            &capability_id.to_string(),
                            capability_id,
                            rtfs_args,
                        ) {
                            Ok(a) => Some(a),
                            Err(e) => {
                                eprintln!("[ccos-mcp] Warning: Failed to log capability call: {}", e);
                                None
                            }
                        }
                    }
                    Err(_) => {
                        eprintln!("[ccos-mcp] Warning: Could not acquire causal chain lock for logging");
                        None
                    }
                };


                // Use block_in_place to handle non-Send future from RTFS runtime
                // This safely runs the future on the current thread
                let cap_id = capability_id.to_string();
                let execution_result = tokio::task::block_in_place(|| {
                    tokio::runtime::Handle::current().block_on(async {
                        mp.execute_capability(&cap_id, &rtfs_inputs).await
                    })
                });

                let (result, success, rtfs_result) = match execution_result {
                    Ok(rtfs_value) => {
                        // Convert rtfs::Value back to serde_json::Value
                        match rtfs_value_to_json(&rtfs_value) {
                            Ok(json_val) => (json_val, true, Some(rtfs_value)),
                            Err(e) => (json!({
                                "error": format!("Failed to convert result: {}", e),
                                "raw_type": format!("{:?}", rtfs_value)
                            }), false, None)
                        }
                    }
                    Err(e) => (json!({
                        "error": format!("{}", e),
                        "capability_id": capability_id,
                        "inputs_received": inputs.clone()
                    }), false, None)
                };

                // Record result to causal chain AFTER execution
                if let Some(action) = action {
                    let exec_result = ccos::types::ExecutionResult {
                        success,
                        value: rtfs_result.unwrap_or(rtfs::runtime::values::Value::Nil),
                        metadata: std::collections::HashMap::new(),
                    };
                    match cc.try_lock() {
                        Ok(mut chain) => {
                            if let Err(e) = chain.record_result(action, exec_result) {
                                eprintln!("[ccos-mcp] Warning: Failed to record result: {}", e);
                            }
                        }
                        Err(_) => {
                            eprintln!("[ccos-mcp] Warning: Could not acquire causal chain lock for recording result");
                        }
                    }
                }

                // Record in session if provided
                let step_info = if let Some(sid) = session_id {
                    let mut store = ss.write().await;
                    if let Some(session) = store.get_mut(sid) {
                        let step = session.add_step(capability_id, inputs.clone(), result.clone(), success);
                        Some(json!({
                            "step_number": step.step_number,
                            "session_id": sid
                        }))
                    } else {
                        None
                    }
                } else {
                    None
                };


                Ok(json!({
                    "success": success,
                    "result": result,
                    "capability_id": capability_id,
                    "rtfs_equivalent": rtfs_equivalent,
                    "rtfs_tip": "In RTFS, capabilities are called using: (call \"capability.id\" :param value)",
                    "session": step_info
                }))
            })
        }),
    );


    // ccos_get_session_plan - Get the accumulated RTFS plan from a session
    let ss3 = session_store.clone();
    server.register_tool(
        "ccos_get_session_plan",
        "Get the accumulated RTFS plan from all steps in a session.",
        json!({
            "type": "object",
            "properties": {
                "session_id": {
                    "type": "string",
                    "description": "The session ID"
                }
            },
            "required": ["session_id"]
        }),
        Box::new(move |params| {
            let ss = ss3.clone();
            Box::pin(async move {
                let session_id = params
                    .get("session_id")
                    .and_then(|v| v.as_str())
                    .unwrap_or("");

                if session_id.is_empty() {
                    return Ok(json!({
                        "success": false,
                        "error": "session_id is required"
                    }));
                }

                let store = ss.read().await;
                if let Some(session) = store.get(session_id) {
                    let rtfs_plan = session.to_rtfs_plan();
                    let steps_summary: Vec<_> = session
                        .steps
                        .iter()
                        .map(|s| {
                            json!({
                                "step": s.step_number,
                                "capability": s.capability_id,
                                "success": s.success,
                                "rtfs": s.rtfs_code
                            })
                        })
                        .collect();

                    Ok(json!({
                        "success": true,
                        "session_id": session_id,
                        "goal": session.goal,
                        "steps_count": session.steps.len(),
                        "steps": steps_summary,
                        "rtfs_plan": rtfs_plan,
                        "created_at": session.created_at
                    }))
                } else {
                    Ok(json!({
                        "success": false,
                        "error": format!("Session '{}' not found", session_id)
                    }))
                }
            })
        }),
    );

    // ccos_end_session - Finalize and optionally save the session plan
    let ss4 = session_store.clone();
    server.register_tool(
        "ccos_end_session",
        "End a session and optionally save the generated RTFS plan to a file.",
        json!({
            "type": "object",
            "properties": {
                "session_id": {
                    "type": "string",
                    "description": "The session ID to end"
                },
                "save_as": {
                    "type": "string",
                    "description": "Optional filename to save the RTFS plan (e.g., 'my-plan.rtfs')"
                }
            },
            "required": ["session_id"]
        }),
        Box::new(move |params| {
            let ss = ss4.clone();
            Box::pin(async move {
                let session_id = params
                    .get("session_id")
                    .and_then(|v| v.as_str())
                    .unwrap_or("");
                let save_as = params.get("save_as").and_then(|v| v.as_str());

                if session_id.is_empty() {
                    return Ok(json!({
                        "success": false,
                        "error": "session_id is required"
                    }));
                }

                let mut store = ss.write().await;
                if let Some(session) = store.remove(session_id) {
                    let rtfs_plan = session.to_rtfs_plan();
                    let steps_count = session.steps.len();

                    // Optionally save to file
                    let saved_path = if let Some(filename) = save_as {
                        let path = std::path::Path::new(filename);
                        match std::fs::write(path, &rtfs_plan) {
                            Ok(_) => Some(filename.to_string()),
                            Err(e) => {
                                return Ok(json!({
                                    "success": false,
                                    "error": format!("Failed to save plan: {}", e),
                                    "rtfs_plan": rtfs_plan
                                }));
                            }
                        }
                    } else {
                        None
                    };

                    Ok(json!({
                        "success": true,
                        "session_id": session_id,
                        "goal": session.goal,
                        "steps_executed": steps_count,
                        "rtfs_plan": rtfs_plan,
                        "saved_to": saved_path
                    }))
                } else {
                    Ok(json!({
                        "success": false,
                        "error": format!("Session '{}' not found", session_id)
                    }))
                }
            })
        }),
    );
    // ccos_synthesize_capability - Generate RTFS capability using LLM
    let ds = discovery_service.clone();
    server.register_tool(
        "ccos_synthesize_capability",
        "Synthesize a new RTFS capability using LLM with guardrails. Returns generated code and quality metrics.",
        json!({
            "type": "object",
            "properties": {
                "description": {
                    "type": "string",
                    "description": "What the capability should do"
                },
                "capability_name": {
                    "type": "string",
                    "description": "Desired capability ID (e.g., 'github.create_issue')"
                },
                "input_schema": {
                    "type": "object",
                    "description": "Expected inputs as JSON Schema"
                },
                "output_schema": {
                    "type": "object",
                    "description": "Expected outputs as JSON Schema"
                }
            },
            "required": ["description"]
        }),
        Box::new(move |params| {
            let _ds = ds.clone();
            Box::pin(async move {
                let description = params.get("description")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string();
                let capability_name = params.get("capability_name")
                    .and_then(|v| v.as_str())
                    .map(|s| s.to_string())
                    .unwrap_or_else(|| format!("synthesized.cap_{}", std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0)));
                let input_schema = params.get("input_schema").cloned();
                let output_schema = params.get("output_schema").cloned();

                if description.is_empty() {
                    return Ok(json!({
                        "success": false,
                        "error": "description is required"
                    }));
                }

                // Get LLM provider from discovery service
                let llm_provider = Arc::new(StubLlmProvider);

                // Create synthesizer with LLM provider
                let synthesizer = CapabilitySynthesizer::with_llm_provider(llm_provider);

                // Build synthesis request
                let request = SynthesisRequest {
                    capability_name: capability_name.clone(),
                    input_schema,
                    output_schema,
                    requires_auth: false,
                    auth_provider: None,
                    description: Some(description.clone()),
                    context: None,
                };

                // Synthesize capability
                match synthesizer.synthesize_capability(&request).await {
                    Ok(result) => Ok(json!({
                        "success": true,
                        "capability_id": result.capability.id,
                        "rtfs_code": result.implementation_code,
                        "quality_score": result.quality_score,
                        "safety_passed": result.safety_passed,
                        "warnings": result.warnings
                    })),
                    Err(e) => Ok(json!({
                        "success": false,
                        "error": e.to_string()
                    }))
                }
            })
        }),
    );

    // ccos_compile_rtfs - Validate RTFS syntax without execution
    server.register_tool(
        "ccos_compile_rtfs",
        "ADVANCED: Validate RTFS code syntax. WARNING: This only checks syntax, does NOT execute. To execute capabilities, use ccos_execute_capability instead.",
        json!({
            "type": "object",
            "properties": {
                "code": {
                    "type": "string",
                    "description": "RTFS source code to compile and validate"
                }
            },
            "required": ["code"]
        }),
        Box::new(move |params| {
            Box::pin(async move {
                let code = params.get("code")
                    .and_then(|v| v.as_str())
                    .unwrap_or("");

                if code.is_empty() {
                    return Ok(json!({
                        "valid": false,
                        "error": "code is required"
                    }));
                }

                // Try to parse the RTFS code
                match rtfs::parser::parse(code) {
                    Ok(ast) => {
                        // Summarize the AST
                        let top_level_count = ast.len();
                        let mut capabilities = Vec::new();
                        let mut functions = Vec::new();

                        for item in &ast {
                            match item {
                                rtfs::ast::TopLevel::Capability(cap) => {
                                    capabilities.push(cap.name.clone());
                                }
                                rtfs::ast::TopLevel::Module(m) => {
                                    functions.push(m.name.0.clone());
                                }
                                _ => {}
                            }
                        }

                        Ok(json!({
                            "valid": true,
                            "ast_summary": {
                                "top_level_items": top_level_count,
                                "capabilities": capabilities,
                                "functions": functions
                            }
                        }))
                    }
                    Err(e) => {
                        Ok(json!({
                            "valid": false,
                            "error": format!("{}", e)
                        }))
                    }
                }
            })
        }),
    );

    // ccos_list_secrets - List all approved secret names and mappings
    server.register_tool(
        "ccos_list_secrets",
        "List all approved secret names and environment variable mappings (values are not revealed). Use this to see what secrets are currently configured in the system.",
        json!({
            "type": "object",
            "properties": {}
        }),
        Box::new(move |_params| {
            Box::pin(async move {
                let store = SecretStore::new(Some(get_workspace_root()))
                    .unwrap_or_else(|_| SecretStore::new(None).unwrap_or_else(|_| {
                        panic!("Failed to create SecretStore")
                    }));

                let local = store.list_local();
                let mut mappings = HashMap::new();
                for name in store.list_mappings() {
                    if let Some(target) = store.get_mapping(name) {
                        mappings.insert(name.to_string(), target.clone());
                    }
                }

                Ok(json!({
                    "success": true,
                    "approved_secrets": local,
                    "mappings": mappings,
                    "hint": "These secrets are available for use in capabilities. If a required secret is missing, it must be approved first."
                }))
            })
        })
    );

    // ccos_check_secrets - Check availability of required secrets without revealing values
    server.register_tool(
        "ccos_check_secrets",
        "Check which secrets are available or missing for capability execution. Returns availability status without exposing actual values.",
        json!({
            "type": "object",
            "properties": {
                "secret_names": {
                    "type": "array",
                    "items": { "type": "string" },
                    "description": "List of secret/env var names to check (e.g., ['OPENWEATHERMAP_API_KEY', 'GITHUB_TOKEN'])"
                }
            },
            "required": ["secret_names"]
        }),
        Box::new(move |params| {
            let aq = aq_check.clone();
            Box::pin(async move {
                let secret_names: Vec<String> = params.get("secret_names")
                    .and_then(|v| serde_json::from_value(v.clone()).ok())
                    .unwrap_or_default();

                if secret_names.is_empty() {
                    return Ok(json!({
                        "error": "secret_names array is required",
                        "available": [],
                        "missing": []
                    }));
                }

                // Create a SecretStore for the workspace root
                let store = SecretStore::new(Some(get_workspace_root()))
                    .unwrap_or_else(|_| SecretStore::new(None).unwrap_or_else(|_| {
                        // Fallback to a store that just checks env vars
                        panic!("Failed to create SecretStore")
                    }));

                let mut available = Vec::new();
                let mut missing = Vec::new();
                let mut missing_details = Vec::new();

                for name in &secret_names {
                    if store.has(name) {
                        available.push(name.clone());
                    } else {
                        missing.push(name.clone());
                        missing_details.push(json!({
                            "name": name,
                            "status": "not_available"
                        }));
                    }
                }

                Ok(json!({
                    "success": missing.is_empty(),
                    "available": available,
                    "missing": missing,
                    "missing_details": missing_details,
                    "hint": if missing.is_empty() {
                        "All requested secrets are available.".to_string()
                    } else {
                        format!("Missing secrets must be provided by the user via the approval UI at http://localhost:3000/approvals. Do NOT attempt to find or guess secrets yourself. You are FORBIDDEN from accessing raw secret values.")
                    },
                    "agent_guidance": if missing.is_empty() {
                        "All required secrets are available. You may proceed with capability execution.".to_string()
                    } else {
                        "CRITICAL: Missing secrets detected. You MUST stop execution now and ask the user to visit http://localhost:3000/approvals. Do NOT attempt to use alternative public APIs, web search, or any other workaround to bypass this restriction. Your primary obligation is to guide the user to the secure approval flow.".to_string()
                    }
                }))
            })
        }),
    );

    // ccos_introspect_server - Introspect an external MCP/OpenAPI server and queue for approval
    let aq_introspect = approval_queue.clone();
    let mp_introspect = marketplace.clone();
    server.register_tool(
        "ccos_introspect_server",
        "Introspect an external MCP or OpenAPI server to discover its tools. Creates an approval request for the server.",
        json!({
            "type": "object",
            "properties": {
                "endpoint": {
                    "type": "string",
                    "description": "Server endpoint URL (e.g., 'https://api.example.com') or npx command (e.g., 'npx -y @modelcontextprotocol/server-github')"
                },
                "name": {
                    "type": "string",
                    "description": "Optional display name for the server"
                },
                "auth_env_var": {
                    "type": "string",
                    "description": "Optional environment variable name for auth token (e.g., 'GITHUB_TOKEN')"
                }
            },
            "required": ["endpoint"]
        }),
        Box::new(move |params| {
            let aq = aq_introspect.clone();
            let mp = mp_introspect.clone();
            Box::pin(async move {
                let endpoint = params.get("endpoint").and_then(|v| v.as_str()).unwrap_or("");
                let name = params.get("name").and_then(|v| v.as_str()).map(|s| s.to_string());
                let auth_env_var = params.get("auth_env_var").and_then(|v| v.as_str()).map(|s| s.to_string());

                if endpoint.is_empty() {
                    return Ok(json!({
                        "success": false,
                        "error": "endpoint is required"
                    }));
                }

                // Create server config for introspection
                let server_name = name.clone().unwrap_or_else(|| {
                    // Extract name from endpoint
                    endpoint.split('/').last().unwrap_or("unknown").to_string()
                });

                let config = MCPServerConfig {
                    name: server_name.clone(),
                    endpoint: endpoint.to_string(),
                    auth_token: auth_env_var.as_ref().and_then(|var| std::env::var(var).ok()),
                    timeout_seconds: 30,
                    protocol_version: "2024-11-05".to_string(),
                };

                // Try to introspect the server
                let discovery_result = match MCPDiscoveryProvider::new_with_rtfs_host_factory(
                    config.clone(),
                    mp.get_rtfs_host_factory(),
                ) {
                    Ok(provider) => {
                        match provider.discover_tools().await {
                            Ok(caps) => Some(caps),

                            Err(e) => {
                                return Ok(json!({
                                    "success": false,
                                    "error": format!("Failed to introspect server: {}", e),
                                    "hint": "Check if the server is running and accessible"
                                }));
                            }
                        }
                    }
                    Err(e) => {
                        return Ok(json!({
                            "success": false,
                            "error": format!("Failed to create discovery provider: {}", e)
                        }));
                    }
                };

                let capabilities = discovery_result.unwrap_or_default();

                // Create ServerInfo
                let server_info = ccos::approval::queue::ServerInfo {
                    name: server_name.clone(),
                    endpoint: endpoint.to_string(),
                    description: Some(format!("Discovered {} capabilities", capabilities.len())),
                    auth_env_var,
                    capabilities_path: None,
                    alternative_endpoints: vec![],
                };

                // Create approval request
                let approval_id = match aq.add_server_discovery(
                    ccos::approval::queue::DiscoverySource::Manual { user: "agent".to_string() },
                    server_info,
                    vec!["dynamic".to_string()],
                    RiskAssessment {
                        level: RiskLevel::Medium,
                        reasons: vec!["Dynamically discovered server".to_string()],
                    },
                    Some("Agent requested introspection".to_string()),
                    24, // expires in 24 hours
                ).await {
                    Ok(id) => id,
                    Err(e) => {
                        return Ok(json!({
                            "success": false,
                            "error": format!("Failed to create approval request: {}", e)
                        }));
                    }
                };

                // Return discovered tools preview
                let tool_previews: Vec<_> = capabilities.iter().take(10).map(|c| {
                    json!({
                        "id": c.id,
                        "name": c.name,
                        "description": c.description
                    })
                }).collect();

                Ok(json!({
                    "success": true,
                    "approval_id": approval_id,
                    "server_name": server_name,
                    "discovered_tools_count": capabilities.len(),
                    "tool_previews": tool_previews,
                    "agent_guidance": format!(
                        "Server '{}' discovered with {} tools. Approval required before tools can be used. \
                         Once approved, call ccos_register_server with approval_id: '{}'",
                        server_name, capabilities.len(), approval_id
                    )
                }))
            })
        }),
    );

    // ccos_register_server - Register an approved server's tools into the marketplace
    let aq_register = approval_queue.clone();
    let mp_register = marketplace.clone();
    server.register_tool(
        "ccos_register_server",
        "Register an approved server's tools into the marketplace. The server must have been approved via the approval queue.",
        json!({
            "type": "object",
            "properties": {
                "approval_id": {
                    "type": "string",
                    "description": "The approval ID returned from ccos_introspect_server"
                }
            },
            "required": ["approval_id"]
        }),
        Box::new(move |params| {
            let aq = aq_register.clone();
            let mp = mp_register.clone();
            Box::pin(async move {
                let approval_id = params.get("approval_id").and_then(|v| v.as_str()).unwrap_or("");

                if approval_id.is_empty() {
                    return Ok(json!({
                        "success": false,
                        "error": "approval_id is required"
                    }));
                }

                // Get the approval request
                let request = match aq.get(approval_id).await {
                    Ok(Some(req)) => req,
                    Ok(None) => {
                        return Ok(json!({
                            "success": false,
                            "error": "Approval request not found",
                            "hint": "The approval may have expired or the ID is incorrect"
                        }));
                    }
                    Err(e) => {
                        return Ok(json!({
                            "success": false,
                            "error": format!("Failed to get approval request: {}", e)
                        }));
                    }
                };

                // Check if approved
                if !matches!(request.status, ccos::approval::types::ApprovalStatus::Approved { .. }) {
                    return Ok(json!({
                        "success": false,
                        "error": "Server has not been approved yet",
                        "status": format!("{:?}", request.status),
                        "hint": "The server must be approved before its tools can be registered. Check the approvals UI."
                    }));
                }

                // Extract server info
                let (server_name, endpoint, auth_env_var) = match &request.category {
                    ApprovalCategory::ServerDiscovery { server_info, .. } => {
                        (server_info.name.clone(), server_info.endpoint.clone(), server_info.auth_env_var.clone())
                    }
                    _ => {
                        return Ok(json!({
                            "success": false,
                            "error": "Approval is not for a server discovery"
                        }));
                    }
                };

                // Create config and discover capabilities
                let config = MCPServerConfig {
                    name: server_name.clone(),
                    endpoint: endpoint.clone(),
                    auth_token: auth_env_var.as_ref().and_then(|var| std::env::var(var).ok()),
                    timeout_seconds: 30,
                    protocol_version: "2024-11-05".to_string(),
                };

                let capabilities = match MCPDiscoveryProvider::new_with_rtfs_host_factory(
                    config,
                    mp.get_rtfs_host_factory(),
                ) {
                    Ok(provider) => {
                        match provider.discover_tools().await {
                            Ok(caps) => caps,

                            Err(e) => {
                                return Ok(json!({
                                    "success": false,
                                    "error": format!("Failed to rediscover server capabilities: {}", e)
                                }));
                            }
                        }
                    }
                    Err(e) => {
                        return Ok(json!({
                            "success": false,
                            "error": format!("Failed to create discovery provider: {}", e)
                        }));
                    }
                };

                // Register each capability in the marketplace
                let mut registered_count = 0;
                for cap in &capabilities {
                    if let Err(e) = mp.register_capability_manifest(cap.clone()).await {
                        eprintln!("[ccos-mcp] Warning: Failed to register capability {}: {}", cap.id, e);
                    } else {
                        registered_count += 1;
                    }
                }


                Ok(json!({
                    "success": true,
                    "server_name": server_name,
                    "registered_count": registered_count,
                    "total_discovered": capabilities.len(),
                    "agent_guidance": format!(
                        "Successfully registered {} tools from '{}'. They are now available via ccos_search_tools and ccos_execute_capability.",
                        registered_count, server_name
                    )
                }))
            })
        }),
    );
}

/// Calculate a simple match score between intent and capability
fn calculate_match_score(intent: &str, description: &str, id: &str) -> f64 {
    let intent_lower = intent.to_lowercase();
    let desc_lower = description.to_lowercase();
    let id_lower = id.to_lowercase();

    // Split on whitespace, dots, and underscores to tokenize capability IDs properly
    let tokenize = |s: &str| -> Vec<String> {
        s.split(|c: char| c.is_whitespace() || c == '.' || c == '_' || c == '-')
            .filter(|w| !w.is_empty() && w.len() > 1) // Skip empty and single-char tokens
            .map(|s| s.to_string())
            .collect()
    };

    let intent_words = tokenize(&intent_lower);
    let desc_words = tokenize(&desc_lower);
    let id_words = tokenize(&id_lower);

    let mut matches = 0.0;

    for word in &intent_words {
        // Check description words
        for dw in &desc_words {
            if dw.contains(word.as_str()) || word.contains(dw.as_str()) {
                matches += 1.0;
                break; // Only count each word once
            }
        }
        // Check ID words
        for iw in &id_words {
            if iw.contains(word.as_str()) || word.contains(iw.as_str()) {
                matches += 0.5;
                break;
            }
        }
    }

    let max_score = intent_words.len() as f64 * 1.5;
    if max_score > 0.0 {
        (matches / max_score).min(1.0)
    } else {
        0.0
    }
}

/// Extract a city name from a goal string (simple heuristic)


fn collect_rtfs_files_recursive(dir: &std::path::Path, rtfs_files: &mut Vec<std::path::PathBuf>) {
    if let Ok(entries) = std::fs::read_dir(dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                collect_rtfs_files_recursive(&path, rtfs_files);
            } else if path.extension().and_then(|s| s.to_str()) == Some("rtfs") {
                rtfs_files.push(path);
            }
        }
    }
}

async fn load_capabilities_from_dir(
    marketplace: &CapabilityMarketplace,
    relative_path: &str,
    verbose: bool,
) {
    let workspace_root = get_workspace_root();
    let approved_dir = workspace_root.join(relative_path);

    if !approved_dir.exists() {
        if verbose {
            eprintln!(
                "[ccos-mcp] Warning: Capabilities directory not found: {:?}",
                approved_dir
            );
        }
        return;
    }

    let mut rtfs_files = Vec::new();
    collect_rtfs_files_recursive(&approved_dir, &mut rtfs_files);

    if verbose {
        eprintln!("[ccos-mcp] Found {} .rtfs files to load", rtfs_files.len());
    }

    let parser = match MCPDiscoveryProvider::new(MCPServerConfig::default()) {
        Ok(p) => p,
        Err(e) => {
            eprintln!("[ccos-mcp] Error initializing RTFS parser: {}", e);
            return;
        }
    };

    let mut loaded_count = 0;
    for file in rtfs_files {
        if let Some(path_str) = file.to_str() {
            match parser.load_rtfs_capabilities(path_str) {
                Ok(module) => {
                    for cap_def in module.capabilities {
                        match parser.rtfs_to_capability_manifest(&cap_def) {
                            Ok(manifest) => {
                                if let Err(e) =
                                    marketplace.register_capability_manifest(manifest).await
                                {
                                    if verbose {
                                        eprintln!(
                                            "[ccos-mcp] Error registering capability from {:?}: {}",
                                            file, e
                                        );
                                    }
                                } else {
                                    loaded_count += 1;
                                }
                            }
                            Err(e) => {
                                if verbose {
                                    eprintln!(
                                        "[ccos-mcp] Error converting RTFS to manifest in {:?}: {}",
                                        file, e
                                    );
                                }
                            }
                        }
                    }
                }
                Err(e) => {
                    if verbose {
                        eprintln!("[ccos-mcp] Error loading RTFS file {:?}: {}", file, e);
                    }
                }
            }
        }
    }

    if verbose {
        eprintln!(
            "[ccos-mcp] Successfully loaded {} capabilities into marketplace",
            loaded_count
        );
    }
}
