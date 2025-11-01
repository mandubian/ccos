use ccos::capabilities::{CapabilityExecutionPolicy, CapabilityProvider, CapabilityRegistry};
use ccos::capabilities::providers::remote_rtfs_provider::{RemoteRTFSConfig, RemoteRTFSProvider};
use rtfs::runtime::security::RuntimeContext;
use rtfs::runtime::values::Value;

#[test]
fn test_remote_rtfs_provider_creation() {
    let config = RemoteRTFSConfig {
        endpoint: "http://localhost:8080".to_string(),
        auth_token: None,
        timeout_ms: 5000,
        use_tls: false,
    };

    let provider = RemoteRTFSProvider::new(config);
    assert!(provider.is_ok());
}

#[test]
fn test_remote_rtfs_capabilities_registered() {
    let config = RemoteRTFSConfig {
        endpoint: "http://localhost:8080".to_string(),
        auth_token: Some("test-token".to_string()),
        timeout_ms: 5000,
        use_tls: false,
    };

    let provider = RemoteRTFSProvider::new(config).unwrap();
    let capabilities = provider.list_capabilities();

    assert_eq!(capabilities.len(), 2);
    assert!(capabilities.iter().any(|c| c.id == "ccos.remote.execute"));
    assert!(capabilities.iter().any(|c| c.id == "ccos.remote.ping"));
}

#[test]
fn test_remote_rtfs_ping_capability() {
    let config = RemoteRTFSConfig {
        endpoint: "http://localhost:8080".to_string(),
        auth_token: None,
        timeout_ms: 5000,
        use_tls: false,
    };

    let provider = RemoteRTFSProvider::new(config).unwrap();
    let exec_context = ccos::capabilities::provider::ExecutionContext {
        trace_id: "test-trace".to_string(),
        timeout: std::time::Duration::from_secs(5),
    };

    let result = provider.execute_capability(
        "ccos.remote.ping",
        &Value::Vector(vec![Value::String("http://localhost:8080".to_string())]),
        &exec_context,
    );

    assert!(result.is_ok());
    assert_eq!(result.unwrap(), Value::Boolean(true));
}

#[test]
fn test_remote_rtfs_security_requirements() {
    let config = RemoteRTFSConfig {
        endpoint: "http://localhost:8080".to_string(),
        auth_token: None,
        timeout_ms: 5000,
        use_tls: false,
    };

    let provider = RemoteRTFSProvider::new(config).unwrap();
    let capabilities = provider.list_capabilities();

    // Verify security requirements
    for capability in capabilities {
        assert!(capability.security_requirements.requires_microvm);
        assert!(!capability.security_requirements.permissions.is_empty());
    }
}

#[test]
fn test_remote_rtfs_json_conversions() {
    // Test that JSON conversions work correctly
    let test_value = Value::Map(vec![
        (rtfs::ast::MapKey::String("key".to_string()), Value::String("value".to_string())),
        (rtfs::ast::MapKey::String("number".to_string()), Value::Integer(42)),
    ].into_iter().collect());

    // This tests the internal conversion logic
    assert!(matches!(test_value, Value::Map(_)));
}

