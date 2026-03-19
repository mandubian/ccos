//! Integration tests for session snapshot and fork functionality.

use autonoetic_gateway::runtime::content_store::ContentStore;
use autonoetic_gateway::runtime::session_snapshot::{SessionFork, SessionSnapshot};
use tempfile::tempdir;

/// Helper to create a test gateway directory
fn create_test_gateway() -> (tempfile::TempDir, std::path::PathBuf) {
    let dir = tempdir().unwrap();
    let gateway_dir = dir.path().join(".gateway");
    std::fs::create_dir_all(&gateway_dir).unwrap();
    (dir, gateway_dir)
}

/// Test that conversation history is persisted via content.write on hibernate.
#[test]
fn test_session_snapshot_capture_and_load() {
    let (_dir, gateway_dir) = create_test_gateway();
    let session_id = "test-session-1";

    let history = vec![
        autonoetic_gateway::llm::Message::user("Hello"),
        autonoetic_gateway::llm::Message::assistant("Hi there!"),
        autonoetic_gateway::llm::Message::user("What is the weather?"),
        autonoetic_gateway::llm::Message::assistant("I'll check that for you."),
    ];

    // Capture snapshot
    let snapshot =
        SessionSnapshot::capture(session_id, &history, 2, None, None, &gateway_dir).unwrap();

    assert_eq!(snapshot.source_session_id, session_id);
    assert_eq!(snapshot.turn_count, 2);
    assert_eq!(snapshot.history.len(), 4);
    assert!(snapshot.content_handle.is_some());

    // Verify content handle is valid
    let handle = snapshot.content_handle.as_ref().unwrap();
    assert!(handle.starts_with("sha256:"));

    // Load from session
    let loaded = SessionSnapshot::load_from_session(session_id, &gateway_dir).unwrap();
    assert_eq!(loaded.history.len(), 4);
    assert_eq!(loaded.turn_count, 2);
}

/// Test that snapshot content is persisted and survives in content store.
#[test]
fn test_snapshot_content_handle_permanence() {
    let (_dir, gateway_dir) = create_test_gateway();
    let session_id = "test-session-2";

    let history = vec![
        autonoetic_gateway::llm::Message::user("Test query"),
        autonoetic_gateway::llm::Message::assistant("Test response"),
    ];

    // Capture snapshot
    let snapshot =
        SessionSnapshot::capture(session_id, &history, 1, None, None, &gateway_dir).unwrap();

    // Can read content by handle
    let handle = snapshot.content_handle.as_ref().unwrap();
    let store = ContentStore::new(&gateway_dir).unwrap();
    let content = store.read(handle).unwrap();
    let loaded_history: Vec<autonoetic_gateway::llm::Message> =
        serde_json::from_slice(&content).unwrap();
    assert_eq!(loaded_history.len(), 2);
}

/// Test session fork creates new session with copied history.
#[test]
fn test_session_fork_creates_new_session() {
    let (_dir, gateway_dir) = create_test_gateway();

    let history = vec![
        autonoetic_gateway::llm::Message::user("Original question"),
        autonoetic_gateway::llm::Message::assistant("Original answer"),
        autonoetic_gateway::llm::Message::user("Follow-up"),
        autonoetic_gateway::llm::Message::assistant("Follow-up answer"),
    ];

    // Create snapshot of original session
    let snapshot =
        SessionSnapshot::capture("original-session", &history, 2, None, None, &gateway_dir)
            .unwrap();

    // Fork the session
    let fork = SessionFork::fork(&snapshot, Some("forked-session"), None, &gateway_dir).unwrap();

    assert_eq!(fork.new_session_id, "forked-session");
    assert_eq!(fork.source_session_id, "original-session");
    assert_eq!(fork.fork_turn, 2);
    assert_eq!(fork.initial_history.len(), 4);

    // Verify history is stored in forked session
    let store = ContentStore::new(&gateway_dir).unwrap();
    let forked_history = store
        .read_by_name("forked-session", "session_history")
        .unwrap();
    let loaded: Vec<autonoetic_gateway::llm::Message> =
        serde_json::from_slice(&forked_history).unwrap();
    assert_eq!(loaded.len(), 4);
}

/// Test fork with branch message appends to history.
#[test]
fn test_session_fork_with_branch_message() {
    let (_dir, gateway_dir) = create_test_gateway();

    let history = vec![
        autonoetic_gateway::llm::Message::user("Question"),
        autonoetic_gateway::llm::Message::assistant("Answer"),
    ];

    let snapshot =
        SessionSnapshot::capture("session-a", &history, 1, None, None, &gateway_dir).unwrap();

    // Fork with branch message
    let fork = SessionFork::fork(
        &snapshot,
        Some("session-b"),
        Some("Try a different approach"),
        &gateway_dir,
    )
    .unwrap();

    // History should have original + branch message
    assert_eq!(fork.initial_history.len(), 3);
    assert_eq!(fork.initial_history[2].content, "Try a different approach");

    // Verify stored history includes branch message
    let store = ContentStore::new(&gateway_dir).unwrap();
    let stored = store.read_by_name("session-b", "session_history").unwrap();
    let loaded: Vec<autonoetic_gateway::llm::Message> = serde_json::from_slice(&stored).unwrap();
    assert_eq!(loaded.len(), 3);
    assert_eq!(loaded[2].content, "Try a different approach");
}

/// Test fork lineage is tracked.
#[test]
fn test_session_fork_lineage_tracking() {
    let (_dir, gateway_dir) = create_test_gateway();

    let history = vec![autonoetic_gateway::llm::Message::user("Start")];

    let snapshot =
        SessionSnapshot::capture("parent-session", &history, 1, None, None, &gateway_dir).unwrap();

    let fork = SessionFork::fork(
        &snapshot,
        Some("child-session"),
        Some("Branch point"),
        &gateway_dir,
    )
    .unwrap();

    // Verify lineage fields
    assert_eq!(fork.source_session_id, "parent-session");
    assert_eq!(fork.new_session_id, "child-session");
    assert_eq!(fork.fork_turn, 1);
    assert!(fork.history_handle.starts_with("sha256:"));
}

/// Test fork without branch message (clean copy).
#[test]
fn test_session_fork_without_branch_message() {
    let (_dir, gateway_dir) = create_test_gateway();

    let history = vec![
        autonoetic_gateway::llm::Message::user("Q1"),
        autonoetic_gateway::llm::Message::assistant("A1"),
    ];

    let snapshot =
        SessionSnapshot::capture("session-a", &history, 1, None, None, &gateway_dir).unwrap();

    // Fork without branch message
    let fork = SessionFork::fork(&snapshot, Some("session-b"), None, &gateway_dir).unwrap();

    // History should be identical to original
    assert_eq!(fork.initial_history.len(), 2);
    assert_eq!(fork.initial_history[0].content, "Q1");
    assert_eq!(fork.initial_history[1].content, "A1");
}

/// Test auto-generated session ID for fork.
#[test]
fn test_session_fork_auto_session_id() {
    let (_dir, gateway_dir) = create_test_gateway();

    let history = vec![autonoetic_gateway::llm::Message::user("Test")];

    let snapshot =
        SessionSnapshot::capture("original", &history, 1, None, None, &gateway_dir).unwrap();

    // Fork with auto-generated session ID
    let fork = SessionFork::fork(&snapshot, None, None, &gateway_dir).unwrap();

    assert!(fork.new_session_id.starts_with("fork-"));
    assert_ne!(fork.new_session_id, "original");
}

/// Test snapshot persists across "restarts" (new store instance).
#[test]
fn test_snapshot_survives_restart() {
    let (_dir, gateway_dir) = create_test_gateway();
    let session_id = "persistent-session";

    let history = vec![
        autonoetic_gateway::llm::Message::user("Remember this"),
        autonoetic_gateway::llm::Message::assistant("I'll remember"),
    ];

    // Create snapshot
    let snapshot =
        SessionSnapshot::capture(session_id, &history, 1, None, None, &gateway_dir).unwrap();

    let handle = snapshot.content_handle.clone().unwrap();

    // Simulate "restart" by creating new store instance
    let store = ContentStore::new(&gateway_dir).unwrap();

    // Content should still be readable
    let content = store.read(&handle).unwrap();
    let loaded: Vec<autonoetic_gateway::llm::Message> = serde_json::from_slice(&content).unwrap();
    assert_eq!(loaded.len(), 2);
}

/// Test multi-level fork (fork of a fork).
#[test]
fn test_multi_level_fork() {
    let (_dir, gateway_dir) = create_test_gateway();

    let history = vec![autonoetic_gateway::llm::Message::user("Original")];

    // Original session
    let snapshot_a =
        SessionSnapshot::capture("session-a", &history, 1, None, None, &gateway_dir).unwrap();

    // Fork to session B
    let fork_b = SessionFork::fork(
        &snapshot_a,
        Some("session-b"),
        Some("Branch 1"),
        &gateway_dir,
    )
    .unwrap();

    // Snapshot session B
    let snapshot_b = SessionSnapshot::capture(
        "session-b",
        &fork_b.initial_history,
        2,
        None,
        None,
        &gateway_dir,
    )
    .unwrap();

    // Fork to session C
    let fork_c = SessionFork::fork(
        &snapshot_b,
        Some("session-c"),
        Some("Branch 2"),
        &gateway_dir,
    )
    .unwrap();

    // Session C should have: Original + Branch 1 + Branch 2 = 3 messages
    assert_eq!(fork_c.initial_history.len(), 3);
    assert_eq!(fork_c.initial_history[0].content, "Original");
    assert_eq!(fork_c.initial_history[1].content, "Branch 1");
    assert_eq!(fork_c.initial_history[2].content, "Branch 2");
}
