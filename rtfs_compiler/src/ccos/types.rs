//! Core CCOS Types
//!
//! This module defines the fundamental data structures for the Cognitive Computing
//! Operating System, based on the CCOS specifications.

use crate::runtime::error::RuntimeError;
use crate::runtime::values::Value;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::time::{SystemTime, UNIX_EPOCH};
use uuid::Uuid;

// Forward declarations for types used in this module - these should be replaced with real imports
pub use super::intent_graph::IntentGraph;
pub use super::causal_chain::CausalChain;  
pub use crate::runtime::capability_marketplace::CapabilityMarketplace;

// Use the real CCOS type from the main module
pub use crate::ccos::CCOS;

// --- ID Type Aliases ---
pub type IntentId = String;
pub type PlanId = String;
pub type ActionId = String;
pub type CapabilityId = String;

// --- Intent Graph (SEP-001) ---

/// Represents the "why" behind all system behavior. A node in the Intent Graph.
#[derive(Debug, Clone, Serialize, Deserialize)]
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

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum IntentStatus {
    Active,
    /// Intent is currently being executed by a Plan (in-flight)
    Executing,
    Completed,
    Failed,
    Archived,
    Suspended,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum PlanStatus {
    Draft,
    Active,
    Completed,
    Failed,
    Paused,
    Aborted,
    Archived,
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

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum EdgeType {
    DependsOn,
    IsSubgoalOf,
    ConflictsWith,
    Enables,
    RelatedTo,
    TriggeredBy,
    Blocks,
}

/// What caused an Intent to be created
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum TriggerSource {
    HumanRequest,        // Direct human input
    PlanExecution,       // Created during plan execution  
    SystemEvent,         // System-triggered (monitoring, alerts)
    IntentCompletion,    // Triggered by another intent completing
    ArbiterInference,    // Arbiter discovered need for this intent
}

/// Complete context for reproducing intent generation
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GenerationContext {
    pub arbiter_version: String,
    pub generation_timestamp: u64,
    pub input_context: HashMap<String, String>,
    pub reasoning_trace: Option<String>,
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
    pub status: PlanStatus,
    pub created_at: u64,
    pub metadata: HashMap<String, Value>,
    
    // New first-class Plan attributes
    pub input_schema: Option<Value>,      // Schema for plan inputs
    pub output_schema: Option<Value>,     // Schema for plan outputs
    pub policies: HashMap<String, Value>, // Execution policies (max-cost, retry, etc.)
    pub capabilities_required: Vec<String>, // Capabilities this plan depends on
    pub annotations: HashMap<String, Value>, // Provenance and metadata (prompt-id, version, etc.)
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
    // Plan Lifecycle
    PlanStarted,
    PlanCompleted,
    PlanAborted,
    PlanPaused,
    PlanResumed,
    
    // Step Lifecycle
    PlanStepStarted,
    PlanStepCompleted,
    PlanStepFailed,
    PlanStepRetrying,
    
    // Execution
    CapabilityCall,
    InternalStep,
    StepProfileDerived,
    
    // Intent Lifecycle (new)
    IntentCreated,
    IntentStatusChanged,
    IntentRelationshipCreated,
    IntentRelationshipModified,
    IntentArchived,
    IntentReactivated,
    
    // Capability Lifecycle (new)
    CapabilityRegistered,
    CapabilityRemoved,
    CapabilityUpdated,
    CapabilityDiscoveryCompleted,
}

/// Represents the outcome of an executed action or plan.
#[derive(Debug, Clone)]
pub struct ExecutionResult {
    pub success: bool,
    pub value: Value,
    pub metadata: HashMap<String, Value>,
}

/// Storable version of Intent that contains only serializable data
/// This is used for persistence and doesn't contain any runtime values
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StorableIntent {
    pub intent_id: IntentId,
    pub name: Option<String>,
    pub original_request: String,
    pub rtfs_intent_source: String,                  // Canonical RTFS intent form
    pub goal: String,
    
    // RTFS source code as strings (serializable)
    pub constraints: HashMap<String, String>,        // RTFS source expressions
    pub preferences: HashMap<String, String>,        // RTFS source expressions
    pub success_criteria: Option<String>,            // RTFS source expression
    
    // Graph relationships
    pub parent_intent: Option<IntentId>,
    pub child_intents: Vec<IntentId>,
    pub triggered_by: TriggerSource,
    
    // Generation context for audit/replay
    pub generation_context: GenerationContext,
    
    pub status: IntentStatus,
    pub priority: u32,
    pub created_at: u64,
    pub updated_at: u64,
    pub metadata: HashMap<String, String>,           // Simple string metadata
}

/// Runtime version of Intent that can be executed in CCOS context
/// Contains parsed RTFS expressions and runtime values
#[derive(Debug, Clone)]
pub struct RuntimeIntent {
    pub intent_id: IntentId,
    pub name: Option<String>,
    pub original_request: String,
    pub rtfs_intent_source: String,                  // Canonical RTFS intent form
    pub goal: String,
    
    // Parsed RTFS expressions (runtime values)
    pub constraints: HashMap<String, Value>,
    pub preferences: HashMap<String, Value>,
    pub success_criteria: Option<Value>,
    
    // Graph relationships
    pub parent_intent: Option<IntentId>,
    pub child_intents: Vec<IntentId>,
    pub triggered_by: TriggerSource,
    
    // Generation context for audit/replay
    pub generation_context: GenerationContext,
    
    pub status: IntentStatus,
    pub priority: u32,
    pub created_at: u64,
    pub updated_at: u64,
    pub metadata: HashMap<String, Value>,
}

/// Execution context for CCOS operations
/// Provides access to current state and runtime for Intent evaluation
pub struct CCOSContext<'a> {
    pub ccos: &'a CCOS,
    pub current_intent: Option<&'a RuntimeIntent>,
    pub current_plan: Option<&'a Plan>,
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
            status: PlanStatus::Draft,
            created_at: SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_secs(),
            metadata: HashMap::new(),
            // New fields with defaults
            input_schema: None,
            output_schema: None,
            policies: HashMap::new(),
            capabilities_required: Vec::new(),
            annotations: HashMap::new(),
        }
    }

    pub fn new_named(name: String, language: PlanLanguage, body: PlanBody, intent_ids: Vec<IntentId>) -> Self {
        let mut plan = Self::new_rtfs(String::new(), intent_ids);
        plan.name = Some(name);
        plan.language = language;
        plan.body = body;
        plan
    }
    
    /// Create a new Plan with full first-class attributes
    pub fn new_with_schemas(
        name: Option<String>,
        intent_ids: Vec<IntentId>,
        body: PlanBody,
        input_schema: Option<Value>,
        output_schema: Option<Value>,
        policies: HashMap<String, Value>,
        capabilities_required: Vec<String>,
        annotations: HashMap<String, Value>,
    ) -> Self {
        Self {
            plan_id: format!("plan-{}", Uuid::new_v4()),
            name,
            intent_ids,
            language: PlanLanguage::Rtfs20,
            body,
            status: PlanStatus::Draft,
            created_at: SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_secs(),
            metadata: HashMap::new(),
            input_schema,
            output_schema,
            policies,
            capabilities_required,
            annotations,
        }
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

impl StorableIntent {
    /// Create a new StorableIntent
    pub fn new(goal: String) -> Self {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs();
        
        Self {
            intent_id: Uuid::new_v4().to_string(),
            name: None,
            original_request: goal.clone(),
            rtfs_intent_source: String::new(),
            goal,
            constraints: HashMap::new(),
            preferences: HashMap::new(),
            success_criteria: None,
            parent_intent: None,
            child_intents: Vec::new(),
            triggered_by: TriggerSource::HumanRequest,
            generation_context: GenerationContext {
                arbiter_version: "1.0.0".to_string(),
                generation_timestamp: now,
                input_context: HashMap::new(),
                reasoning_trace: None,
            },
            status: IntentStatus::Active,
            priority: 0,
            created_at: now,
            updated_at: now,
            metadata: HashMap::new(),
        }
    }
    
    /// Convert to RuntimeIntent by parsing RTFS expressions
    pub fn to_runtime_intent(&self, ccos: &CCOS) -> Result<RuntimeIntent, RuntimeError> {
        let mut rtfs_runtime = ccos.rtfs_runtime.lock().unwrap();
        
        let mut constraints = HashMap::new();
        for (key, source) in &self.constraints {
            let parsed = rtfs_runtime.parse_expression(source)?;
            constraints.insert(key.clone(), parsed);
        }
        
        let mut preferences = HashMap::new();
        for (key, source) in &self.preferences {
            let parsed = rtfs_runtime.parse_expression(source)?;
            preferences.insert(key.clone(), parsed);
        }
        
        let success_criteria = if let Some(source) = &self.success_criteria {
            Some(rtfs_runtime.parse_expression(source)?)
        } else {
            None
        };
        
        let mut metadata = HashMap::new();
        for (key, value) in &self.metadata {
            metadata.insert(key.clone(), Value::String(value.clone()));
        }
        
        Ok(RuntimeIntent {
            intent_id: self.intent_id.clone(),
            name: self.name.clone(),
            original_request: self.original_request.clone(),
            rtfs_intent_source: self.rtfs_intent_source.clone(),
            goal: self.goal.clone(),
            constraints,
            preferences,
            success_criteria,
            parent_intent: self.parent_intent.clone(),
            child_intents: self.child_intents.clone(),
            triggered_by: self.triggered_by.clone(),
            generation_context: self.generation_context.clone(),
            status: self.status.clone(),
            priority: self.priority,
            created_at: self.created_at,
            updated_at: self.updated_at,
            metadata,
        })
    }
}

impl RuntimeIntent {
    /// Convert to StorableIntent by converting runtime values back to RTFS source
    pub fn to_storable_intent(&self, ccos: &CCOS) -> Result<StorableIntent, RuntimeError> {
        let rtfs_runtime = ccos.rtfs_runtime.lock().unwrap();
        
        let mut constraints = HashMap::new();
        for (key, value) in &self.constraints {
            let source = rtfs_runtime.value_to_source(value)?;
            constraints.insert(key.clone(), source);
        }
        
        let mut preferences = HashMap::new();
        for (key, value) in &self.preferences {
            let source = rtfs_runtime.value_to_source(value)?;
            preferences.insert(key.clone(), source);
        }
        
        let success_criteria = if let Some(value) = &self.success_criteria {
            Some(rtfs_runtime.value_to_source(value)?)
        } else {
            None
        };
        
        let mut metadata = HashMap::new();
        for (key, value) in &self.metadata {
            if let Value::String(s) = value {
                metadata.insert(key.clone(), s.clone());
            } else {
                let source = rtfs_runtime.value_to_source(value)?;
                metadata.insert(key.clone(), source);
            }
        }
        
        Ok(StorableIntent {
            intent_id: self.intent_id.clone(),
            name: self.name.clone(),
            original_request: self.original_request.clone(),
            rtfs_intent_source: self.rtfs_intent_source.clone(),
            goal: self.goal.clone(),
            constraints,
            preferences,
            success_criteria,
            parent_intent: self.parent_intent.clone(),
            child_intents: self.child_intents.clone(),
            triggered_by: self.triggered_by.clone(),
            generation_context: self.generation_context.clone(),
            status: self.status.clone(),
            priority: self.priority,
            created_at: self.created_at,
            updated_at: self.updated_at,
            metadata,
        })
    }
    
    /// Evaluate success criteria in CCOS context
    pub fn evaluate_success_criteria(&self, context: &CCOSContext) -> Result<Value, RuntimeError> {
        if let Some(criteria) = &self.success_criteria {
            let mut rtfs_runtime = context.ccos.rtfs_runtime.lock().unwrap();
            rtfs_runtime.evaluate_with_ccos(criteria, context.ccos)
        } else {
            Ok(Value::Boolean(true)) // Default to success if no criteria
        }
    }
    
    /// Evaluate a constraint by name
    pub fn evaluate_constraint(&self, name: &str, context: &CCOSContext) -> Result<Value, RuntimeError> {
        if let Some(constraint) = self.constraints.get(name) {
            let mut rtfs_runtime = context.ccos.rtfs_runtime.lock().unwrap();
            rtfs_runtime.evaluate_with_ccos(constraint, context.ccos)
        } else {
            Err(RuntimeError::Generic(format!("Constraint '{}' not found", name)))
        }
    }
    
    /// Evaluate a preference by name
    pub fn evaluate_preference(&self, name: &str, context: &CCOSContext) -> Result<Value, RuntimeError> {
        if let Some(preference) = self.preferences.get(name) {
            let mut rtfs_runtime = context.ccos.rtfs_runtime.lock().unwrap();
            rtfs_runtime.evaluate_with_ccos(preference, context.ccos)
        } else {
            Err(RuntimeError::Generic(format!("Preference '{}' not found", name)))
        }
    }
}

impl CCOSContext<'_> {
    /// Create a new CCOS context
    pub fn new<'a>(
        ccos: &'a CCOS,
        current_intent: Option<&'a RuntimeIntent>,
        current_plan: Option<&'a Plan>,
    ) -> CCOSContext<'a> {
        CCOSContext {
            ccos,
            current_intent,
            current_plan,
        }
    }
    
    /// Get current intent if available
    pub fn current_intent(&self) -> Option<&RuntimeIntent> {
        self.current_intent
    }
    
    /// Get current plan if available
    pub fn current_plan(&self) -> Option<&Plan> {
        self.current_plan
    }
    
    /// Access CCOS components
    pub fn intent_graph(&self) -> std::sync::MutexGuard<super::intent_graph::IntentGraph> {
        self.ccos.intent_graph.lock().unwrap()
    }
    
    pub fn causal_chain(&self) -> std::sync::MutexGuard<super::causal_chain::CausalChain> {
        self.ccos.causal_chain.lock().unwrap()
    }
    
    pub fn capability_marketplace(&self) -> &std::sync::Arc<crate::runtime::capability_marketplace::CapabilityMarketplace> {
        &self.ccos.capability_marketplace
    }
}

#[cfg(test)]
mod tests {
    use crate::runtime::capability::Capability;

    use super::*;

    #[test]
    fn test_intent_creation() {
        let mut intent = Intent::new("Test goal".to_string());
        intent.constraints.insert("max_cost".to_string(), Value::Float(100.0));
        intent.preferences.insert("priority".to_string(), Value::String("speed".to_string()));

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
        use crate::runtime::values::Arity;
        use std::rc::Rc;
        use crate::runtime::error::RuntimeResult;
        use crate::runtime::values::Value;

        let capability = Capability::new(
            "image-processing/sharpen".to_string(),
            Arity::Fixed(2),
            Rc::new(|_args: Vec<Value>| -> RuntimeResult<Value> {
                Ok(Value::Nil)
            })
        );

        assert_eq!(capability.id, "image-processing/sharpen");
        assert_eq!(capability.arity, Arity::Fixed(2));
    }
}