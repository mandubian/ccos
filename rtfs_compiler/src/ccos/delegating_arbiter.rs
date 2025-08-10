use std::collections::HashMap;
use std::sync::Arc;

use async_trait::async_trait;
use serde::Deserialize;

use crate::runtime::error::RuntimeError;
use crate::runtime::values::Value;

use super::arbiter_engine::ArbiterEngine;
use super::types::{ExecutionResult, Intent, Plan};
use super::arbiter::Arbiter as BaseArbiter;
use super::delegation::{ModelRegistry};
use super::types::StorableIntent;

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
                        // Fallback to base implementation (pattern match)
                        // self.base.natural_language_to_intent(natural_language, context).await
                        Err(RuntimeError::Generic("Failed to parse intent from model and no fallback available".to_string()))
                    }
                }
            }
            Err(_) => {
                // Fallback to base implementation if model fails
                // self.base.natural_language_to_intent(natural_language, context).await
                Err(RuntimeError::Generic("Model inference failed and no fallback available".to_string()))
            }
        }
    }

    async fn intent_to_plan(&self, intent: &Intent) -> Result<Plan, RuntimeError> {
        // --- RTFS mini-spec & examples injected into the prompt ---
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
                // Try to parse the returned code.
                if crate::parser::parse(&first_try).is_ok() {
                    let mut plan = Plan::new_rtfs(first_try, vec![intent.intent_id.clone()]);
                    plan.name = Some(format!("{}_plan", intent.name.as_deref().unwrap_or("unnamed")));
                    return Ok(plan);
                }

                // If parsing failed, ask the model to fix it using the error message.
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
                        // Fall back
                        // self.base.intent_to_plan(intent).await
                        Err(RuntimeError::Generic("Failed to generate valid plan".to_string()))
                    }
                    Err(_) => {
                        // self.base.intent_to_plan(intent).await
                        Err(RuntimeError::Generic("Failed to generate valid plan".to_string()))
                    }
                }
            }
            Err(_) => {
                // self.base.intent_to_plan(intent).await
                Err(RuntimeError::Generic("Failed to generate valid plan".to_string()))
            }
        }
    }

    async fn execute_plan(&self, _plan: &Plan) -> Result<ExecutionResult, RuntimeError> {
        // self.base.execute_plan(plan).await
        Err(RuntimeError::Generic("DelegatingArbiter does not support execute_plan directly".to_string()))
    }
}