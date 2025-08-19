//! Integration tests for mandatory intent lifecycle audit via IntentEventSink
//!
//! This test module validates that intent status transitions are properly audited
//! through the IntentEventSink abstraction and recorded in the CausalChain.

use rtfs_compiler::ccos::{
    event_sink::{IntentEventSink, NoopIntentEventSink},
    types::{IntentStatus, ExecutionResult, StorableIntent, TriggerSource, GenerationContext, ActionType},
    intent_graph::{IntentGraph, IntentGraphConfig, IntentLifecycleManager},
};
use rtfs_compiler::runtime::{RuntimeError, values::Value};
use std::sync::{Arc, Mutex};
use std::collections::HashMap;
use std::time::{SystemTime, UNIX_EPOCH};

/// Simple event sink that records events for testing
#[derive(Debug)]
struct MockIntentEventSink {
    events: Arc<Mutex<Vec<(String, IntentStatus, IntentStatus, String, Option<String>)>>>,
}

impl MockIntentEventSink {
    fn new() -> Self {
        Self {
            events: Arc::new(Mutex::new(Vec::new())),
        }
    }
    
    fn get_events(&self) -> Vec<(String, IntentStatus, IntentStatus, String, Option<String>)> {
        self.events.lock().unwrap().clone()
    }
}

impl IntentEventSink for MockIntentEventSink {
    fn emit_status_change(
        &self,
        intent_id: &String,
        old_status: &IntentStatus,
        new_status: &IntentStatus,
        reason: &str,
        triggering_plan_id: Option<&str>,
    ) -> Result<(), RuntimeError> {
        let mut events = self.events.lock().unwrap();
        events.push((
            intent_id.clone(),
            old_status.clone(),
            new_status.clone(),
            reason.to_string(),
            triggering_plan_id.map(|s| s.to_string()),
        ));
        Ok(())
    }
}

/// Helper function to create a test intent
fn create_test_intent(id: &str, goal: &str) -> StorableIntent {
    let timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs();

    StorableIntent {
        intent_id: id.to_string(),
        name: Some(format!("Test Intent {}", id)),
        original_request: format!("Original request for {}", goal),
        rtfs_intent_source: format!("(intent \"{}\")", goal),
        goal: goal.to_string(),
        constraints: HashMap::new(),
        preferences: HashMap::new(),
        success_criteria: None,
        parent_intent: None,
        child_intents: Vec::new(),
        triggered_by: TriggerSource::HumanRequest,
        generation_context: GenerationContext {
            arbiter_version: "test-1.0".to_string(),
            generation_timestamp: timestamp,
            input_context: HashMap::new(),
            reasoning_trace: None,
        },
        status: IntentStatus::Active,
        priority: 1,
        created_at: timestamp,
        updated_at: timestamp,
        metadata: HashMap::new(),
    }
}

#[tokio::test]
async fn test_noop_event_sink() {
    // Test that NoopIntentEventSink doesn't fail
    let sink = NoopIntentEventSink;
    
    let result = sink.emit_status_change(
        &"test-intent-1".to_string(),
        &IntentStatus::Active,
        &IntentStatus::Completed,
        "Test transition",
        Some("test-plan"),
    );
    
    assert!(result.is_ok(), "NoopIntentEventSink should never fail");
}

#[tokio::test]
async fn test_intent_graph_with_event_sink() {
    // Create an IntentGraph with a mock event sink
    let mock_sink = Arc::new(MockIntentEventSink::new());
    let sink_clone = mock_sink.clone();
    let mut intent_graph = IntentGraph::with_event_sink(sink_clone)
        .expect("Failed to create IntentGraph with event sink");
    
    // Create and store a test intent
    let intent = create_test_intent("test-intent-1", "Test goal");
    intent_graph.store_intent(intent.clone())
        .expect("Failed to store intent");
    
    // Create a successful execution result and update the intent
    let success_result = ExecutionResult {
        success: true,
        value: Value::String("Completed successfully".to_string()),
        metadata: HashMap::new(),
    };
    
    intent_graph.update_intent(intent.clone(), &success_result)
        .expect("Failed to update intent with success result");
    
    // Verify the event sink recorded the status change event
    let events = mock_sink.get_events();
    
    // Should have at least one status change event
    assert!(!events.is_empty(), "Should have at least one status change event");
    
    // Verify the event contains the expected status transition
    let (event_intent_id, old_status, new_status, reason, _plan_id) = &events[0];
    assert_eq!(event_intent_id, &intent.intent_id);
    assert_eq!(old_status, &IntentStatus::Active);
    assert_eq!(new_status, &IntentStatus::Completed);
    assert!(reason.contains("successfully"), "Reason should mention success: {}", reason);
}

#[tokio::test]
async fn test_intent_failure_audit() {
    // Test that failed execution results transition to Failed status and emit audit
    let mock_sink = Arc::new(MockIntentEventSink::new());
    let sink_clone = mock_sink.clone();
    let mut intent_graph = IntentGraph::with_event_sink(sink_clone)
        .expect("Failed to create IntentGraph with event sink");
    
    // Create and store a test intent
    let intent = create_test_intent("test-intent-2", "Test goal for failure");
    intent_graph.store_intent(intent.clone())
        .expect("Failed to store intent");
    
    // Create a failed execution result and update the intent
    let failure_result = ExecutionResult {
        success: false,
        value: Value::String("Something went wrong".to_string()),
        metadata: HashMap::new(),
    };
    
    intent_graph.update_intent(intent.clone(), &failure_result)
        .expect("Failed to update intent with failure result");
    
    // Verify the event sink recorded the status change event to Failed
    let events = mock_sink.get_events();
    
    assert!(!events.is_empty(), "Should have status change event for failure");
    
    // Verify the event shows transition to Failed status
    let (event_intent_id, old_status, new_status, reason, _plan_id) = &events[0];
    assert_eq!(event_intent_id, &intent.intent_id);
    assert_eq!(old_status, &IntentStatus::Active);
    assert_eq!(new_status, &IntentStatus::Failed);
    assert!(reason.contains("failed"), "Reason should mention failure: {}", reason);
}

#[tokio::test]
async fn test_lifecycle_manager_direct_audit() {
    // Test the IntentLifecycleManager directly with event sink
    let mock_sink = Arc::new(MockIntentEventSink::new());
    let sink_clone = mock_sink.clone();
    let lifecycle = IntentLifecycleManager;
    
    // Create test storage (using the IntentGraph's storage as a proxy)
    let config = IntentGraphConfig::default();
    let mut storage = rtfs_compiler::ccos::intent_graph::IntentGraphStorage::new(config).await;
    
    let intent = create_test_intent("test-intent-3", "Direct lifecycle test");
    storage.store_intent(intent.clone()).await
        .expect("Failed to store intent");
    
    // Test successful completion
    let success_result = ExecutionResult {
        success: true,
        value: Value::String("Success".to_string()),
        metadata: HashMap::new(),
    };
    
    lifecycle.complete_intent(&mut storage, sink_clone.as_ref(), &intent.intent_id, &success_result).await
        .expect("Failed to complete intent");
    
    // Verify audit in event sink
    let events = mock_sink.get_events();
    
    assert!(!events.is_empty(), "Should have status change event");
    
    let (event_intent_id, old_status, new_status, reason, _plan_id) = &events[0];
    assert_eq!(event_intent_id, &intent.intent_id);
    assert_eq!(old_status, &IntentStatus::Active);
    assert_eq!(new_status, &IntentStatus::Completed);
    assert!(reason.contains("successfully"), "Reason should mention success: {}", reason);
    
    // Now test failure case
    let failure_result = ExecutionResult {
        success: false,
        value: Value::String("Failure".to_string()),
        metadata: HashMap::new(),
    };
    
    let intent2 = create_test_intent("test-intent-4", "Direct lifecycle failure test");
    storage.store_intent(intent2.clone()).await
        .expect("Failed to store intent2");
    
    lifecycle.complete_intent(&mut storage, sink_clone.as_ref(), &intent2.intent_id, &failure_result).await
        .expect("Failed to complete intent with failure");
    
    // Verify failed intent gets Failed status
    let events2 = mock_sink.get_events();
    let failure_event = events2.iter().find(|(id, _, _, _, _)| id == &intent2.intent_id)
        .expect("Should have event for failed intent");
    
    let (_, old_status, new_status, reason, _) = failure_event;
    assert_eq!(old_status, &IntentStatus::Active);
    assert_eq!(new_status, &IntentStatus::Failed);
    assert!(reason.contains("failed"), "Reason should mention failure: {}", reason);
}

#[tokio::test]
async fn test_all_status_transitions_audited() {
    // Test that all status transition methods emit audit events
    let mock_sink = Arc::new(MockIntentEventSink::new());
    let sink_clone = mock_sink.clone();
    let lifecycle = IntentLifecycleManager;
    
    let config = IntentGraphConfig::default();
    let mut storage = rtfs_compiler::ccos::intent_graph::IntentGraphStorage::new(config).await;
    
    let intent = create_test_intent("test-intent-5", "All transitions test");
    storage.store_intent(intent.clone()).await
        .expect("Failed to store intent");
    
    // Test suspend
    lifecycle.suspend_intent(&mut storage, sink_clone.as_ref(), &intent.intent_id, "Test suspend".to_string()).await
        .expect("Failed to suspend intent");
    
    // Test resume
    lifecycle.resume_intent(&mut storage, sink_clone.as_ref(), &intent.intent_id, "Test resume".to_string()).await
        .expect("Failed to resume intent");
    
    // Test archive
    lifecycle.archive_intent(&mut storage, sink_clone.as_ref(), &intent.intent_id, "Test archive".to_string()).await
        .expect("Failed to archive intent");
    
    // Test reactivate
    lifecycle.reactivate_intent(&mut storage, sink_clone.as_ref(), &intent.intent_id, "Test reactivate".to_string()).await
        .expect("Failed to reactivate intent");
    
    // Verify all transitions were audited
    let events = mock_sink.get_events();
    
    // Should have 4 status change events (suspend, resume, archive, reactivate)
    assert_eq!(events.len(), 4, "Should have exactly 4 status change events, got {}", events.len());
    
    // Verify each transition type is present
    let reasons: Vec<String> = events.iter().map(|(_, _, _, reason, _)| reason.clone()).collect();
    
    assert!(reasons.iter().any(|r| r.contains("suspend")), "Should have suspend transition");
    assert!(reasons.iter().any(|r| r.contains("resume")), "Should have resume transition");
    assert!(reasons.iter().any(|r| r.contains("archive")), "Should have archive transition");
    assert!(reasons.iter().any(|r| r.contains("reactivate")), "Should have reactivate transition");
    
    // Verify the status progression
    let expected_statuses = [
        (IntentStatus::Active, IntentStatus::Suspended),    // suspend
        (IntentStatus::Suspended, IntentStatus::Active),    // resume
        (IntentStatus::Active, IntentStatus::Archived),     // archive
        (IntentStatus::Archived, IntentStatus::Active),     // reactivate
    ];
    
    for (i, (expected_old, expected_new)) in expected_statuses.iter().enumerate() {
        let (_, old_status, new_status, _, _) = &events[i];
        assert_eq!(old_status, expected_old, "Event {} should have old status {:?}, got {:?}", i, expected_old, old_status);
        assert_eq!(new_status, expected_new, "Event {} should have new status {:?}, got {:?}", i, expected_new, new_status);
    }
}