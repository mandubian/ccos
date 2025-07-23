//! Causal Chain Implementation
//!
//! This module implements the Causal Chain of Thought - an immutable, verifiable ledger
//! that records every significant action with its complete audit trail.

use super::types::{Action, ActionId, ActionType, CapabilityId, ExecutionResult, Intent, IntentId, PlanId};
use crate::runtime::error::RuntimeError;
use crate::runtime::values::Value;
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::time::{SystemTime, UNIX_EPOCH};
use uuid::Uuid;

/// Immutable ledger storage
#[derive(Debug)]
pub struct ImmutableLedger {
    // In a full implementation, this would be append-only file storage
    actions: Vec<Action>,
    hash_chain: Vec<String>,
    indices: LedgerIndices,
}

impl ImmutableLedger {
    pub fn new() -> Self {
        Self {
            actions: Vec::new(),
            hash_chain: Vec::new(),
            indices: LedgerIndices::new(),
        }
    }

    pub fn append_action(&mut self, action: &Action) -> Result<(), RuntimeError> {
        // Calculate hash for this action
        let action_hash = self.calculate_action_hash(action);

        // Calculate the chain hash (includes previous hash)
        let chain_hash = self.calculate_chain_hash(&action_hash);

        // Append to ledger
        self.actions.push(action.clone());
        self.hash_chain.push(chain_hash);

        // Update indices
        self.indices.index_action(action)?;

        Ok(())
    }

    pub fn append(&mut self, action: &Action) -> Result<String, RuntimeError> {
        self.append_action(action)?;
        Ok(action.action_id.clone())
    }

    pub fn get_action(&self, action_id: &ActionId) -> Option<&Action> {
        self.actions.iter().find(|a| a.action_id == *action_id)
    }

    pub fn get_actions_by_intent(&self, intent_id: &IntentId) -> Vec<&Action> {
        self.indices
            .intent_actions
            .get(intent_id)
            .map(|action_ids| {
                action_ids
                    .iter()
                    .filter_map(|id| self.get_action(id))
                    .collect()
            })
            .unwrap_or_default()
    }

    pub fn get_actions_by_plan(&self, plan_id: &PlanId) -> Vec<&Action> {
        self.indices
            .plan_actions
            .get(plan_id)
            .map(|action_ids| {
                action_ids
                    .iter()
                    .filter_map(|id| self.get_action(id))
                    .collect()
            })
            .unwrap_or_default()
    }

    pub fn get_actions_by_capability(&self, capability_id: &CapabilityId) -> Vec<&Action> {
        self.indices
            .capability_actions
            .get(capability_id)
            .map(|action_ids| {
                action_ids
                    .iter()
                    .filter_map(|id| self.get_action(id))
                    .collect()
            })
            .unwrap_or_default()
    }

    pub fn get_all_actions(&self) -> &[Action] {
        &self.actions
    }

    pub fn verify_integrity(&self) -> Result<bool, RuntimeError> {
        let mut last_chain_hash: Option<&String> = None;
        for (i, action) in self.actions.iter().enumerate() {
            let action_hash = self.calculate_action_hash(action);
            let mut hasher = Sha256::new();
            if let Some(prev_hash) = last_chain_hash {
                hasher.update(prev_hash.as_bytes());
            }
            hasher.update(action_hash.as_bytes());
            let expected_chain_hash = format!("{:x}", hasher.finalize());

            if self.hash_chain[i] != expected_chain_hash {
                return Ok(false);
            }
            last_chain_hash = Some(&self.hash_chain[i]);
        }
        Ok(true)
    }

    fn calculate_action_hash(&self, action: &Action) -> String {
        let mut hasher = Sha256::new();

        // Hash all action fields
        hasher.update(action.action_id.as_bytes());
        hasher.update(action.plan_id.as_bytes());
        hasher.update(action.intent_id.as_bytes());
        if let Some(function_name) = &action.function_name {
            hasher.update(function_name.as_bytes());
        }
        hasher.update(action.timestamp.to_string().as_bytes());

        // Hash arguments
        for arg in &action.arguments {
            hasher.update(format!("{:?}", arg).as_bytes());
        }

        // Hash result
        if let Some(result) = &action.result {
            hasher.update(format!("{:?}", result).as_bytes());
        }

        // Hash metadata
        for (key, value) in &action.metadata {
            hasher.update(key.as_bytes());
            hasher.update(format!("{:?}", value).as_bytes());
        }

        format!("{:x}", hasher.finalize())
    }

    fn calculate_chain_hash(&self, action_hash: &str) -> String {
        let mut hasher = Sha256::new();

        // Include previous hash in chain
        if let Some(prev_hash) = self.hash_chain.last() {
            hasher.update(prev_hash.as_bytes());
        }

        hasher.update(action_hash.as_bytes());
        format!("{:x}", hasher.finalize())
    }
}

/// Indices for fast lookup
#[derive(Debug)]
pub struct LedgerIndices {
    intent_actions: HashMap<IntentId, Vec<ActionId>>,
    plan_actions: HashMap<PlanId, Vec<ActionId>>,
    capability_actions: HashMap<CapabilityId, Vec<ActionId>>,
    function_actions: HashMap<String, Vec<ActionId>>,
    timestamp_index: Vec<ActionId>, // Chronological order
}

impl LedgerIndices {
    pub fn new() -> Self {
        Self {
            intent_actions: HashMap::new(),
            plan_actions: HashMap::new(),
            capability_actions: HashMap::new(),
            function_actions: HashMap::new(),
            timestamp_index: Vec::new(),
        }
    }

    pub fn index_action(&mut self, action: &Action) -> Result<(), RuntimeError> {
        // Index by intent
        self.intent_actions
            .entry(action.intent_id.clone())
            .or_insert_with(Vec::new)
            .push(action.action_id.clone());

        // Index by plan
        self.plan_actions
            .entry(action.plan_id.clone())
            .or_insert_with(Vec::new)
            .push(action.action_id.clone());

        // Index by capability (stored in function_name for capability calls)
        if action.action_type == ActionType::CapabilityCall {
            if let Some(function_name) = &action.function_name {
                self.capability_actions
                    .entry(function_name.clone())
                    .or_insert_with(Vec::new)
                    .push(action.action_id.clone());
            }
        }

        // Index by function
        if let Some(function_name) = &action.function_name {
            self.function_actions
                .entry(function_name.clone())
                .or_insert_with(Vec::new)
                .push(action.action_id.clone());
        }

        // Index by timestamp
        self.timestamp_index.push(action.action_id.clone());

        Ok(())
    }
}

/// Cryptographic signing for actions
#[derive(Debug)]
pub struct CryptographicSigning {
    // In a full implementation, this would use proper PKI
    signing_key: String,
    verification_keys: HashMap<String, String>,
}

impl CryptographicSigning {
    pub fn new() -> Self {
        Self {
            signing_key: format!("key-{}", Uuid::new_v4()),
            verification_keys: HashMap::new(),
        }
    }

    pub fn sign_action(&self, action: &Action) -> String {
        let mut hasher = Sha256::new();
        hasher.update(self.signing_key.as_bytes());
        hasher.update(action.action_id.as_bytes());
        hasher.update(action.timestamp.to_string().as_bytes());
        format!("{:x}", hasher.finalize())
    }

    pub fn verify_signature(&self, action: &Action, signature: &str) -> bool {
        let expected_signature = self.sign_action(action);
        signature == expected_signature
    }

    pub fn add_verification_key(&mut self, key_id: String, public_key: String) {
        self.verification_keys.insert(key_id, public_key);
    }
}

/// Provenance tracking
#[derive(Debug)]
pub struct ProvenanceTracker {
    action_provenance: HashMap<ActionId, ActionProvenance>,
}

impl ProvenanceTracker {
    pub fn new() -> Self {
        Self {
            action_provenance: HashMap::new(),
        }
    }

    pub fn track_action(&mut self, action: &Action, intent: &Intent) -> Result<(), RuntimeError> {
        let provenance = ActionProvenance {
            action_id: action.action_id.clone(),
            intent_id: action.intent_id.clone(),
            plan_id: action.plan_id.clone(),
            capability_id: action.function_name.clone(),
            execution_context: ExecutionContext::new(),
            data_sources: Vec::new(),
            ethical_rules: Vec::new(),
            timestamp: action.timestamp,
        };

        self.action_provenance
            .insert(action.action_id.clone(), provenance);
        Ok(())
    }

    pub fn get_provenance(&self, action_id: &ActionId) -> Option<&ActionProvenance> {
        self.action_provenance.get(action_id)
    }
}

/// Complete provenance information for an action
#[derive(Debug, Clone)]
pub struct ActionProvenance {
    pub action_id: ActionId,
    pub intent_id: IntentId,
    pub plan_id: PlanId,
    pub capability_id: Option<CapabilityId>,
    pub execution_context: ExecutionContext,
    pub data_sources: Vec<String>,
    pub ethical_rules: Vec<String>,
    pub timestamp: u64,
}

/// Execution context for an action
#[derive(Debug, Clone)]
pub struct ExecutionContext {
    pub user_id: Option<String>,
    pub session_id: Option<String>,
    pub environment: HashMap<String, Value>,
    pub security_level: SecurityLevel,
}

impl ExecutionContext {
    pub fn new() -> Self {
        Self {
            user_id: None,
            session_id: None,
            environment: HashMap::new(),
            security_level: SecurityLevel::Standard,
        }
    }
}

#[derive(Debug, Clone)]
pub enum SecurityLevel {
    Public,
    Standard,
    Confidential,
    Secret,
}

/// Performance metrics tracking
#[derive(Debug)]
pub struct PerformanceMetrics {
    capability_metrics: HashMap<CapabilityId, CapabilityMetrics>,
    function_metrics: HashMap<String, FunctionMetrics>,
    cost_tracking: CostTracker,
}

impl PerformanceMetrics {
    pub fn new() -> Self {
        Self {
            capability_metrics: HashMap::new(),
            function_metrics: HashMap::new(),
            cost_tracking: CostTracker::new(),
        }
    }

    pub fn record_action(&mut self, action: &Action) -> Result<(), RuntimeError> {
        // Update capability metrics (for capability calls)
        if action.action_type == ActionType::CapabilityCall {
            if let Some(function_name) = &action.function_name {
                let metrics = self
                    .capability_metrics
                    .entry(function_name.clone())
                    .or_insert_with(CapabilityMetrics::new);
                metrics.record_action(action);
            }
        }

        // Update function metrics
        if let Some(function_name) = &action.function_name {
            let function_metrics = self
                .function_metrics
                .entry(function_name.clone())
                .or_insert_with(FunctionMetrics::new);
            function_metrics.record_action(action);
        }

        // Update cost tracking
        self.cost_tracking.record_cost(action.cost.unwrap_or(0.0));

        Ok(())
    }

    pub fn get_capability_metrics(
        &self,
        capability_id: &CapabilityId,
    ) -> Option<&CapabilityMetrics> {
        self.capability_metrics.get(capability_id)
    }

    pub fn get_function_metrics(&self, function_name: &str) -> Option<&FunctionMetrics> {
        self.function_metrics.get(function_name)
    }

    pub fn get_total_cost(&self) -> f64 {
        self.cost_tracking.total_cost
    }
}

/// Metrics for a specific capability
#[derive(Debug, Clone)]
pub struct CapabilityMetrics {
    pub total_calls: u64,
    pub successful_calls: u64,
    pub failed_calls: u64,
    pub total_cost: f64,
    pub total_duration_ms: u64,
    pub average_duration_ms: f64,
    pub reliability_score: f64,
}

impl CapabilityMetrics {
    pub fn new() -> Self {
        Self {
            total_calls: 0,
            successful_calls: 0,
            failed_calls: 0,
            total_cost: 0.0,
            total_duration_ms: 0,
            average_duration_ms: 0.0,
            reliability_score: 1.0,
        }
    }

    pub fn record_action(&mut self, action: &Action) {
        self.total_calls += 1;
        self.total_cost += action.cost.unwrap_or(0.0);
        self.total_duration_ms += action.duration_ms.unwrap_or(0);

        // Success/failure tracking removed: Action does not have a success field

        self.average_duration_ms = self.total_duration_ms as f64 / self.total_calls as f64;
        self.reliability_score = self.successful_calls as f64 / self.total_calls as f64;
    }
}

/// Metrics for a specific function
#[derive(Debug, Clone)]
pub struct FunctionMetrics {
    pub total_calls: u64,
    pub successful_calls: u64,
    pub failed_calls: u64,
    pub total_cost: f64,
    pub total_duration_ms: u64,
    pub average_duration_ms: f64,
}

impl FunctionMetrics {
    pub fn new() -> Self {
        Self {
            total_calls: 0,
            successful_calls: 0,
            failed_calls: 0,
            total_cost: 0.0,
            total_duration_ms: 0,
            average_duration_ms: 0.0,
        }
    }

    pub fn record_action(&mut self, action: &Action) {
        self.total_calls += 1;
        self.total_cost += action.cost.unwrap_or(0.0);
        self.total_duration_ms += action.duration_ms.unwrap_or(0);

        // Success/failure tracking removed: Action does not have a success field

        self.average_duration_ms = self.total_duration_ms as f64 / self.total_calls as f64;
    }
}

/// Cost tracking
#[derive(Debug)]
pub struct CostTracker {
    pub total_cost: f64,
    pub cost_by_intent: HashMap<IntentId, f64>,
    pub cost_by_plan: HashMap<PlanId, f64>,
    pub cost_by_capability: HashMap<CapabilityId, f64>,
}

impl CostTracker {
    pub fn new() -> Self {
        Self {
            total_cost: 0.0,
            cost_by_intent: HashMap::new(),
            cost_by_plan: HashMap::new(),
            cost_by_capability: HashMap::new(),
        }
    }

    pub fn record_cost(&mut self, cost: f64) {
        self.total_cost += cost;
    }

    pub fn record_action_cost(&mut self, action: &Action) {
        let cost = action.cost.unwrap_or(0.0);

        // Track by intent
        *self
            .cost_by_intent
            .entry(action.intent_id.clone())
            .or_insert(0.0) += cost;

        // Track by plan
        *self
            .cost_by_plan
            .entry(action.plan_id.clone())
            .or_insert(0.0) += cost;

        // Track by capability: Action does not have capability_id field, skip
    }
}

/// Main Causal Chain implementation
#[derive(Debug)]
pub struct CausalChain {
    ledger: ImmutableLedger,
    signing: CryptographicSigning,
    provenance: ProvenanceTracker,
    metrics: PerformanceMetrics,
}

impl CausalChain {
    pub fn new() -> Result<Self, RuntimeError> {
        Ok(Self {
            ledger: ImmutableLedger::new(),
            signing: CryptographicSigning::new(),
            provenance: ProvenanceTracker::new(),
            metrics: PerformanceMetrics::new(),
        })
    }

    /// Create a new action from an intent
    pub fn create_action(&mut self, intent: Intent) -> Result<Action, RuntimeError> {
        let plan_id = format!("plan-{}", Uuid::new_v4());
        let action = Action::new(
            ActionType::InternalStep,
            plan_id,
            intent.intent_id.clone(),
        )
        .with_name("execute_intent")
        .with_args(vec![Value::String(intent.goal.clone())]);

        // Track provenance
        self.provenance.track_action(&action, &intent)?;

        Ok(action)
    }

    /// Record the result of an action
    pub fn record_result(
        &mut self,
        mut action: Action,
        result: ExecutionResult,
    ) -> Result<(), RuntimeError> {
        // Update action with result
        action = action.with_result(result.clone());

        // Add metadata from result
        for (key, value) in result.metadata {
            action.metadata.insert(key, value);
        }

        // Sign the action
        let signature = self.signing.sign_action(&action);
        action
            .metadata
            .insert("signature".to_string(), Value::String(signature));

        // Append to ledger
        self.ledger.append_action(&action)?;

        // Record metrics
        self.metrics.record_action(&action)?;

        // Record cost
        self.metrics.cost_tracking.record_action_cost(&action);

        Ok(())
    }

    /// Get an action by ID
    pub fn get_action(&self, action_id: &ActionId) -> Option<&Action> {
        self.ledger.get_action(action_id)
    }

    /// Get the total number of actions in the causal chain
    pub fn get_action_count(&self) -> usize {
        self.ledger.actions.len()
    }

    /// Get an action by index
    pub fn get_action_by_index(&self, index: usize) -> Option<&Action> {
        self.ledger.actions.get(index)
    }

    /// Get all actions for an intent
    pub fn get_actions_for_intent(&self, intent_id: &IntentId) -> Vec<&Action> {
        self.ledger.get_actions_by_intent(intent_id)
    }

    /// Get all actions for a plan
    pub fn get_actions_for_plan(&self, plan_id: &PlanId) -> Vec<&Action> {
        self.ledger.get_actions_by_plan(plan_id)
    }

    /// Get all actions for a capability
    pub fn get_actions_for_capability(&self, capability_id: &CapabilityId) -> Vec<&Action> {
        self.ledger.get_actions_by_capability(capability_id)
    }

    /// Get provenance for an action
    pub fn get_provenance(&self, action_id: &ActionId) -> Option<&ActionProvenance> {
        self.provenance.get_provenance(action_id)
    }

    /// Get capability metrics
    pub fn get_capability_metrics(
        &self,
        capability_id: &CapabilityId,
    ) -> Option<&CapabilityMetrics> {
        self.metrics.get_capability_metrics(capability_id)
    }

    /// Get function metrics
    pub fn get_function_metrics(&self, function_name: &str) -> Option<&FunctionMetrics> {
        self.metrics.get_function_metrics(function_name)
    }

    /// Get total cost
    pub fn get_total_cost(&self) -> f64 {
        self.metrics.get_total_cost()
    }

    /// Verify ledger integrity
    pub fn verify_integrity(&self) -> Result<bool, RuntimeError> {
        self.ledger.verify_integrity()
    }

    /// Get all actions in chronological order
    pub fn get_all_actions(&self) -> &[Action] {
        self.ledger.get_all_actions()
    }

    /// Get actions within a time range
    pub fn get_actions_in_range(&self, start_time: u64, end_time: u64) -> Vec<&Action> {
        self.ledger
            .get_all_actions()
            .iter()
            .filter(|action| action.timestamp >= start_time && action.timestamp <= end_time)
            .collect()
    }

    /// Log a lifecycle event (PlanStarted, PlanAborted, PlanCompleted, ...)
    pub fn log_plan_event(
        &mut self,
        plan_id: &PlanId,
        intent_id: &IntentId,
        action_type: super::types::ActionType,
    ) -> Result<Action, RuntimeError> {
        use super::types::ActionType;
        // Create lifecycle action
        let action = Action::new(
            action_type,
            plan_id.clone(),
            intent_id.clone(),
        );

        // Sign
        let signature = self.signing.sign_action(&action);
        let mut signed_action = action.clone();
        signed_action
            .metadata
            .insert("signature".to_string(), Value::String(signature));

        // Append to ledger & metrics
        self.ledger.append_action(&signed_action)?;
        self.metrics.record_action(&signed_action)?;

        Ok(signed_action)
    }

    /// Convenience wrappers
    pub fn log_plan_started(
        &mut self,
        plan_id: &PlanId,
        intent_id: &IntentId,
    ) -> Result<(), RuntimeError> {
        self.log_plan_event(plan_id, intent_id, super::types::ActionType::PlanStarted)?;
        Ok(())
    }

    pub fn log_plan_aborted(
        &mut self,
        plan_id: &PlanId,
        intent_id: &IntentId,
    ) -> Result<(), RuntimeError> {
        self.log_plan_event(plan_id, intent_id, super::types::ActionType::PlanAborted)?;
        Ok(())
    }

    pub fn log_plan_completed(
        &mut self,
        plan_id: &PlanId,
        intent_id: &IntentId,
    ) -> Result<(), RuntimeError> {
        self.log_plan_event(plan_id, intent_id, super::types::ActionType::PlanCompleted)?;
        Ok(())
    }

    // ---------------------------------------------------------------------
    // Capability call logging
    // ---------------------------------------------------------------------

    /// Record a capability call in the causal chain.
    pub fn log_capability_call(
        &mut self,
        plan_id: &PlanId,
        intent_id: &IntentId,
        capability_id: &CapabilityId,
        function_name: &str,
        args: Vec<Value>,
    ) -> Result<Action, RuntimeError> {
        use super::types::ActionType;

        // Build action
        let action = Action::new(
            ActionType::CapabilityCall,
            plan_id.clone(),
            intent_id.clone(),
        )
        .with_name(function_name)
        .with_args(args);

        // Sign
        let signature = self.signing.sign_action(&action);
        let mut signed_action = action.clone();
        signed_action
            .metadata
            .insert("signature".to_string(), Value::String(signature));

        // Append ledger and metrics
        self.ledger.append_action(&signed_action)?;
        self.metrics.record_action(&signed_action)?;

        Ok(signed_action)
    }

    pub fn append(&mut self, action: &Action) -> Result<String, RuntimeError> {
        self.ledger.append(action)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::runtime::values::Value;

    #[test]
    fn test_causal_chain_creation() {
        let chain = CausalChain::new();
        assert!(chain.is_ok());
    }

    #[test]
    fn test_action_creation_and_recording() {
        let mut chain = CausalChain::new().unwrap();
        let intent = Intent::new("Test goal".to_string());

        let action = chain.create_action(intent).unwrap();
        assert_eq!(action.function_name.as_ref().unwrap(), "execute_intent");

        let result = ExecutionResult {
            success: true,
            value: Value::Float(42.0),
            metadata: HashMap::new(),
        };

        // Clone action to avoid partial move error
        assert!(chain.record_result(action, result.clone()).is_ok());
    }

    #[test]
    fn test_ledger_integrity() {
        let mut chain = CausalChain::new().unwrap();
        let intent = Intent::new("Test goal".to_string());
        let action = chain.create_action(intent).unwrap();

        let result = ExecutionResult {
            success: true,
            value: Value::Nil,
            metadata: HashMap::new(),
        };

        chain.record_result(action, result).unwrap();

        assert!(chain.verify_integrity().unwrap());
    }

    #[test]
    fn test_metrics_tracking() {
        let mut chain = CausalChain::new().unwrap();
        let intent = Intent::new("Test goal".to_string());
        let action = chain.create_action(intent).unwrap();

        let result = ExecutionResult {
            success: true,
            value: Value::Nil,
            metadata: HashMap::new(),
        };

        chain.record_result(action, result).unwrap();

        assert_eq!(chain.get_total_cost(), 0.0); // Default cost is 0.0
    }
}
