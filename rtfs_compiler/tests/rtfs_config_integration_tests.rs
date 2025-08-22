//! RTFS Configuration Integration Tests
//! 
//! This module tests the integration between RTFS parsing and agent configuration,
//! specifically validating the (agent.config ...) form parsing and conversion.

use rtfs_compiler::config::AgentConfigParser;
use rtfs_compiler::parser::parse_expression;
use rtfs_compiler::runtime::error::{RuntimeError, RuntimeResult};

#[test]
fn test_rtfs_agent_config_parsing() -> RuntimeResult<()> {
    println!("=== Testing RTFS Agent Config Parsing ===\n");

    // Test 1: Basic agent config parsing
    println!("1. Testing basic agent config parsing:");
    let config_content = r#"
    (agent.config
      :version "0.1"
      :agent-id "agent.test"
      :profile :microvm)
    "#;
    
    let expr = parse_expression(config_content.trim())
        .map_err(|e| RuntimeError::Generic(format!("Parse error: {:?}", e)))?;
    let config = AgentConfigParser::extract_agent_config_from_expression(&expr)?;
    
    assert_eq!(config.version, "0.1");
    assert_eq!(config.agent_id, "agent.test");
    assert_eq!(config.profile, "microvm");
    println!("  ✅ Basic config parsed successfully");

    // Test 2: Complex agent config with all fields
    println!("\n2. Testing complex agent config parsing:");
    let complex_config = r#"
    (agent.config
      :version "1.0"
      :agent-id "agent.complex"
      :profile :microvm
      :features [:network :telemetry :attestation]
      :orchestrator
        {:isolation
          {:mode :microvm
           :fs {:ephemeral true
                :mounts {:capabilities {:mode :ro}
                         :data {:mode :rw}}}}}
      :network
        {:enabled true
         :egress {:via :proxy
                  :allow-domains ["example.com" "api.github.com"]
                  :mtls true}}
      :microvm
        {:kernel {:image "kernels/vmlinuz-min"
                  :cmdline "console=ttyS0 root=/dev/vda1"}
         :rootfs {:image "rootfs.img"
                  :ro true}
         :resources {:vcpus 4
                     :mem-mb 1024}
         :devices {:nic {:enabled true
                        :proxy-ns "proxy"}
                  :vsock {:enabled true
                          :cid 3}}
         :attestation {:enabled true
                      :expect-rootfs-hash "sha256:abc123"}})
    "#;
    
    let expr = parse_expression(complex_config.trim())
        .map_err(|e| RuntimeError::Generic(format!("Parse error: {:?}", e)))?;
    let config = AgentConfigParser::extract_agent_config_from_expression(&expr)?;
    
    assert_eq!(config.version, "1.0");
    assert_eq!(config.agent_id, "agent.complex");
    assert_eq!(config.profile, "microvm");
    assert_eq!(config.features, vec!["network", "telemetry", "attestation"]);
    
    // Check orchestrator config
    assert_eq!(config.orchestrator.isolation.mode, "microvm");
    assert_eq!(config.orchestrator.isolation.fs.ephemeral, true);
    
    // Check network config
    assert_eq!(config.network.enabled, true);
    assert_eq!(config.network.egress.via, "proxy");
    assert_eq!(config.network.egress.allow_domains, vec!["example.com", "api.github.com"]);
    assert_eq!(config.network.egress.mtls, true);
    
    // Check microvm config
    let microvm = config.microvm.unwrap();
    assert_eq!(microvm.kernel.image, "kernels/vmlinuz-min");
    assert_eq!(microvm.kernel.cmdline, "console=ttyS0 root=/dev/vda1");
    assert_eq!(microvm.rootfs.image, "rootfs.img");
    assert_eq!(microvm.rootfs.ro, true);
    assert_eq!(microvm.resources.vcpus, 4);
    assert_eq!(microvm.resources.mem_mb, 1024);
    assert_eq!(microvm.devices.nic.enabled, true);
    assert_eq!(microvm.devices.nic.proxy_ns, "proxy");
    assert_eq!(microvm.devices.vsock.enabled, true);
    assert_eq!(microvm.devices.vsock.cid, 3);
    assert_eq!(microvm.attestation.enabled, true);
    assert_eq!(microvm.attestation.expect_rootfs_hash, "sha256:abc123");
    println!("  ✅ Complex config parsed successfully");

    // Test 3: Minimal agent config
    println!("\n3. Testing minimal agent config:");
    let minimal_config = r#"
    (agent.config
      :version "0.1"
      :agent-id "agent.minimal"
      :profile :minimal)
    "#;
    
    let expr = parse_expression(minimal_config.trim())
        .map_err(|e| RuntimeError::Generic(format!("Parse error: {:?}", e)))?;
    let config = AgentConfigParser::extract_agent_config_from_expression(&expr)?;
    
    assert_eq!(config.version, "0.1");
    assert_eq!(config.agent_id, "agent.minimal");
    assert_eq!(config.profile, "minimal");
    
    // Should have defaults for other fields
    assert_eq!(config.orchestrator.isolation.mode, "wasm");
    assert_eq!(config.network.enabled, false);
    assert_eq!(config.microvm, None);
    println!("  ✅ Minimal config parsed successfully");

    // Test 4: Agent config with different profile types
    println!("\n4. Testing different profile types:");
    let profiles = vec!["minimal", "microvm", "wasm", "process"];
    
    for profile in profiles {
        let profile_config = format!(
            r#"
            (agent.config
              :version "0.1"
              :agent-id "agent.{}"
              :profile :{})
            "#,
            profile, profile
        );
        
        let expr = parse_expression(profile_config.trim())
            .map_err(|e| RuntimeError::Generic(format!("Parse error: {:?}", e)))?;
        let config = AgentConfigParser::extract_agent_config_from_expression(&expr)?;
        
        assert_eq!(config.profile, profile);
        println!("  ✅ Profile '{}' parsed successfully", profile);
    }

    println!("\n=== RTFS Agent Config Parsing Tests Complete ===");
    println!("✅ All RTFS configuration parsing tests passed!");
    Ok(())
}

#[test]
fn test_rtfs_config_error_handling() -> RuntimeResult<()> {
    println!("=== Testing RTFS Config Error Handling ===\n");

    // Test 1: Invalid agent.config form
    println!("1. Testing invalid agent.config form:");
    let invalid_config = r#"
    (invalid.config
      :version "0.1"
      :agent-id "agent.test")
    "#;
    
    let expr = parse_expression(invalid_config.trim())
        .map_err(|e| RuntimeError::Generic(format!("Parse error: {:?}", e)))?;
    let result = AgentConfigParser::extract_agent_config_from_expression(&expr);
    
    match result {
        Ok(_) => panic!("❌ Should have failed for invalid config form"),
        Err(e) => {
            println!("  ✅ Correctly rejected invalid config: {}", e);
            assert!(e.to_string().contains("Expected agent.config"));
        }
    }

    // Test 2: Missing required fields
    println!("\n2. Testing missing required fields:");
    let incomplete_config = r#"
    (agent.config
      :version "0.1")
    "#;
    
    let expr = parse_expression(incomplete_config.trim())
        .map_err(|e| RuntimeError::Generic(format!("Parse error: {:?}", e)))?;
    let result = AgentConfigParser::extract_agent_config_from_expression(&expr);
    
    match result {
        Ok(config) => {
            // Should use defaults for missing fields
            assert_eq!(config.version, "0.1");
            assert_eq!(config.agent_id, "agent.default");
            assert_eq!(config.profile, "minimal");
            println!("  ✅ Used defaults for missing fields");
        },
        Err(e) => {
            println!("  ❌ Unexpected error: {}", e);
            return Err(e);
        }
    }

    // Test 3: Invalid expression type
    println!("\n3. Testing invalid expression type:");
    let invalid_expr = r#"42"#; // Just a number, not a function call
    
    let expr = parse_expression(invalid_expr)
        .map_err(|e| RuntimeError::Generic(format!("Parse error: {:?}", e)))?;
    let result = AgentConfigParser::extract_agent_config_from_expression(&expr);
    
    match result {
        Ok(_) => panic!("❌ Should have failed for invalid expression type"),
        Err(e) => {
            println!("  ✅ Correctly rejected invalid expression: {}", e);
            assert!(e.to_string().contains("Expected agent.config"));
        }
    }

    println!("\n=== RTFS Config Error Handling Tests Complete ===");
    println!("✅ All error handling tests passed!");
    Ok(())
}

#[test]
fn test_rtfs_config_integration_with_microvm() -> RuntimeResult<()> {
    println!("=== Testing RTFS Config Integration with MicroVM ===\n");

    // Test 1: MicroVM-specific configuration
    println!("1. Testing MicroVM-specific configuration:");
    let microvm_config = r#"
    (agent.config
      :version "1.0"
      :agent-id "agent.microvm"
      :profile :microvm
      :features [:network :attestation]
      :microvm
        {:kernel {:image "kernels/vmlinuz-min"
                  :cmdline "console=ttyS0"}
         :rootfs {:image "rootfs.img"
                  :ro true}
         :resources {:vcpus 2
                     :mem-mb 512}
         :devices {:nic {:enabled true
                        :proxy-ns "proxy"}
                  :vsock {:enabled true
                          :cid 3}}
         :attestation {:enabled true
                      :expect-rootfs-hash "sha256:abc123"}})
    "#;
    
    let expr = parse_expression(microvm_config.trim())
        .map_err(|e| RuntimeError::Generic(format!("Parse error: {:?}", e)))?;
    let config = AgentConfigParser::extract_agent_config_from_expression(&expr)?;
    
    // Validate MicroVM-specific fields
    assert_eq!(config.profile, "microvm");
    assert!(config.features.contains(&"network".to_string()));
    assert!(config.features.contains(&"attestation".to_string()));
    
    let microvm = config.microvm.unwrap();
    assert_eq!(microvm.kernel.image, "kernels/vmlinuz-min");
    assert_eq!(microvm.kernel.cmdline, "console=ttyS0");
    assert_eq!(microvm.rootfs.image, "rootfs.img");
    assert_eq!(microvm.rootfs.ro, true);
    assert_eq!(microvm.resources.vcpus, 2);
    assert_eq!(microvm.resources.mem_mb, 512);
    assert_eq!(microvm.devices.nic.enabled, true);
    assert_eq!(microvm.devices.nic.proxy_ns, "proxy");
    assert_eq!(microvm.devices.vsock.enabled, true);
    assert_eq!(microvm.devices.vsock.cid, 3);
    assert_eq!(microvm.attestation.enabled, true);
    assert_eq!(microvm.attestation.expect_rootfs_hash, "sha256:abc123");
    println!("  ✅ MicroVM configuration parsed successfully");

    // Test 2: Network configuration validation
    println!("\n2. Testing network configuration validation:");
    // Network settings use defaults since not specified in the config
    assert_eq!(config.network.enabled, false);
    assert_eq!(config.network.egress.via, "none");
    assert_eq!(config.network.egress.allow_domains, Vec::<String>::new());
    assert_eq!(config.network.egress.mtls, false);
    println!("  ✅ Network configuration validated");

    // Test 3: Orchestrator configuration validation
    println!("\n3. Testing orchestrator configuration validation:");
    // Orchestrator settings use defaults since not specified in the config
    assert_eq!(config.orchestrator.isolation.mode, "wasm");
    assert_eq!(config.orchestrator.isolation.fs.ephemeral, false);
    println!("  ✅ Orchestrator configuration validated");

    println!("\n=== RTFS Config Integration Tests Complete ===");
    println!("✅ All integration tests passed!");
    Ok(())
}
