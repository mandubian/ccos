use std::error::Error;
use std::path::PathBuf;

use ccos::utils::run_example_with_args;

// IMPORTANT: This test requires a valid OPENROUTER_API_KEY to be set in the environment,
// or for the 'stub/dev' profile to be explicitly selected via --profile stub/dev
// and CCOS_ALLOW_STUB_PROVIDER=1 to be set.
// It also requires network access for LLM calls if not using the stub provider.
#[tokio::test]
async fn test_autonomous_planner_with_simple_goal() -> Result<(), Box<dyn Error>> {
    // Ensure CCOS_ALLOW_STUB_PROVIDER is set for local testing if no real API key is available
    if std::env::var("OPENROUTER_API_KEY").is_err() {
        eprintln!("WARNING: OPENROUTER_API_KEY not set. Using stub LLM provider for test. For real LLM interaction, set OPENROUTER_API_KEY.");
        std::env::set_var("CCOS_DELEGATING_MODEL", "stub");
        std::env::set_var("CCOS_LLM_MODEL", "stub");
        std::env::set_var("CCOS_LLM_PROVIDER", "stub");
        std::env::set_var("CCOS_ALLOW_STUB_PROVIDER", "1");
    }

    let example_path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("examples")
        .join("smart_assistant_planner_viz.rs");

    let goal = "Find the current time and display it.";
    let args = vec![
        "--goal",
        goal,
        "--execute-plan",
        // Add a profile explicitly for testing to ensure consistency
        // For actual LLM, you'd omit this or specify a real profile like "openrouter_free:balanced"
        // For now, if OPENROUTER_API_KEY is not set, we default to stub above.
        // If it IS set, we let the default profile from agent_config.toml take over.
        // "--profile", "stub/dev", // Only use this if you want to force stub for testing
    ];

    let (stdout, stderr) = run_example_with_args(&example_path, args).await?;

    // Print stdout and stderr for debugging purposes
    eprintln!("\n--- Example Stdout ---\n{}", stdout);
    eprintln!("\n--- Example Stderr ---\n{}", stderr);

    // Basic assertions to check if a plan was created and executed.
    // The exact output might vary based on capabilities, but we expect certain keywords.
    assert!(
        stdout.contains("Proposed Steps") || stdout.contains("Plan RTFS"),
        "Stdout should contain proposed steps or plan RTFS"
    );
    // Check for execution result in stdout
    assert!(
        stdout.contains("✅ Execution result:"),
        "Stdout should contain successful execution result"
    );
    // Check for plan execution failure in stderr if it was a stub run, otherwise expect success
    if std::env::var("OPENROUTER_API_KEY").is_err() {
        // If stub, expect to see the plan being generated, even if execution fails meaningfully
        assert!(
            stderr.contains("WARNING: Delegating arbiter is running with the stub LLM provider") || stdout.contains("call :system.time.get_current_ms"),
            "Stderr should indicate stub LLM or stdout should contain call to get_current_time_ms"
        );
        assert!(
            !stdout.contains("❌ Plan execution failed") && !stderr.contains("❌ Plan execution failed"),
            "Output should NOT contain explicit plan execution failure with stub, if planning worked."
        );
    } else {
        // If real LLM, expect success and no failure messages
        assert!(
            !stdout.contains("❌ Plan execution failed") && !stderr.contains("❌ Plan execution failed"),
            "Output should NOT contain explicit plan execution failure with real LLM"
        );
    }


    Ok(())
}
