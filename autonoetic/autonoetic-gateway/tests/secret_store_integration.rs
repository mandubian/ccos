//! Integration tests for secure secret store directives.
//!
//! Run with:
//!   cargo test -p autonoetic-gateway --test secret_store_integration -- --nocapture

use autonoetic_gateway::runtime::store::SecretStoreRuntime;
use tempfile::tempdir;

#[test]
fn test_response_secret_is_persisted_and_redacted() -> anyhow::Result<()> {
    let tmp = tempdir()?;
    let vault_path = tmp.path().join("vault.json");

    let old_vault_env = std::env::var("AUTONOETIC_VAULT_PATH").ok();
    std::env::set_var("AUTONOETIC_VAULT_PATH", vault_path.display().to_string());

    let instructions = r#"
#### 1. register-agent
**Store**:
- From: `response.agent_id` → To: `memory:moltbook.agent_id`
- From: `response.secret` → To: `secret:MOLTBOOK_SECRET` (Requires Approval)
"#;

    let mut runtime = SecretStoreRuntime::from_instructions(instructions)?
        .ok_or_else(|| anyhow::anyhow!("Expected secret store directives to be loaded"))?;

    let tool_response = r#"{"agent_id":"agent-123","secret":"super-secret-value"}"#;
    let redacted = runtime.apply_and_redact(tool_response)?;
    let redacted_json: serde_json::Value = serde_json::from_str(&redacted)?;

    assert_eq!(redacted_json["agent_id"], "agent-123");
    assert_eq!(redacted_json["secret"], "[REDACTED]");

    let vault_raw = std::fs::read_to_string(&vault_path)?;
    let vault_json: serde_json::Value = serde_json::from_str(&vault_raw)?;
    assert_eq!(vault_json["MOLTBOOK_SECRET"], "super-secret-value");

    match old_vault_env {
        Some(v) => std::env::set_var("AUTONOETIC_VAULT_PATH", v),
        None => std::env::remove_var("AUTONOETIC_VAULT_PATH"),
    }
    Ok(())
}
