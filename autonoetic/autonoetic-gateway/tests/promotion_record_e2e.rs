//! End-to-end test for content-linked promotion gate flow.
//!
//! Tests the complete lifecycle:
//! 1. Coder writes content → content_handle = sha256:...
//! 2. Evaluator validates content → calls promotion.record(pass=true)
//! 3. Auditor audits content → calls promotion.record(pass=true)
//! 4. specialized_builder calls agent.install with source_content_handle
//! 5. Gateway verifies promotion records → agent IS installed

mod support;

use autonoetic_gateway::policy::PolicyEngine;
use autonoetic_gateway::runtime::content_store::ContentStore;
use autonoetic_gateway::runtime::promotion_store::PromotionStore;
use autonoetic_gateway::runtime::tools::default_registry;
use autonoetic_types::agent::{AgentIdentity, AgentManifest, RuntimeDeclaration};
use autonoetic_types::capability::Capability;
use autonoetic_types::config::{AgentInstallApprovalPolicy, GatewayConfig};
use autonoetic_types::promotion::PromotionRole;
use std::path::{Path, PathBuf};
use tempfile::tempdir;

fn build_test_artifact(base_dir: &Path, files: &[(&str, &str)]) -> (String, PathBuf) {
    let gateway_dir = base_dir.join(".gateway");
    std::fs::create_dir_all(&gateway_dir).unwrap();
    let content_store = ContentStore::new(&gateway_dir).unwrap();
    let artifact_store =
        autonoetic_gateway::artifact_store::ArtifactStore::new(&gateway_dir).unwrap();
    let session_id = "test-session";
    let mut input_names = Vec::new();
    for (path, content) in files {
        let handle = content_store.write(content.as_bytes()).unwrap();
        content_store
            .register_name(session_id, path, &handle)
            .unwrap();
        input_names.push(path.to_string());
    }
    let bundle = artifact_store
        .build(&input_names, None, session_id)
        .unwrap();
    let promotion_store = PromotionStore::new(&gateway_dir).unwrap();
    let _ = promotion_store.record_promotion(
        bundle.artifact_id.clone(),
        Some(bundle.digest.clone()),
        PromotionRole::Evaluator,
        "evaluator.default",
        true,
        vec![],
        Some("Test auto-pass".to_string()),
    );
    let _ = promotion_store.record_promotion(
        bundle.artifact_id.clone(),
        Some(bundle.digest.clone()),
        PromotionRole::Auditor,
        "auditor.default",
        true,
        vec![],
        Some("Test auto-pass".to_string()),
    );
    (bundle.artifact_id, gateway_dir)
}

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

fn evaluator_manifest() -> AgentManifest {
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
            id: "evaluator.default".to_string(),
            name: "evaluator.default".to_string(),
            description: "Evaluator".to_string(),
        },
        capabilities: vec![Capability::SandboxFunctions {
            allowed: vec!["sandbox.".to_string(), "content.".to_string()],
        }],
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

fn auditor_manifest() -> AgentManifest {
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
            id: "auditor.default".to_string(),
            name: "auditor.default".to_string(),
            description: "Auditor".to_string(),
        },
        capabilities: vec![Capability::SandboxFunctions {
            allowed: vec!["content.".to_string()],
        }],
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

fn promotion_gate_with_evidence(
    declared_capabilities: &[&str],
    remote_access_detected: bool,
    source_content_handle: Option<&str>,
) -> serde_json::Value {
    let mut gate = serde_json::json!({
        "evaluator_pass": true,
        "auditor_pass": true,
        "security_analysis": {
            "passed": true,
            "threats_detected": [],
            "remote_access_detected": remote_access_detected
        },
        "capability_analysis": {
            "inferred_capabilities": declared_capabilities,
            "missing_capabilities": [],
            "declared_capabilities": declared_capabilities,
            "analysis_passed": true
        }
    });
    if let Some(handle) = source_content_handle {
        gate["source_content_handle"] = serde_json::Value::String(handle.to_string());
    }
    gate
}

/// Full promotion flow:
/// 1. Write content to content store (simulates coder producing files)
/// 2. Evaluator records promotion (simulates evaluator.default calling promotion.record)
/// 3. Auditor records promotion (simulates auditor.default calling promotion.record)
/// 4. Specialized_builder installs agent with source_content_handle
/// 5. Gateway verifies promotion records and installs the agent
#[tokio::test]
async fn test_promotion_record_full_pass_flow() {
    let temp = tempdir().expect("tempdir should create");
    let agents_dir = temp.path().join("agents");
    let builder_dir = agents_dir.join("specialized_builder.default");
    std::fs::create_dir_all(&builder_dir).expect("builder dir should create");

    let script_content = b"import json\nprint(json.dumps({'temp': 22}))\n";
    let (artifact_id, gateway_dir) = build_test_artifact(
        temp.path(),
        &[("main.py", &String::from_utf8_lossy(script_content))],
    );

    let config = GatewayConfig {
        agents_dir: agents_dir.clone(),
        agent_install_approval_policy: AgentInstallApprovalPolicy::Never,
        ..Default::default()
    };

    // --- Step 1: Coder writes content to content store ---
    let store = ContentStore::new(&gateway_dir).expect("content store should create");
    let content_handle = store.write(script_content).expect("content should write");
    assert!(content_handle.starts_with("sha256:"));
    println!("Content handle: {}", content_handle);

    // --- Step 2: Evaluator records promotion (pass=true) ---
    let eval_manifest = evaluator_manifest();
    let eval_policy = PolicyEngine::new(eval_manifest.clone());
    let registry = default_registry();

    let eval_args = serde_json::json!({
        "artifact_id": artifact_id,
        "role": "evaluator",
        "pass": true,
        "findings": [],
        "summary": "All tests passed"
    });

    let eval_result = registry
        .execute(
            "promotion.record",
            &eval_manifest,
            &eval_policy,
            &builder_dir,
            Some(&gateway_dir),
            &serde_json::to_string(&eval_args).unwrap(),
            Some("session-eval-test"),
            None,
            Some(&config),
        )
        .expect("evaluator promotion.record should succeed");

    let eval_parsed: serde_json::Value = serde_json::from_str(&eval_result).unwrap();
    assert_eq!(eval_parsed.get("ok").and_then(|v| v.as_bool()), Some(true));

    // --- Step 3: Auditor records promotion (pass=true) ---
    let audit_manifest = auditor_manifest();
    let audit_policy = PolicyEngine::new(audit_manifest.clone());

    let audit_args = serde_json::json!({
        "artifact_id": artifact_id,
        "role": "auditor",
        "pass": true,
        "findings": [],
        "summary": "Security audit passed"
    });

    let audit_result = registry
        .execute(
            "promotion.record",
            &audit_manifest,
            &audit_policy,
            &builder_dir,
            Some(&gateway_dir),
            &serde_json::to_string(&audit_args).unwrap(),
            Some("session-audit-test"),
            None,
            Some(&config),
        )
        .expect("auditor promotion.record should succeed");

    let audit_parsed: serde_json::Value = serde_json::from_str(&audit_result).unwrap();
    assert_eq!(audit_parsed.get("ok").and_then(|v| v.as_bool()), Some(true));

    // --- Step 4: Verify promotion store has both records ---
    let store = PromotionStore::new(&gateway_dir).expect("promotion store should create");
    assert!(
        store.has_passed(&artifact_id, &PromotionRole::Evaluator),
        "evaluator should have passed"
    );
    assert!(
        store.has_passed(&artifact_id, &PromotionRole::Auditor),
        "auditor should have passed"
    );
    assert!(
        store.is_fully_promoted(&artifact_id),
        "content should be fully promoted"
    );

    // --- Step 5: specialized_builder installs agent with source_content_handle ---
    let install_args = serde_json::json!({
        "agent_id": "weather.fetcher",
        "name": "Weather Fetcher",
        "description": "Fetches weather from API",
        "instructions": "---\nname: weather.fetcher\ndescription: Fetches weather\nexecution_mode: script\nscript_entry: main.py\n---\n# Weather Fetcher\nFetches weather data.",
        "capabilities": [
            { "type": "NetworkAccess", "hosts": ["api.open-meteo.com"] }
        ],
        "artifact_id": artifact_id,
        "source_content_handle": content_handle,
        "promotion_gate": promotion_gate_with_evidence(
            &["NetworkAccess"],
            true,
            Some(&content_handle),
        )
    });

    let install_result = registry
        .execute(
            "agent.install",
            &evolution_manifest(),
            &PolicyEngine::new(evolution_manifest()),
            &builder_dir,
            Some(&gateway_dir),
            &serde_json::to_string(&install_args).unwrap(),
            Some("session-install-test"),
            None,
            Some(&config),
        )
        .expect("install should succeed with valid promotion records");

    let install_parsed: serde_json::Value = serde_json::from_str(&install_result).unwrap();
    assert_eq!(install_parsed.get("ok").and_then(|v| v.as_bool()), Some(true));
    assert_eq!(
        install_parsed.get("status").and_then(|v| v.as_str()),
        Some("agent_installed")
    );

    // --- Step 6: Verify agent was installed ---
    let agent_dir = agents_dir.join("weather.fetcher");
    assert!(agent_dir.exists(), "weather.fetcher agent should be installed");
    assert!(agent_dir.join("SKILL.md").exists(), "SKILL.md should exist");
    assert!(agent_dir.join("main.py").exists(), "main.py should exist");
}
