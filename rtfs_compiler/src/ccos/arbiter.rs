use super::{
    causal_chain::CausalChain,
    context_horizon::ContextHorizonManager,
    intent_graph::IntentGraph,
    task_context::TaskContext,
    types::{ExecutionResult, Intent, IntentId, Plan, PlanId},
};
use crate::ast::MapKey;
use crate::runtime::error::RuntimeError;
use crate::runtime::stdlib::StandardLibrary;
use crate::runtime::values::Arity;
use crate::runtime::values::Value;
use crate::runtime::{capability::inject_capability, environment::Environment};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};
use uuid::Uuid;
use super::arbiter_engine::ArbiterEngine;
use async_trait::async_trait;

/// Expected data type for user prompt response
#[derive(Debug, Clone)]
pub enum PromptExpectedType {
    Text,
    Number,
    Boolean,
    Choice(Vec<String>),
}

/// Ticket representing a pending user prompt issued by the Arbiter
#[derive(Debug, Clone)]
pub struct PromptTicket {
    pub ticket_id: String,
    pub prompt: String,
    pub expected_type: PromptExpectedType,
    pub timestamp: u64,
}

/// The Arbiter - LLM kernel that converts natural language to structured intents and plans
pub struct Arbiter {
    /// Intent graph for managing intent lifecycle
    intent_graph: Arc<Mutex<IntentGraph>>,
    /// Task context for hierarchical context management
    task_context: Arc<Mutex<TaskContext>>,
    /// Causal chain for tracking action outcomes
    causal_chain: Arc<Mutex<CausalChain>>,
    /// Context horizon for adaptive window management
    context_horizon: Arc<Mutex<ContextHorizonManager>>,
    /// Configuration for the LLM kernel
    config: ArbiterConfig,
    /// Learning state and patterns
    learning_state: Arc<Mutex<LearningState>>,
    /// Pending user prompts awaiting external input
    pending_prompts: Arc<Mutex<HashMap<String, PromptTicket>>>,
}

/// Configuration for the Arbiter LLM kernel
#[derive(Debug, Clone)]
pub struct ArbiterConfig {
    pub model: String,
    pub context_window: usize,
    pub learning_rate: f64,
    pub delegation_threshold: f64,
    pub cost_budget: f64,
    pub ethical_constraints: Vec<String>,
}

/// Learning state for the Arbiter
#[derive(Debug, Default)]
struct LearningState {
    pub patterns: HashMap<String, f64>,
    pub cost_history: Vec<f64>,
    pub success_rates: HashMap<String, f64>,
    pub human_feedback: Vec<HumanFeedback>,
}

/// Human feedback for reality grounding
#[derive(Debug, Clone)]
pub struct HumanFeedback {
    pub intent_id: IntentId,
    pub satisfaction_score: f64,
    pub alignment_score: f64,
    pub cost_effectiveness: f64,
    pub comments: String,
    pub timestamp: u64,
}

impl Arbiter {
    /// Create a new Arbiter with default configuration
    pub fn new() -> Result<Self, RuntimeError> {
        Ok(Self {
            intent_graph: Arc::new(Mutex::new(IntentGraph::new()?)),
            task_context: Arc::new(Mutex::new(TaskContext::new()?)),
            causal_chain: Arc::new(Mutex::new(CausalChain::new()?)),
            context_horizon: Arc::new(Mutex::new(ContextHorizonManager::new()?)),
            config: ArbiterConfig::default(),
            learning_state: Arc::new(Mutex::new(LearningState::default())),
            pending_prompts: Arc::new(Mutex::new(HashMap::new())),
        })
    }

    /// Create a new Arbiter with custom configuration
    pub fn with_config(config: ArbiterConfig) -> Result<Self, RuntimeError> {
        Ok(Self {
            intent_graph: Arc::new(Mutex::new(IntentGraph::new()?)),
            task_context: Arc::new(Mutex::new(TaskContext::new()?)),
            causal_chain: Arc::new(Mutex::new(CausalChain::new()?)),
            context_horizon: Arc::new(Mutex::new(ContextHorizonManager::new()?)),
            config,
            learning_state: Arc::new(Mutex::new(LearningState::default())),
            pending_prompts: Arc::new(Mutex::new(HashMap::new())),
        })
    }

    /// Main entry point: Convert natural language to intent and execute plan
    pub async fn process_natural_language(
        &self,
        natural_language: &str,
        context: Option<HashMap<String, Value>>,
    ) -> Result<ExecutionResult, RuntimeError> {
        // Step 1: Convert natural language to structured intent
        let intent = self
            .natural_language_to_intent(natural_language, context)
            .await?;

        // Step 2: Generate or select appropriate plan
        let plan = self.intent_to_plan(&intent).await?;

        // Step 3: Execute the plan
        let result = self.execute_plan(&plan).await?;

        // Step 4: Learn from the execution
        self.learn_from_execution(&intent, &plan, &result).await?;

        Ok(result)
    }

    /// Convert natural language to structured intent using LLM reasoning
    async fn natural_language_to_intent(
        &self,
        natural_language: &str,
        context: Option<HashMap<String, Value>>,
    ) -> Result<Intent, RuntimeError> {
        // TODO: Integrate with actual LLM API
        // For now, simulate LLM reasoning with pattern matching

        let intent = if natural_language.to_lowercase().contains("sentiment") {
            Intent::with_name(
                "analyze_user_sentiment".to_string(),
                natural_language.to_string(),
                "Analyze user sentiment from recent interactions".to_string(),
            )
        } else if natural_language.to_lowercase().contains("optimize")
            || natural_language.to_lowercase().contains("performance")
        {
            Intent::with_name(
                "optimize_response_time".to_string(),
                natural_language.to_string(),
                "Optimize system performance and response time".to_string(),
            )
        } else if natural_language.to_lowercase().contains("learn")
            || natural_language.to_lowercase().contains("pattern")
        {
            Intent::with_name(
                "learn_from_interaction".to_string(),
                natural_language.to_string(),
                "Extract learning patterns from interaction data".to_string(),
            )
        } else {
            // Default intent for unknown requests
            Intent::with_name(
                "general_assistance".to_string(),
                natural_language.to_string(),
                natural_language.to_string(),
            )
        };

        // Store intent in graph
        {
            let mut graph = self.intent_graph.lock().unwrap();
            graph.store_intent(intent.clone())?;
        }

        Ok(intent)
    }

    /// Generate or select appropriate plan for an intent
    async fn intent_to_plan(&self, intent: &Intent) -> Result<Plan, RuntimeError> {
        // TODO: Integrate with LLM for plan generation
        // For now, use pattern matching to select from predefined plans

        let plan = match intent.name.as_str() {
            "analyze_user_sentiment" => {
                let mut plan = Plan::new_rtfs(
                    r#"
                    (let [user-data (fetch-user-interactions :limit 100)
                          processed (map process-interaction user-data)
                          sentiment-scores (map analyze-sentiment processed)
                          aggregated (aggregate-sentiment sentiment-scores)
                          report (generate-sentiment-report aggregated)]
                      (store-result :intent "analyze_user_sentiment" :result report))
                    "#
                    .to_string(),
                    vec![intent.intent_id.clone()],
                );
                plan.name = "sentiment_analysis_pipeline".to_string();
                plan
            }
            "optimize_response_time" => {
                let mut plan = Plan::new_rtfs(
                    r#"
                    (let [current-metrics (get-system-metrics)
                          bottlenecks (identify-bottlenecks current-metrics)
                          optimizations (generate-optimizations bottlenecks)
                          impact (estimate-impact optimizations)]
                      (if (> impact 0.1)
                        (apply-optimizations optimizations)
                        (log "Optimization impact too low")))
                    "#
                    .to_string(),
                    vec![intent.intent_id.clone()],
                );
                plan.name = "performance_optimization_plan".to_string();
                plan
            }
            "learn_from_interaction" => {
                let mut plan = Plan::new_rtfs(
                    r#"
                    (let [interaction-data (get-interaction-history :days 7)
                          patterns (extract-patterns interaction-data)
                          insights (analyze-insights patterns)
                          learning (synthesize-learning insights)]
                      (store-learning :patterns learning)
                      (update-behavior-models learning))
                    "#
                    .to_string(),
                    vec![intent.intent_id.clone()],
                );
                plan.name = "learning_extraction_plan".to_string();
                plan
            }
            _ => {
                // Generate a generic plan
                let mut plan = Plan::new_rtfs(
                    r#"
                    (let [result (process-request :goal goal)]
                      (store-result :intent intent-id :result result))
                    "#
                    .to_string(),
                    vec![intent.intent_id.clone()],
                );
                plan.name = "generic_plan".to_string();
                plan
            }
        };

        Ok(plan)
    }

    /// Execute a plan and track the execution
    pub async fn execute_plan(&self, plan: &Plan) -> Result<ExecutionResult, RuntimeError> {
        // ------------------------------------------------------------------
        // Logging: show plan & context being executed
        // ------------------------------------------------------------------
        println!("[Arbiter] ⏩ Executing Plan '{}' (id={}) linked to intents: {:?}", plan.name, plan.plan_id, plan.intent_ids);

        // ------------------------------------------------------------------
        // Build execution environment with stdlib and inject capability wrappers
        // ------------------------------------------------------------------
        let mut env: Environment = StandardLibrary::create_global_environment();

        let intent_id = plan.intent_ids.first().cloned().unwrap_or_default();

        // Inject standard capabilities so that every call is recorded in the causal chain.
        self.inject_standard_capabilities(&mut env, &plan.plan_id, &intent_id)?;

        // Log PlanStarted lifecycle action
        {
            let mut chain = self.causal_chain.lock().unwrap();
            let _ = chain.log_plan_started(&plan.plan_id, &intent_id);
        }

        // TODO: Integrate with RTFS runtime for actual execution
        // For now, simulate execution with pattern matching

        let result = match plan.name.as_str() {
            "sentiment_analysis_pipeline" => {
                println!("[Arbiter] ▶ Simulating execution path: sentiment_analysis_pipeline");
                // Simulate sentiment analysis execution
                ExecutionResult {
                    success: true,
                    value: Value::Map({
                        let mut map = HashMap::new();
                        map.insert(
                            MapKey::String("sentiment_score".to_string()),
                            Value::Float(0.75),
                        );
                        map.insert(MapKey::String("confidence".to_string()), Value::Float(0.85));
                        map.insert(
                            MapKey::String("sample_size".to_string()),
                            Value::Integer(100),
                        );
                        map
                    }),
                    metadata: {
                        let mut meta = HashMap::new();
                        meta.insert("execution_time_ms".to_string(), Value::Integer(150));
                        meta.insert("cost".to_string(), Value::Float(0.05));
                        meta
                    },
                }
            }
            "performance_optimization_plan" => {
                println!("[Arbiter] ▶ Simulating execution path: performance_optimization_plan");
                // Simulate performance optimization execution
                ExecutionResult {
                    success: true,
                    value: Value::Map({
                        let mut map = HashMap::new();
                        map.insert(
                            MapKey::String("optimizations_applied".to_string()),
                            Value::Integer(3),
                        );
                        map.insert(
                            MapKey::String("performance_improvement".to_string()),
                            Value::Float(0.15),
                        );
                        map.insert(
                            MapKey::String("cost_savings".to_string()),
                            Value::Float(0.02),
                        );
                        map
                    }),
                    metadata: {
                        let mut meta = HashMap::new();
                        meta.insert("execution_time_ms".to_string(), Value::Integer(200));
                        meta.insert("cost".to_string(), Value::Float(0.03));
                        meta
                    },
                }
            }
            "learning_extraction_plan" => {
                println!("[Arbiter] ▶ Simulating execution path: learning_extraction_plan");
                // Simulate learning extraction execution
                ExecutionResult {
                    success: true,
                    value: Value::Map({
                        let mut map = HashMap::new();
                        map.insert(
                            MapKey::String("patterns_found".to_string()),
                            Value::Integer(5),
                        );
                        map.insert(
                            MapKey::String("learning_confidence".to_string()),
                            Value::Float(0.78),
                        );
                        map.insert(
                            MapKey::String("adaptation_score".to_string()),
                            Value::Float(0.82),
                        );
                        map
                    }),
                    metadata: {
                        let mut meta = HashMap::new();
                        meta.insert("execution_time_ms".to_string(), Value::Integer(300));
                        meta.insert("cost".to_string(), Value::Float(0.08));
                        meta
                    },
                }
            }
            _ => {
                // Generic execution result
                println!("[Arbiter] ▶ Executing generic plan path");
                ExecutionResult {
                    success: true,
                    value: Value::String("Generic execution completed".to_string()),
                    metadata: {
                        let mut meta = HashMap::new();
                        meta.insert("execution_time_ms".to_string(), Value::Integer(100));
                        meta.insert("cost".to_string(), Value::Float(0.01));
                        meta
                    },
                }
            }
        };

        // Log PlanCompleted lifecycle action
        {
            let mut chain = self.causal_chain.lock().unwrap();
            let _ = chain.log_plan_completed(&plan.plan_id, &intent_id);
        }

        // Update intent status based on result
        {
            let mut graph = self.intent_graph.lock().unwrap();
            for intent_id in &plan.intent_ids {
                if let Some(intent) = graph.get_intent(intent_id) {
                    let mut update_intent = intent.clone();
                    update_intent.intent_id = intent_id.clone();
                    graph.update_intent(update_intent, &result)?;
                }
            }
        }

        Ok(result)
    }

    /// Inject commonly used capabilities into the evaluator environment so that
    /// each invocation is automatically logged in the Causal Chain.
    fn inject_standard_capabilities(
        &self,
        env: &mut Environment,
        plan_id: &PlanId,
        intent_id: &IntentId,
    ) -> Result<(), RuntimeError> {
        // Clone shared state pointers for the logger closure.
        let chain_arc = Arc::clone(&self.causal_chain);
        let plan_id = plan_id.clone();
        let intent_id = intent_id.clone();

        let logger = move |cap_id: &str, args: &Vec<Value>| {
            let mut chain = chain_arc.lock().unwrap();
            chain.log_capability_call(
                &plan_id,
                &intent_id,
                &cap_id.to_string(),
                cap_id,
                args.clone(),
            )?;
            Ok(())
        };

        // Wrap the built-in `ask-human` function.
        inject_capability(
            env,
            "ask-human",
            super::types::ASK_HUMAN_CAPABILITY_ID,
            Arity::Range(1, 2),
            logger,
        )
    }

    /// Learn from execution results and update patterns
    async fn learn_from_execution(
        &self,
        intent: &Intent,
        plan: &Plan,
        result: &ExecutionResult,
    ) -> Result<(), RuntimeError> {
        let mut learning = self.learning_state.lock().unwrap();

        // Update success rates
        let success_key = format!("{}:{}", intent.name, plan.name);
        let current_rate = learning.success_rates.get(&success_key).unwrap_or(&0.5);
        let new_rate = if result.success {
            *current_rate * 0.9 + 0.1
        } else {
            *current_rate * 0.9
        };
        learning.success_rates.insert(success_key, new_rate);

        // Track costs
        if let Some(cost) = result.metadata.get("cost") {
            if let Value::Float(cost_value) = cost {
                learning.cost_history.push(*cost_value);
            }
        }

        // Update patterns based on successful executions
        if result.success {
            let pattern_key = format!("successful_{}", intent.name);
            let current_pattern = learning.patterns.get(&pattern_key).unwrap_or(&0.0);
            let new_pattern = current_pattern + self.config.learning_rate;
            learning.patterns.insert(pattern_key, new_pattern);
        }

        Ok(())
    }

    /// Record human feedback for reality grounding
    pub fn record_human_feedback(&self, feedback: HumanFeedback) -> Result<(), RuntimeError> {
        let mut learning = self.learning_state.lock().unwrap();
        learning.human_feedback.push(feedback);
        Ok(())
    }

    /// Get learning insights and patterns
    pub fn get_learning_insights(&self) -> Result<HashMap<String, Value>, RuntimeError> {
        let learning = self.learning_state.lock().unwrap();

        let mut insights = HashMap::new();

        // Average success rate
        let avg_success: f64 = learning.success_rates.values().sum::<f64>()
            / learning.success_rates.len().max(1) as f64;
        insights.insert(
            "average_success_rate".to_string(),
            Value::Float(avg_success),
        );

        // Average cost
        let avg_cost: f64 =
            learning.cost_history.iter().sum::<f64>() / learning.cost_history.len().max(1) as f64;
        insights.insert("average_cost".to_string(), Value::Float(avg_cost));

        // Top patterns
        let mut sorted_patterns: Vec<_> = learning.patterns.iter().collect();
        sorted_patterns.sort_by(|a, b| b.1.partial_cmp(a.1).unwrap());

        let top_patterns: Vec<Value> = sorted_patterns
            .iter()
            .take(5)
            .map(|(k, v)| {
                Value::Map({
                    let mut map = HashMap::new();
                    map.insert(
                        MapKey::String("pattern".to_string()),
                        Value::String(k.to_string()),
                    );
                    map.insert(MapKey::String("strength".to_string()), Value::Float(**v));
                    map
                })
            })
            .collect();

        insights.insert("top_patterns".to_string(), Value::Vector(top_patterns));

        Ok(insights)
    }

    /// Get the intent graph for external access
    pub fn get_intent_graph(&self) -> Arc<Mutex<IntentGraph>> {
        Arc::clone(&self.intent_graph)
    }

    /// Get the task context for external access
    pub fn get_task_context(&self) -> Arc<Mutex<TaskContext>> {
        Arc::clone(&self.task_context)
    }

    /// Get the causal chain for external access
    pub fn get_causal_chain(&self) -> Arc<Mutex<CausalChain>> {
        Arc::clone(&self.causal_chain)
    }

    /// Get the context horizon for external access
    pub fn get_context_horizon(&self) -> Arc<Mutex<ContextHorizonManager>> {
        self.context_horizon.clone()
    }

    // ---------------------------------------------------------------------
    // User-prompt APIs (ask-human stub capability)
    // ---------------------------------------------------------------------

    /// Issue a prompt that requires human input. Returns a `Value::ResourceHandle(ticket_id)`
    /// that can flow through the RTFS runtime until the user provides a value.
    pub fn issue_user_prompt(&self, prompt: String, expected_type: PromptExpectedType) -> Value {
        let ticket_id = format!("prompt-{}", Uuid::new_v4());
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs();

        let ticket = PromptTicket {
            ticket_id: ticket_id.clone(),
            prompt,
            expected_type,
            timestamp: now,
        };

        {
            let mut map = self.pending_prompts.lock().unwrap();
            map.insert(ticket_id.clone(), ticket);
        }

        Value::ResourceHandle(ticket_id)
    }

    /// Resolve a previously issued user prompt by supplying the user's response.
    /// Returns the resolved value if successful.
    pub fn resolve_user_prompt(
        &self,
        ticket_id: &str,
        user_value: Value,
    ) -> Result<Value, RuntimeError> {
        let mut map = self.pending_prompts.lock().unwrap();
        if map.remove(ticket_id).is_some() {
            // In a future version we could validate `user_value` against expected_type.
            Ok(user_value)
        } else {
            Err(RuntimeError::Generic(format!(
                "Unknown or already-resolved user prompt ticket: {}",
                ticket_id
            )))
        }
    }

    /// Return a snapshot of all currently pending user prompts.
    pub fn list_pending_prompts(&self) -> Vec<PromptTicket> {
        let map = self.pending_prompts.lock().unwrap();
        map.values().cloned().collect()
    }
}

impl Default for ArbiterConfig {
    fn default() -> Self {
        Self {
            model: "claude-3.5-sonnet".to_string(),
            context_window: 100000,
            learning_rate: 0.01,
            delegation_threshold: 0.8,
            cost_budget: 100.0,
            ethical_constraints: vec![
                "privacy".to_string(),
                "transparency".to_string(),
                "fairness".to_string(),
            ],
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_arbiter_natural_language_processing() {
        let arbiter = Arbiter::new().unwrap();

        // Test sentiment analysis request
        let result = arbiter
            .process_natural_language("Analyze user sentiment from recent interactions", None)
            .await
            .unwrap();

        assert!(result.success);

        // Test performance optimization request
        let result = arbiter
            .process_natural_language("Optimize system performance", None)
            .await
            .unwrap();

        assert!(result.success);

        // Test learning request
        let result = arbiter
            .process_natural_language("Learn from user interaction patterns", None)
            .await
            .unwrap();

        assert!(result.success);
    }

    #[test]
    fn test_human_feedback_recording() {
        let arbiter = Arbiter::new().unwrap();

        let feedback = HumanFeedback {
            intent_id: "test-intent".to_string(),
            satisfaction_score: 0.8,
            alignment_score: 0.9,
            cost_effectiveness: 0.7,
            comments: "Good performance".to_string(),
            timestamp: 1234567890,
        };

        arbiter.record_human_feedback(feedback).unwrap();

        let insights = arbiter.get_learning_insights().unwrap();
        assert!(insights.contains_key("average_success_rate"));
    }
}

#[async_trait(?Send)]
impl ArbiterEngine for Arbiter {
    async fn natural_language_to_intent(
        &self,
        natural_language: &str,
        context: Option<std::collections::HashMap<String, Value>>,
    ) -> Result<Intent, RuntimeError> {
        // Reuse internal method
        self.natural_language_to_intent(natural_language, context)
            .await
    }

    async fn intent_to_plan(&self, intent: &Intent) -> Result<Plan, RuntimeError> {
        self.intent_to_plan(intent).await
    }

    async fn execute_plan(&self, plan: &Plan) -> Result<ExecutionResult, RuntimeError> {
        self.execute_plan(plan).await
    }

    async fn learn_from_execution(
        &self,
        intent: &Intent,
        plan: &Plan,
        result: &ExecutionResult,
    ) -> Result<(), RuntimeError> {
        self.learn_from_execution(intent, plan, result).await
    }
}
