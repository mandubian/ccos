use super::super::types::{Action, ActionId, CapabilityId, IntentId, PlanId};
use crate::runtime::error::RuntimeError;
use crate::runtime::values::Value;
use std::collections::HashMap;

/// Provenance tracking
#[derive(Debug)]
pub struct ProvenanceTracker {
    pub action_provenance: HashMap<ActionId, ActionProvenance>,
}

impl ProvenanceTracker {
    pub fn new() -> Self {
        Self {
            action_provenance: HashMap::new(),
        }
    }

    pub fn track_action(
        &mut self,
        action: &Action,
        _intent: &super::super::types::Intent,
    ) -> Result<(), RuntimeError> {
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
