//! Working Memory ingestion sink for Causal Chain events
//!
//! Bridges Causal Chain actions to Working Memory using the existing MemoryIngestor.
//! Maps Action objects to ActionRecord format for seamless integration.

use std::sync::{Arc, Mutex};

use super::event_sink::CausalChainEventSink;
use super::types::Action;
use super::working_memory::facade::WorkingMemory;
use super::working_memory::ingestor::{ActionRecord, MemoryIngestor};
use crate::runtime::values::Value;

/// Working Memory ingestion sink that subscribes to Causal Chain events
pub struct WmIngestionSink {
    wm: Arc<Mutex<WorkingMemory>>,
}

impl std::fmt::Debug for WmIngestionSink {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("WmIngestionSink")
            .field("wm", &"Arc<Mutex<WorkingMemory>>")
            .finish()
    }
}

impl WmIngestionSink {
    /// Create a new Working Memory ingestion sink
    pub fn new(wm: Arc<Mutex<WorkingMemory>>) -> Self {
        Self { wm }
    }

    /// Map a Causal Chain Action to a Working Memory ActionRecord
    pub fn map_action_to_record(action: &Action) -> ActionRecord {
        // Convert millis to seconds for WM ingestor
        let ts_s = action.timestamp / 1000;

        let kind = format!("{:?}", action.action_type);
        let provider = action.function_name.clone();

        // Extract attestation if present
        let attestation_hash = action.metadata.get("signature").and_then(|v| {
            if let Value::String(s) = v {
                Some(s.clone())
            } else {
                None
            }
        });

        // Compact content payload for idempotent hashing and human scan
        let args_str = action
            .arguments
            .as_ref()
            .map(|args| format!("{:?}", args))
            .unwrap_or_default();
        let result_str = action
            .result
            .as_ref()
            .map(|r| format!("{:?}", r))
            .unwrap_or_default();
        let meta_str = if action.metadata.is_empty() {
            String::new()
        } else {
            format!("{:?}", action.metadata)
        };

        let content = format!(
            "args={}; result={}; meta={}",
            args_str, result_str, meta_str
        );
        let summary = provider.clone().unwrap_or_else(|| kind.clone());

        ActionRecord {
            action_id: action.action_id.clone(),
            kind,
            provider,
            timestamp_s: ts_s,
            summary,
            content,
            plan_id: Some(action.plan_id.clone()),
            intent_id: Some(action.intent_id.clone()),
            step_id: None, // Optionally populate if a step id is available via metadata
            attestation_hash,
            content_hash: None, // Let MemoryIngestor compute deterministic hash
        }
    }
}

impl CausalChainEventSink for WmIngestionSink {
    fn on_action_appended(&self, action: &Action) {
        if let Ok(mut wm) = self.wm.lock() {
            let record = Self::map_action_to_record(action);
            let _ = MemoryIngestor::ingest_action(&mut wm, &record); // idempotent
        }
    }
}

/// Rebuild Working Memory from Causal Chain ledger (replay utility)
pub fn rebuild_working_memory_from_ledger(
    wm: &mut WorkingMemory,
    chain: &super::causal_chain::CausalChain,
) -> Result<(), super::working_memory::backend::WorkingMemoryError> {
    let mut records = Vec::new();
    for action in chain.get_all_actions().iter() {
        let record = WmIngestionSink::map_action_to_record(action);
        records.push(record);
    }
    MemoryIngestor::replay_all(wm, &records)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ccos::types::{Action, ActionType, ExecutionResult};
    use crate::ccos::working_memory::backend::QueryParams;
    use crate::ccos::working_memory::backend_inmemory::InMemoryJsonlBackend;
    use crate::runtime::values::Value;
    use std::collections::HashMap;

    use uuid::Uuid;

    fn create_test_action(action_type: ActionType, function_name: Option<String>) -> Action {
        let mut action = Action::new(
            action_type,
            format!("plan-{}", Uuid::new_v4()),
            format!("intent-{}", Uuid::new_v4()),
        );

        if let Some(fname) = function_name {
            action = action.with_name(&fname);
        }

        action = action.with_args(vec![Value::String("test-arg".to_string())]);

        let result = ExecutionResult {
            success: true,
            value: Value::String("test-result".to_string()),
            metadata: HashMap::new(),
        };
        action = action.with_result(result);

        // Add signature metadata like CausalChain does
        action.metadata.insert(
            "signature".to_string(),
            Value::String("test-signature".to_string()),
        );

        action
    }

    #[test]
    fn test_action_mapping() {
        let action = create_test_action(
            ActionType::CapabilityCall,
            Some("test-capability".to_string()),
        );
        let record = WmIngestionSink::map_action_to_record(&action);

        assert_eq!(record.action_id, action.action_id);
        assert_eq!(record.kind, "CapabilityCall");
        assert_eq!(record.provider, Some("test-capability".to_string()));
        assert_eq!(record.summary, "test-capability");
        assert!(record.content.contains("args="));
        assert!(record.content.contains("result="));
        assert!(record.content.contains("meta="));
        assert_eq!(record.attestation_hash, Some("test-signature".to_string()));
    }

    #[test]
    fn test_wm_ingestion_sink_integration() {
        let backend = InMemoryJsonlBackend::new(None, Some(10), Some(10_000));
        let wm = Arc::new(Mutex::new(WorkingMemory::new(Box::new(backend))));
        let sink = WmIngestionSink::new(wm.clone());

        // Simulate causal chain appending an action
        let action = create_test_action(ActionType::PlanStarted, None);
        sink.on_action_appended(&action);

        // Verify the action was ingested into working memory
        let wm_lock = wm.lock().unwrap();
        let results = wm_lock.query(&QueryParams::default()).unwrap();
        assert_eq!(results.entries.len(), 1);

        let entry = &results.entries[0];
        assert!(entry.tags.contains("causal-chain"));
        assert!(entry.tags.contains("distillation"));
        assert!(entry.tags.contains("wisdom"));
        assert!(entry.tags.contains("planstarted")); // lowercased action type
    }

    #[test]
    fn test_replay_utility() {
        use crate::ccos::causal_chain::CausalChain;

        let mut chain = CausalChain::new().unwrap();
        let action1 = create_test_action(ActionType::PlanStarted, None);
        let action2 = create_test_action(ActionType::CapabilityCall, Some("test-cap".to_string()));

        // Simulate adding actions to the chain (simplified)
        chain
            .record_result(
                action1.clone(),
                ExecutionResult {
                    success: true,
                    value: Value::String("result1".to_string()),
                    metadata: HashMap::new(),
                },
            )
            .unwrap();
        chain
            .record_result(
                action2.clone(),
                ExecutionResult {
                    success: true,
                    value: Value::String("result2".to_string()),
                    metadata: HashMap::new(),
                },
            )
            .unwrap();

        // Now rebuild working memory from the chain
        let backend = InMemoryJsonlBackend::new(None, Some(10), Some(10_000));
        let mut wm = WorkingMemory::new(Box::new(backend));

        rebuild_working_memory_from_ledger(&mut wm, &chain).unwrap();

        let results = wm.query(&QueryParams::default()).unwrap();
        assert_eq!(results.entries.len(), 2);

        // Verify entries are properly tagged
        for entry in &results.entries {
            assert!(entry.tags.contains("causal-chain"));
            assert!(entry.tags.contains("distillation"));
            assert!(entry.tags.contains("wisdom"));
        }
    }
}
