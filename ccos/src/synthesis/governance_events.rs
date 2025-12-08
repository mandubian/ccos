//! Governance Event Recording for Causal Chain
//!
//! This module provides helper functions to record governance-related events
//! to the causal chain, enabling full audit trails for AI self-programming.

use crate::causal_chain::CausalChain;
use crate::types::{Action, ActionType, ExecutionResult};
use rtfs::runtime::error::RuntimeError;
use rtfs::runtime::values::Value;
use std::collections::HashMap;

/// Records a governance event to the causal chain
pub struct GovernanceEventRecorder;

impl GovernanceEventRecorder {
    /// Record that a capability version was created before modification
    pub fn record_version_created(
        chain: &mut CausalChain,
        capability_id: &str,
        version_id: &str,
        reason: &str,
        plan_id: &str,
        intent_id: &str,
    ) -> Result<String, RuntimeError> {
        let mut action = Action::new(
            ActionType::CapabilityVersionCreated,
            plan_id.to_string(),
            intent_id.to_string(),
        )
        .with_name(&format!("version_created:{}", capability_id));

        action.metadata.insert(
            "capability_id".to_string(),
            Value::String(capability_id.to_string()),
        );
        action.metadata.insert(
            "version_id".to_string(),
            Value::String(version_id.to_string()),
        );
        action
            .metadata
            .insert("reason".to_string(), Value::String(reason.to_string()));

        chain.append(&action)
    }

    /// Record that a capability was rolled back to a previous version
    pub fn record_rollback(
        chain: &mut CausalChain,
        capability_id: &str,
        from_version: Option<&str>,
        to_version: &str,
        plan_id: &str,
        intent_id: &str,
    ) -> Result<String, RuntimeError> {
        let mut action = Action::new(
            ActionType::CapabilityRollback,
            plan_id.to_string(),
            intent_id.to_string(),
        )
        .with_name(&format!("rollback:{}", capability_id));

        action.metadata.insert(
            "capability_id".to_string(),
            Value::String(capability_id.to_string()),
        );
        if let Some(from) = from_version {
            action
                .metadata
                .insert("from_version".to_string(), Value::String(from.to_string()));
        }
        action.metadata.insert(
            "to_version".to_string(),
            Value::String(to_version.to_string()),
        );

        chain.append(&action)
    }

    /// Record that capability synthesis has started
    pub fn record_synthesis_started(
        chain: &mut CausalChain,
        capability_id: &str,
        intent: &str,
        plan_id: &str,
        intent_id: &str,
    ) -> Result<String, RuntimeError> {
        let mut action = Action::new(
            ActionType::CapabilitySynthesisStarted,
            plan_id.to_string(),
            intent_id.to_string(),
        )
        .with_name(&format!("synthesis_start:{}", capability_id));

        action.metadata.insert(
            "capability_id".to_string(),
            Value::String(capability_id.to_string()),
        );
        action
            .metadata
            .insert("intent".to_string(), Value::String(intent.to_string()));

        chain.append(&action)
    }

    /// Record that capability synthesis completed (success or failure)
    pub fn record_synthesis_completed(
        chain: &mut CausalChain,
        capability_id: &str,
        success: bool,
        error_message: Option<&str>,
        plan_id: &str,
        intent_id: &str,
    ) -> Result<String, RuntimeError> {
        let mut action = Action::new(
            ActionType::CapabilitySynthesisCompleted,
            plan_id.to_string(),
            intent_id.to_string(),
        )
        .with_name(&format!("synthesis_complete:{}", capability_id));

        action.metadata.insert(
            "capability_id".to_string(),
            Value::String(capability_id.to_string()),
        );
        action
            .metadata
            .insert("success".to_string(), Value::Boolean(success));

        if let Some(err) = error_message {
            action
                .metadata
                .insert("error".to_string(), Value::String(err.to_string()));
        }

        // Add result to action
        let result = ExecutionResult {
            success,
            value: if success {
                Value::String(capability_id.to_string())
            } else {
                Value::Nil
            },
            metadata: HashMap::new(),
        };
        let action = action.with_result(result);

        chain.append(&action)
    }

    /// Record that an action requires human approval
    pub fn record_approval_requested(
        chain: &mut CausalChain,
        action_type: &str,
        action_id: &str,
        description: &str,
        session_id: &str,
        plan_id: &str,
        intent_id: &str,
    ) -> Result<String, RuntimeError> {
        let mut action = Action::new(
            ActionType::GovernanceApprovalRequested,
            plan_id.to_string(),
            intent_id.to_string(),
        )
        .with_name(&format!("approval_requested:{}", action_type));

        action.metadata.insert(
            "pending_action_type".to_string(),
            Value::String(action_type.to_string()),
        );
        action.metadata.insert(
            "pending_action_id".to_string(),
            Value::String(action_id.to_string()),
        );
        action.metadata.insert(
            "description".to_string(),
            Value::String(description.to_string()),
        );
        action.metadata.insert(
            "session_id".to_string(),
            Value::String(session_id.to_string()),
        );

        chain.append(&action)
    }

    /// Record that an action was approved by human
    pub fn record_approval_granted(
        chain: &mut CausalChain,
        action_id: &str,
        approver: Option<&str>,
        plan_id: &str,
        intent_id: &str,
    ) -> Result<String, RuntimeError> {
        let mut action = Action::new(
            ActionType::GovernanceApprovalGranted,
            plan_id.to_string(),
            intent_id.to_string(),
        )
        .with_name("approval_granted");

        action.metadata.insert(
            "approved_action_id".to_string(),
            Value::String(action_id.to_string()),
        );
        if let Some(who) = approver {
            action
                .metadata
                .insert("approver".to_string(), Value::String(who.to_string()));
        }

        chain.append(&action)
    }

    /// Record that an action was denied by human
    pub fn record_approval_denied(
        chain: &mut CausalChain,
        action_id: &str,
        reason: Option<&str>,
        plan_id: &str,
        intent_id: &str,
    ) -> Result<String, RuntimeError> {
        let mut action = Action::new(
            ActionType::GovernanceApprovalDenied,
            plan_id.to_string(),
            intent_id.to_string(),
        )
        .with_name("approval_denied");

        action.metadata.insert(
            "denied_action_id".to_string(),
            Value::String(action_id.to_string()),
        );
        if let Some(r) = reason {
            action
                .metadata
                .insert("denial_reason".to_string(), Value::String(r.to_string()));
        }

        chain.append(&action)
    }

    /// Record that a bounded exploration limit was reached
    pub fn record_exploration_limit_reached(
        chain: &mut CausalChain,
        session_id: &str,
        limit_type: &str, // "synthesis" or "decomposition"
        current_count: u32,
        max_allowed: u32,
        plan_id: &str,
        intent_id: &str,
    ) -> Result<String, RuntimeError> {
        let mut action = Action::new(
            ActionType::BoundedExplorationLimitReached,
            plan_id.to_string(),
            intent_id.to_string(),
        )
        .with_name(&format!("limit_reached:{}", limit_type));

        action.metadata.insert(
            "session_id".to_string(),
            Value::String(session_id.to_string()),
        );
        action.metadata.insert(
            "limit_type".to_string(),
            Value::String(limit_type.to_string()),
        );
        action.metadata.insert(
            "current_count".to_string(),
            Value::Integer(current_count as i64),
        );
        action.metadata.insert(
            "max_allowed".to_string(),
            Value::Integer(max_allowed as i64),
        );

        chain.append(&action)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_record_version_created() {
        let mut chain = CausalChain::new().unwrap();
        let result = GovernanceEventRecorder::record_version_created(
            &mut chain,
            "generated/test-cap",
            "version_123",
            "pre-synthesis backup",
            "plan-1",
            "intent-1",
        );
        assert!(result.is_ok());
        assert_eq!(chain.get_action_count(), 1);
    }

    #[test]
    fn test_record_synthesis_lifecycle() {
        let mut chain = CausalChain::new().unwrap();

        // Record start
        let start = GovernanceEventRecorder::record_synthesis_started(
            &mut chain,
            "generated/new-cap",
            "Create a helper function",
            "plan-1",
            "intent-1",
        );
        assert!(start.is_ok());

        // Record completion
        let complete = GovernanceEventRecorder::record_synthesis_completed(
            &mut chain,
            "generated/new-cap",
            true,
            None,
            "plan-1",
            "intent-1",
        );
        assert!(complete.is_ok());

        assert_eq!(chain.get_action_count(), 2);
    }

    #[test]
    fn test_record_exploration_limit() {
        let mut chain = CausalChain::new().unwrap();
        let result = GovernanceEventRecorder::record_exploration_limit_reached(
            &mut chain,
            "session-abc",
            "synthesis",
            10,
            10,
            "plan-1",
            "intent-1",
        );
        assert!(result.is_ok());
    }
}
