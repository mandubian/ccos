use ccos::capabilities::providers::a2a_provider::{A2AConfig, A2AProvider};
use ccos::capabilities::{CapabilityExecutionPolicy, CapabilityProvider, CapabilityRegistry};
use rtfs::runtime::security::RuntimeContext;
use rtfs::runtime::values::Value;

#[test]
fn test_a2a_provider_creation() {
    let config = A2AConfig {
        agent_id: "agent-123".to_string(),
        endpoint: "http://localhost:8080/agent/agent-123".to_string(),
        protocol: "http".to_string(),
        auth_token: None,
        timeout_ms: 5000,
    };

    let provider = A2AProvider::new(config, "source-agent".to_string());
    assert!(provider.is_ok());
}

#[test]
fn test_a2a_capabilities_registered() {
    let config = A2AConfig {
        agent_id: "agent-123".to_string(),
        endpoint: "http://localhost:8080/agent/agent-123".to_string(),
        protocol: "http".to_string(),
        auth_token: Some("test-token".to_string()),
        timeout_ms: 5000,
    };

    let provider = A2AProvider::new(config, "source-agent".to_string()).unwrap();
    let capabilities = provider.list_capabilities();

    assert_eq!(capabilities.len(), 3);
    assert!(capabilities.iter().any(|c| c.id == "ccos.a2a.send"));
    assert!(capabilities.iter().any(|c| c.id == "ccos.a2a.query"));
    assert!(capabilities.iter().any(|c| c.id == "ccos.a2a.discover"));
}

#[test]
fn test_a2a_discover_capability() {
    let config = A2AConfig {
        agent_id: "agent-123".to_string(),
        endpoint: "http://localhost:8080/agent/agent-123".to_string(),
        protocol: "http".to_string(),
        auth_token: None,
        timeout_ms: 5000,
    };

    let provider = A2AProvider::new(config, "source-agent".to_string()).unwrap();
    let exec_context = ccos::capabilities::provider::ExecutionContext {
        trace_id: "test-trace".to_string(),
        timeout: std::time::Duration::from_secs(5),
    };

    let result = provider.execute_capability(
        "ccos.a2a.discover",
        &Value::Vector(vec![Value::String("*".to_string())]),
        &exec_context,
    );

    assert!(result.is_ok());
    // Should return a map with agents list
    match result.unwrap() {
        Value::Map(_) => {}
        other => panic!("Expected map, got {:?}", other),
    }
}

#[test]
fn test_a2a_security_requirements() {
    let config = A2AConfig {
        agent_id: "agent-123".to_string(),
        endpoint: "http://localhost:8080/agent/agent-123".to_string(),
        protocol: "http".to_string(),
        auth_token: None,
        timeout_ms: 5000,
    };

    let provider = A2AProvider::new(config, "source-agent".to_string()).unwrap();
    let capabilities = provider.list_capabilities();

    // Verify security requirements for all A2A capabilities
    for capability in capabilities {
        assert!(capability.security_requirements.requires_microvm);
        assert!(!capability.security_requirements.permissions.is_empty());
        // A2A should have NetworkAccess and AgentCommunication permissions
        assert!(capability.security_requirements.permissions.len() >= 2);
    }
}

#[test]
fn test_a2a_metadata() {
    let config = A2AConfig {
        agent_id: "agent-123".to_string(),
        endpoint: "http://localhost:8080/agent/agent-123".to_string(),
        protocol: "http".to_string(),
        auth_token: None,
        timeout_ms: 5000,
    };

    let provider = A2AProvider::new(config, "source-agent".to_string()).unwrap();
    let metadata = provider.metadata();

    assert_eq!(metadata.name, "A2A Provider");
    assert_eq!(metadata.version, "0.1.0");
    assert!(metadata.description.contains("Agent-to-Agent"));
    assert!(!metadata.dependencies.is_empty());
}
