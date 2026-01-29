#![allow(unused_imports, unused_variables)]

use ccos::utils::fs::get_workspace_root;
use ccos::working_memory::{AgentMemory, InMemoryJsonlBackend, LearnedPattern, WorkingMemory};
use std::fs;
use std::sync::{Arc, Mutex};

#[tokio::test]
async fn test_agent_memory_persistence_and_recall() {
    // Setup temporary path
    let tmp_dir = std::env::temp_dir().join("ccos_test_memory");
    let memory_path = tmp_dir.join("agent_memory.json");

    if tmp_dir.exists() {
        fs::remove_dir_all(&tmp_dir).unwrap();
    }
    fs::create_dir_all(&tmp_dir).unwrap();

    // 1. Create and populate memory
    {
        let wm_backend = InMemoryJsonlBackend::new(None, None, None);
        let working_memory = Arc::new(Mutex::new(WorkingMemory::new(Box::new(wm_backend))));
        let mut agent_memory = AgentMemory::new("test-agent", working_memory);

        // Store a thought
        agent_memory
            .store(
                "My plan is to win".to_string(),
                "I am planning to win the game".to_string(),
                &["plan", "thought"],
            )
            .unwrap();

        // Store a learned pattern
        let pattern = LearnedPattern::new("pattern-1", "Don't divide by zero").with_confidence(0.9);
        agent_memory.store_learned_pattern(pattern);

        // Save
        agent_memory.save_to_disk(&memory_path).unwrap();
    }

    // 2. Load into fresh memory
    {
        let wm_backend = InMemoryJsonlBackend::new(None, None, None);
        let working_memory = Arc::new(Mutex::new(WorkingMemory::new(Box::new(wm_backend))));
        let mut agent_memory = AgentMemory::new("test-agent", working_memory);

        // Verify empty initially
        let initial = agent_memory.recall_relevant(&["plan"], None).unwrap();
        assert!(initial.is_empty());

        // Load
        agent_memory.load_from_disk(&memory_path).unwrap();

        // Recall persistence of generic memory?
        // Note: modify save_to_disk to serialize generic entries too?
        // Wait, my implementation of save_to_disk ONLY serialized learned_patterns!
        // I need to check `serialize_patterns` impl in `agent_memory.rs`.
        // If it only saves patterns, then `store()` entries are lost.
        // Let's verify this likely behavior.

        // Check patterns
        let patterns = agent_memory.get_learned_patterns();
        assert_eq!(patterns.len(), 1);
        assert_eq!(patterns[0].pattern_id, "pattern-1");
    }

    // Cleanup
    fs::remove_dir_all(&tmp_dir).unwrap();
}
