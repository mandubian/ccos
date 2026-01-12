use ccos::capabilities::registry::CapabilityRegistry;
use ccos::capability_marketplace::CapabilityMarketplace;
use ccos::mcp::session::{create_session_store, Session};
use serde_json::json;
use std::sync::Arc;
use tokio::sync::RwLock;

#[tokio::test]
async fn test_trace_synthesis_flow() {
    // 1. Setup Environment
    let temp_dir = tempfile::TempDir::new().expect("Failed to create temp dir");
    // Ensure directories exist
    let sessions_path = temp_dir.path().join("sessions");
    let caps_path = temp_dir.path().join("capabilities");
    let agents_path = caps_path.join("agents");

    std::fs::create_dir_all(&sessions_path).unwrap();
    std::fs::create_dir_all(&agents_path).unwrap();

    // Set env vars to force usage of temp dir
    // logic in utils::fs uses CCOS_CAPABILITY_STORAGE as base
    std::env::set_var("CCOS_CAPABILITY_STORAGE", caps_path.to_str().unwrap());
    std::env::set_var("CCOS_SESSIONS_STORAGE", sessions_path.to_str().unwrap());

    // 2. Create and Save a Session
    let mut session = Session::new("test-goal");
    session.add_step(
        "fs.list_dir",
        json!({"path": "/tmp"}),
        json!(["file1", "file2"]),
        true,
    );
    session.add_step(
        "fs.read_file",
        json!({"path": "/tmp/file1"}),
        json!("content"),
        true,
    );

    // Save session
    let path = ccos::mcp::session::save_session(&session, Some(&sessions_path), None)
        .expect("Failed to save session");

    println!("Session saved to: {:?}", path);
    assert!(path.exists());

    // 3. Simulate Synthesis Logic
    // We fetch the session back from disk to verify that part
    let retrieved_session = ccos::mcp::session::find_session_on_disk(&session.id)
        .await
        .expect("Failed to find session on disk");

    assert_eq!(retrieved_session.id, session.id);
    assert_eq!(retrieved_session.steps.len(), 2);

    // Generate RTFS content equivalent to what the capability handles
    let agent_name = "test_agent";
    let agent_id = format!("agent.{}", "test_agent"); // simple slugify
    let description = "Test Agent Description";

    let steps_logic = {
        let mut lines = Vec::new();
        lines.push("(do".to_string());
        for step in &retrieved_session.steps {
            lines.push(format!("    {}", step.rtfs_code));
        }
        lines.push("  )".to_string());
        lines.join("\n")
    };

    let rtfs_content = format!(
        r#";; Synthesized Agent
(capability "{}"
  (description "{}")
  (meta {{
    :kind :agent
    :planning false
    :source-session "{}"
  }})
  (action [inputs]
    {}
  ))
"#,
        agent_id, description, session.id, steps_logic
    );

    // Write to agents dir
    let filename = format!("{}.rtfs", agent_name);
    let filepath = agents_path.join(&filename);
    std::fs::write(&filepath, &rtfs_content).expect("Failed to write agent file");

    // 4. Verify Output File
    assert!(filepath.exists());
    let content = std::fs::read_to_string(&filepath).unwrap();
    println!("Generated RTFS Content:\n{}", content);

    assert!(content.contains(":kind :agent"));
    assert!(content.contains(r#":planning false"#));
    assert!(content.contains(&format!(":source-session \"{}\"", session.id)));
    assert!(content.contains("(call \"fs.list_dir\""));
    assert!(content.contains("(call \"fs.read_file\""));

    // Cleanup env vars (optional as tests are isolated usually, but good practice)
    std::env::remove_var("CCOS_CAPABILITY_STORAGE");
    std::env::remove_var("CCOS_SESSIONS_STORAGE");
}
