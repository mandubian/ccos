//! Core CCOS Types
//!
//! This module defines the fundamental data structures for the Cognitive Computing
//! Operating System, based on the CCOS specifications.

use crate::runtime::values::Value;
use std::collections::HashMap;
use std::time::{SystemTime, UNIX_EPOCH};
use uuid::Uuid;

// --- ID Type Aliases ---
pub type IntentId = String;
pub type PlanId = String;
pub type ActionId = String;
pub type CapabilityId = String;

// --- Intent Graph (SEP-001) ---

/// Represents the "why" behind all system behavior. A node in the Intent Graph.
#[derive(Debug, Clone)]
pub struct Intent {
    pub intent_id: IntentId,
    pub name: Option<String>,
    pub original_request: String,
    pub goal: String,
    pub constraints: HashMap<String, Value>,
    pub preferences: HashMap<String, Value>,
    pub success_criteria: Option<Value>, // RTFS expression
    pub status: IntentStatus,
    pub created_at: u64,
    pub updated_at: u64,
    pub metadata: HashMap<String, Value>,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum IntentStatus {
    Active,
    Completed,
    Failed,
    Archived,
    Suspended,
}

/// Defines the relationship between two Intents.
#[derive(Debug, Clone)]
pub struct IntentEdge {
    pub from_intent: IntentId,
    pub to_intent: IntentId,
    pub edge_type: EdgeType,
    pub weight: Option<f64>,
    pub metadata: HashMap<String, Value>,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum EdgeType {
    DependsOn,
    IsSubgoalOf,
    ConflictsWith,
    Enables,
    RelatedTo,
}

// --- Plans and Orchestration (SEP-002) ---

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum PlanLanguage {
    Rtfs20,
    // Future language support - currently unused but keeps the door open
    #[allow(dead_code)]
    Wasm,
    #[allow(dead_code)]
    Python,
    #[allow(dead_code)]
    GraphJson,
    #[allow(dead_code)]
    Other(String),
}

/// Represents the "how" for achieving an Intent. An immutable, archivable script.
#[derive(Debug, Clone)]
pub struct Plan {
    pub plan_id: PlanId,
    pub name: Option<String>,
    pub intent_ids: Vec<IntentId>,
    pub language: PlanLanguage,
    pub body: PlanBody, // Flexible body representation
    pub created_at: u64,
    pub metadata: HashMap<String, Value>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum PlanBody {
    Rtfs(String),     // RTFS source code
    Wasm(Vec<u8>),    // compiled WASM bytecode
}

// --- Causal Chain (SEP-003) ---

/// Represents an immutable record in the Causal Chain - the "what happened".
#[derive(Debug, Clone)]
pub struct Action {
    pub action_id: ActionId,
    pub parent_action_id: Option<ActionId>,
    pub plan_id: PlanId,
    pub intent_id: IntentId,
    pub action_type: ActionType,
    pub function_name: Option<String>, // e.g., step name, capability id
    pub arguments: Option<Vec<Value>>,
    pub result: Option<ExecutionResult>,
    pub cost: Option<f64>,
    pub duration_ms: Option<u64>,
    pub timestamp: u64,
    pub metadata: HashMap<String, Value>, // For attestations, rule IDs, etc.
}

/// Categorizes the type of event being recorded in the Causal Chain.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum ActionType {
    PlanStarted,
    PlanCompleted,
    PlanAborted,
    PlanPaused,
    PlanResumed,
    PlanStepStarted,
    PlanStepCompleted,
    PlanStepFailed,
    PlanStepRetrying,
    CapabilityCall,
    InternalStep,
}

/// Represents the outcome of an executed action or plan.
#[derive(Debug, Clone)]
pub struct ExecutionResult {
    pub success: bool,
    pub value: Value,
    pub metadata: HashMap<String, Value>,
}

// --- Implementations ---

impl Intent {
    pub fn new(goal: String) -> Self {
        let now = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_secs();
        Self {
            intent_id: format!("intent-{}", Uuid::new_v4()),
            name: None,
            original_request: goal.clone(),
            goal,
            constraints: HashMap::new(),
            preferences: HashMap::new(),
            success_criteria: None,
            status: IntentStatus::Active,
            created_at: now,
            updated_at: now,
            metadata: HashMap::new(),
        }
    }

    pub fn with_name(mut self, name: String) -> Self {
        self.name = Some(name);
        self
    }
}

impl Plan {
    pub fn new_rtfs(rtfs_code: String, intent_ids: Vec<IntentId>) -> Self {
        Self {
            plan_id: format!("plan-{}", Uuid::new_v4()),
            name: None,
            intent_ids,
            language: PlanLanguage::Rtfs20,
            body: PlanBody::Rtfs(rtfs_code),
            created_at: SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_secs(),
            metadata: HashMap::new(),
        }
    }

    pub fn new_named(name: String, language: PlanLanguage, body: PlanBody, intent_ids: Vec<IntentId>) -> Self {
        let mut plan = Self::new_rtfs(String::new(), intent_ids);
        plan.name = Some(name);
        plan.language = language;
        plan.body = body;
        plan
    }
}

impl Action {
    pub fn new(action_type: ActionType, plan_id: PlanId, intent_id: IntentId) -> Self {
        Self {
            action_id: format!("action-{}", Uuid::new_v4()),
            parent_action_id: None,
            plan_id,
            intent_id,
            action_type,
            function_name: None,
            arguments: None,
            result: None,
            cost: None,
            duration_ms: None,
            timestamp: SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_millis() as u64,
            metadata: HashMap::new(),
        }
    }

    pub fn with_parent(mut self, parent_id: Option<String>) -> Self {
        self.parent_action_id = parent_id;
        self
    }

    pub fn with_name(mut self, name: &str) -> Self {
        self.function_name = Some(name.to_string());
        self
    }

    pub fn with_args(mut self, args: Vec<Value>) -> Self {
        self.arguments = Some(args);
        self
    }

    pub fn with_arguments(mut self, args: &[Value]) -> Self {
        self.arguments = Some(args.to_vec());
        self
    }

    pub fn new_capability(plan_id: PlanId, intent_id: IntentId, name: &str, args: &[Value]) -> Self {
        Self::new(ActionType::CapabilityCall, plan_id, intent_id)
            .with_name(name)
            .with_arguments(args)
    }

    pub fn with_result(mut self, result: ExecutionResult) -> Self {
        self.result = Some(result);
        self
    }

    pub fn with_error(mut self, error_message: &str) -> Self {
        self.result = Some(ExecutionResult {
            success: false,
            value: Value::Nil,
            metadata: HashMap::from([("error".to_string(), Value::String(error_message.to_string()))]),
        });
        self
    }
}

impl ExecutionResult {
    pub fn with_error(mut self, error_message: &str) -> Self {
        self.success = false;
        self.metadata.insert("error".to_string(), Value::String(error_message.to_string()));
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_intent_creation() {
        let intent = Intent::new("Test goal".to_string())
            .with_constraint("max_cost".to_string(), Value::Float(100.0))
            .with_preference("priority".to_string(), Value::String("speed".to_string()));

        assert_eq!(intent.goal, "Test goal");
        assert_eq!(
            intent.constraints.get("max_cost"),
            Some(&Value::Float(100.0))
        );
        assert_eq!(
            intent.preferences.get("priority"),
            Some(&Value::String("speed".to_string()))
        );
    }

    #[test]
    fn test_plan_creation() {
        let plan = Plan::new_rtfs("(+ 1 2)".to_string(), vec!["intent-1".to_string()]);

        assert_eq!(plan.language, PlanLanguage::Rtfs20);
        assert_eq!(plan.body, PlanBody::Rtfs("(+ 1 2)".to_string()));
        assert_eq!(plan.intent_ids, vec!["intent-1"]);
    }

    #[test]
    fn test_action_creation() {
        let action = Action::new(ActionType::CapabilityCall, "plan-1".to_string(), "intent-1".to_string());

        assert_eq!(action.plan_id, "plan-1");
        assert_eq!(action.intent_id, "intent-1");
    }

    #[test]
    fn test_capability_creation() {
        let capability = Capability::new(
            "image-processing/sharpen".to_string(),
            "provider-1".to_string(),
            "(sharpen image factor)".to_string(),
        );

        assert_eq!(capability.function_name, "image-processing/sharpen");
        assert_eq!(capability.provider_id, "provider-1");
        assert_eq!(capability.status, CapabilityStatus::Active);
    }
}