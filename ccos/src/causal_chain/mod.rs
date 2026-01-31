/// Query struct for flexible filtering of actions in the causal chain
#[derive(Debug, Default, Clone)]
pub struct CausalQuery {
    pub intent_id: Option<IntentId>,
    pub plan_id: Option<PlanId>,
    pub action_type: Option<ActionType>,
    pub time_range: Option<(u64, u64)>,
    pub parent_action_id: Option<ActionId>,
    pub function_prefix: Option<String>,
}

impl CausalQuery {
    pub fn new() -> Self {
        Self::default()
    }
}

/// Causal Chain Implementation split into focused submodules.
use super::event_sink::CausalChainEventSink;
use rtfs::runtime::error::RuntimeError;
use rtfs::runtime::values::Value;
use std::collections::HashMap;
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};
use uuid::Uuid;

pub mod ledger;
pub mod metrics;
pub mod provenance;
pub mod signing;

use super::types::{
    Action, ActionId, ActionType, CapabilityId, ExecutionResult, Intent, IntentId, PlanId,
};
use crate::causal_chain::ledger::ImmutableLedger;
use crate::causal_chain::metrics::{CapabilityMetrics, FunctionMetrics, PerformanceMetrics};
use crate::causal_chain::provenance::ActionProvenance;
use crate::causal_chain::provenance::ProvenanceTracker;
use crate::causal_chain::signing::CryptographicSigning;

/// Simple in-memory log buffer for structured JSON logs (test-friendly)
#[derive(Debug)]
struct LogBuffer {
    entries: Vec<String>,
    capacity: usize,
}

impl LogBuffer {
    fn new(capacity: usize) -> Self {
        Self {
            entries: Vec::with_capacity(capacity.min(1024)),
            capacity,
        }
    }
    fn push(&mut self, entry: String) {
        if self.entries.len() >= self.capacity {
            self.entries.remove(0);
        }
        self.entries.push(entry);
    }
    fn recent(&self, max: usize) -> Vec<String> {
        let len = self.entries.len();
        let start = len.saturating_sub(max);
        self.entries[start..].to_vec()
    }
}

/// Main Causal Chain implementation
#[derive(Debug)]
pub struct CausalChain {
    ledger: ImmutableLedger,
    signing: CryptographicSigning,
    provenance: ProvenanceTracker,
    metrics: PerformanceMetrics,
    event_sinks: Vec<Arc<dyn CausalChainEventSink>>,
    logs: LogBuffer,
}

impl CausalChain {
    /// Query actions with flexible filters (intent, plan, type, time, parent)
    pub fn query_actions(&self, query: &CausalQuery) -> Vec<&Action> {
        let mut actions: Vec<&Action> = self.get_all_actions().iter().collect();

        if let Some(ref intent_id) = query.intent_id {
            actions = actions
                .into_iter()
                .filter(|a| &a.intent_id == intent_id)
                .collect();
        }
        if let Some(ref plan_id) = query.plan_id {
            actions = actions
                .into_iter()
                .filter(|a| &a.plan_id == plan_id)
                .collect();
        }
        if let Some(ref action_type) = query.action_type {
            actions = actions
                .into_iter()
                .filter(|a| &a.action_type == action_type)
                .collect();
        }
        if let Some((start, end)) = query.time_range {
            actions = actions
                .into_iter()
                .filter(|a| a.timestamp >= start && a.timestamp <= end)
                .collect();
        }
        if let Some(ref parent_id) = query.parent_action_id {
            actions = actions
                .into_iter()
                .filter(|a| a.parent_action_id.as_ref() == Some(parent_id))
                .collect();
        }
        if let Some(ref prefix) = query.function_prefix {
            actions = actions
                .into_iter()
                .filter(|a| a.function_name.as_deref().unwrap_or("").starts_with(prefix))
                .collect();
        }
        actions
    }

    /// Get children actions for a given parent_action_id
    pub fn get_children(&self, parent_id: &ActionId) -> Vec<&Action> {
        self.ledger.get_children(parent_id)
    }

    /// Get parent action for a given action_id
    pub fn get_parent(&self, action_id: &ActionId) -> Option<&Action> {
        self.ledger.get_parent(action_id)
    }
    pub fn new() -> Result<Self, RuntimeError> {
        Ok(Self {
            ledger: ImmutableLedger::new(),
            signing: CryptographicSigning::new(),
            provenance: ProvenanceTracker::new(),
            metrics: PerformanceMetrics::new(),
            event_sinks: Vec::new(),
            logs: LogBuffer::new(
                std::env::var("CCOS_LOG_BUFFER_CAPACITY")
                    .ok()
                    .and_then(|v| v.parse::<usize>().ok())
                    .unwrap_or(256),
            ),
        })
    }

    /// Register an event sink to be notified of action appends
    pub fn register_event_sink(&mut self, sink: Arc<dyn CausalChainEventSink>) {
        self.event_sinks.push(sink);
    }

    /// Notify all registered event sinks of an action append
    fn notify_sinks(&mut self, action: &Action) {
        use std::time::Instant;

        for sink in &self.event_sinks {
            if sink.is_wm_sink() {
                // Measure latency for Working Memory ingestion
                let start_time = Instant::now();
                sink.on_action_appended(action);
                let latency_ms = start_time.elapsed().as_millis() as u64;
                self.metrics.record_wm_ingest_latency(latency_ms);
            } else {
                sink.on_action_appended(action);
            }
        }
    }

    fn log_action_json(&mut self, action: &Action, event: &str) {
        // Minimal JSON without pulling serde_json dependency at runtime
        let fn_name = action.function_name.as_deref().unwrap_or("");
        let json = format!(
            "{{\"event\":\"{}\",\"action_type\":\"{:?}\",\"action_id\":\"{}\",\"function_name\":\"{}\",\"intent_id\":\"{}\",\"plan_id\":\"{}\",\"timestamp\":{}}}",
            event,
            action.action_type,
            action.action_id,
            fn_name,
            action.intent_id,
            action.plan_id,
            action.timestamp
        );
        self.logs.push(json);
    }

    /// Create a new action from an intent, with audit metadata provided by privileged system components
    pub fn create_action(
        &mut self,
        intent: Intent,
        audit_metadata: Option<HashMap<String, Value>>,
    ) -> Result<Action, RuntimeError> {
        let plan_id = format!("plan-{}", Uuid::new_v4());
        let mut action = Action::new(
            ActionType::InternalStep,
            plan_id.clone(),
            intent.intent_id.clone(),
        )
        .with_name("execute_intent")
        .with_args(vec![Value::String(intent.goal.clone())]);

        // Insert provided audit metadata if present
        if let Some(meta) = audit_metadata {
            for (key, value) in meta {
                action.metadata.insert(key, value);
            }
        }

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
        if action.action_type == ActionType::CapabilityCall {
            // For capability calls: write a separate CapabilityResult node linked to the call
            let mut result_action = Action::new(
                ActionType::CapabilityResult,
                action.plan_id.clone(),
                action.intent_id.clone(),
            )
            .with_parent(Some(action.action_id.clone()))
            .with_name(action.function_name.as_deref().unwrap_or("<unknown>"))
            .with_result(result.clone());

            if let Some(sid) = &action.session_id {
                result_action = result_action.with_session(sid);
            }

            // Attach metadata from result
            for (key, value) in result.metadata {
                result_action.metadata.insert(key, value);
            }

            // Sign the action
            let signature = self.signing.sign_action(&result_action);
            result_action
                .metadata
                .insert("signature".to_string(), Value::String(signature));

            // Append to ledger
            self.ledger.append_action(&result_action)?;

            // Record metrics and cost
            self.metrics.record_action(&result_action)?;
            self.metrics
                .cost_tracking
                .record_action_cost(&result_action);

            // Notify event sinks
            self.notify_sinks(&result_action);

            // Log structured line
            self.log_action_json(&result_action, "capability_result_recorded");

            Ok(())
        } else {
            // For all other actions, preserve behavior: append the same action with result
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

            // Notify event sinks
            self.notify_sinks(&action);

            // Log structured line
            self.log_action_json(&action, "action_result_recorded");

            Ok(())
        }
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
        // Create lifecycle action
        let action = Action::new(action_type, plan_id.clone(), intent_id.clone());

        // Sign
        let signature = self.signing.sign_action(&action);
        let mut signed_action = action.clone();
        signed_action
            .metadata
            .insert("signature".to_string(), Value::String(signature));

        // Append to ledger & metrics
        self.ledger.append_action(&signed_action)?;
        self.metrics.record_action(&signed_action)?;

        // Log structured line
        self.log_action_json(&signed_action, "plan_event");

        // Notify event sinks
        self.notify_sinks(&signed_action);

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
    // Intent lifecycle logging
    // ---------------------------------------------------------------------

    /// Log Intent creation in the causal chain
    pub fn log_intent_created(
        &mut self,
        plan_id: &PlanId,
        intent_id: &IntentId,
        goal: &str,
        triggered_by: Option<&str>,
    ) -> Result<Action, RuntimeError> {
        let mut action = Action::new(
            ActionType::IntentCreated,
            plan_id.clone(),
            intent_id.clone(),
        )
        .with_name("create_intent")
        .with_args(vec![Value::String(goal.to_string())]);

        // Add metadata
        if let Some(trigger) = triggered_by {
            action.metadata.insert(
                "triggered_by".to_string(),
                Value::String(trigger.to_string()),
            );
        }
        action
            .metadata
            .insert("goal".to_string(), Value::String(goal.to_string()));

        // Sign and record
        let signature = self.signing.sign_action(&action);
        action
            .metadata
            .insert("signature".to_string(), Value::String(signature));

        self.ledger.append_action(&action)?;
        self.metrics.record_action(&action)?;

        Ok(action)
    }

    /// Log Intent status change in the causal chain
    pub fn log_intent_status_change(
        &mut self,
        plan_id: &PlanId,
        intent_id: &IntentId,
        old_status: &str,
        new_status: &str,
        reason: &str,
        triggering_action_id: Option<&str>,
    ) -> Result<Action, RuntimeError> {
        let mut action = Action::new(
            ActionType::IntentStatusChanged,
            plan_id.clone(),
            intent_id.clone(),
        )
        .with_name("change_intent_status")
        .with_args(vec![
            Value::String(old_status.to_string()),
            Value::String(new_status.to_string()),
            Value::String(reason.to_string()),
        ]);

        // Add rich metadata for audit trail
        action.metadata.insert(
            "old_status".to_string(),
            Value::String(old_status.to_string()),
        );
        action.metadata.insert(
            "new_status".to_string(),
            Value::String(new_status.to_string()),
        );
        action
            .metadata
            .insert("reason".to_string(), Value::String(reason.to_string()));

        if let Some(triggering_id) = triggering_action_id {
            action.metadata.insert(
                "triggering_action_id".to_string(),
                Value::String(triggering_id.to_string()),
            );
        }

        // Add timestamp for audit trail correlation
        action.metadata.insert(
            "transition_timestamp".to_string(),
            Value::String(action.timestamp.to_string()),
        );

        // Sign and record
        let signature = self.signing.sign_action(&action);
        action
            .metadata
            .insert("signature".to_string(), Value::String(signature));

        self.ledger.append_action(&action)?;
        self.metrics.record_action(&action)?;

        Ok(action)
    }

    /// Log Intent relationship creation in the causal chain
    pub fn log_intent_relationship_created(
        &mut self,
        plan_id: &PlanId,
        intent_id: &IntentId,
        from_intent: &IntentId,
        to_intent: &IntentId,
        relationship_type: &str,
        weight: Option<f64>,
        metadata: Option<&HashMap<String, Value>>,
    ) -> Result<Action, RuntimeError> {
        let mut action = Action::new(
            ActionType::IntentRelationshipCreated,
            plan_id.clone(),
            intent_id.clone(),
        )
        .with_name("create_intent_relationship")
        .with_args(vec![
            Value::String(from_intent.clone()),
            Value::String(to_intent.clone()),
            Value::String(relationship_type.to_string()),
        ]);

        // Add relationship metadata
        action.metadata.insert(
            "from_intent".to_string(),
            Value::String(from_intent.clone()),
        );
        action
            .metadata
            .insert("to_intent".to_string(), Value::String(to_intent.clone()));
        action.metadata.insert(
            "relationship_type".to_string(),
            Value::String(relationship_type.to_string()),
        );

        if let Some(w) = weight {
            action
                .metadata
                .insert("weight".to_string(), Value::Float(w));
        }

        // Add any additional metadata
        if let Some(meta) = metadata {
            for (key, value) in meta {
                action
                    .metadata
                    .insert(format!("rel_{}", key), value.clone());
            }
        }

        // Sign and record
        let signature = self.signing.sign_action(&action);
        action
            .metadata
            .insert("signature".to_string(), Value::String(signature));

        self.ledger.append_action(&action)?;
        self.metrics.record_action(&action)?;

        Ok(action)
    }

    /// Log Intent archival in the causal chain
    pub fn log_intent_archived(
        &mut self,
        plan_id: &PlanId,
        intent_id: &IntentId,
        reason: &str,
    ) -> Result<Action, RuntimeError> {
        let mut action = Action::new(
            ActionType::IntentArchived,
            plan_id.clone(),
            intent_id.clone(),
        )
        .with_name("archive_intent")
        .with_args(vec![Value::String(reason.to_string())]);

        // Add metadata
        action
            .metadata
            .insert("reason".to_string(), Value::String(reason.to_string()));
        action.metadata.insert(
            "archived_at".to_string(),
            Value::String(action.timestamp.to_string()),
        );

        // Sign and record
        let signature = self.signing.sign_action(&action);
        action
            .metadata
            .insert("signature".to_string(), Value::String(signature));

        self.ledger.append_action(&action)?;
        self.metrics.record_action(&action)?;

        Ok(action)
    }

    /// Log Intent reactivation in the causal chain
    pub fn log_intent_reactivated(
        &mut self,
        plan_id: &PlanId,
        intent_id: &IntentId,
        reason: &str,
    ) -> Result<Action, RuntimeError> {
        let mut action = Action::new(
            ActionType::IntentReactivated,
            plan_id.clone(),
            intent_id.clone(),
        )
        .with_name("reactivate_intent")
        .with_args(vec![Value::String(reason.to_string())]);

        // Add metadata
        action
            .metadata
            .insert("reason".to_string(), Value::String(reason.to_string()));
        action.metadata.insert(
            "reactivated_at".to_string(),
            Value::String(action.timestamp.to_string()),
        );

        // Sign and record
        let signature = self.signing.sign_action(&action);
        action
            .metadata
            .insert("signature".to_string(), Value::String(signature));

        self.ledger.append_action(&action)?;
        self.metrics.record_action(&action)?;

        Ok(action)
    }

    // ---------------------------------------------------------------------
    // Capability call logging
    // ---------------------------------------------------------------------

    /// Record a capability call in the causal chain.
    pub fn log_capability_call(
        &mut self,
        session_id: Option<&str>,
        plan_id: &PlanId,
        intent_id: &IntentId,
        _capability_id: &CapabilityId,
        function_name: &str,
        args: Vec<Value>,
    ) -> Result<Action, RuntimeError> {
        // Build action
        let mut action = Action::new(
            ActionType::CapabilityCall,
            plan_id.clone(),
            intent_id.clone(),
        )
        .with_name(function_name)
        .with_args(args);

        if let Some(sid) = session_id {
            action = action.with_session(sid);
        }

        // Sign
        let signature = self.signing.sign_action(&action);
        let mut signed_action = action.clone();
        signed_action
            .metadata
            .insert("signature".to_string(), Value::String(signature));

        // Append ledger and metrics
        self.ledger.append_action(&signed_action)?;
        self.metrics.record_action(&signed_action)?;

        // Log structured line
        self.log_action_json(&signed_action, "capability_call");

        Ok(signed_action)
    }

    pub fn append(&mut self, action: &Action) -> Result<String, RuntimeError> {
        // Append to ledger
        self.ledger.append_action(action)?;

        // Record metrics
        self.metrics.record_action(action)?;

        // Notify event sinks
        self.notify_sinks(action);

        // Log structured line
        self.log_action_json(action, "action_appended");

        Ok(action.action_id.clone())
    }

    // Summarize recent actions for fast recall.
    pub fn summarize_recent(&self, count: usize) -> String {
        let total = self.ledger.actions.len();
        let start = if count > total { 0 } else { total - count };
        let mut summary = String::new();
        for action in &self.ledger.actions[start..] {
            summary.push_str(&format!(
                "Action {}: type={:?}, intent={}, plan={}, timestamp={}\n",
                action.action_id,
                action.action_type,
                action.intent_id,
                action.plan_id,
                action.timestamp
            ));
        }
        summary
    }

    /// Record a delegation lifecycle event (helper for M4)
    pub fn record_delegation_event(
        &mut self,
        session_id: Option<&str>,
        intent_id: &IntentId,
        event_kind: &str,
        metadata: std::collections::HashMap<String, Value>,
    ) -> Result<(), RuntimeError> {
        let mut action = Action {
            action_id: uuid::Uuid::new_v4().to_string(),
            parent_action_id: None,
            session_id: session_id.map(|s| s.to_string()),
            plan_id: "delegation".to_string(),
            intent_id: intent_id.clone(),
            action_type: ActionType::InternalStep,
            function_name: Some(format!("delegation.{}", event_kind)),
            arguments: None,
            result: None,
            cost: None,
            duration_ms: None,
            timestamp: SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_millis() as u64,
            metadata,
        };
        // Sign & attach
        let signature = self.signing.sign_action(&action);
        action
            .metadata
            .insert("signature".to_string(), Value::String(signature));
        // Append, record metrics (internal step), notify sinks
        self.ledger.append_action(&action)?;
        self.metrics.record_action(&action)?;
        self.notify_sinks(&action);
        // Log structured line
        self.log_action_json(&action, "delegation_event");
        Ok(())
    }

    /// Return up to `max` most recent structured log lines as JSON strings
    pub fn recent_logs(&self, max: usize) -> Vec<String> {
        self.logs.recent(max)
    }

    /// Get Working Memory ingest latency metrics
    pub fn get_wm_ingest_latency_metrics(
        &self,
    ) -> &crate::causal_chain::metrics::WmIngestLatencyMetrics {
        self.metrics.get_wm_ingest_latency_metrics()
    }

    // ---------------------------------------------------------------------
    // Export and Replay Support
    // ---------------------------------------------------------------------

    /// Export all actions for a given plan as a serializable format for replay/audit
    /// Returns actions in chronological order with full context needed for replay
    ///
    /// Actions are logged in real-time as events occur during plan execution.
    /// This function ensures chronological order by sorting by timestamp.
    pub fn export_plan_actions(&self, plan_id: &PlanId) -> Vec<&Action> {
        let mut actions = self.get_actions_for_plan(plan_id);
        actions.sort_by_key(|a| a.timestamp);
        actions
    }

    /// Export all actions for a given intent as a serializable format for replay/audit
    /// Returns actions in chronological order with full context needed for replay
    ///
    /// Actions are logged in real-time as events occur during intent execution.
    /// This function ensures chronological order by sorting by timestamp.
    pub fn export_intent_actions(&self, intent_id: &IntentId) -> Vec<&Action> {
        let mut actions = self.get_actions_for_intent(intent_id);
        actions.sort_by_key(|a| a.timestamp);
        actions
    }

    /// Export all actions in chronological order for full chain replay
    /// This preserves the complete immutable audit trail
    ///
    /// Actions are logged in real-time as events occur, so the ledger's Vec insertion
    /// order should match chronological order. This function sorts by timestamp to
    /// guarantee chronological correctness even if there are edge cases.
    pub fn export_all_actions(&self) -> Vec<&Action> {
        let actions = self.get_all_actions();
        // Actions are appended to Vec in real-time as events occur, preserving
        // insertion order. We sort by timestamp to guarantee chronological order.
        let mut sorted: Vec<&Action> = actions.iter().collect();
        sorted.sort_by_key(|a| a.timestamp);
        sorted
    }

    /// Export actions matching a query in chronological order
    /// Useful for selective replay of specific event types or time ranges
    ///
    /// Actions are logged in real-time as events occur, preserving chronological order.
    /// This function ensures chronological order by sorting by timestamp.
    pub fn export_actions(&self, query: &CausalQuery) -> Vec<&Action> {
        let mut actions = self.query_actions(query);
        actions.sort_by_key(|a| a.timestamp);
        actions
    }

    /// Get execution trace for a plan - ordered sequence of actions with parent-child relationships
    /// Returns a tree-structured representation suitable for replay
    ///
    /// Actions are logged in real-time during plan execution, preserving chronological order.
    /// This function ensures actions are sorted by timestamp for guaranteed chronological correctness.
    pub fn get_plan_execution_trace(&self, plan_id: &PlanId) -> Vec<&Action> {
        // Get all plan actions (already in insertion order, which should match chronological)
        let mut all_actions = self.get_actions_for_plan(plan_id);

        // Sort by timestamp to guarantee chronological order
        all_actions.sort_by_key(|a| a.timestamp);

        all_actions
    }

    /// Verify chain integrity and return diagnostic information
    /// Returns (is_valid, total_actions, first_timestamp, last_timestamp)
    pub fn verify_and_summarize(
        &self,
    ) -> Result<(bool, usize, Option<u64>, Option<u64>), RuntimeError> {
        let is_valid = self.verify_integrity()?;
        let total_actions = self.get_action_count();
        let actions = self.get_all_actions();

        let first_timestamp = actions.first().map(|a| a.timestamp);
        let last_timestamp = actions.last().map(|a| a.timestamp);

        Ok((is_valid, total_actions, first_timestamp, last_timestamp))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rtfs::runtime::values::Value;

    #[test]
    fn test_causal_chain_creation() {
        let chain = CausalChain::new();
        assert!(chain.is_ok());
    }

    #[test]
    fn test_action_creation_and_recording() {
        let mut chain = CausalChain::new().unwrap();
        let intent = Intent::new("Test goal".to_string());

        // Simulate privileged system providing real audit metadata
        let mut audit_metadata = HashMap::new();
        audit_metadata.insert(
            "constitutional_rule_id".to_string(),
            Value::String("rule-777".to_string()),
        );
        audit_metadata.insert(
            "delegation_decision_id".to_string(),
            Value::String("deleg-abc123".to_string()),
        );
        audit_metadata.insert(
            "capability_attestation_id".to_string(),
            Value::String("attest-xyz789".to_string()),
        );
        audit_metadata.insert(
            "triggered_by".to_string(),
            Value::String("system-orchestrator".to_string()),
        );
        audit_metadata.insert(
            "audit_trail".to_string(),
            Value::String("audit-log-999".to_string()),
        );

        let action = chain
            .create_action(intent, Some(audit_metadata.clone()))
            .unwrap();
        assert_eq!(action.function_name.as_ref().unwrap(), "execute_intent");

        // Check audit metadata
        assert_eq!(
            action.metadata.get("constitutional_rule_id"),
            Some(&Value::String("rule-777".to_string()))
        );
        assert_eq!(
            action.metadata.get("delegation_decision_id"),
            Some(&Value::String("deleg-abc123".to_string()))
        );
        assert_eq!(
            action.metadata.get("capability_attestation_id"),
            Some(&Value::String("attest-xyz789".to_string()))
        );
        assert_eq!(
            action.metadata.get("triggered_by"),
            Some(&Value::String("system-orchestrator".to_string()))
        );
        assert_eq!(
            action.metadata.get("audit_trail"),
            Some(&Value::String("audit-log-999".to_string()))
        );

        let result = ExecutionResult {
            success: true,
            value: Value::Float(42.0),
            metadata: HashMap::new(),
        };

        // Clone action to avoid partial move error
        assert!(chain.record_result(action.clone(), result.clone()).is_ok());

        // Query by audit metadata
        let query = CausalQuery {
            intent_id: Some(action.intent_id.clone()),
            plan_id: Some(action.plan_id.clone()),
            action_type: Some(action.action_type.clone()),
            time_range: None,
            parent_action_id: None,
            function_prefix: None,
        };
        let results = chain.query_actions(&query);
        assert!(results
            .iter()
            .any(|a| a.metadata.get("constitutional_rule_id")
                == Some(&Value::String("rule-777".to_string()))));
    }

    #[test]
    fn test_ledger_integrity() {
        let mut chain = CausalChain::new().unwrap();
        let intent = Intent::new("Test goal".to_string());
        let mut audit_metadata = HashMap::new();
        audit_metadata.insert(
            "constitutional_rule_id".to_string(),
            Value::String("rule-888".to_string()),
        );
        audit_metadata.insert(
            "delegation_decision_id".to_string(),
            Value::String("deleg-xyz888".to_string()),
        );
        audit_metadata.insert(
            "capability_attestation_id".to_string(),
            Value::String("attest-abc888".to_string()),
        );
        audit_metadata.insert(
            "triggered_by".to_string(),
            Value::String("test-integrity".to_string()),
        );
        audit_metadata.insert(
            "audit_trail".to_string(),
            Value::String("audit-log-888".to_string()),
        );
        let action = chain.create_action(intent, Some(audit_metadata)).unwrap();

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
        let mut audit_metadata = HashMap::new();
        audit_metadata.insert(
            "constitutional_rule_id".to_string(),
            Value::String("rule-999".to_string()),
        );
        audit_metadata.insert(
            "delegation_decision_id".to_string(),
            Value::String("deleg-xyz999".to_string()),
        );
        audit_metadata.insert(
            "capability_attestation_id".to_string(),
            Value::String("attest-abc999".to_string()),
        );
        audit_metadata.insert(
            "triggered_by".to_string(),
            Value::String("test-metrics".to_string()),
        );
        audit_metadata.insert(
            "audit_trail".to_string(),
            Value::String("audit-log-999".to_string()),
        );
        let action = chain.create_action(intent, Some(audit_metadata)).unwrap();

        let result = ExecutionResult {
            success: true,
            value: Value::Nil,
            metadata: HashMap::new(),
        };

        chain.record_result(action, result).unwrap();

        assert_eq!(chain.get_total_cost(), 0.0); // Default cost is 0.0
    }

    #[test]
    fn test_event_sink_notification() {
        use std::sync::{Arc, Mutex};

        // Simple mock sink that records received action ids.
        #[derive(Debug)]
        struct MockSink {
            received: Arc<Mutex<Vec<String>>>,
        }

        impl crate::event_sink::CausalChainEventSink for MockSink {
            fn on_action_appended(&self, action: &Action) {
                let mut guard = self.received.lock().unwrap();
                guard.push(action.action_id.clone());
            }
        }

        let mut chain = CausalChain::new().unwrap();

        // Shared vec to assert notifications
        let received = Arc::new(Mutex::new(Vec::new()));
        let sink = MockSink {
            received: received.clone(),
        };

        // Register sink
        let arc_sink: Arc<dyn crate::event_sink::CausalChainEventSink> = Arc::new(sink);
        chain.register_event_sink(arc_sink);

        // Create and record an action - record_result triggers notify_sinks
        let intent = Intent::new("Notify goal".to_string());
        let action = chain.create_action(intent, None).unwrap();

        let result = ExecutionResult {
            success: true,
            value: Value::Nil,
            metadata: HashMap::new(),
        };

        chain.record_result(action.clone(), result).unwrap();

        // Ensure the sink got the action id
        let guard = received.lock().unwrap();
        assert!(guard.iter().any(|id| id == &action.action_id));
    }

    #[test]
    fn test_export_plan_actions_chronological_order() {
        let mut chain = CausalChain::new().unwrap();
        let plan_id = "plan-export-test".to_string();
        let intent_id = "intent-export-test".to_string();

        // Create actions with varying timestamps (simulate real-time logging)
        let action1 = Action::new(ActionType::PlanStarted, plan_id.clone(), intent_id.clone());
        std::thread::sleep(std::time::Duration::from_millis(10));
        let action2 = Action::new(
            ActionType::CapabilityCall,
            plan_id.clone(),
            intent_id.clone(),
        )
        .with_name("test.capability");
        std::thread::sleep(std::time::Duration::from_millis(10));
        let action3 = Action::new(
            ActionType::PlanCompleted,
            plan_id.clone(),
            intent_id.clone(),
        );

        // Append actions (not in chronological order to test sorting)
        chain.append(&action3).unwrap();
        chain.append(&action1).unwrap();
        chain.append(&action2).unwrap();

        // Export and verify chronological order
        let exported = chain.export_plan_actions(&plan_id);
        assert_eq!(exported.len(), 3);
        assert!(exported[0].timestamp <= exported[1].timestamp);
        assert!(exported[1].timestamp <= exported[2].timestamp);
        assert_eq!(exported[0].action_type, ActionType::PlanStarted);
        assert_eq!(exported[1].action_type, ActionType::CapabilityCall);
        assert_eq!(exported[2].action_type, ActionType::PlanCompleted);
    }

    #[test]
    fn test_export_intent_actions_chronological_order() {
        let mut chain = CausalChain::new().unwrap();
        let plan_id = "plan-intent-export".to_string();
        let intent_id = "intent-export-test-2".to_string();

        let action1 = Action::new(
            ActionType::IntentCreated,
            plan_id.clone(),
            intent_id.clone(),
        );
        std::thread::sleep(std::time::Duration::from_millis(10));
        let action2 = Action::new(
            ActionType::IntentStatusChanged,
            plan_id.clone(),
            intent_id.clone(),
        );

        chain.append(&action2).unwrap();
        chain.append(&action1).unwrap();

        let exported = chain.export_intent_actions(&intent_id);
        assert_eq!(exported.len(), 2);
        assert!(exported[0].timestamp <= exported[1].timestamp);
    }

    #[test]
    fn test_export_all_actions_chronological_order() {
        let mut chain = CausalChain::new().unwrap();
        let plan_id1 = "plan-1".to_string();
        let plan_id2 = "plan-2".to_string();
        let intent_id = "intent-all".to_string();

        let action1 = Action::new(ActionType::PlanStarted, plan_id1.clone(), intent_id.clone());
        std::thread::sleep(std::time::Duration::from_millis(10));
        let action2 = Action::new(ActionType::PlanStarted, plan_id2.clone(), intent_id.clone());
        std::thread::sleep(std::time::Duration::from_millis(10));
        let action3 = Action::new(
            ActionType::PlanCompleted,
            plan_id1.clone(),
            intent_id.clone(),
        );

        // Append out of order
        chain.append(&action3).unwrap();
        chain.append(&action1).unwrap();
        chain.append(&action2).unwrap();

        let exported = chain.export_all_actions();
        assert_eq!(exported.len(), 3);
        // Verify chronological order
        for i in 0..(exported.len() - 1) {
            assert!(
                exported[i].timestamp <= exported[i + 1].timestamp,
                "Actions not in chronological order at index {}",
                i
            );
        }
    }

    #[test]
    fn test_get_plan_execution_trace() {
        let mut chain = CausalChain::new().unwrap();
        let plan_id = "plan-trace-test".to_string();
        let intent_id = "intent-trace".to_string();

        let action1 = Action::new(ActionType::PlanStarted, plan_id.clone(), intent_id.clone());
        std::thread::sleep(std::time::Duration::from_millis(10));
        let _action2_id = format!("action-{}", uuid::Uuid::new_v4());
        let action2 = Action::new(
            ActionType::CapabilityCall,
            plan_id.clone(),
            intent_id.clone(),
        )
        .with_parent(Some(action1.action_id.clone()))
        .with_name("test.capability");
        std::thread::sleep(std::time::Duration::from_millis(10));
        let action3 = Action::new(
            ActionType::PlanCompleted,
            plan_id.clone(),
            intent_id.clone(),
        )
        .with_parent(Some(action2.action_id.clone()));

        chain.append(&action3).unwrap();
        chain.append(&action1).unwrap();
        chain.append(&action2).unwrap();

        let trace = chain.get_plan_execution_trace(&plan_id);
        assert_eq!(trace.len(), 3);
        // Verify chronological order
        assert!(trace[0].timestamp <= trace[1].timestamp);
        assert!(trace[1].timestamp <= trace[2].timestamp);
        // Verify parent-child relationships are preserved
        assert_eq!(trace[0].action_type, ActionType::PlanStarted);
        assert_eq!(trace[1].parent_action_id, Some(trace[0].action_id.clone()));
    }

    #[test]
    fn test_verify_and_summarize() {
        let mut chain = CausalChain::new().unwrap();
        let intent = Intent::new("Test summarize".to_string());
        let action = chain.create_action(intent, None).unwrap();

        let result = ExecutionResult {
            success: true,
            value: Value::Nil,
            metadata: HashMap::new(),
        };

        chain.record_result(action, result).unwrap();

        let (is_valid, total_actions, first_ts, last_ts) = chain.verify_and_summarize().unwrap();

        assert!(is_valid);
        assert!(total_actions > 0);
        assert!(first_ts.is_some());
        assert!(last_ts.is_some());
        assert!(first_ts.unwrap() <= last_ts.unwrap());
    }

    #[test]
    fn test_export_actions_with_query() {
        let mut chain = CausalChain::new().unwrap();
        let plan_id = "plan-query".to_string();
        let intent_id = "intent-query".to_string();

        let action1 = Action::new(ActionType::PlanStarted, plan_id.clone(), intent_id.clone());
        std::thread::sleep(std::time::Duration::from_millis(10));
        let action2 = Action::new(
            ActionType::CapabilityCall,
            plan_id.clone(),
            intent_id.clone(),
        )
        .with_name("test.capability");
        std::thread::sleep(std::time::Duration::from_millis(10));
        let action3 = Action::new(ActionType::InternalStep, plan_id.clone(), intent_id.clone());

        chain.append(&action1).unwrap();
        chain.append(&action2).unwrap();
        chain.append(&action3).unwrap();

        // Query for only CapabilityCall actions
        let query = CausalQuery {
            intent_id: None,
            plan_id: Some(plan_id.clone()),
            action_type: Some(ActionType::CapabilityCall),
            time_range: None,
            parent_action_id: None,
            function_prefix: None,
        };

        let exported = chain.export_actions(&query);
        assert_eq!(exported.len(), 1);
        assert_eq!(exported[0].action_type, ActionType::CapabilityCall);
    }
}
