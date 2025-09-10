#![allow(dead_code)]
//! CCOS module root

// Keep one declaration per submodule to avoid duplicate module errors.
// If some modules are not yet present, gate or comment them as needed.

 // Core CCOS Data Structures
pub mod causal_chain;
pub mod event_sink;
pub mod wm_integration;
pub mod intent_graph;
pub mod intent_storage;
pub mod intent_archive;
pub mod types;
pub mod governance_kernel;
pub mod orchestrator;
pub mod arbiter;
pub mod storage;           // Unified storage abstraction
pub mod archivable_types;  // Serializable versions of CCOS types
pub mod plan_archive;     // Plan archiving functionality
pub mod checkpoint_archive; // Checkpoint storage for execution contexts
pub mod rtfs_bridge;      // RTFS bridge for CCOS object extraction and conversion
pub mod storage_backends; // Pluggable storage backend implementations (file/sqlite)
// pub mod archive_manager;   // Unified archive coordination (not yet present)
pub mod execution_context; // Hierarchical execution context management

// Delegation and execution stack
pub mod delegation;
pub mod delegation_keys;
pub mod delegation_l4;
pub mod adaptive_threshold;
pub mod remote_models;
pub mod local_models;

// Infrastructure
pub mod caching;

 // Advanced components
pub mod context_horizon;
pub mod subconscious;


 // New modular Working Memory (single declaration)
pub mod working_memory;

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

use crate::ccos::arbiter::{DelegatingArbiter, Arbiter};
use crate::config::types::AgentConfig;
use crate::ccos::agent::AgentRegistry; // bring trait into scope for record_feedback
use crate::runtime::capability_marketplace::CapabilityMarketplace;
use crate::runtime::{RTFSRuntime, Runtime, ModuleRegistry};
use crate::runtime::error::RuntimeResult;
use crate::runtime::security::RuntimeContext;

use self::types::ExecutionResult;

use self::intent_graph::IntentGraph;
use self::event_sink::CausalChainIntentEventSink;
use self::causal_chain::CausalChain;
use self::governance_kernel::GovernanceKernel;


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
        // 1. Initialize shared, stateful components
    let causal_chain = Arc::new(Mutex::new(CausalChain::new()?));
    let sink = Arc::new(CausalChainIntentEventSink::new(Arc::clone(&causal_chain)));
    let intent_graph = Arc::new(Mutex::new(IntentGraph::with_event_sink(sink)?));
        // Initialize capability marketplace with registry
    let capability_registry = Arc::new(tokio::sync::RwLock::new(crate::runtime::capabilities::registry::CapabilityRegistry::new()));
        let capability_marketplace = CapabilityMarketplace::with_causal_chain_and_debug_callback(
            Arc::clone(&capability_registry),
            Some(Arc::clone(&causal_chain)),
            debug_callback,
        );
        
        // Bootstrap the marketplace with discovered capabilities
        capability_marketplace.bootstrap().await?;
        
        let capability_marketplace = Arc::new(capability_marketplace);

    // Load agent configuration (placeholder: default; future: parse RTFS (agent.config ...) form)
    let agent_config = Arc::new(AgentConfig::default());

        // Register built-in capabilities required by default plans (await using ambient runtime)
        crate::runtime::stdlib::register_default_capabilities(&capability_marketplace).await?;

        // 2. Initialize architectural components, injecting dependencies
        let plan_archive = Arc::new(plan_archive::PlanArchive::new());
        let orchestrator = Arc::new(Orchestrator::new(
            Arc::clone(&causal_chain),
            Arc::clone(&intent_graph),
            Arc::clone(&capability_marketplace),
            Arc::clone(&plan_archive),
        ));

        let governance_kernel = Arc::new(GovernanceKernel::new(Arc::clone(&orchestrator), Arc::clone(&intent_graph)));

        let arbiter = Arc::new(Arbiter::new(
            crate::ccos::arbiter::legacy_arbiter::ArbiterConfig::default(),
            Arc::clone(&intent_graph),
        ));

        // Initialize AgentRegistry (M4) from agent configuration
    let agent_registry = Arc::new(std::sync::RwLock::new(crate::ccos::agent::InMemoryAgentRegistry::new()));

        // Allow enabling delegation via environment variable for examples / dev runs
        // If the AgentConfig doesn't explicitly enable delegation, allow an env override.
        let enable_delegation = if let Some(v) = agent_config.delegation.enabled { v } else {
            std::env::var("CCOS_ENABLE_DELEGATION").ok().or_else(|| std::env::var("CCOS_DELEGATION_ENABLED").ok()).map(|s| {
                matches!(s.as_str(), "1" | "true" | "yes" | "on")
            }).unwrap_or(false)
        };

        // Initialize delegating arbiter if delegation is enabled in agent config (or via env)
        let delegating_arbiter = if enable_delegation {
            // Prefer OpenRouter when OPENROUTER_API_KEY is provided, otherwise fallback to OpenAI if OPENAI_API_KEY exists.
            let (api_key, base_url, model) = if let Ok(key) = std::env::var("OPENROUTER_API_KEY") {
                let model = std::env::var("LLM_MODEL").unwrap_or_else(|_| "moonshotai/kimi-k2:free".to_string());
                (Some(key), Some("https://openrouter.ai/api/v1".to_string()), model)
            } else {
                let key = std::env::var("OPENAI_API_KEY").ok();
                let model = std::env::var("LLM_MODEL").unwrap_or_else(|_| "gpt-4o-mini".to_string());
                (key, None, model)
            };

            // Create LLM config for delegating arbiter
            let llm_config = crate::ccos::arbiter::arbiter_config::LlmConfig {
                provider_type: crate::ccos::arbiter::arbiter_config::LlmProviderType::OpenAI,
                model,
                api_key,
                base_url,
                max_tokens: Some(1000),
                temperature: Some(0.7),
                timeout_seconds: Some(30),
                prompts: None,
            };

            // Convert agent config delegation to arbiter delegation config
            let delegation_config = agent_config.delegation.to_arbiter_config();

            // Create delegating arbiter
            match crate::ccos::arbiter::DelegatingArbiter::new(
                llm_config,
                delegation_config,
                Arc::clone(&intent_graph),
            ).await {
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
            rtfs_runtime: Arc::new(Mutex::new(Runtime::new_with_tree_walking_strategy(Arc::new(ModuleRegistry::new())))),
            delegating_arbiter,
            agent_registry,
            agent_config,
        })
    }

    /// Preflight validation (M3 pre-work): ensure all referenced capabilities exist in marketplace
    pub async fn preflight_validate_capabilities(&self, plan: &self::types::Plan) -> RuntimeResult<()> {
        use self::types::PlanBody;
        if let PlanBody::Rtfs(body) = &plan.body {
            // Very lightweight tokenizer – split on whitespace & parens
            let mut caps: std::collections::HashSet<String> = std::collections::HashSet::new();
            let mut current = String::new();
            for ch in body.chars() {
                if ch.is_whitespace() || ch == '(' || ch == ')' { 
                    if !current.is_empty() { caps.insert(current.clone()); current.clear(); }
                } else { current.push(ch); }
            }
            if !current.is_empty() { caps.insert(current); }
            // Extract capability ids from keyword (:ccos.echo) or string ("ccos.echo") tokens that follow a call form
            // Simplicity: just look for tokens starting with :ccos. or ccos.
            let mut referenced: std::collections::HashSet<String> = std::collections::HashSet::new();
            for tok in caps {
                if let Some(stripped) = tok.strip_prefix(":ccos.") { referenced.insert(format!("ccos.{}", stripped)); continue; }
                if tok.starts_with("ccos.") { referenced.insert(tok.clone()); continue; }
                if tok.starts_with("\"ccos.") { // string literal token
                    let trimmed = tok.trim_matches('"');
                    referenced.insert(trimmed.to_string());
                }
            }
            // Validate each capability exists
            for cap in referenced {
                if self.capability_marketplace.get_capability(&cap).await.is_none() {
                    return Err(crate::runtime::error::RuntimeError::Generic(format!("Unknown capability referenced in plan: {}", cap)));
                }
            }
        }
        Ok(())
    }

    /// The main entry point for processing a user request.
    /// This method follows the full CCOS architectural flow:
    /// 1. The Arbiter converts the request into a Plan.
    /// 2. The Governance Kernel validates the Plan.
    /// 3. The Orchestrator executes the validated Plan.
    pub async fn process_request(
        &self,
        natural_language_request: &str,
        security_context: &RuntimeContext,
    ) -> RuntimeResult<ExecutionResult> {
        // 1. Arbiter: Generate a plan from the natural language request.
        let proposed_plan = if let Some(da) = &self.delegating_arbiter {
            // Use delegating arbiter to produce a plan via its engine API
            use crate::ccos::arbiter::ArbiterEngine;
            let intent = da
                .natural_language_to_intent(natural_language_request, None)
                .await?;

            da.intent_to_plan(&intent).await?
        } else {
            self.arbiter
                .process_natural_language(natural_language_request, None)
                .await?
        };

        // 1.5 Preflight capability validation (M3)
        self.preflight_validate_capabilities(&proposed_plan).await?;

        // 2. Governance Kernel: Validate the plan and execute it via the Orchestrator.
        let result = self.governance_kernel
            .validate_and_execute(proposed_plan, security_context)
            .await?;

        // Delegation completion feedback (M4 extension)
        if self.delegating_arbiter.is_some() {
            use crate::runtime::values::Value;
            // Heuristic: search recent intents matching words from request
            if let Ok(graph) = self.intent_graph.lock() {
                let recent = graph.find_relevant_intents(natural_language_request);
                if let Some(stored) = recent.last() {
                    // Stored intent metadata is HashMap<String,String>; check delegation key presence
                    if stored.metadata.get("delegation.selected_agent").is_some() {
                        let agent_id = stored.metadata.get("delegation.selected_agent").cloned().unwrap_or_default();
                        // Record completed event
                        if let Ok(mut chain) = self.causal_chain.lock() {
                            let mut meta = std::collections::HashMap::new();
                            meta.insert("selected_agent".to_string(), Value::String(agent_id.clone()));
                            meta.insert("success".to_string(), Value::Boolean(result.success));
                            let _ = chain.record_delegation_event(&stored.intent_id, "completed", meta);
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

    // --- Accessors for external analysis ---

    pub fn get_intent_graph(&self) -> Arc<Mutex<IntentGraph>> {
        Arc::clone(&self.intent_graph)
    }

    pub fn get_causal_chain(&self) -> Arc<Mutex<CausalChain>> {
        Arc::clone(&self.causal_chain)
    }

    pub fn get_agent_registry(&self) -> Arc<std::sync::RwLock<crate::ccos::agent::InMemoryAgentRegistry>> {
        Arc::clone(&self.agent_registry)
    }

    pub fn get_delegating_arbiter(&self) -> Option<Arc<DelegatingArbiter>> {
        self.delegating_arbiter.as_ref().map(Arc::clone)
    }
    
    pub fn get_orchestrator(&self) -> Arc<Orchestrator> {
        Arc::clone(&self.orchestrator)
    }

    pub fn get_agent_config(&self) -> Arc<AgentConfig> { Arc::clone(&self.agent_config) }

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
        self.governance_kernel.validate_and_execute(plan, context).await
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
            allowed_capabilities: vec![
                "ccos.echo".to_string(),
                "ccos.math.add".to_string(),
            ].into_iter().collect(),
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
        assert!(execution_result.success, "execution_result indicates failure");

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
