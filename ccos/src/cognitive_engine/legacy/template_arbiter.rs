//! Template-based Arbiter Engine
//!
//! This module provides a template-driven approach to intent and plan generation
//! using pattern matching and predefined RTFS templates. This is useful for
//! common, well-defined tasks that don't require LLM reasoning.

use async_trait::async_trait;
use regex::Regex;
use std::collections::HashMap;

use crate::cognitive_engine::config::{IntentPattern, PlanTemplate, TemplateConfig};
use crate::cognitive_engine::engine::CognitiveEngine;
use crate::types::{
    Intent, IntentStatus, Plan, PlanBody, PlanLanguage, PlanStatus, StorableIntent,
};
use rtfs::runtime::error::RuntimeError;
use rtfs::runtime::values::Value;

/// Template-based arbiter that uses pattern matching and predefined templates
pub struct TemplateArbiter {
    #[allow(dead_code)]
    config: TemplateConfig,
    intent_patterns: Vec<IntentPattern>,
    plan_templates: Vec<PlanTemplate>,
    intent_graph: std::sync::Arc<std::sync::Mutex<crate::types::IntentGraph>>,
}

impl TemplateArbiter {
    /// Create a new template arbiter with the given configuration
    pub fn new(
        config: TemplateConfig,
        intent_graph: std::sync::Arc<std::sync::Mutex<crate::types::IntentGraph>>,
    ) -> Result<Self, RuntimeError> {
        // Load intent patterns and plan templates from configuration
        let intent_patterns = config.intent_patterns.clone();
        let plan_templates = config.plan_templates.clone();

        Ok(Self {
            config,
            intent_patterns,
            plan_templates,
            intent_graph,
        })
    }

    /// Match natural language input against intent patterns
    fn match_intent_pattern(&self, natural_language: &str) -> Option<&IntentPattern> {
        let lower_nl = natural_language.to_lowercase();

        for pattern in &self.intent_patterns {
            // Check regex pattern
            if let Ok(regex) = Regex::new(&pattern.pattern) {
                if regex.is_match(&lower_nl) {
                    return Some(pattern);
                }
            }
        }

        None
    }

    /// Find a plan template that matches the given intent
    fn find_plan_template(&self, intent_name: &str) -> Option<&PlanTemplate> {
        // For now, we'll use the first template that has the intent name in its variables
        // In a real implementation, we'd have a more sophisticated matching system
        for template in &self.plan_templates {
            if template.variables.contains(&intent_name.to_string()) {
                return Some(template);
            }
        }

        None
    }

    /// Generate intent from pattern match
    fn generate_intent_from_pattern(
        &self,
        pattern: &IntentPattern,
        natural_language: &str,
        context: Option<HashMap<String, Value>>,
    ) -> Intent {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs();

        // Convert context to string format for template substitution
        let context_str = context.map(|ctx| {
            ctx.into_iter()
                .map(|(k, v)| (k, format!("{}", v)))
                .collect::<HashMap<String, String>>()
        });

        // Apply template substitution to goal
        let mut goal = pattern.goal_template.clone();
        if let Some(ctx) = &context_str {
            for (key, value) in ctx {
                goal = goal.replace(&format!("{{{}}}", key), value);
            }
        }

        Intent {
            intent_id: format!("template_intent_{}", uuid::Uuid::new_v4()),
            name: Some(pattern.intent_name.clone()),
            original_request: natural_language.to_string(),
            goal,
            constraints: {
                let mut map = HashMap::new();
                for constraint in &pattern.constraints {
                    map.insert(constraint.clone(), Value::String(constraint.clone()));
                }
                map
            },
            preferences: {
                let mut map = HashMap::new();
                for preference in &pattern.preferences {
                    map.insert(preference.clone(), Value::String(preference.clone()));
                }
                map
            },
            success_criteria: None,
            status: IntentStatus::Active,
            created_at: now,
            updated_at: now,
            metadata: HashMap::new(),
        }
    }

    /// Generate plan from template
    fn generate_plan_from_template(
        &self,
        template: &PlanTemplate,
        intent: &Intent,
        context: Option<HashMap<String, Value>>,
    ) -> Plan {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs();

        // Convert context to string format for template substitution
        let context_str = context.map(|ctx| {
            ctx.into_iter()
                .map(|(k, v)| (k, format!("{}", v)))
                .collect::<HashMap<String, String>>()
        });

        // Apply template substitution to RTFS content
        let mut rtfs_content = template.rtfs_template.clone();

        // Substitute intent variables
        rtfs_content = rtfs_content.replace("{intent_id}", &intent.intent_id);
        rtfs_content = rtfs_content.replace(
            "{intent_name}",
            intent.name.as_ref().unwrap_or(&"".to_string()),
        );
        rtfs_content = rtfs_content.replace("{goal}", &intent.goal);

        // Substitute context variables
        if let Some(ctx) = &context_str {
            for (key, value) in ctx {
                rtfs_content = rtfs_content.replace(&format!("{{{}}}", key), value);
            }
        }

        Plan {
            plan_id: format!("template_plan_{}", uuid::Uuid::new_v4()),
            name: Some(template.name.clone()),
            intent_ids: vec![intent.intent_id.clone()],
            language: PlanLanguage::Rtfs20,
            body: PlanBody::Rtfs(rtfs_content),
            status: PlanStatus::Draft,
            created_at: now,
            metadata: HashMap::new(),
            input_schema: None,
            output_schema: None,
            policies: HashMap::new(),
            capabilities_required: vec![],
            annotations: HashMap::new(),
        }
    }

    /// Store intent in the intent graph
    async fn store_intent(&self, intent: &Intent) -> Result<(), RuntimeError> {
        let mut graph = self
            .intent_graph
            .lock()
            .map_err(|_| RuntimeError::Generic("Failed to lock intent graph".to_string()))?;

        // Convert to storable intent
        let storable = StorableIntent {
            intent_id: intent.intent_id.clone(),
            name: intent.name.clone(),
            original_request: intent.original_request.clone(),
            rtfs_intent_source: "template_generated".to_string(),
            goal: intent.goal.clone(),
            constraints: intent
                .constraints
                .iter()
                .map(|(k, v)| (k.clone(), format!("{}", v)))
                .collect(),
            preferences: intent
                .preferences
                .iter()
                .map(|(k, v)| (k.clone(), format!("{}", v)))
                .collect(),
            success_criteria: intent.success_criteria.as_ref().map(|v| format!("{}", v)),
            parent_intent: None,
            child_intents: vec![],
            triggered_by: crate::types::TriggerSource::HumanRequest,
            generation_context: crate::types::GenerationContext {
                arbiter_version: "1.0.0".to_string(),
                generation_timestamp: intent.created_at,
                input_context: HashMap::new(),
                reasoning_trace: Some("Pattern matched template".to_string()),
            },
            status: intent.status.clone(),
            priority: 1,
            created_at: intent.created_at,
            updated_at: intent.updated_at,
            metadata: HashMap::new(),
        };

        graph
            .storage
            .store_intent(storable)
            .await
            .map_err(|e| RuntimeError::Generic(format!("Failed to store intent: {}", e)))?;

        Ok(())
    }
}

#[async_trait(?Send)]
impl CognitiveEngine for TemplateArbiter {
    async fn natural_language_to_intent(
        &self,
        natural_language: &str,
        context: Option<HashMap<String, Value>>,
    ) -> Result<Intent, RuntimeError> {
        // Try to match against intent patterns
        if let Some(pattern) = self.match_intent_pattern(natural_language) {
            let intent = self.generate_intent_from_pattern(pattern, natural_language, context);

            // Store the intent
            self.store_intent(&intent).await?;

            Ok(intent)
        } else {
            // No pattern match found
            Err(RuntimeError::Generic(format!(
                "No template pattern found for request: '{}'",
                natural_language
            )))
        }
    }

    async fn intent_to_plan(&self, intent: &Intent) -> Result<Plan, RuntimeError> {
        let intent_name = intent
            .name
            .as_ref()
            .ok_or_else(|| RuntimeError::Generic("Intent has no name".to_string()))?;

        // Find matching plan template
        if let Some(template) = self.find_plan_template(intent_name) {
            Ok(self.generate_plan_from_template(template, intent, None))
        } else {
            Err(RuntimeError::Generic(format!(
                "No plan template found for intent: '{}'",
                intent_name
            )))
        }
    }

    async fn execute_plan(
        &self,
        plan: &Plan,
    ) -> Result<crate::types::ExecutionResult, RuntimeError> {
        // For template arbiter, we return a placeholder execution result
        // In a real implementation, this would execute the RTFS plan
        Ok(crate::types::ExecutionResult {
            success: true,
            value: Value::String("Template arbiter execution placeholder".to_string()),
            metadata: {
                let mut meta = HashMap::new();
                meta.insert("plan_id".to_string(), Value::String(plan.plan_id.clone()));
                meta.insert(
                    "template_engine".to_string(),
                    Value::String("template".to_string()),
                );
                meta
            },
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cognitive_engine::config::{
        FallbackBehavior, IntentPattern, PlanTemplate, TemplateConfig,
    };

    fn create_test_config() -> TemplateConfig {
        TemplateConfig {
            intent_patterns: vec![
                IntentPattern {
                    name: "sentiment_analysis".to_string(),
                    pattern: r"(?i)analyze.*sentiment|sentiment.*analysis".to_string(),
                    intent_name: "analyze_sentiment".to_string(),
                    goal_template: "Analyze user sentiment from {source}".to_string(),
                    constraints: vec!["accuracy".to_string()],
                    preferences: vec!["speed".to_string()],
                },
                IntentPattern {
                    name: "backup_operation".to_string(),
                    pattern: r"(?i)backup|save.*data|protect.*data".to_string(),
                    intent_name: "backup_data".to_string(),
                    goal_template: "Create backup of {data_type} data".to_string(),
                    constraints: vec!["encryption".to_string()],
                    preferences: vec!["compression".to_string()],
                },
            ],
            plan_templates: vec![
                PlanTemplate {
                    name: "sentiment_analysis_plan".to_string(),
                    rtfs_template: r#"
(do
    (step "Fetch Data" (call :ccos.echo "fetching {source} data"))
    (step "Analyze Sentiment" (call :ccos.echo "analyzing sentiment"))
    (step "Generate Report" (call :ccos.echo "generating sentiment report"))
)
                    "#
                    .trim()
                    .to_string(),
                    variables: vec!["analyze_sentiment".to_string(), "source".to_string()],
                },
                PlanTemplate {
                    name: "backup_plan".to_string(),
                    rtfs_template: r#"
(do
    (step "Validate Data" (call :ccos.echo "validating {data_type} data"))
    (step "Create Backup" (call :ccos.echo "creating encrypted backup"))
    (step "Verify Backup" (call :ccos.echo "verifying backup integrity"))
)
                    "#
                    .trim()
                    .to_string(),
                    variables: vec!["backup_data".to_string(), "data_type".to_string()],
                },
            ],
            fallback: FallbackBehavior::Error,
        }
    }

    #[tokio::test]
    async fn test_template_arbiter_creation() {
        let config = create_test_config();
        let intent_graph = std::sync::Arc::new(std::sync::Mutex::new(
            crate::types::IntentGraph::new().unwrap(),
        ));

        let arbiter = TemplateArbiter::new(config, intent_graph);
        assert!(arbiter.is_ok());
    }

    #[tokio::test]
    async fn test_intent_pattern_matching() {
        let config = create_test_config();
        let intent_graph = std::sync::Arc::new(std::sync::Mutex::new(
            crate::types::IntentGraph::new().unwrap(),
        ));

        let arbiter = TemplateArbiter::new(config, intent_graph).unwrap();

        // Test sentiment analysis pattern
        let pattern = arbiter.match_intent_pattern("analyze user sentiment from chat logs");
        assert!(pattern.is_some());
        {
            let p = pattern.unwrap();
            assert_eq!(p.intent_name.clone(), "analyze_sentiment");
        }

        // Test backup pattern
        let pattern = arbiter.match_intent_pattern("create backup of database");
        assert!(pattern.is_some());
        {
            let p = pattern.unwrap();
            assert_eq!(p.intent_name.clone(), "backup_data");
        }

        // Test no match
        let pattern = arbiter.match_intent_pattern("random request");
        assert!(pattern.is_none());
    }

    #[tokio::test]
    async fn test_intent_generation() {
        let config = create_test_config();
        let intent_graph = std::sync::Arc::new(std::sync::Mutex::new(
            crate::types::IntentGraph::new().unwrap(),
        ));

        let arbiter = TemplateArbiter::new(config, intent_graph).unwrap();

        let mut context = HashMap::new();
        context.insert("source".to_string(), Value::String("chat_logs".to_string()));

        let intent = arbiter
            .natural_language_to_intent("analyze user sentiment from chat logs", Some(context))
            .await
            .unwrap();

        assert!(intent.name.is_some() && intent.name.as_ref().unwrap().contains("sentiment"));
        assert!(intent.goal.contains("chat_logs"));
        assert!(intent.constraints.contains_key("accuracy"));
    }

    #[tokio::test]
    async fn test_plan_generation() {
        let config = create_test_config();
        let intent_graph = std::sync::Arc::new(std::sync::Mutex::new(
            crate::types::IntentGraph::new().unwrap(),
        ));

        let arbiter = TemplateArbiter::new(config, intent_graph).unwrap();

        let intent = Intent {
            intent_id: "test_intent".to_string(),
            name: Some("analyze_sentiment".to_string()),
            original_request: "test".to_string(),
            goal: "test goal".to_string(),
            constraints: HashMap::new(),
            preferences: HashMap::new(),
            success_criteria: None,
            status: IntentStatus::Active,
            created_at: 0,
            updated_at: 0,
            metadata: HashMap::new(),
        };

        let plan = arbiter.intent_to_plan(&intent).await.unwrap();
        assert_eq!(plan.name, Some("sentiment_analysis_plan".to_string()));
        assert!(matches!(plan.body, PlanBody::Rtfs(_)));
    }
}
