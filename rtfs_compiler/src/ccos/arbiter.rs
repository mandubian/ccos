//! CCOS Arbiter
//!
//! This module defines the Arbiter, the primary cognitive component of a CCOS instance.
//! The Arbiter is responsible for high-level reasoning, decision-making, and learning.
//! It acts as the "mind" of the system, interpreting user requests and generating
//! plans to achieve them. It operates in a low-privilege sandbox and proposes
//! plans to the Governance Kernel for validation and execution.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use crate::runtime::error::RuntimeError;
use crate::runtime::values::Value;

use super::intent_graph::IntentGraph;
use super::types::{Intent, Plan};
#[allow(unused_imports)]
use super::types::PlanBody;
use super::types::StorableIntent;

/// The Arbiter - LLM kernel that converts natural language to structured intents and plans.
pub struct Arbiter {
    /// Intent graph for managing intent lifecycle.
    intent_graph: Arc<Mutex<IntentGraph>>,
    /// Configuration for the LLM kernel.
    config: ArbiterConfig,
    // TODO: Add reference to ContextHorizon and WorkingMemory for context-aware planning.
}

/// Configuration for the Arbiter LLM kernel.
#[derive(Debug, Clone)]
pub struct ArbiterConfig {
    pub model: String,
    pub context_window: usize,
    pub delegation_threshold: f64,
}

impl Arbiter {
    /// Create a new Arbiter with a given configuration.
    pub fn new(config: ArbiterConfig, intent_graph: Arc<Mutex<IntentGraph>>) -> Self {
        Self {
            intent_graph,
            config,
        }
    }

    /// Expose the IntentGraph so other engines (e.g., DelegatingArbiter) can store intents
    pub fn get_intent_graph(&self) -> Arc<Mutex<IntentGraph>> {
        Arc::clone(&self.intent_graph)
    }

    /// Main entry point: Convert natural language to a `Plan`.
    /// The returned plan is then passed to the Governance Kernel and Orchestrator.
    pub async fn process_natural_language(
        &self,
        natural_language: &str,
        context: Option<HashMap<String, Value>>,
    ) -> Result<Plan, RuntimeError> {
        // Step 1: Convert natural language to structured intent.
        let intent = self.natural_language_to_intent(natural_language, context).await?;

        // Step 2: Generate an appropriate plan for the intent.
        let plan = self.intent_to_plan(&intent).await?;

        Ok(plan)
    }

    /// Convert natural language to a structured `Intent` using LLM reasoning.
    async fn natural_language_to_intent(
        &self,
        natural_language: &str,
        _context: Option<HashMap<String, Value>>,
    ) -> Result<Intent, RuntimeError> {
        // TODO: Integrate with an actual LLM API for robust intent formulation.
        // For now, simulate LLM reasoning with pattern matching.

        let mut intent = Intent::new(natural_language.to_string());

        if natural_language.to_lowercase().contains("sentiment") {
            intent.name = Some("analyze_user_sentiment".to_string());
        } else if natural_language.to_lowercase().contains("optimize") {
            intent.name = Some("optimize_response_time".to_string());
        } else {
            intent.name = Some("general_assistance".to_string());
        };

        // Store the newly created intent in the graph.
        {
            let mut graph = self.intent_graph.lock()
                .map_err(|_| RuntimeError::Generic("Failed to lock IntentGraph".to_string()))?;

            // Minimal StorableIntent mapping; RTFS-specific fields remain empty for now
            let mut st = StorableIntent::new(intent.goal.clone());
            st.intent_id = intent.intent_id.clone();
            st.name = intent.name.clone();
            st.original_request = intent.original_request.clone();
            st.status = super::types::IntentStatus::Active;

            graph.store_intent(st)?;
        }

        Ok(intent)
    }

    /// Generate an appropriate `Plan` for a given `Intent`.
    async fn intent_to_plan(&self, intent: &Intent) -> Result<Plan, RuntimeError> {
        // TODO: Integrate with an LLM for sophisticated plan generation.
        // For now, use pattern matching to select from predefined plan bodies.

        let plan_body = match intent.name.as_deref() {
            Some("analyze_user_sentiment") => {
                r#"
                (do
                    (step "Fetch Data" (let [user-data (call :data.fetch-user-interactions :limit 100)]))
                    (step "Analyze Sentiment" (let [sentiment (call :ml.analyze-sentiment user-data)]))
                    (step "Generate Report" (call :reporting.generate-sentiment-report sentiment))
                )
                "#
            }
            Some("optimize_response_time") => {
                r#"
                (do
                    (step "Get Metrics" (let [metrics (call :monitoring.get-system-metrics)]))
                    (step "Identify Bottlenecks" (let [bottlenecks (call :analysis.identify-bottlenecks metrics)]))
                    (step.if (not (empty? bottlenecks))
                        (step "Apply Optimizations" (call :system.apply-optimizations bottlenecks))
                        (step "Log No-Op" (call :logging.log "No bottlenecks found"))
                    )
                )
                "#
            }
            _ => {
                // A generic plan for simple requests.
                r#"
                (do
                    (step "Process Generic Request" (call :generic.process-request goal))
                )
                "#
            }
        };

        let mut plan = Plan::new_rtfs(plan_body.to_string(), vec![intent.intent_id.clone()]);
        plan.name = intent.name.clone();
        Ok(plan)
    }
}

impl Default for ArbiterConfig {
    fn default() -> Self {
        Self {
            model: "claude-3.5-sonnet".to_string(),
            context_window: 100000,
            delegation_threshold: 0.8,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ccos::intent_graph::IntentGraph;
    use crate::ccos::intent_graph::IntentGraphConfig;

    #[tokio::test]
    async fn test_arbiter_proposes_plan() {
        let intent_graph = Arc::new(Mutex::new(IntentGraph::new_async(IntentGraphConfig::default()).await.unwrap()));
        let arbiter = Arbiter::new(ArbiterConfig::default(), intent_graph);

        // Test sentiment analysis request
        let plan = arbiter
            .process_natural_language("Analyze user sentiment from recent interactions", None)
            .await
            .unwrap();

        assert_eq!(plan.name, Some("analyze_user_sentiment".to_string()));
        if let PlanBody::Rtfs(body_text) = &plan.body {
            assert!(body_text.contains(":ml.analyze-sentiment"));
        } else {
            panic!("Plan body is not textual");
        }

        // Test performance optimization request
        let plan = arbiter
            .process_natural_language("Optimize system performance", None)
            .await
            .unwrap();

        assert_eq!(plan.name, Some("optimize_response_time".to_string()));
        if let PlanBody::Rtfs(body_text) = &plan.body {
            assert!(body_text.contains(":monitoring.get-system-metrics"));
        } else {
            panic!("Plan body is not textual");
        }
    }
}