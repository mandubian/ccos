use rtfs_compiler::ccos::capabilities::registry::CapabilityRegistry;
use rtfs_compiler::runtime::microvm::MicroVMSettings;
use rtfs_compiler::runtime::values::Value;
use rtfs_compiler::runtime::error::RuntimeResult;
use rtfs_compiler::runtime::security::RuntimeContext;

fn main() -> RuntimeResult<()> {
    println!("=== RTFS/CCOS MicroVM Architecture Demo ===\n");
    
    // Create a capability registry
    let mut registry = CapabilityRegistry::new();
    
    // List available MicroVM providers
    println!("Available MicroVM providers:");
    for provider in registry.list_microvm_providers() {
        println!("  - {}", provider);
    }
    println!();
    
    // Set the MicroVM provider to mock for demonstration
    registry.set_microvm_provider("mock")?;
    println!("Using MicroVM provider: {:?}\n", registry.get_microvm_provider());
    
    // Load MicroVM configuration
    let settings = MicroVMSettings::default();
    println!("MicroVM Settings:");
    println!("  Default provider: {}", settings.default_provider);
    println!("  Default timeout: {:?}", settings.default_config.timeout);
    println!("  Default memory limit: {} MB", settings.default_config.memory_limit_mb);
    println!("  Default CPU limit: {:.1}%", settings.default_config.cpu_limit * 100.0);
    println!();
    
    // Demonstrate capability execution with MicroVM isolation
    println!("=== Testing Capabilities with MicroVM Isolation ===\n");
    
    // Create a controlled runtime context for testing
    let runtime_context = RuntimeContext::controlled(vec![
        "ccos.network.http-fetch".to_string(),
        "ccos.io.open-file".to_string(),
        "ccos.system.current-time".to_string(),
    ]);
    
    // Test network capability
    println!("1. Testing network capability (ccos.network.http-fetch):");
    let args = vec![Value::String("http://localhost:9999/mock".to_string())];
    match registry.execute_capability_with_microvm("ccos.network.http-fetch", args, Some(&runtime_context)) {
        Ok(result) => println!("   Result: {:?}", result),
        Err(e) => println!("   Error: {}", e),
    }
    println!();
    
    // Test file I/O capability
    println!("2. Testing file I/O capability (ccos.io.open-file):");
    let args = vec![Value::String("/tmp/test.txt".to_string()), Value::String("r".to_string())];
    match registry.execute_capability_with_microvm("ccos.io.open-file", args, Some(&runtime_context)) {
        Ok(result) => println!("   Result: {:?}", result),
        Err(e) => println!("   Error: {}", e),
    }
    println!();
    
    // Test safe capability (doesn't require MicroVM)
    println!("3. Testing safe capability (ccos.system.current-time):");
    let args = vec![];
    match registry.execute_capability_with_microvm("ccos.system.current-time", args, Some(&runtime_context)) {
        Ok(result) => println!("   Result: {:?}", result),
        Err(e) => println!("   Error: {}", e),
    }
    println!();
    
    // Show configuration for specific capabilities
    println!("=== Capability-Specific Configurations ===\n");
    
    let http_config = settings.get_config_for_capability("ccos.network.http-fetch");
    println!("HTTP Fetch Configuration:");
    println!("  Timeout: {:?}", http_config.timeout);
    println!("  Memory limit: {} MB", http_config.memory_limit_mb);
    println!("  CPU limit: {:.1}%", http_config.cpu_limit * 100.0);
    println!("  Network policy: {:?}", http_config.network_policy);
    println!("  FS policy: {:?}", http_config.fs_policy);
    println!();
    
    let file_config = settings.get_config_for_capability("ccos.io.open-file");
    println!("File I/O Configuration:");
    println!("  Timeout: {:?}", file_config.timeout);
    println!("  Memory limit: {} MB", file_config.memory_limit_mb);
    println!("  CPU limit: {:.1}%", file_config.cpu_limit * 100.0);
    println!("  Network policy: {:?}", file_config.network_policy);
    println!("  FS policy: {:?}", file_config.fs_policy);
    println!();
    
    println!("=== MicroVM Architecture Summary ===");
    println!("✓ Pluggable MicroVM providers (mock, process, firecracker, gvisor, wasm)");
    println!("✓ Capability-specific security policies");
    println!("✓ Configuration-driven isolation settings");
    println!("✓ Clean separation between runtime and isolation layer");
    println!("✓ Non-bloated integration with RTFS/CCOS");
    
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_microvm_provider_selection() {
        let mut registry = CapabilityRegistry::new();
        
        // Should default to None
        assert!(registry.get_microvm_provider().is_none());
        
        // Should be able to set to mock
        assert!(registry.set_microvm_provider("mock").is_ok());
        assert_eq!(registry.get_microvm_provider(), Some("mock"));
        
        // Should reject unknown provider
        assert!(registry.set_microvm_provider("unknown").is_err());
    }
    
    #[test]
    fn test_microvm_settings() {
        let settings = MicroVMSettings::default();
        
        // Should have default provider
        assert_eq!(settings.default_provider, "mock");
        
        // Should have capability-specific configs
        assert!(settings.capability_configs.contains_key("ccos.network.http-fetch"));
        assert!(settings.capability_configs.contains_key("ccos.io.open-file"));
        
        // Should return appropriate config for capability
        let http_config = settings.get_config_for_capability("ccos.network.http-fetch");
        assert_eq!(http_config.memory_limit_mb, 64);
        
        let unknown_config = settings.get_config_for_capability("unknown.capability");
        assert_eq!(unknown_config.memory_limit_mb, settings.default_config.memory_limit_mb);
    }
}
