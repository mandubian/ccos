//! Integration tests for Tier 2 memory provenance and secure sharing.

use autonoetic_types::memory::MemoryVisibility;
// use std::path::PathBuf;
use tempfile::tempdir;

/// Helper to create a test gateway directory with memory database
fn create_test_gateway() -> tempfile::TempDir {
    let dir = tempdir().unwrap();
    std::fs::create_dir_all(dir.path().join(".gateway")).unwrap();
    dir
}

/// Test that a writer agent can store a fact in Tier 2 memory and a reader agent
/// can recall it under allowed scope after sharing.
#[test]
fn test_tier2_memory_cross_agent_sharing() {
    let ws = create_test_gateway();
    let gateway_dir = ws.path().join(".gateway");

    // Writer agent stores a fact
    let mem_writer =
        autonoetic_gateway::runtime::memory::Tier2Memory::new(&gateway_dir, "writer-agent")
            .unwrap();

    let memory = mem_writer
        .remember(
            "fact_123",
            "general",
            "writer-agent",
            "session:test:turn:1",
            "Paris is the capital of France",
        )
        .unwrap();

    assert_eq!(memory.memory_id, "fact_123");
    assert_eq!(memory.content, "Paris is the capital of France");
    assert_eq!(memory.visibility, MemoryVisibility::Private);

    // Reader agent cannot access private memory
    let mem_reader =
        autonoetic_gateway::runtime::memory::Tier2Memory::new(&gateway_dir, "reader-agent")
            .unwrap();

    let err = mem_reader.recall("fact_123").unwrap_err();
    assert!(err.to_string().contains("not accessible"));

    // Writer shares with reader
    let shared = mem_writer
        .share_with("fact_123", vec!["reader-agent".to_string()])
        .unwrap();
    assert_eq!(shared.visibility, MemoryVisibility::Shared);
    assert!(shared.allowed_agents.contains(&"reader-agent".to_string()));

    // Reader can now access the shared memory
    let recalled = mem_reader.recall("fact_123").unwrap();
    assert_eq!(recalled.content, "Paris is the capital of France");
    assert_eq!(recalled.owner_agent_id, "writer-agent");
    assert_eq!(recalled.visibility, MemoryVisibility::Shared);
}

/// Test that unauthorized agents cannot read private memories.
#[test]
fn test_tier2_memory_unauthorized_access_denied() {
    let ws = create_test_gateway();
    let gateway_dir = ws.path().join(".gateway");

    // Agent A writes a private memory
    let mem_a =
        autonoetic_gateway::runtime::memory::Tier2Memory::new(&gateway_dir, "agent-a").unwrap();

    mem_a
        .remember(
            "private_fact",
            "secrets",
            "agent-a",
            "test:unauthorized",
            "This is agent A's secret",
        )
        .unwrap();

    // Agent A can read its own memory
    let recalled = mem_a.recall("private_fact").unwrap();
    assert_eq!(recalled.content, "This is agent A's secret");

    // Agent B cannot read agent A's private memory
    let mem_b =
        autonoetic_gateway::runtime::memory::Tier2Memory::new(&gateway_dir, "agent-b").unwrap();

    let err = mem_b.recall("private_fact").unwrap_err();
    assert!(err.to_string().contains("not accessible"));
    assert!(err.to_string().contains("agent-b"));
}

/// Test that all shared memories include proper provenance tracking.
#[test]
fn test_tier2_memory_provenance_tracking() {
    let ws = create_test_gateway();
    let gateway_dir = ws.path().join(".gateway");

    let mem =
        autonoetic_gateway::runtime::memory::Tier2Memory::new(&gateway_dir, "test-agent").unwrap();

    // Write a memory with specific source reference
    let memory = mem
        .remember(
            "provenance_test",
            "test",
            "test-agent",
            "session:abc123:turn:5",
            "Test content for provenance",
        )
        .unwrap();

    // Verify all provenance fields are set
    assert_eq!(memory.memory_id, "provenance_test");
    assert_eq!(memory.scope, "test");
    assert_eq!(memory.owner_agent_id, "test-agent");
    assert_eq!(memory.writer_agent_id, "test-agent");
    assert_eq!(memory.source_ref, "session:abc123:turn:5");
    assert!(!memory.created_at.is_empty());
    assert!(!memory.updated_at.is_empty());
    assert!(!memory.content_hash.is_empty());

    // Verify content hash is correct SHA-256
    use sha2::{Digest, Sha256};
    let mut hasher = Sha256::new();
    hasher.update("Test content for provenance".as_bytes());
    let expected_hash = hex::encode(hasher.finalize());
    assert_eq!(memory.content_hash, expected_hash);

    // Share with another agent and verify provenance is preserved
    let shared = mem
        .share_with("provenance_test", vec!["other-agent".to_string()])
        .unwrap();
    assert_eq!(shared.writer_agent_id, "test-agent"); // Original writer preserved
    assert_eq!(shared.source_ref, "session:abc123:turn:5"); // Source ref preserved
    assert_eq!(shared.content_hash, expected_hash); // Content hash unchanged
}

/// Test memory search functionality with visibility filtering.
#[test]
fn test_tier2_memory_search_with_visibility() {
    let ws = create_test_gateway();
    let gateway_dir = ws.path().join(".gateway");

    let mem = autonoetic_gateway::runtime::memory::Tier2Memory::new(&gateway_dir, "search-agent")
        .unwrap();

    // Write multiple memories in the same scope
    mem.remember(
        "fact_1",
        "weather",
        "search-agent",
        "test:1",
        "Paris is sunny",
    )
    .unwrap();

    mem.remember(
        "fact_2",
        "weather",
        "search-agent",
        "test:2",
        "London is rainy",
    )
    .unwrap();

    mem.remember(
        "fact_3",
        "geography",
        "search-agent",
        "test:3",
        "Paris is in France",
    )
    .unwrap();

    // Search by scope
    let results = mem.search("weather", None).unwrap();
    assert_eq!(results.len(), 2);

    // Search by scope and query
    let results = mem.search("weather", Some("Paris")).unwrap();
    assert_eq!(results.len(), 1);
    assert_eq!(results[0].memory_id, "fact_1");

    // Search non-existent scope
    let results = mem.search("nonexistent", None).unwrap();
    assert_eq!(results.len(), 0);
}

/// Test global memory visibility.
#[test]
fn test_tier2_memory_global_visibility() {
    let ws = create_test_gateway();
    let gateway_dir = ws.path().join(".gateway");

    let mem_owner =
        autonoetic_gateway::runtime::memory::Tier2Memory::new(&gateway_dir, "owner-agent").unwrap();

    let mem_reader1 =
        autonoetic_gateway::runtime::memory::Tier2Memory::new(&gateway_dir, "reader-agent-1")
            .unwrap();

    let mem_reader2 =
        autonoetic_gateway::runtime::memory::Tier2Memory::new(&gateway_dir, "reader-agent-2")
            .unwrap();

    // Owner writes a memory
    mem_owner
        .remember(
            "global_fact",
            "public",
            "owner-agent",
            "test:global",
            "This is public knowledge",
        )
        .unwrap();

    // Initially private - readers cannot access
    assert!(mem_reader1.recall("global_fact").is_err());
    assert!(mem_reader2.recall("global_fact").is_err());

    // Make global
    let global = mem_owner.make_global("global_fact").unwrap();
    assert_eq!(global.visibility, MemoryVisibility::Global);
    assert!(global.allowed_agents.is_empty());

    // Now all readers can access
    let r1 = mem_reader1.recall("global_fact").unwrap();
    assert_eq!(r1.content, "This is public knowledge");

    let r2 = mem_reader2.recall("global_fact").unwrap();
    assert_eq!(r2.content, "This is public knowledge");
}

/// Test that only owners can make memories global.
#[test]
fn test_tier2_memory_only_owner_can_make_global() {
    let ws = create_test_gateway();
    let gateway_dir = ws.path().join(".gateway");

    let mem_owner =
        autonoetic_gateway::runtime::memory::Tier2Memory::new(&gateway_dir, "owner-agent").unwrap();

    let mem_non_owner =
        autonoetic_gateway::runtime::memory::Tier2Memory::new(&gateway_dir, "non-owner-agent")
            .unwrap();

    // Owner writes a memory
    mem_owner
        .remember(
            "owned_fact",
            "test",
            "owner-agent",
            "test:owned",
            "Owned fact",
        )
        .unwrap();

    // Share with non-owner so they can access it (but still can't make global)
    mem_owner
        .share_with("owned_fact", vec!["non-owner-agent".to_string()])
        .unwrap();

    // Non-owner can read it but cannot make it global
    let err = mem_non_owner.make_global("owned_fact").unwrap_err();
    assert!(err
        .to_string()
        .contains("Only the owner can make a memory global"));
}

/// Test memory listing by scope.
#[test]
fn test_tier2_memory_list_scopes() {
    let ws = create_test_gateway();
    let gateway_dir = ws.path().join(".gateway");

    let mem =
        autonoetic_gateway::runtime::memory::Tier2Memory::new(&gateway_dir, "scope-agent").unwrap();

    // Initially no scopes
    let scopes = mem.list_scopes().unwrap();
    assert!(scopes.is_empty());

    // Write memories in different scopes
    mem.remember("f1", "scope_a", "scope-agent", "t:1", "content1")
        .unwrap();
    mem.remember("f2", "scope_b", "scope-agent", "t:2", "content2")
        .unwrap();
    mem.remember("f3", "scope_a", "scope-agent", "t:3", "content3")
        .unwrap();

    // List scopes
    let scopes = mem.list_scopes().unwrap();
    assert_eq!(scopes.len(), 2);
    assert!(scopes.contains(&"scope_a".to_string()));
    assert!(scopes.contains(&"scope_b".to_string()));
}
