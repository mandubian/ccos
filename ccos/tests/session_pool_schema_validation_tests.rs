use ccos::capabilities::registry::CapabilityRegistry;
use ccos::capabilities::session_pool::{SessionHandler, SessionId, SessionPoolManager};
use ccos::capability_marketplace::types::{CapabilityManifest, LocalCapability, ProviderType};
use ccos::capability_marketplace::CapabilityMarketplace;
use rtfs::ast::{PrimitiveType, TypeExpr};
use rtfs::runtime::error::RuntimeResult;
use rtfs::runtime::values::Value;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;

struct MockSessionHandler;

impl SessionHandler for MockSessionHandler {
    fn initialize_session(
        &self,
        _capability_id: &str,
        _metadata: &HashMap<String, String>,
    ) -> RuntimeResult<SessionId> {
        Ok("mock-session".to_string())
    }

    fn execute_with_session(
        &self,
        _session_id: &SessionId,
        _capability_id: &str,
        _args: &[Value],
    ) -> RuntimeResult<Value> {
        // Intentionally return a bad type to ensure output schema validation is enforced
        Ok(Value::String("not-an-int".to_string()))
    }

    fn terminate_session(&self, _session_id: &SessionId) -> RuntimeResult<()> {
        Ok(())
    }
}

#[tokio::test]
async fn session_managed_capability_enforces_output_schema() {
    let registry = Arc::new(RwLock::new(CapabilityRegistry::new()));
    let marketplace = CapabilityMarketplace::new(registry);

    // Install a session pool with a mock "mcp" handler.
    let mut pool = SessionPoolManager::new();
    pool.register_handler("mcp", Arc::new(MockSessionHandler));
    marketplace.set_session_pool(Arc::new(pool)).await;

    // Register a manifest that triggers session management via metadata.
    let mut manifest = CapabilityManifest::new(
        "mcp.test.bad_output".to_string(),
        "Bad output capability".to_string(),
        "Returns a string even though output schema is Int".to_string(),
        // Provider doesn't matter for the session path (metadata triggers it),
        // but using Local keeps the manifest minimal.
        ProviderType::Local(LocalCapability {
            handler: Arc::new(|_inputs: &Value| -> RuntimeResult<Value> { Ok(Value::Nil) }),
        }),
        "1.0.0".to_string(),
    );
    manifest.input_schema = Some(TypeExpr::Primitive(PrimitiveType::Int));
    manifest.output_schema = Some(TypeExpr::Primitive(PrimitiveType::Int));
    // Ensure SessionPoolManager can detect provider type and session is required.
    manifest
        .metadata
        .insert("mcp_server_url".to_string(), "http://example".to_string());
    manifest
        .metadata
        .insert("mcp_requires_session".to_string(), "true".to_string());

    marketplace
        .register_capability_manifest(manifest)
        .await
        .expect("register_capability_manifest should succeed");

    // Input passes (Int), output fails (string) -> must return an error.
    let err = marketplace
        .execute_capability("mcp.test.bad_output", &Value::Integer(1))
        .await
        .unwrap_err();
    let msg = err.to_string();
    assert!(
        msg.contains("Output validation failed") || msg.contains("Type mismatch"),
        "unexpected error: {}",
        msg
    );
}

#[tokio::test]
async fn session_managed_capability_enforces_input_schema_before_execution() {
    let registry = Arc::new(RwLock::new(CapabilityRegistry::new()));
    let marketplace = CapabilityMarketplace::new(registry);

    // Install a session pool with a mock "mcp" handler.
    let mut pool = SessionPoolManager::new();
    pool.register_handler("mcp", Arc::new(MockSessionHandler));
    marketplace.set_session_pool(Arc::new(pool)).await;

    let mut manifest = CapabilityManifest::new(
        "mcp.test.bad_input".to_string(),
        "Bad input capability".to_string(),
        "Should fail input schema before session handler is invoked".to_string(),
        ProviderType::Local(LocalCapability {
            handler: Arc::new(|_inputs: &Value| -> RuntimeResult<Value> { Ok(Value::Nil) }),
        }),
        "1.0.0".to_string(),
    );
    manifest.input_schema = Some(TypeExpr::Primitive(PrimitiveType::Int));
    manifest.output_schema = Some(TypeExpr::Primitive(PrimitiveType::Int));
    manifest
        .metadata
        .insert("mcp_server_url".to_string(), "http://example".to_string());
    manifest
        .metadata
        .insert("mcp_requires_session".to_string(), "true".to_string());

    marketplace
        .register_capability_manifest(manifest)
        .await
        .expect("register_capability_manifest should succeed");

    let err = marketplace
        .execute_capability("mcp.test.bad_input", &Value::String("nope".to_string()))
        .await
        .unwrap_err();
    let msg = err.to_string();
    assert!(
        msg.contains("Input validation failed") || msg.contains("Type mismatch"),
        "unexpected error: {}",
        msg
    );
}
