use std::sync::{Arc, Mutex};

use rtfs::ccos::causal_chain::CausalChain;
use rtfs::ccos::event_sink::CausalChainIntentEventSink;
use rtfs::ccos::governance_kernel::GovernanceKernel;
use rtfs::ccos::intent_graph::core::IntentGraph;
use rtfs::ccos::types::{EdgeType, IntentId, IntentStatus, Plan, StorableIntent};
use rtfs::runtime::security::RuntimeContext;

fn ensure_test_env() {
    // Avoid host fallback panics in CI
    std::env::set_var("CCOS_TEST_FALLBACK_CONTEXT", "1");
}

fn deps_completed(intent_graph: &Arc<Mutex<IntentGraph>>, intent_id: &IntentId) -> bool {
    let graph = intent_graph.lock().unwrap();
    let edges = graph.get_edges_for_intent(intent_id);
    let parents: Vec<IntentId> = edges
        .into_iter()
        .filter(|e| e.edge_type == EdgeType::DependsOn && e.from == *intent_id)
        .map(|e| e.to)
        .collect();
    for pid in parents {
        if let Some(p) = graph.get_intent(&pid) {
            if p.status != IntentStatus::Completed {
                return false;
            }
        } else {
            return false;
        }
    }
    true
}

#[test]
fn test_dependency_order_and_root_completion() {
    ensure_test_env();

    // Core systems
    let causal_chain = Arc::new(Mutex::new(CausalChain::new().unwrap()));
    let intent_graph = Arc::new(Mutex::new(
        IntentGraph::with_event_sink(Arc::new(CausalChainIntentEventSink::new(Arc::clone(
            &causal_chain,
        ))))
        .unwrap(),
    ));

    // Use GovernanceKernel instead of direct Orchestrator access
    let governance_kernel = GovernanceKernel::new(causal_chain.clone());

    // Build intents: root <-sub- fetch, analyze, announce
    let mut root = StorableIntent::new("Root goal".to_string());
    root.name = Some("root".into());
    let root_id = root.intent_id.clone();

    let mut fetch = StorableIntent::new("Fetch data".to_string());
    fetch.name = Some("fetch".into());
    fetch.parent_intent = Some(root_id.clone());
    let fetch_id = fetch.intent_id.clone();

    let mut analyze = StorableIntent::new("Analyze data".to_string());
    analyze.name = Some("analyze".into());
    analyze.parent_intent = Some(root_id.clone());
    let analyze_id = analyze.intent_id.clone();

    let mut announce = StorableIntent::new("Announce".to_string());
    announce.name = Some("announce".into());
    announce.parent_intent = Some(root_id.clone());
    let announce_id = announce.intent_id.clone();

    // Store intents and edges
    {
        let mut g = intent_graph.lock().unwrap();
        g.store_intent(root.clone()).unwrap();
        g.store_intent(fetch.clone()).unwrap();
        g.store_intent(analyze.clone()).unwrap();
        g.store_intent(announce.clone()).unwrap();
    }

    // IsSubgoalOf edges (child -> root)
    {
        let mut g = intent_graph.lock().unwrap();
        g.create_edge(fetch_id.clone(), root_id.clone(), EdgeType::IsSubgoalOf)
            .unwrap();
        g.create_edge(analyze_id.clone(), root_id.clone(), EdgeType::IsSubgoalOf)
            .unwrap();
        g.create_edge(announce_id.clone(), root_id.clone(), EdgeType::IsSubgoalOf)
            .unwrap();
    }

    // Dependencies: analyze depends on fetch; announce depends on analyze
    {
        let mut g = intent_graph.lock().unwrap();
        g.create_edge(analyze_id.clone(), fetch_id.clone(), EdgeType::DependsOn)
            .unwrap();
        g.create_edge(announce_id.clone(), analyze_id.clone(), EdgeType::DependsOn)
            .unwrap();
    }

    // Gating should initially be false for analyze and announce
    assert!(!deps_completed(&intent_graph, &analyze_id));
    assert!(!deps_completed(&intent_graph, &announce_id));

    // Plans: pure RTFS, no capabilities required
    let plan_fetch = Plan::new_rtfs(
        "(do (step \"Fetch\" 1))".to_string(),
        vec![fetch_id.clone()],
    );
    let plan_analyze = Plan::new_rtfs(
        "(do (step \"Analyze\" (+ 1 2)))".to_string(),
        vec![analyze_id.clone()],
    );
    let plan_announce = Plan::new_rtfs(
        "(do (step \"Announce\" (str \"done-\" 42)))".to_string(),
        vec![announce_id.clone()],
    );

    let ctx = RuntimeContext::pure();

    // Execute fetch -> should complete (using governance-enforced interface)
    let r1 =
        futures::executor::block_on(async { governance_kernel.execute_plan_governed(&plan_fetch, &ctx).await })
            .unwrap();
    assert!(r1.success);

    // Now analyze is unblocked
    assert!(deps_completed(&intent_graph, &analyze_id));
    let r2 =
        futures::executor::block_on(async { governance_kernel.execute_plan_governed(&plan_analyze, &ctx).await })
            .unwrap();
    assert!(r2.success);

    // Now announce is unblocked
    assert!(deps_completed(&intent_graph, &announce_id));
    let r3 = futures::executor::block_on(async {
        governance_kernel.execute_plan_governed(&plan_announce, &ctx).await
    })
    .unwrap();
    assert!(r3.success);

    // If all subgoals completed, mark root Completed (manual policy for now)
    let children = intent_graph.lock().unwrap().get_child_intents(&root_id);
    assert_eq!(children.len(), 3);
    assert!(children.iter().all(|c| c.status == IntentStatus::Completed));
    {
        let mut g = intent_graph.lock().unwrap();
        g.set_intent_status_with_audit(&root_id, IntentStatus::Completed, Some("test"), None)
            .unwrap();
    }
    let r = intent_graph.lock().unwrap().get_intent(&root_id).unwrap();
    assert_eq!(r.status, IntentStatus::Completed);

    // Causal chain should contain status changes for each intent
    let chain = causal_chain.lock().unwrap();
    for id in [&root_id, &fetch_id, &analyze_id, &announce_id] {
        let actions = chain.get_actions_for_intent(id);
        assert!(
            actions
                .iter()
                .any(|a| a.action_type
                    == rtfs::ccos::types::ActionType::IntentStatusChanged),
            "Expected IntentStatusChanged in causal chain for {}",
            id
        );
    }
}
