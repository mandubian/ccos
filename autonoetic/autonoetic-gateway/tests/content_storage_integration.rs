//! Integration tests for content-addressable storage and knowledge tools.

use autonoetic_gateway::execution::extract_artifacts_from_content_store;
use autonoetic_gateway::runtime::content_store::ContentStore;
use tempfile::tempdir;

/// Helper to create a test gateway directory
fn create_test_gateway() -> (tempfile::TempDir, std::path::PathBuf) {
    let dir = tempdir().unwrap();
    let gateway_dir = dir.path().join(".gateway");
    std::fs::create_dir_all(&gateway_dir).unwrap();
    (dir, gateway_dir)
}

// ---------------------------------------------------------------------------
// Content Store Tests
// ---------------------------------------------------------------------------

#[test]
fn test_content_store_write_and_read_by_handle() {
    let (_dir, gateway_dir) = create_test_gateway();
    let store = ContentStore::new(&gateway_dir).unwrap();

    let content = b"Hello, World!";
    let handle = store.write(content).unwrap();

    assert!(handle.starts_with("sha256:"));
    assert_eq!(store.read(&handle).unwrap(), content);
}

#[test]
fn test_content_store_write_and_read_by_name() {
    let (_dir, gateway_dir) = create_test_gateway();
    let store = ContentStore::new(&gateway_dir).unwrap();

    let content = b"def main():\n    print('hello')\n";
    let handle = store.write(content).unwrap();

    store
        .register_name("session-1", "main.py", &handle)
        .unwrap();

    // Read by name
    let result = store.read_by_name("session-1", "main.py").unwrap();
    assert_eq!(result, content);
}

#[test]
fn test_content_store_read_by_name_or_handle() {
    let (_dir, gateway_dir) = create_test_gateway();
    let store = ContentStore::new(&gateway_dir).unwrap();

    let content = b"test content";
    let handle = store.write(content).unwrap();
    store
        .register_name("session-1", "test.txt", &handle)
        .unwrap();

    // Read by name
    let by_name = store
        .read_by_name_or_handle("session-1", "test.txt")
        .unwrap();
    assert_eq!(by_name, content);

    // Read by handle
    let by_handle = store.read_by_name_or_handle("session-1", &handle).unwrap();
    assert_eq!(by_handle, content);
}

#[test]
fn test_content_store_session_visibility_across_sessions() {
    let (_dir, gateway_dir) = create_test_gateway();
    let store = ContentStore::new(&gateway_dir).unwrap();

    let root_session = "demo-session";
    let child_session = "demo-session/coder-abc";

    store.set_root_session(child_session, root_session).unwrap();

    let content = b"session visible data";
    let handle = store.write(content).unwrap();

    // Write with session visibility
    store
        .register_name_with_visibility(
            child_session,
            "shared.txt",
            &handle,
            autonoetic_gateway::runtime::content_store::ContentVisibility::Session,
        )
        .unwrap();

    // Root session can read child's session-visible content
    let root_read = store
        .read_by_name_or_handle(root_session, "shared.txt")
        .unwrap();
    assert_eq!(root_read, content);
}

#[test]
fn test_content_store_handle_visibility_enforced() {
    let (_dir, gateway_dir) = create_test_gateway();
    let store = ContentStore::new(&gateway_dir).unwrap();

    let root = "root-session";
    let child_a = "root-session/agent-a";
    let child_b = "root-session/agent-b";

    store.set_root_session(child_a, root).unwrap();
    store.set_root_session(child_b, root).unwrap();

    // Agent A writes session-visible content
    let content = b"shared across sessions";
    let handle = store.write(content).unwrap();
    store
        .register_name_with_visibility(
            child_a,
            "shared.txt",
            &handle,
            autonoetic_gateway::runtime::content_store::ContentVisibility::Session,
        )
        .unwrap();

    // Agent B CAN read by handle because it's session-visible via root
    let result = store.read_by_name_or_handle(child_b, &handle).unwrap();
    assert_eq!(result, content);

    // Agent A writes PRIVATE content
    let private_content = b"private data";
    let private_handle = store.write(private_content).unwrap();
    store
        .register_name_with_visibility(
            child_a,
            "private.txt",
            &private_handle,
            autonoetic_gateway::runtime::content_store::ContentVisibility::Private,
        )
        .unwrap();

    // Agent B CANNOT read private handle
    let result = store.read_by_name_or_handle(child_b, &private_handle);
    assert!(
        result.is_err(),
        "private handle should not be visible cross-session"
    );

    // Agent A CAN read its own private handle
    let result = store
        .read_by_name_or_handle(child_a, &private_handle)
        .unwrap();
    assert_eq!(result, private_content);
}

#[test]
fn test_content_store_handle_not_visible_across_roots() {
    let (_dir, gateway_dir) = create_test_gateway();
    let store = ContentStore::new(&gateway_dir).unwrap();

    // Two sessions with DIFFERENT roots
    let handle = store.write(b"data").unwrap();
    store
        .register_name("root-a/agent-1", "file.txt", &handle)
        .unwrap();

    // Session under different root cannot read by handle
    let result = store.read_by_name_or_handle("root-b/agent-2", &handle);
    assert!(
        result.is_err(),
        "handle should not be visible across different root sessions"
    );
}

#[test]
fn test_content_store_deduplication() {
    let (_dir, gateway_dir) = create_test_gateway();
    let store = ContentStore::new(&gateway_dir).unwrap();

    let content = b"duplicate content";
    let handle1 = store.write(content).unwrap();
    let handle2 = store.write(content).unwrap();

    // Same content produces same handle
    assert_eq!(handle1, handle2);

    // Only one blob stored
    let stats = store.stats().unwrap();
    assert_eq!(stats.entry_count, 1);
}

// ---------------------------------------------------------------------------
// Knowledge Store Tests (via Tier2Memory)
// ---------------------------------------------------------------------------

#[test]
fn test_knowledge_store_and_recall() {
    let (_dir, gateway_dir) = create_test_gateway();

    let mem =
        autonoetic_gateway::runtime::memory::Tier2Memory::new(&gateway_dir, "test-agent").unwrap();

    let memory = mem
        .remember(
            "fact_001",
            "geography",
            "test-agent",
            "session:test:turn:1",
            "Tokyo is the capital of Japan",
        )
        .unwrap();

    assert_eq!(memory.memory_id, "fact_001");
    assert_eq!(memory.content, "Tokyo is the capital of Japan");
    assert_eq!(memory.scope, "geography");

    // Recall the fact
    let recalled = mem.recall("fact_001").unwrap();
    assert_eq!(recalled.content, "Tokyo is the capital of Japan");
}

#[test]
fn test_knowledge_search_by_scope() {
    let (_dir, gateway_dir) = create_test_gateway();

    let mem =
        autonoetic_gateway::runtime::memory::Tier2Memory::new(&gateway_dir, "test-agent").unwrap();

    // Store multiple facts in same scope
    mem.remember("fact_a", "cities", "test-agent", "s1", "Paris is in France")
        .unwrap();
    mem.remember("fact_b", "cities", "test-agent", "s1", "Tokyo is in Japan")
        .unwrap();
    mem.remember(
        "fact_c",
        "countries",
        "test-agent",
        "s1",
        "France is in Europe",
    )
    .unwrap();

    // Search cities scope
    let results = mem.search("cities", None).unwrap();
    assert_eq!(results.len(), 2);

    // Search with query
    let results = mem.search("cities", Some("Paris")).unwrap();
    assert_eq!(results.len(), 1);
    assert_eq!(results[0].content, "Paris is in France");
}

#[test]
fn test_knowledge_share_between_agents() {
    let (_dir, gateway_dir) = create_test_gateway();

    // Agent A stores a fact
    let mem_a =
        autonoetic_gateway::runtime::memory::Tier2Memory::new(&gateway_dir, "agent-a").unwrap();

    mem_a
        .remember(
            "shared_fact",
            "team",
            "agent-a",
            "s1",
            "Project deadline is March 20",
        )
        .unwrap();

    // Agent B cannot access it yet
    let mem_b =
        autonoetic_gateway::runtime::memory::Tier2Memory::new(&gateway_dir, "agent-b").unwrap();
    assert!(mem_b.recall("shared_fact").is_err());

    // Agent A shares with Agent B
    let shared = mem_a
        .share_with("shared_fact", vec!["agent-b".to_string()])
        .unwrap();
    assert!(shared.allowed_agents.contains(&"agent-b".to_string()));

    // Agent B can now access it
    let recalled = mem_b.recall("shared_fact").unwrap();
    assert_eq!(recalled.content, "Project deadline is March 20");
}

// ---------------------------------------------------------------------------
// Artifact Extraction Tests
// ---------------------------------------------------------------------------

#[test]
fn test_artifact_extraction_from_skill_md() {
    let (_dir, gateway_dir) = create_test_gateway();
    let session_id = "test-session";
    let store = ContentStore::new(&gateway_dir).unwrap();

    // Write SKILL.md with frontmatter
    let skill_content = r#"---
name: "my_agent"
description: "A test agent"
script_entry: "main.py"
io:
  accepts:
    type: object
  returns:
    type: string
---
# My Agent

This is a test agent.
"#;
    let skill_handle = store.write(skill_content.as_bytes()).unwrap();
    store
        .register_name(session_id, "my_agent/SKILL.md", &skill_handle)
        .unwrap();

    // Write a script file
    let main_py = b"print('hello')";
    let main_handle = store.write(main_py).unwrap();
    store
        .register_name(session_id, "my_agent/main.py", &main_handle)
        .unwrap();

    // Extract artifacts
    let artifacts = extract_artifacts_from_content_store(&gateway_dir, session_id).unwrap();

    assert_eq!(artifacts.len(), 1);
    let artifact = &artifacts[0];
    assert_eq!(artifact.name, "my_agent");
    assert_eq!(artifact.description, "A test agent");
    assert_eq!(artifact.entry_point, Some("main.py".to_string()));
    assert!(artifact.files.contains(&"my_agent/SKILL.md".to_string()));
    assert!(artifact.files.contains(&"my_agent/main.py".to_string()));
}

#[test]
fn test_artifact_extraction_handles_missing_frontmatter() {
    let (_dir, gateway_dir) = create_test_gateway();
    let session_id = "test-session";
    let store = ContentStore::new(&gateway_dir).unwrap();

    // Write SKILL.md without proper frontmatter
    let skill_content = "# Just a markdown file\nNo frontmatter here.";
    let skill_handle = store.write(skill_content.as_bytes()).unwrap();
    store
        .register_name(session_id, "agent/SKILL.md", &skill_handle)
        .unwrap();

    // Extract artifacts - should still work with defaults
    let artifacts = extract_artifacts_from_content_store(&gateway_dir, session_id).unwrap();

    assert_eq!(artifacts.len(), 1);
    let artifact = &artifacts[0];
    assert_eq!(artifact.name, "agent"); // Derived from directory name
    assert!(artifact.files.contains(&"agent/SKILL.md".to_string()));
}

// ---------------------------------------------------------------------------
// Content Store Stats
// ---------------------------------------------------------------------------

#[test]
fn test_content_store_statistics() {
    let (_dir, gateway_dir) = create_test_gateway();
    let store = ContentStore::new(&gateway_dir).unwrap();

    store.write(b"content1").unwrap();
    store.write(b"content2").unwrap();
    store.write(b"content1").unwrap(); // duplicate

    let stats = store.stats().unwrap();
    assert_eq!(stats.entry_count, 2); // deduplicated
    assert!(stats.total_size_bytes > 0);
}

// ---------------------------------------------------------------------------
// Shared Knowledge in Execution
// ---------------------------------------------------------------------------

#[test]
fn test_collect_shared_knowledge_finds_shared_records() {
    let (_dir, gateway_dir) = create_test_gateway();

    // Writer agent stores and shares a fact
    let writer_mem =
        autonoetic_gateway::runtime::memory::Tier2Memory::new(&gateway_dir, "writer-agent")
            .unwrap();

    writer_mem
        .remember(
            "shared_fact",
            "team-knowledge",
            "writer-agent",
            "session:test",
            "Deployment requires approval",
        )
        .unwrap();

    // Share with reader agent
    writer_mem
        .share_with("shared_fact", vec!["reader-agent".to_string()])
        .unwrap();

    // Collect shared knowledge for the reader
    let shared = autonoetic_gateway::execution::collect_shared_knowledge(
        &gateway_dir,
        "reader-agent",
        "writer-agent",
    );

    assert_eq!(shared.len(), 1);
    assert_eq!(shared[0].id, "shared_fact");
    assert_eq!(shared[0].scope, "team-knowledge");
    assert!(shared[0].content_preview.contains("Deployment"));
}

#[test]
fn test_collect_shared_knowledge_excludes_private() {
    let (_dir, gateway_dir) = create_test_gateway();

    // Writer agent stores a private fact
    let writer_mem =
        autonoetic_gateway::runtime::memory::Tier2Memory::new(&gateway_dir, "writer-agent")
            .unwrap();

    writer_mem
        .remember(
            "private_fact",
            "secrets",
            "writer-agent",
            "session:test",
            "This is private",
        )
        .unwrap();

    // Collect knowledge for reader - should not include private facts
    let shared = autonoetic_gateway::execution::collect_shared_knowledge(
        &gateway_dir,
        "reader-agent",
        "writer-agent",
    );

    assert_eq!(shared.len(), 0);
}

#[test]
fn test_collect_shared_knowledge_includes_global() {
    let (_dir, gateway_dir) = create_test_gateway();

    // Writer agent stores a global fact
    let writer_mem =
        autonoetic_gateway::runtime::memory::Tier2Memory::new(&gateway_dir, "writer-agent")
            .unwrap();

    let memory = writer_mem
        .remember(
            "global_fact",
            "public",
            "writer-agent",
            "session:test",
            "Public knowledge for all",
        )
        .unwrap();

    // Make it global
    writer_mem.make_global(&memory.memory_id).unwrap();

    // Collect knowledge for any reader - should include global facts
    let shared = autonoetic_gateway::execution::collect_shared_knowledge(
        &gateway_dir,
        "any-reader-agent",
        "writer-agent",
    );

    assert_eq!(shared.len(), 1);
    assert_eq!(shared[0].id, "global_fact");
}
