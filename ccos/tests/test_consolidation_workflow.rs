use ccos::utils::fs::get_workspace_root;
use ccos::working_memory::{AgentMemory, InMemoryJsonlBackend, WorkingMemory};
use std::fs;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

// Mock structures to simulate MCP tools behavior or use the actual tools if possible.
// Since we can't easily spin up the full MCP server harness here without a lot of boilerplate,
// we will verify by inspecting the file system side-effects which is what the user cares about.

#[tokio::test]
async fn test_simulated_agent_workflow() {
    // 1. Setup paths
    let root = get_workspace_root();
    let memory_path = root.join(".ccos/agent_memory/system_memory.json");
    let agents_dir = root.join(".ccos/capabilities/agents"); // Adjust based on config, let's assume default for now or check config
                                                             // Actually, `get_configured_capabilities_path` usually defaults to workspace root/rtfs or similar.
                                                             // Let's rely on where the code claimed to write: `crate::utils::fs::get_configured_capabilities_path().join("agents")`

    // Clean up previous runs
    if memory_path.exists() {
        let _ = fs::remove_file(&memory_path);
    }
    // We won't delete the whole agents dir to avoid messing up user's workspace,
    // but we'll use a unique agent name.
    let agent_name = "test_consolidation_bot";

    // 2. Initialize Agent Memory (simulating server startup)
    let wm_backend = InMemoryJsonlBackend::new(None, None, None);
    let working_memory = Arc::new(Mutex::new(WorkingMemory::new(Box::new(wm_backend))));
    let mut agent_memory = AgentMemory::new("system-mcp", working_memory);

    // 3. Simulate "recording learning" (What the agent should do)
    // "I have consolidated a new agent called test_consolidation_bot"
    let pattern_desc = format!(
        "To test consolidation, use agent.{} [Context: testing workflow] [Outcome: success]",
        agent_name
    );
    let p = ccos::working_memory::LearnedPattern::new("pattern-consolidation", &pattern_desc)
        .with_confidence(1.0);

    agent_memory.store_learned_pattern(p);
    agent_memory.save_to_disk(&memory_path).unwrap();

    // 4. Verify file exists and contains data
    assert!(memory_path.exists(), "Memory file should exist");
    let content = fs::read_to_string(&memory_path).unwrap();
    assert!(
        content.contains("pattern-consolidation"),
        "Memory should contain the pattern"
    );
    assert!(
        content.contains("test_consolidation_bot"),
        "Memory should contain the agent name"
    );

    // 5. Simulate "recall" (What happens next time)
    // Create new memory instance (simulating restart)
    let wm_backend2 = InMemoryJsonlBackend::new(None, None, None);
    let working_memory2 = Arc::new(Mutex::new(WorkingMemory::new(Box::new(wm_backend2))));
    let mut agent_memory2 = AgentMemory::new("system-mcp", working_memory2);

    agent_memory2.load_from_disk(&memory_path).unwrap();

    let patterns = agent_memory2.get_learned_patterns();
    assert_eq!(patterns.len(), 1);
    assert_eq!(patterns[0].description, pattern_desc);

    // Also verify indexing in WM
    let recalled = agent_memory2.recall_relevant(&["learning"], None).unwrap();
    assert!(
        !recalled.is_empty(),
        "Should recall indexed learned pattern via tags"
    );
    assert!(recalled[0].content.contains(&pattern_desc));
}
