//! Integration tests for coder agent content generation workflow.
//!
//! Tests the content store and artifact creation patterns that coder agents use.

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

/// Test the basic coder workflow: write files, create artifact.
#[test]
fn test_coder_workflow_single_file_artifact() {
    let (_dir, gateway_dir) = create_test_gateway();
    let store = ContentStore::new(&gateway_dir).unwrap();
    let session_id = "coder-session-1";

    // Step 1: Coder writes main.py
    let main_py = r#"def hello(name: str) -> str:
    """Return a greeting."""
    return f"Hello, {name}!"

if __name__ == "__main__":
    print(hello("World"))
"#;
    let main_handle = store.write(main_py.as_bytes()).unwrap();
    store
        .register_name(session_id, "greeting/main.py", &main_handle)
        .unwrap();

    // Step 2: Coder writes SKILL.md for artifact generation
    let skill_md = r#"---
name: "greeting"
description: "A simple greeting module"
script_entry: "main.py"
io:
  accepts:
    type: object
    properties:
      name:
        type: string
    required: [name]
  returns:
    type: string
---
# Greeting Module

A Python module that generates personalized greetings.
"#;
    let skill_handle = store.write(skill_md.as_bytes()).unwrap();
    store
        .register_name(session_id, "greeting/SKILL.md", &skill_handle)
        .unwrap();

    // Step 3: Verify artifact is created
    let artifacts = extract_artifacts_from_content_store(&gateway_dir, session_id).unwrap();

    assert_eq!(artifacts.len(), 1, "Should have one artifact");
    let artifact = &artifacts[0];
    assert_eq!(artifact.name, "greeting");
    assert_eq!(artifact.description, "A simple greeting module");
    assert_eq!(artifact.entry_point, Some("main.py".to_string()));
    assert!(artifact.files.contains(&"greeting/main.py".to_string()));
    assert!(artifact.files.contains(&"greeting/SKILL.md".to_string()));

    // Step 4: Planner can read the files via content.read
    let main_content = store.read_by_name(session_id, "greeting/main.py").unwrap();
    assert_eq!(String::from_utf8(main_content).unwrap(), main_py);
}

/// Test multi-file project: module with __init__.py, main.py, utils.py
#[test]
fn test_coder_workflow_multi_file_project() {
    let (_dir, gateway_dir) = create_test_gateway();
    let store = ContentStore::new(&gateway_dir).unwrap();
    let session_id = "coder-session-2";

    // Coder creates a Python package
    let files = vec![
        (
            "calculator/__init__.py",
            r#""""
A simple calculator module.
"""

__version__ = "1.0.0"
"#,
        ),
        (
            "calculator/main.py",
            r#"from .operations import add, multiply
from .utils import format_result

def calculate(a: int, b: int, op: str) -> str:
    """Perform calculation and return formatted result."""
    if op == "add":
        result = add(a, b)
    elif op == "multiply":
        result = multiply(a, b)
    else:
        raise ValueError(f"Unknown operation: {op}")
    return format_result(a, op, b, result)

if __name__ == "__main__":
    print(calculate(2, 3, "add"))
"#,
        ),
        (
            "calculator/operations.py",
            r#"def add(a: int, b: int) -> int:
    """Add two numbers."""
    return a + b

def multiply(a: int, b: int) -> int:
    """Multiply two numbers."""
    return a * b
"#,
        ),
        (
            "calculator/utils.py",
            r#"def format_result(a: int, op: str, b: int, result: int) -> str:
    """Format calculation result as string."""
    return f"{a} {op} {b} = {result}"
"#,
        ),
        (
            "calculator/SKILL.md",
            r#"---
name: "calculator"
description: "A simple Python calculator package"
script_entry: "main.py"
io:
  accepts:
    type: object
    properties:
      a:
        type: integer
      b:
        type: integer
      op:
        type: string
        enum: [add, multiply]
    required: [a, b, op]
  returns:
    type: string
---
# Calculator Package

A well-structured Python calculator with operations and utilities.

## Usage

```python
from calculator.main import calculate
result = calculate(2, 3, "add")  # "2 add 3 = 5"
```
"#,
        ),
    ];

    // Write all files
    for (name, content) in &files {
        let handle = store.write(content.as_bytes()).unwrap();
        store.register_name(session_id, name, &handle).unwrap();
    }

    // Extract artifact
    let artifacts = extract_artifacts_from_content_store(&gateway_dir, session_id).unwrap();

    assert_eq!(artifacts.len(), 1);
    let artifact = &artifacts[0];
    assert_eq!(artifact.name, "calculator");
    assert_eq!(artifact.entry_point, Some("main.py".to_string()));

    // All 5 files should be in the artifact
    assert_eq!(artifact.files.len(), 5);
    assert!(artifact
        .files
        .contains(&"calculator/__init__.py".to_string()));
    assert!(artifact.files.contains(&"calculator/main.py".to_string()));
    assert!(artifact
        .files
        .contains(&"calculator/operations.py".to_string()));
    assert!(artifact.files.contains(&"calculator/utils.py".to_string()));
    assert!(artifact.files.contains(&"calculator/SKILL.md".to_string()));

    // Planner can read any file by name
    let utils_content = store
        .read_by_name(session_id, "calculator/utils.py")
        .unwrap();
    assert!(String::from_utf8(utils_content)
        .unwrap()
        .contains("format_result"));
}

/// Test content handle sharing: planner reads coder's output via handle (with root visibility).
#[test]
fn test_cross_session_content_handle_sharing() {
    let (_dir, gateway_dir) = create_test_gateway();
    let store = ContentStore::new(&gateway_dir).unwrap();

    let root_session = "planner-session-3";
    let coder_session = "planner-session-3/coder-abc";

    // Set up root visibility
    store.set_root_session(coder_session, root_session).unwrap();

    // Coder writes session-visible content
    let script = "print('Generated by coder')";
    let handle = store.write(script.as_bytes()).unwrap();
    store
        .register_name_with_visibility(
            coder_session,
            "output.py",
            &handle,
            autonoetic_gateway::runtime::content_store::ContentVisibility::Session,
        )
        .unwrap();

    // Planner (root) reads by handle — visible because session-scoped
    let content = store.read_by_name_or_handle(root_session, &handle).unwrap();
    assert_eq!(String::from_utf8(content).unwrap(), script);

    // Planner can also register it in its own session by name
    store
        .register_name(root_session, "coder_script.py", &handle)
        .unwrap();
    let content2 = store.read_by_name(root_session, "coder_script.py").unwrap();
    assert_eq!(String::from_utf8(content2).unwrap(), script);
}

/// Test persistence: content survives session cleanup intent.
#[test]
fn test_content_persistence_for_reusable_artifacts() {
    let (_dir, gateway_dir) = create_test_gateway();
    let store = ContentStore::new(&gateway_dir).unwrap();
    let session_id = "builder-session";

    // Builder creates an artifact
    let script = "def reusable(): return 'I can be reused'";
    let handle = store.write(script.as_bytes()).unwrap();
    store
        .register_name(session_id, "reusable/main.py", &handle)
        .unwrap();

    // Content still readable by any session via handle
    let content = store.read(&handle).unwrap();
    assert_eq!(String::from_utf8(content).unwrap(), script);
}

/// Test content deduplication across agents.
#[test]
fn test_content_deduplication_across_agents() {
    let (_dir, gateway_dir) = create_test_gateway();
    let store = ContentStore::new(&gateway_dir).unwrap();

    // Agent A writes some content
    let common_code = "def common_function(): return 42";
    let handle_a = store.write(common_code.as_bytes()).unwrap();
    store
        .register_name("session-a", "common.py", &handle_a)
        .unwrap();

    // Agent B writes identical content
    let handle_b = store.write(common_code.as_bytes()).unwrap();
    store
        .register_name("session-b", "common.py", &handle_b)
        .unwrap();

    // Same content = same handle
    assert_eq!(handle_a, handle_b);

    // Only one blob in storage
    let stats = store.stats().unwrap();
    assert_eq!(stats.entry_count, 1);
}

/// Test artifact extraction from root-level SKILL.md.
#[test]
fn test_artifact_from_root_skill_md() {
    let (_dir, gateway_dir) = create_test_gateway();
    let store = ContentStore::new(&gateway_dir).unwrap();
    let session_id = "root-agent-session";

    // Simple agent with root-level SKILL.md
    let skill_md = r#"---
name: "simple_tool"
description: "A simple tool"
script_entry: "tool.py"
---
# Simple Tool
"#;
    let skill_handle = store.write(skill_md.as_bytes()).unwrap();
    store
        .register_name(session_id, "SKILL.md", &skill_handle)
        .unwrap();

    let tool_py = "print('tool executed')";
    let tool_handle = store.write(tool_py.as_bytes()).unwrap();
    store
        .register_name(session_id, "tool.py", &tool_handle)
        .unwrap();

    let artifacts = extract_artifacts_from_content_store(&gateway_dir, session_id).unwrap();

    assert_eq!(artifacts.len(), 1);
    assert_eq!(artifacts[0].name, "simple_tool");
    // Should include files at root level
    assert!(artifacts[0].files.contains(&"SKILL.md".to_string()));
    assert!(artifacts[0].files.contains(&"tool.py".to_string()));
}
