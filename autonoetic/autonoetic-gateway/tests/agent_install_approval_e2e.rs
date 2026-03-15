//! End-to-end test for agent.install with approval flow.
//!
//! Tests the full lifecycle:
//! 1. Specialized_builder calls agent.install with high-risk capability (NetConnect)
//! 2. Gateway returns approval_required (agent NOT installed)
//! 3. Programmatically approve the pending request
//! 4. Retry with install_approval_ref → agent IS installed
//! 5. Verify the newly installed agent files exist

mod support;

use autonoetic_gateway::runtime::tools::default_registry;
use autonoetic_gateway::policy::PolicyEngine;
use autonoetic_types::agent::{AgentIdentity, AgentManifest, RuntimeDeclaration};
use autonoetic_types::capability::Capability;
use autonoetic_types::config::{AgentInstallApprovalPolicy, GatewayConfig};
use tempfile::tempdir;

fn evolution_manifest() -> AgentManifest {
    AgentManifest {
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
            id: "specialized_builder.default".to_string(),
            name: "specialized_builder.default".to_string(),
            description: "Builder".to_string(),
        },
        capabilities: vec![Capability::AgentSpawn { max_children: 10 }],
        llm_config: None,
        limits: None,
        background: None,
        disclosure: None,
        io: None,
        middleware: None,
        execution_mode: Default::default(),
        script_entry: None,
        gateway_url: None,
        gateway_token: None,
    }
}

/// Full approval flow via direct tool registry calls:
/// 1. Call agent.install with high-risk NetConnect capability
/// 2. Gateway returns approval_required (agent NOT installed)
/// 3. Programmatically approve the request
/// 4. Retry with install_approval_ref → agent IS installed
/// 5. Verify SKILL.md, script files, and payload cleanup
#[tokio::test]
async fn test_agent_install_full_approval_flow() {
    let manifest = evolution_manifest();
    let policy = PolicyEngine::new(manifest.clone());
    let temp = tempdir().expect("tempdir should create");
    let agents_dir = temp.path().join("agents");
    let builder_dir = agents_dir.join("specialized_builder.default");
    std::fs::create_dir_all(&builder_dir).expect("builder dir should create");

    let config = GatewayConfig {
        agents_dir: agents_dir.clone(),
        agent_install_approval_policy: AgentInstallApprovalPolicy::Always,
        ..Default::default()
    };

    let registry = default_registry();

    // --- Step 1: Call agent.install with high-risk capability (NetConnect) ---
    let install_args = serde_json::json!({
        "agent_id": "weather.fetcher",
        "name": "Weather Fetcher",
        "description": "Fetches weather from Open-Meteo API",
        "instructions": "---\nname: weather.fetcher\ndescription: Fetches weather\nexecution_mode: script\nscript_entry: main.py\n---\n# Weather Fetcher\nFetches weather data for given coordinates.",
        "capabilities": [
            { "type": "NetConnect", "hosts": ["api.open-meteo.com"] }
        ],
        "files": [
            { "path": "main.py", "content": "import json\nprint(json.dumps({'temp': 22}))\n" }
        ],
        "promotion_gate": { "evaluator_pass": true, "auditor_pass": true }
    });

    let result = registry
        .execute("agent.install", &manifest, &policy, &builder_dir, None,
            &serde_json::to_string(&install_args).unwrap(),
            Some("session-approval-test"), None, Some(&config))
        .expect("install should return approval request, not error");

    // --- Step 2: Verify approval_required response ---
    let parsed: serde_json::Value = serde_json::from_str(&result).unwrap();
    assert_eq!(parsed.get("ok").and_then(|v| v.as_bool()), Some(false));
    assert_eq!(parsed.get("approval_required").and_then(|v| v.as_bool()), Some(true));

    let request_id = parsed.get("request_id").and_then(|v| v.as_str()).unwrap();
    assert!(parsed.get("message").and_then(|v| v.as_str()).unwrap_or("").contains("approval"));

    // --- Step 3: Verify agent was NOT installed ---
    let child_dir = agents_dir.join("weather.fetcher");
    assert!(!child_dir.exists(), "agent should not be installed while approval is pending");

    // --- Step 4: Verify payload was stored ---
    let payload_path = agents_dir
        .join(".gateway").join("scheduler").join("approvals").join("pending")
        .join(format!("{}_payload.json", request_id));
    assert!(payload_path.exists(), "payload file should exist");

    // --- Step 5: Programmatically approve ---
    let approved_dir = agents_dir
        .join(".gateway").join("scheduler").join("approvals").join("approved");
    std::fs::create_dir_all(&approved_dir).unwrap();

    std::fs::write(
        approved_dir.join(format!("{}.json", request_id)),
        serde_json::to_string(&serde_json::json!({
            "request_id": request_id,
            "agent_id": "specialized_builder.default",
            "session_id": "session-approval-test",
            "action": {
                "type": "agent_install",
                "agent_id": "weather.fetcher",
                "summary": "Weather fetcher with NetConnect to api.open-meteo.com",
                "requested_by_agent_id": "specialized_builder.default",
                "install_fingerprint": "test_fingerprint"
            },
            "status": "approved",
            "decided_at": chrono::Utc::now().to_rfc3339(),
            "decided_by": "test-admin"
        })).unwrap(),
    ).unwrap();

    // --- Step 6: Retry with install_approval_ref ---
    let retry_args = serde_json::json!({
        "agent_id": "weather.fetcher",
        "instructions": "---\nname: weather.fetcher\ndescription: Fetches weather\nexecution_mode: script\nscript_entry: main.py\n---\n# Weather Fetcher\nFetches weather data for given coordinates.",
        "capabilities": [
            { "type": "NetConnect", "hosts": ["api.open-meteo.com"] }
        ],
        "files": [
            { "path": "main.py", "content": "import json\nprint(json.dumps({'temp': 22}))\n" }
        ],
        "promotion_gate": {
            "evaluator_pass": true,
            "auditor_pass": true,
            "install_approval_ref": request_id
        }
    });

    let retry_result = registry
        .execute("agent.install", &manifest, &policy, &builder_dir, None,
            &serde_json::to_string(&retry_args).unwrap(),
            Some("session-approval-test"), None, Some(&config))
        .expect("retry should succeed with stored payload");

    let retry_parsed: serde_json::Value = serde_json::from_str(&retry_result).unwrap();
    assert_eq!(retry_parsed.get("ok").and_then(|v| v.as_bool()), Some(true));
    assert_eq!(retry_parsed.get("status").and_then(|v| v.as_str()), Some("agent_installed"));

    // --- Step 7: Verify agent was installed ---
    assert!(child_dir.exists(), "weather.fetcher agent should be installed");
    assert!(child_dir.join("SKILL.md").exists(), "SKILL.md should exist");
    assert!(child_dir.join("main.py").exists(), "main.py should exist");

    // --- Step 8: Verify SKILL.md content ---
    let skill = std::fs::read_to_string(child_dir.join("SKILL.md")).unwrap();
    assert!(skill.contains("weather.fetcher"), "SKILL.md should contain agent name");
    assert!(skill.contains("script"), "SKILL.md should contain script mode");

    // --- Step 9: Verify payload cleanup ---
    assert!(!payload_path.exists(), "payload file should be cleaned up after successful install");
}

/// Verify that invalid approval_ref is rejected.
#[tokio::test]
async fn test_agent_install_rejects_invalid_approval_ref() {
    let manifest = evolution_manifest();
    let policy = PolicyEngine::new(manifest.clone());
    let temp = tempdir().expect("tempdir should create");
    let agents_dir = temp.path().join("agents");
    let builder_dir = agents_dir.join("specialized_builder.default");
    std::fs::create_dir_all(&builder_dir).unwrap();

    let config = GatewayConfig {
        agents_dir: agents_dir.clone(),
        agent_install_approval_policy: AgentInstallApprovalPolicy::Always,
        ..Default::default()
    };

    let registry = default_registry();
    let args = serde_json::json!({
        "agent_id": "fake.agent",
        "instructions": "# Fake Agent",
        "capabilities": [{ "type": "NetConnect", "hosts": ["api.example.com"] }],
        "promotion_gate": {
            "evaluator_pass": true,
            "auditor_pass": true,
            "install_approval_ref": "non-existent-request-id"
        }
    });

    let result = registry
        .execute("agent.install", &manifest, &policy, &builder_dir, None,
            &serde_json::to_string(&args).unwrap(),
            None, None, Some(&config));

    // Invalid approval_ref returns an error (not a JSON response)
    assert!(result.is_err(), "invalid approval_ref should return error");
    let err_msg = result.unwrap_err().to_string();
    assert!(
        err_msg.contains("not found") || err_msg.contains("not approved") || err_msg.contains("approval"),
        "Error should mention approval, got: {}",
        err_msg
    );
    assert!(!agents_dir.join("fake.agent").exists(), "agent should not be installed");
}

/// Verify approval policy modes: Always requires approval, Never skips it.
#[tokio::test]
async fn test_agent_install_approval_policies() {
    let manifest = evolution_manifest();
    let policy = PolicyEngine::new(manifest.clone());
    let temp = tempdir().expect("tempdir should create");
    let registry = default_registry();

    let install_args = serde_json::json!({
        "agent_id": "test.worker",
        "instructions": "# Test Worker",
        "capabilities": [{ "type": "NetConnect", "hosts": ["api.example.com"] }],
        "promotion_gate": { "evaluator_pass": true, "auditor_pass": true }
    });

    // Policy: Always → should require approval
    let agents_dir_always = temp.path().join("agents_always");
    let builder_dir_always = agents_dir_always.join("specialized_builder.default");
    std::fs::create_dir_all(&builder_dir_always).unwrap();
    let config_always = GatewayConfig {
        agents_dir: agents_dir_always.clone(),
        agent_install_approval_policy: AgentInstallApprovalPolicy::Always,
        ..Default::default()
    };
    let result_always = registry
        .execute("agent.install", &manifest, &policy, &builder_dir_always, None,
            &serde_json::to_string(&install_args).unwrap(),
            None, None, Some(&config_always))
        .unwrap();
    let parsed: serde_json::Value = serde_json::from_str(&result_always).unwrap();
    assert_eq!(parsed.get("approval_required").and_then(|v| v.as_bool()), Some(true),
        "Always policy should require approval");

    // Policy: Never → should install directly
    let agents_dir_never = temp.path().join("agents_never");
    let builder_dir_never = agents_dir_never.join("specialized_builder.default");
    std::fs::create_dir_all(&builder_dir_never).unwrap();
    let config_never = GatewayConfig {
        agents_dir: agents_dir_never.clone(),
        agent_install_approval_policy: AgentInstallApprovalPolicy::Never,
        ..Default::default()
    };
    let result_never = registry
        .execute("agent.install", &manifest, &policy, &builder_dir_never, None,
            &serde_json::to_string(&install_args).unwrap(),
            None, None, Some(&config_never))
        .unwrap();
    let parsed2: serde_json::Value = serde_json::from_str(&result_never).unwrap();
    assert_eq!(parsed2.get("ok").and_then(|v| v.as_bool()), Some(true),
        "Never policy should install directly");
    assert!(agents_dir_never.join("test.worker").exists(),
        "test.worker should be installed with Never policy");
}
