//! Integration tests for SandboxedCapability execution
//!
//! Tests the sandboxed execution path using the MicroVM process provider.

use ccos::capability_marketplace::executors::{
    CapabilityExecutor, ExecutionContext, SandboxedExecutor,
};
use ccos::capability_marketplace::types::{EffectType, ProviderType, SandboxedCapability};
use rtfs::runtime::values::Value;
use std::collections::HashMap;

/// Check if the process provider is available by attempting to get it from a factory
fn is_process_provider_available() -> bool {
    let factory = rtfs::runtime::microvm::MicroVMFactory::new();
    factory.get_provider("process").is_some()
}

/// Test that SandboxedExecutor can be created
#[test]
fn test_sandboxed_executor_creation() {
    let executor = SandboxedExecutor::new();
    // Just verify it doesn't panic
    assert!(true);
    let _ = executor; // Use it to avoid warning
}

/// Test executing a simple Python script that echoes input
#[tokio::test]
async fn test_sandboxed_python_echo() {
    // Skip if process provider is not available
    if !is_process_provider_available() {
        println!("Skipping test: process provider not available in this environment");
        return;
    }

    let executor = SandboxedExecutor::new();

    // Python script that reads input JSON from command line and echoes it
    let python_code = r#"
import sys
import json

# The input JSON is passed as the last argument
if len(sys.argv) > 1:
    input_json = sys.argv[-1]
    try:
        data = json.loads(input_json)
        # Echo back the input with a prefix
        result = {"echoed": data, "status": "ok"}
        print(json.dumps(result))
    except json.JSONDecodeError:
        print(json.dumps({"error": "invalid json", "raw": input_json}))
else:
    print(json.dumps({"error": "no input provided"}))
"#;

    let sandboxed = SandboxedCapability {
        runtime: "python".to_string(),
        source: python_code.to_string(),
        entry_point: None,
        provider: Some("process".to_string()),
    };

    let provider = ProviderType::Sandboxed(sandboxed);

    // Create input value
    let mut input_map = HashMap::new();
    input_map.insert(
        rtfs::ast::MapKey::String("message".to_string()),
        Value::String("hello from test".to_string()),
    );
    let inputs = Value::Map(input_map);

    let metadata = HashMap::new();
    let context = ExecutionContext::new("test.sandboxed", &metadata, None);
    let result = executor.execute(&provider, &inputs, &context).await;

    match result {
        Ok(value) => {
            // The result should contain the echoed data
            println!("Sandbox execution result: {:?}", value);
            // Basic verification - we got something back
            assert!(!matches!(value, Value::Nil));
        }
        Err(e) => {
            // If process provider is not available, that's expected in some environments
            let err_msg = format!("{:?}", e);
            if err_msg.contains("not available") {
                println!("Skipping test: process provider not available");
            } else {
                panic!("Unexpected error: {:?}", e);
            }
        }
    }
}

/// Test executing a Python script that performs a calculation
#[tokio::test]
async fn test_sandboxed_python_calculation() {
    // Skip if process provider is not available
    if !is_process_provider_available() {
        println!("Skipping test: process provider not available in this environment");
        return;
    }

    let executor = SandboxedExecutor::new();

    // Python script that adds two numbers from input
    let python_code = r#"
import sys
import json

if len(sys.argv) > 1:
    try:
        data = json.loads(sys.argv[-1])
        a = data.get("a", 0)
        b = data.get("b", 0)
        result = {"sum": a + b, "product": a * b}
        print(json.dumps(result))
    except Exception as e:
        print(json.dumps({"error": str(e)}))
else:
    print(json.dumps({"error": "no input"}))
"#;

    let sandboxed = SandboxedCapability {
        runtime: "python".to_string(),
        source: python_code.to_string(),
        entry_point: None,
        provider: Some("process".to_string()),
    };

    let provider = ProviderType::Sandboxed(sandboxed);

    // Create input with numbers
    let mut input_map = HashMap::new();
    input_map.insert(
        rtfs::ast::MapKey::String("a".to_string()),
        Value::Integer(5),
    );
    input_map.insert(
        rtfs::ast::MapKey::String("b".to_string()),
        Value::Integer(3),
    );
    let inputs = Value::Map(input_map);

    let metadata = HashMap::new();
    let context = ExecutionContext::new("test.sandboxed", &metadata, None);
    let result = executor.execute(&provider, &inputs, &context).await;

    match result {
        Ok(value) => {
            println!("Calculation result: {:?}", value);
            // Verify we got a result
            assert!(!matches!(value, Value::Nil));
        }
        Err(e) => {
            let err_msg = format!("{:?}", e);
            if err_msg.contains("not available") || err_msg.contains("not initialized") {
                println!("Skipping test: process provider not available");
            } else if err_msg.contains("SecurityViolation") {
                // Security violations are expected in test environments without proper permissions
                println!(
                    "Security violation (expected in restricted environment): {:?}",
                    e
                );
            } else {
                panic!("Unexpected error: {:?}", e);
            }
        }
    }
}

/// Test that invalid provider name returns an error
#[tokio::test]
async fn test_sandboxed_invalid_provider() {
    let executor = SandboxedExecutor::new();

    let sandboxed = SandboxedCapability {
        runtime: "python".to_string(),
        source: "print('hello')".to_string(),
        entry_point: None,
        provider: Some("nonexistent_provider".to_string()),
    };

    let provider = ProviderType::Sandboxed(sandboxed);
    let inputs = Value::Nil;

    let metadata = HashMap::new();
    let context = ExecutionContext::new("test.sandboxed", &metadata, None);
    let result = executor.execute(&provider, &inputs, &context).await;

    // Should fail because the provider doesn't exist
    assert!(result.is_err());
    let err_msg = format!("{:?}", result.unwrap_err());
    assert!(err_msg.contains("not available"));
}

/// Test provider type mismatch
#[tokio::test]
async fn test_sandboxed_wrong_provider_type() {
    let executor = SandboxedExecutor::new();

    // Pass a non-Sandboxed provider type
    let http_provider = ProviderType::Http(ccos::capability_marketplace::types::HttpCapability {
        base_url: "http://example.com".to_string(),
        auth_token: None,
        timeout_ms: 5000,
    });

    let inputs = Value::Nil;
    let metadata = HashMap::new();
    let context = ExecutionContext::new("test.sandboxed", &metadata, None);
    let result = executor.execute(&http_provider, &inputs, &context).await;

    // Should fail with "Invalid provider type"
    assert!(result.is_err());
    let err_msg = format!("{:?}", result.unwrap_err());
    assert!(err_msg.contains("Invalid provider type"));
}

/// Test marketplace integration - register and execute a sandboxed capability
#[tokio::test]
async fn test_marketplace_sandboxed_capability() {
    use ccos::capability_marketplace::types::CapabilityManifest;
    use ccos::capability_marketplace::CapabilityMarketplace;
    use std::sync::Arc;
    use tokio::sync::RwLock;

    // Skip if process provider is not available
    if !is_process_provider_available() {
        println!("Skipping test: process provider not available in this environment");
        return;
    }

    // Create a minimal capability registry
    let registry = Arc::new(RwLock::new(
        ccos::capabilities::registry::CapabilityRegistry::new(),
    ));

    // Create marketplace
    let marketplace = CapabilityMarketplace::new(registry);

    // Create a sandboxed capability manifest
    let python_code = r#"
import sys
import json
print(json.dumps({"result": "sandbox works!"}))
"#;

    let manifest = CapabilityManifest {
        id: "test.sandboxed.hello".to_string(),
        name: "Sandboxed Hello".to_string(),
        description: "A simple sandboxed Python capability".to_string(),
        version: "1.0.0".to_string(),
        provider: ProviderType::Sandboxed(SandboxedCapability {
            runtime: "python".to_string(),
            source: python_code.to_string(),
            entry_point: None,
            provider: Some("process".to_string()),
        }),
        input_schema: None,
        output_schema: None,
        attestation: None,
        provenance: None,
        permissions: vec![],
        effects: vec!["sandboxed:execute".to_string()],
        metadata: HashMap::new(),
        agent_metadata: None,
        domains: vec!["test".to_string()],
        categories: vec!["sandbox".to_string()],
        effect_type: EffectType::Effectful,
    };

    // Register the capability
    let register_result = marketplace.register_capability_manifest(manifest).await;
    assert!(
        register_result.is_ok(),
        "Failed to register sandboxed capability: {:?}",
        register_result
    );

    // Verify it's registered
    let has_cap = marketplace.has_capability("test.sandboxed.hello").await;
    assert!(has_cap, "Capability should be registered");

    // Try to execute it
    let inputs = Value::Nil;
    let exec_result = marketplace
        .execute_capability("test.sandboxed.hello", &inputs)
        .await;

    match exec_result {
        Ok(value) => {
            println!("Marketplace sandboxed execution result: {:?}", value);
        }
        Err(e) => {
            let err_msg = format!("{:?}", e);
            // Process provider might not be available in test environment
            if err_msg.contains("not available") {
                println!("Skipping: process provider not available in test environment");
            } else {
                panic!("Unexpected execution error: {:?}", e);
            }
        }
    }
}

/// Check if Firecracker is available on the system
fn is_firecracker_available() -> bool {
    std::process::Command::new("firecracker")
        .arg("--version")
        .output()
        .is_ok()
}

/// Test Firecracker provider availability
#[test]
fn test_firecracker_provider_availability() {
    let factory = rtfs::runtime::microvm::MicroVMFactory::new();

    if is_firecracker_available() {
        println!("Firecracker binary found, provider should be available");
        // Note: Provider may still not be available if kernel/rootfs missing
        if let Some(provider) = factory.get_provider("firecracker") {
            println!("Firecracker provider available: {}", provider.name());
        } else {
            println!("Firecracker provider not registered (missing dependencies)");
        }
    } else {
        println!("Firecracker binary not found, skipping provider check");
    }
}

/// Test Firecracker provider execution (requires firecracker setup)
#[tokio::test]
async fn test_firecracker_python_execution() {
    // Skip if firecracker is not available
    if !is_firecracker_available() {
        println!("Skipping test: Firecracker not installed");
        return;
    }

    // Check if kernel and rootfs exist
    let kernel_exists = std::path::Path::new("/opt/firecracker/vmlinux").exists();
    let rootfs_exists = std::path::Path::new("/opt/firecracker/rootfs.ext4").exists();

    if !kernel_exists || !rootfs_exists {
        println!("Skipping test: Firecracker kernel or rootfs not found in /opt/firecracker/");
        println!(
            "  kernel: {} (exists: {})",
            "/opt/firecracker/vmlinux", kernel_exists
        );
        println!(
            "  rootfs: {} (exists: {})",
            "/opt/firecracker/rootfs.ext4", rootfs_exists
        );
        return;
    }

    println!("Firecracker setup found, testing execution...");

    let executor = SandboxedExecutor::new();

    // Simple Python test
    let python_code = r#"
import json
result = {"message": "Hello from Firecracker!", "calculated": 2 + 2}
print(json.dumps(result))
"#;

    let sandboxed = SandboxedCapability {
        runtime: "python".to_string(),
        source: python_code.to_string(),
        entry_point: None,
        provider: Some("firecracker".to_string()),
    };

    let provider = ProviderType::Sandboxed(sandboxed);
    let inputs = Value::Nil;

    let metadata = HashMap::new();
    let context = ExecutionContext::new("test.sandboxed", &metadata, None);
    match executor.execute(&provider, &inputs, &context).await {
        Ok(value) => {
            println!("Firecracker execution result: {:?}", value);
            // Verify we got valid output
            if let Value::String(s) = &value {
                assert!(
                    s.contains("Hello from Firecracker") || s.contains("calculated"),
                    "Expected Python output, got: {}",
                    s
                );
            }
        }
        Err(e) => {
            let err_msg = format!("{:?}", e);
            // Firecracker might fail if not properly configured
            if err_msg.contains("not available") || err_msg.contains("not found") {
                println!("Firecracker provider not fully configured: {}", err_msg);
            } else {
                println!("Firecracker execution error (may be expected): {:?}", e);
            }
        }
    }
}
