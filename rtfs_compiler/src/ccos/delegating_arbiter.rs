use std::collections::HashMap;
use std::sync::Arc;

use async_trait::async_trait;
use serde::Deserialize;

use crate::runtime::error::RuntimeError;
use crate::runtime::values::Value;

use super::arbiter_engine::ArbiterEngine;
use super::types::{ExecutionResult, Intent, Plan};
use super::arbiter::Arbiter as BaseArbiter;
use crate::config::types::AgentConfig;
use super::delegation::ModelRegistry;
use super::types::StorableIntent;
use super::agent_registry::{AgentRegistry, IntentDraft};
use super::causal_chain::CausalChain;
use super::governance_kernel::GovernanceKernel;
use std::sync::Mutex as StdMutex;

/// An Arbiter implementation that uses the CCOS DelegationEngine + ModelRegistry to
/// ask a language model for both Intent extraction and Plan generation.
/// Falls back to the internal pattern-based Arbiter when the model output
/// cannot be parsed or the model is unavailable.
#[derive(Clone)]
pub struct DelegatingArbiter {
    base: Arc<BaseArbiter>,
    model_registry: Arc<ModelRegistry>,
    /// Which model id to call (must be registered in the registry)
    model_id: String,
    /// Optional AgentRegistry for pre-LLM delegation decisions (M4)
    agent_registry: Option<Arc<std::sync::RwLock<super::agent_registry::InMemoryAgentRegistry>>>,
    /// Optional causal chain for recording delegation events
    causal_chain: Option<Arc<StdMutex<CausalChain>>>,
    /// Optional governance kernel for delegation validation
    governance_kernel: Option<Arc<GovernanceKernel>>,
    /// Global agent configuration for delegation settings
    agent_config: Option<Arc<AgentConfig>>,
}

impl DelegatingArbiter {
    /// Create a new DelegatingArbiter.
    pub fn new(model_registry: Arc<ModelRegistry>, model_id: &str) -> Result<Self, RuntimeError> {
        use super::arbiter::ArbiterConfig;
        use super::intent_graph::IntentGraph;
        use std::sync::{Arc, Mutex};
        let base = Arc::new(BaseArbiter::new(
            ArbiterConfig::default(),
            Arc::new(Mutex::new(IntentGraph::new().unwrap())),
        ));
        Ok(Self {
            base,
            model_registry,
            model_id: model_id.to_string(),
            agent_registry: None,
            causal_chain: None,
            governance_kernel: None,
            agent_config: None,
        })
    }

    /// Create a DelegatingArbiter that reuses an existing base Arbiter (and its IntentGraph).
    pub fn with_base(
        base: Arc<BaseArbiter>,
        model_registry: Arc<ModelRegistry>,
        agent_registry: Option<Arc<std::sync::RwLock<super::agent_registry::InMemoryAgentRegistry>>>,
        causal_chain: Option<Arc<StdMutex<CausalChain>>>,
        governance_kernel: Option<Arc<GovernanceKernel>>,
        agent_config: Option<Arc<AgentConfig>>,
        model_id: &str,
    ) -> Result<Self, RuntimeError> {
        Ok(Self {
            base,
            model_registry,
            model_id: model_id.to_string(),
            agent_registry,
            causal_chain,
            governance_kernel,
            agent_config,
        })
    }

    /// Helper: run the configured model synchronously.
    fn run_model(&self, prompt: &str) -> Result<String, RuntimeError> {
        if let Some(provider) = self.model_registry.get(&self.model_id) {
            provider
                .infer(prompt)
                .map_err(|e| RuntimeError::Generic(format!("Model inference error: {}", e)))
        } else {
            Err(RuntimeError::Generic(format!(
                "Model '{}' not found in registry",
                self.model_id
            )))
        }
    }

    fn attempt_agent_delegation(&self, nl: &str) -> Option<Intent> {
        let reg = self.agent_registry.as_ref()?;
        let draft = IntentDraft { goal: nl.to_string(), constraint_keys: Self::extract_constraint_keys(nl) };
        // Resolve delegation settings (threshold, max candidates) via helper
    let settings = DelegationSettings::resolve(self.agent_config.as_ref().map(Arc::clone));
    // Short-circuit if delegation disabled
    if !settings.enabled { return None; }
        let candidate_limit = settings.max_candidates.unwrap_or(3).max(1) as usize;
        let candidates = reg.read().ok()?.find_candidates(&draft, candidate_limit);
        let top = candidates.first()?;
        if top.score > settings.threshold.unwrap_or(0.65) {
            // Enforce minimum skill hits if configured
            if let Some(min_hits) = settings.min_skill_hits { if top.skill_hits < min_hits { return None; } }
            // Build intent early (not yet stored) so events reference real intent id
            let mut intent = Intent::new(draft.goal.clone()).with_name(top.descriptor.agent_id.clone());
            // Emit proposed event
            if let Some(chain_arc) = &self.causal_chain {
                if let Ok(mut chain) = chain_arc.lock() {
                    let mut meta = std::collections::HashMap::new();
                    meta.insert("proposed_agent".to_string(), Value::String(top.descriptor.agent_id.clone()));
                    meta.insert("score".to_string(), Value::Float(top.score));
                    meta.insert("skill_hits".to_string(), Value::Integer(top.skill_hits as i64));
                    meta.insert("candidates".to_string(), Value::String(candidates.iter().map(|c| format!("{}:{:.2}", c.descriptor.agent_id, c.score)).collect::<Vec<_>>().join(",")));
                    let _ = chain.record_delegation_event(&intent.intent_id, "proposed", meta);
                }
            }
            // Governance validation (if kernel attached)
            if let Some(gov) = &self.governance_kernel {
                if let Err(err) = gov.validate_delegation(&intent, &top.descriptor.agent_id, top.score) {
                    if let Some(chain_arc) = &self.causal_chain {
                        if let Ok(mut chain) = chain_arc.lock() {
                            let mut meta = std::collections::HashMap::new();
                            meta.insert("agent".to_string(), Value::String(top.descriptor.agent_id.clone()));
                            meta.insert("score".to_string(), Value::Float(top.score));
                            meta.insert("reason".to_string(), Value::String(format!("{}", err)));
                            let _ = chain.record_delegation_event(&intent.intent_id, "rejected", meta);
                        }
                    }
                    return None;
                }
            }
            // Approved path
             intent.original_request = nl.to_string();
             intent.metadata.insert("delegation.selected_agent".to_string(), Value::String(top.descriptor.agent_id.clone()));
             intent.metadata.insert("delegation.rationale".to_string(), Value::String(top.rationale.clone()));
             let cand_str = candidates.iter().map(|c| format!("{}:{:.2}", c.descriptor.agent_id, c.score)).collect::<Vec<_>>().join(",");
             intent.metadata.insert("delegation.candidates".to_string(), Value::String(cand_str.clone()));
             // Persist intent
             let graph_arc = self.base.get_intent_graph();
             if let Ok(mut graph) = graph_arc.lock() {
                 let mut st = StorableIntent::new(intent.goal.clone());
                 st.intent_id = intent.intent_id.clone();
                 st.name = intent.name.clone();
                 st.original_request = intent.original_request.clone();
                 st.status = super::types::IntentStatus::Active;
                 let _ = graph.store_intent(st);
             }
             // Record delegation event in causal chain if available
             if let Some(chain_arc) = &self.causal_chain {
                 if let Ok(mut chain) = chain_arc.lock() {
                     let mut meta = std::collections::HashMap::new();
                     meta.insert("selected_agent".to_string(), Value::String(top.descriptor.agent_id.clone()));
                     meta.insert("score".to_string(), Value::Float(top.score));
                     meta.insert("rationale".to_string(), Value::String(top.rationale.clone()));
                     meta.insert("skill_hits".to_string(), Value::Integer(top.skill_hits as i64));
                     meta.insert("candidates".to_string(), Value::String(cand_str));
                     let _ = chain.record_delegation_event(&intent.intent_id, "approved", meta);
                 }
             }
             return Some(intent);
        }
        None
    }

    fn extract_constraint_keys(nl: &str) -> Vec<String> {
        let mut keys = Vec::new();
        let lower = nl.to_lowercase();
        if lower.contains("budget") || lower.contains("cost") { keys.push("budget".to_string()); }
        if lower.contains("eu") || lower.contains("europe") { keys.push("data-locality".to_string()); }
        if lower.contains("latency") { keys.push("latency".to_string()); }
        keys
    }
}

#[derive(Deserialize)]
struct IntentJson {
    name: String,
    #[serde(default)]
    goal: Option<String>,
    #[serde(default)]
    description: Option<String>,
}

#[async_trait(?Send)]
impl ArbiterEngine for DelegatingArbiter {
    async fn natural_language_to_intent(
        &self,
        natural_language: &str,
        context: Option<HashMap<String, Value>>,
    ) -> Result<Intent, RuntimeError> {
        // Pre-LLM agent delegation attempt (M4)
        if let Some(intent) = self.attempt_agent_delegation(natural_language) { return Ok(intent); }

        // First attempt: ask the model to convert NL → Intent JSON.
        let system_prompt = "Convert the following user request into minimal JSON with keys: name (snake_case), goal (string), description (string). Respond with ONLY the JSON.";
        let prompt = format!("{}\nUSER_REQUEST:\n{}", system_prompt, natural_language);

        match self.run_model(&prompt) {
            Ok(raw) => {
                // Attempt to parse JSON.
                match serde_json::from_str::<IntentJson>(&raw) {
                    Ok(parsed) => {
                        let mut intent = Intent::new(parsed.goal.clone().unwrap_or_else(|| natural_language.to_string()))
                            .with_name(parsed.name);
                        intent.original_request = natural_language.to_string();
                        if let Some(desc) = parsed.description {
                            intent.metadata.insert("description".to_string(), Value::String(desc));
                        }
                        if let Some(ctx) = context {
                            for (k, v) in ctx {
                                intent.metadata.insert(k, v);
                            }
                        }
                        // Store via base intent_graph
                        let graph_arc = self.base.get_intent_graph();
                        if let Ok(mut graph) = graph_arc.lock() {
                            let mut st = StorableIntent::new(intent.goal.clone());
                            st.intent_id = intent.intent_id.clone();
                            st.name = intent.name.clone();
                            st.original_request = intent.original_request.clone();
                            st.status = super::types::IntentStatus::Active;
                            if let Err(e) = graph.store_intent(st) {
                                return Err(e);
                            }
                        } else {
                            return Err(RuntimeError::Generic("Failed to lock IntentGraph".to_string()));
                        }
                        Ok(intent)
                    }
                    Err(_) => {
                        Err(RuntimeError::Generic("Failed to parse intent from model and no fallback available".to_string()))
                    }
                }
            }
            Err(_) => {
                Err(RuntimeError::Generic("Model inference failed and no fallback available".to_string()))
            }
        }
    }

    async fn intent_to_plan(&self, intent: &Intent) -> Result<Plan, RuntimeError> {
        // If delegation selected, currently still fall back to LLM planning path.
        // Future: call selected agent's planning API.
        let system_prompt = r#"You are an RTFS planner.

RTFS is a Lisp-flavoured DSL.  Core syntax (excerpt):

 program  =  (do expr*)
 expr     =  atom | list | vector | map
 list     =  '(' expr* ')'
 atom     =  symbol | string | integer | float | boolean | nil

Examples:

;; read a file and print first line
(do
  (let [content (read-file "Cargo.toml")]
       first    (first-line content))
  (println first))

;; simple JSON processing
(do
  (let data (json-parse (read-file "deps.json")))
  (for-each d data
    (println (get d "name"))))

STRICT RULES:
1. Respond with **plain RTFS code only**, no markdown fences.
2. It must start with `(do` and parse with the grammar above.
"#;

        let goal_clone = intent.goal.clone();
        let intent_json = format!("{{\"name\": \"{}\", \"goal\": \"{}\"}}", intent.name.as_deref().unwrap_or("unnamed"), goal_clone);
        let prompt = format!("{}\nINTENT_JSON:\n{}", system_prompt, intent_json);

        match self.run_model(&prompt) {
            Ok(first_try) => {
                if crate::parser::parse(&first_try).is_ok() {
                    let mut plan = Plan::new_rtfs(first_try, vec![intent.intent_id.clone()]);
                    plan.name = Some(format!("{}_plan", intent.name.as_deref().unwrap_or("unnamed")));
                    return Ok(plan);
                }
                let parse_err = crate::parser::parse(&first_try).err().unwrap();
                let retry_prompt = format!(
                    "{}\nThe previous RTFS code did not parse. Parse error: {}\nPlease return corrected RTFS code only.",
                    system_prompt, parse_err
                );
                match self.run_model(&retry_prompt) {
                    Ok(second_try) => {
                        if crate::parser::parse(&second_try).is_ok() {
                            let mut plan = Plan::new_rtfs(second_try, vec![intent.intent_id.clone()]);
                            plan.name = Some(format!("{}_plan", intent.name.as_deref().unwrap_or("unnamed")));
                            return Ok(plan);
                        }
                        Err(RuntimeError::Generic("Failed to generate valid plan".to_string()))
                    }
                    Err(_) => {
                        Err(RuntimeError::Generic("Failed to generate valid plan".to_string()))
                    }
                }
            }
            Err(_) => {
                Err(RuntimeError::Generic("Failed to generate valid plan".to_string()))
            }
        }
    }

    async fn execute_plan(&self, _plan: &Plan) -> Result<ExecutionResult, RuntimeError> {
        Err(RuntimeError::Generic("DelegatingArbiter does not support execute_plan directly".to_string()))
    }
}

/// Effective delegation settings (resolved from env + config)
struct DelegationSettings {
    threshold: Option<f64>,
    max_candidates: Option<u32>,
    min_skill_hits: Option<u32>,
    enabled: bool,
}

impl DelegationSettings {
    fn resolve(cfg: Option<Arc<AgentConfig>>) -> Self {
        // Start from config values (all optional). Enabled defaults to true if registry present.
        let (mut threshold, mut max_candidates, mut min_skill_hits, mut enabled) = if let Some(c) = cfg {
            (
                c.delegation.threshold,
                c.delegation.max_candidates,
                c.delegation.min_skill_hits,
                c.delegation.enabled.unwrap_or(true),
            )
        } else { (None, None, None, true) };
        // Env overrides
        if let Ok(v) = std::env::var("CCOS_DELEGATION_THRESHOLD") { if let Ok(p) = v.parse::<f64>() { threshold = Some(p); } }
        if let Ok(v) = std::env::var("CCOS_DELEGATION_MAX_CANDIDATES") { if let Ok(p) = v.parse::<u32>() { max_candidates = Some(p); } }
        if let Ok(v) = std::env::var("CCOS_DELEGATION_MIN_SKILL_HITS") { if let Ok(p) = v.parse::<u32>() { min_skill_hits = Some(p); } }
        if let Ok(v) = std::env::var("CCOS_DELEGATION_ENABLED") { enabled = matches!(v.as_str(), "1" | "true" | "TRUE" | "True"); }
        Self { threshold, max_candidates, min_skill_hits, enabled }
    }
}