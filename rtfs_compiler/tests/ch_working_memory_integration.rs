use rtfs_compiler::ccos::context_horizon::ContextHorizonManager;
use rtfs_compiler::ccos::working_memory::boundaries::{Boundary, BoundaryType};
use rtfs_compiler::ccos::working_memory::ingestor::{ActionRecord, MemoryIngestor};

#[test]
fn test_ch_fetch_wisdom_time_window_integration() {
    let manager = ContextHorizonManager::new().unwrap();

    // Build sample actions with controlled timestamps
    let now = chrono::Utc::now().timestamp() as u64;

    let mk = |id: &str, ts: u64, kind: &str, content: &str| ActionRecord {
        action_id: id.to_string(),
        kind: kind.to_string(),
        provider: Some("demo.provider:v1".to_string()),
        timestamp_s: ts,
        summary: format!("summary-{}", id),
        content: content.to_string(),
        plan_id: Some("plan-1".to_string()),
        intent_id: Some("intent-1".to_string()),
        step_id: None,
        attestation_hash: None,
        content_hash: None,
    };

    let a = mk("a", now - 30, "PlanStarted", "c1");
    let b = mk("b", now - 20, "PlanStarted", "c2");
    let c = mk("c", now - 10, "PlanCompleted", "c3");

    // Ingest into the manager's Working Memory
    {
        let wm_arc = manager.working_memory();
        let mut wm = wm_arc.lock().expect("WorkingMemory lock");
        MemoryIngestor::ingest_action(&mut wm, &a).unwrap();
        MemoryIngestor::ingest_action(&mut wm, &b).unwrap();
        MemoryIngestor::ingest_action(&mut wm, &c).unwrap();
    } // drop WM lock before calling CH methods that also lock WM

    // Define boundaries: window should select b and c (exclude a)
    let from_ts = now.saturating_sub(25);
    let to_ts = now.saturating_sub(5);
    let boundaries = vec![
        Boundary::new("time", BoundaryType::TimeLimit)
            .with_constraint("from_ts", serde_json::json!(from_ts))
            .with_constraint("to_ts", serde_json::json!(to_ts)),
        Boundary::new("limit", BoundaryType::TokenLimit)
            .with_constraint("max_tokens", serde_json::json!(10usize)),
    ];

    // Fetch entries via CH helper
    let entries = manager
        .fetch_wisdom_from_working_memory(&boundaries)
        .expect("fetch_wisdom_from_working_memory");

    assert!(!entries.is_empty(), "Expected at least one wisdom entry");

    // Ensure all are tagged 'wisdom' and within the requested time window
    for e in &entries {
        assert!(
            e.tags.contains("wisdom"),
            "Entry missing 'wisdom' tag: {:?}",
            e.tags
        );
        assert!(
            e.timestamp_s >= from_ts && e.timestamp_s <= to_ts,
            "Entry outside window: {} not in [{}, {}]",
            e.timestamp_s,
            from_ts,
            to_ts
        );
    }

    // Expect action_ids "b" and "c" only, newest first ("c" first)
    let ids: Vec<_> = entries.iter().map(|e| e.meta.action_id.clone()).collect();
    assert!(
        ids.contains(&Some("b".to_string())),
        "Expected action_id b in results, got {:?}",
        ids
    );
    assert!(
        ids.contains(&Some("c".to_string())),
        "Expected action_id c in results, got {:?}",
        ids
    );
    assert!(
        !ids.contains(&Some("a".to_string())),
        "Did not expect action_id a in results, got {:?}",
        ids
    );

    // Verify ordering by recency desc (c newer than b)
    assert_eq!(
        entries.first().unwrap().meta.action_id.as_deref(),
        Some("c"),
        "Expected newest (c) first, got {:?}",
        entries.first().unwrap().meta.action_id.as_deref()
    );
}
