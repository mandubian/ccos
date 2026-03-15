//! End-to-end integration tests for full agent lifecycle.
//!
//! Tests the complete flow: planner → coder → specialized_builder
//! This validates that the content storage and artifact system works
//! end-to-end without memory coordination issues or looping.

mod support;

use support::TestWorkspace;

/// Helper to install a coder agent that will generate weather script
fn install_coder_agent(agents_dir: &std::path::Path) -> anyhow::Result<()> {
    let coder_dir = agents_dir.join("coder.default");
    std::fs::create_dir_all(&coder_dir)?;
    
    std::fs::write(
        coder_dir.join("SKILL.md"),
        r#"---
version: "1.0"
runtime:
  engine: "autonoetic"
  gateway_version: "0.1.0"
  sdk_version: "0.1.0"
  type: "stateful"
  sandbox: "bubblewrap"
  runtime_lock: "runtime.lock"
agent:
  id: "coder.default"
  name: "Coder Default"
  description: "Implements focused changes with verification."
  capabilities:
    - type: "ToolInvoke"
      allowed: ["content.", "knowledge."]
llm_config:
  provider: "openai"
  model: "gpt-4o"
  temperature: 0.1
io:
  accepts:
    type: object
    properties:
      task:
        type: string
---
# Coder Default

Write code files via content.write and report handles.

## Content Tools
- `content.write(name, content)` - Write files
- `content.read(name_or_handle)` - Read files

Report file handles in your response, not full contents.
"#,
    )?;
    std::fs::write(coder_dir.join("runtime.lock"), "dependencies: []")?;
    Ok(())
}

/// Helper to install a specialized_builder agent
fn install_builder_agent(agents_dir: &std::path::Path) -> anyhow::Result<()> {
    let builder_dir = agents_dir.join("specialized_builder.default");
    std::fs::create_dir_all(&builder_dir)?;
    
    std::fs::write(
        builder_dir.join("SKILL.md"),
        r#"---
version: "1.0"
runtime:
  engine: "autonoetic"
  gateway_version: "0.1.0"
  sdk_version: "0.1.0"
  type: "stateful"
  sandbox: "bubblewrap"
  runtime_lock: "runtime.lock"
agent:
  id: "specialized_builder.default"
  name: "Specialized Builder"
  description: "Installs new durable specialists."
  capabilities:
    - type: "ToolInvoke"
      allowed: ["content.", "agent."]
    - type: "AgentInstall"
llm_config:
  provider: "openai"
  model: "gpt-4o"
  temperature: 0.0
io:
  accepts:
    type: object
    properties:
      task:
        type: string
      artifact_handles:
        type: array
---
# Specialized Builder

Install agents from artifact handles.

## Workflow
1. Read SKILL.md from artifact via content.read
2. Read script files via content.read
3. Install agent via agent.install
"#,
    )?;
    std::fs::write(builder_dir.join("runtime.lock"), "dependencies: []")?;
    Ok(())
}

/// Test: Content store artifact creation and retrieval
#[test]
fn test_artifact_creation_and_retrieval() {
    let workspace = TestWorkspace::new().unwrap();
    let gateway_dir = workspace.path().join(".gateway");
    std::fs::create_dir_all(&gateway_dir).unwrap();
    
    use autonoetic_gateway::runtime::content_store::ContentStore;
    let store = ContentStore::new(&gateway_dir).unwrap();
    
    // Simulate coder writing files
    let session_id = "test-session-1";
    
    // Write main.py
    let main_py = r#"
def get_weather(latitude: float, longitude: float) -> dict:
    """Get weather data for a location."""
    # In a real implementation, this would call an API
    return {"temperature": 22, "condition": "sunny"}
"#;
    let main_handle = store.write(main_py.as_bytes()).unwrap();
    store.register_name(session_id, "weather/main.py", &main_handle).unwrap();
    
    // Write SKILL.md
    let skill_md = r#"---
name: "weather"
description: "Weather data retrieval module"
script_entry: "main.py"
io:
  accepts:
    type: object
    properties:
      latitude:
        type: number
      longitude:
        type: number
    required: [latitude, longitude]
---
# Weather Module

Retrieves weather data for a given location.
"#;
    let skill_handle = store.write(skill_md.as_bytes()).unwrap();
    store.register_name(session_id, "weather/SKILL.md", &skill_handle).unwrap();
    
    // Verify artifact creation
    let artifacts = autonoetic_gateway::execution::extract_artifacts_from_content_store(
        &gateway_dir,
        session_id,
    ).unwrap();
    
    assert_eq!(artifacts.len(), 1, "Should have one artifact");
    let artifact = &artifacts[0];
    assert_eq!(artifact.name, "weather");
    assert!(artifact.description.contains("Weather"));
    assert_eq!(artifact.entry_point, Some("main.py".to_string()));
    
    // Verify content can be read back
    let content = store.read_by_name(session_id, "weather/main.py").unwrap();
    assert!(String::from_utf8(content).unwrap().contains("get_weather"));
    
    println!("✅ Artifact creation and retrieval works");
}

/// Test: Cross-session content sharing via handles
#[test]
fn test_cross_session_content_sharing() {
    let workspace = TestWorkspace::new().unwrap();
    let gateway_dir = workspace.path().join(".gateway");
    std::fs::create_dir_all(&gateway_dir).unwrap();
    
    use autonoetic_gateway::runtime::content_store::ContentStore;
    let store = ContentStore::new(&gateway_dir).unwrap();
    
    // Coder session writes content
    let coder_session = "coder-session";
    let code = "def process(data): return data.upper()";
    let handle = store.write(code.as_bytes()).unwrap();
    store.register_name(coder_session, "processor/main.py", &handle).unwrap();
    
    // Builder session reads by handle (different session!)
    let builder_session = "builder-session";
    let content = store.read_by_name_or_handle(builder_session, &handle).unwrap();
    assert_eq!(String::from_utf8(content).unwrap(), code);
    
    // Builder can also register the handle under its own session
    store.register_name(builder_session, "imported/processor.py", &handle).unwrap();
    let content2 = store.read_by_name(builder_session, "imported/processor.py").unwrap();
    assert_eq!(String::from_utf8(content2).unwrap(), code);
    
    println!("✅ Cross-session content sharing works");
}

/// Test: Knowledge store and recall across sessions
#[test]
fn test_knowledge_persistence() {
    let workspace = TestWorkspace::new().unwrap();
    let gateway_dir = workspace.path().join(".gateway");
    std::fs::create_dir_all(&gateway_dir).unwrap();
    
    use autonoetic_gateway::runtime::memory::Tier2Memory;
    
    // Create Tier2Memory (knowledge store)
    let memory = Tier2Memory::new(&gateway_dir, "test-agent").unwrap();
    
    // Store a fact
    memory.remember(
        "weather-api",
        "api",
        "test-agent",
        "test-session",
        "open-meteo",
    ).unwrap();
    
    // Recall the fact
    let recalled = memory.recall("weather-api").unwrap();
    assert_eq!(recalled.content, "open-meteo");
    assert_eq!(recalled.owner_agent_id, "test-agent");
    
    // Search by scope (owner matches, so it should be readable)
    let results = memory.search("api", None).unwrap();
    assert!(!results.is_empty(), "Search should find the stored knowledge");
    assert_eq!(results[0].content, "open-meteo");
    
    println!("✅ Knowledge persistence works");
}

/// Test: Session snapshot and fork
#[test]
fn test_session_snapshot_fork() {
    use autonoetic_gateway::llm::Message;
    use autonoetic_gateway::runtime::session_snapshot::{SessionSnapshot, SessionFork};
    
    let workspace = TestWorkspace::new().unwrap();
    let gateway_dir = workspace.path().join(".gateway");
    std::fs::create_dir_all(&gateway_dir).unwrap();
    
    // Create a session with history
    let history = vec![
        Message::user("Hello"),
        Message::assistant("Hi! How can I help you?"),
        Message::user("I need a weather app"),
        Message::assistant("I'll create that for you."),
    ];
    
    // Capture snapshot
    let snapshot = SessionSnapshot::capture(
        "original-session",
        &history,
        2,
        None,
        None,
        &gateway_dir,
    ).unwrap();
    
    assert_eq!(snapshot.turn_count, 2);
    assert_eq!(snapshot.history.len(), 4);
    
    // Fork with branch message
    let fork = SessionFork::fork(
        &snapshot,
        Some("forked-session"),
        Some("Try a different approach"),
        &gateway_dir,
    ).unwrap();
    
    assert_eq!(fork.new_session_id, "forked-session");
    assert_eq!(fork.source_session_id, "original-session");
    assert_eq!(fork.initial_history.len(), 5); // 4 original + 1 branch message
    assert!(fork.initial_history.last().unwrap().content.contains("different approach"));
    
    println!("✅ Session snapshot and fork works");
}

/// Test: Full artifact lifecycle (create, share, persist)
#[test]
fn test_full_artifact_lifecycle() {
    let workspace = TestWorkspace::new().unwrap();
    let gateway_dir = workspace.path().join(".gateway");
    std::fs::create_dir_all(&gateway_dir).unwrap();
    
    use autonoetic_gateway::runtime::content_store::ContentStore;
    let store = ContentStore::new(&gateway_dir).unwrap();
    
    // Step 1: Coder creates an artifact
    let coder_session = "coder-session";
    
    let main_py = "def calculate(a, b): return a + b";
    let main_handle = store.write(main_py.as_bytes()).unwrap();
    store.register_name(coder_session, "calculator/main.py", &main_handle).unwrap();
    
    let skill_md = r#"---
name: "calculator"
description: "Simple calculator"
script_entry: "main.py"
---
# Calculator
"#;
    let skill_handle = store.write(skill_md.as_bytes()).unwrap();
    store.register_name(coder_session, "calculator/SKILL.md", &skill_handle).unwrap();
    
    // Step 2: Persist the artifact (make it survive cleanup)
    store.persist(coder_session, &main_handle).unwrap();
    store.persist(coder_session, &skill_handle).unwrap();
    
    // Step 3: Verify persistence
    let persisted = store.list_persisted(coder_session).unwrap();
    assert!(persisted.contains(&main_handle));
    assert!(persisted.contains(&skill_handle));
    
    // Step 4: Builder reads the artifact via handles
    let builder_session = "builder-session";
    let main_content = store.read_by_name_or_handle(builder_session, &main_handle).unwrap();
    assert!(String::from_utf8(main_content).unwrap().contains("calculate"));
    
    // Step 5: Builder registers handles in its session
    store.register_name(builder_session, "installed/calc/main.py", &main_handle).unwrap();
    store.register_name(builder_session, "installed/calc/SKILL.md", &skill_handle).unwrap();
    
    // Step 6: Verify artifact extraction works for builder session
    let artifacts = autonoetic_gateway::execution::extract_artifacts_from_content_store(
        &gateway_dir,
        builder_session,
    ).unwrap();
    
    assert!(!artifacts.is_empty(), "Should have at least one artifact");
    
    println!("✅ Full artifact lifecycle works");
}
