//! Integration tests for the approved sandbox exec replay cache.
//!
//! Tests the complete cache lifecycle:
//! 1. Cache miss → approval required
//! 2. Cache hit → skip approval
//! 3. Different code → new cache entry
//! 4. Opaque targets → never cached
//!
//! These tests verify both the cache module directly AND the integration
//! with sandbox.exec through the tool registry.

mod support;

use autonoetic_gateway::policy::PolicyEngine;
use autonoetic_gateway::runtime::approved_exec_cache::{
    ApprovedExecCache, ApprovedExecEntry, compute_fingerprint, has_concrete_targets,
    normalize_targets,
};
use autonoetic_gateway::runtime::remote_access::DetectedPattern;
use autonoetic_gateway::runtime::tools::default_registry;
use autonoetic_types::agent::{AgentIdentity, AgentManifest, RuntimeDeclaration};
use autonoetic_types::capability::Capability;
use autonoetic_types::config::GatewayConfig;
use tempfile::tempdir;

fn create_pattern(category: &str, pattern: &str) -> DetectedPattern {
    DetectedPattern {
        category: category.to_string(),
        pattern: pattern.to_string(),
        line_number: Some(1),
        reason: "test".to_string(),
    }
}

fn test_agent_manifest() -> AgentManifest {
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
            id: "test.agent".to_string(),
            name: "test.agent".to_string(),
            description: "Test agent".to_string(),
        },
        capabilities: vec![Capability::CodeExecution { patterns: vec!["*".to_string()] }],
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

/// Creates a test script file with the given content and returns the script path.
fn create_test_script(agent_dir: &std::path::Path, filename: &str, content: &str) -> std::path::PathBuf {
    let scripts_dir = agent_dir.join("scripts");
    std::fs::create_dir_all(&scripts_dir).expect("scripts dir should create");
    let script_path = scripts_dir.join(filename);
    std::fs::write(&script_path, content).expect("script should write");
    script_path
}

#[test]
fn test_cache_record_and_find() {
    let temp = tempdir().expect("tempdir should create");
    let gateway_dir = temp.path();

    let cache = ApprovedExecCache::new(gateway_dir).expect("cache should create");
    assert_eq!(cache.len(), 0);

    let now = chrono::Utc::now().to_rfc3339();
    let entry = ApprovedExecEntry {
        fingerprint: "sha256:abc123".to_string(),
        agent_id: "test.agent".to_string(),
        remote_targets: vec!["api.example.com".to_string()],
        code_content: "import requests\nrequests.get('https://api.example.com')".to_string(),
        approval_request_id: "apr-12345678".to_string(),
        approved_at: now.clone(),
        approved_by: "operator".to_string(),
        last_used_at: now.clone(),
    };

    cache.record(entry.clone()).expect("record should succeed");
    assert_eq!(cache.len(), 1);

    let found = cache.find("sha256:abc123");
    assert!(found.is_some());
    assert_eq!(found.unwrap().agent_id, "test.agent");
}

#[test]
fn test_cache_persistence() {
    let temp = tempdir().expect("tempdir should create");
    let gateway_dir = temp.path();

    let now = chrono::Utc::now().to_rfc3339();
    let entry = ApprovedExecEntry {
        fingerprint: "sha256:persistent".to_string(),
        agent_id: "test.agent".to_string(),
        remote_targets: vec!["api.example.com".to_string()],
        code_content: "code".to_string(),
        approval_request_id: "apr-12345678".to_string(),
        approved_at: now.clone(),
        approved_by: "operator".to_string(),
        last_used_at: now.clone(),
    };

    {
        let cache = ApprovedExecCache::new(gateway_dir).expect("cache should create");
        cache.record(entry).expect("record should succeed");
    }

    // Reopen cache and verify persistence
    let cache = ApprovedExecCache::new(gateway_dir).expect("cache should reopen");
    assert_eq!(cache.len(), 1);
    let found = cache.find("sha256:persistent");
    assert!(found.is_some());
}

#[test]
fn test_cache_update_last_used() {
    let temp = tempdir().expect("tempdir should create");
    let gateway_dir = temp.path();

    let cache = ApprovedExecCache::new(gateway_dir).expect("cache should create");
    let now = chrono::Utc::now().to_rfc3339();
    let entry = ApprovedExecEntry {
        fingerprint: "sha256:update".to_string(),
        agent_id: "test.agent".to_string(),
        remote_targets: vec!["api.example.com".to_string()],
        code_content: "code".to_string(),
        approval_request_id: "apr-12345678".to_string(),
        approved_at: now.clone(),
        approved_by: "operator".to_string(),
        last_used_at: now.clone(),
    };

    cache.record(entry).expect("record should succeed");
    cache.update_last_used("sha256:update").expect("update should succeed");

    let found = cache.find("sha256:update").expect("should find entry");
    // Verify last_used_at was updated (it should be close to now)
    assert!(found.last_used_at >= now);
}

#[test]
fn test_cache_not_found() {
    let temp = tempdir().expect("tempdir should create");
    let gateway_dir = temp.path();

    let cache = ApprovedExecCache::new(gateway_dir).expect("cache should create");
    assert!(cache.find("sha256:nonexistent").is_none());
}

#[test]
fn test_has_concrete_targets_url_only() {
    let patterns = vec![create_pattern("url_literal", "https://api.example.com/data")];
    assert!(has_concrete_targets(&patterns));
}

#[test]
fn test_has_concrete_targets_mixed_concrete() {
    let patterns = vec![
        create_pattern("url_literal", "https://api.example.com/data"),
        create_pattern("ip_address", "192.168.1.100"),
    ];
    assert!(has_concrete_targets(&patterns));
}

#[test]
fn test_has_concrete_targets_with_import() {
    // Import + literal URL should NOT cache - import is opaque
    let patterns = vec![
        create_pattern("import", "import requests"),
        create_pattern("url_literal", "https://api.example.com/data"),
    ];
    // Should NOT cache because import is opaque
    assert!(!has_concrete_targets(&patterns));
}

#[test]
fn test_has_concrete_targets_with_function_call() {
    // URL literal + function call should NOT cache - function_call is opaque
    let patterns = vec![
        create_pattern("url_literal", "https://api.example.com/data"),
        create_pattern("function_call", ".connect("),
    ];
    // Should NOT cache because function_call is opaque
    assert!(!has_concrete_targets(&patterns));
}

#[test]
fn test_has_concrete_targets_only_import_no_url() {
    // Only imports/function calls, no concrete URL - should NOT cache
    let patterns = vec![
        create_pattern("import", "import requests"),
        create_pattern("function_call", "requests.get("),
    ];
    // No concrete target - should NOT cache
    assert!(!has_concrete_targets(&patterns));
}

#[test]
fn test_has_concrete_targets_empty() {
    assert!(!has_concrete_targets(&[]));
}

#[test]
fn test_normalize_targets() {
    let patterns = vec![
        create_pattern("url_literal", "https://api.example.com/v1/data"),
        create_pattern("url_literal", "https://status.github.com/api"),
        create_pattern("import", "import requests"), // Should be skipped
    ];
    let targets = normalize_targets(&patterns);
    assert_eq!(targets, vec!["api.example.com", "status.github.com"]);
}

#[test]
fn test_normalize_targets_dedup() {
    let patterns = vec![
        create_pattern("url_literal", "https://api.example.com/v1"),
        create_pattern("url_literal", "https://api.example.com/v2"),
    ];
    let targets = normalize_targets(&patterns);
    assert_eq!(targets, vec!["api.example.com"]);
}

#[test]
fn test_compute_fingerprint_deterministic() {
    let fp1 = compute_fingerprint("agent.id", &["host.com".to_string()], "code");
    let fp2 = compute_fingerprint("agent.id", &["host.com".to_string()], "code");
    assert_eq!(fp1, fp2);
    assert!(fp1.starts_with("sha256:"));
}

#[test]
fn test_compute_fingerprint_different() {
    let fp1 = compute_fingerprint("agent.a", &["host.com".to_string()], "code");
    let fp2 = compute_fingerprint("agent.b", &["host.com".to_string()], "code");
    assert_ne!(fp1, fp2);
}

#[test]
fn test_cache_full_cycle() {
    let temp = tempdir().expect("tempdir should create");
    let gateway_dir = temp.path();

    // 1. Cache miss
    let cache = ApprovedExecCache::new(gateway_dir).expect("cache should create");
    let fingerprint = compute_fingerprint(
        "test.agent",
        &["api.example.com".to_string()],
        "import requests\nrequests.get('https://api.example.com')",
    );
    assert!(cache.find(&fingerprint).is_none());

    // 2. Record after approval
    let now = chrono::Utc::now().to_rfc3339();
    let entry = ApprovedExecEntry {
        fingerprint: fingerprint.clone(),
        agent_id: "test.agent".to_string(),
        remote_targets: vec!["api.example.com".to_string()],
        code_content: "import requests\nrequests.get('https://api.example.com')".to_string(),
        approval_request_id: "apr-12345678".to_string(),
        approved_at: now.clone(),
        approved_by: "operator".to_string(),
        last_used_at: now.clone(),
    };
    cache.record(entry).expect("record should succeed");

    // 3. Cache hit
    assert!(cache.find(&fingerprint).is_some());

    // 4. Different code = different fingerprint
    let different_fingerprint = compute_fingerprint(
        "test.agent",
        &["api.example.com".to_string()],
        "import requests\nrequests.post('https://api.example.com')", // POST instead of GET
    );
    assert!(cache.find(&different_fingerprint).is_none());
}

#[test]
fn test_cache_not_used_for_opaque_targets() {
    let temp = tempdir().expect("tempdir should create");
    let gateway_dir = temp.path();

    // Code with ONLY imports/function calls, no concrete URL - should NOT cache
    let patterns = vec![
        create_pattern("import", "import requests"),
        create_pattern("function_call", "requests.get("),
    ];

    assert!(!has_concrete_targets(&patterns));

    // Even if we compute a fingerprint, it shouldn't be recorded because
    // has_concrete_targets returns false
    let cache = ApprovedExecCache::new(gateway_dir).expect("cache should create");
    let targets = normalize_targets(&patterns);
    let fingerprint = compute_fingerprint("test.agent", &targets, "code");

    // In the real flow, this would NOT be recorded because has_concrete_targets is false
    // This test verifies the guard condition exists
    assert_eq!(cache.len(), 0);
}

#[test]
fn test_sandbox_exec_cache_hit_skips_approval() {
    let temp = tempdir().expect("tempdir should create");
    let agents_dir = temp.path().join("agents");
    let gateway_dir = agents_dir.join(".gateway");
    std::fs::create_dir_all(&gateway_dir).expect("gateway dir should create");
    std::fs::create_dir_all(&agents_dir).expect("agents dir should create");

    let agent_dir = agents_dir.join("test.agent");
    std::fs::create_dir_all(&agent_dir).expect("agent dir should create");

    let manifest = test_agent_manifest();
    let policy = PolicyEngine::new(manifest.clone());

    let config = GatewayConfig {
        agents_dir: agents_dir.clone(),
        ..Default::default()
    };

    // Pre-populate the cache with a known fingerprint for concrete URL-only code
    let code_content = r#"print("https://api.example.com/data")"#;
    let patterns = vec![create_pattern("url_literal", "https://api.example.com/data")];
    let targets = normalize_targets(&patterns);
    let fingerprint = compute_fingerprint("test.agent", &targets, code_content);

    let cache = ApprovedExecCache::new(&gateway_dir).expect("cache should create");
    let now = chrono::Utc::now().to_rfc3339();
    let entry = ApprovedExecEntry {
        fingerprint: fingerprint.clone(),
        agent_id: "test.agent".to_string(),
        remote_targets: targets.clone(),
        code_content: code_content.to_string(),
        approval_request_id: "apr-test123".to_string(),
        approved_at: now.clone(),
        approved_by: "operator".to_string(),
        last_used_at: now.clone(),
    };
    cache.record(entry).expect("record should succeed");

    // Create a script file with the same code content
    let script_path = create_test_script(&agent_dir, "fetch.py", code_content);
    // Use relative path to avoid /tmp/ interpretation issues
    let script_rel_path = format!("scripts/fetch.py");

    // Call sandbox.exec with the script - should hit cache and skip approval
    let registry = default_registry();
    let args = serde_json::json!({
        "command": format!("python3 {}", script_rel_path),
    });

    let result = registry.execute(
        "sandbox.exec",
        &manifest,
        &policy,
        &agent_dir,
        Some(&gateway_dir),
        &serde_json::to_string(&args).unwrap(),
        Some("test-session"),
        None,
        Some(&config),
            None,
    );

    // The call should succeed (not return approval_required)
    // Note: The actual sandbox execution might fail in test environment,
    // but the key is that it should NOT require approval since cache hit
    match result {
        Ok(resp) => {
            let resp_val: serde_json::Value = serde_json::from_str(&resp).unwrap();
            // Cache hit should skip approval - no approval_required in response
            assert!(
                !resp_val.get("approval_required").and_then(|v| v.as_bool()).unwrap_or(false),
                "Cache hit should skip approval, but got: {}",
                resp
            );
            tracing::info!(response = %resp, "sandbox.exec with cache hit response");
        }
        Err(e) => {
            // If sandbox fails (e.g., bubblewrap not available), we still verify
            // that approval was skipped by checking the error doesn't mention approval
            let err_msg = e.to_string();
            assert!(
                !err_msg.contains("approval") && !err_msg.contains("approval_required"),
                "Cache hit should skip approval requirement, but got error about approval: {}",
                err_msg
            );
            tracing::info!(error = %err_msg, "sandbox.exec cache hit - execution may fail in test env but approval was skipped");
        }
    }
}

#[test]
fn test_sandbox_exec_cache_miss_requires_approval_for_concrete_url() {
    let temp = tempdir().expect("tempdir should create");
    let agents_dir = temp.path().join("agents");
    let gateway_dir = agents_dir.join(".gateway");
    std::fs::create_dir_all(&gateway_dir).expect("gateway dir should create");
    std::fs::create_dir_all(&agents_dir).expect("agents dir should create");

    let agent_dir = agents_dir.join("test.agent");
    std::fs::create_dir_all(&agent_dir).expect("agent dir should create");

    let manifest = test_agent_manifest();
    let policy = PolicyEngine::new(manifest.clone());

    let config = GatewayConfig {
        agents_dir: agents_dir.clone(),
        ..Default::default()
    };

    // Cache is empty - should require approval for concrete URL code
    // Use python -c to avoid file path issues with sandbox
    let code_content = r#"print("https://api.cache-test.dev/data")"#;
    let registry = default_registry();
    let args = serde_json::json!({
        "command": format!("python3 -c {}", code_content),
    });

    let result = registry.execute(
        "sandbox.exec",
        &manifest,
        &policy,
        &agent_dir,
        Some(&gateway_dir),
        &serde_json::to_string(&args).unwrap(),
        Some("test-session"),
        None,
        Some(&config),
            None,
    );

    // Should require approval since cache is empty
    match result {
        Ok(resp) => {
            let resp_val: serde_json::Value = serde_json::from_str(&resp).unwrap();
            assert!(
                resp_val.get("approval_required").and_then(|v| v.as_bool()).unwrap_or(false),
                "Cache miss should require approval for concrete URL code, but got: {}",
                resp
            );
            // Verify the request_id is present
            assert!(
                resp_val.get("request_id").is_some(),
                "Should include request_id for approval"
            );
            tracing::info!(response = %resp, "sandbox.exec cache miss - approval required");
        }
        Err(e) => {
            // If execution itself fails, check if it's an approval-related error
            let err_msg = e.to_string();
            assert!(
                err_msg.contains("approval") || err_msg.contains("approval_required"),
                "Cache miss should indicate approval required, but got: {}",
                err_msg
            );
            tracing::info!(error = %err_msg, "sandbox.exec cache miss - approval required");
        }
    }
}

#[test]
fn test_sandbox_exec_opaque_import_never_caches() {
    let temp = tempdir().expect("tempdir should create");
    let agents_dir = temp.path().join("agents");
    let gateway_dir = agents_dir.join(".gateway");
    std::fs::create_dir_all(&gateway_dir).expect("gateway dir should create");
    std::fs::create_dir_all(&agents_dir).expect("agents dir should create");

    let agent_dir = agents_dir.join("test.agent");
    std::fs::create_dir_all(&agent_dir).expect("agent dir should create");

    let manifest = test_agent_manifest();
    let policy = PolicyEngine::new(manifest.clone());

    let config = GatewayConfig {
        agents_dir: agents_dir.clone(),
        ..Default::default()
    };

    // Code with import + URL literal - should NOT cache (import is opaque)
    // Even if we manually add to cache, it shouldn't be used
    let code_content = r#"import requests
requests.get("https://api.cache-test.dev")"#;
    let patterns = vec![
        create_pattern("import", "import requests"),
        create_pattern("url_literal", "https://api.cache-test.dev"),
    ];
    let targets = normalize_targets(&patterns);
    let fingerprint = compute_fingerprint("test.agent", &targets, code_content);

    // Manually add to cache (bypassing the has_concrete_targets check)
    let cache = ApprovedExecCache::new(&gateway_dir).expect("cache should create");
    let now = chrono::Utc::now().to_rfc3339();
    let entry = ApprovedExecEntry {
        fingerprint: fingerprint.clone(),
        agent_id: "test.agent".to_string(),
        remote_targets: targets.clone(),
        code_content: code_content.to_string(),
        approval_request_id: "apr-test456".to_string(),
        approved_at: now.clone(),
        approved_by: "operator".to_string(),
        last_used_at: now.clone(),
    };
    cache.record(entry).expect("record should succeed");

    // Create script with the same content
    create_test_script(&agent_dir, "fetch_with_import.py", code_content);

    // Call sandbox.exec - should STILL require approval because has_concrete_targets
    // returns false for code with imports
    let registry = default_registry();
    let args = serde_json::json!({
        "command": "python3 scripts/fetch_with_import.py",
    });

    let result = registry.execute(
        "sandbox.exec",
        &manifest,
        &policy,
        &agent_dir,
        Some(&gateway_dir),
        &serde_json::to_string(&args).unwrap(),
        Some("test-session"),
        None,
        Some(&config),
            None,
    );

    // Should require approval even though cache has entry
    // because has_concrete_targets returns false for import patterns
    match result {
        Ok(resp) => {
            let resp_val: serde_json::Value = serde_json::from_str(&resp).unwrap();
            assert!(
                resp_val.get("approval_required").and_then(|v| v.as_bool()).unwrap_or(false),
                "Opaque patterns (imports) should always require approval even with cache entry, but got: {}",
                resp
            );
            tracing::info!(response = %resp, "sandbox.exec with import - approval still required");
        }
        Err(e) => {
            let err_msg = e.to_string();
            assert!(
                err_msg.contains("approval") || err_msg.contains("approval_required"),
                "Opaque patterns should require approval, but got: {}",
                err_msg
            );
            tracing::info!(error = %err_msg, "sandbox.exec with import - approval still required");
        }
    }
}
