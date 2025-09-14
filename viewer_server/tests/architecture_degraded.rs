//! Test forcing a degraded snapshot path by poisoning the intent graph lock.
//! We spawn a task that panics while holding the intent graph mutex to mark it poisoned.

use std::sync::Arc;
use rtfs_compiler::ccos::CCOS;
use viewer_server::build_architecture_snapshot;

#[tokio::test(flavor = "current_thread")]
async fn architecture_snapshot_degraded_flag_on_lock_failure() {
    // Use a LocalSet because we need spawn_local to intentionally poison the mutex.
    let local = tokio::task::LocalSet::new();
    local.run_until(async {
    // Prepare CCOS with normal config
    let storage_path = std::path::PathBuf::from("demo_storage_test_degraded");
    let intent_graph_config = rtfs_compiler::ccos::intent_graph::config::IntentGraphConfig::with_file_archive_storage(storage_path.clone());
    let plan_archive_path = storage_path.join("plans");
    std::fs::create_dir_all(&plan_archive_path).ok();
    let ccos = Arc::new(CCOS::new_with_configs_and_debug_callback(intent_graph_config, Some(plan_archive_path), None)
        .await
        .expect("ccos init"));

        // Poison the intent graph mutex intentionally.
        {
            let ig_arc = ccos.get_intent_graph();
            let ig_clone = ig_arc.clone();
            // Spawn a local task that locks and panics; this will poison the mutex.
            let handle = tokio::task::spawn_local(async move {
                // Lock then panic
                let _lock = ig_clone.lock().expect("lock");
                panic!("intentional mutex poison for degraded-mode test");
            });
            // Run the local task and ignore its panic (we expect it).
            let _ = handle.await; // join error due to panic
        }

        // Now attempt to build snapshot; lock should fail -> degraded=true, warning present
        let snapshot = build_architecture_snapshot(&ccos, false, 3, None).await;
        let degraded = snapshot
            .get("meta")
            .and_then(|m| m.get("degraded"))
            .and_then(|v| v.as_bool())
            .unwrap_or(false);
        assert!(degraded, "Expected degraded flag to be true after lock poisoning: snapshot={:?}", snapshot);
        let warnings_len = snapshot
            .get("meta")
            .and_then(|m| m.get("warnings"))
            .and_then(|v| v.as_array())
            .map(|a| a.len())
            .unwrap_or(0);
        assert!(warnings_len > 0, "Expected at least one warning recorded in degraded mode");
    }).await;
}
