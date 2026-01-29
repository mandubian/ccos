//! CCOS MCP Server
//!
//! Exposes CCOS capabilities as MCP tools.
//! Supports two transports:
//!   - HTTP (default): Streamable HTTP for persistent long-running daemon
#![allow(unused_imports, unused_variables, dead_code)]
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
use std::sync::{Arc, Mutex as StdMutex};

use async_trait::async_trait;
use clap::{Parser, ValueEnum};
use serde_json::json;
use tokio::sync::RwLock;

use ccos::approval::{
    queue::{RiskAssessment, RiskLevel},
    UnifiedApprovalQueue,
};
use ccos::arbiter::llm_provider::LlmProvider;

use ccos::config::types::{AgentConfig, ValidationConfig};
use ccos::types::Plan;

// ModularPlanner imports for LLM-based decomposition
// (No currently used imports)

use ccos::mcp::core::MCPDiscoveryService;
use ccos::capability_marketplace::CapabilityMarketplace;
use ccos::catalog::CatalogFilter;
use ccos::causal_chain::CausalChain;
use ccos::discovery::llm_discovery::LlmDiscoveryService;
use ccos::mcp::http_transport::{run_http_transport_with_approvals, HttpTransportConfig};
use ccos::mcp::server::MCPServer;
use ccos::secrets::SecretStore;
use ccos::synthesis::core::schema_serializer::type_expr_to_rtfs_pretty;
use ccos::synthesis::dialogue::capability_synthesizer::{CapabilitySynthesizer, SynthesisRequest};
use ccos::synthesis::mcp_introspector::MCPIntrospector;
use ccos::ops::server_discovery_pipeline::{PipelineTarget, ServerDiscoveryPipeline};
use futures::future::BoxFuture;
use ccos::types::StorableIntent;
use ccos::utils::fs::get_workspace_root;
use ccos::utils::value_conversion::{json_to_rtfs_value, rtfs_value_to_json};
use ccos::mcp::session::{
    create_session_store, save_session, Session, SessionStore,
};
use ccos::CCOS;
use rtfs::runtime::error::{RuntimeError, RuntimeResult};
use rtfs::runtime::security::RuntimeContext;
use ccos::working_memory::{
    AgentMemory, InMemoryJsonlBackend, LearnedPattern, WorkingMemory,
};

// ============================================================================
// RTFS Teaching Tools Constants
// ============================================================================

const GRAMMAR_OVERVIEW: &str = r#"RTFS Grammar Overview

RTFS uses homoiconic s-expression syntax where code and data share the same representation.

Core Syntax:
- Lists: (func arg1 arg2) - code and function calls
- Vectors: [1 2 3] - ordered sequences  
- Maps: {:key "value"} - key-value associations
- Comments: ;; single line or #| block |#

Basic Pattern:
(operator operand1 operand2 ...)

;; --- Data Transformation ---
(call "ccos.json.parse" "{\"a\": 1}")      ; => {:a 1} (keys become keywords)
(call "ccos.json.stringify" {:a 1})        ; => "{\"a\":1}"

;; --- System Capabilities ---
(call "ccos.system.get-env" "PATH")        ; => "/usr/bin:..."
(call "ccos.system.current-time")          ; => 1678900000

;; --- I/O Capabilities ---
(call "ccos.io.log" "message")             ; => nil
(call "ccos.io.read-file" "/tmp/foo")      ; => "content"
(call "ccos.io.write-file" "/tmp/foo" "bar"); => nil

;; --- Network Capabilities ---
(call "ccos.network.http-fetch" "https://api.example.com")

;; --- State Capabilities ---
(call "ccos.state.kv.put" :key "value")    ; => true
(call "ccos.state.kv.get" :key)            ; => "value"

;; Example
(let [x 1]
  (if (> x 0)
    (call :io/println "positive")
    "negative"))
"#;

const GRAMMAR_LITERALS: &str = r#"RTFS Literals

;; --- Primitives ---
42                    ; integer
3.14                  ; float
"hello world"         ; string (UTF-8, escapes: \n \t \")
true                  ; boolean true
false                 ; boolean false
nil                   ; null/empty value

;; --- Extended Types ---
:keyword              ; keyword (self-evaluating, starts with :)
:my.ns/qualified      ; namespaced keyword
:com.acme:v1.0/api    ; versioned keyword
2026-01-06T10:00:00Z  ; ISO 8601 timestamp
resource://handle     ; resource handle
"#;

const GRAMMAR_COLLECTIONS: &str = r#"RTFS Collections

;; --- Lists (Code) ---
(+ 1 2 3)                  ; function call => 6
(if true "yes" "no")       ; special form
(first (1 2 3))            ; => 1
(cons 0 (1 2))             ; => (0 1 2)

;; --- Vectors (Data) ---
[1 2 3 4]                  ; literal vector
(vector 1 2 3)             ; construct vector
(get [10 20 30] 1)         ; => 20 (indexed access)
(conj [1 2] 3)             ; => [1 2 3]

;; --- Maps ---
{:name "Alice" :age 30}    ; map literal
(get {:a 1 :b 2} :a)       ; => 1
(assoc {:a 1} :b 2)        ; => {:a 1 :b 2}
(keys {:a 1 :b 2})         ; => (:a :b)
"#;

const GRAMMAR_SPECIAL_FORMS: &str = r#"RTFS Special Forms

;; --- Variable Binding ---
(def pi 3.14159)           ; global definition

(let [x 1 y (+ x 2)]       ; lexical scoping
  (* x y))                 ; => 3

;; defn in let body creates closures
(let [factor 10]
  (defn scale [x] (* x factor))  ; captures 'factor'
  (scale 5))                     ; => 50

;; --- Destructuring ---
(let [[a b] [1 2]] (+ a b))                    ; vector => 3
(let [{:keys [name age]} {:name "Al" :age 30}] 
  name)                                         ; map => "Al"

;; --- Control Flow ---
(if (> x 0) "positive" "non-positive")

(do                        ; sequencing
  (call :io/println "step1")
  (call :io/println "step2")
  42)                      ; returns last expr

(match value               ; pattern matching
  0 "zero"
  [x y] (str "pair: " x y)
  {:name n} (str "hi " n)
  _ "other")

;; --- Functions ---
(fn [x] (* x x))           ; anonymous function
(defn add [x y] (+ x y))   ; named function
(defn sum [& args]         ; variadic (rest args)
  (reduce + 0 args))

;; --- Host Calls ---
(call :ccos.http/get {:url "..."})
(call :ccos.state.kv/set "key" value)
"#;

const GRAMMAR_TYPES: &str = r#"RTFS Type Expressions

;; --- Primitive Types ---
:int :float :string :bool :nil :any

;; --- Collection Types ---
[:vector :int]                          ; vector of ints
[:tuple :string :int]                   ; fixed tuple
[:map [:name :string] [:age :int]]      ; map schema

;; --- Function Types ---
[:fn [:int :int] :int]                  ; (int, int) -> int
[:fn [:string & :any] :string]          ; variadic
[:and :int [:> 0]]                      ; positive int
[:and :int [:>= 0] [:< 100]]            ; int in [0, 100)
[:and :string [:min-length 1] [:max-length 255]]
[:and :string [:matches-regex "^[a-z]+$"]]

;; --- Union & Optional ---
[:union :int :string :nil]              ; one of these types
:string?                                ; sugar for [:union :string :nil]

;; --- Refined Types (constraints) ---
[:and :int [:> 0]]                      ; positive int
[:and :int [:>= 0] [:< 100]]            ; int in [0, 100)
[:and :string [:min-length 1] [:max-length 255]]
[:and :string [:matches-regex "^[a-z]+$"]]

;; --- Schema Example ---
[:map
  [:id :string]
  [:name :string]
  [:age [:and :int [:>= 0]]]
  [:email {:optional true} :string]]
"#;

const GRAMMAR_PURITY_EFFECTS: &str = r#"RTFS Purity and Effect Boundaries

CRITICAL: RTFS is a PURE language. ALL side effects are delegated to the host.

;; --- The Golden Rule ---
;; Pure code: arithmetic, logic, data transformation - runs in RTFS
;; Effectful code: I/O, network, state - MUST use (call ...) to delegate to host

;; --- Pure (no call needed) ---
(+ 1 2 3)                          ; arithmetic - pure
(map inc [1 2 3])                  ; data transformation - pure
(filter even? [1 2 3 4])           ; filtering - pure
(let [x 10] (* x x))               ; binding and computation - pure

;; --- Effectful (REQUIRES call) ---
(call :ccos.io/println "Hello")    ; I/O - effect via host
(call :ccos.http/get {:url "..."}) ; network - effect via host
(call :ccos.fs/read-file "/path")  ; file system - effect via host
(call :ccos.state.kv/get "key")    ; state access - effect via host

;; --- Why Purity Matters ---
;; 1. Deterministic: same inputs always give same outputs
;; 2. Testable: pure functions can be tested in isolation
;; 3. Safe: LLM-generated code can't accidentally cause side effects
;; 4. Auditable: all effects go through CCOS governance pipeline

;; --- Capabilities Declare Effects ---
(capability "my-tool/fetch-data"
  :effects [:io :network]          ; <-- declares what effects are used
  :implementation
  (fn [input]
    (let [url (str "https://api.example.com/" (:id input))]
      (call :ccos.http/get {:url url}))))  ; effect delegated to host

;; --- Effect Categories ---
;; :io       - console, logging, printing
;; :network  - HTTP requests, API calls
;; :fs       - file system read/write
;; :state    - key-value store, database
;; :time     - current time, timestamps
;; :random   - random number generation

;; --- Execution Model ---
;; 1. RTFS evaluates pure code
;; 2. When (call ...) encountered, YIELDS to host
;; 3. Host performs effect, returns result
;; 4. RTFS continues with pure code
"#;

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
// Moved to ccos::mcp::session

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
    if let Some(ref _config) = agent_config {
        // Set workspace root based on config file location
        // The config is at <workspace>/config/agent_config.toml, so workspace is parent of parent
        // Must canonicalize first since args.config may be relative (e.g., "config/agent_config.toml")
        let config_path = std::path::PathBuf::from(&args.config);
        let abs_config_path = if config_path.is_absolute() {
            config_path
        } else {
            std::env::current_dir()
                .map(|cwd| cwd.join(&config_path))
                .unwrap_or(config_path)
        };

        if let Some(config_parent) = abs_config_path.parent() {
            if let Some(workspace_root) = config_parent.parent() {
                if args.verbose {
                    eprintln!(
                        "[ccos-mcp] Setting workspace root to: {}",
                        workspace_root.display()
                    );
                }
                ccos::utils::fs::set_workspace_root(workspace_root.to_path_buf());
            } else {
                // Config is at top level, use parent
                ccos::utils::fs::set_workspace_root(config_parent.to_path_buf());
            }
        }
    }

    // Initialize CCOS components
    let ccos = CCOS::new_with_agent_config_and_configs_and_debug_callback(
        ccos::intent_graph::config::IntentGraphConfig::default(),
        None,
        agent_config.clone(),
        None,
    )
    .await
    .map_err(|e| format!("Failed to initialize CCOS: {}", e))?;
    let ccos = Arc::new(ccos);

    // Extract components for compatibility
    let marketplace = ccos.get_capability_marketplace();
    let _causal_chain = Arc::new(StdMutex::new(CausalChain::new()?)); // Warning: CCOS creates its own chain, but here we are creating a NEW one because CCOS chain is Mutex (Async/Sync mismatch) or we want to share?
                                                                      // Wait, CCOS has pub causal_chain: Arc<Mutex<CausalChain>>.
                                                                      // ccos-mcp uses Arc<StdMutex<CausalChain>>.
                                                                      // If CCOS uses std::sync::Mutex (checked in types.rs), then types match.
                                                                      // So I should use:
    let causal_chain = ccos.causal_chain.clone();

    // Populate marketplace with approved capabilities
    let workspace_root = ccos::utils::fs::get_workspace_root();
    let _ = marketplace
        .import_capabilities_from_rtfs_dir_recursive(
            workspace_root.join("capabilities/servers/approved"),
        )
        .await;
    let _ = marketplace
        .import_capabilities_from_rtfs_dir_recursive(workspace_root.join("capabilities/core"))
        .await;
    let _ = marketplace
        .import_capabilities_from_rtfs_dir_recursive(workspace_root.join("capabilities/learned"))
        .await;
    let _ = marketplace
        .import_capabilities_from_rtfs_dir_recursive(workspace_root.join("capabilities/agents"))
        .await;

    // Register Planner v2 capabilities in the MCP server marketplace.
    //
    // `ccos_consolidate_session` depends on `planner.synthesize_agent_from_trace`, which is a
    // Planner v2 capability. Without this, MCP consolidation fails with:
    // "Unknown capability: planner.synthesize_agent_from_trace".
    //
    // We keep this best-effort (warn, don't abort) so MCP can still run in reduced mode.
    if let Err(e) = ccos::planner::capabilities_v2::register_planner_capabilities_v2(
        Arc::clone(&marketplace),
        ccos.get_catalog(),
        Arc::clone(&ccos),
    )
    .await
    {
        eprintln!(
            "[ccos-mcp] Warning: failed to register planner v2 capabilities: {}",
            e
        );
    }

    // Create discovery services
    let llm_discovery = if let Some(config) = &agent_config {
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
        let storage_path = agent_config
            .as_ref()
            .map(|cfg| ccos::utils::fs::resolve_workspace_path(&cfg.storage.approvals_dir))
            .unwrap_or_else(|| std::path::PathBuf::from(".ccos/approvals"));

        if args.verbose {
            eprintln!("[ccos-mcp] Using approval storage at: {}", storage_path.display());
        }

        let storage = ccos::approval::storage_file::FileApprovalStorage::new(storage_path)
            .expect("Failed to create approval storage");
        let queue = UnifiedApprovalQueue::new(std::sync::Arc::new(storage));

        // Spawn filesystem sync as a background task to recover orphaned server directories
        // This runs asynchronously to avoid blocking startup
        let sync_queue = queue.clone();
        let verbose = args.verbose;
        tokio::spawn(async move {
            if verbose {
                eprintln!("[ccos-mcp] Running filesystem sync to recover orphaned server directories...");
            }
            match sync_queue.sync_with_filesystem().await {
                Ok(report) => {
                    if !report.created_pending.is_empty() || !report.created_approved.is_empty() {
                        eprintln!(
                            "[ccos-mcp] Filesystem sync: recovered {} pending, {} approved servers",
                            report.created_pending.len(),
                            report.created_approved.len()
                        );
                    }
                    for warning in &report.warnings {
                        eprintln!("[ccos-mcp] Sync warning: {}", warning);
                    }
                }
                Err(e) => {
                    eprintln!("[ccos-mcp] Warning: filesystem sync failed: {}", e);
                }
            }
        });

        queue
    };

    // Create MCP server with tool handlers
    let mut server = MCPServer::new("ccos", env!("CARGO_PKG_VERSION"));

    // Register core CCOS tools
    let session_store = create_session_store();

    // Initialize Agent Memory for Tangible Learning
    let wm_backend = InMemoryJsonlBackend::new(None, None, None);
    let working_memory = Arc::new(StdMutex::new(WorkingMemory::new(Box::new(wm_backend))));
    let agent_memory = Arc::new(RwLock::new(AgentMemory::new("system-mcp", working_memory)));

    // Try to load persisted memory
    // We use a specific path in the workspace dot-dir
    let memory_path = get_workspace_root().join(".ccos/agent_memory/system_memory.json");
    if memory_path.exists() {
        // We need an async block to await the lock
        let am = agent_memory.clone();
        let path = memory_path.clone();
        tokio::spawn(async move {
            let mut mem = am.write().await;
            if let Err(e) = mem.load_from_disk(&path) {
                eprintln!("[ccos-mcp] Failed to load agent memory: {}", e);
            } else {
                 eprintln!("[ccos-mcp] Loaded agent memory from {}", path.display());
            }
        });
    }

    register_ccos_tools(
        &mut server,
        ccos.clone(),
        marketplace.clone(),
        causal_chain,
        llm_discovery,
        session_store,
        approval_queue.clone(),
        agent_memory.clone(), // Pass AgentMemory
        agent_config.clone(),
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
            run_http_transport_with_approvals(
                server,
                config,
                Some(approval_queue),
                Some(marketplace.clone()),
            )
            .await?;
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

// ============================================================================
// Helper functionality
// ============================================================================

/// Helper to register a tool with both the MCP Server (for direct client access)
/// and the CapabilityMarketplace (for agent discovery via ccos_search_capabilities and internal execution)
fn register_ecosystem_tool(
    server: &mut MCPServer,
    marketplace: Arc<CapabilityMarketplace>,
    name: &str,
    description: &str,
    input_schema: serde_json::Value,
    handler: Arc<dyn Fn(serde_json::Value) -> BoxFuture<'static, Result<serde_json::Value, String>> + Send + Sync>,
) {
    // 1. Register with MCP Server
    let server_handler = handler.clone();
    server.register_tool(
        name,
        description,
        input_schema.clone(),
        Box::new(move |p| {
            let h = server_handler.clone();
            Box::pin(async move {
                h(p).await.map_err(|e| RuntimeError::Generic(e))
            })
        }),
    );

    // 2. Register with CapabilityMarketplace (async)
    // We spawn a task because the marketplace registration is async but this function is sync
    let name = name.to_string();
    let description = description.to_string();
    let mp_handler = handler.clone();
    
    // Wrap the handler for the Marketplace's different signature types
    // Wrap the handler for the Marketplace's different signature types
    let native_handler = Arc::new(move |p: &rtfs::runtime::Value| -> BoxFuture<'static, RuntimeResult<rtfs::runtime::Value>> {
        let p_json_res = serde_json::to_value(p);
        let h = mp_handler.clone();
        Box::pin(async move {
            let p_json = p_json_res.map_err(|e| RuntimeError::Generic(e.to_string()))?;
            match h(p_json).await {
                Ok(v) => serde_json::from_value(v).map_err(|e| RuntimeError::Generic(e.to_string())),
                Err(e) => Err(RuntimeError::Generic(e)),
            }
        })
    });

    tokio::spawn(async move {
        // Convert JSON schema to RTFS TypeExpr
        let input_type_expr = MCPIntrospector::type_expr_from_json_schema(&input_schema).ok();
        
        if let Err(e) = marketplace.register_native_capability_with_schema(
            name.clone(),
            name.clone(),
            description,
            native_handler,
            "system".to_string(),
            input_type_expr,
            None // Output schema unknown/any
        ).await {
            eprintln!("[CCOS] Failed to register ecosystem tool '{}' in marketplace: {}", name, e);
        } else {
            // eprintln!("[CCOS] Registered ecosystem tool '{}' in marketplace", name);
        }
    });
}

fn register_ccos_tools(
    server: &mut MCPServer,
    ccos: Arc<CCOS>,
    marketplace: Arc<CapabilityMarketplace>,
    causal_chain: Arc<StdMutex<CausalChain>>,
    discovery_service: Arc<LlmDiscoveryService>,
    session_store: SessionStore,
    approval_queue: UnifiedApprovalQueue<ccos::approval::storage_file::FileApprovalStorage>,
    agent_memory: Arc<RwLock<AgentMemory>>,
    agent_config: Option<AgentConfig>,
) {
    // Hide low-level/internal capabilities from MCP discovery surfaces.
    // They may still be callable by specific higher-level tools (e.g. consolidation),
    // but should not appear in ccos_list_capabilities / ccos_search_capabilities.
    let is_hidden_capability_id = |id: &str| -> bool { id.starts_with("planner.") };

    // ccos_search_capabilities
    let mp = marketplace.clone();
    let handler = Arc::new(move |params: serde_json::Value| -> BoxFuture<'static, Result<serde_json::Value, String>> {
        let mp = mp.clone();
        Box::pin(async move {
            let query = params.get("query").and_then(|v| v.as_str()).unwrap_or("");
            let domains = params.get("domains").and_then(|v| v.as_array());
            let limit = params.get("limit").and_then(|v| v.as_u64()).unwrap_or(10) as usize;
            let min_score = params.get("min_score").and_then(|v| v.as_f64()).unwrap_or(0.0);

            if query.is_empty() {
                return Ok(json!({
                    "error": "query is required",
                    "results": []
                }));
            }

            // Use centralized catalog search
            let catalog_opt = mp.get_catalog().await;
            let catalog = match catalog_opt.as_ref() {
                Some(c) => c,
                None => return Ok(json!({
                    "error": "Catalog service unavailable",
                    "results": []
                })),
            };
            
            // Use keyword search which now includes improved matching logic
            let hits = catalog.search_keyword(query, None, limit).await;
            
            #[derive(serde::Serialize)]
            struct CapabilityInfo {
                id: String,
                name: String,
                description: String,
                usage: String,
                input_schema: Option<serde_json::Value>,
                domains: Vec<String>,
            }

            let mut scored_results: Vec<_> = hits
                .into_iter()
                .map(|h| {
                    // Convert CatalogEntry to CapabilityInfo
                    let id = h.entry.id.clone();
                    let cap_info = CapabilityInfo {
                        id: id.clone(),
                        name: h.entry.name.clone().unwrap_or(id),
                        description: h.entry.description.unwrap_or_default(),
                        usage: format!("(call \"{}\" ...)", h.entry.search_blob.split_whitespace().next().unwrap_or("")), // Simplified usage
                        input_schema: h.entry.input_schema.and_then(|s| serde_json::from_str(&s).ok()),
                        domains: h.entry.domains.clone(),
                    };
                    (cap_info, h.score)
                })
                .filter(|(_, score)| (*score as f64) >= min_score) // min_score is f64, score is f64 (from CatalogHit)
                .collect();

            // Apply domain filter if provided
            if let Some(domains) = domains {
                let domain_strs: Vec<&str> = domains.iter().filter_map(|v| v.as_str()).collect();
                if !domain_strs.is_empty() {
                    scored_results = scored_results
                        .into_iter()
                        .filter(|(c, _)| {
                            domain_strs.iter().any(|&d| {
                                c.id.contains(d) || c.domains.iter().any(|cd| cd.contains(d))
                            })
                        })
                        .collect();
                }
            }

            // Sort by score descending
            scored_results.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));

            // Filter out low-level/internal capabilities from discovery surfaces
            scored_results.retain(|(c, _)| !is_hidden_capability_id(&c.id));

            let total = scored_results.len();
            let limited: Vec<_> = scored_results.into_iter().take(limit).collect();

            Ok(json!({
                "results": limited.iter().map(|(c, score)| json!({
                    "id": c.id,
                    "name": c.name,
                    "description": c.description,
                    "match_score": score,
                    "input_schema": c.input_schema
                })).collect::<Vec<_>>(),
                "total": total,
                "hint": if total > 0 { "Use ccos_execute_capability to run these" } else { "Try a different query or check available capabilities with ccos_list_capabilities" }
            }))
        })
    });


    // ccos_search_servers
    let aq_search = approval_queue.clone();
    let handler_servers = Arc::new(move |params: serde_json::Value| -> BoxFuture<'static, Result<serde_json::Value, String>> {
        let aq = aq_search.clone();
        Box::pin(async move {
            let query = params.get("query").and_then(|v| v.as_str()).unwrap_or("").to_lowercase();
            let limit = params.get("limit").and_then(|v| v.as_u64()).unwrap_or(20) as usize;

            // List all approvals
            let all_approvals = aq.list(ccos::approval::types::ApprovalFilter::default()).await
                .map_err(|e| format!("Failed to list approvals: {}", e))?;

            #[derive(serde::Serialize)]
            struct ServerSummary {
                name: String,
                description: String,
                status: String,
                tags: Vec<String>,
                capability_count: usize,
                source: String,
            }

            let mut results: Vec<ServerSummary> = all_approvals
                .into_iter()
                .filter_map(|req| {
                    // Only consider ServerDiscovery requests
                    if let ccos::approval::types::ApprovalCategory::ServerDiscovery { 
                        server_info, 
                        domain_match,
                        source,
                        capability_files,
                        .. 
                    } = req.category {
                        // Filter by status (only Approved or Pending)
                        let status_str = if req.status.is_approved() {
                            "approved"
                        } else if req.status.is_pending() {
                            "pending"
                        } else {
                            return None;
                        };

                        // Check matches
                        let name_match = server_info.name.to_lowercase().contains(&query);
                        let desc_match = server_info.description.as_ref()
                            .map(|d| d.to_lowercase().contains(&query))
                            .unwrap_or(false);
                        let tag_match = domain_match.iter().any(|d| d.to_lowercase().contains(&query));

                        if query.is_empty() || name_match || desc_match || tag_match {
                            Some(ServerSummary {
                                name: server_info.name,
                                description: server_info.description.unwrap_or_default(),
                                status: status_str.to_string(),
                                tags: domain_match,
                                capability_count: capability_files.map(|f| f.len()).unwrap_or(0),
                                source: source.name(),
                            })
                        } else {
                            None
                        }
                    } else {
                        None
                    }
                })
                .take(limit)
                .collect();

            // Sort by status (approved first) then name
            results.sort_by(|a, b| {
                match (a.status.as_str(), b.status.as_str()) {
                    ("approved", "pending") => std::cmp::Ordering::Less,
                    ("pending", "approved") => std::cmp::Ordering::Greater,
                    _ => a.name.cmp(&b.name),
                }
            });

            Ok(json!({
                "servers": results,
                "count": results.len(),
                "hint": "Use ccos_list_capabilities to see details or ccos_search_capabilities to find specific tools."
            }))
        })
    });

    register_ecosystem_tool(
        server,
        marketplace.clone(),
        "ccos_search_capabilities",
        "Search for capabilities by query, ID pattern, or domain. Use this to find capabilities before executing them.",
        json!({
            "type": "object",
            "properties": {
                "query": { 
                    "type": "string", 
                    "description": "Search query - matches against capability ID, name, and description" 
                },
                "domains": {
                    "type": "array",
                    "items": { "type": "string" },
                    "description": "Optional domain filters (e.g., ['github', 'weather'])"
                },
                "limit": {
                    "type": "integer",
                    "default": 10,
                    "description": "Maximum results to return"
                },
                "min_score": {
                    "type": "number",
                    "default": 0.0,
                    "description": "Minimum relevance score (0-1) to include in results"
                }
            },
            "required": ["query"]
        }),
        handler,
    );

    register_ecosystem_tool(
        server,
        marketplace.clone(),
        "ccos_search_servers",
        "Search for available server integrations. Use this to find servers to introspect or use.",
        json!({
            "type": "object",
            "properties": {
                "query": {
                    "type": "string",
                    "description": "Optional search query to filter by name, description, or tags"
                },
                "limit": {
                    "type": "integer",
                    "default": 20,
                    "description": "Maximum number of results (default: 20)"
                }
            }
        }),
        handler_servers,
    );

    // ccos_suggest_apis - Pure LLM suggestions for external APIs (no auto-approval)
    // Returns suggestions for user selection, then user can approve and introspect
    let ds_suggest = discovery_service.clone();
    server.register_tool(
        "ccos_suggest_apis",
        "Ask LLM to suggest well-known APIs for a given goal. Returns suggestions without auto-queueing approvals. Logic: (1) If 1 suggestion, use it. (2) If several and you are confident, choose one + explain why to user. (3) If several and you have doubt, ask user to choose.",
        json!({
            "type": "object",
            "properties": {
                "query": {
                    "type": "string",
                    "description": "What you want to accomplish (e.g., 'send SMS notifications', 'get weather data')"
                }
            },
            "required": ["query"]
        }),
        Box::new(move |params| {
            let ds = ds_suggest.clone();
            Box::pin(async move {
                let query = params.get("query").and_then(|v| v.as_str()).unwrap_or("");

                if query.is_empty() {
                    return Ok(json!({
                        "success": false,
                        "error": "query is required"
                    }));
                }

                eprintln!("[CCOS] Suggesting APIs for: '{}'...", query);

                // Use LLM to suggest APIs (no URL hint = pure LLM suggestions)
                let suggestions = match ds.search_external_apis(query, None).await {
                    Ok(results) => results,
                    Err(e) => {
                        return Ok(json!({
                            "success": false,
                            "suggestions": [],
                            "error": format!("LLM suggestion failed: {}", e)
                        }));
                    }
                };

                if suggestions.is_empty() {
                    return Ok(json!({
                        "success": true,
                        "suggestions": [],
                        "note": "No APIs suggested for this query. Try a more specific description."
                    }));
                }

                // Convert to simple JSON format for user selection
                let api_list: Vec<_> = suggestions.iter().take(5).map(|api| {
                    json!({
                        "name": api.name,
                        "endpoint": api.endpoint,
                        "docs_url": api.docs_url,
                        "description": api.description,
                        "auth_env_var": api.auth_env_var
                    })
                }).collect();

                Ok(json!({
                    "success": true,
                    "suggestions": api_list,
                    "next_steps": [
                        "If exactly one suggestion, you may use it directly.",
                        "If several suggestions and you are confident, choose one and explain why to the user.",
                        "If several suggestions and you have doubt, present all and ask the user to choose one.",
                        "When an API is selected, call ccos_introspect_remote_api with its docs_url (NOT the endpoint)."
                    ]
                }))
            })
        }),
    );

    /* DISABLED: ccos_find_capability - replaced by ccos_suggest_apis for interactive flow
    // ccos_find_capability - Unified capability search (local + optional external discovery)
    // This is the primary tool agents should use to find capabilities
    let mp_find = marketplace.clone();
    let ds_find = discovery_service.clone();
    let aq_find = approval_queue.clone();
    server.register_tool(
        "ccos_find_capability",
        "PRIMARY: Find capabilities matching a query. Searches local capabilities first. If allow_external is true and no local match found, discovers from external sources (MCP, OpenAPI, web docs).",
        json!({
            "type": "object",
            "properties": {
                "query": {
                    "type": "string",
                    "description": "What capability you're looking for (e.g., 'get weather forecast', 'send email')"
                },
                "allow_external": {
                    "type": "boolean",
                    "default": false,
                    "description": "If true and no local match, search external sources (slower, requires approval)"
                },
                "url_hint": {
                    "type": "string",
                    "description": "Optional URL hint for external discovery (e.g., 'https://openweathermap.org/api')"
                },
                "min_score": {
                    "type": "number",
                    "default": 0.3,
                    "description": "Minimum relevance score (0-1) for local matches"
                }
            },
            "required": ["query"]
        }),
        Box::new(move |params| {
            let mp = mp_find.clone();
            let ds = ds_find.clone();
            let aq = aq_find.clone();
            Box::pin(async move {
                let query = params.get("query").and_then(|v| v.as_str()).unwrap_or("");
                let allow_external = params.get("allow_external").and_then(|v| v.as_bool()).unwrap_or(false);
                let url_hint = params.get("url_hint").and_then(|v| v.as_str()).map(|s| s.to_string());
                let min_score = params.get("min_score").and_then(|v| v.as_f64()).unwrap_or(0.3);

                if query.is_empty() {
                    return Ok(json!({
                        "success": false,
                        "error": "query is required"
                    }));
                }

                // Step 1: Search using CatalogService
                let catalog_opt = mp.get_catalog().await;
                let catalog = match catalog_opt.as_ref() {
                    Some(c) => c,
                    None => return Ok(json!({
                        "success": false,
                        "error": "Catalog service unavailable"
                    })),
                };

                let hits = catalog.search_keyword(query, None, 50).await;

                // Convert to results format expected by caller
                let local_results: Vec<_> = hits
                    .iter()
                    .filter(|h| h.score >= min_score)
                    .take(5)
                    .map(|h| {
                         json!({
                            "id": h.entry.id,
                            "name": h.entry.name.clone().unwrap_or_else(|| h.entry.id.clone()),
                            "description": h.entry.description.clone().unwrap_or_default(),
                            "match_score": h.score,
                            "source": "catalog"
                        })
                    })
                    .collect();
                if !local_results.is_empty() {
                    return Ok(json!({
                        "success": true,
                        "capabilities": local_results,
                        "source": "local",
                        "hint": "Use ccos_execute_capability to run these"
                    }));
                }

                // Step 2: If no local results and external allowed, try external discovery
                if !allow_external {
                    return Ok(json!({
                        "success": true,
                        "capabilities": [],
                        "source": "local",
                        "note": "No local capabilities found. Set allow_external: true to search external sources (slower, requires approval)."
                    }));
                }

                // External discovery - use LLM to search for APIs
                eprintln!("[CCOS] No local match for '{}', attempting external discovery...", query);

                // Try to discover external APIs
                let discovery_result = match ds.search_external_apis(query, url_hint.as_deref()).await {
                    Ok(results) => results,
                    Err(e) => {
                        return Ok(json!({
                            "success": false,
                            "capabilities": [],
                            "source": "external_search_failed",
                            "error": format!("External discovery failed: {}", e),
                            "hint": "Try providing a url_hint to a known API documentation page"
                        }));
                    }
                };

                if discovery_result.is_empty() {
                    return Ok(json!({
                        "success": true,
                        "capabilities": [],
                        "source": "external",
                        "note": "No external APIs found matching the query. Try a more specific query or provide a url_hint."
                    }));
                }

                // Create approval requests for discovered APIs
                let mut pending_approvals = Vec::new();
                for api in discovery_result.iter().take(3) {
                    // Create approval request
                    use ccos::approval::queue::{DiscoverySource, ServerInfo};

                    let server_info = ServerInfo {
                        name: api.name.clone(),
                        endpoint: api.endpoint.clone(),
                        description: Some(api.description.clone()),
                        auth_env_var: api.auth_env_var.clone(),
                        capabilities_path: None,
                        alternative_endpoints: vec![],
                    };

                    let category = ApprovalCategory::ServerDiscovery {
                        source: DiscoverySource::WebSearch {
                            url: format!("llm-discovery:{}", query),
                        },
                        server_info: server_info.clone(),
                        server_id: Some(ccos::utils::fs::sanitize_filename(&api.name)),
                        version: Some(1),
                        domain_match: vec![query.to_string()],
                        requesting_goal: Some(query.to_string()),
                        health: None,
                        capability_files: None,
                    };

                    let approval_request = ccos::approval::types::ApprovalRequest::new(
                        category,
                        RiskAssessment {
                            level: RiskLevel::Medium,
                            reasons: vec!["External API discovery requires approval".to_string()],
                        },
                        24, // 24 hour expiry
                        Some(format!("Discovered via query: {}", query)),
                    );

                    let approval_id = approval_request.id.clone();
                    if let Err(e) = aq.add(approval_request).await {
                        eprintln!("[CCOS] Failed to submit approval: {}", e);
                    } else {
                        pending_approvals.push(json!({
                            "approval_id": approval_id,
                            "api_name": api.name,
                            "endpoint": api.endpoint,
                            "description": api.description
                        }));
                    }
                }

                Ok(json!({
                    "success": true,
                    "capabilities": [],
                    "source": "external_discovery",
                    "pending_approvals": pending_approvals,
                    "note": "External APIs discovered and queued for approval. Once approved, they will be available as local capabilities.",
                    "next_step": "Use ccos_list_capabilities to check for newly approved capabilities"
                }))
            })
        }),
    );
    END DISABLED */

    // ccos/log_thought
    // Logs thought to AgentMemory and persists it.
    let am_log = agent_memory.clone();
    // ccos/log_thought
    // Logs thought to AgentMemory and persists it.
    let am_log = agent_memory.clone();
    let handler = Arc::new(move |params: serde_json::Value| -> BoxFuture<'static, Result<serde_json::Value, String>> {
        let am = am_log.clone();
        Box::pin(async move {
            let thought = params.get("thought").and_then(|v| v.as_str()).unwrap_or("");
            let plan_id = params.get("plan_id").and_then(|v| v.as_str()).unwrap_or("agent-thought");
            let is_failure = params.get("is_failure").and_then(|v| v.as_bool()).unwrap_or(false);

            // Log to stderr for observability
            eprintln!("[ccos/log_thought] plan={} thought={}", plan_id, thought);

            // Store in memory
            {
                let memory = am.read().await;
                
                let mut tags = vec!["thought".to_string(), plan_id.to_string()];
                if is_failure {
                    tags.push("learning".to_string());
                    tags.push("failure".to_string());
                }

                memory.store(
                    format!("Thought: {}", plan_id),
                    thought,
                    &tags.iter().map(|s| s.as_str()).collect::<Vec<_>>(),
                ).map_err(|e| e.to_string())?;

                // Save to disk
                let path = get_workspace_root().join(".ccos/agent_memory/system_memory.json");
                 memory.save_to_disk(&path).map_err(|e| e.to_string())?;
            }

            Ok(json!({
                "success": true,
                "logged": true,
                "persisted": true
            }))
        })
    });
    register_ecosystem_tool(
        server,
        marketplace.clone(),
        "ccos_log_thought",
        "Record the agent's reasoning process (and occasional failures) into Agent Memory",
        json!({
            "type": "object",
            "properties": {
                "plan_id": { "type": "string", "description": "Optional plan ID to associate with" },
                "thought": { "type": "string", "description": "The agent's reasoning or thought" },
                "context": { "type": "object", "description": "Context metadata" },
                "is_failure": { "type": "boolean", "description": "If true, records this as a failure pattern" }
            },
            "required": ["thought"]
        }),
        handler,
    );

    // ccos/recall_memories
    let am_recall = agent_memory.clone();
    // ccos/recall_memories
    let am_recall = agent_memory.clone();
    let handler = Arc::new(move |params: serde_json::Value| -> BoxFuture<'static, Result<serde_json::Value, String>> {
        let am = am_recall.clone();
        Box::pin(async move {
            let tags_val = params.get("tags").and_then(|v| v.as_array());
            let limit = params.get("limit").and_then(|v| v.as_u64()).map(|v| v as usize);
            
            let tags: Vec<String> = if let Some(arr) = tags_val {
                arr.iter().filter_map(|v| v.as_str().map(|s| s.to_string())).collect()
            } else {
                vec![]
            };
            
            if tags.is_empty() {
                return Err("At least one tag is required for recall".to_string());
            }
            
            let tags_str: Vec<&str> = tags.iter().map(|s| s.as_str()).collect();

            let memory = am.read().await;
            let results = memory.recall_relevant(&tags_str, limit)
                .map_err(|e| e.to_string())?;
            
            // Convert WorkingMemoryEntry to JSON
            let result_json: Vec<serde_json::Value> = results.into_iter().map(|entry| {
                json!({
                    "id": entry.id.to_string(),
                    "content": entry.content,
                    "meta": entry.meta
                })
            }).collect();

            Ok(json!({
                "memories": result_json
            }))
        })
    });
    register_ecosystem_tool(
        server,
        marketplace.clone(),
        "ccos_recall_memories",
        "Recall relevant memories or patterns based on tags",
        json!({
            "type": "object",
            "properties": {
                "tags": { "type": "array", "items": { "type": "string" }, "description": "Filter by tags (e.g. 'thought', 'failure', 'learning', 'agent-thought')" },
                "limit": { "type": "integer", "description": "Max results" }
            },
            "required": ["tags"]
         }),
         handler,
    );

    // ccos/get_constitution
    let ccos_const = ccos.clone();
    // ccos/get_constitution
    let ccos_const = ccos.clone();
    let handler = Arc::new(move |_params: serde_json::Value| -> BoxFuture<'static, Result<serde_json::Value, String>> {
        let ccos = ccos_const.clone();
        Box::pin(async move {
            let rules = ccos.governance_kernel.get_rules();
            let rules_json = serde_json::to_value(rules)
                .map_err(|e| format!("Failed to serialize constitution: {}", e))?;
            
            Ok(json!({
                "constitution": rules_json
            }))
        })
    });
    register_ecosystem_tool(
        server,
        marketplace.clone(),
        "ccos_get_constitution",
        "Get the system constitution rules and policies",
        json!({
            "type": "object",
            "properties": {},
            "required": []
        }),
        handler,
    );

    // ccos/get_guidelines
    // ccos/get_guidelines
    let handler = Arc::new(move |_params: serde_json::Value| -> BoxFuture<'static, Result<serde_json::Value, String>> {
        Box::pin(async move {
            let docs_path = get_workspace_root().join("docs/agent_guidelines.md");
            
            if !docs_path.exists() {
                 return Err("Guidelines file not found at docs/agent_guidelines.md".to_string());
            }

            let content = tokio::fs::read_to_string(&docs_path).await
                .map_err(|e| format!("Failed to read guidelines: {}", e))?;
            
            Ok(json!({
                "guidelines": content
            }))
        })
    });
    register_ecosystem_tool(
        server,
        marketplace.clone(),
        "ccos_get_guidelines",
        "Get the official guidelines for AI agents operating within CCOS. Read this to understand how to interact effectively with the system.",
        json!({
            "type": "object",
            "properties": {},
            "required": []
        }),
        handler,
    );
    let am_learn = agent_memory.clone();
    let am_learn = agent_memory.clone();
    let handler = Arc::new(move |params: serde_json::Value| -> BoxFuture<'static, Result<serde_json::Value, String>> {
         let am = am_learn.clone();
         Box::pin(async move {
             let pattern = params.get("pattern").and_then(|v| v.as_str()).unwrap_or("");
             let context = params.get("context").and_then(|v| v.as_str()).unwrap_or("");
             let outcome = params.get("outcome").and_then(|v| v.as_str()).unwrap_or("");
             let confidence = params.get("confidence").and_then(|v| v.as_f64()).unwrap_or(0.8);

             // Generate ID
             let ts = chrono::Utc::now().timestamp_micros();
             let id = format!("lp-{}", ts);
             
             let desc = format!("{} [Context: {}] [Outcome: {}]", pattern, context, outcome);

             let p = LearnedPattern::new(id, desc)
                 .with_confidence(confidence);

             // Store
             {
                 // AgentMemory needs mutable access to store_learned_pattern
                 let mut memory = am.write().await;
                 memory.store_learned_pattern(p);
                 
                 // Save
                 let path = get_workspace_root().join(".ccos/agent_memory/system_memory.json");
                 memory.save_to_disk(&path).map_err(|e| e.to_string())?;
             }

             Ok(json!({
                 "success": true,
                 "recorded": true
             }))
         })
    });
    register_ecosystem_tool(
        server,
        marketplace.clone(),
        "ccos_record_learning",
        "Explicitly record a learned pattern (e.g. a reusable solution or a failure to avoid)",
        json!({
            "type": "object",
            "properties": {
                "pattern": { "type": "string", "description": "The pattern description" },
                "context": { "type": "string", "description": "Context where this applies" },
                "outcome": { "type": "string", "description": "Outcome observed" },
                "confidence": { "type": "number", "description": "Confidence score 0.0-1.0" }
            },
            "required": ["pattern", "context", "outcome"]
        }),
        handler,
    );

    // ccos/list_capabilities - List internal CCOS capabilities from the marketplace
    let mp3 = marketplace.clone();
    // ccos/list_capabilities - List internal CCOS capabilities from the marketplace
    let mp3 = marketplace.clone();
    let handler = Arc::new(move |_params: serde_json::Value| -> BoxFuture<'static, Result<serde_json::Value, String>> {
        let mp = mp3.clone();
        Box::pin(async move {
            let caps = mp
                .list_capabilities()
                .await
                .into_iter()
                .filter(|c| !is_hidden_capability_id(&c.id))
                .collect::<Vec<_>>();

            Ok(json!({
                "capabilities": caps.iter().map(|c| json!({
                    "id": c.id,
                    "name": c.name,
                    "description": c.description
                })).collect::<Vec<_>>(),
                "count": caps.len(),
                "note": "These are internal CCOS capabilities. Some low-level capabilities are intentionally hidden from this list. For MCP tools exposed by this server, use the tools/list MCP method."
            }))
        })
    });
    register_ecosystem_tool(
        server,
        marketplace.clone(),
        "ccos_list_capabilities",
        "List all registered CCOS capabilities from the internal capability marketplace (not MCP tools - use tools/list for that)",
        json!({
            "type": "object",
            "properties": {},
            "required": []
        }),
        handler,
    );

    // ccos/inspect_capability - Inspect a specific capability's details and schema
    let mp_inspect = marketplace.clone();
    // ccos/inspect_capability - Inspect a specific capability's details and schema
    let mp_inspect = marketplace.clone();
    let handler = Arc::new(move |params: serde_json::Value| -> BoxFuture<'static, Result<serde_json::Value, String>> {
        let mp = mp_inspect.clone();
        Box::pin(async move {
            let cap_id = params
                .get("capability_id")
                .and_then(|v| v.as_str())
                .ok_or_else(|| "Missing capability_id parameter".to_string())?;

            let cap = mp.get_capability(cap_id).await.ok_or_else(|| {
                format!("Capability not found: {}", cap_id)
            })?;

            let input_schema_rtfs = cap
                .input_schema
                .as_ref()
                .map(|s| type_expr_to_rtfs_pretty(s))
                .unwrap_or_else(|| ":any".to_string());

            let output_schema_rtfs = cap
                .output_schema
                .as_ref()
                .map(|s| type_expr_to_rtfs_pretty(s))
                .unwrap_or_else(|| ":any".to_string());

            Ok(json!({
                "id": cap.id,
                "name": cap.name,
                "description": cap.description,
                "version": cap.version,
                "input_schema": input_schema_rtfs,
                "output_schema": output_schema_rtfs,
                "domains": cap.domains,
                "categories": cap.categories,
                "provider": format!("{:?}", cap.provider),
            }))
        })
    });
    register_ecosystem_tool(
        server,
        marketplace.clone(),
        "ccos_inspect_capability",
        "Inspect a capability's details, including its RTFS input/output schemas",
        json!({
            "type": "object",
            "properties": {
                "capability_id": {
                    "type": "string",
                    "description": "The ID of the capability to inspect"
                }
            },
            "required": ["capability_id"]
        }),
        handler,
    );

    // =========================================
    // Phase 2: Static Tools
    // =========================================

    // =========================================
    // DEPRECATED: ccos_analyze_goal
    // Superseded by ccos_plan which provides the same analysis
    // plus actionable sub-steps, gap detection, and next_action guidance.
    // Kept for reference; can be removed in a future cleanup.
    // =========================================
    // let ds = discovery_service.clone();
    // server.register_tool(
    //     "ccos_analyze_goal",
    //     "Analyze a user's goal to extract intents, required capabilities, and suggested next steps",
    //     json!({
    //         "type": "object",
    //         "properties": {
    //             "goal": {
    //                 "type": "string",
    //                 "description": "The user's high-level goal to analyze"
    //             },
    //             "context": {
    //                 "type": "object",
    //                 "description": "Optional context about current state, constraints, etc."
    //             }
    //         },
    //         "required": ["goal"]
    //     }),
    //     Box::new(move |params| {
    //         let ds = ds.clone();
    //         Box::pin(async move {
    //             let goal = params.get("goal").and_then(|v| v.as_str()).unwrap_or("");
    //
    //             if goal.is_empty() {
    //                 return Ok(json!({
    //                     "error": "Goal cannot be empty"
    //                 }));
    //             }
    //
    //             // Try to analyze goal using LLM discovery service
    //             match ds.analyze_goal(goal).await {
    //                 Ok(analysis) => {
    //                     // LLM analysis succeeded
    //                     Ok(json!({
    //                         "goal": goal,
    //                         "analysis": {
    //                             "primary_action": analysis.primary_action,
    //                             "target_object": analysis.target_object,
    //                             "domain_keywords": analysis.domain_keywords,
    //                             "synonyms": analysis.synonyms,
    //                             "implied_concepts": analysis.implied_concepts,
    //                             "expanded_queries": analysis.expanded_queries,
    //                             "confidence": analysis.confidence
    //                         },
    //                         "next_steps": analysis.expanded_queries.iter()
    //                             .map(|q| format!("ccos_search_tools {{ query: '{}' }}", q))
    //                             .collect::<Vec<_>>()
    //                     }))
    //                 }
    //                 Err(_e) => {
    //                     // Fallback to keyword-based analysis
    //                     let fallback = ds.fallback_intent_analysis(goal);
    //                     Ok(json!({
    //                         "goal": goal,
    //                         "analysis": {
    //                             "primary_action": fallback.primary_action,
    //                             "target_object": fallback.target_object,
    //                             "domain_keywords": fallback.domain_keywords,
    //                             "synonyms": fallback.synonyms,
    //                             "implied_concepts": fallback.implied_concepts,
    //                             "expanded_queries": fallback.expanded_queries,
    //                             "confidence": fallback.confidence
    //                         },
    //                         "mode": "fallback",
    //                         "note": "LLM analysis unavailable, using keyword-based fallback",
    //                         "next_steps": fallback.expanded_queries.iter()
    //                             .map(|q| format!("ccos_search_tools {{ query: '{}' }}", q))
    //                             .collect::<Vec<_>>()
    //                     }))
    //                 }
    //             }
    //         })
    //     }),
    // );

    // ccos/execute_plan - Execute an RTFS plan
    let ccos_exec = ccos.clone();

    server.register_tool(
        "ccos_execute_plan",
        "Execute an RTFS plan and return the result.",
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
                },
                "original_goal": {
                    "type": "string",
                    "description": "Optional: The original user intent that triggered this execution. Preserved in session for learning and replay."
                }
            },
            "required": ["plan"]
        }),
        Box::new(move |params| {
            let ccos = ccos_exec.clone();
            Box::pin(async move {
                let plan_str = params.get("plan").and_then(|v| v.as_str()).unwrap_or("");
                let dry_run = params
                    .get("dry_run")
                    .and_then(|v| v.as_bool())
                    .unwrap_or(false);
                let original_goal = params.get("original_goal").and_then(|v| v.as_str());

                if plan_str.is_empty() {
                    return Ok(json!({
                        "success": false,
                        "error": "Plan cannot be empty"
                    }));
                }

                let plan_preview = if plan_str.len() > 200 {
                   format!("{}...", &plan_str[..200]) 
                } else {
                   plan_str.to_string()
                };

                // Parse valid RTFS to create a proper plan object
                // The CCOS validation flow handles syntax checks, but we need a Plan object
                // We use Plan::new_rtfs which takes the code directly
                
                // Create plan object, optionally linking to an intent if goal is provided
                let plan_obj = if let Some(goal) = original_goal {
                     let intent = StorableIntent::new(goal.to_string());
                     
                     // Register intent in graph if we can acquire lock (best effort)
                     if let Ok(mut ig) = ccos.get_intent_graph().lock() {
                         // We ignore storage errors here to simplify the flow - the plan will just have a dangling ID if storage fails
                         let _ = ig.store_intent(intent.clone());
                     }
                     
                     Plan::new_rtfs(plan_str.to_string(), vec![intent.intent_id])
                } else {
                     Plan::new_rtfs(plan_str.to_string(), vec![])
                }; 
                // Note: Plan constructor generates a random ID, but we want to track it possibly
                let plan_id_str = plan_obj.plan_id.clone();
                
                if dry_run {
                    // For dry run, we use the enhanced parser to check for syntax errors in the full program
                     let parse_result = rtfs::parser::parse_with_enhanced_errors(plan_str, None);
                     let is_valid = parse_result.is_ok();
                     let parse_error = parse_result.err().map(|e| format!("{:?}", e));
                     return Ok(json!({
                        "success": is_valid,
                        "dry_run": true,
                        "valid": is_valid,
                        "parse_error": parse_error,
                        "plan_preview": plan_preview,
                        "message": if is_valid {
                            "Plan syntax is valid"
                        } else {
                            "Plan has syntax errors"
                        }
                    }));
                }

                // Execute the plan
                // validate_and_execute_plan is ASYNC so we await it directly
                let result = ccos.validate_and_execute_plan(plan_obj, &RuntimeContext::full()).await;

                match result {
                    Ok(exec_result) => {
                        // exec_result is ExecutionResult
                        let output_json = rtfs_value_to_json(&exec_result.value);
                         Ok(json!({
                            "success": exec_result.success,
                            "result": output_json,
                            "plan_id": plan_id_str,
                            "plan_preview": plan_preview,
                            "metadata": exec_result.metadata
                        }))
                    },
                    Err(e) => Ok(json!({
                        "success": false,
                        "error": e.to_string(),
                        "details": e,
                        "plan_id": plan_id_str,
                        "plan_preview": plan_preview
                    }))
                }
            })
        })
    );

    // ccos/resolve_intent - Find capabilities matching an intent
    let mp4 = marketplace.clone();
    let ds4 = discovery_service.clone();
    server.register_tool(
        "ccos_decompose",
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

                // 1. Get catalog
                let catalog_opt = mp.get_catalog().await;
                let catalog = match catalog_opt.as_ref() {
                    Some(c) => c,
                    None => return Ok(json!({
                        "capabilities": [],
                        "error": "Catalog service unavailable"
                    })),
                };

                // 2. Search catalog with filter
                let mut filter = CatalogFilter::default();
                if let Some(d) = domain {
                    filter.domains = vec![d.to_string()];
                }
                
                let hits = catalog.search_keyword(intent_str, Some(&filter), 50).await;

                if hits.is_empty() {
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
                use ccos::discovery::DiscoveryCategory;

                let search_results: Vec<RegistrySearchResult> = hits
                    .iter()
                    .map(|h| RegistrySearchResult {
                        source: DiscoverySource::Manual { user: "marketplace".to_string() },
                        server_info: ServerInfo {
                            name: h.entry.id.clone(),
                            endpoint: String::new(),
                            description: h.entry.description.clone(),
                            auth_env_var: None,
                            capabilities_path: None,
                            alternative_endpoints: Vec::new(),
                            capability_files: None,
                        },
                        match_score: h.score as f32,
                        alternative_endpoints: Vec::new(),
                        category: DiscoveryCategory::Other,
                    })
                    .collect();

                // 5. Rank results using LLM
                let ranked = match ds.rank_results(intent_str, analysis.as_ref(), search_results).await {
                    Ok(r) => r,
                    Err(_) => {
                        // Fallback to catalog scores if ranking fails
                        return Ok(json!({
                            "intent": intent_str,
                            "capabilities": hits.iter().map(|h| {
                                json!({
                                    "id": h.entry.id,
                                    "name": h.entry.id,
                                    "description": h.entry.description,
                                    "score": h.score
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
    // Planning Tool - ccos_plan (ModularPlanner with LLM decomposition)
    // ============================================================================

    // ccos_plan - Decompose goal into sub-intents using LLM
    // NOTE: This replaces the previous keyword-matching approach with LLM-based decomposition
    // Agent orchestrates execution; CCOS provides control, recording, exploration
    let mp_plan = marketplace.clone();
    let ds_plan = discovery_service.clone();
    server.register_tool(
        "ccos_plan",
        "Decompose goal into sub-intents using LLM. Returns abstract steps WITHOUT tool suggestions - agent decides which tools to use. Identifies capability gaps that need resolution via ccos_suggest_apis.",
        json!({
            "type": "object",
            "properties": {
                "goal": {
                    "type": "string",
                    "description": "The goal to accomplish (e.g., 'get weather in paris tomorrow and send as email')"
                }
            },
            "required": ["goal"]
        }),
        Box::new(move |params| {
            let mp = mp_plan.clone();
            let ds = ds_plan.clone();
            Box::pin(async move {
                let goal = params.get("goal").and_then(|v| v.as_str()).unwrap_or("");

                if goal.is_empty() {
                    return Ok(json!({
                        "success": false,
                        "error": "Goal is required"
                    }));
                }

                // Step 1: Analyze the goal using LLM discovery service
                let analysis = match ds.analyze_goal(goal).await {
                    Ok(a) => a,
                    Err(_) => ds.fallback_intent_analysis(goal),
                };

                // Step 2: Get all capabilities from marketplace for gap detection
                let all_caps = mp.list_capabilities().await;

                // Step 3: Decompose goal into sub-intents
                // Since we don't have direct LLM access here, we use the discovery service's
                // intent analysis to break down into semantic components
                let sub_intents = extract_sub_intents_from_goal(goal, &analysis);
                
                // Step 4: Build a unified plan with all steps in order
                // Each step shows its resolution status inline
                let mut plan = Vec::new();
                let mut gap_count = 0;
                let mut first_gap_query: Option<String> = None;
                let mut first_executable_capability: Option<String> = None;
                
                for (i, sub_intent) in sub_intents.iter().enumerate() {
                    // Find best matching capability
                    // Find best matching capability using LLM
                    // This replaces the naive calculate_match_score loop
                    let ranked = match ds.rank_capabilities(
                        &sub_intent.description, 
                        None, 
                        all_caps.iter().map(|c| c.clone()).collect()
                    ).await {
                        Ok(r) => r,
                        Err(e) => {
                            eprintln!("LLM ranking failed: {}, falling back to naive matching", e);
                            Vec::new()
                        }
                    };

                    let mut best_match: Option<(ccos::capability_marketplace::types::CapabilityManifest, f64)> = None;

                    if let Some(top) = ranked.first() {
                        if top.score > 0.6 { // Higher threshold for LLM confidence
                            best_match = Some((top.capability.clone(), top.score));
                        }
                    } else {
                        // Fallback to catalog matching if LLM fails or returns empty
                        // Use catalog search for fallback
                        if let Some(catalog) = mp.get_catalog().await {
                             let hits = catalog.search_keyword(&sub_intent.description, None, 1).await;
                             if let Some(hit) = hits.first() {
                                 if hit.score > 0.4 {
                                     // Locate full manifest in all_caps
                                     if let Some(cap) = all_caps.iter().find(|c| c.id == hit.entry.id) {
                                         best_match = Some((cap.clone(), hit.score as f64));
                                     }
                                 }
                             }
                        }
                    }
                    
                    if let Some((cap, score)) = best_match {
                        // Resolved step
                        let input_schema_display = cap.input_schema.as_ref()
                            .map(|s| serde_json::to_value(s).unwrap_or(json!({"type": "object"})))
                            .unwrap_or(json!({"type": "object"}));
                        
                        if first_executable_capability.is_none() && gap_count == 0 {
                            first_executable_capability = Some(cap.id.clone());
                        }
                        
                        plan.push(json!({
                            "step": i + 1,
                            "intent": sub_intent.description,
                            "intent_type": sub_intent.intent_type,
                            "status": "resolved",
                            "capability": {
                                "id": cap.id,
                                "name": cap.name,
                                "input_schema": input_schema_display
                            },
                            "match_score": score,
                            "depends_on": sub_intent.depends_on
                        }));
                    } else {
                        // Gap - no matching capability
                        let suggested_query = infer_api_query_from_intent(&sub_intent.description);
                        
                        if first_gap_query.is_none() {
                            first_gap_query = Some(suggested_query.clone());
                        }
                        gap_count += 1;
                        
                        plan.push(json!({
                            "step": i + 1,
                            "intent": sub_intent.description,
                            "intent_type": sub_intent.intent_type,
                            "status": "gap",
                            "reason": "No matching capability found",
                            "suggested_query": suggested_query,
                            "depends_on": sub_intent.depends_on
                        }));
                    }
                }
                
                let has_gaps = gap_count > 0;
                let total_steps = plan.len();
                let resolved_count = total_steps - gap_count;
                
                // Build next_action - one clear action based on state
                let next_action = if let Some(query) = first_gap_query {
                    // There's a gap to resolve
                    json!({
                        "tool": "ccos_suggest_apis",
                        "args": { "query": query },
                        "reason": format!("Step requires a capability that is not currently available ({} gap(s) total)", gap_count)
                    })
                } else if let Some(cap_id) = first_executable_capability {
                    // All resolved, ready to execute
                    json!({
                        "tool": "ccos_execute_capability",
                        "args": { "capability_id": cap_id },
                        "reason": "All steps have matching capabilities. Execute step 1."
                    })
                } else {
                    // No steps at all
                    json!({
                        "tool": "ccos_search_capabilities",
                        "args": { "query": goal },
                        "reason": "Could not decompose goal into actionable steps. Try searching for related capabilities."
                    })
                };

                Ok(json!({
                    "goal": goal,
                    "plan": plan,
                    "summary": {
                        "total_steps": total_steps,
                        "resolved": resolved_count,
                        "gaps": gap_count,
                        "ready_to_execute": !has_gaps && total_steps > 0
                    },
                    "next_action": next_action
                }))
            })
        }),
    );

    // ============================================================================
    // Session Management Tools
    // ============================================================================

    // ccos_session_start - Start a new planning/execution session
    let ss1 = session_store.clone();
    server.register_tool(
        "ccos_session_start",
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
                },
                "original_goal": {
                    "type": "string",
                    "description": "Optional: The original user intent that triggered this execution. Preserved in session for learning and replay."
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
                let original_goal = params.get("original_goal").and_then(|v| v.as_str());

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
                            session_id,
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

                // AUTO-SESSION: Create session automatically if not provided
                // This ensures every execution is tracked and LLM learns about sessions
                let (effective_session_id, session_was_created) = if let Some(sid) = session_id {
                    (sid.to_string(), false)
                } else {
                    // Auto-create a session for this capability execution
                    let mut store = ss.write().await;
                    let session_goal = format!("Auto-session for {}", capability_id);
                    let auto_session = if let Some(og) = original_goal {
                        Session::new_with_original_goal(&session_goal, og)
                    } else {
                        Session::new(&session_goal)
                    };
                    let new_sid = auto_session.id.clone();
                    store.insert(new_sid.clone(), auto_session);
                    (new_sid, true)
                };

                // Record in session
                let step_info = {
                    let mut store = ss.write().await;
                    if let Some(session) = store.get_mut(&effective_session_id) {
                        let step = session.add_step(capability_id, inputs.clone(), result.clone(), success);
                        let rtfs_plan = session.to_rtfs_plan();
                        Some(json!({
                            "session_id": effective_session_id,
                            "step_number": step.step_number,
                            "total_steps": session.steps.len(),
                            "rtfs_plan_so_far": rtfs_plan,
                            "session_was_auto_created": session_was_created
                        }))
                    } else {
                        None
                    }
                };

                // Build response with session guidance
                let session_guidance = if session_was_created {
                    "A session was auto-created for this execution. Use the returned session_id in subsequent ccos_execute_capability calls to build a multi-step RTFS plan. Call ccos_session_plan when done to get the complete RTFS code."
                } else {
                    "Step recorded to session. Continue with next step or call ccos_session_plan to get the complete RTFS code."
                };

                Ok(json!({
                    "success": success,
                    "result": result,
                    "capability_id": capability_id,
                    "rtfs_equivalent": rtfs_equivalent,
                    "session": step_info,
                    "session_guidance": session_guidance,
                    "next_steps": [
                        "To continue: call ccos_execute_capability with same session_id",
                        "To get full plan: call ccos_session_plan with session_id"
                    ]
                }))
            })
        }),
    );

    // ccos_session_plan - Get the accumulated RTFS plan from a session
    let ss3 = session_store.clone();
    server.register_tool(
        "ccos_session_plan",
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
                    // Session not in memory - try to find on disk
                    let sessions_dir = ccos::utils::fs::get_configured_sessions_path();
                    
                    // Look for session file matching the ID
                    let mut found_file = None;
                    if let Ok(entries) = std::fs::read_dir(&sessions_dir) {
                        for entry in entries.flatten() {
                            let filename = entry.file_name().to_string_lossy().to_string();
                            if filename.starts_with(session_id) && filename.ends_with(".rtfs") {
                                found_file = Some(entry.path());
                                break;
                            }
                        }
                    }
                    
                    if let Some(filepath) = found_file {
                        if let Ok(content) = std::fs::read_to_string(&filepath) {
                            return Ok(json!({
                                "success": true,
                                "session_id": session_id,
                                "loaded_from_disk": true,
                                "filepath": filepath.display().to_string(),
                                "rtfs_content": content,
                                "note": "Session was loaded from saved file. In-memory session was not found (server may have restarted)."
                            }));
                        }
                    }
                    
                    Ok(json!({
                        "success": false,
                        "error": format!("Session '{}' not found in memory or on disk at {}", session_id, sessions_dir.display()),
                        "hint": "Sessions are stored in memory during execution. If the server restarted, the session was lost. Check if a .rtfs file exists in the sessions directory."
                    }))
                }
            })
        }),
    );

    // ccos_session_end - Finalize and optionally save the session plan
    let ss4 = session_store.clone();
    server.register_tool(
        "ccos_session_end",
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

                    // Always save session to file by default
                    let saved_path = match save_session(&session, None, save_as) {
                        Ok(p) => Some(p.to_string_lossy().to_string()),
                        Err(e) => {
                            eprintln!("[ccos-mcp] Failed to auto-save session: {}", e);
                            None
                        }
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

    // ccos_consolidate_session - Convert a session into a reusable Agent Capability
    let mp_consolidate = marketplace.clone();
    server.register_tool(
        "ccos_consolidate_session",
        "Consolidates a Session Trace into a new Agent Capability using the Planner. Returns the new Agent ID which is immediately available for use.",
        json!({
            "type": "object",
            "properties": {
                "session_id": {
                    "type": "string",
                    "description": "ID of the session to consolidate (must be saved to disk)"
                },
                "agent_name": {
                    "type": "string",
                    "description": "Name for the new agent (e.g. 'research_helper')"
                },
                "description": {
                    "type": "string",
                    "description": "Description of what the agent does"
                }
            },
            "required": ["session_id", "agent_name"]
        }),
        Box::new(move |params| {
            let mp = mp_consolidate.clone();
            Box::pin(async move {
                let session_id = params
                    .get("session_id")
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| RuntimeError::Generic("session_id is required".to_string()))?;

                let agent_name = params
                    .get("agent_name")
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| RuntimeError::Generic("agent_name is required".to_string()))?;

                let description = params
                    .get("description")
                    .and_then(|v| v.as_str())
                    .map(|s| s.to_string());

                let inputs = json!({
                    "session_id": session_id,
                    "agent_name": agent_name,
                    "description": description
                });

                // Call planner.synthesize_agent_from_trace
                let rtfs_inputs = json_to_rtfs_value(&inputs)
                    .map_err(|e| RuntimeError::Generic(format!("Input conversion failed: {}", e)))?;

                let result_value = tokio::task::block_in_place(|| {
                    tokio::runtime::Handle::current().block_on(async {
                        mp.execute_capability(
                            "planner.synthesize_agent_from_trace",
                            &rtfs_inputs,
                        )
                        .await
                    })
                });

                match result_value {
                    Ok(val) => rtfs_value_to_json(&val)
                        .map_err(|e| RuntimeError::Generic(format!("Output conversion failed: {}", e))),
                    Err(e) => Ok(json!({
                        "success": false,
                        "error": e.to_string()
                    })),
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
                        
                        // Queue secret approval
                        let _ = aq.add_secret_approval(
                            "ccos.system.secrets".to_string(),
                            name.clone(),
                            format!("Required for capability execution: {}", name),
                            24 * 7, // 7 days
                        ).await;

                        missing_details.push(json!({
                            "name": name,
                            "status": "not_available",
                            "approval_queued": true
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
                        format!("Missing secrets have been queued for approval. Visit http://localhost:3000/approvals to provide them. Do NOT attempt to find or guess secrets yourself.")
                    },
                    "agent_guidance": if missing.is_empty() {
                        "All required secrets are available. You may proceed with capability execution.".to_string()
                    } else {
                        "CRITICAL: Missing secrets detected and queued. You MUST stop execution now and ask the user to visit http://localhost:3000/approvals. Only resume once the user confirms they have provided the secrets.".to_string()
                    }
                }))
            })
        }),
    );

    // ccos_introspect_remote_api - Introspect an external MCP/OpenAPI server and queue for approval
    let aq_introspect = approval_queue.clone();
    let ds_introspect = discovery_service.clone();
    let agent_config_for_pipeline = agent_config.clone();
    server.register_tool(
        "ccos_introspect_remote_api",
        "Introspect an external server (MCP, OpenAPI, or HTML documentation) to discover its capabilities. Works with MCP endpoints, OpenAPI specs, and API documentation pages. Creates an approval request for the server.",
        json!({
            "type": "object",
            "properties": {
                "endpoint": {
                    "type": "string",
                    "description": "Server endpoint URL (e.g., 'https://api.example.com'), documentation URL (e.g., 'https://example.com/docs/api'), or npx command (e.g., 'npx -y @modelcontextprotocol/server-github')"
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
            let ds = ds_introspect.clone();
            let agent_config = agent_config_for_pipeline.clone();
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

                let server_name_guess = name.clone().unwrap_or_else(|| {
                    if let Ok(url) = url::Url::parse(endpoint) {
                        url.host_str()
                            .map(|h| h.replace(".", "_"))
                            .unwrap_or_else(|| endpoint.split('/').last().unwrap_or("unknown").to_string())
                    } else {
                        endpoint.split('/').last().unwrap_or("unknown").to_string()
                    }
                });

                // Use from_config for consistency with ccos-explore
                let default_config = AgentConfig::default();
                let cfg = agent_config.as_ref().unwrap_or(&default_config);
                let pipeline = match ServerDiscoveryPipeline::from_config(
                    cfg,
                    aq.clone(),
                    Some(ds.clone()),
                )
                .await
                {
                    Ok(p) => p,
                    Err(e) => {
                        return Ok(json!({
                            "success": false,
                            "error": format!("Failed to initialize discovery pipeline: {}", e)
                        }))
                    }
                };

                let target = PipelineTarget {
                    input: endpoint.to_string(),
                    name: name.clone(),
                    auth_env_var: auth_env_var.clone(),
                };

                let queue_result = match pipeline.stage_and_queue(target).await {
                    Ok(res) => res,
                    Err(e) => {
                        return Ok(json!({
                            "success": false,
                            "error": format!("Introspection failed: {}", e),
                            "ask_user": generate_introspection_failure_suggestions(&server_name_guess, endpoint)
                        }))
                    }
                };

                let server_name = queue_result
                    .preview
                    .as_ref()
                    .map(|p| p.server_name.clone())
                    .unwrap_or(server_name_guess);

                let mut response = json!({
                    "success": true,
                    "approval_id": queue_result.approval_id,
                    "server_name": server_name,
                    "source": queue_result.source,
                    "capability_files_count": queue_result.capability_files.len(),
                    "agent_guidance": format!("Server '{}' introspected. MANDATORY: You must now ask the user to manually approve this server at the /approvals UI. DO NOT call ccos_register_server until the user confirms approval.", server_name),
                    "next_steps": [
                        "Notify the user that introspection is complete.",
                        "Tell the user to go to the CCOS Control Center at /approvals to approve the new capabilities.",
                        "WAIT for the user to confirm they have approved the server.",
                        "Only after user confirmation, call ccos_register_server with approval_id to finalize registration."
                    ]
                });

                if let Some(preview) = &queue_result.preview {
                    response["discovered_endpoints_count"] = json!(preview.endpoints.len());
                    response["endpoint_previews"] = json!(preview
                        .endpoints
                        .iter()
                        .take(10)
                        .map(|ep| json!({"id": ep.id, "name": ep.name, "method": ep.method, "path": ep.path}))
                        .collect::<Vec<_>>());
                }

                Ok(response)
            })
        }),
    );



    // ccos_list_approvals - List all approvals with optional status filter
    let aq_list = approval_queue.clone();
    server.register_tool(
        "ccos_list_approvals",
        "List all approval requests with optional status filter. Use this to see pending, rejected, expired, or approved requests.",
        json!({
            "type": "object",
            "properties": {
                "status": {
                    "type": "string",
                    "description": "Filter by status: 'pending', 'rejected', 'expired', 'approved', or 'all' (default: 'all')",
                    "enum": ["pending", "rejected", "expired", "approved", "all"]
                },
                "limit": {
                    "type": "integer",
                    "description": "Maximum number of results to return (default: 20)"
                }
            }
        }),
        Box::new(move |params| {
            let aq = aq_list.clone();
            Box::pin(async move {
                let status_filter = params.get("status")
                    .and_then(|v| v.as_str())
                    .unwrap_or("all");
                let limit = params.get("limit")
                    .and_then(|v| v.as_i64())
                    .unwrap_or(20) as usize;

                let all_approvals = match aq.list(ccos::approval::types::ApprovalFilter::default()).await {
                    Ok(approvals) => approvals,
                    Err(e) => return Ok(json!({"error": format!("Failed to list approvals: {}", e)})),
                };


                let filtered: Vec<_> = all_approvals
                    .into_iter()
                    .filter(|req| {
                        match status_filter {
                            "pending" => req.status.is_pending(),
                            "rejected" => req.status.is_rejected(),
                            "expired" => matches!(req.status, ccos::approval::types::ApprovalStatus::Expired { .. }),
                            "approved" => matches!(req.status, ccos::approval::types::ApprovalStatus::Approved { .. }),
                            _ => true, // "all"
                        }
                    })
                    .take(limit)
                    .collect();

                let result: Vec<serde_json::Value> = filtered.iter().map(|req| {
                    let status_str = if req.status.is_pending() {
                        "pending"
                    } else if req.status.is_rejected() {
                        "rejected"
                    } else if matches!(req.status, ccos::approval::types::ApprovalStatus::Expired { .. }) {
                        "expired"
                    } else {
                        "approved"
                    };

                    let category = match &req.category {
                        ccos::approval::types::ApprovalCategory::ServerDiscovery { server_info, .. } => {
                            json!({ "type": "ServerDiscovery", "name": server_info.name, "endpoint": server_info.endpoint })
                        }
                        ccos::approval::types::ApprovalCategory::EffectApproval { capability_id, effects, .. } => {
                            json!({ "type": "EffectApproval", "capability_id": capability_id, "effects": effects })
                        }
                        ccos::approval::types::ApprovalCategory::SynthesisApproval { capability_id, .. } => {
                            json!({ "type": "SynthesisApproval", "capability_id": capability_id })
                        }
                        ccos::approval::types::ApprovalCategory::LlmPromptApproval { .. } => {
                            json!({ "type": "LlmPromptApproval" })
                        }
                        ccos::approval::types::ApprovalCategory::SecretRequired { capability_id, secret_type, .. } => {
                            json!({ "type": "SecretRequired", "capability_id": capability_id, "secret_type": secret_type })
                        }
                        ccos::approval::types::ApprovalCategory::BudgetExtension { plan_id, dimension, requested_additional, .. } => {
                            json!({ "type": "BudgetExtension", "plan_id": plan_id, "dimension": dimension, "requested_additional": requested_additional })
                        }
                    };

                    json!({
                        "id": req.id,
                        "status": status_str,
                        "category": category,
                        "requested_at": req.requested_at.to_rfc3339(),
                        "expires_at": req.expires_at.to_rfc3339()
                    })
                }).collect();

                Ok(json!({
                    "success": true,
                    "count": result.len(),
                    "approvals": result,
                    "hint": "Use ccos_reapprove to re-approve rejected or expired approvals."
                }))
            })
        }),
    );

    // ccos_reapprove - Re-approve a rejected or expired approval
    let aq_reapprove = approval_queue.clone();
    server.register_tool(
        "ccos_reapprove",
        "Re-approve a previously rejected or expired approval request. Use ccos_list_approvals to find rejected/expired approvals first.",
        json!({
            "type": "object",
            "properties": {
                "approval_id": {
                    "type": "string",
                    "description": "The ID of the approval to re-approve"
                }
            },
            "required": ["approval_id"]
        }),
        Box::new(move |params| {
            let aq = aq_reapprove.clone();
            Box::pin(async move {
                let approval_id = match params.get("approval_id").and_then(|v| v.as_str()) {
                    Some(id) => id,
                    None => return Ok(json!({"error": "Missing required parameter: approval_id"})),
                };

                // Check if the approval exists
                let req = match aq.get(approval_id).await {
                    Ok(Some(r)) => r,
                    Ok(None) => return Ok(json!({"error": format!("Approval not found: {}", approval_id)})),
                    Err(e) => return Ok(json!({"error": format!("Failed to get approval: {}", e)})),
                };

                // Approve it
                if let Err(e) = aq.approve(
                    approval_id,
                    ccos::approval::types::ApprovalAuthority::User("mcp_agent".to_string()),
                    Some("Re-approved via MCP tool".to_string())
                ).await {
                    return Ok(json!({"error": format!("Failed to re-approve: {}", e)}));
                }

                let category_type = match &req.category {
                    ccos::approval::types::ApprovalCategory::ServerDiscovery { .. } => "ServerDiscovery",
                    ccos::approval::types::ApprovalCategory::EffectApproval { .. } => "EffectApproval",
                    ccos::approval::types::ApprovalCategory::SynthesisApproval { .. } => "SynthesisApproval",
                    ccos::approval::types::ApprovalCategory::LlmPromptApproval { .. } => "LlmPromptApproval",
                    ccos::approval::types::ApprovalCategory::SecretRequired { .. } => "SecretRequired",
                    ccos::approval::types::ApprovalCategory::BudgetExtension { .. } => "BudgetExtension",
                };

                Ok(json!({
                    "success": true,
                    "approval_id": approval_id,
                    "category_type": category_type,
                    "message": "Approval has been re-approved successfully"
                }))
            })
        }),
    );

    // ccos_budget_approve - Approve a budget extension and resume execution from checkpoint
    let aq_budget_approve = approval_queue.clone();
    let ccos_budget = ccos.clone();
    server.register_tool(
        "ccos_budget_approve",
        "Approve a budget extension and resume execution from the associated checkpoint.",
        json!({
            "type": "object",
            "properties": {
                "approval_id": {
                    "type": "string",
                    "description": "Approval ID for the budget extension request"
                },
                "checkpoint_id": {
                    "type": "string",
                    "description": "Optional checkpoint ID to resume (auto-resolved if omitted)"
                },
                "extensions": {
                    "type": "object",
                    "description": "Optional map of budget extensions (e.g. {\"llm_tokens\": 5000, \"cost_usd\": 0.25})"
                },
                "reason": {
                    "type": "string",
                    "description": "Optional approval reason"
                }
            },
            "required": ["approval_id"]
        }),
        Box::new(move |params| {
            let aq = aq_budget_approve.clone();
            let ccos = ccos_budget.clone();
            Box::pin(async move {
                let approval_id = match params.get("approval_id").and_then(|v| v.as_str()) {
                    Some(id) => id,
                    None => return Ok(json!({"error": "Missing required parameter: approval_id"})),
                };

                let checkpoint_override = params.get("checkpoint_id").and_then(|v| v.as_str());
                let reason = params.get("reason").and_then(|v| v.as_str()).map(|s| s.to_string());

                let req = match aq.get(approval_id).await {
                    Ok(Some(r)) => r,
                    Ok(None) => return Ok(json!({"error": format!("Approval not found: {}", approval_id)})),
                    Err(e) => return Ok(json!({"error": format!("Failed to load approval: {}", e)})),
                };

                if !req.status.is_pending() {
                    return Ok(json!({"error": "Approval is not pending"}));
                }

                let (plan_id, intent_id, dimension, requested_additional) = match &req.category {
                    ccos::approval::types::ApprovalCategory::BudgetExtension {
                        plan_id,
                        intent_id,
                        dimension,
                        requested_additional,
                        ..
                    } => (
                        plan_id.clone(),
                        intent_id.clone(),
                        dimension.clone(),
                        *requested_additional,
                    ),
                    _ => return Ok(json!({"error": "Approval is not a budget extension"})),
                };

                // Approve the request first
                if let Err(e) = aq
                    .approve(
                        approval_id,
                        ccos::approval::types::ApprovalAuthority::User("mcp_agent".to_string()),
                        reason.clone().or(Some("Budget extension approved via MCP".to_string())),
                    )
                    .await
                {
                    return Ok(json!({"error": format!("Failed to approve: {}", e)}));
                }

                let checkpoint_id = if let Some(cid) = checkpoint_override {
                    cid.to_string()
                } else {
                    match ccos
                        .orchestrator
                        .find_latest_checkpoint_for_plan_intent(&plan_id, &intent_id)
                    {
                        Some(rec) => rec.checkpoint_id,
                        None => {
                            return Ok(json!({
                                "error": "No checkpoint found for plan/intent",
                                "plan_id": plan_id,
                                "intent_id": intent_id
                            }))
                        }
                    }
                };

                let mut extensions: std::collections::HashMap<String, f64> =
                    std::collections::HashMap::new();
                if let Some(map) = params.get("extensions").and_then(|v| v.as_object()) {
                    for (k, v) in map {
                        if let Some(f) = v.as_f64() {
                            extensions.insert(k.clone(), f);
                        } else if let Some(i) = v.as_i64() {
                            extensions.insert(k.clone(), i as f64);
                        }
                    }
                }
                if extensions.is_empty() {
                    extensions.insert(dimension.clone(), requested_additional);
                }

                let plan = match ccos.orchestrator.get_plan_by_id(&plan_id) {
                    Ok(Some(plan)) => plan,
                    Ok(None) => {
                        return Ok(json!({
                            "error": "Plan not found in archive",
                            "plan_id": plan_id
                        }))
                    }
                    Err(e) => return Ok(json!({"error": format!("Failed to load plan: {}", e)})),
                };

                let context = RuntimeContext::full();
                let exec_result = ccos
                    .orchestrator
                    .resume_and_continue_from_checkpoint_with_budget_extensions(
                        &plan,
                        &context,
                        &checkpoint_id,
                        Some(extensions),
                    )
                    .await;

                match exec_result {
                    Ok(result) => {
                        let output_json = rtfs_value_to_json(&result.value);
                        Ok(json!({
                            "success": result.success,
                            "plan_id": plan_id,
                            "intent_id": intent_id,
                            "checkpoint_id": checkpoint_id,
                            "result": output_json,
                            "metadata": result.metadata
                        }))
                    }
                    Err(e) => Ok(json!({
                        "success": false,
                        "error": e.to_string(),
                        "plan_id": plan_id,
                        "intent_id": intent_id,
                        "checkpoint_id": checkpoint_id
                    })),
                }
            })
        }),
    );

    // ccos_budget_deny - Deny a budget extension and discard the checkpoint
    let aq_budget_deny = approval_queue.clone();
    let ccos_budget_deny = ccos.clone();
    server.register_tool(
        "ccos_budget_deny",
        "Deny a budget extension approval and cancel the pending resume.",
        json!({
            "type": "object",
            "properties": {
                "approval_id": {
                    "type": "string",
                    "description": "Approval ID for the budget extension request"
                },
                "checkpoint_id": {
                    "type": "string",
                    "description": "Optional checkpoint ID to discard (auto-resolved if omitted)"
                },
                "reason": {
                    "type": "string",
                    "description": "Optional denial reason"
                }
            },
            "required": ["approval_id"]
        }),
        Box::new(move |params| {
            let aq = aq_budget_deny.clone();
            let ccos = ccos_budget_deny.clone();
            Box::pin(async move {
                let approval_id = match params.get("approval_id").and_then(|v| v.as_str()) {
                    Some(id) => id,
                    None => return Ok(json!({"error": "Missing required parameter: approval_id"})),
                };
                let checkpoint_override = params.get("checkpoint_id").and_then(|v| v.as_str());
                let reason = params
                    .get("reason")
                    .and_then(|v| v.as_str())
                    .unwrap_or("Budget extension denied via MCP")
                    .to_string();

                let req = match aq.get(approval_id).await {
                    Ok(Some(r)) => r,
                    Ok(None) => return Ok(json!({"error": format!("Approval not found: {}", approval_id)})),
                    Err(e) => return Ok(json!({"error": format!("Failed to load approval: {}", e)})),
                };

                if !req.status.is_pending() {
                    return Ok(json!({"error": "Approval is not pending"}));
                }

                let (plan_id, intent_id) = match &req.category {
                    ccos::approval::types::ApprovalCategory::BudgetExtension {
                        plan_id,
                        intent_id,
                        ..
                    } => (plan_id.clone(), intent_id.clone()),
                    _ => return Ok(json!({"error": "Approval is not a budget extension"})),
                };

                if let Err(e) = aq
                    .reject(
                        approval_id,
                        ccos::approval::types::ApprovalAuthority::User("mcp_agent".to_string()),
                        reason.clone(),
                    )
                    .await
                {
                    return Ok(json!({"error": format!("Failed to reject: {}", e)}));
                }

                let checkpoint_id = if let Some(cid) = checkpoint_override {
                    Some(cid.to_string())
                } else {
                    ccos
                        .orchestrator
                        .find_latest_checkpoint_for_plan_intent(&plan_id, &intent_id)
                        .map(|rec| rec.checkpoint_id)
                };

                if let Some(cid) = &checkpoint_id {
                    let _ = ccos.orchestrator.remove_checkpoint(cid);
                }

                Ok(json!({
                    "success": true,
                    "approval_id": approval_id,
                    "plan_id": plan_id,
                    "intent_id": intent_id,
                    "checkpoint_id": checkpoint_id,
                    "message": "Budget extension denied"
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
                    "description": "The approval ID returned from ccos_introspect_remote_api"
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

                match ccos::ops::server::register_approved_server(
                    approval_id.to_string(),
                    &aq,
                    &mp,
                ).await {
                    Ok(count) => {
                        Ok(json!({
                            "success": true,
                            "message": format!("Successfully registered {} tools", count),
                            "tools_count": count
                        }))
                    },
                    Err(e) => Ok(json!({
                        "success": false,
                        "error": format!("Failed to register server: {}", e)
                    }))
                }
            })
        }),
    );

    // ============================================================================
    // RTFS Teaching Tools
    // ============================================================================

    // rtfs_get_grammar - Get RTFS grammar reference by category
    server.register_tool(
        "rtfs_get_grammar",
        "Get RTFS language grammar reference. Returns syntax rules by category.",
        json!({
            "type": "object",
            "properties": {
                "category": {
                    "type": "string",
                    "enum": ["overview", "literals", "collections", "special_forms", "types", "purity_effects", "all"],
                    "default": "overview",
                    "description": "Grammar category to retrieve"
                }
            }
        }),
        Box::new(move |params| {
            Box::pin(async move {
                let category = params.get("category")
                    .and_then(|v| v.as_str())
                    .unwrap_or("overview");

                let content = match category {
                    "overview" => GRAMMAR_OVERVIEW,
                    "literals" => GRAMMAR_LITERALS,
                    "collections" => GRAMMAR_COLLECTIONS,
                    "special_forms" => GRAMMAR_SPECIAL_FORMS,
                    "types" => GRAMMAR_TYPES,
                    "purity_effects" => GRAMMAR_PURITY_EFFECTS,
                    "all" => &format!("{}\n\n{}\n\n{}\n\n{}\n\n{}\n\n{}",
                        GRAMMAR_OVERVIEW, GRAMMAR_LITERALS, GRAMMAR_COLLECTIONS,
                        GRAMMAR_SPECIAL_FORMS, GRAMMAR_TYPES, GRAMMAR_PURITY_EFFECTS),
                    _ => GRAMMAR_OVERVIEW,
                };

                Ok(json!({
                    "category": category,
                    "content": content
                }))
            })
        }),
    );

    // rtfs_get_samples - Get example RTFS code snippets by category
    server.register_tool(
        "rtfs_get_samples",
        "Get example RTFS code snippets by category",
        json!({
            "type": "object",
            "properties": {
                "category": {
                    "type": "string",
                    "enum": ["basic", "bindings", "control_flow", "functions", "capabilities", "types", "all"],
                    "default": "basic",
                    "description": "Sample category to retrieve"
                }
            }
        }),
        Box::new(move |params| {
            Box::pin(async move {
                let category = params.get("category")
                    .and_then(|v| v.as_str())
                    .unwrap_or("basic");

                let samples = get_samples_for_category(category);

                Ok(json!({
                    "category": category,
                    "samples": samples
                }))
            })
        }),
    );

    // rtfs_compile - Parse/compile RTFS code and return result or detailed errors
    server.register_tool(
        "rtfs_compile",
        "Compile RTFS code. Returns success info or detailed parse errors.",
        json!({
            "type": "object",
            "properties": {
                "code": { "type": "string", "description": "RTFS source code" },
                "show_ast": { "type": "boolean", "default": false }
            },
            "required": ["code"]
        }),
        Box::new(move |params| {
            Box::pin(async move {
                let code = params.get("code").and_then(|v| v.as_str()).unwrap_or("");
                let show_ast = params
                    .get("show_ast")
                    .and_then(|v| v.as_bool())
                    .unwrap_or(false);

                if code.is_empty() {
                    return Ok(json!({
                        "success": false,
                        "error": "No code provided"
                    }));
                }

                match rtfs::parser::parse(code) {
                    Ok(ast) => {
                        let mut result = json!({
                            "success": true,
                            "message": "Compilation successful",
                            "expression_count": ast.len()
                        });

                        if show_ast {
                            result["ast"] = json!(format!("{:#?}", ast));
                        }

                        Ok(result)
                    }
                    Err(parse_error) => Ok(json!({
                        "success": false,
                        "error": {
                            "message": format!("{:?}", parse_error)
                        },
                        "code_preview": code.chars().take(100).collect::<String>()
                    })),
                }
            })
        }),
    );

    // rtfs_explain_error - Explain error messages in plain English with common causes
    server.register_tool(
        "rtfs_explain_error",
        "Explain an RTFS error message in plain English with common causes",
        json!({
            "type": "object",
            "properties": {
                "error_message": { "type": "string", "description": "The error message to explain" },
                "code": { "type": "string", "description": "The code that produced error (optional)" }
            },
            "required": ["error_message"]
        }),
        Box::new(move |params| {
            Box::pin(async move {
                let error_msg = params.get("error_message")
                    .and_then(|v| v.as_str())
                    .unwrap_or("");
                let code = params.get("code").and_then(|v| v.as_str());

                Ok(json!(explain_rtfs_error(error_msg, code)))
            })
        }),
    );

    // rtfs_repair - Suggest repairs for broken RTFS code (heuristics or LLM-based)
    server.register_tool(
        "rtfs_repair",
        "Suggest repairs for broken RTFS code. Set use_llm=true for comprehensive LLM-based repairs using prompt templates.",
        json!({
            "type": "object",
            "properties": {
                "code": { "type": "string", "description": "Broken RTFS code" },
                "error_message": { "type": "string", "description": "Optional error message from compilation" },
                "use_llm": { "type": "boolean", "description": "Use LLM-based repair with comprehensive prompt templates (slower but more accurate)", "default": false }
            },
            "required": ["code"]
        }),
        Box::new({
            let ccos = ccos.clone();
            move |params| {
                let ccos = ccos.clone();
                Box::pin(async move {
                    let code = params.get("code").and_then(|v| v.as_str()).unwrap_or("");
                    let error = params.get("error_message").and_then(|v| v.as_str());
                    let use_llm = params.get("use_llm").and_then(|v| v.as_bool()).unwrap_or(false);

                    if use_llm {
                        // Use LLM-based repair with comprehensive prompt templates
                        let validation_config = &ccos.agent_config.validation;
                        let result = repair_rtfs_code_with_llm(code, error, validation_config).await;
                        Ok(json!(result))
                    } else {
                        // Fast heuristic-based repair
                        let suggestions = repair_rtfs_code(code);
                        Ok(json!(suggestions))
                    }
                })
            }
        }),
    );
}

/// Get RTFS code samples for a given category
fn get_samples_for_category(category: &str) -> Vec<serde_json::Value> {
    match category {
        "basic" => vec![
            json!({
                "name": "Arithmetic",
                "code": "(+ 1 2 3)",
                "result": "6",
                "explanation": "Add numbers together"
            }),
            json!({
                "name": "String concatenation",
                "code": "(str \"Hello, \" \"World!\")",
                "result": "\"Hello, World!\"",
                "explanation": "Concatenate strings"
            }),
            json!({
                "name": "Vector creation",
                "code": "[1 2 3 4 5]",
                "result": "[1, 2, 3, 4, 5]",
                "explanation": "Create a vector literal"
            }),
        ],
        "bindings" => vec![
            json!({
                "name": "Simple let binding",
                "code": "(let [x 5] x)",
                "result": "5",
                "explanation": "Bind x to 5, return x"
            }),
            json!({
                "name": "Multiple bindings",
                "code": "(let [x 5 y 10] (+ x y))",
                "result": "15",
                "explanation": "Bind multiple values, use in expression"
            }),
            json!({
                "name": "Destructuring",
                "code": "(let [{:keys [name age]} {:name \"Alice\" :age 30}] name)",
                "result": "\"Alice\"",
                "explanation": "Extract values from map using destructuring"
            }),
        ],
        "control_flow" => vec![
            json!({
                "name": "If expression",
                "code": "(if (> 5 3) \"yes\" \"no\")",
                "result": "\"yes\"",
                "explanation": "Conditional expression"
            }),
            json!({
                "name": "Pattern matching",
                "code": "(match 42\n  0 \"zero\"\n  n (str \"number: \" n))",
                "result": "\"number: 42\"",
                "explanation": "Match value against patterns"
            }),
        ],
        "functions" => vec![
            json!({
                "name": "Anonymous function",
                "code": "((fn [x] (* x x)) 5)",
                "result": "25",
                "explanation": "Define and immediately call anonymous function"
            }),
            json!({
                "name": "Named function",
                "code": "(defn greet [name]\n  (str \"Hello, \" name))\n(greet \"World\")",
                "result": "\"Hello, World\"",
                "explanation": "Define named function, then call it"
            }),
            json!({
                "name": "Higher-order function",
                "code": "(map (fn [x] (* x 2)) [1 2 3])",
                "result": "[2, 4, 6]",
                "explanation": "Apply function to each element"
            }),
        ],
        "capabilities" => vec![
            json!({
                "name": "Call capability",
                "code": "(call :ccos.http/get {:url \"https://api.example.com\"})",
                "explanation": "Call CCOS HTTP capability"
            }),
            json!({
                "name": "Capability definition",
                "code": "(capability \"my-tool/greet\"\n  :description \"Greet a user\"\n  :input-schema [:map [:name :string]]\n  :output-schema :string\n  :implementation\n  (fn [input]\n    (str \"Hello, \" (:name input))))",
                "explanation": "Define a new capability with schema"
            }),
        ],
        "types" => vec![
            json!({
                "name": "Type annotation",
                "code": "(defn add [x :int y :int] :int\n  (+ x y))",
                "explanation": "Function with type annotations"
            }),
            json!({
                "name": "Complex schema",
                "code": "[:map\n  [:name :string]\n  [:age [:and :int [:>= 0]]]\n  [:email {:optional true} :string]]",
                "explanation": "Map schema with optional field and refined type"
            }),
        ],
        "all" | _ => {
            let mut all = vec![];
            for cat in [
                "basic",
                "bindings",
                "control_flow",
                "functions",
                "capabilities",
                "types",
            ] {
                all.extend(get_samples_for_category(cat));
            }
            all
        }
    }
}

/// Explain RTFS error messages in plain English
fn explain_rtfs_error(error: &str, code: Option<&str>) -> serde_json::Value {
    let error_lower = error.to_lowercase();

    let (explanation, common_causes, fix_suggestions) =
        if error_lower.contains("expected") && error_lower.contains("expression") {
            (
                "The parser expected an expression but found something else.",
                vec![
                    "Unbalanced parentheses - missing opening or closing paren",
                    "Empty list without proper content",
                    "Incomplete expression at end of input",
                ],
                vec![
                    "Check that all ( have matching )",
                    "Ensure lists have at least an operator: (+ 1 2) not ()",
                    "Complete any unfinished expressions",
                ],
            )
        } else if error_lower.contains("unexpected") && error_lower.contains("token") {
            (
                "The parser encountered a token it didn't expect in this position.",
                vec![
                    "Wrong syntax for the construct being used",
                    "Missing required elements",
                    "Extra tokens that don't belong",
                ],
                vec![
                    "Review the syntax for the form you're using",
                    "Check for typos in keywords",
                    "Ensure proper ordering of elements",
                ],
            )
        } else if error_lower.contains("unbalanced") || error_lower.contains("unclosed") {
            (
                "There are mismatched brackets in the code.",
                vec![
                    "Missing closing ) ] or }",
                    "Extra opening ( [ or {",
                    "Brackets of wrong type used",
                ],
                vec![
                    "Count opening and closing brackets",
                    "Use an editor with bracket matching",
                    "Remember: () for calls, [] for vectors/bindings, {} for maps",
                ],
            )
        } else if error_lower.contains("undefined") || error_lower.contains("not found") {
            (
                "A symbol or function was referenced but not defined.",
                vec![
                    "Typo in function or variable name",
                    "Using a variable before it's defined",
                    "Missing import or require",
                ],
                vec![
                    "Check spelling of the symbol",
                    "Ensure definitions come before usage",
                    "Use :keys in let bindings for map destructuring",
                ],
            )
        } else {
            (
                "This error indicates a problem with the RTFS code.",
                vec!["Syntax error", "Semantic error"],
                vec![
                    "Review the code structure",
                    "Compare with working examples",
                    "Use rtfs_get_samples to see correct syntax",
                ],
            )
        };

    let mut result = json!({
        "error": error,
        "explanation": explanation,
        "common_causes": common_causes,
        "suggestions": fix_suggestions
    });

    if let Some(code) = code {
        let open_parens = code.matches('(').count();
        let close_parens = code.matches(')').count();
        let open_brackets = code.matches('[').count();
        let close_brackets = code.matches(']').count();
        let open_braces = code.matches('{').count();
        let close_braces = code.matches('}').count();

        let mut issues = vec![];
        if open_parens != close_parens {
            issues.push(format!(
                "Paren mismatch: {} '(' vs {} ')'",
                open_parens, close_parens
            ));
        }
        if open_brackets != close_brackets {
            issues.push(format!(
                "Bracket mismatch: {} '[' vs {} ']'",
                open_brackets, close_brackets
            ));
        }
        if open_braces != close_braces {
            issues.push(format!(
                "Brace mismatch: {} '{{' vs {} '}}'",
                open_braces, close_braces
            ));
        }

        if !issues.is_empty() {
            result["bracket_analysis"] = json!(issues);
        }
    }

    result
}

/// Suggest repairs for broken RTFS code using heuristics
fn repair_rtfs_code(code: &str) -> serde_json::Value {
    let mut suggestions: Vec<serde_json::Value> = vec![];

    // Handle all bracket types
    let pairs = [('(', ')', "parenthesis"), ('[', ']', "square bracket"), ('{', '}', "curly brace")];
    let mut result_code = code.to_string();

    for (open, close, name) in pairs {
        let open_count = code.matches(open).count();
        let close_count = code.matches(close).count();

        if open_count > close_count {
            let missing = open_count - close_count;
            let added = close.to_string().repeat(missing);
            result_code.push_str(&added);
            suggestions.push(json!({
                "type": "bracket_fix",
                "description": format!("Add {} missing closing {}", missing, name),
                "repaired_code": result_code.clone(),
                "confidence": "high"
            }));
        } else if close_count > open_count {
            let extra = close_count - open_count;
            // For extra closing, we can't easily know where the opening one should be, 
            // but we can suggest removing them if they are at the very end
            let mut trimmed = result_code.clone();
            let mut removed = 0;
            while trimmed.ends_with(close) && removed < extra {
                trimmed.pop();
                removed += 1;
            }
            
            if removed > 0 {
                result_code = trimmed;
                suggestions.push(json!({
                    "type": "bracket_fix",
                    "description": format!("Remove {} extra closing {} from end", removed, name),
                    "repaired_code": result_code.clone(),
                    "confidence": "high"
                }));
            } else {
                // Try a more aggressive repair: remove the first occurrence of the extra bracket
                let mut aggressive_repair = result_code.clone();
                if let Some(pos) = aggressive_repair.find(close) {
                    aggressive_repair.remove(pos);
                    result_code = aggressive_repair;
                    suggestions.push(json!({
                        "type": "bracket_fix",
                        "description": format!("Remove extra closing {} at position {}", name, pos),
                        "repaired_code": result_code.clone(),
                        "confidence": "medium"
                    }));
                } else {
                    suggestions.push(json!({
                        "type": "bracket_fix",
                        "description": format!("Remove {} extra closing {} or add opening ones", extra, name),
                        "confidence": "low"
                    }));
                }
            }
        }
    }

    let typo_fixes = vec![
        ("defun", "defn"),
        ("lambda", "fn"),
        ("define", "def"),
        ("cond", "match"),
        ("null", "nil"),
        ("True", "true"),
        ("False", "false"),
        ("None", "nil"),
    ];

    for (wrong, right) in typo_fixes {
        if code.contains(wrong) {
            suggestions.push(json!({
                "type": "typo_fix",
                "description": format!("Replace '{}' with '{}'", wrong, right),
                "repaired_code": code.replace(wrong, right),
                "confidence": "high"
            }));
        }
    }

    if code.contains("let ") && !code.contains("let [") {
        suggestions.push(json!({
            "type": "syntax_fix",
            "description": "let requires bindings in square brackets: (let [x 1] ...)",
            "example": "(let [x 1 y 2] (+ x y))",
            "confidence": "medium"
        }));
    }

    let best_repair = if let Some(first) = suggestions.first() {
        first
            .get("repaired_code")
            .and_then(|v| v.as_str())
            .map(String::from)
    } else {
        None
    };

    let repair_valid = if let Some(ref repair) = best_repair {
        rtfs::parser::parse(repair).is_ok()
    } else {
        false
    };

    json!({
        "original_code": code,
        "suggestions": suggestions,
        "best_repair": best_repair,
        "repair_valid": repair_valid,
        "note": "Use rtfs_repair_with_llm for comprehensive LLM-based repairs using prompt templates."
    })
}

/// LLM-based repair using comprehensive prompt templates from assets/prompts/cognitive_engine/auto_repair/v1/
/// This provides better repairs than heuristics for complex syntax/semantic errors.
async fn repair_rtfs_code_with_llm(
    code: &str,
    error_message: Option<&str>,
    config: &ValidationConfig,
) -> serde_json::Value {
    use ccos::config::types::ValidationConfig as _;
    use ccos::synthesis::validation::{
        auto_repair_plan, repair_runtime_error_with_retry, ValidationError, ValidationErrorType,
    };

    // Convert error message to validation errors for the template
    let errors = if let Some(err) = error_message {
        vec![ValidationError {
            error_type: ValidationErrorType::Other,
            message: err.to_string(),
            location: None,
            suggested_fix: None,
        }]
    } else {
        // Try to compile and get actual errors
        match rtfs::parser::parse(code) {
            Ok(_) => vec![], // No parse errors
            Err(e) => vec![ValidationError {
                error_type: ValidationErrorType::Other,
                message: format!("{}", e),
                location: None,
                suggested_fix: None,
            }],
        }
    };

    if errors.is_empty() {
        return json!({
            "success": true,
            "message": "Code parses without errors",
            "original_code": code,
            "repaired_code": null
        });
    }

    // First try the runtime-repair loop with parse-feedback retries. This is a good default
    // when the caller provides an error message (including runtime errors), and it is more
    // robust than a single-shot template render.
    if let Some(err) = error_message {
        match repair_runtime_error_with_retry(code, err, config).await {
            Ok(Some(repaired)) => {
                let repair_valid = rtfs::parser::parse(&repaired).is_ok();
                return json!({
                    "success": true,
                    "original_code": code,
                    "repaired_code": repaired,
                    "repair_valid": repair_valid,
                    "method": "llm_runtime_retry",
                    "note": "Repaired using LLM runtime-repair loop with parse-feedback retries."
                });
            }
            Ok(None) => {
                // fall through to template approach
            }
            Err(e) => {
                // fall through to template approach but preserve error for debugging
                eprintln!("[ccos-mcp] runtime repair retry failed: {}", e);
            }
        }
    }

    match auto_repair_plan(code, &errors, 1, config).await {
        Ok(Some(repaired)) => {
            let repair_valid = rtfs::parser::parse(&repaired).is_ok();
            json!({
                "success": true,
                "original_code": code,
                "repaired_code": repaired,
                "repair_valid": repair_valid,
                "method": "llm_template",
                "note": "Repaired using LLM with comprehensive RTFS grammar hints and strategy templates."
            })
        }
        Ok(None) => {
            // LLM repair failed, fall back to heuristics
            let heuristic_result = repair_rtfs_code(code);
            json!({
                "success": false,
                "llm_failed": true,
                "fallback_to_heuristics": true,
                "heuristic_result": heuristic_result,
                "note": "LLM repair failed or not available. Using heuristic-based suggestions."
            })
        }
        Err(e) => {
            json!({
                "success": false,
                "error": e,
                "fallback_to_heuristics": true,
                "heuristic_result": repair_rtfs_code(code)
            })
        }
    }
}

/// Search for OpenAPI spec URLs for a given API name using web search
/// Returns a list of potential OpenAPI spec URLs found
async fn search_for_openapi_spec(api_name: &str) -> Vec<String> {
    use ccos::synthesis::runtime::web_search_discovery::WebSearchDiscovery;

    let mut results = Vec::new();

    // Check if web search is enabled
    if !ccos::discovery::registry_search::RegistrySearcher::is_web_search_enabled() {
        eprintln!("[CCOS] Web search disabled, skipping OpenAPI spec search");
        return results;
    }

    eprintln!("[CCOS] Searching for OpenAPI spec for: {}", api_name);

    // Use search_for_api_specs which includes OpenAPI spec searches
    let mut web_searcher = WebSearchDiscovery::new("auto".to_string());

    if let Ok(search_results) = web_searcher.search_for_api_specs(api_name).await {
        for result in search_results {
            let url_lower = result.url.to_lowercase();
            // Filter for likely OpenAPI spec URLs
            if url_lower.ends_with(".json")
                || url_lower.ends_with(".yaml")
                || url_lower.ends_with(".yml")
                || url_lower.contains("openapi")
                || url_lower.contains("swagger")
            {
                if !results.contains(&result.url) {
                    eprintln!("[CCOS] Found potential OpenAPI spec: {}", result.url);
                    results.push(result.url);
                }
            }
        }
    }

    results.into_iter().take(3).collect() // Return at most 3 URLs
}

/// Generate user-friendly suggestions when introspection fails
fn generate_introspection_failure_suggestions(api_name: &str, endpoint: &str) -> serde_json::Value {
    json!({
        "question": format!("Could not automatically introspect '{}'. Can you help?", api_name),
        "suggestions": [
            format!("Check if {} has a GitHub repo with an openapi.json or swagger.yaml file", api_name),
            format!("Look for 'API Reference', 'Swagger', or 'OpenAPI' links in their documentation"),
            format!("Search for '{} openapi spec' to find the specification URL", api_name),
            format!("If this is an MCP server, try: npx -y @{}/mcp-server", api_name.to_lowercase().replace(" ", "-")),
            format!("Provide the direct OpenAPI spec URL (ending in .json or .yaml)")
        ],
        "original_endpoint": endpoint
    })
}

/// Represents a sub-intent extracted from a goal for planning
#[derive(Debug, Clone)]
struct SubIntent {
    description: String,
    intent_type: String,
    depends_on: Vec<usize>,
}

/// Extract sub-intents from a goal using the discovery service's analysis
fn extract_sub_intents_from_goal(
    goal: &str,
    analysis: &ccos::discovery::llm_discovery::IntentAnalysis,
) -> Vec<SubIntent> {
    let mut sub_intents = Vec::new();

    // Strategy: Break goal into semantic units based on conjunctions and implied steps
    // This is a simplified decomposition - in production would use LLM

    // First, try to detect explicit conjunctions in the goal
    let goal_lower = goal.to_lowercase();
    let parts: Vec<&str> = if goal_lower.contains(" and ") {
        goal.split(" and ").collect()
    } else if goal_lower.contains(" then ") {
        goal.split(" then ").collect()
    } else if goal_lower.contains(", ") {
        goal.split(", ").collect()
    } else {
        vec![goal]
    };

    for (i, part) in parts.iter().enumerate() {
        let part_trimmed = part.trim();
        if part_trimmed.is_empty() {
            continue;
        }

        // Infer intent type from keywords
        let intent_type = infer_intent_type(part_trimmed);

        // Dependencies: each subsequent step depends on previous
        let depends_on = if i > 0 { vec![i - 1] } else { vec![] };

        sub_intents.push(SubIntent {
            description: part_trimmed.to_string(),
            intent_type,
            depends_on,
        });
    }

    // If no explicit decomposition found, create a single intent from analysis
    if sub_intents.is_empty() {
        sub_intents.push(SubIntent {
            description: goal.to_string(),
            intent_type: analysis.primary_action.clone(),
            depends_on: vec![],
        });
    }

    sub_intents
}

/// Infer the type of intent from the text
fn infer_intent_type(text: &str) -> String {
    let lower = text.to_lowercase();

    if lower.contains("get")
        || lower.contains("fetch")
        || lower.contains("retrieve")
        || lower.contains("find")
    {
        "retrieve".to_string()
    } else if lower.contains("send")
        || lower.contains("email")
        || lower.contains("notify")
        || lower.contains("alert")
    {
        "communicate".to_string()
    } else if lower.contains("create") || lower.contains("make") || lower.contains("generate") {
        "create".to_string()
    } else if lower.contains("update") || lower.contains("modify") || lower.contains("change") {
        "update".to_string()
    } else if lower.contains("delete") || lower.contains("remove") {
        "delete".to_string()
    } else if lower.contains("search") || lower.contains("query") || lower.contains("look") {
        "search".to_string()
    } else {
        "action".to_string()
    }
}

/// Infer an API search query from an intent description
fn infer_api_query_from_intent(intent: &str) -> String {
    let lower = intent.to_lowercase();

    // Extract key domain words and action words
    let domain_keywords: Vec<&str> = vec![
        "weather",
        "email",
        "calendar",
        "file",
        "database",
        "github",
        "slack",
        "twitter",
        "notification",
        "payment",
        "user",
        "auth",
        "api",
        "http",
        "message",
        "document",
        "image",
        "video",
        "audio",
        "data",
        "report",
    ];

    let mut found_domains: Vec<&str> = domain_keywords
        .iter()
        .filter(|kw| lower.contains(*kw))
        .copied()
        .collect();

    // If no specific domain found, extract key nouns
    if found_domains.is_empty() {
        // Simple heuristic: take meaningful words
        let words: Vec<&str> = intent
            .split_whitespace()
            .filter(|w| w.len() > 3)
            .take(3)
            .collect();
        return words.join(" ") + " API";
    }

    // Build query from found domains
    found_domains.truncate(2);
    found_domains.join(" ") + " API service"
}
