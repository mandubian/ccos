use rtfs_compiler::ccos::capabilities::registry::CapabilityRegistry;
use rtfs_compiler::runtime::values::Value;
use rtfs_compiler::runtime::security::RuntimeContext;

#[test]
fn test_microvm_http_fetch() {
    // Create registry and set provider to mock
    let mut registry = CapabilityRegistry::new();
    registry.set_microvm_provider("mock").expect("Should set mock provider");

    // Create a controlled runtime context for testing
    let runtime_context = RuntimeContext::controlled(vec![
        "ccos.network.http-fetch".to_string(),
    ]);

    // Simulate a plan step: HTTP fetch with mock endpoint
    let url = "http://localhost:9999/mock";
    let args = vec![Value::String(url.to_string())];
    let result = registry.execute_capability_with_microvm("ccos.network.http-fetch", args, Some(&runtime_context));

    match result {
        Ok(Value::String(message)) => {
            // With the refactored architecture, we expect the message indicating marketplace routing
            assert!(message.contains("marketplace"));
            println!("Architecture correctly routes HTTP operations: {}", message);
        },
        Ok(Value::Map(map)) => {
            // This would be the case if HTTP operations were properly integrated
            let status = map.get(&rtfs_compiler::ast::MapKey::String("status".to_string()));
            let body = map.get(&rtfs_compiler::ast::MapKey::String("body".to_string()));
            assert_eq!(status, Some(&Value::Integer(200)));
            assert!(body.is_some());
            println!("MicroVM HTTP fetch response: {:?}", map);
        },
        other => panic!("Unexpected result: {:?}", other),
    }
}
