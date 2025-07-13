//! Core CCOS Types
//!
//! This module defines the fundamental types for the Cognitive Computing Operating System.
//! These types represent the decoupled objects from the vision: Intent, Plan, Action, Capability, Resource.

use crate::runtime::error::RuntimeError;
use crate::runtime::values::Value;
use std::collections::HashMap;
use std::time::{SystemTime, UNIX_EPOCH};
use uuid::Uuid;

/// Unique identifier for an Intent
pub type IntentId = String;

/// Unique identifier for a Plan
pub type PlanId = String;

/// Unique identifier for an Action
pub type ActionId = String;

/// Unique identifier for a Capability
pub type CapabilityId = String;

/// Unique identifier for a Resource
pub type ResourceId = String;

/// Unique identifier for a Context
pub type ContextKey = String;

/// Unique identifier for a Task
pub type TaskId = String;

/// Represents a node in the Living Intent Graph
/// This is the persistent, addressable object representing the "why" behind a task
#[derive(Debug, Clone)]
pub struct Intent {
    /// Human-readable symbolic name (RTFS symbol) for the intent
    pub name: String,
    pub intent_id: IntentId,
    /// Original natural language request from user
    pub original_request: String,
    pub goal: String,
    pub constraints: HashMap<String, Value>,
    pub preferences: HashMap<String, Value>,
    pub success_criteria: Option<Value>, // RTFS function
    pub emotional_tone: Option<String>,
    pub parent_intent: Option<IntentId>,
    pub created_at: u64,
    pub updated_at: u64,
    pub status: IntentStatus,
    pub metadata: HashMap<String, Value>,
}

impl Intent {
    /// Create a new intent using the same string for original request and goal
    pub fn new(goal: String) -> Self {
        Self::new_with_request(goal.clone(), goal)
    }

    /// Create a new intent with explicit original natural language request and goal
    pub fn new_with_request(original_request: String, goal: String) -> Self {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs();

        Self {
            intent_id: format!("intent-{}", Uuid::new_v4()),
            name: String::new(),
            original_request,
            goal,
            constraints: HashMap::new(),
            preferences: HashMap::new(),
            success_criteria: None,
            emotional_tone: None,
            parent_intent: None,
            created_at: now,
            updated_at: now,
            status: IntentStatus::Active,
            metadata: HashMap::new(),
        }
    }

    /// Create a new intent with explicit symbolic name
    pub fn with_name(name: String, original_request: String, goal: String) -> Self {
        let mut intent = Self::new_with_request(original_request, goal);
        intent.name = name;
        intent
    }

    /// Create a new intent from natural language request
    pub fn from_natural_language(original_request: String) -> Self {
        Self::new_with_request(original_request.clone(), original_request)
    }

    pub fn with_constraint(mut self, key: String, value: Value) -> Self {
        self.constraints.insert(key, value);
        self
    }

    pub fn with_preference(mut self, key: String, value: Value) -> Self {
        self.preferences.insert(key, value);
        self
    }

    pub fn with_parent(mut self, parent_intent: IntentId) -> Self {
        self.parent_intent = Some(parent_intent);
        self
    }

    pub fn with_metadata(mut self, key: String, value: Value) -> Self {
        self.metadata.insert(key, value);
        self
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum IntentStatus {
    Active,
    Completed,
    Failed,
    Archived,
    Suspended,
}

/// Represents the current lifecycle state of a plan
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum PlanStatus {
    Running,
    Paused,
    Completed,
    Aborted,
}

/// Represents a transient but archivable RTFS script
/// This is the "how" - the concrete, executable RTFS program
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum PlanLanguage {
    Rtfs20,
    Wasm,
    Python,
    GraphJson,
    Other(String),
}

#[derive(Debug, Clone, PartialEq)]
pub enum PlanBody {
    Text(String),
    Bytes(Vec<u8>),
}

/// Represents a node in the Living Intent Graph
/// This is the persistent, addressable object representing the "why" behind a task
#[derive(Debug, Clone)]
pub struct Plan {
    /// Symbolic name for the plan (RTFS symbol or friendly label)
    pub name: String,
    pub plan_id: PlanId,
    /// Formal definition of the data this plan requires to run.
    pub input_schema: Option<Value>, // RTFS Schema
    /// Formal definition of the data this plan is guaranteed to produce.
    pub output_schema: Option<Value>, // RTFS Schema
    pub language: PlanLanguage,
    pub body: PlanBody,
    pub intent_ids: Vec<IntentId>,
    /// Lifecycle status of this plan
    pub status: PlanStatus,
    /// If this plan replaces another, keep the parent id for audit
    pub parent_plan_id: Option<PlanId>,
    /// If aborted, the index (0-based) of the step where execution stopped
    pub aborted_at_step: Option<u32>,
    pub created_at: u64,
    pub executed_at: Option<u64>,
    pub metadata: HashMap<String, Value>,
}

impl Plan {
    pub fn new(language: PlanLanguage, body: PlanBody, intent_ids: Vec<IntentId>) -> Self {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs();

        Self {
            plan_id: format!("plan-{}", Uuid::new_v4()),
            name: String::new(),
            input_schema: None,
            output_schema: None,
            language,
            body,
            intent_ids,
            status: PlanStatus::Running,
            parent_plan_id: None,
            aborted_at_step: None,
            created_at: now,
            executed_at: None,
            metadata: HashMap::new(),
        }
    }

    /// Create a new plan with explicit symbolic name
    pub fn new_named(
        name: String,
        language: PlanLanguage,
        body: PlanBody,
        intent_ids: Vec<IntentId>,
    ) -> Self {
        let mut plan = Self::new(language, body, intent_ids);
        plan.name = name;
        plan
    }

    /// Convenience helper for plain RTFS 2.0 source code
    pub fn new_rtfs(rtfs_code: String, intent_ids: Vec<IntentId>) -> Self {
        Self::new(PlanLanguage::Rtfs20, PlanBody::Text(rtfs_code), intent_ids)
    }

    /// mark as aborted helper
    pub fn mark_aborted(&mut self, step_index: u32) {
        self.status = PlanStatus::Aborted;
        self.aborted_at_step = Some(step_index);
    }

    /// mark completed helper
    pub fn mark_completed(&mut self) {
        self.status = PlanStatus::Completed;
        self.executed_at = Some(
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_secs(),
        );
    }
}

/// Action category for provenance tracking
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum ActionType {
    /// Standard capability/function execution
    CapabilityCall,
    /// Lifecycle markers for plans
    PlanStarted,
    PlanPaused,
    PlanResumed,
    PlanAborted,
    PlanCompleted,
    /// Optional fine-grained runtime step inside RTFS evaluator
    InternalStep,
}

/// Built-in capability id for human-in-the-loop questions
pub const ASK_HUMAN_CAPABILITY_ID: &str = "ccos.ask-human";

/// Represents an immutable record in the Causal Chain
/// This is the "what happened" - a single, auditable event
#[derive(Debug, Clone)]
pub struct Action {
    pub action_id: ActionId,
    pub plan_id: PlanId,
    pub intent_id: IntentId,
    /// Category of this action (capability call, plan lifecycle, etc.)
    pub action_type: ActionType,
    pub capability_id: Option<CapabilityId>,
    pub function_name: String,
    pub arguments: Vec<Value>,
    pub result: Option<Value>,
    pub success: bool,
    pub error_message: Option<String>,
    pub cost: f64,
    pub duration_ms: u64,
    pub timestamp: u64,
    pub metadata: HashMap<String, Value>,
}

impl Action {
    /// Create a new capability call action
    pub fn new_capability(
        plan_id: PlanId,
        intent_id: IntentId,
        function_name: String,
        arguments: Vec<Value>,
    ) -> Self {
        Self::base_new(
            ActionType::CapabilityCall,
            plan_id,
            intent_id,
            function_name,
            arguments,
        )
    }

    /// Create a plan lifecycle action (Started, Aborted, etc.)
    pub fn new_lifecycle(plan_id: PlanId, intent_id: IntentId, action_type: ActionType) -> Self {
        Self::base_new(action_type, plan_id, intent_id, String::new(), Vec::new())
    }

    fn base_new(
        action_type: ActionType,
        plan_id: PlanId,
        intent_id: IntentId,
        function_name: String,
        arguments: Vec<Value>,
    ) -> Self {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs();

        Self {
            action_id: format!("action-{}", Uuid::new_v4()),
            plan_id,
            intent_id,
            action_type,
            capability_id: None,
            function_name,
            arguments,
            result: None,
            success: false,
            error_message: None,
            cost: 0.0,
            duration_ms: 0,
            timestamp: now,
            metadata: HashMap::new(),
        }
    }

    /// Backwards-compat helper (old signature)
    pub fn new(
        plan_id: PlanId,
        intent_id: IntentId,
        function_name: String,
        arguments: Vec<Value>,
    ) -> Self {
        Self::new_capability(plan_id, intent_id, function_name, arguments)
    }

    pub fn with_capability(mut self, capability_id: CapabilityId) -> Self {
        self.capability_id = Some(capability_id);
        self
    }

    pub fn with_result(mut self, result: Value, success: bool) -> Self {
        self.result = Some(result);
        self.success = success;
        self
    }

    pub fn with_error(mut self, error_message: String) -> Self {
        self.error_message = Some(error_message);
        self.success = false;
        self
    }

    pub fn with_cost_and_duration(mut self, cost: f64, duration_ms: u64) -> Self {
        self.cost = cost;
        self.duration_ms = duration_ms;
        self
    }
}

/// Represents a formal declaration of a service available on the Marketplace
/// This is the "who can do it" - includes function signature and SLA metadata
#[derive(Debug, Clone)]
pub struct Capability {
    pub capability_id: CapabilityId,
    pub function_name: String,
    pub provider_id: String,
    pub function_signature: String,
    pub sla: ServiceLevelAgreement,
    pub ethical_alignment_profile: EthicalAlignmentProfile,
    pub created_at: u64,
    pub updated_at: u64,
    pub status: CapabilityStatus,
    pub metadata: HashMap<String, Value>,
}

impl Capability {
    pub fn new(function_name: String, provider_id: String, function_signature: String) -> Self {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs();

        Self {
            capability_id: format!("capability-{}", Uuid::new_v4()),
            function_name,
            provider_id,
            function_signature,
            sla: ServiceLevelAgreement::default(),
            ethical_alignment_profile: EthicalAlignmentProfile::default(),
            created_at: now,
            updated_at: now,
            status: CapabilityStatus::Active,
            metadata: HashMap::new(),
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum CapabilityStatus {
    Active,
    Inactive,
    Deprecated,
    Suspended,
}

/// Service Level Agreement for a capability
#[derive(Debug, Clone)]
pub struct ServiceLevelAgreement {
    pub cost_per_call: f64,
    pub expected_speed_ms: u64,
    pub confidence_metrics: HashMap<String, f64>,
    pub data_provenance: String,
    pub availability_percentage: f64,
    pub max_concurrent_calls: u32,
}

impl Default for ServiceLevelAgreement {
    fn default() -> Self {
        Self {
            cost_per_call: 0.0,
            expected_speed_ms: 1000,
            confidence_metrics: HashMap::new(),
            data_provenance: "unknown".to_string(),
            availability_percentage: 99.9,
            max_concurrent_calls: 100,
        }
    }
}

/// Ethical alignment profile for a capability
#[derive(Debug, Clone)]
pub struct EthicalAlignmentProfile {
    pub harm_prevention_score: f64,
    pub privacy_respect_score: f64,
    pub transparency_score: f64,
    pub fairness_score: f64,
    pub accountability_score: f64,
}

impl Default for EthicalAlignmentProfile {
    fn default() -> Self {
        Self {
            harm_prevention_score: 0.5,
            privacy_respect_score: 0.5,
            transparency_score: 0.5,
            fairness_score: 0.5,
            accountability_score: 0.5,
        }
    }
}

/// Represents a handle or pointer to a large data payload
/// This allows Plans to remain lightweight by referencing data
#[derive(Debug, Clone)]
pub struct Resource {
    pub resource_id: ResourceId,
    pub uri: String,
    pub resource_type: String,
    pub size_bytes: Option<u64>,
    pub checksum: Option<String>,
    pub created_at: u64,
    pub expires_at: Option<u64>,
    pub metadata: HashMap<String, Value>,
}

impl Resource {
    pub fn new(uri: String, resource_type: String) -> Self {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs();

        Self {
            resource_id: format!("resource-{}", Uuid::new_v4()),
            uri,
            resource_type,
            size_bytes: None,
            checksum: None,
            created_at: now,
            expires_at: None,
            metadata: HashMap::new(),
        }
    }
}

/// Represents a task for execution
#[derive(Debug, Clone)]
pub struct Task {
    pub task_id: TaskId,
    pub description: String,
    pub metadata: HashMap<String, Value>,
}

impl Task {
    pub fn new(task_id: TaskId, description: String) -> Self {
        Self {
            task_id,
            description,
            metadata: HashMap::new(),
        }
    }

    pub fn with_metadata(mut self, key: String, value: Value) -> Self {
        self.metadata.insert(key, value);
        self
    }
}

/// Represents the context loaded for a specific task execution
#[derive(Debug, Clone)]
pub struct Context {
    pub intents: Vec<Intent>,
    pub wisdom: DistilledWisdom,
    pub plan: AbstractPlan,
    pub metadata: HashMap<String, Value>,
}

impl Context {
    pub fn new() -> Self {
        Self {
            intents: Vec::new(),
            wisdom: DistilledWisdom::new(),
            plan: AbstractPlan::new(),
            metadata: HashMap::new(),
        }
    }
}

/// Distilled wisdom from the Subconscious analysis
#[derive(Debug, Clone)]
pub struct DistilledWisdom {
    pub agent_reliability_scores: HashMap<String, f64>,
    pub failure_patterns: Vec<String>,
    pub optimized_strategies: Vec<String>,
    pub cost_insights: HashMap<String, f64>,
    pub performance_metrics: HashMap<String, f64>,
}

impl DistilledWisdom {
    pub fn new() -> Self {
        Self {
            agent_reliability_scores: HashMap::new(),
            failure_patterns: Vec::new(),
            optimized_strategies: Vec::new(),
            cost_insights: HashMap::new(),
            performance_metrics: HashMap::new(),
        }
    }
}

/// Abstract plan with references instead of concrete data
#[derive(Debug, Clone)]
pub struct AbstractPlan {
    pub steps: Vec<AbstractStep>,
    pub data_handles: Vec<ResourceId>,
    pub metadata: HashMap<String, Value>,
}

impl AbstractPlan {
    pub fn new() -> Self {
        Self {
            steps: Vec::new(),
            data_handles: Vec::new(),
            metadata: HashMap::new(),
        }
    }
}

/// Abstract step in a plan
#[derive(Debug, Clone)]
pub struct AbstractStep {
    pub function_name: String,
    pub data_handle: Option<ResourceId>,
    pub capability_id: Option<CapabilityId>,
    pub metadata: HashMap<String, Value>,
}

/// Result of executing an intent
#[derive(Debug, Clone)]
pub struct ExecutionResult {
    pub success: bool,
    pub value: Value,
    pub metadata: HashMap<String, Value>,
}

/// Edge relationship types in the Intent Graph
#[derive(Debug, Clone, PartialEq)]
pub enum EdgeType {
    DependsOn,
    IsSubgoalOf,
    ConflictsWith,
    Enables,
    RelatedTo,
}

/// Edge in the Intent Graph
#[derive(Debug, Clone)]
pub struct Edge {
    pub from_intent: IntentId,
    pub to_intent: IntentId,
    pub edge_type: EdgeType,
    pub weight: f64,
    pub metadata: HashMap<String, Value>,
}

impl Edge {
    pub fn new(from_intent: IntentId, to_intent: IntentId, edge_type: EdgeType) -> Self {
        Self {
            from_intent,
            to_intent,
            edge_type,
            weight: 1.0,
            metadata: HashMap::new(),
        }
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
        assert_eq!(plan.body, PlanBody::Text("(+ 1 2)".to_string()));
        assert_eq!(plan.name, "");
        assert_eq!(plan.intent_ids, vec!["intent-1"]);
        assert!(plan.executed_at.is_none());
        assert!(plan.input_schema.is_none());
        assert!(plan.output_schema.is_none());
    }

    #[test]
    fn test_action_creation() {
        let action = Action::new(
            "plan-1".to_string(),
            "intent-1".to_string(),
            "add".to_string(),
            vec![Value::Float(1.0), Value::Float(2.0)],
        );

        assert_eq!(action.plan_id, "plan-1");
        assert_eq!(action.intent_id, "intent-1");
        assert_eq!(action.function_name, "add");
        assert_eq!(action.arguments.len(), 2);
        assert!(!action.success);
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
