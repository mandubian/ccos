use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use regex::Regex;
use serde_json;

use crate::runtime::error::RuntimeError;
use crate::runtime::values::Value;

use super::arbiter_config::ArbiterConfig;
use super::arbiter_engine::ArbiterEngine;
use super::plan_generation::{
    PlanGenerationProvider, PlanGenerationResult, StubPlanGenerationProvider,
};
use crate::ccos::capability_marketplace::CapabilityMarketplace;
use crate::ccos::intent_graph::IntentGraph;
use crate::ccos::types::{
    ExecutionResult, GenerationContext, Intent, IntentId, IntentStatus, Plan, PlanBody,
    PlanLanguage, PlanStatus, StorableIntent, TriggerSource,
};
use tokio::sync::RwLock;

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
        // Check compound patterns first (more specific)
        if (lower_nl.contains("hello") || lower_nl.contains("hi") || lower_nl.contains("greet"))
            && (lower_nl.contains("add")
                || lower_nl.contains("sum")
                || lower_nl.contains("plus")
                || lower_nl.contains("calculate")
                || lower_nl.contains("math"))
        {
            // Compound goal: greeting + math operation
            let numbers: Vec<i64> = Regex::new(r"\b(\d+)\b")
                .unwrap()
                .captures_iter(nl)
                .filter_map(|cap| cap.get(1)?.as_str().parse().ok())
                .collect();

            let mut metadata = HashMap::new();
            if !numbers.is_empty() {
                // Store numbers as an RTFS-like vector literal so prompts expecting RTFS see a native form
                let vec_literal = format!(
                    "[{}]",
                    numbers
                        .iter()
                        .map(|n| n.to_string())
                        .collect::<Vec<_>>()
                        .join(" ")
                );
                metadata.insert("numbers".to_string(), Value::String(vec_literal));
            }

            Intent {
                intent_id: format!("dummy_compound_{}", uuid::Uuid::new_v4()),
                name: Some("greet_and_calculate".to_string()),
                goal: "Greet user and perform mathematical calculation".to_string(),
                original_request: nl.to_string(),
                constraints: HashMap::new(),
                preferences: {
                    let mut map = HashMap::new();
                    map.insert(
                        "friendliness".to_string(),
                        Value::String("high".to_string()),
                    );
                    map.insert("precision".to_string(), Value::String("exact".to_string()));
                    map
                },
                success_criteria: Some(Value::String(
                    "greeting_and_calculation_completed".to_string(),
                )),
                status: IntentStatus::Active,
                created_at: std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap()
                    .as_secs(),
                updated_at: std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap()
                    .as_secs(),
                metadata,
            }
        } else if lower_nl.contains("sentiment")
            || lower_nl.contains("analyze")
            || lower_nl.contains("feeling")
        {
            // Sentiment / analysis intent
            let numbers: Vec<i64> = Regex::new(r"\b(\d+)\b")
                .unwrap()
                .captures_iter(nl)
                .filter_map(|cap| cap.get(1)?.as_str().parse().ok())
                .collect();

            let mut metadata = HashMap::new();
            if !numbers.is_empty() {
                let vec_literal = format!(
                    "[{}]",
                    numbers
                        .iter()
                        .map(|n| n.to_string())
                        .collect::<Vec<_>>()
                        .join(" ")
                );
                metadata.insert("numbers".to_string(), Value::String(vec_literal));
            }

            Intent {
                intent_id: format!("dummy_sentiment_{}", uuid::Uuid::new_v4()),
                name: Some("analyze_user_sentiment".to_string()),
                goal: "Analyze user sentiment".to_string(),
                original_request: nl.to_string(),
                constraints: HashMap::new(),
                preferences: {
                    let mut map = HashMap::new();
                    map.insert(
                        "sensitivity".to_string(),
                        Value::String("medium".to_string()),
                    );
                    map
                },
                success_criteria: Some(Value::String("sentiment_analysis_completed".to_string())),
                status: IntentStatus::Active,
                created_at: std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap()
                    .as_secs(),
                updated_at: std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap()
                    .as_secs(),
                metadata,
            }
        } else {
            // Other patterns: check for optimization keywords
            if lower_nl.contains("optimiz")
                || lower_nl.contains("performance")
                || lower_nl.contains("latency")
            {
                Intent {
                    intent_id: format!("dummy_optimize_{}", uuid::Uuid::new_v4()),
                    name: Some("optimize_response_time".to_string()),
                    goal: "Optimize system performance".to_string(),
                    original_request: nl.to_string(),
                    constraints: HashMap::new(),
                    preferences: {
                        let mut map = HashMap::new();
                        map.insert(
                            "priority".to_string(),
                            Value::String("performance".to_string()),
                        );
                        map
                    },
                    success_criteria: Some(Value::String("optimization_applied".to_string())),
                    status: IntentStatus::Active,
                    created_at: std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .unwrap()
                        .as_secs(),
                    updated_at: std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .unwrap()
                        .as_secs(),
                    metadata: HashMap::new(),
                }
            } else if lower_nl.contains("add")
                || lower_nl.contains("sum")
                || lower_nl.contains("plus")
                || lower_nl.contains("calculate")
                || lower_nl.contains("math")
            {
                // Math-only requests
                let numbers: Vec<i64> = Regex::new(r"\b(\d+)\b")
                    .unwrap()
                    .captures_iter(nl)
                    .filter_map(|cap| cap.get(1)?.as_str().parse().ok())
                    .collect();

                let mut metadata = HashMap::new();
                if !numbers.is_empty() {
                    metadata.insert(
                        "numbers".to_string(),
                        Value::String(format!(
                            "[{}]",
                            numbers
                                .iter()
                                .map(|n| n.to_string())
                                .collect::<Vec<_>>()
                                .join(" ")
                        )),
                    );
                }

                Intent {
                    intent_id: format!("dummy_math_{}", uuid::Uuid::new_v4()),
                    name: Some("perform_math_operation".to_string()),
                    goal: "Perform mathematical calculation".to_string(),
                    original_request: nl.to_string(),
                    constraints: HashMap::new(),
                    preferences: {
                        let mut map = HashMap::new();
                        map.insert("precision".to_string(), Value::String("exact".to_string()));
                        map
                    },
                    success_criteria: Some(Value::String("calculation_completed".to_string())),
                    status: IntentStatus::Active,
                    created_at: std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .unwrap()
                        .as_secs(),
                    updated_at: std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .unwrap()
                        .as_secs(),
                    metadata,
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
                    status: IntentStatus::Active,
                    created_at: std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .unwrap()
                        .as_secs(),
                    updated_at: std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .unwrap()
                        .as_secs(),
                    metadata: HashMap::new(),
                }
            }
        }
    }

    /// Generate a deterministic RTFS plan based on the intent.
    fn generate_dummy_plan(&self, intent: &Intent) -> Plan {
        let (plan_body, capabilities_required) = match intent.name.as_deref() {
            Some("analyze_user_sentiment") => (
                r#"
(do
    (step "Fetch Data" (call :ccos.echo "fetched user interactions"))
    (step "Analyze Sentiment" (call :ccos.echo "sentiment: positive"))
    (step "Generate Report" (call :ccos.echo "report generated"))
)
"#
                .to_string(),
                vec!["ccos.echo".to_string()],
            ),
            Some("optimize_response_time") => (
                r#"
(do
    (step "Get Metrics" (call :ccos.echo "metrics collected"))
    (step "Identify Bottlenecks" (call :ccos.echo "bottlenecks identified"))
    (step "Apply Optimizations" (call :ccos.echo "optimizations applied"))
)
"#
                .to_string(),
                vec!["ccos.echo".to_string()],
            ),
            Some("greet_user") => (
                r#"
(do
    (step "Generate Greeting" (call :ccos.echo "Hello! How can I help you today?"))
)
"#
                .to_string(),
                vec!["ccos.echo".to_string()],
            ),
            Some("greet_and_calculate") => {
                // Extract numbers from metadata
                let numbers_str = match intent.metadata.get("numbers") {
                    Some(Value::String(s)) => s.clone(),
                    _ => "[]".to_string(),
                };
                let numbers: Vec<i64> = serde_json::from_str(&numbers_str).unwrap_or_default();

                if numbers.len() >= 2 {
                    // Generate compound plan: greet + add numbers
                    let a = numbers[0];
                    let b = numbers[1];
                    (
                        format!(
                            r#"
(do
    (step "Generate Greeting" (call :ccos.echo "Hello! Let me help you with that calculation."))
    (step "Perform Addition" (call :ccos.math.add {{:args [{} {}]}}))
    (step "Display Result" (call :ccos.echo "The result is: "))
)
"#,
                            a, b
                        ),
                        vec!["ccos.echo".to_string(), "ccos.math.add".to_string()],
                    )
                } else {
                    // Fallback if not enough numbers
                    (
                        r#"
(do
    (step "Generate Greeting" (call :ccos.echo "Hello!"))
    (step "Handle Math Request" (call :ccos.echo "Please provide numbers to add"))
)
"#
                        .to_string(),
                        vec!["ccos.echo".to_string()],
                    )
                }
            }
            Some("perform_math_operation") => {
                // Extract numbers from metadata
                let numbers_str = match intent.metadata.get("numbers") {
                    Some(Value::String(s)) => s.clone(),
                    _ => "[]".to_string(),
                };
                let numbers: Vec<i64> = serde_json::from_str(&numbers_str).unwrap_or_default();

                if numbers.len() >= 2 {
                    // Generate compound plan: add numbers and return result
                    let a = numbers[0];
                    let b = numbers[1];
                    (
                        format!(
                            r#"
(do
    (let [result (call :ccos.math.add {{:args [{} {}]}})]
    (call :ccos.echo (str "The result of adding {} and {} is: " result)))
)
"#,
                            a, b, a, b
                        ),
                        vec!["ccos.math.add".to_string(), "ccos.echo".to_string()],
                    )
                } else {
                    // Fallback if not enough numbers
                    (
                        r#"
(do
    (step "Handle Math Request" (call :ccos.echo "Please provide at least two numbers to add"))
)
"#
                        .to_string(),
                        vec!["ccos.echo".to_string()],
                    )
                }
            }
            _ => (
                // Default plan for general assistance
                r#"
(do
    (step "Process Request" (call :ccos.echo "processing your request"))
    (step "Provide Response" (call :ccos.echo "here is your response"))
)
"#
                .to_string(),
                vec!["ccos.echo".to_string()],
            ),
        };

        Plan {
            plan_id: format!("dummy_plan_{}", uuid::Uuid::new_v4()),
            name: intent.name.clone(),
            intent_ids: vec![intent.intent_id.clone()],
            language: PlanLanguage::Rtfs20,
            body: PlanBody::Rtfs(plan_body.trim().to_string()),
            status: PlanStatus::Draft,
            created_at: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_secs(),
            metadata: HashMap::new(),
            input_schema: None,
            output_schema: None,
            policies: HashMap::new(),
            capabilities_required,
            annotations: HashMap::new(),
        }
    }

    /// Store the intent in the intent graph.
    fn store_intent(&self, intent: &Intent) -> Result<(), RuntimeError> {
        let mut graph = self
            .intent_graph
            .lock()
            .map_err(|_| RuntimeError::Generic("Failed to lock IntentGraph".to_string()))?;

        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs();
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
            triggered_by: TriggerSource::HumanRequest,
            generation_context: GenerationContext {
                arbiter_version: "dummy-1.0".to_string(),
                generation_timestamp: now,
                input_context: HashMap::new(),
                reasoning_trace: Some("Dummy arbiter deterministic generation".to_string()),
            },
            status: IntentStatus::Active,
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
                    step_count, self.config.security_config.max_plan_complexity
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
                if !self
                    .config
                    .security_config
                    .allowed_capability_prefixes
                    .is_empty()
                {
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

    /// Build a tiny demo graph: root → fetch → analyze → announce
    fn build_demo_graph(&self, goal: &str) -> Result<IntentId, RuntimeError> {
        let mut graph = self
            .intent_graph
            .lock()
            .map_err(|_| RuntimeError::Generic("Failed to lock IntentGraph".to_string()))?;

        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs();
        let mk = |name: &str, goal: &str| StorableIntent {
            intent_id: format!("intent-{}", uuid::Uuid::new_v4()),
            name: Some(name.to_string()),
            original_request: goal.to_string(),
            rtfs_intent_source: format!("(intent {} :goal \"{}\")", name, goal),
            goal: goal.to_string(),
            constraints: HashMap::new(),
            preferences: HashMap::new(),
            success_criteria: None,
            parent_intent: None,
            child_intents: vec![],
            triggered_by: TriggerSource::HumanRequest,
            generation_context: GenerationContext {
                arbiter_version: "dummy-graph-1.0".to_string(),
                generation_timestamp: now,
                input_context: HashMap::new(),
                reasoning_trace: Some("programmatic demo graph".to_string()),
            },
            status: IntentStatus::Active,
            priority: 1,
            created_at: now,
            updated_at: now,
            metadata: HashMap::new(),
        };

        let root = mk("root_objective", goal);
        let fetch = mk("fetch_data", "Fetch data");
        let analyze = mk("analyze_data", "Analyze fetched data");
        let announce = mk("announce_results", "Announce the analysis result");

        let root_id = root.intent_id.clone();
        let fetch_id = fetch.intent_id.clone();
        let analyze_id = analyze.intent_id.clone();
        let announce_id = announce.intent_id.clone();

        graph.store_intent(root)?;
        graph.store_intent(fetch)?;
        graph.store_intent(analyze)?;
        graph.store_intent(announce)?;

        use crate::ccos::types::EdgeType;
        graph.create_edge(fetch_id.clone(), root_id.clone(), EdgeType::IsSubgoalOf)?;
        graph.create_edge(analyze_id.clone(), root_id.clone(), EdgeType::IsSubgoalOf)?;
        graph.create_edge(announce_id.clone(), root_id.clone(), EdgeType::IsSubgoalOf)?;
        graph.create_edge(analyze_id.clone(), fetch_id.clone(), EdgeType::DependsOn)?;
        graph.create_edge(announce_id.clone(), analyze_id.clone(), EdgeType::DependsOn)?;

        Ok(root_id)
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

    async fn intent_to_plan(&self, intent: &Intent) -> Result<Plan, RuntimeError> {
        let plan = self.generate_dummy_plan(intent);

        // Validate the plan
        self.validate_plan(&plan)?;

        Ok(plan)
    }

    async fn execute_plan(&self, _plan: &Plan) -> Result<ExecutionResult, RuntimeError> {
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

    async fn natural_language_to_graph(
        &self,
        natural_language_goal: &str,
    ) -> Result<IntentId, RuntimeError> {
        // Try using RTFS graph interpreter for a deterministic tiny graph; fallback to programmatic
        let rtfs = format!(
            r#"(do
  (intent "root_objective" {{:goal "{}"}})
  (intent "fetch_data" {{:goal "Fetch data"}})
  (intent "analyze_data" {{:goal "Analyze fetched data"}})
  (intent "announce_results" {{:goal "Announce the analysis result"}})
  (edge :IsSubgoalOf "fetch_data" "root_objective")
  (edge :IsSubgoalOf "analyze_data" "root_objective")
  (edge :IsSubgoalOf "announce_results" "root_objective")
  (edge :DependsOn "analyze_data" "fetch_data")
  (edge :DependsOn "announce_results" "analyze_data"))"#,
            natural_language_goal.replace('"', "\"")
        );

        let mut g = self
            .intent_graph
            .lock()
            .map_err(|_| RuntimeError::Generic("Failed to lock IntentGraph".to_string()))?;
        match crate::ccos::rtfs_bridge::graph_interpreter::build_graph_from_rtfs(&rtfs, &mut g) {
            Ok(root_id) => Ok(root_id),
            Err(_) => {
                // Fallback to legacy programmatic builder
                drop(g);
                self.build_demo_graph(natural_language_goal)
            }
        }
    }

    async fn generate_plan_for_intent(
        &self,
        intent: &StorableIntent,
    ) -> Result<PlanGenerationResult, RuntimeError> {
        // Use the stub plan generator; marketplace isn't strictly needed for the stub
        let provider = StubPlanGenerationProvider;
        // Create a minimal marketplace placeholder for signature compatibility if needed later
        let plan_res = provider
            .generate_plan(
                &Intent {
                    intent_id: intent.intent_id.clone(),
                    name: intent.name.clone(),
                    original_request: intent.original_request.clone(),
                    goal: intent.goal.clone(),
                    constraints: HashMap::new(),
                    preferences: HashMap::new(),
                    success_criteria: None,
                    status: IntentStatus::Active,
                    created_at: intent.created_at,
                    updated_at: intent.updated_at,
                    metadata: HashMap::new(),
                },
                Arc::new(CapabilityMarketplace::new(Arc::new(RwLock::new(
                    crate::ccos::capabilities::registry::CapabilityRegistry::new(),
                )))),
            )
            .await?;
        Ok(plan_res)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ccos::intent_graph::IntentGraphConfig;

    #[tokio::test]
    async fn test_dummy_arbiter_sentiment() {
        let intent_graph = Arc::new(Mutex::new(
            IntentGraph::new_async(IntentGraphConfig::default())
                .await
                .unwrap(),
        ));
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
        let intent_graph = Arc::new(Mutex::new(
            IntentGraph::new_async(IntentGraphConfig::default())
                .await
                .unwrap(),
        ));
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
        let intent_graph = Arc::new(Mutex::new(
            IntentGraph::new_async(IntentGraphConfig::default())
                .await
                .unwrap(),
        ));
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
        let intent_graph = Arc::new(Mutex::new(
            IntentGraph::new_async(IntentGraphConfig::default())
                .await
                .unwrap(),
        ));
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
        let intent_graph = Arc::new(Mutex::new(
            IntentGraph::new_async(IntentGraphConfig::default())
                .await
                .unwrap(),
        ));
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
