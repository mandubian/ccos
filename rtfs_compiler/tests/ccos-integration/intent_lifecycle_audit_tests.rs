use std::sync::{Arc, Mutex};

use rtfs_compiler::ccos::causal_chain::CausalChain;
use rtfs_compiler::ccos::event_sink::CausalChainIntentEventSink;
use rtfs_compiler::ccos::intent_graph::IntentGraph; // re-exported in mod
use rtfs_compiler::ccos::types::{
    Action, ActionType, ExecutionResult, IntentStatus, StorableIntent,
};
use rtfs_compiler::runtime::values::Value;

fn mk_graph_with_chain() -> (Arc<Mutex<CausalChain>>, IntentGraph) {
    let chain = Arc::new(Mutex::new(CausalChain::new().expect("causal chain")));
    let sink = Arc::new(CausalChainIntentEventSink::new(Arc::clone(&chain)));
    // IntentGraph::with_event_sink expects just the sink (config defaults)
    let graph = IntentGraph::with_event_sink(sink).expect("intent graph");
    (chain, graph)
}

fn has_status(actions: &[&Action], status_sub: &str) -> bool {
    actions.iter().any(|a| {
        a.action_type == ActionType::IntentStatusChanged
            && a.metadata
                .get("new_status")
                .map(|v| v.to_string())
                .unwrap_or_default()
                .contains(status_sub)
    })
}

#[test]
fn complete_and_fail_emit_audit() {
    let (chain, mut graph) = mk_graph_with_chain();

    // Completed case
    let intent = StorableIntent::new("emit audit success".into());
    let id1 = intent.intent_id.clone();
    graph.store_intent(intent).unwrap();
    let exec_ok = ExecutionResult {
        success: true,
        value: Value::String("ok".into()),
        metadata: Default::default(),
    };
    graph.complete_intent(&id1, &exec_ok).unwrap();
    assert_eq!(
        graph.get_intent(&id1).unwrap().status,
        IntentStatus::Completed
    );
    {
        let c = chain.lock().unwrap();
        let actions = c.get_actions_for_intent(&id1);
        assert!(
            has_status(&actions, "Completed"),
            "Missing Completed status change action"
        );
    }

    // Failed case (complete_intent will map success=false to Failed)
    let intent2 = StorableIntent::new("emit audit fail".into());
    let id2 = intent2.intent_id.clone();
    graph.store_intent(intent2).unwrap();
    let exec_fail = ExecutionResult {
        success: false,
        value: Value::String("err".into()),
        metadata: Default::default(),
    };
    graph.complete_intent(&id2, &exec_fail).unwrap();
    assert_eq!(graph.get_intent(&id2).unwrap().status, IntentStatus::Failed);
    let c = chain.lock().unwrap();
    let actions2 = c.get_actions_for_intent(&id2);
    assert!(
        has_status(&actions2, "Failed"),
        "Missing Failed status change action"
    );
}

#[test]
fn suspend_and_resume_emit_audit() {
    let (chain, mut graph) = mk_graph_with_chain();
    let intent = StorableIntent::new("suspend resume audit".into());
    let id = intent.intent_id.clone();
    graph.store_intent(intent).unwrap();
    graph.suspend_intent(&id, "pause".into()).unwrap();
    assert_eq!(
        graph.get_intent(&id).unwrap().status,
        IntentStatus::Suspended
    );
    graph.resume_intent(&id, "resume".into()).unwrap();
    assert_eq!(graph.get_intent(&id).unwrap().status, IntentStatus::Active);
    let c = chain.lock().unwrap();
    let actions = c.get_actions_for_intent(&id);
    assert!(
        has_status(&actions, "Suspended"),
        "Missing Suspended status change action"
    );
    assert!(
        has_status(&actions, "Active"),
        "Missing Active status change action after resume"
    );
}

#[test]
fn archive_and_reactivate_emit_audit() {
    let (chain, mut graph) = mk_graph_with_chain();
    let intent = StorableIntent::new("archive reactivate audit".into());
    let id = intent.intent_id.clone();
    graph.store_intent(intent).unwrap();
    graph.archive_intent(&id, "done".to_string()).unwrap();
    assert_eq!(
        graph.get_intent(&id).unwrap().status,
        IntentStatus::Archived
    );
    graph.reactivate_intent(&id, "oops".to_string()).unwrap();
    assert_eq!(graph.get_intent(&id).unwrap().status, IntentStatus::Active);
    let c = chain.lock().unwrap();
    let actions = c.get_actions_for_intent(&id);
    assert!(
        has_status(&actions, "Archived"),
        "Missing Archived status change action"
    );
    assert!(
        has_status(&actions, "Active"),
        "Missing Active status change action after reactivate"
    );
}

#[test]
fn archive_completed_intents_emits_audit() {
    let (chain, mut graph) = mk_graph_with_chain();
    // create and complete two intents
    let intent1 = StorableIntent::new("to archive 1".into());
    let id1 = intent1.intent_id.clone();
    graph.store_intent(intent1).unwrap();
    let intent2 = StorableIntent::new("to archive 2".into());
    let id2 = intent2.intent_id.clone();
    graph.store_intent(intent2).unwrap();
    let exec_ok = ExecutionResult {
        success: true,
        value: Value::String("ok".into()),
        metadata: Default::default(),
    };
    graph.complete_intent(&id1, &exec_ok).unwrap();
    graph.complete_intent(&id2, &exec_ok).unwrap();
    // invoke archive completed helper
    graph.archive_completed_intents().unwrap();
    assert_eq!(
        graph.get_intent(&id1).unwrap().status,
        IntentStatus::Archived
    );
    assert_eq!(
        graph.get_intent(&id2).unwrap().status,
        IntentStatus::Archived
    );
    let c = chain.lock().unwrap();
    let a1 = c.get_actions_for_intent(&id1);
    let a2 = c.get_actions_for_intent(&id2);
    assert!(
        has_status(&a1, "Archived"),
        "Intent 1 missing Archived audit event"
    );
    assert!(
        has_status(&a2, "Archived"),
        "Intent 2 missing Archived audit event"
    );
}
