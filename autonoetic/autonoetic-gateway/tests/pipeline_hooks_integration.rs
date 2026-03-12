use autonoetic_gateway::llm::{
    CompletionRequest, CompletionResponse, LlmDriver, Message, StopReason, TokenUsage,
};
use autonoetic_gateway::runtime::lifecycle::AgentExecutor;
use autonoetic_types::agent::{AdaptationHooks, AgentIdentity, AgentManifest, RuntimeDeclaration};
use std::sync::Arc;
use tempfile::tempdir;

struct MockLlm {
    last_req: std::sync::Mutex<Option<CompletionRequest>>,
}

#[async_trait::async_trait]
impl LlmDriver for MockLlm {
    async fn complete(&self, req: &CompletionRequest) -> anyhow::Result<CompletionResponse> {
        *self.last_req.lock().unwrap() = Some(req.clone());
        Ok(CompletionResponse {
            text: format!("Processed: {}", req.messages.last().unwrap().content),
            tool_calls: vec![],
            usage: TokenUsage::default(),
            stop_reason: StopReason::EndTurn,
        })
    }
}

#[tokio::test]
async fn test_pre_process_hook_transforms_input() -> anyhow::Result<()> {
    let temp = tempdir()?;
    let agent_dir = temp.path();

    // Create a pre-process hook script (python)
    let hook_path = agent_dir.join("pre_hook.py");
    std::fs::write(
        &hook_path,
        r#"
import sys
import json

req = json.load(sys.stdin)
# Transform the last message content
if req['messages']:
    req['messages'][-1]['content'] = "TRANSFORMED: " + req['messages'][-1]['content']

print(json.dumps(req))
"#,
    )?;

    let manifest = AgentManifest {
        version: "1.0".to_string(),
        runtime: RuntimeDeclaration {
            engine: "autonoetic".to_string(),
            gateway_version: "0.1.0".to_string(),
            sdk_version: "0.1.0".to_string(),
            runtime_type: "stateful".to_string(),
            sandbox: "bubblewrap".to_string(),
            runtime_lock: "runtime.lock".to_string(),
        },
        agent: AgentIdentity {
            id: "test-agent".to_string(),
            name: "test-agent".to_string(),
            description: "test".to_string(),
        },
        capabilities: vec![],
        llm_config: None,
        limits: None,
        background: None,
        disclosure: None,
        adaptation_hooks: Some(AdaptationHooks {
            pre_process: Some("python3 pre_hook.py".to_string()),
            post_process: None,
        }),
        io: None,
        middleware: None,
    };

    let mock_llm = Arc::new(MockLlm {
        last_req: std::sync::Mutex::new(None),
    });
    let mut executor = AgentExecutor::new(
        manifest.clone(),
        "instructions".to_string(),
        mock_llm.clone(),
        agent_dir.to_path_buf(),
        autonoetic_gateway::runtime::tools::default_registry(),
    )
    .with_adaptation_hooks(manifest.adaptation_hooks.clone().unwrap());

    let mut history = vec![Message::user("hello")];
    executor.execute_with_history(&mut history).await?;

    let last_req = mock_llm.last_req.lock().unwrap().take().unwrap();
    assert_eq!(
        last_req.messages.last().unwrap().content,
        "TRANSFORMED: hello"
    );

    Ok(())
}

#[tokio::test]
async fn test_pre_process_hook_skip_llm() -> anyhow::Result<()> {
    let temp = tempdir()?;
    let agent_dir = temp.path();

    // Create a pre-process hook script that signals skip_llm
    let hook_path = agent_dir.join("skip_hook.py");
    std::fs::write(
        &hook_path,
        r#"
import json
print(json.dumps({"skip_llm": True, "assistant_reply": "DET_REPLY"}))
"#,
    )?;

    let manifest = AgentManifest {
        version: "1.0".to_string(),
        runtime: RuntimeDeclaration {
            engine: "autonoetic".to_string(),
            gateway_version: "0.1.0".to_string(),
            sdk_version: "0.1.0".to_string(),
            runtime_type: "stateful".to_string(),
            sandbox: "bubblewrap".to_string(),
            runtime_lock: "runtime.lock".to_string(),
        },
        agent: AgentIdentity {
            id: "test-agent".to_string(),
            name: "test-agent".to_string(),
            description: "test".to_string(),
        },
        capabilities: vec![],
        llm_config: None,
        limits: None,
        background: None,
        disclosure: None,
        adaptation_hooks: Some(AdaptationHooks {
            pre_process: Some("python3 skip_hook.py".to_string()),
            post_process: None,
        }),
        io: None,
        middleware: None,
    };

    let mock_llm = Arc::new(MockLlm {
        last_req: std::sync::Mutex::new(None),
    });
    let mut executor = AgentExecutor::new(
        manifest.clone(),
        "instructions".to_string(),
        mock_llm.clone(),
        agent_dir.to_path_buf(),
        autonoetic_gateway::runtime::tools::default_registry(),
    )
    .with_adaptation_hooks(manifest.adaptation_hooks.clone().unwrap());

    let mut history = vec![Message::user("hello")];
    let reply = executor.execute_with_history(&mut history).await?;

    assert_eq!(reply, Some("DET_REPLY".to_string()));
    assert!(
        mock_llm.last_req.lock().unwrap().is_none(),
        "LLM should not have been called"
    );

    Ok(())
}

#[tokio::test]
async fn test_post_process_hook_transforms_output() -> anyhow::Result<()> {
    let temp = tempdir()?;
    let agent_dir = temp.path();

    // Create a post-process hook script (python)
    let hook_path = agent_dir.join("post_hook.py");
    std::fs::write(
        &hook_path,
        r#"
import sys
import json

resp = json.load(sys.stdin)
# Transform the text response
resp['text'] = "AFTER: " + resp['text']

print(json.dumps(resp))
"#,
    )?;

    let manifest = AgentManifest {
        version: "1.0".to_string(),
        runtime: RuntimeDeclaration {
            engine: "autonoetic".to_string(),
            gateway_version: "0.1.0".to_string(),
            sdk_version: "0.1.0".to_string(),
            runtime_type: "stateful".to_string(),
            sandbox: "bubblewrap".to_string(),
            runtime_lock: "runtime.lock".to_string(),
        },
        agent: AgentIdentity {
            id: "test-agent".to_string(),
            name: "test-agent".to_string(),
            description: "test".to_string(),
        },
        capabilities: vec![],
        llm_config: None,
        limits: None,
        background: None,
        disclosure: None,
        adaptation_hooks: Some(AdaptationHooks {
            pre_process: None,
            post_process: Some("python3 post_hook.py".to_string()),
        }),
        io: None,
        middleware: None,
    };

    let mock_llm = Arc::new(MockLlm {
        last_req: std::sync::Mutex::new(None),
    });
    let mut executor = AgentExecutor::new(
        manifest.clone(),
        "instructions".to_string(),
        mock_llm.clone(),
        agent_dir.to_path_buf(),
        autonoetic_gateway::runtime::tools::default_registry(),
    )
    .with_adaptation_hooks(manifest.adaptation_hooks.clone().unwrap());

    let mut history = vec![Message::user("hello")];
    let reply = executor.execute_with_history(&mut history).await?;

    assert_eq!(reply, Some("AFTER: Processed: hello".to_string()));

    Ok(())
}
