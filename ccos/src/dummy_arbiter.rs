use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use regex::Regex;

use rtfs::runtime::error::RuntimeError;
use rtfs::runtime::values::Value;

use super::arbiter_config::ArbiterConfig;
use super::arbiter_engine::ArbiterEngine;
use super::intent_graph::IntentGraph;
use super::types::{ExecutionResult, Intent, Plan, PlanBody, StorableIntent};

/// A deterministic dummy arbiter for testing purposes.
/// This arbiter provides predictable responses based on simple pattern matching
/// and is useful for unit tests and CI/CD pipelines.
pub struct DummyArbiter {
    config: ArbiterConfig,
    intent_graph: Arc<Mutex<IntentGraph>>,
}

impl DummyArbiter {
    /// Create a new dummy arbiter with the given configuration.
    pub fn new(config: ArbiterConfig, intent_graph: Arc<Mutex<IntentGraph>>) -> Self {
        Self {
            config,
            intent_graph,
        }
    }

    /// Generate a deterministic intent based on natural language input.
    fn generate_dummy_intent(&self, nl: &str) -> Intent {
        let lower_nl = nl.to_lowercase();
        
        // Simple pattern matching for deterministic responses
        if lower_nl.contains("sentiment") || lower_nl.contains("analyze") || lower_nl.contains("feeling") {
            Intent {
                intent_id: format!("dummy_sentiment_{}", uuid::Uuid::new_v4()),
                name: Some("analyze_user_sentiment".to_string()),
                goal: "Analyze sentiment from user interactions".to_string(),
                original_request: nl.to_string(),
                constraints: {
                    let mut map = HashMap::new();
                    map.insert("privacy".to_string(), Value::String("high".to_string()));
                    map
                },
                preferences: {
                    let mut map = HashMap::new();
                    map.insert("accuracy".to_string(), Value::String("high".to_string()));
                    map
                },
                success_criteria: Some(Value::String("sentiment_analyzed".to_string())),
                status: super::types::IntentStatus::Active,
                created_at: std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_secs(),
                updated_at: std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_secs(),
                metadata: HashMap::new(),
            }
        } else if lower_nl.contains("optimize") || lower_nl.contains("improve") || lower_nl.contains("performance") {
            Intent {
                intent_id: format!("dummy_optimize_{}", uuid::Uuid::new_v4()),
                name: Some("optimize_response_time".to_string()),
                goal: "Optimize system performance".to_string(),
                original_request: nl.to_string(),
                constraints: {
                    let mut map = HashMap::new();
                    map.insert("budget".to_string(), Value::String("low".to_string()));
                    map
                },
                preferences: {
                    let mut map = HashMap::new();
                    map.insert("speed".to_string(), Value::String("high".to_string()));
                    map
                },
                success_criteria: Some(Value::String("performance_optimized".to_string())),
                status: super::types::IntentStatus::Active,
                created_at: std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_secs(),
                updated_at: std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_secs(),
                metadata: HashMap::new(),
            }
        } else if lower_nl.contains("hello") || lower_nl.contains("hi") || lower_nl.contains("greet") {
            Intent {
                intent_id: format!("dummy_greeting_{}", uuid::Uuid::new_v4()),
                name: Some("greet_user".to_string()),
                goal: "Greet the user".to_string(),
                original_request: nl.to_string(),
                constraints: HashMap::new(),
                preferences: {
                    let mut map = HashMap::new();
                    map.insert("friendliness".to_string(), Value::String("high".to_string()));
                    map
                },
                success_criteria: Some(Value::String("user_greeted".to_string())),
                status: super::types::IntentStatus::Active,
                created_at: std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_secs(),
                updated_at: std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_secs(),
                metadata: HashMap::new(),
            }
        } else {
            // Default intent for unrecognized patterns
            Intent {
                intent_id: format!("dummy_general_{}", uuid::Uuid::new_v4()),
                name: Some("general_assistance".to_string()),
                goal: "Provide general assistance".to_string(),
                original_request: nl.to_string(),
                constraints: HashMap::new(),
                preferences: {
                    let mut map = HashMap::new();
                    map.insert("helpfulness".to_string(), Value::String("high".to_string()));
                    map
                },
                success_criteria: Some(Value::String("assistance_provided".to_string())),
                status: super::types::IntentStatus::Active,
                created_at: std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_secs(),
                updated_at: std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_secs(),
                metadata: HashMap::new(),
            }
        }
    }

    /// Generate a deterministic RTFS plan based on the intent.
    fn generate_dummy_plan(&self, intent: &Intent) -> Plan {
        let plan_body = match intent.name.as_deref() {
            Some("analyze_user_sentiment") => {
                r#"
(do
    (step "Fetch Data" (call :ccos.echo "fetched user interactions"))
    (step "Analyze Sentiment" (call :ccos.echo "sentiment: positive"))
    (step "Generate Report" (call :ccos.echo "report generated"))
)
"#
            }
            Some("optimize_response_time") => {
                r#"
(do
    (step "Get Metrics" (call :ccos.echo "metrics collected"))
    (step "Identify Bottlenecks" (call :ccos.echo "bottlenecks identified"))
    (step "Apply Optimizations" (call :ccos.echo "optimizations applied"))
)
"#
            }
            Some("greet_user") => {
                r#"
(do
    (step "Generate Greeting" (call :ccos.echo "Hello! How can I help you today?"))
)
"#
            }
            _ => {
                // Default plan for general assistance
                r#"
(do
    (step "Process Request" (call :ccos.echo "processing your request"))
    (step "Provide Response" (call :ccos.echo "here is your response"))
)
"#
            }
        };

        Plan {
            plan_id: format!("dummy_plan_{}", uuid::Uuid::new_v4()),
            name: intent.name.clone(),
            intent_ids: vec![intent.intent_id.clone()],
            language: super::types::PlanLanguage::Rtfs20,
            body: super::types::PlanBody::Rtfs(plan_body.trim().to_string()),
            status: super::types::PlanStatus::Draft,
            created_at: std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_secs(),
            metadata: HashMap::new(),
        }
    }

    /// Store the intent in the intent graph.
    fn store_intent(&self, intent: &Intent) -> Result<(), RuntimeError> {
        let mut graph = self.intent_graph.lock()
            .map_err(|_| RuntimeError::Generic("Failed to lock IntentGraph".to_string()))?;

        let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_secs();
        let storable_intent = StorableIntent {
            intent_id: intent.intent_id.clone(),
            name: intent.name.clone(),
            original_request: intent.original_request.clone(),
            rtfs_intent_source: format!("(intent {} \"{}\")", intent.intent_id, intent.goal),
            goal: intent.goal.clone(),
            constraints: {
                let mut map = HashMap::new();
                for (k, v) in &intent.constraints {
                    map.insert(k.clone(), format!("{:?}", v));
                }
                map
            },
            preferences: {
                let mut map = HashMap::new();
                for (k, v) in &intent.preferences {
                    map.insert(k.clone(), format!("{:?}", v));
                }
                map
            },
            success_criteria: intent.success_criteria.as_ref().map(|v| format!("{:?}", v)),
            parent_intent: None,
            child_intents: vec![],
            triggered_by: super::types::TriggerSource::HumanRequest,
            generation_context: super::types::GenerationContext {
                arbiter_version: "dummy-1.0".to_string(),
                generation_timestamp: now,
                input_context: HashMap::new(),
                reasoning_trace: Some("Dummy arbiter deterministic generation".to_string()),
            },
            status: super::types::IntentStatus::Active,
            priority: 1,
            created_at: now,
            updated_at: now,
            metadata: {
                let mut map = HashMap::new();
                for (k, v) in &intent.metadata {
                    map.insert(k.clone(), format!("{:?}", v));
                }
                map
            },
        };

        graph.store_intent(storable_intent)
    }

    /// Validate the generated plan against security constraints.
    fn validate_plan(&self, plan: &Plan) -> Result<(), RuntimeError> {
        // Check plan complexity
        if let PlanBody::Rtfs(body) = &plan.body {
            let step_count = body.matches("(step").count();
            if step_count > self.config.security_config.max_plan_complexity {
                return Err(RuntimeError::Generic(format!(
                    "Plan too complex: {} steps (max: {})",
                    step_count,
                    self.config.security_config.max_plan_complexity
                )));
            }
        }

        // Check capability prefixes
        if let PlanBody::Rtfs(body) = &plan.body {
            let capability_regex = Regex::new(r":([a-zA-Z0-9._-]+)").unwrap();
            for cap in capability_regex.captures_iter(body) {
                let capability = &cap[1];
                
                // Check blocked prefixes
                for blocked in &self.config.security_config.blocked_capability_prefixes {
                    if capability.starts_with(blocked) {
                        return Err(RuntimeError::Generic(format!(
                            "Blocked capability: {}",
                            capability
                        )));
                    }
                }
                
                // Check allowed prefixes (if any are specified)
                if !self.config.security_config.allowed_capability_prefixes.is_empty() {
                    let mut allowed = false;
                    for allowed_prefix in &self.config.security_config.allowed_capability_prefixes {
                        if capability.starts_with(allowed_prefix) {
                            allowed = true;
                            break;
                        }
                    }
                    if !allowed {
                        return Err(RuntimeError::Generic(format!(
                            "Capability not allowed: {}",
                            capability
                        )));
                    }
                }
            }
        }

        Ok(())
    }
}

#[async_trait(?Send)]
impl ArbiterEngine for DummyArbiter {
    async fn natural_language_to_intent(
        &self,
        natural_language: &str,
        _context: Option<HashMap<String, Value>>,
    ) -> Result<Intent, RuntimeError> {
        let intent = self.generate_dummy_intent(natural_language);
        
        // Store the intent in the graph
        self.store_intent(&intent)?;
        
        Ok(intent)
    }

    async fn intent_to_plan(
        &self,
        intent: &Intent,
    ) -> Result<Plan, RuntimeError> {
        let plan = self.generate_dummy_plan(intent);
        
        // Validate the plan
        self.validate_plan(&plan)?;
        
        Ok(plan)
    }

    async fn execute_plan(
        &self,
        plan: &Plan,
    ) -> Result<ExecutionResult, RuntimeError> {
        // For dummy arbiter, we just return a success result
        // In a real implementation, this would execute the RTFS plan
        Ok(ExecutionResult {
            success: true,
            value: Value::String("Dummy execution completed successfully".to_string()),
            metadata: HashMap::new(),
        })
    }

    async fn learn_from_execution(
        &self,
        _intent: &Intent,
        _plan: &Plan,
        _result: &ExecutionResult,
    ) -> Result<(), RuntimeError> {
        // Dummy arbiter doesn't learn
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::intent_graph::IntentGraphConfig;

    #[tokio::test]
    async fn test_dummy_arbiter_sentiment() {
        let intent_graph = Arc::new(Mutex::new(IntentGraph::new_async(IntentGraphConfig::default()).await.unwrap()));
        let config = ArbiterConfig::default();
        let arbiter = DummyArbiter::new(config, intent_graph);

        let intent = arbiter
            .natural_language_to_intent("Analyze user sentiment", None)
            .await
            .unwrap();

        assert_eq!(intent.name, Some("analyze_user_sentiment".to_string()));
        assert!(intent.goal.contains("sentiment"));
    }

    #[tokio::test]
    async fn test_dummy_arbiter_optimization() {
        let intent_graph = Arc::new(Mutex::new(IntentGraph::new_async(IntentGraphConfig::default()).await.unwrap()));
        let config = ArbiterConfig::default();
        let arbiter = DummyArbiter::new(config, intent_graph);

        let intent = arbiter
            .natural_language_to_intent("Optimize system performance", None)
            .await
            .unwrap();

        assert_eq!(intent.name, Some("optimize_response_time".to_string()));
        assert!(intent.goal.contains("performance"));
    }

    #[tokio::test]
    async fn test_dummy_arbiter_plan_generation() {
        let intent_graph = Arc::new(Mutex::new(IntentGraph::new_async(IntentGraphConfig::default()).await.unwrap()));
        let config = ArbiterConfig::default();
        let arbiter = DummyArbiter::new(config, intent_graph);

        let intent = arbiter
            .natural_language_to_intent("Analyze sentiment", None)
            .await
            .unwrap();

        let plan = arbiter.intent_to_plan(&intent).await.unwrap();

        if let PlanBody::Rtfs(body) = &plan.body {
            assert!(body.contains(":ccos.echo"));
            assert!(body.contains("Analyze Sentiment"));
        } else {
            panic!("Plan body should be RTFS");
        }
    }

    #[tokio::test]
    async fn test_dummy_arbiter_plan_validation() {
        let intent_graph = Arc::new(Mutex::new(IntentGraph::new_async(IntentGraphConfig::default()).await.unwrap()));
        let mut config = ArbiterConfig::default();
        config.security_config.max_plan_complexity = 1;
        let arbiter = DummyArbiter::new(config, intent_graph);

        let intent = arbiter
            .natural_language_to_intent("Analyze sentiment", None)
            .await
            .unwrap();

        // This should fail because the sentiment plan has 3 steps but max complexity is 1
        let result = arbiter.intent_to_plan(&intent).await;
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("too complex"));
    }

    #[tokio::test]
    async fn test_dummy_arbiter_execution() {
        let intent_graph = Arc::new(Mutex::new(IntentGraph::new_async(IntentGraphConfig::default()).await.unwrap()));
        let config = ArbiterConfig::default();
        let arbiter = DummyArbiter::new(config, intent_graph);

        let intent = arbiter
            .natural_language_to_intent("Hello", None)
            .await
            .unwrap();

        let plan = arbiter.intent_to_plan(&intent).await.unwrap();
        let result = arbiter.execute_plan(&plan).await.unwrap();

        assert!(result.success);
        if let Value::String(msg) = &result.value {
            assert!(msg.contains("completed successfully"));
        } else {
            panic!("Expected string value");
        }
    }
}
