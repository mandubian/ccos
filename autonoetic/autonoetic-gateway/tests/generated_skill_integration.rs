use autonoetic_gateway::execution::GatewayExecutionService;
use autonoetic_gateway::runtime::lifecycle::AgentExecutor;
use autonoetic_gateway::runtime::parser::SkillParser;
use std::path::Path;
use std::sync::Arc;
mod support;

use support::agents::{install_generated_skill_learner_agent, APPROVED_REUSE_MATH_AGENT_SKILL};
use support::{
    approve_pending_request_and_tick, require_single_pending_approval, spawn_gateway_server,
    EnvGuard, JsonRpcClient, OpenAiStub, TestWorkspace,
};

// ---------------------------------------------------------------------------
// Artifact loader (simulates hot-loading a skill as a new agent)
// ---------------------------------------------------------------------------

/// Once the system writes an approved `skills/X.md` into the learning agent's directory,
/// this copies it to `agents/<X>_agent/SKILL.md` so the gateway can parse it.
fn artifact_loader(learning_agent_dir: &Path, gateway_agents_dir: &Path) {
    let skills_dir = learning_agent_dir.join("skills");
    if !skills_dir.exists() {
        return;
    }
    for entry in std::fs::read_dir(&skills_dir).unwrap() {
        let entry = entry.unwrap();
        let path = entry.path();
        if path.is_file() && path.extension().unwrap_or_default() == "md" {
            let stem = path.file_stem().unwrap().to_str().unwrap();
            let agent_id = format!("{}_agent", stem); // e.g. "math_agent"
            let target_dir = gateway_agents_dir.join(&agent_id);
            std::fs::create_dir_all(&target_dir).unwrap();
            let content = std::fs::read_to_string(&path).unwrap();
            std::fs::write(target_dir.join("SKILL.md"), content).unwrap();
        }
    }
}

fn count_occurrences(haystack: &str, needle: &str) -> usize {
    haystack.matches(needle).count()
}

// ---------------------------------------------------------------------------
// Test B: generate skill → approval → approved execution
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_generated_skill_approval_and_execution() {
    let stub = OpenAiStub::spawn(|_, body_json| async move {
        let messages = body_json["messages"].as_array().cloned().unwrap_or_default();
        let latest_user_message = messages
            .iter()
            .rev()
            .find_map(|message| {
                if message["role"].as_str() == Some("user") {
                    message["content"].as_str()
                } else {
                    None
                }
            })
            .unwrap_or("");
        let is_tool_result_turn = messages
            .iter()
            .any(|message| message["role"].as_str() == Some("tool"));
        let is_learner_phase1 = latest_user_message.contains("Draft content");
        let is_learner_phase2 = latest_user_message.contains("PoC Evidence:");
        let is_poc = latest_user_message.contains("PoC Execution");
        let is_math_agent_reuse =
            latest_user_message.contains("math_agent") && !is_learner_phase1 && !is_learner_phase2;

        if is_learner_phase1 && !is_tool_result_turn {
            serde_json::json!({
                "id": "chatcmpl-learner-1",
                "object": "chat.completion",
                "created": 1,
                "model": "gpt-4o",
                "choices": [{
                    "index": 0,
                    "message": {
                        "role": "assistant",
                        "content": format!("CONTENT_START\n{}\nCONTENT_END", APPROVED_REUSE_MATH_AGENT_SKILL)
                    },
                    "finish_reason": "stop"
                }],
                "usage": {"prompt_tokens": 1, "completion_tokens": 1, "total_tokens": 2}
            })
        } else if is_learner_phase2 && !is_tool_result_turn {
            let evidence_ref = if latest_user_message.contains("poc_session") {
                "poc_session"
            } else {
                "unknown"
            };
            let draft_args = serde_json::json!({
                "path": "skills/math.md",
                "content": APPROVED_REUSE_MATH_AGENT_SKILL,
                "evidence_ref": evidence_ref
            })
            .to_string();
            serde_json::json!({
                "id": "chatcmpl-learner-2",
                "object": "chat.completion",
                "created": 2,
                "model": "gpt-4o",
                "choices": [{
                    "index": 0,
                    "message": {
                        "role": "assistant",
                        "tool_calls": [{
                            "id": "call_draft",
                            "type": "function",
                            "function": {
                                "name": "skill.draft",
                                "arguments": draft_args
                            }
                        }]
                    },
                    "finish_reason": "tool_calls"
                }],
                "usage": {"prompt_tokens": 1, "completion_tokens": 1, "total_tokens": 2}
            })
        } else if is_learner_phase2 && is_tool_result_turn {
            serde_json::json!({
                "id": "chatcmpl-learner-3",
                "object": "chat.completion",
                "created": 3,
                "model": "gpt-4o",
                "choices": [{
                    "index": 0,
                    "message": { "role": "assistant", "content": "Skill submitted with evidence." },
                    "finish_reason": "stop"
                }],
                "usage": {"prompt_tokens": 1, "completion_tokens": 1, "total_tokens": 2}
            })
        } else if is_poc {
            serde_json::json!({
                "id": "chatcmpl-poc",
                "object": "chat.completion",
                "created": 10,
                "model": "gpt-4o",
                "choices": [{
                    "index": 0,
                    "message": { "role": "assistant", "content": "42" },
                    "finish_reason": "stop"
                }],
                "usage": {"prompt_tokens": 1, "completion_tokens": 1, "total_tokens": 2}
            })
        } else if is_math_agent_reuse {
            serde_json::json!({
                "id": "chatcmpl-reuse",
                "object": "chat.completion",
                "created": 20,
                "model": "gpt-4o",
                "choices": [{
                    "index": 0,
                    "message": { "role": "assistant", "content": "reused math agent successfully: 42" },
                    "finish_reason": "stop"
                }],
                "usage": {"prompt_tokens": 1, "completion_tokens": 1, "total_tokens": 2}
            })
        } else {
            serde_json::json!({
                "id": "chatcmpl-default",
                "object": "chat.completion",
                "created": 0,
                "model": "gpt-4o",
                "choices": [{
                    "index": 0,
                    "message": { "role": "assistant", "content": "Default response" },
                    "finish_reason": "stop"
                }],
                "usage": {"prompt_tokens": 1, "completion_tokens": 1, "total_tokens": 2}
            })
        }
    })
    .await
    .unwrap();
    let _g1 = EnvGuard::set("AUTONOETIC_LLM_BASE_URL", stub.completion_url());
    let _g2 = EnvGuard::set("OPENAI_API_KEY", "test-key");

    // --- Directory setup ---
    let workspace = TestWorkspace::new().unwrap();
    let agents_dir = workspace.agents_dir.clone();
    let learner_id = "learner";
    let learner_dir = agents_dir.join(learner_id);
    install_generated_skill_learner_agent(&learner_dir, learner_id).unwrap();

    // --- Gateway setup ---
    let config = autonoetic_types::config::GatewayConfig {
        port: 0,
        ofp_port: 0,
        agents_dir: agents_dir.clone(),
        max_concurrent_spawns: 4,
        max_pending_spawns_per_agent: 10,
        background_scheduler_enabled: true,
        background_min_interval_secs: 1,
        ..Default::default()
    };

    let execution_service = Arc::new(GatewayExecutionService::new(config.clone()));
    let (listen_addr, server_task) = spawn_gateway_server(config.clone()).await.unwrap();
    let mut client = JsonRpcClient::connect(listen_addr).await.unwrap();

    let session_id = "session-test-b";

    // --- Step 1: Ingest request to draft skill content ---
    let resp1 = client
        .event_ingest(
            "1",
            learner_id,
            session_id,
            "chat",
            "Draft content for a math skill.",
            None,
        )
        .await
        .unwrap();
    let content_reply = resp1
        .result
        .as_ref()
        .and_then(|r| r.get("assistant_reply"))
        .and_then(|v| v.as_str())
        .unwrap();
    assert!(content_reply.contains("CONTENT_START"));

    // --- Step 2: Extract content and execute as PoC ---
    let skill_content = content_reply
        .split("CONTENT_START\n")
        .nth(1)
        .and_then(|s| s.split("\nCONTENT_END").next())
        .expect("Should extract skill content from learner reply");

    let poc_dir = workspace.path().join("poc_temp");
    std::fs::create_dir_all(&poc_dir).expect("poc dir should create");
    let (poc_manifest, poc_instructions) = SkillParser::parse(skill_content)
        .expect("drafted skill content should be parseable by SkillParser");

    let poc_driver = autonoetic_gateway::llm::build_driver(
        poc_manifest.llm_config.clone().unwrap(),
        reqwest::Client::new(),
    )
    .expect("poc driver should build");

    // Enable evidence capture for the PoC
    let _g3 = EnvGuard::set("AUTONOETIC_EVIDENCE_MODE", "full");

    let mut poc_executor = AgentExecutor::new(
        poc_manifest,
        poc_instructions,
        poc_driver,
        poc_dir.clone(),
        autonoetic_gateway::runtime::tools::default_registry(),
    )
    .with_initial_user_message("PoC Execution".to_string())
    .with_session_id("poc_session".to_string());

    let mut poc_history = vec![autonoetic_gateway::llm::Message::user(
        "PoC Execution".to_string(),
    )];
    let poc_result = poc_executor
        .execute_with_history(&mut poc_history)
        .await
        .expect("PoC execution failed");

    assert_eq!(
        poc_result.unwrap(),
        "42",
        "PoC execution returned wrong value"
    );

    // Verify evidence was captured for the PoC
    let evidence_dir = poc_dir.join("history").join("evidence").join("poc_session");
    assert!(
        evidence_dir.exists(),
        "PoC evidence directory should exist at {:?}",
        evidence_dir
    );
    let poc_history_file = poc_dir.join("history").join("causal_chain.jsonl");
    let poc_history_log = std::fs::read_to_string(&poc_history_file).unwrap_or_default();
    assert!(
        poc_history_log.contains("\"session_id\":\"poc_session\""),
        "Expected poc_session in PoC causal chain"
    );
    assert!(
        poc_history_log.contains("\"category\":\"lifecycle\",\"action\":\"wake\""),
        "Expected lifecycle wake entry in PoC causal chain"
    );
    assert!(
        poc_history_log.contains("\"category\":\"llm\",\"action\":\"completion\""),
        "Expected llm completion entry in PoC causal chain"
    );

    // --- Step 3: Learner submits the draft with evidence ---
    // The evidence_ref is the session_id we used for the PoC
    let evidence_ref = "poc_session".to_string();

    let resp_draft = client
        .event_ingest(
            "draft-with-evidence",
            learner_id,
            session_id,
            "chat",
            &format!("PoC Evidence: {}", evidence_ref),
            None,
        )
        .await
        .unwrap();
    assert!(
        resp_draft.error.is_none(),
        "Draft submission failed: {:?}",
        resp_draft.error
    );

    // Tick scheduler to promote to ApprovalRequest
    let r_state = std::fs::read_to_string(learner_dir.join("state").join("reevaluation.json")).unwrap();
    println!("R_STATE: {}", r_state);
    let draft_request = require_single_pending_approval(execution_service.clone(), &config)
        .await
        .unwrap();
    assert_eq!(
        draft_request.evidence_ref.as_deref(),
        Some(evidence_ref.as_str()),
        "Evidence ref should be attached to approval request"
    );

    // --- Step 3.5: Verify pre-approval blocking ---
    // Try to install and use the skill before approval.
    artifact_loader(&learner_dir, &agents_dir);

    let resp_blocked = client
        .event_ingest(
            "blocked-ingest",
            "math_agent",
            "session-blocked",
            "chat",
            "this should fail",
            None,
        )
        .await
        .unwrap();
    assert!(
        resp_blocked.error.is_some(),
        "Ingest to math_agent MUST fail before approval. Result was: {:?}",
        resp_blocked.result
    );
    let error_msg = resp_blocked.error.unwrap().message;
    assert!(
        error_msg.contains("not found") || error_msg.contains("Permission Denied"),
        "Error message should indicate missing agent or permission denied, got: {}",
        error_msg
    );

    // --- Step 4: Programmatically approve ---
    approve_pending_request_and_tick(
        execution_service.clone(),
        &config,
        &draft_request,
        "admin",
        Some("Looks good".to_string()),
    )
    .await
    .unwrap();
    assert!(
        learner_dir.join("skills").join("math.md").exists(),
        "Approved skill file should be written to learner/skills/math.md"
    );

    // --- Step 6: artifact_loader installs the skill as a new agent ---
    artifact_loader(&learner_dir, &agents_dir);
    assert!(
        agents_dir.join("math_agent").join("SKILL.md").exists(),
        "artifact_loader should install math_agent/SKILL.md"
    );

    // --- Step 7: Verify gateway can call the newly loaded agent ---
    let resp2 = client
        .event_ingest(
            "2",
            "math_agent",
            "session-test-b-reuse",
            "chat",
            "math_agent",
            None,
        )
        .await
        .unwrap();
    let text = resp2
        .result
        .as_ref()
        .and_then(|r| r.get("assistant_reply"))
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    assert!(
        text.contains("reused math agent successfully: 42"),
        "Expected math_agent reply but got: {:?}",
        text
    );

    // --- Step 8: Assert causal lineage events ---
    let gateway_history_file = agents_dir
        .join(".gateway")
        .join("history")
        .join("causal_chain.jsonl");
    let gateway_history = std::fs::read_to_string(&gateway_history_file).unwrap_or_default();

    // Check for specific causal actions
    assert!(
        gateway_history.contains("background.approval.requested"),
        "Expected background.approval.requested in causal chain"
    );
    assert_eq!(
        count_occurrences(&gateway_history, "background.approval.requested"),
        1,
        "Expected exactly one approval requested event"
    );
    assert!(
        gateway_history.contains("background.approval.completed"),
        "Expected background.approval.completed in causal chain"
    );
    assert_eq!(
        count_occurrences(&gateway_history, "background.approval.completed"),
        1,
        "Expected exactly one approval completed event"
    );

    // Verify session ID propagation for the learner
    assert!(
        gateway_history.contains(session_id),
        "Expected session_id {} in learner causal chain",
        session_id
    );

    // Verify session ID propagation for the reused math agent
    assert!(
        gateway_history.contains("session-test-b-reuse"),
        "Expected session-test-b-reuse in math_agent causal chain"
    );

    // Verify ingress events were captured
    assert!(
        gateway_history.contains("\"action\":\"event.ingest.completed\""),
        "Expected event.ingest.completed action in causal chain"
    );

    // Expansion: check for skill.draft tool call in learner logs
    let learner_history_file = learner_dir.join("history").join("causal_chain.jsonl");
    let learner_history = std::fs::read_to_string(&learner_history_file).unwrap_or_default();
    assert!(
        learner_history.contains("\"tool_name\":\"skill.draft\""),
        "Expected skill.draft tool call in learner causal chain"
    );
    assert_eq!(
        count_occurrences(
            &learner_history,
            "\"category\":\"tool_invoke\",\"action\":\"requested\""
        ),
        1,
        "Expected exactly one tool invocation request in learner history so approved reuse does not regenerate the artifact"
    );
    assert_eq!(
        count_occurrences(
            &learner_history,
            "\"category\":\"tool_invoke\",\"action\":\"completed\""
        ),
        1,
        "Expected exactly one completed tool invocation in learner history"
    );

    // Verify evidence ref in learner log
    assert!(
        learner_history.contains("poc_session"),
        "Expected evidence_ref 'poc_session' in learner causal chain"
    );

    // Verify blocked session failure in learner logs
    assert!(
        gateway_history.contains("session-blocked"),
        "Expected session-blocked in gateway causal chain"
    );
    assert!(
        gateway_history.contains("event.ingest.failed"),
        "Expected event.ingest.failed for session-blocked"
    );

    // Verify math_agent reuse inherited its ancestor/approval context (implied by success)
    let math_history_file = agents_dir
        .join("math_agent")
        .join("history")
        .join("causal_chain.jsonl");
    let math_history = std::fs::read_to_string(&math_history_file).unwrap_or_default();
    assert!(
        math_history.contains("session-test-b-reuse"),
        "Expected reuse session in math_agent causal chain"
    );
    assert!(
        math_history.contains("\"category\":\"llm\",\"action\":\"completion\""),
        "Expected approved reuse llm completion in math_agent causal chain"
    );

    server_task.abort();
}
