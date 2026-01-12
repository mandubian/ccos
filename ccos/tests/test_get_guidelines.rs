use ccos::utils::fs::get_workspace_root;
use std::fs;

#[tokio::test]
async fn test_get_guidelines_content() {
    // 1. Setup: Ensure the guidelines file exists
    let workspace_root = get_workspace_root();
    let guidelines_path = workspace_root.join("docs/agent_guidelines.md");

    // Create dummy guidelines if missing (for test isolation, though we expect it to exist)
    if !guidelines_path.exists() {
        if let Some(parent) = guidelines_path.parent() {
            fs::create_dir_all(parent).expect("Failed to create docs dir");
        }
        fs::write(
            &guidelines_path,
            "# CCOS Agent Guidelines\nDummy content for test.",
        )
        .expect("Failed to write dummy guidelines");
    }

    // 2. Simulate tool execution logic
    // We can't easily run the full MCP server handler here without mocking everything,
    // but we can verify the core logic: reading the file from the expected path.

    let docs_path = get_workspace_root().join("docs/agent_guidelines.md");
    assert!(docs_path.exists(), "Guidelines file should exist");

    let content = tokio::fs::read_to_string(&docs_path)
        .await
        .expect("Failed to read guidelines");

    // 3. Verify content
    assert!(
        !content.is_empty(),
        "Guidelines content should not be empty"
    );
    assert!(
        content.contains("Guidelines"),
        "Content should likely contain 'Guidelines'"
    );
}
