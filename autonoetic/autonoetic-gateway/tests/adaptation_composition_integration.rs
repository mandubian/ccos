use autonoetic_gateway::agent::repository::AgentRepository;
use autonoetic_gateway::llm::LlmDriver;
use autonoetic_gateway::runtime::lifecycle::AgentExecutor;
use autonoetic_types::agent::{AdaptationHooks, AgentManifest, AssetAction, AssetChange};
use autonoetic_types::capability::Capability;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use tempfile::tempdir;

#[derive(Debug)]
struct MockLlm {
    reply: String,
}

#[async_trait::async_trait]
impl LlmDriver for MockLlm {
    async fn complete(
        &self,
        _req: &autonoetic_gateway::llm::CompletionRequest,
    ) -> anyhow::Result<autonoetic_gateway::llm::CompletionResponse> {
        Ok(autonoetic_gateway::llm::CompletionResponse {
            text: self.reply.clone(),
            tool_calls: Vec::new(),
            stop_reason: autonoetic_gateway::llm::StopReason::EndTurn,
            usage: autonoetic_gateway::llm::TokenUsage {
                input_tokens: 10,
                output_tokens: 10,
            },
        })
    }
}

#[tokio::test]
async fn test_adaptation_composition_materializes_assets_in_sandbox() {
    let temp = tempdir().expect("tempdir should create");
    let agents_dir = temp.path().join("agents");
    let gateway_dir = agents_dir.join(".gateway");
    let adaptations_dir = gateway_dir.join("adaptations").join("test-agent");
    std::fs::create_dir_all(&adaptations_dir).expect("dirs should create");

    // 1. Create base agent
    let agent_dir = agents_dir.join("test-agent");
    std::fs::create_dir_all(agent_dir.join("state")).expect("agent dir should create");
    let skill_md = r#"---
name: "test-agent"
description: "Base agent"
metadata:
  autonoetic:
    version: "1.0"
    runtime:
      engine: "autonoetic"
      gateway_version: "0.1.0"
      sdk_version: "0.1.0"
      type: "stateful"
      sandbox: "bubblewrap"
      runtime_lock: "runtime.lock"
    agent:
      id: "test-agent"
      name: "test-agent"
      description: "Base agent"
    capabilities: []
---
# Base Agent
"#;
    std::fs::write(agent_dir.join("SKILL.md"), skill_md).expect("skill.md should write");

    // 2. Create adaptation overlay
    let adaptation_id = "v1-overlay";
    let overlay_json = serde_json::json!({
        "adaptation_id": adaptation_id,
        "behavior_overlay": "Adapted behavior.",
        "asset_changes": [
            {
                "path": "skills/virtual_tool.sh",
                "content": "#!/bin/sh\necho 'hello from virtual tool'",
                "action": "create"
            }
        ],
        "adaptation_hooks": {
            "pre_process": "cat"
        },
        "metadata": {
            "adapted_at": "2026-03-12T00:00:00Z"
        }
    });
    std::fs::write(
        adaptations_dir.join(format!("{}.json", adaptation_id)),
        serde_json::to_string_pretty(&overlay_json).expect("json should serialize"),
    )
    .expect("overlay should write");

    // 3. Load agent with adaptation
    let repo = AgentRepository::new(agents_dir);
    let loaded = repo
        .get_sync_with_adaptations("test-agent", Some(&[adaptation_id.to_string()]))
        .expect("should load adapted agent");

    assert_eq!(loaded.adaptation_assets.len(), 1);
    assert_eq!(loaded.adaptation_assets[0].path, "skills/virtual_tool.sh");
    assert!(loaded.adaptation_hooks.pre_process.is_some());

    // 4. Execute a turn that uses the virtual asset via a hook or standard tool
    // We'll mock the LLM to just return a simple reply
    let llm = Arc::new(MockLlm {
        reply: "Observed virtual asset".to_string(),
    });

    let mut executor = AgentExecutor::new(
        loaded.manifest,
        loaded.instructions,
        llm,
        loaded.dir,
        autonoetic_gateway::runtime::tools::default_registry(),
    )
    .with_adaptation_hooks(loaded.adaptation_hooks)
    .with_adaptation_assets(loaded.adaptation_assets)
    .with_gateway_dir(gateway_dir)
    .with_session_id("session-1");

    // We'll test the projection directly first
    let projected_dir = executor
        .project_adaptation_assets("session-1")
        .expect("projection should succeed");

    let virtual_tool_path = projected_dir.join("skills/virtual_tool.sh");
    assert!(virtual_tool_path.exists());
    let content = std::fs::read_to_string(virtual_tool_path).expect("should read virtual tool");
    assert_eq!(content, "#!/bin/sh\necho 'hello from virtual tool'");

    // Verify symlinks work
    let skill_md_path = projected_dir.join("SKILL.md");
    assert!(skill_md_path.exists());
    assert!(skill_md_path.is_symlink());

    // 5. Execute with history
    let mut history = vec![autonoetic_gateway::llm::Message::user("test")];
    let result = executor
        .execute_with_history(&mut history)
        .await
        .expect("execution should succeed");

    assert!(result.is_some());
    assert_eq!(result.unwrap(), "Observed virtual asset");
}
