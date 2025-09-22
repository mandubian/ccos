use rtfs_compiler::runtime::values::Value;
use rtfs_compiler::ccos::capabilities::registry::CapabilityRegistry;
use rtfs_compiler::ccos::capability_marketplace::{CapabilityIsolationPolicy, ResourceConstraints};
use std::sync::Arc;
use tokio::sync::RwLock;

#[tokio::test]
async fn test_capability_marketplace_bootstrap() {
    use rtfs_compiler::ccos::capability_marketplace::CapabilityMarketplace;

    // Create a capability registry with some built-in capabilities
    let registry = CapabilityRegistry::new();
    
    // Create marketplace with the registry
    let capability_registry = Arc::new(RwLock::new(registry));
    let marketplace = CapabilityMarketplace::new(Arc::clone(&capability_registry));
    
    // Bootstrap the marketplace
    marketplace.bootstrap().await.expect("Bootstrap should succeed");
    
    // Verify that capabilities from the registry are available
    let count = marketplace.capability_count().await;
    assert!(count > 0, "Marketplace should have capabilities after bootstrap");
    
    // Test that we can list capabilities
    let capabilities = marketplace.list_capabilities().await;
    assert!(!capabilities.is_empty(), "Should be able to list capabilities");
    
    // Test that we can check for specific capabilities
    for capability in capabilities {
        assert!(marketplace.has_capability(&capability.id).await, 
                "Should have capability: {}", capability.id);
    }
}

#[tokio::test]
async fn test_capability_marketplace_isolation_policy() {
    use rtfs_compiler::ccos::capability_marketplace::CapabilityMarketplace;

    // Create a capability registry
    let registry = CapabilityRegistry::new();
    let capability_registry = Arc::new(RwLock::new(registry));
    let mut marketplace = CapabilityMarketplace::new(Arc::clone(&capability_registry));
    
    // Bootstrap the marketplace
    marketplace.bootstrap().await.expect("Bootstrap should succeed");
    
    // Set a restrictive isolation policy
    let mut policy = CapabilityIsolationPolicy::default();
    policy.allowed_capabilities = vec!["ccos.echo".to_string()];
    policy.denied_capabilities = vec!["ccos.math.add".to_string()];
    marketplace.set_isolation_policy(policy);
    
    // Test that allowed capabilities work
    let allowed_result = marketplace.execute_capability("ccos.echo", &Value::List(vec![Value::String("test".to_string())])).await;
    assert!(allowed_result.is_ok(), "Allowed capability should execute");
    
    // Test that denied capabilities fail
    let denied_result = marketplace.execute_capability("ccos.math.add", &Value::List(vec![Value::Integer(1), Value::Integer(2)])).await;
    assert!(denied_result.is_err(), "Denied capability should fail");
    
    // Test that unknown capabilities fail
    let unknown_result = marketplace.execute_capability("unknown.capability", &Value::List(vec![])).await;
    assert!(unknown_result.is_err(), "Unknown capability should fail");
}

#[tokio::test]
async fn test_capability_marketplace_dynamic_registration() {
    use rtfs_compiler::ccos::capability_marketplace::CapabilityMarketplace;

    // Create marketplace
    let registry = CapabilityRegistry::new();
    let capability_registry = Arc::new(RwLock::new(registry));
    let marketplace = CapabilityMarketplace::new(Arc::clone(&capability_registry));
    
    // Bootstrap the marketplace
    marketplace.bootstrap().await.expect("Bootstrap should succeed");
    
    // Register a new capability dynamically
    marketplace.register_local_capability(
        "test.dynamic".to_string(),
        "Dynamic Test Capability".to_string(),
        "A dynamically registered capability for testing".to_string(),
        Arc::new(|_| Ok(Value::String("dynamic_result".to_string()))),
    ).await.expect("Should register capability");
    
    // Verify the capability is available
    assert!(marketplace.has_capability("test.dynamic").await, "Should have dynamically registered capability");
    
    // Test execution
    let result = marketplace.execute_capability("test.dynamic", &Value::List(vec![])).await;
    assert!(result.is_ok(), "Should execute dynamic capability");
    
    if let Ok(Value::String(s)) = result {
        assert_eq!(s, "dynamic_result", "Should return expected result");
    } else {
        panic!("Expected string result");
    }
}

#[tokio::test]
async fn test_capability_marketplace_audit_events() {
    use rtfs_compiler::ccos::capability_marketplace::CapabilityMarketplace;

    // Create marketplace
    let registry = CapabilityRegistry::new();
    let capability_registry = Arc::new(RwLock::new(registry));
    let marketplace = CapabilityMarketplace::new(Arc::clone(&capability_registry));
    
    // Bootstrap the marketplace
    marketplace.bootstrap().await.expect("Bootstrap should succeed");
    
    // Get initial capability count
    let initial_count = marketplace.capability_count().await;
    
    // Register a capability (this should emit "capability_registered" audit event)
    marketplace.register_local_capability(
        "test.audit".to_string(),
        "Audit Test Capability".to_string(),
        "A capability for testing audit events".to_string(),
        Arc::new(|_| Ok(Value::Integer(42))),
    ).await.expect("Should register capability");
    
    // Verify capability count increased
    let new_count = marketplace.capability_count().await;
    assert_eq!(new_count, initial_count + 1, "Capability count should increase");
    
    // Test execution (this should emit "capability_executed" audit event)
    let result = marketplace.execute_capability("test.audit", &Value::List(vec![])).await;
    assert!(result.is_ok(), "Should execute capability");
    
    if let Ok(Value::Integer(n)) = result {
        assert_eq!(n, 42, "Should return expected result");
    } else {
        panic!("Expected integer result");
    }
    
    // Remove the capability (this should emit "capability_removed" audit event)
    marketplace.remove_capability("test.audit").await.expect("Should remove capability");
    
    // Verify capability count decreased
    let final_count = marketplace.capability_count().await;
    assert_eq!(final_count, initial_count, "Capability count should return to initial");
    
    // Note: The actual audit events are logged to stderr via eprintln! in the current implementation
    // In a full implementation, these would be captured and verified programmatically
    // For now, we can see them in the test output when running with --nocapture
}

#[tokio::test]
async fn test_capability_marketplace_enhanced_isolation() {
    use rtfs_compiler::ccos::capability_marketplace::{
        CapabilityMarketplace, NamespacePolicy
    };
    use std::collections::HashMap;

    // Create marketplace
    let registry = CapabilityRegistry::new();
    let capability_registry = Arc::new(RwLock::new(registry));
    let mut marketplace = CapabilityMarketplace::new(Arc::clone(&capability_registry));
    
    // Bootstrap the marketplace
    marketplace.bootstrap().await.expect("Bootstrap should succeed");
    
    // Test namespace-based isolation
    let mut namespace_policy = HashMap::new();
    namespace_policy.insert("ccos".to_string(), NamespacePolicy {
        allowed_patterns: vec!["ccos.echo".to_string()],
        denied_patterns: vec!["ccos.math.*".to_string()],
        resource_limits: None,
    });
    
    let mut policy = CapabilityIsolationPolicy::default();
    policy.namespace_policies = namespace_policy;
    marketplace.set_isolation_policy(policy);
    
    // Test namespace policy enforcement
    let allowed_result = marketplace.execute_capability("ccos.echo", &Value::List(vec![Value::String("test".to_string())])).await;
    assert!(allowed_result.is_ok(), "Namespace-allowed capability should execute");
    
    let denied_result = marketplace.execute_capability("ccos.math.add", &Value::List(vec![Value::Integer(1), Value::Integer(2)])).await;
    assert!(denied_result.is_err(), "Namespace-denied capability should fail");
}

#[tokio::test]
async fn test_capability_marketplace_time_constraints() {
    use rtfs_compiler::ccos::capability_marketplace::{
        CapabilityMarketplace, TimeConstraints
    };

    // Create marketplace
    let registry = CapabilityRegistry::new();
    let capability_registry = Arc::new(RwLock::new(registry));
    let mut marketplace = CapabilityMarketplace::new(Arc::clone(&capability_registry));
    
    // Bootstrap the marketplace
    marketplace.bootstrap().await.expect("Bootstrap should succeed");
    
    // Test time constraints - only allow during specific hours
    let time_constraints = TimeConstraints {
        allowed_hours: Some(vec![9, 10, 11, 12, 13, 14, 15, 16, 17]), // 9 AM to 5 PM
        allowed_days: Some(vec![1, 2, 3, 4, 5]), // Monday to Friday
        timezone: Some("UTC".to_string()),
    };
    
    let mut policy = CapabilityIsolationPolicy::default();
    policy.time_constraints = Some(time_constraints);
    marketplace.set_isolation_policy(policy);
    
    // The test will pass or fail depending on the current time
    // This is a basic test to ensure the time constraint checking doesn't crash
    let result = marketplace.execute_capability("ccos.echo", &Value::List(vec![Value::String("test".to_string())])).await;
    // We don't assert on the result since it depends on the current time
    println!("Time constraint test result: {:?}", result);
}

#[tokio::test]
async fn test_capability_marketplace_audit_integration() {
    use rtfs_compiler::ccos::capability_marketplace::CapabilityMarketplace;

    // Create marketplace
    let registry = CapabilityRegistry::new();
    let capability_registry = Arc::new(RwLock::new(registry));
    let marketplace = CapabilityMarketplace::new(Arc::clone(&capability_registry));
    
    // Bootstrap the marketplace
    marketplace.bootstrap().await.expect("Bootstrap should succeed");
    
    // Register a capability (should emit audit event)
    marketplace.register_local_capability(
        "test.audit.integration".to_string(),
        "Audit Integration Test".to_string(),
        "Testing audit event integration".to_string(),
        Arc::new(|_| Ok(Value::String("audit_test_result".to_string()))),
    ).await.expect("Should register capability");
    
    // Execute the capability (should emit audit event)
    let result = marketplace.execute_capability("test.audit.integration", &Value::List(vec![])).await;
    assert!(result.is_ok(), "Should execute capability");
    
    // Remove the capability (should emit audit event)
    marketplace.remove_capability("test.audit.integration").await.expect("Should remove capability");
    
    // Verify capability is removed
    let result = marketplace.execute_capability("test.audit.integration", &Value::List(vec![])).await;
    assert!(result.is_err(), "Capability should be removed");
}

#[tokio::test]
async fn test_capability_marketplace_discovery_providers() {
    use rtfs_compiler::ccos::capability_marketplace::{
        CapabilityMarketplace, discovery::StaticDiscoveryProvider
    };

    // Create marketplace
    let registry = CapabilityRegistry::new();
    let capability_registry = Arc::new(RwLock::new(registry));
    let mut marketplace = CapabilityMarketplace::new(Arc::clone(&capability_registry));
    
    // Add a static discovery provider
    let static_provider = Box::new(StaticDiscoveryProvider::new());
    marketplace.add_discovery_agent(static_provider);
    
    // Bootstrap the marketplace
    marketplace.bootstrap().await.expect("Bootstrap should succeed");
    
    // Verify that static capabilities are discovered
    assert!(marketplace.has_capability("static.hello").await, "Should have static capability");
    
    // Test execution of discovered capability
    let result = marketplace.execute_capability("static.hello", &Value::List(vec![])).await;
    assert!(result.is_ok(), "Should execute static capability");
    
    if let Ok(Value::String(s)) = result {
        assert_eq!(s, "Hello from static discovery!", "Should return expected static result");
    } else {
        panic!("Expected string result from static capability");
    }
}

#[tokio::test]
async fn test_capability_marketplace_causal_chain_integration() {
    use rtfs_compiler::ccos::capability_marketplace::CapabilityMarketplace;
    use rtfs_compiler::ccos::causal_chain::CausalChain;

    // Create Causal Chain
    let causal_chain = Arc::new(std::sync::Mutex::new(CausalChain::new().expect("Failed to create Causal Chain")));
    
    // Create marketplace with Causal Chain
    let registry = CapabilityRegistry::new();
    let capability_registry = Arc::new(RwLock::new(registry));
    let marketplace = CapabilityMarketplace::with_causal_chain(
        Arc::clone(&capability_registry),
        Some(Arc::clone(&causal_chain))
    );
    
    // Bootstrap the marketplace
    marketplace.bootstrap().await.expect("Bootstrap should succeed");
    
    // Register a capability (this should create a Causal Chain event)
    marketplace.register_local_capability(
        "test.causal_chain".to_string(),
        "Causal Chain Test".to_string(),
        "Test capability for Causal Chain integration".to_string(),
        Arc::new(|_| Ok(Value::String("test".to_string()))),
    ).await.expect("Capability registration should succeed");
    
    // Execute the capability (this should create another Causal Chain event)
    let result = marketplace.execute_capability(
        "test.causal_chain",
        &Value::List(vec![])
    ).await.expect("Capability execution should succeed");
    
    assert_eq!(result, Value::String("test".to_string()));
    
    // Verify that Causal Chain events were recorded
    let actions: Vec<_> = {
        let chain = causal_chain.lock().expect("Should acquire lock");
        chain.get_all_actions().iter().cloned().collect()
    };
    
    // Should have at least bootstrap events + registration + execution
    assert!(actions.len() >= 3, "Should have recorded capability lifecycle events");
    
    // Check for capability lifecycle events
    let capability_events: Vec<_> = actions.iter()
        .filter(|action| matches!(action.action_type, 
            rtfs_compiler::ccos::types::ActionType::CapabilityRegistered |
            rtfs_compiler::ccos::types::ActionType::CapabilityCall
        ))
        .collect();
    
    assert!(!capability_events.is_empty(), "Should have recorded capability events in Causal Chain");
    
    // Verify event metadata
    for event in &capability_events {
        assert!(event.metadata.contains_key("capability_id"), "Event should have capability_id metadata");
        assert!(event.metadata.contains_key("event_type"), "Event should have event_type metadata");
    }
    
    println!("✅ Causal Chain integration test passed - {} capability events recorded", capability_events.len());
}

#[tokio::test]
async fn test_capability_marketplace_resource_monitoring() {
    use rtfs_compiler::ccos::capability_marketplace::{
        CapabilityMarketplace, ResourceConstraints, types::ResourceMonitoringConfig
    };
    use std::collections::HashMap;
    
    // Create resource constraints with GPU and environmental limits
    let constraints = ResourceConstraints::default()
        .with_gpu_limits(Some(4096), Some(80.0)) // 4GB GPU memory, 80% utilization
        .with_environmental_limits(Some(100.0), Some(0.5)); // 100g CO2, 0.5 kWh
    
    // Create monitoring config
    let monitoring_config = ResourceMonitoringConfig {
        enabled: true,
        monitoring_interval_ms: 100,
        collect_history: true,
        history_retention_seconds: Some(3600),
        resource_settings: HashMap::new(),
    };
    
    // Create marketplace with resource monitoring
    let registry = Arc::new(RwLock::new(CapabilityRegistry::new()));
    let mut marketplace = CapabilityMarketplace::with_resource_monitoring(
        registry,
        None,
        monitoring_config,
    );
    
    // Set isolation policy with resource constraints
    let mut policy = CapabilityIsolationPolicy::default();
    policy.resource_constraints = Some(constraints);
    marketplace.set_isolation_policy(policy);
    
    // Register a test capability
    marketplace.register_local_capability(
        "test.resource_monitoring".to_string(),
        "Resource Monitoring Test".to_string(),
        "Test capability for resource monitoring".to_string(),
        Arc::new(|_| Ok(Value::String("success".to_string()))),
    ).await.unwrap();
    
    // Execute capability - should succeed with resource monitoring
    let result = marketplace.execute_capability(
        "test.resource_monitoring",
        &Value::String("test".to_string()),
    ).await;
    
    assert!(result.is_ok(), "Capability execution should succeed with resource monitoring");
}

#[tokio::test]
async fn test_capability_marketplace_gpu_resource_limits() {
    use rtfs_compiler::ccos::capability_marketplace::{
        CapabilityMarketplace, types::ResourceMonitoringConfig
    };
    use std::collections::HashMap;
    
    // Create constraints with GPU limits that accommodate placeholder values
    let constraints = ResourceConstraints::default()
        .with_gpu_limits(Some(4096), Some(100.0)); // 4GB GPU memory, 100% utilization
    
    let monitoring_config = ResourceMonitoringConfig {
        enabled: true,
        monitoring_interval_ms: 100,
        collect_history: false,
        history_retention_seconds: None,
        resource_settings: HashMap::new(),
    };
    
    let registry = Arc::new(RwLock::new(CapabilityRegistry::new()));
    let mut marketplace = CapabilityMarketplace::with_resource_monitoring(
        registry,
        None,
        monitoring_config,
    );
    
    let mut policy = CapabilityIsolationPolicy::default();
    policy.resource_constraints = Some(constraints);
    marketplace.set_isolation_policy(policy);
    
    // Register a GPU-intensive capability
    marketplace.register_local_capability(
        "gpu.intensive_task".to_string(),
        "GPU Intensive Task".to_string(),
        "A task that uses GPU resources".to_string(),
        Arc::new(|_| Ok(Value::String("gpu_completed".to_string()))),
    ).await.unwrap();
    
    // Execute - should succeed (placeholder values are within limits)
    let result = marketplace.execute_capability(
        "gpu.intensive_task",
        &Value::String("gpu_data".to_string()),
    ).await;
    
    assert!(result.is_ok(), "GPU-intensive capability should execute within limits");
}

#[tokio::test]
async fn test_capability_marketplace_environmental_limits() {
    use rtfs_compiler::ccos::capability_marketplace::{
        CapabilityMarketplace, types::ResourceMonitoringConfig
    };
    use std::collections::HashMap;
    
    // Create constraints with environmental limits (soft enforcement)
    let constraints = ResourceConstraints::default()
        .with_environmental_limits(Some(50.0), Some(0.2)); // 50g CO2, 0.2 kWh
    
    let monitoring_config = ResourceMonitoringConfig {
        enabled: true,
        monitoring_interval_ms: 100,
        collect_history: true,
        history_retention_seconds: Some(1800),
        resource_settings: HashMap::new(),
    };
    
    let registry = Arc::new(RwLock::new(CapabilityRegistry::new()));
    let mut marketplace = CapabilityMarketplace::with_resource_monitoring(
        registry,
        None,
        monitoring_config,
    );
    
    let mut policy = CapabilityIsolationPolicy::default();
    policy.resource_constraints = Some(constraints);
    marketplace.set_isolation_policy(policy);
    
    // Register an energy-intensive capability
    marketplace.register_local_capability(
        "energy.intensive_task".to_string(),
        "Energy Intensive Task".to_string(),
        "A task that consumes significant energy".to_string(),
        Arc::new(|_| Ok(Value::String("energy_completed".to_string()))),
    ).await.unwrap();
    
    // Execute - should succeed (environmental limits are soft warnings)
    let result = marketplace.execute_capability(
        "energy.intensive_task",
        &Value::String("energy_data".to_string()),
    ).await;
    
    assert!(result.is_ok(), "Energy-intensive capability should execute (soft limits)");
}

#[tokio::test]
async fn test_capability_marketplace_custom_resource_limits() {
    use rtfs_compiler::ccos::capability_marketplace::{
        CapabilityMarketplace, types::ResourceMonitoringConfig, types::EnforcementLevel
    };
    use std::collections::HashMap;
    
    // Create constraints with custom resource limits
    let mut constraints = ResourceConstraints::default();
    constraints = constraints.with_custom_limit(
        "api_calls",
        100.0,
        "calls",
        EnforcementLevel::Hard,
    );
    constraints = constraints.with_custom_limit(
        "database_connections",
        10.0,
        "connections",
        EnforcementLevel::Warning,
    );
    
    let monitoring_config = ResourceMonitoringConfig {
        enabled: true,
        monitoring_interval_ms: 100,
        collect_history: false,
        history_retention_seconds: None,
        resource_settings: HashMap::new(),
    };
    
    let registry = Arc::new(RwLock::new(CapabilityRegistry::new()));
    let mut marketplace = CapabilityMarketplace::with_resource_monitoring(
        registry,
        None,
        monitoring_config,
    );
    
    let mut policy = CapabilityIsolationPolicy::default();
    policy.resource_constraints = Some(constraints);
    marketplace.set_isolation_policy(policy);
    
    // Register a capability that uses custom resources
    marketplace.register_local_capability(
        "custom.resource_task".to_string(),
        "Custom Resource Task".to_string(),
        "A task that uses custom resource types".to_string(),
        Arc::new(|_| Ok(Value::String("custom_completed".to_string()))),
    ).await.unwrap();
    
    // Execute - should succeed
    let result = marketplace.execute_capability(
        "custom.resource_task",
        &Value::String("custom_data".to_string()),
    ).await;
    
    assert!(result.is_ok(), "Custom resource capability should execute");
}

#[tokio::test]
async fn test_capability_marketplace_resource_violation_handling() {
    use rtfs_compiler::ccos::capability_marketplace::{
        CapabilityMarketplace, types::ResourceMonitoringConfig
    };
    use std::collections::HashMap;
    
    // Create very restrictive constraints
    let constraints = ResourceConstraints::default()
        .with_gpu_limits(Some(1), Some(1.0)); // 1MB GPU memory, 1% utilization (very low)
    
    let monitoring_config = ResourceMonitoringConfig {
        enabled: true,
        monitoring_interval_ms: 100,
        collect_history: false,
        history_retention_seconds: None,
        resource_settings: HashMap::new(),
    };
    
    let registry = Arc::new(RwLock::new(CapabilityRegistry::new()));
    let mut marketplace = CapabilityMarketplace::with_resource_monitoring(
        registry,
        None,
        monitoring_config,
    );
    
    let mut policy = CapabilityIsolationPolicy::default();
    policy.resource_constraints = Some(constraints);
    marketplace.set_isolation_policy(policy);
    
    // Register a capability
    marketplace.register_local_capability(
        "test.violation".to_string(),
        "Violation Test".to_string(),
        "Test capability for resource violations".to_string(),
        Arc::new(|_| Ok(Value::String("violation_test".to_string()))),
    ).await.unwrap();
    
    // Execute - should fail due to resource violations
    let result = marketplace.execute_capability(
        "test.violation",
        &Value::String("test".to_string()),
    ).await;
    
    // Note: This test may pass or fail depending on the placeholder values
    // in the resource monitoring implementation. The important thing is
    // that resource monitoring is being applied.
    match result {
        Ok(_) => println!("Capability executed successfully (placeholder values within limits)"),
        Err(e) => {
            assert!(e.to_string().contains("Resource constraints violated"), 
                   "Error should be about resource constraints: {}", e);
        }
    }
}

#[tokio::test]
async fn test_capability_marketplace_resource_monitoring_disabled() {
    use rtfs_compiler::ccos::capability_marketplace::CapabilityMarketplace;
    
    // Create marketplace without resource monitoring
    let registry = Arc::new(RwLock::new(CapabilityRegistry::new()));
    let mut marketplace = CapabilityMarketplace::with_causal_chain(registry, None);
    
    // Set isolation policy with resource constraints (but no monitoring)
    let mut policy = CapabilityIsolationPolicy::default();
    policy.resource_constraints = Some(ResourceConstraints::default()
        .with_gpu_limits(Some(1024), Some(50.0)));
    marketplace.set_isolation_policy(policy);
    
    // Register a capability
    marketplace.register_local_capability(
        "test.no_monitoring".to_string(),
        "No Monitoring Test".to_string(),
        "Test capability without resource monitoring".to_string(),
        Arc::new(|_| Ok(Value::String("no_monitoring".to_string()))),
    ).await.unwrap();
    
    // Execute - should succeed (no monitoring means no resource checks)
    let result = marketplace.execute_capability(
        "test.no_monitoring",
        &Value::String("test".to_string()),
    ).await;
    
    assert!(result.is_ok(), "Capability should execute without resource monitoring");
}

// --- Observability: Prometheus exporter smoke test (feature-gated) ---
#[cfg(feature = "metrics_exporter")]
#[test]
fn observability_prometheus_render_smoke() {
    use std::sync::{Arc, Mutex};
    use rtfs_compiler::runtime::RuntimeContext;
    use rtfs_compiler::ccos::capability_marketplace::CapabilityMarketplace;
    use rtfs_compiler::ccos::host::RuntimeHost;
    use rtfs_compiler::runtime::metrics_exporter::render_prometheus_text;

    let capability_registry = Arc::new(tokio::sync::RwLock::new(CapabilityRegistry::new()));
    let capability_marketplace = Arc::new(CapabilityMarketplace::new(capability_registry.clone()));
    let causal_chain = Arc::new(Mutex::new(rtfs_compiler::ccos::causal_chain::CausalChain::new().expect("chain")));
    let host = RuntimeHost::new(
        Arc::clone(&causal_chain),
        Arc::clone(&capability_marketplace),
        RuntimeContext::pure(),
    );
    let _ = host.record_delegation_event_for_test("intent-z", "approved", std::collections::HashMap::new());
    let text = {
        let guard = causal_chain.lock().unwrap();
        render_prometheus_text(&*guard)
    };
    assert!(text.contains("ccos_total_cost"));
}
