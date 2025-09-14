//! Integration-style test for architecture snapshot invariants.
//! Focus: schema basics, recent intents limit, absence of raw secrets.

use std::sync::Arc;
use rtfs_compiler::ccos::{CCOS, runtime_service};
use viewer_server::build_architecture_snapshot; // uses pub(crate) visibility within crate

#[tokio::test(flavor = "current_thread")]
async fn architecture_snapshot_basic_invariants() {
    // Init CCOS in-memory (reuse demo storage path to avoid altering prod state)
    let storage_path = std::path::PathBuf::from("demo_storage_test");
    let intent_graph_config = rtfs_compiler::ccos::intent_graph::config::IntentGraphConfig::with_file_archive_storage(storage_path.clone());
    let plan_archive_path = storage_path.join("plans");
    std::fs::create_dir_all(&plan_archive_path).ok();

    let ccos = Arc::new(CCOS::new_with_configs_and_debug_callback(intent_graph_config, Some(plan_archive_path), None).await.expect("ccos init"));
    // Start runtime service (needed for marketplace etc.)
    // runtime_service uses spawn_local; we need a LocalSet.
    let local = tokio::task::LocalSet::new();
    let snapshot = local.run_until(async {
        let _handle = runtime_service::start_service(Arc::clone(&ccos)).await;
        build_architecture_snapshot(&ccos, false, 3, None).await
    }).await;

    // version present
    assert_eq!(snapshot.get("version").and_then(|v| v.as_str()), Some("1"));
    // graph model nodes non-empty
    let node_count = snapshot
        .get("graph_model").and_then(|v| v.get("nodes")).and_then(|v| v.as_array())
        .map(|a| a.len()).unwrap_or(0);
    assert!(node_count > 0, "graph_model.nodes should be non-empty");
    // recent intents obey limit (<=3)
    let recent_len = snapshot
        .get("components").and_then(|v| v.get("intent_graph"))
        .and_then(|v| v.get("recent")).and_then(|v| v.as_array())
        .map(|a| a.len()).unwrap_or(0);
    assert!(recent_len <= 3, "recent intents should be capped");
    // no raw secret values (flags should be SET/NOT_SET only)
    if let Some(flags) = snapshot.get("environment").and_then(|e| e.get("flags")).and_then(|f| f.as_object()) {
        for (_k, v) in flags { if let Some(s) = v.as_str() { assert!(s=="SET" || s=="NOT_SET"); } }
    }
}
