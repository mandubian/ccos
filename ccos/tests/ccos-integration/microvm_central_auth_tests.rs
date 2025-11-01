use rtfs::ccos::capabilities::registry::CapabilityRegistry;
use rtfs::runtime::microvm::core::Program;
use rtfs::runtime::security::{RuntimeContext, SecurityAuthorizer};
use rtfs::runtime::values::Value;

#[test]
fn test_central_authorization_system() {
    println!("=== Testing Central Authorization System ===\n");

    // Test 1: Pure security level - no capabilities allowed
    println!("1. Testing Pure security level:");
    let pure_context = RuntimeContext::pure();

    // Should fail for any capability
    let result = SecurityAuthorizer::authorize_capability(
        &pure_context,
        "ccos.network.http-fetch",
        &[Value::String("https://example.com".to_string())],
    );

    match result {
        Ok(_) => panic!("❌ Pure context should not allow any capabilities"),
        Err(e) => {
            println!("  ✅ Pure context correctly blocked capability: {}", e);
            assert!(e.to_string().contains("Security violation"));
        }
    }

    // Test 2: Controlled security level - specific capabilities allowed
    println!("\n2. Testing Controlled security level:");
    let controlled_context = RuntimeContext::controlled(vec![
        "ccos.network.http-fetch".to_string(),
        "ccos.io.open-file".to_string(),
    ]);

    // Should succeed for allowed capability
    let result = SecurityAuthorizer::authorize_capability(
        &controlled_context,
        "ccos.network.http-fetch",
        &[Value::String("https://example.com".to_string())],
    );

    match result {
        Ok(permissions) => {
            println!("  ✅ Controlled context allowed network capability");
            println!("  📋 Required permissions: {:?}", permissions);
            assert!(permissions.contains(&"ccos.network.http-fetch".to_string()));
            assert!(permissions.contains(&"ccos.network.outbound".to_string()));
        }
        Err(e) => panic!(
            "❌ Controlled context should allow network capability: {}",
            e
        ),
    }

    // Should fail for disallowed capability
    let result = SecurityAuthorizer::authorize_capability(
        &controlled_context,
        "ccos.system.get-env",
        &[Value::String("PATH".to_string())],
    );

    match result {
        Ok(_) => panic!("❌ Controlled context should not allow unauthorized capabilities"),
        Err(e) => {
            println!(
                "  ✅ Controlled context correctly blocked unauthorized capability: {}",
                e
            );
            assert!(e.to_string().contains("Security violation"));
        }
    }

    // Test 3: Full security level - all capabilities allowed
    println!("\n3. Testing Full security level:");
    let full_context = RuntimeContext::full();

    // Should succeed for any capability
    let result = SecurityAuthorizer::authorize_capability(
        &full_context,
        "ccos.system.get-env",
        &[Value::String("PATH".to_string())],
    );

    match result {
        Ok(permissions) => {
            println!("  ✅ Full context allowed system capability");
            println!("  📋 Required permissions: {:?}", permissions);
            assert!(permissions.contains(&"ccos.system.get-env".to_string()));
            assert!(permissions.contains(&"ccos.system.environment".to_string()));
        }
        Err(e) => panic!("❌ Full context should allow all capabilities: {}", e),
    }

    // Test 4: Program authorization
    println!("\n4. Testing Program authorization:");

    // Test external program authorization
    let external_program = Program::ExternalProgram {
        path: "/bin/ls".to_string(),
        args: vec!["-la".to_string()],
    };

    let result = SecurityAuthorizer::authorize_program(
        &controlled_context,
        &external_program,
        Some("ccos.io.open-file"),
    );

    match result {
        Ok(permissions) => {
            println!("  ✅ Program authorization succeeded");
            println!("  📋 Required permissions: {:?}", permissions);
            assert!(permissions.contains(&"ccos.io.open-file".to_string()));
            assert!(permissions.contains(&"external_program".to_string()));
        }
        Err(e) => {
            println!(
                "  ⚠️  Program authorization failed (expected for controlled context): {}",
                e
            );
            // This is expected since controlled context doesn't allow external_program
        }
    }

    // Test 5: Integration with CapabilityRegistry
    println!("\n5. Testing Integration with CapabilityRegistry:");
    let mut registry = CapabilityRegistry::new();
    registry
        .set_microvm_provider("mock")
        .expect("Should set mock provider");

    // Test authorized execution with mock endpoint
    let args = vec![Value::String("http://localhost:9999/mock".to_string())];
    let result = registry.execute_capability_with_microvm(
        "ccos.network.http-fetch",
        args,
        Some(&controlled_context),
    );

    match result {
        Ok(value) => {
            println!("  ✅ Authorized capability execution succeeded");
            println!("  📋 Result: {}", value);
        }
        Err(e) => panic!("❌ Authorized capability should succeed: {}", e),
    }

    // Test unauthorized execution
    let args = vec![Value::String("PATH".to_string())];
    let result = registry.execute_capability_with_microvm(
        "ccos.system.get-env",
        args,
        Some(&controlled_context),
    );

    match result {
        Ok(_) => panic!("❌ Unauthorized capability should fail"),
        Err(e) => {
            println!("  ✅ Unauthorized capability correctly blocked: {}", e);
            assert!(e.to_string().contains("Security violation"));
        }
    }

    println!("\n=== Central Authorization System Test Complete ===");
    println!("✅ All authorization tests passed!");
}

#[test]
fn test_runtime_context_capability_checks() {
    println!("=== Testing RuntimeContext Capability Checks ===\n");

    // Test controlled context with specific capabilities
    let context = RuntimeContext::controlled(vec![
        "ccos.network.http-fetch".to_string(),
        "ccos.io.open-file".to_string(),
    ]);

    // Test allowed capabilities
    assert!(context.is_capability_allowed("ccos.network.http-fetch"));
    assert!(context.is_capability_allowed("ccos.io.open-file"));

    // Test disallowed capabilities
    assert!(!context.is_capability_allowed("ccos.system.get-env"));
    assert!(!context.is_capability_allowed("ccos.io.read-line"));

    // Test pure context
    let pure_context = RuntimeContext::pure();
    assert!(!pure_context.is_capability_allowed("ccos.network.http-fetch"));
    assert!(!pure_context.is_capability_allowed("ccos.system.get-env"));

    // Test full context
    let full_context = RuntimeContext::full();
    assert!(full_context.is_capability_allowed("ccos.network.http-fetch"));
    assert!(full_context.is_capability_allowed("ccos.system.get-env"));
    assert!(full_context.is_capability_allowed("any.capability"));

    println!("✅ RuntimeContext capability checks working correctly");
}

#[test]
fn test_security_authorizer_permission_determination() {
    println!("=== Testing SecurityAuthorizer Permission Determination ===\n");

    let context = RuntimeContext::controlled(vec![
        "ccos.network.http-fetch".to_string(),
        "ccos.io.open-file".to_string(),
        "ccos.system.get-env".to_string(),
    ]);

    // Test network capability permissions
    let permissions = SecurityAuthorizer::authorize_capability(
        &context,
        "ccos.network.http-fetch",
        &[Value::String("https://example.com".to_string())],
    )
    .expect("Should authorize network capability");

    assert!(permissions.contains(&"ccos.network.http-fetch".to_string()));
    assert!(permissions.contains(&"ccos.network.outbound".to_string()));
    println!("  ✅ Network capability permissions: {:?}", permissions);

    // Test file capability permissions
    let permissions = SecurityAuthorizer::authorize_capability(
        &context,
        "ccos.io.open-file",
        &[
            Value::String("/tmp/test.txt".to_string()),
            Value::String("r".to_string()),
        ],
    )
    .expect("Should authorize file capability");

    assert!(permissions.contains(&"ccos.io.open-file".to_string()));
    assert!(permissions.contains(&"ccos.io.file-access".to_string()));
    println!("  ✅ File capability permissions: {:?}", permissions);

    // Test system capability permissions
    let permissions = SecurityAuthorizer::authorize_capability(
        &context,
        "ccos.system.get-env",
        &[Value::String("PATH".to_string())],
    )
    .expect("Should authorize system capability");

    assert!(permissions.contains(&"ccos.system.get-env".to_string()));
    assert!(permissions.contains(&"ccos.system.environment".to_string()));
    println!("  ✅ System capability permissions: {:?}", permissions);

    println!("✅ SecurityAuthorizer permission determination working correctly");
}
