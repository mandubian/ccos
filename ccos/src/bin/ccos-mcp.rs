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
pub fn save_session(
    session: &Session,
    sessions_dir: Option<&std::path::Path>,
    filename: Option<&str>,
) -> std::io::Result<std::path::PathBuf> {
    let dir = match sessions_dir {
        Some(d) => d.to_path_buf(),
        None => ccos::utils::fs::get_configured_sessions_path(),
    };
    
    // Create sessions directory if it doesn't exist
    std::fs::create_dir_all(&dir)?;
    
    // Generate filename: use override if provided, otherwise slugify goal
    let actual_filename = match filename {
        Some(f) => {
            if f.ends_with(".rtfs") {
                f.to_string()
            } else {
                format!("{}.rtfs", f)
            }
        }
        None => {
            let goal_slug = slugify_goal(&session.goal);
            format!("{}_{}.rtfs", session.id, goal_slug)
        }
    };
    
    let filepath = dir.join(&actual_filename);
    
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
                    eprintln!("[ccos-mcp] Setting workspace root to: {}", workspace_root.display());
                }
                ccos::utils::fs::set_workspace_root(workspace_root.to_path_buf());
            } else {
                // Config is at top level, use parent
                ccos::utils::fs::set_workspace_root(config_parent.to_path_buf());
            }
        }
    }

    // Populate marketplace with approved capabilities
    let _ = marketplace.import_capabilities_from_rtfs_dir_recursive(ccos::utils::fs::get_workspace_root().join("capabilities/servers/approved")).await;
    let _ = marketplace.import_capabilities_from_rtfs_dir_recursive(ccos::utils::fs::get_workspace_root().join("capabilities/core")).await;

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
        marketplace.clone(),
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
            run_http_transport_with_approvals(server, config, Some(approval_queue), Some(marketplace.clone())).await?;
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

    // ccos_suggest_apis - Pure LLM suggestions for external APIs (no auto-approval)
    // Returns suggestions for user selection, then user can approve and introspect
    let ds_suggest = discovery_service.clone();
    server.register_tool(
        "ccos_suggest_apis",
        "Ask LLM to suggest well-known APIs for a given goal. Returns suggestions without auto-queueing approvals - agent should present options to user, then call ccos_introspect_server on selected API.",
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
                        "Present these options to the user",
                        "When user selects one, call ccos_introspect_server with the docs_url (NOT the endpoint)",
                        "After introspection, capabilities will be queued for approval"
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

                // Step 1: Search local capabilities
                let all_caps = mp.list_capabilities().await;
                let mut scored_results: Vec<_> = all_caps
                    .into_iter()
                    .map(|c| {
                        let score = calculate_match_score(query, &c.description, &c.id);
                        (c, score)
                    })
                    .filter(|(_, score)| *score >= min_score)
                    .collect();

                // Sort by score descending
                scored_results.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));

                let local_results: Vec<_> = scored_results.iter().take(5).map(|(c, score)| {
                    json!({
                        "id": c.id,
                        "name": c.name, 
                        "description": c.description,
                        "match_score": score,
                        "source": "local"
                    })
                }).collect();

                // If we found local results, return them
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
                        "input_schema": cap.input_schema,
                        "inputs_template": inputs,
                        "match_score": *score,
                        "how_to_call": format!(
                            "Call ccos_execute_capability with capability_id=\"{}\" and inputs={{...}}",
                            cap.id
                        )
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

                // Build explicit next action guidance for LLM
                let first_step_id = execution_steps.first()
                    .and_then(|s| s.get("capability_id"))
                    .and_then(|v| v.as_str())
                    .unwrap_or("unknown");

                let next_action = if ready_to_execute {
                    json!({
                        "tool": "ccos_execute_capability",
                        "capability_id": first_step_id,
                        "instruction": format!(
                            "Execute step 1 by calling ccos_execute_capability with capability_id='{}'. Check the input_schema in step 1 to know which parameters are required.",
                            first_step_id
                        )
                    })
                } else {
                    json!({
                        "tool": "ccos_discover_capabilities",
                        "instruction": "No suitable capabilities found. Search for alternatives."
                    })
                };

                Ok(json!({
                    "success": true,
                    "goal": goal,
                    "feasibility": feasibility,
                    "ready_to_execute": ready_to_execute,
                    
                    // EXPLICIT GUIDANCE FOR LLM
                    "important": "The capabilities below are CCOS tools you can execute NOW using ccos_execute_capability. Do NOT search for other tools - use these capability_id values directly.",
                    "next_action": next_action,
                    
                    "execution_plan": {
                        "steps": execution_steps,
                        "step_count": execution_steps.len(),
                        "workflow": "For each step: call ccos_execute_capability with the capability_id and inputs based on input_schema"
                    },
                    
                    "analysis": {
                        "primary_action": analysis.primary_action,
                        "target_object": analysis.target_object,
                        "domain_keywords": analysis.domain_keywords,
                        "confidence": analysis.confidence
                    },
                    
                    "execution_instructions": [
                        "1. Call ccos_execute_capability with the first step's capability_id",
                        format!("2. Use capability_id='{}' and build inputs from input_schema", first_step_id),
                        "3. A session will be auto-created - note the session_id in the response",
                        "4. For subsequent steps, include session_id to build a complete RTFS plan",
                        "5. After all steps complete, call ccos_get_session_plan to get the full RTFS code"
                    ],
                    
                    "session_note": "Sessions track your execution and build an RTFS plan. Either: (A) let sessions auto-create, or (B) call ccos_start_session first for explicit control. Always pass session_id to subsequent steps."
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
                    let auto_session = Session::new(&format!("Auto-session for {}", capability_id));
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
                    "A session was auto-created for this execution. Use the returned session_id in subsequent ccos_execute_capability calls to build a multi-step RTFS plan. Call ccos_get_session_plan when done to get the complete RTFS code."
                } else {
                    "Step recorded to session. Continue with next step or call ccos_get_session_plan to get the complete RTFS code."
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
                        "To get full plan: call ccos_get_session_plan with session_id"
                    ]
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
            let _aq = aq_check.clone();
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
    let ds_introspect = discovery_service.clone(); // For HTML docs fallback
    server.register_tool(
        "ccos_introspect_server",
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
            let mp = mp_introspect.clone();
            let ds = ds_introspect.clone();
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

                // Detect if this looks like an OpenAPI spec URL
                let is_openapi_url = endpoint.ends_with(".json") 
                    || endpoint.ends_with(".yaml") 
                    || endpoint.ends_with(".yml")
                    || endpoint.contains("swagger")
                    || endpoint.contains("openapi");

                // Create server config for introspection
                let server_name = name.clone().unwrap_or_else(|| {
                    // Extract name from endpoint - try to get domain or last path segment
                    if let Ok(url) = url::Url::parse(endpoint) {
                        url.host_str()
                            .map(|h| h.replace(".", "_"))
                            .unwrap_or_else(|| endpoint.split('/').last().unwrap_or("unknown").to_string())
                    } else {
                        endpoint.split('/').last().unwrap_or("unknown").to_string()
                    }
                });

                // If it looks like an OpenAPI spec, use IntrospectionService
                if is_openapi_url {
                    eprintln!("[CCOS] Detected OpenAPI spec URL, using IntrospectionService...");
                    
                    let introspection_service = ccos::ops::introspection_service::IntrospectionService::new();
                    
                    match introspection_service.introspect_openapi(endpoint, &server_name).await {
                        Ok(result) if result.success => {
                            let api_result = result.api_result.as_ref().unwrap();
                            eprintln!("[CCOS] IntrospectionService found {} endpoints", api_result.endpoints.len());
                            
                            // Create approval request
                            let approval_id = match introspection_service.create_approval_request(
                                &result,
                                endpoint,
                                &aq,
                                24 * 7, // 7 days
                            ).await {
                                Ok(id) => id,
                                Err(e) => {
                                    return Ok(json!({
                                        "success": false,
                                        "error": format!("Failed to create approval request: {}", e)
                                    }));
                                }
                            };

                            // Generate RTFS files
                            let workspace_root = ccos::utils::fs::get_workspace_root();
                            let server_id = ccos::utils::fs::sanitize_filename(&server_name);
                            let pending_dir = workspace_root.join("capabilities/servers/pending").join(&server_id);
                            
                            match introspection_service.generate_rtfs_files(&result, &pending_dir, endpoint) {
                                Ok(gen_result) => {
                                    eprintln!("[CCOS] Saved {} RTFS capabilities to: {}", 
                                        gen_result.capability_files.len(), 
                                        gen_result.output_dir.display());
                                    
                                    // Return detailed discovery result
                                    let endpoint_previews: Vec<_> = api_result.endpoints.iter().take(10).map(|ep| {
                                        json!({
                                            "id": ep.endpoint_id,
                                            "name": ep.name,
                                            "method": ep.method,
                                            "path": ep.path,
                                            "description": ep.description
                                        })
                                    }).collect();

                                    return Ok(json!({
                                        "success": true,
                                        "approval_id": approval_id,
                                        "server_name": server_name,
                                        "source": "openapi",
                                        "api_title": api_result.api_title,
                                        "api_version": api_result.api_version,
                                        "base_url": api_result.base_url,
                                        "discovered_endpoints_count": api_result.endpoints.len(),
                                        "capability_files_count": gen_result.capability_files.len(),
                                        "endpoint_previews": endpoint_previews,
                                        "capabilities_path": gen_result.output_dir.to_string_lossy(),
                                        "auth_type": api_result.auth_requirements.auth_type,
                                        "agent_guidance": format!(
                                            "OpenAPI spec '{}' introspected with {} endpoints. \
                                             {} RTFS capability files (with schemas) + server.json saved to {}. \
                                             Approval required before use. Once approved, call ccos_register_server with approval_id: '{}'",
                                            api_result.api_title, api_result.endpoints.len(), 
                                            gen_result.capability_files.len(), 
                                            gen_result.output_dir.display(), 
                                            approval_id
                                        )
                                    }));
                                }
                                Err(e) => {
                                    return Ok(json!({
                                        "success": true,
                                        "approval_id": approval_id,
                                        "server_name": server_name,
                                        "source": "openapi",
                                        "warning": format!("Failed to generate RTFS files: {}", e)
                                    }));
                                }
                            }
                        }
                        Ok(result) => {
                            eprintln!("[CCOS] IntrospectionService failed: {:?}, falling back to MCP...", result.error);
                            // Fall through to MCP introspection below
                        }
                        Err(e) => {
                            eprintln!("[CCOS] IntrospectionService error: {}, falling back to MCP...", e);
                            // Fall through to MCP introspection below
                        }
                    }
                }

                // Try to introspect as MCP server
                let config = MCPServerConfig {
                    name: server_name.clone(),
                    endpoint: endpoint.to_string(),
                    auth_token: auth_env_var.as_ref().and_then(|var| std::env::var(var).ok()),
                    timeout_seconds: 30,
                    protocol_version: "2024-11-05".to_string(),
                };

                let mut discovery_source = "mcp";
                let discovery_result = match MCPDiscoveryProvider::new_with_rtfs_host_factory(
                    config.clone(),
                    mp.get_rtfs_host_factory(),
                ) {
                    Ok(provider) => {
                        match provider.discover_tools().await {
                            Ok(caps) => Some(caps),

                            Err(mcp_err) => {
                                // MCP failed - try HTML docs fallback
                                eprintln!("[CCOS] MCP introspection failed: {}, trying HTML docs fallback...", mcp_err);
                                discovery_source = "html_docs";
                                
                                // Use LLM to parse documentation page
                                match ds.search_external_apis(&server_name, Some(endpoint)).await {
                                    Ok(apis) if !apis.is_empty() => {
                                        // Convert ExternalApiResult to DiscoveredMCPTool-like info
                                        // For now, return info about the discovered API
                                        return Ok(json!({
                                            "success": true,
                                            "source": "html_docs",
                                            "discovered_apis": apis.iter().map(|api| json!({
                                                "name": api.name,
                                                "endpoint": api.endpoint,
                                                "description": api.description,
                                                "auth_env_var": api.auth_env_var
                                            })).collect::<Vec<_>>(),
                                            "note": "Discovered API info from documentation. To create detailed capabilities, provide an OpenAPI spec URL or approve this server and manually add capabilities.",
                                            "next_step": "If this looks correct, you can use ccos_execute_capability to call these endpoints directly, or create an approval request manually."
                                        }));
                                    }
                                    Ok(_) => {
                                        return Ok(json!({
                                            "success": false,
                                            "source": "html_docs",
                                            "error": format!("Could not extract API information from documentation at {}", endpoint),
                                            "hint": "Try providing an OpenAPI spec URL or a more detailed API reference page"
                                        }));
                                    }
                                    Err(doc_err) => {
                                        return Ok(json!({
                                            "success": false,
                                            "error": format!("MCP introspection failed: {}. HTML docs fallback also failed: {}", mcp_err, doc_err),
                                            "hint": "Check if the URL is accessible and contains API documentation"
                                        }));
                                    }
                                }
                            }
                        }
                    }
                    Err(e) => {
                        // Provider creation failed - also try HTML docs fallback
                        eprintln!("[CCOS] MCP provider creation failed: {}, trying HTML docs fallback...", e);
                        discovery_source = "html_docs";
                        
                        match ds.search_external_apis(&server_name, Some(endpoint)).await {
                            Ok(apis) if !apis.is_empty() => {
                                return Ok(json!({
                                    "success": true,
                                    "source": "html_docs",
                                    "discovered_apis": apis.iter().map(|api| json!({
                                        "name": api.name,
                                        "endpoint": api.endpoint,
                                        "description": api.description,
                                        "auth_env_var": api.auth_env_var
                                    })).collect::<Vec<_>>(),
                                    "note": "Discovered API info from documentation.",
                                    "next_step": "To use this API, you may need to manually configure capabilities."
                                }));
                            }
                            _ => {
                                return Ok(json!({
                                    "success": false,
                                    "error": format!("Failed to create discovery provider: {}. HTML docs fallback found no APIs.", e)
                                }));
                            }
                        }
                    }
                };

                let capabilities = discovery_result.unwrap_or_default();

                // Create ServerInfo
                let server_info = ccos::approval::queue::ServerInfo {
                    name: server_name.clone(),
                    endpoint: endpoint.to_string(),
                    description: Some(format!("Discovered {} capabilities via {}", capabilities.len(), discovery_source)),
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
                        reasons: vec![format!("Dynamically discovered server via {}", discovery_source)],
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
                    "source": discovery_source,
                    "discovered_tools_count": capabilities.len(),
                    "tool_previews": tool_previews,
                    "agent_guidance": format!(
                        "Server '{}' discovered with {} tools via {}. Approval required before tools can be used. \
                         Once approved, call ccos_register_server with approval_id: '{}'",
                        server_name, capabilities.len(), discovery_source, approval_id
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

                // NEW: Check if this is an OpenAPI server with already-generated RTFS files in the approved directory
                let server_id = ccos::utils::fs::sanitize_filename(&server_name);
                let workspace_root = ccos::utils::fs::get_workspace_root();
                let approved_dir = workspace_root.join("capabilities/servers/approved").join(&server_id);
                
                if approved_dir.exists() {
                     eprintln!("[ccos-mcp] Dynamically loading approved RTFS capabilities for {}", server_name);
                     
                     match mp.import_capabilities_from_rtfs_dir_recursive(&approved_dir).await {
                         Ok(loaded_count) => {
                             return Ok(json!({
                                "success": true,
                                "server_name": server_name,
                                "registered_count": loaded_count,
                                "total_discovered": loaded_count,
                                "agent_guidance": format!(
                                    "Successfully loaded {} RTFS capabilities from '{}'. They are now available via ccos_search_tools and ccos_execute_capability.",
                                    loaded_count, server_name
                                )
                            }));
                         }
                         Err(e) => {
                             return Ok(json!({
                                "success": false,
                                "error": format!("Failed to load capabilities: {}", e)
                            }));
                         }
                     }
                }

                // Fallback: Original MCP discovery for native MCP servers
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
                let show_ast = params.get("show_ast").and_then(|v| v.as_bool()).unwrap_or(false);

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
                    Err(parse_error) => {
                        Ok(json!({
                            "success": false,
                            "error": {
                                "message": format!("{:?}", parse_error)
                            },
                            "code_preview": code.chars().take(100).collect::<String>()
                        }))
                    }
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

    // rtfs_repair - Suggest repairs for broken RTFS code using heuristics
    server.register_tool(
        "rtfs_repair",
        "Suggest repairs for broken RTFS code",
        json!({
            "type": "object",
            "properties": {
                "code": { "type": "string", "description": "Broken RTFS code" },
                "error_message": { "type": "string", "description": "Optional error message from compilation" }
            },
            "required": ["code"]
        }),
        Box::new(move |params| {
            Box::pin(async move {
                let code = params.get("code").and_then(|v| v.as_str()).unwrap_or("");
                let _error = params.get("error_message").and_then(|v| v.as_str());

                let suggestions = repair_rtfs_code(code);

                Ok(json!(suggestions))
            })
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
            for cat in ["basic", "bindings", "control_flow", "functions", "capabilities", "types"] {
                all.extend(get_samples_for_category(cat));
            }
            all
        }
    }
}

/// Explain RTFS error messages in plain English
fn explain_rtfs_error(error: &str, code: Option<&str>) -> serde_json::Value {
    let error_lower = error.to_lowercase();

    let (explanation, common_causes, fix_suggestions) = if error_lower.contains("expected") && error_lower.contains("expression") {
        (
            "The parser expected an expression but found something else.",
            vec![
                "Unbalanced parentheses - missing opening or closing paren",
                "Empty list without proper content",
                "Incomplete expression at end of input"
            ],
            vec![
                "Check that all ( have matching )",
                "Ensure lists have at least an operator: (+ 1 2) not ()",
                "Complete any unfinished expressions"
            ]
        )
    } else if error_lower.contains("unexpected") && error_lower.contains("token") {
        (
            "The parser encountered a token it didn't expect in this position.",
            vec![
                "Wrong syntax for the construct being used",
                "Missing required elements",
                "Extra tokens that don't belong"
            ],
            vec![
                "Review the syntax for the form you're using",
                "Check for typos in keywords",
                "Ensure proper ordering of elements"
            ]
        )
    } else if error_lower.contains("unbalanced") || error_lower.contains("unclosed") {
        (
            "There are mismatched brackets in the code.",
            vec![
                "Missing closing ) ] or }",
                "Extra opening ( [ or {",
                "Brackets of wrong type used"
            ],
            vec![
                "Count opening and closing brackets",
                "Use an editor with bracket matching",
                "Remember: () for calls, [] for vectors/bindings, {} for maps"
            ]
        )
    } else if error_lower.contains("undefined") || error_lower.contains("not found") {
        (
            "A symbol or function was referenced but not defined.",
            vec![
                "Typo in function or variable name",
                "Using a variable before it's defined",
                "Missing import or require"
            ],
            vec![
                "Check spelling of the symbol",
                "Ensure definitions come before usage",
                "Use :keys in let bindings for map destructuring"
            ]
        )
    } else {
        (
            "This error indicates a problem with the RTFS code.",
            vec!["Syntax error", "Semantic error"],
            vec![
                "Review the code structure",
                "Compare with working examples",
                "Use rtfs_get_samples to see correct syntax"
            ]
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
            issues.push(format!("Paren mismatch: {} '(' vs {} ')'", open_parens, close_parens));
        }
        if open_brackets != close_brackets {
            issues.push(format!("Bracket mismatch: {} '[' vs {} ']'", open_brackets, close_brackets));
        }
        if open_braces != close_braces {
            issues.push(format!("Brace mismatch: {} '{{' vs {} '}}'", open_braces, close_braces));
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

    let open_parens = code.matches('(').count();
    let close_parens = code.matches(')').count();

    if open_parens > close_parens {
        let missing = open_parens - close_parens;
        let repaired = format!("{}{}", code, ")".repeat(missing));
        suggestions.push(json!({
            "type": "bracket_fix",
            "description": format!("Add {} missing closing parenthesis", missing),
            "repaired_code": repaired,
            "confidence": "high"
        }));
    } else if close_parens > open_parens {
        let extra = close_parens - open_parens;
        suggestions.push(json!({
            "type": "bracket_fix",
            "description": format!("Remove {} extra closing parenthesis or add opening", extra),
            "confidence": "medium"
        }));
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
        first.get("repaired_code").and_then(|v| v.as_str()).map(String::from)
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
        "note": "Suggestions are heuristic-based. Review before using."
    })
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
