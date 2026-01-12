use ccos::capabilities::registry::CapabilityRegistry;
use ccos::capability_marketplace::CapabilityMarketplace;
use ccos::catalog::CatalogService;
use ccos::mcp::session::{create_session_store, save_session, Session, SessionStore};
use ccos::planner::capabilities_v2::register_planner_capabilities_v2;
use ccos::utils::value_conversion::{json_to_rtfs_value, rtfs_value_to_json};
use ccos::working_memory::{AgentMemory, InMemoryJsonlBackend, LearnedPattern, WorkingMemory};
use ccos::CCOS;
use rtfs::ast::MapKey;
use rtfs::runtime::values::Value;
use serde_json::json;
use std::sync::Arc;
use tokio::sync::RwLock;

// --- Test Setup ---

async fn setup_environment() -> (
    Arc<CapabilityMarketplace>,
    Arc<CCOS>,
    SessionStore,
    Arc<RwLock<AgentMemory>>,
    tempfile::TempDir,
) {
    let temp_dir = tempfile::tempdir().expect("Failed to create temp dir");
    let root = temp_dir.path().to_path_buf();

    // Setup Registry and Marketplace
    let registry = Arc::new(RwLock::new(CapabilityRegistry::new()));
    let marketplace = Arc::new(CapabilityMarketplace::new(registry));
    let catalog = Arc::new(CatalogService::new());

    // Init CCOS (async)
    let ccos = Arc::new(CCOS::new().await.expect("Failed to init CCOS"));

    let session_store = create_session_store();

    // Agent Memory
    // Use the temp dir for persistence
    let memory_path = root.join("agent_memory/system_memory.json");
    if let Some(parent) = memory_path.parent() {
        std::fs::create_dir_all(parent).unwrap();
    }

    let wm_backend = InMemoryJsonlBackend::new(None, None, None);
    let working_memory = Arc::new(std::sync::Mutex::new(WorkingMemory::new(Box::new(
        wm_backend,
    ))));
    let agent_memory = Arc::new(RwLock::new(AgentMemory::new("test-agent", working_memory)));

    // Register Planner Capabilities
    register_planner_capabilities_v2(marketplace.clone(), catalog.clone(), ccos.clone())
        .await
        .expect("Failed to register planner capabilities");

    // Register simple echo capability
    let echo_handler = Arc::new(|input: &Value| {
        let msg = match input {
            Value::Map(m) => {
                // MapKey can be String or Keyword
                // json_to_rtfs_value typically uses String keys for object properties
                let key = MapKey::String("message".to_string());
                match m.get(&key) {
                    Some(val) => match val {
                        Value::String(s) => s.clone(),
                        _ => "default".to_string(),
                    },
                    None => "default".to_string(),
                }
            }
            _ => "default".to_string(),
        };
        Ok(Value::String(format!("Echo: {}", msg)))
    });

    marketplace
        .register_local_capability(
            "test.echo".to_string(),
            "Echo".to_string(),
            "Echoes input".to_string(),
            echo_handler,
        )
        .await
        .unwrap();

    (marketplace, ccos, session_store, agent_memory, temp_dir)
}

#[tokio::test]
async fn test_full_consolidation_cycle() {
    let (marketplace, _ccos, session_store, agent_memory, _temp_dir) = setup_environment().await;

    // --- 1. Start Session ---
    let goal = "Consolidate echo capability";
    let session_id = {
        let mut store = session_store.write().await;
        // Session::new takes string slice
        let session = Session::new(goal);
        let id = session.id.clone();
        store.insert(id.clone(), session);
        id
    };
    println!("Session ID: {}", session_id);

    // --- 2. Execute Echo (Generate Trace) ---
    {
        // Simulate what ccos_execute_capability does
        let cap_id = "test.echo";
        let inputs = json!({"message": "Hello World"});
        let rtfs_inputs = json_to_rtfs_value(&inputs).unwrap();

        // Execute via marketplace
        let result_value = marketplace
            .execute_capability(cap_id, &rtfs_inputs)
            .await
            .unwrap();
        let result_json = rtfs_value_to_json(&result_value).unwrap();

        // Record step in session
        let mut store = session_store.write().await;
        let session = store.get_mut(&session_id).unwrap();

        // add_step signature: &mut self, capability_id: &str, ...
        // So pass simple string slice
        session.add_step(cap_id, inputs, result_json, true);

        // Save session
        // save_session signature: &Session, Option<&Path>, Option<&str> -> Result<PathBuf>
        // We save to temp session dir if possible, or just default.
        // But planner reads from default.
        // If we save to default, we pollute user workspace.
        // However, we cannot mock `get_configured_sessions_path` easily.
        // We will save to default and print a warning.
        // A better way would be using `env::set_var` to override CCOS_HOME if CCOS supports it,
        // but that might affect other tests running in parallel if any.
        // `get_workspace_root` uses `env::var("CCOS_WORKSPACE_ROOT")`.
        // We could try setting it?

        // Let's just save to default and clean up if possible.
        // Since we know the session ID, we can delete the file after.
        let saved_path = save_session(session, None, None).expect("Failed to save session");
        println!("Saved session to: {:?}", saved_path);
    }

    // --- 3. Consolidate Session (Create Agent) ---
    // This calls `planner.synthesize_agent_from_trace`
    // We call it directly via marketplace to simulate the tool
    {
        let agent_name = "full_cycle_echo_agent";
        let inputs = json!({
            "session_id": session_id,
            "agent_name": agent_name,
            "description": "An agent that echoes things"
        });
        let rtfs_inputs = json_to_rtfs_value(&inputs).unwrap();

        let result = marketplace
            .execute_capability("planner.synthesize_agent_from_trace", &rtfs_inputs)
            .await;

        // If consolidation fails, check why.
        // Likely failing to find session on disk if paths mismatched.
        if let Err(e) = &result {
            println!("Consolidation error: {:?}", e);
        }
        assert!(result.is_ok());

        // Verify output
        let output = rtfs_value_to_json(&result.unwrap()).unwrap();
        let path = output["manifest"]["path"].as_str().unwrap();
        assert!(
            std::path::Path::new(path).exists(),
            "Agent RTFS file not created"
        );
        println!("Created agent at: {}", path);

        // Cleanup created agent file
        // std::fs::remove_file(path).unwrap_or(());
    }

    // --- 4. Record Learning (Update Memory) ---
    // Simulate `ccos_record_learning` tool behavior
    {
        let pattern = "We now have an echo agent for echoing tasks.";
        let context = "Consolidation test";
        let outcome = "Created full_cycle_echo_agent";
        let confidence = 1.0;

        let desc = format!("{} [Context: {}] [Outcome: {}]", pattern, context, outcome);
        // LearnedPattern::new takes string slices
        let p = LearnedPattern::new("learned-echo-agent", &desc).with_confidence(confidence);

        let mut mem = agent_memory.write().await;
        mem.store_learned_pattern(p);

        // Persistence
        let memory_file = _temp_dir.path().join("system_memory.json");
        mem.save_to_disk(&memory_file).unwrap();
        assert!(memory_file.exists());
    }

    // --- 5. Recall Memory ---
    // Simulate `ccos_recall_memories`
    {
        let memory_file = _temp_dir.path().join("system_memory.json");

        let wm_backend2 = InMemoryJsonlBackend::new(None, None, None);
        let working_memory2 = Arc::new(std::sync::Mutex::new(WorkingMemory::new(Box::new(
            wm_backend2,
        ))));
        let mut agent_memory2 = AgentMemory::new("test-agent", working_memory2);

        agent_memory2.load_from_disk(&memory_file).unwrap();

        let relevant = agent_memory2.recall_relevant(&["learning"], None).unwrap();
        assert!(!relevant.is_empty(), "Should recall learned pattern");
        assert!(
            relevant[0].content.contains("echo agent"),
            "Content should match"
        );
    }

    // Cleanup - remove session file
    let sessions_dir = ccos::utils::fs::get_configured_sessions_path();
    // Try to find files starting with session id
    if let Ok(entries) = std::fs::read_dir(sessions_dir) {
        for entry in entries.flatten() {
            let name = entry.file_name().to_string_lossy().to_string();
            if name.starts_with(&session_id) {
                let _ = std::fs::remove_file(entry.path());
            }
        }
    }
}
