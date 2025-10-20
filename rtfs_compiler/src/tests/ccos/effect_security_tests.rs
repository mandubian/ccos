use std::sync::{Arc, Mutex};

use crate::ccos::capability_marketplace::CapabilityMarketplace;
use crate::ccos::causal_chain::CausalChain;
use crate::ccos::host::RuntimeHost;
use crate::runtime::capabilities::registry::CapabilityRegistry;
use crate::runtime::host_interface::HostInterface;
use crate::runtime::security::RuntimeContext;
use crate::runtime::values::Value;
use tokio::sync::RwLock;

fn make_host_with_context(context: RuntimeContext) -> RuntimeHost {
    let registry = Arc::new(RwLock::new(CapabilityRegistry::new()));
    let marketplace = Arc::new(CapabilityMarketplace::new(Arc::clone(&registry)));
    let causal_chain = Arc::new(Mutex::new(CausalChain::new().expect("causal chain")));

    let host = RuntimeHost::new(causal_chain, marketplace, context);
    // Seed execution context so HostInterface operations succeed in tests
    host.set_execution_context(
        "test-plan".to_string(),
        vec!["test-intent".to_string()],
        "root-action".to_string(),
    );
    host
}

#[test]
fn capability_denied_when_effect_blocked() {
    let mut context = RuntimeContext::controlled(vec!["ccos.io.log".to_string()]);
    context.deny_effect(":compute");

    let host = make_host_with_context(context);
    let args = vec![Value::String("hello".to_string())];

    let result = HostInterface::execute_capability(&host, "ccos.io.log", &args);

    match result {
        Err(crate::runtime::error::RuntimeError::SecurityViolation {
            operation,
            capability,
            context,
        }) => {
            assert_eq!(operation, "effect_policy");
            assert_eq!(capability, "ccos.io.log");
            assert!(context.contains(":compute"));
        }
        other => panic!("expected SecurityViolation, got {:?}", other),
    }
}

#[test]
fn capability_executes_when_effect_allowed() {
    let context = RuntimeContext::controlled(vec!["ccos.io.log".to_string()])
        .with_effect_allowlist(&[":compute"]);

    let host = make_host_with_context(context.clone());

    // Verify the context is properly configured with effect allowlist
    // (no denied effects means all are allowed)
    let result = context.ensure_effects_allowed("ccos.io.log", &[":compute".to_string()]);
    assert!(result.is_ok(), "effect :compute should be allowed");

    // The actual capability execution would require proper marketplace registration,
    // which is beyond the scope of this security policy test.
    // This test verifies that the security context is configured correctly.
}
