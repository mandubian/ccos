//! Simulated HTTP endpoint test: directly exercises snapshot builder with capability inclusion
//! avoiding Axum runtime Send/Sync constraints of non-Send CCOS internals.

use std::sync::Arc;
use rtfs_compiler::ccos::{CCOS, runtime_service};
use viewer_server::build_architecture_snapshot;

#[tokio::test(flavor = "current_thread")]
async fn architecture_endpoint_simulated_includes_meta_and_capabilities() {
    let storage_path = std::path::PathBuf::from("demo_storage_test_endpoint_sim");
    let intent_graph_config = rtfs_compiler::ccos::intent_graph::config::IntentGraphConfig::with_file_archive_storage(storage_path.clone());
    let plan_archive_path = storage_path.join("plans");
    std::fs::create_dir_all(&plan_archive_path).ok();
    let ccos = Arc::new(CCOS::new_with_configs_and_debug_callback(intent_graph_config, Some(plan_archive_path), None)
        .await
        .expect("ccos init"));

    // Start runtime service (spawn_local requires LocalSet but test flavor current_thread provides a per-thread reactor)
    let local = tokio::task::LocalSet::new();
    let snapshot = local.run_until(async {
        let _handle = runtime_service::start_service(Arc::clone(&ccos)).await;
        build_architecture_snapshot(&ccos, true, 5, Some(25)).await
    }).await;

    assert_eq!(snapshot.get("version").and_then(|v| v.as_str()), Some("1"));
    assert!(snapshot.get("graph_model").is_some(), "graph_model present");
    // capabilities included
    assert!(snapshot.get("capabilities").is_some(), "capabilities list present when include=true");
    // meta optional; if present warnings must be array
    if let Some(meta) = snapshot.get("meta") {
        if let Some(w) = meta.get("warnings") { assert!(w.is_array()); }
    }
}
