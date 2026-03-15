//! Live integration test for coder agent content generation with real LLM.
//!
//! Run with:
//!   OPENROUTER_API_KEY=<key> cargo test -p autonoetic-gateway --test coder_live_integration -- --nocapture
//!
//! This test spins up a real gateway and uses a live LLM to test:
//! 1. Coder agent receives a task
//! 2. LLM decides to use content.write tool
//! 3. Files are stored in content store
//! 4. SKILL.md creates an artifact
//! 5. Another agent can read the generated files

use autonoetic_gateway::llm::{self, CompletionRequest, LlmDriver, Message, ToolDefinition};
use autonoetic_gateway::runtime::content_store::ContentStore;
use serde_json::json;
use std::sync::Arc;

/// Create a driver for OpenRouter.
fn make_openrouter_driver(model: &str) -> anyhow::Result<Arc<dyn LlmDriver>> {
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(120))
        .build()?;

    let resolved = llm::provider::resolve(
        "openrouter",
        model,
        Some(0.3),
        Some(2048),
        None,
        None,
        false, // chat_only
    )?;

    Ok(Arc::new(llm::openai::OpenAiDriver::new(client, resolved)))
}

#[tokio::test]
async fn test_live_coder_tool_call_content_write() -> anyhow::Result<()> {
    // Skip if key not set
    let api_key = match std::env::var("OPENROUTER_API_KEY") {
        Ok(k) => k,
        Err(_) => {
            eprintln!("Skipping live test: OPENROUTER_API_KEY not set");
            eprintln!("Run with: OPENROUTER_API_KEY=<key> cargo test -p autonoetic-gateway --test coder_live_integration -- --nocapture");
            return Ok(());
        }
    };
    drop(api_key); // Driver reads from env directly

    // Use a capable model for code generation
    let model = "google/gemini-3-flash-preview";

    println!("=== Testing live coder with model: {} ===", model);

    let driver = make_openrouter_driver(model)?;

    // Define the content.write tool for the LLM
    let content_write_tool = ToolDefinition {
        name: "content.write".to_string(),
        description: "Write content to the session's content store. Returns a content handle (sha256:...).".to_string(),
        input_schema: json!({
            "type": "object",
            "properties": {
                "name": {
                    "type": "string",
                    "description": "File name (e.g., 'main.py', 'SKILL.md')"
                },
                "content": {
                    "type": "string",
                    "description": "File content to store"
                }
            },
            "required": ["name", "content"]
        }),
    };

    // Make a request
    let req = CompletionRequest {
        model: model.to_string(),
        messages: vec![
            Message::system(
                r#"You are a coder agent. Your job is to write Python code files.

Rules:
1. Use the content.write tool to save files - do NOT return file contents in your response
2. Create a SKILL.md file with YAML frontmatter for artifact generation
3. Report only file names and handles in your response

SKILL.md format:
```yaml
---
name: "module_name"
description: "Brief description"
script_entry: "main.py"
---
# Module Name

Description in markdown.
```

Always write working, syntactically correct Python code."#
            ),
            Message::user("Write a Python module that calculates the Fibonacci sequence up to n terms. Include a fibonacci(n) function and a SKILL.md."),
        ],
        tools: vec![content_write_tool],
        max_tokens: Some(2048),
        temperature: Some(0.3),
        metadata: None,
    };

    let resp = driver.complete(&req).await?;

    println!("=== LLM Response ===");
    println!("Text: {:?}", resp.text);
    println!("Tool calls: {:?}", resp.tool_calls);
    println!("Stop reason: {:?}", resp.stop_reason);
    println!("Usage: in={} out={}", resp.usage.input_tokens, resp.usage.output_tokens);

    // Validate we got content.write tool calls
    let content_writes: Vec<_> = resp.tool_calls
        .iter()
        .filter(|tc| tc.name == "content.write")
        .collect();

    println!("\n=== Content Writes ({} total) ===", content_writes.len());

    for (i, tool_call) in content_writes.iter().enumerate() {
        let args: serde_json::Value = serde_json::from_str(&tool_call.arguments)
            .unwrap_or(json!({"error": "invalid JSON"}));

        println!("\nFile #{}:", i + 1);
        println!("  Name: {:?}", args.get("name").and_then(|v| v.as_str()));
        if let Some(content) = args.get("content").and_then(|v| v.as_str()) {
            println!("  Content length: {} chars", content.len());
            println!("  Preview: {}...", &content[..content.len().min(100)]);
        }
    }

    // Validate at least one file was written
    assert!(
        !content_writes.is_empty(),
        "Expected at least one content.write tool call, got: text={:?}, tool_calls={:?}",
        resp.text,
        resp.tool_calls
    );

    // Validate at least one file is SKILL.md
    let has_skill_md = content_writes.iter().any(|tc| {
        serde_json::from_str::<serde_json::Value>(&tc.arguments)
            .ok()
            .and_then(|args| args.get("name").and_then(|n| n.as_str()).map(|n| n.contains("SKILL.md")))
            .unwrap_or(false)
    });

    assert!(has_skill_md, "Expected at least one content.write for SKILL.md");

    println!("\n✅ Test passed: LLM generated files via content.write");
    println!("   - {} files written", content_writes.len());
    println!("   - SKILL.md present: {}", has_skill_md);

    Ok(())
}

#[tokio::test]
async fn test_live_coder_with_content_store_integration() -> anyhow::Result<()> {
    // Skip if key not set
    if std::env::var("OPENROUTER_API_KEY").is_err() {
        eprintln!("Skipping live test: OPENROUTER_API_KEY not set");
        return Ok(());
    }

    // Use a faster model
    let model = "google/gemini-3-flash-preview";
    println!("=== Testing full content store integration with model: {} ===", model);

    // Set up a temp directory with gateway
    let temp = tempfile::tempdir()?;
    let gateway_dir = temp.path().join(".gateway");
    std::fs::create_dir_all(&gateway_dir)?;

    let store = ContentStore::new(&gateway_dir)?;
    let session_id = "test-session-1";

    // Simulate what an agent would do: write files to content store
    let main_py = r#""""
Fibonacci sequence calculator.
"""

def fibonacci(n: int) -> list[int]:
    """Calculate Fibonacci sequence up to n terms."""
    if n <= 0:
        return []
    elif n == 1:
        return [0]
    
    sequence = [0, 1]
    for _ in range(2, n):
        sequence.append(sequence[-1] + sequence[-2])
    return sequence


if __name__ == "__main__":
    import sys
    n = int(sys.argv[1]) if len(sys.argv) > 1 else 10
    result = fibonacci(n)
    print(f"Fibonacci({n}): {result}")
"#;

    let skill_md = r#"---
name: "fibonacci"
description: "Fibonacci sequence calculator module"
script_entry: "main.py"
io:
  accepts:
    type: object
    properties:
      n:
        type: integer
        description: Number of Fibonacci terms to calculate
    required: [n]
  returns:
    type: object
    properties:
      sequence:
        type: array
        items:
          type: integer
---
# Fibonacci Calculator

A Python module that calculates Fibonacci sequences.

## Usage

```bash
python main.py 10
# Output: Fibonacci(10): [0, 1, 1, 2, 3, 5, 8, 13, 21, 34]
```
"#;

    // Write files to content store (as agent would)
    let main_handle = store.write(main_py.as_bytes())?;
    store.register_name(session_id, "fibonacci/main.py", &main_handle)?;
    println!("✓ Wrote fibonacci/main.py: {}", main_handle);

    let skill_handle = store.write(skill_md.as_bytes())?;
    store.register_name(session_id, "fibonacci/SKILL.md", &skill_handle)?;
    println!("✓ Wrote fibonacci/SKILL.md: {}", skill_handle);

    // Verify content can be read back
    let main_content = store.read_by_name(session_id, "fibonacci/main.py")?;
    assert!(String::from_utf8(main_content)?.contains("def fibonacci"));
    println!("✓ Verified main.py content");

    let skill_content = store.read_by_name(session_id, "fibonacci/SKILL.md")?;
    assert!(String::from_utf8(skill_content)?.contains("name: \"fibonacci\""));
    println!("✓ Verified SKILL.md content");

    // Extract artifact
    let artifacts = autonoetic_gateway::execution::extract_artifacts_from_content_store(
        &gateway_dir,
        session_id,
    )?;

    println!("\n=== Artifacts ===");
    for artifact in &artifacts {
        println!("Name: {}", artifact.name);
        println!("Description: {}", artifact.description);
        println!("Entry point: {:?}", artifact.entry_point);
        println!("Files: {:?}", artifact.files);
    }

    assert_eq!(artifacts.len(), 1);
    assert_eq!(artifacts[0].name, "fibonacci");
    assert_eq!(artifacts[0].entry_point, Some("main.py".to_string()));
    assert!(artifacts[0].files.contains(&"fibonacci/main.py".to_string()));
    assert!(artifacts[0].files.contains(&"fibonacci/SKILL.md".to_string()));

    println!("\n✅ Test passed: Content store integration works");

    // Stats
    let stats = store.stats()?;
    println!("Content store stats: {} entries, {} bytes", stats.entry_count, stats.total_size_bytes);

    Ok(())
}
