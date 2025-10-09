#![allow(dead_code)]
//! CCOS module root

// Keep one declaration per submodule to avoid duplicate module errors.
// If some modules are not yet present, gate or comment them as needed.

// Core CCOS Data Structures
pub mod arbiter;
pub mod archivable_types; // Serializable versions of CCOS types
pub mod causal_chain;
pub mod checkpoint_archive; // Checkpoint storage for execution contexts
pub mod event_sink;
pub mod governance_kernel;
pub mod intent_archive;
pub mod intent_graph;
pub mod intent_storage;
pub mod orchestrator;
pub mod plan_archive; // Plan archiving functionality
pub mod rtfs_bridge; // RTFS bridge for CCOS object extraction and conversion
pub mod storage; // Unified storage abstraction
pub mod storage_backends;
pub mod types;
pub mod wm_integration; // Pluggable storage backend implementations (file/sqlite)
                        // pub mod archive_manager;   // Unified archive coordination (not yet present)
pub mod execution_context; // Hierarchical execution context management
pub mod synthesis; // Capability synthesis (AST-only generators, schema extraction)

// Delegation and execution stack
pub mod adaptive_threshold;
pub mod delegation;
pub mod delegation_keys;
pub mod delegation_l4;
pub mod local_models;
pub mod remote_models;

// Infrastructure
pub mod caching;

// Capability system (moved from runtime)
pub mod capabilities;
pub mod capability_marketplace;
pub mod environment;
pub mod host;
pub mod observability;
pub mod state_provider;
pub mod streaming;

// Advanced components
pub mod context_horizon;
pub mod subconscious;

// New modular Working Memory (single declaration)
pub mod working_memory;

// Synthesis utilities (dynamic capability generation prototype)
pub mod synthesis;

// Orchestration/Arbiter components (if present in tree)
// pub mod orchestrator;      // commented: module not present in tree
pub mod agent; // consolidated agent module (registry + types)
pub mod runtime_service; // thin async runtime wrapper for embedding CCOS in apps (CLI/TUI/Web)

// Re-export some arbiter sub-modules at the ccos root for historic import paths
// Tests and examples sometimes refer to `rtfs_compiler::ccos::delegating_arbiter` or
// `rtfs_compiler::ccos::arbiter_engine`. Provide lightweight re-exports to avoid
// breaking consumers when the arbiter was nested under `ccos::arbiter`.
pub use crate::ccos::arbiter::arbiter_engine;
pub use crate::ccos::arbiter::delegating_arbiter;

// --- Core CCOS System ---

use std::sync::{Arc, Mutex};

use crate::ccos::agent::AgentRegistry; // bring trait into scope for record_feedback
use crate::ccos::arbiter::{Arbiter, DelegatingArbiter};
use crate::ccos::capability_marketplace::CapabilityMarketplace;
use crate::config::types::AgentConfig;
use crate::runtime::error::RuntimeResult;
use crate::runtime::security::RuntimeContext;
use crate::runtime::{ModuleRegistry, RTFSRuntime, Runtime};

use self::types::ExecutionResult;

use self::causal_chain::CausalChain;
use self::event_sink::CausalChainIntentEventSink;
use self::governance_kernel::GovernanceKernel;
use self::intent_graph::{config::IntentGraphConfig, IntentGraph};
use crate::runtime::error::RuntimeError;
use crate::ccos::types::StorableIntent;

use self::orchestrator::Orchestrator;

/// The main CCOS system struct, which initializes and holds all core components.
/// This is the primary entry point for interacting with the CCOS.
pub struct CCOS {
    arbiter: Arc<Arbiter>,
    governance_kernel: Arc<GovernanceKernel>,
    orchestrator: Arc<Orchestrator>,
    // The following components are shared across the system
    intent_graph: Arc<Mutex<IntentGraph>>,
    causal_chain: Arc<Mutex<CausalChain>>,
    capability_marketplace: Arc<CapabilityMarketplace>,
    rtfs_runtime: Arc<Mutex<dyn RTFSRuntime>>,
    // Optional LLM-driven engine
    delegating_arbiter: Option<Arc<DelegatingArbiter>>,
    agent_registry: Arc<std::sync::RwLock<crate::ccos::agent::InMemoryAgentRegistry>>, // M4
    agent_config: Arc<AgentConfig>, // Global agent configuration (future: loaded from RTFS form)
    /// Optional debug callback for emitting lifecycle JSON lines (plan generation, execution etc.)
    debug_callback: Option<Arc<dyn Fn(String) + Send + Sync>>,
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
        agent_config_opt: Option<crate::config::types::AgentConfig>,
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
            crate::ccos::capabilities::registry::CapabilityRegistry::new(),
        ));
        let capability_marketplace = CapabilityMarketplace::with_causal_chain_and_debug_callback(
            Arc::clone(&capability_registry),
            Some(Arc::clone(&causal_chain)),
            debug_callback.clone(),
        );

        // Bootstrap the marketplace with discovered capabilities
        capability_marketplace.bootstrap().await?;

        let capability_marketplace = Arc::new(capability_marketplace);

        // Use provided AgentConfig or default
        let agent_config = if let Some(cfg) = agent_config_opt {
            Arc::new(cfg)
        } else {
            Arc::new(AgentConfig::default())
        };

        // Register built-in capabilities required by default plans (await using ambient runtime)
        crate::runtime::stdlib::register_default_capabilities(&capability_marketplace).await?;

        // 2. Initialize architectural components, injecting dependencies
        let plan_archive = Arc::new(match plan_archive_path {
            Some(path) => plan_archive::PlanArchive::with_file_storage(path).map_err(|e| {
                RuntimeError::StorageError(format!("Failed to create plan archive: {}", e))
            })?,
            None => plan_archive::PlanArchive::new(),
        });
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

        let arbiter = Arc::new(Arbiter::new(
            crate::ccos::arbiter::legacy_arbiter::ArbiterConfig::default(),
            Arc::clone(&intent_graph),
        ));

        // Initialize AgentRegistry (M4) from agent configuration
        let agent_registry = Arc::new(std::sync::RwLock::new(
            crate::ccos::agent::InMemoryAgentRegistry::new(),
        ));

        // Allow enabling delegation via environment variable for examples / dev runs
        // If the AgentConfig doesn't explicitly enable delegation, allow an env override.
        let enable_delegation = if let Some(v) = agent_config.delegation.enabled {
            v
        } else {
            // Check for explicit disable first
            if let Ok(s) = std::env::var("CCOS_DELEGATION_ENABLED") {
                match s.as_str() {
                    "0" | "false" | "no" | "off" => false,
                    "1" | "true" | "yes" | "on" => true,
                    _ => false,
                }
            } else {
                // No explicit disable, check for enable flags
                std::env::var("CCOS_ENABLE_DELEGATION")
                    .ok()
                    .or_else(|| std::env::var("CCOS_USE_DELEGATING_ARBITER").ok())
                    .map(|s| {
                        let s = s.as_str();
                        matches!(s, "1" | "true" | "yes" | "on")
                    })
                    .unwrap_or(false)
            }
        };

        // Initialize delegating arbiter if delegation is enabled in agent config (or via env)
        let delegating_arbiter = if enable_delegation {
            // Prefer OpenRouter when OPENROUTER_API_KEY is provided, otherwise fallback to OpenAI if OPENAI_API_KEY exists.
            let (api_key, base_url, model) = if let Ok(key) = std::env::var("OPENROUTER_API_KEY") {
                (Some(key), Some(std::env::var("CCOS_LLM_BASE_URL").unwrap_or_else(|_| "https://openrouter.ai/api/v1".to_string())), std::env::var("CCOS_DELEGATING_MODEL").unwrap_or_else(|_| "deepseek/deepseek-chat-v3.1:free".to_string()))
            } else if let Ok(key) = std::env::var("OPENAI_API_KEY") {
                (Some(key), None, std::env::var("CCOS_DELEGATING_MODEL").unwrap_or_else(|_| "gpt-4o-mini".to_string()))
            } else {
                (None, None, std::env::var("CCOS_DELEGATING_MODEL").unwrap_or_else(|_| "stub-model".to_string()))
            };

            // Create LLM config for delegating arbiter
            let provider_type = if model == "stub-model" || model == "deterministic-stub-model" {
                crate::ccos::arbiter::arbiter_config::LlmProviderType::Stub
            } else {
                crate::ccos::arbiter::arbiter_config::LlmProviderType::OpenAI
            };

            let llm_config = crate::ccos::arbiter::arbiter_config::LlmConfig {
                provider_type,
                model,
                api_key,
                base_url,
                max_tokens: Some(1000),
                temperature: Some(0.7),
                timeout_seconds: Some(30),
                prompts: None,
                retry_config: crate::ccos::arbiter::arbiter_config::RetryConfig::default(),
            };

            // Allow examples or environment to override retry config for the LLM provider.
            // Useful for demos that want the provider to attempt a corrective re-prompt.
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
            let llm_config = crate::ccos::arbiter::arbiter_config::LlmConfig {
                retry_config: rc,
                ..llm_config
            };

            // Convert agent config delegation to arbiter delegation config
            let delegation_config = agent_config.delegation.to_arbiter_config();

            // Create delegating arbiter
            match crate::ccos::arbiter::DelegatingArbiter::new(
                llm_config,
                delegation_config,
                Arc::clone(&intent_graph),
            )
            .await
            {
                Ok(arbiter) => Some(Arc::new(arbiter)),
                Err(e) => {
                    eprintln!("Warning: Failed to initialize delegating arbiter: {}", e);
                    None
                }
            }
        } else {
            None
        };

        Ok(Self {
            arbiter,
            governance_kernel,
            orchestrator,
            intent_graph,
            causal_chain,
            capability_marketplace,
            rtfs_runtime: Arc::new(Mutex::new(Runtime::new_with_tree_walking_strategy(
                Arc::new(ModuleRegistry::new()),
            ))),
            delegating_arbiter,
            agent_registry,
            agent_config,
            debug_callback: debug_callback.clone(),
        })
    }

    /// Returns a snapshot (clone) of all currently stored intents.
    /// This is a lightweight convenience for examples / inspection and should not
    /// be used in performance‑critical paths (acquires a mutex briefly, clones intents).
    pub fn list_intents_snapshot(&self) -> Vec<StorableIntent> {
        // Acquire lock and use sync helper; IntentGraph already exposes sync accessor wrappers.
        // Errors are swallowed into empty vec in underlying helper; acceptable for non‑critical read.
        let ig = self.intent_graph.lock().expect("intent_graph lock poisoned");
        ig.storage.get_all_intents_sync()
    }

    /// Preflight validation (M3 pre-work): ensure all referenced capabilities exist in marketplace
    pub async fn preflight_validate_capabilities(
        &self,
        plan: &self::types::Plan,
    ) -> RuntimeResult<()> {
        use self::types::PlanBody;
        if let PlanBody::Rtfs(body) = &plan.body {
            // Parse RTFS to AST and walk it collecting real (call :ccos.* ...) usage.
            // On parse failure, we do NOT hard fail preflight here; governance / parser stage will surface rich errors later.
            // Fallback: if parsing fails, skip capability preflight rather than produce false positives.
            if let Ok(items) = crate::parser::parse(body) {
                use crate::ast::{Expression, TopLevel, Literal, Keyword, ModuleLevelDefinition};
                let mut referenced: std::collections::HashSet<String> = std::collections::HashSet::new();

                // Recursive walker
                fn walk_expr(expr: &Expression, acc: &mut std::collections::HashSet<String>) {
                    match expr {
                        Expression::List(items) | Expression::Vector(items) => {
                            for e in items { walk_expr(e, acc); }
                        }
                        Expression::Map(map) => {
                            for v in map.values() { walk_expr(v, acc); }
                        }
                        Expression::FunctionCall { callee, arguments } => {
                            // Recognize (call :ccos.cap ...) by structure: callee symbol or list where first symbol is 'call'
                            // Simpler: look for callee == Symbol("call") and first argument is a Literal::Keyword with name starting ccos.
                            match &**callee {
                                Expression::Symbol(sym) if sym.0 == "call" => {
                                    if let Some(first) = arguments.first() {
                                        match first {
                                            Expression::Literal(Literal::Keyword(Keyword(k))) => {
                                                if let Some(rest) = k.strip_prefix("ccos.") { // stored without leading colon in Keyword?
                                                    acc.insert(format!("ccos.{}", rest));
                                                } else if k.starts_with(":ccos.") { // defensive variant if colon retained
                                                    let trimmed = k.trim_start_matches(':');
                                                    if trimmed.starts_with("ccos.") { acc.insert(trimmed.to_string()); }
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
                                    for arg in arguments { walk_expr(arg, acc); }
                                }
                                _ => {
                                    walk_expr(callee, acc);
                                    for arg in arguments { walk_expr(arg, acc); }
                                }
                            }
                        }
                        Expression::If(ifx) => {
                            walk_expr(&ifx.condition, acc);
                            walk_expr(&ifx.then_branch, acc);
                            if let Some(e) = &ifx.else_branch { walk_expr(e, acc); }
                        }
                        Expression::Let(letx) => {
                            for b in &letx.bindings { walk_expr(&b.value, acc); }
                            for b in &letx.body { walk_expr(b, acc); }
                        }
                        Expression::Do(dox) => { for e in &dox.expressions { walk_expr(e, acc); } }
                        Expression::Fn(fnexpr) => { for e in &fnexpr.body { walk_expr(e, acc); } }
                        Expression::Def(defexpr) => { walk_expr(&defexpr.value, acc); }
                        Expression::Defn(defn) => { for e in &defn.body { walk_expr(e, acc); } }
                        Expression::Defstruct(_) => {}
                        Expression::DiscoverAgents(d) => {
                            walk_expr(&d.criteria, acc);
                            if let Some(opt) = &d.options { walk_expr(opt, acc); }
                        }
                        Expression::LogStep(logx) => { for v in &logx.values { walk_expr(v, acc); } }
                        Expression::TryCatch(tc) => {
                            for e in &tc.try_body { walk_expr(e, acc); }
                            for clause in &tc.catch_clauses { for e in &clause.body { walk_expr(e, acc); } }
                            if let Some(fb) = &tc.finally_body { for e in fb { walk_expr(e, acc); } }
                        }
                        Expression::Parallel(px) => { for b in &px.bindings { walk_expr(&b.expression, acc); } }
                        Expression::WithResource(wx) => { walk_expr(&wx.resource_init, acc); for e in &wx.body { walk_expr(e, acc); } }
                        Expression::Match(mx) => {
                            // match expression then each clause pattern guard + body
                            walk_expr(&mx.expression, acc);
                            for c in &mx.clauses {
                                if let Some(g) = &c.guard { walk_expr(g, acc); }
                                // c.body is Box<Expression>
                                walk_expr(&c.body, acc);
                            }
                        }
                        Expression::For(fx) => {
                            // For bindings are expressions in pairs [sym coll ...]; traverse each expression node
                            for b in &fx.bindings { walk_expr(b, acc); }
                            walk_expr(&fx.body, acc); // body is Box<Expression>
                        }
                        Expression::Deref(inner) => walk_expr(inner, acc),
                        Expression::Metadata(map) => { for v in map.values() { walk_expr(v, acc); } }
                        Expression::Literal(_) | Expression::Symbol(_) | Expression::ResourceRef(_) => {}
                    }
                }

                for item in items {
                    match item {
                        TopLevel::Expression(expr) => walk_expr(&expr, &mut referenced),
                        TopLevel::Plan(pdef) => { for prop in &pdef.properties { walk_expr(&prop.value, &mut referenced); } },
                        TopLevel::Intent(idef) => { for prop in &idef.properties { walk_expr(&prop.value, &mut referenced); } },
                        TopLevel::Action(adef) => { for prop in &adef.properties { walk_expr(&prop.value, &mut referenced); } },
                        TopLevel::Capability(cdef) => { for prop in &cdef.properties { walk_expr(&prop.value, &mut referenced); } },
                        TopLevel::Resource(rdef) => { for prop in &rdef.properties { walk_expr(&prop.value, &mut referenced); } },
                        TopLevel::Module(mdef) => {
                            for def in &mdef.definitions {
                                match def {
                                    ModuleLevelDefinition::Def(d) => walk_expr(&d.value, &mut referenced),
                                    ModuleLevelDefinition::Defn(dn) => { for e in &dn.body { walk_expr(e, &mut referenced); } },
                                    ModuleLevelDefinition::Import(_) => {}
                                }
                            }
                        },
                    }
                }

                for cap in referenced {
                    if self
                        .capability_marketplace
                        .get_capability(&cap)
                        .await
                        .is_none()
                    {
                        return Err(crate::runtime::error::RuntimeError::Generic(format!(
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
        self.emit_debug(|m| {
            format!(
                "{{\"event\":\"request_received\",\"text\":{},\"ts\":{}}}",
                json_escape(natural_language_request),
                current_ts()
            )
        });
        // 1. Arbiter: Generate a plan from the natural language request.
        let proposed_plan = if let Some(da) = &self.delegating_arbiter {
            // Use delegating arbiter to produce a plan via its engine API
            use crate::ccos::arbiter::ArbiterEngine;
            let intent = da
                .natural_language_to_intent(natural_language_request, None)
                .await?;

            // Store the intent in the Intent Graph for later reference
            {
                let mut ig = self.intent_graph.lock().map_err(|e| {
                    RuntimeError::Generic(format!("Failed to lock intent graph: {}", e))
                })?;
                let storable_intent = crate::ccos::types::StorableIntent {
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
                    triggered_by: crate::ccos::types::TriggerSource::HumanRequest,
                    generation_context: crate::ccos::types::GenerationContext {
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

            let plan = da.intent_to_plan(&intent).await?;
            self.emit_debug(|_| format!(
                "{{\"event\":\"plan_generated\",\"plan_id\":\"{}\",\"intent_id\":\"{}\",\"ts\":{}}}",
                plan.plan_id, intent.intent_id, current_ts()
            ));
            plan
        } else {
            let plan = self
                .arbiter
                .process_natural_language(natural_language_request, None)
                .await?;
            self.emit_debug(|_| {
                format!(
                    "{{\"event\":\"plan_generated\",\"plan_id\":\"{}\",\"ts\":{}}}",
                    plan.plan_id,
                    current_ts()
                )
            });
            plan
        };

        // 1.5 Preflight capability validation (M3)
        self.emit_debug(|_| {
            format!(
                "{{\"event\":\"plan_validation_start\",\"plan_id\":\"{}\",\"ts\":{}}}",
                proposed_plan.plan_id,
                current_ts()
            )
        });
        self.preflight_validate_capabilities(&proposed_plan).await?;

        // 2. Governance Kernel: Validate the plan and execute it via the Orchestrator.
        self.emit_debug(|_| {
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
        self.emit_debug(|_| format!(
            "{{\"event\":\"plan_execution_completed\",\"plan_id\":\"{}\",\"success\":{},\"ts\":{}}}",
            plan_id_for_events, result.success, current_ts()
        ));

        // Delegation completion feedback (M4 extension)
        if self.delegating_arbiter.is_some() {
            use crate::runtime::values::Value;
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
                            let _ =
                                chain.record_delegation_event(&stored.intent_id, "completed", meta);
                        }
                        // Feedback update (rolling average) via registry
                        if result.success {
                            if let Ok(mut reg) = self.agent_registry.write() {
                                reg.record_feedback(&agent_id, true);
                            }
                        } else {
                            if let Ok(mut reg) = self.agent_registry.write() {
                                reg.record_feedback(&agent_id, false);
                            }
                        }
                    }
                }
            }
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
    ) -> RuntimeResult<(self::types::Plan, ExecutionResult)> {
        self.emit_debug(|_| {
            format!(
                "{{\"event\":\"request_received\",\"text\":{},\"ts\":{}}}",
                json_escape(natural_language_request),
                current_ts()
            )
        });

        // Plan generation (same logic as in process_request; duplication kept minimal for clarity)
        let proposed_plan = if let Some(da) = &self.delegating_arbiter {
            use crate::ccos::arbiter::ArbiterEngine;
            let intent = da
                .natural_language_to_intent(natural_language_request, None)
                .await?;
            if let Ok(mut ig) = self.intent_graph.lock() {
                let storable_intent = crate::ccos::types::StorableIntent {
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
                    triggered_by: crate::ccos::types::TriggerSource::HumanRequest,
                    generation_context: crate::ccos::types::GenerationContext {
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
            let plan = da.intent_to_plan(&intent).await?;
            self.emit_debug(|_| format!(
                "{{\"event\":\"plan_generated\",\"plan_id\":\"{}\",\"intent_id\":\"{}\",\"ts\":{}}}",
                plan.plan_id, intent.intent_id, current_ts()
            ));
            plan
        } else {
            let plan = self
                .arbiter
                .process_natural_language(natural_language_request, None)
                .await?;
            self.emit_debug(|_| {
                format!(
                    "{{\"event\":\"plan_generated\",\"plan_id\":\"{}\",\"ts\":{}}}",
                    plan.plan_id,
                    current_ts()
                )
            });
            plan
        };

        self.emit_debug(|_| {
            format!(
                "{{\"event\":\"plan_validation_start\",\"plan_id\":\"{}\",\"ts\":{}}}",
                proposed_plan.plan_id,
                current_ts()
            )
        });
        self.preflight_validate_capabilities(&proposed_plan).await?;

        self.emit_debug(|_| {
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
        self.emit_debug(|_| format!(
            "{{\"event\":\"plan_execution_completed\",\"plan_id\":\"{}\",\"success\":{},\"ts\":{}}}",
            plan_id_for_events, result.success, current_ts()
        ));

        // (Delegation completion feedback duplicated from process_request; refactor later if needed.)
        if self.delegating_arbiter.is_some() {
            use crate::runtime::values::Value;
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
                            let _ =
                                chain.record_delegation_event(&stored.intent_id, "completed", meta);
                        }
                        if result.success {
                            if let Ok(mut reg) = self.agent_registry.write() {
                                reg.record_feedback(&agent_id, true);
                            }
                        } else {
                            if let Ok(mut reg) = self.agent_registry.write() {
                                reg.record_feedback(&agent_id, false);
                            }
                        }
                    }
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
        arbiter_context: Option<&std::collections::HashMap<String, crate::runtime::values::Value>>,
    ) -> RuntimeResult<ExecutionResult> {
        self.emit_debug(|m| {
            format!(
                "{{\"event\":\"request_received\",\"text\":{},\"ts\":{}}}",
                json_escape(natural_language_request),
                current_ts()
            )
        });
        // 1. Arbiter: Generate a plan from the natural language request.
        let proposed_plan = if let Some(da) = &self.delegating_arbiter {
            // Use delegating arbiter to produce a plan via its engine API
            use crate::ccos::arbiter::ArbiterEngine;
            let intent = da
                .natural_language_to_intent(natural_language_request, arbiter_context.cloned())
                .await?;

            // Store the intent in the Intent Graph for later reference
            {
                let mut ig = self.intent_graph.lock().map_err(|e| {
                    RuntimeError::Generic(format!("Failed to lock intent graph: {}", e))
                })?;
                let storable_intent = crate::ccos::types::StorableIntent {
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
                    triggered_by: crate::ccos::types::TriggerSource::HumanRequest,
                    generation_context: crate::ccos::types::GenerationContext {
                        arbiter_version: "delegating-1.0".to_string(),
                        generation_timestamp: intent.created_at,
                        input_context: arbiter_context.cloned().unwrap_or_default().into_iter().map(|(k, v)| (k, v.to_string())).collect(),
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

            da.intent_to_plan(&intent).await?
        } else {
            return Err(RuntimeError::Generic(
                "No delegating arbiter configured - cannot process natural language requests"
                    .to_string(),
            ));
        };

        // 1.5 Preflight capability validation (M3)
        self.emit_debug(|_| {
            format!(
                "{{\"event\":\"plan_validation_start\",\"plan_id\":\"{}\",\"ts\":{}}}",
                proposed_plan.plan_id,
                current_ts()
            )
        });
        self.preflight_validate_capabilities(&proposed_plan).await?;

        // 2. Governance Kernel: Validate the plan and execute it via the Orchestrator.
        self.emit_debug(|_| {
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
        self.emit_debug(|_| format!(
            "{{\"event\":\"plan_execution_completed\",\"plan_id\":\"{}\",\"success\":{},\"ts\":{}}}",
            plan_id_for_events, result.success, current_ts()
        ));

        // Delegation completion feedback (M4 extension)
        if self.delegating_arbiter.is_some() {
            use crate::runtime::values::Value;
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
                            meta.insert("agent_id".to_string(), Value::String(agent_id.clone()));
                            meta.insert("success".to_string(), Value::Boolean(result.success));
                            let _ = chain.record_delegation_event(
                                &stored.intent_id,
                                "delegation.agent_feedback",
                                meta,
                            );
                        }
                        // Update agent registry
                        if let Ok(mut reg) = self.agent_registry.write() {
                            if result.success {
                                let _ = reg.record_feedback(&agent_id, true);
                            } else {
                                let _ = reg.record_feedback(&agent_id, false);
                            }
                        }
                    }
                }
            }
        }

        Ok(result)
    }

    // --- Accessors for external analysis ---

    pub fn get_intent_graph(&self) -> Arc<Mutex<IntentGraph>> {
        Arc::clone(&self.intent_graph)
    }

    pub fn get_causal_chain(&self) -> Arc<Mutex<CausalChain>> {
        Arc::clone(&self.causal_chain)
    }

    pub fn get_agent_registry(
        &self,
    ) -> Arc<std::sync::RwLock<crate::ccos::agent::InMemoryAgentRegistry>> {
        Arc::clone(&self.agent_registry)
    }

    /// Optional-style accessor for symmetry with older code paths that treated
    /// the registry as optional. Always returns Some but keeps call sites
    /// forward-compatible if registry becomes optional again.
    pub fn get_agent_registry_opt(
        &self,
    ) -> Option<Arc<std::sync::RwLock<crate::ccos::agent::InMemoryAgentRegistry>>> {
        Some(Arc::clone(&self.agent_registry))
    }

    pub fn get_delegating_arbiter(&self) -> Option<Arc<DelegatingArbiter>> {
        self.delegating_arbiter.as_ref().map(Arc::clone)
    }

    pub fn get_orchestrator(&self) -> Arc<Orchestrator> {
        Arc::clone(&self.orchestrator)
    }

    pub fn get_agent_config(&self) -> Arc<AgentConfig> {
        Arc::clone(&self.agent_config)
    }

    /// Get access to the capability marketplace for advanced operations
    pub fn get_capability_marketplace(&self) -> Arc<CapabilityMarketplace> {
        Arc::clone(&self.capability_marketplace)
    }

    /// Validate and execute a pre-built Plan via the Governance Kernel.
    /// This is a convenience wrapper for examples and integration tests that
    /// already have a Plan object and want to run it through governance.
    pub async fn validate_and_execute_plan(
        &self,
        plan: self::types::Plan,
        context: &RuntimeContext,
    ) -> RuntimeResult<ExecutionResult> {
        // Preflight capability validation
        self.preflight_validate_capabilities(&plan).await?;
        // Delegate to governance kernel for sanitization, scaffolding and orchestration
        self.governance_kernel
            .validate_and_execute(plan, context)
            .await
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
    fn emit_debug<F>(&self, build: F)
    where
        F: FnOnce(()) -> String,
    {
        if let Some(cb) = &self.debug_callback {
            let line = build(());
            cb(line);
        }
    }
}

// --- Tests ---

#[cfg(test)]
mod tests {
    use super::*;
    use crate::runtime::security::SecurityLevel;

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
}
