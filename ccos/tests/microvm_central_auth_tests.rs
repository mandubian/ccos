use ccos::capabilities::registry::CapabilityRegistry;
use rtfs::runtime::security::RuntimeContext;
use rtfs::runtime::values::Value;

#[test]
fn test_http_fetch_proxy_metadata_allowlist_enforced() {
    let mut registry = CapabilityRegistry::new();
    registry.set_http_mocking_enabled(false);

    let mut context = RuntimeContext::controlled(vec!["ccos.network.http-fetch".to_string()]);
    context.cross_plan_params.insert(
        "sandbox_allowed_hosts".to_string(),
        Value::String("allowed.example".to_string()),
    );

    let args = vec![Value::String("https://example.com".to_string())];
    let result = registry.execute_capability_with_microvm(
        "ccos.network.http-fetch",
        args,
        Some(&context),
    );

    match result {
        Ok(_) => panic!("Expected allowlist enforcement to block host"),
        Err(e) => {
            let msg = e.to_string();
            assert!(msg.contains("HTTP allowlist"));
        }
    }
}
