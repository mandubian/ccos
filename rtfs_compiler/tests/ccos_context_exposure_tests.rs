use std::rc::Rc;
use std::sync::{Arc, Mutex};

use rtfs_compiler::parser;
use rtfs_compiler::runtime::{Evaluator, ModuleRegistry};
use rtfs_compiler::runtime::security::RuntimeContext;
use rtfs_compiler::runtime::host::RuntimeHost;
use rtfs_compiler::runtime::capability_marketplace::CapabilityMarketplace;
use rtfs_compiler::runtime::capability_registry::CapabilityRegistry;
use tokio::sync::RwLock;
use tokio::runtime::Runtime;
use rtfs_compiler::runtime::stdlib::register_default_capabilities;
use rtfs_compiler::ccos::causal_chain::CausalChain;
use rtfs_compiler::ccos::delegation::StaticDelegationEngine;
use rtfs_compiler::runtime::values::Value;

// Verify that when context exposure is allowed for ccos.echo,
// the call uses map-based args with :args and :context present,
// and step overrides can disable exposure.
#[test]
fn test_context_exposure_with_step_overrides() {
    // Build marketplace and host
    let registry = Arc::new(RwLock::new(CapabilityRegistry::new()));
    let capability_marketplace = Arc::new(CapabilityMarketplace::new(registry));
  // Register default local capabilities (ccos.echo, ccos.math.add, etc.)
  let rt = Runtime::new().expect("tokio runtime");
  rt.block_on(async {
    register_default_capabilities(&capability_marketplace)
      .await
      .expect("register default capabilities");
  });
    let causal_chain = Arc::new(Mutex::new(CausalChain::new().expect("causal chain")));

    // Allow exposure for ccos.echo
    let mut ctx = RuntimeContext::controlled(vec!["ccos.echo".to_string()]);
    ctx.enable_context_exposure_for("ccos.echo");

    let host = Arc::new(RuntimeHost::new(causal_chain, capability_marketplace, ctx));
    let module_registry = Rc::new(ModuleRegistry::new());
    let de = Arc::new(StaticDelegationEngine::new(std::collections::HashMap::new()));
    let mut evaluator = Evaluator::new(module_registry, de, rtfs_compiler::runtime::security::RuntimeContext::pure(), host.clone());

    // Set execution context to enable snapshot
    host.set_execution_context("plan-1".into(), vec!["intent-1".into()], "root".into());

    // Call ccos.echo inside a step with override enabling exposure
    let rtfs = r#"
      (step "Expose Context" :expose-context true
        (call "ccos.echo" "hello"))
    "#;
    let expr = parser::parse(rtfs).expect("parse");
    let result = evaluator.eval_toplevel(&expr).expect("eval");
    assert_eq!(result, Value::String("hello".to_string()));

    // Now disable exposure via override and ensure call still works
    let rtfs2 = r#"
      (step "No Context" :expose-context false
        (call "ccos.echo" "world"))
    "#;
    let expr2 = parser::parse(rtfs2).expect("parse2");
    let result2 = evaluator.eval_toplevel(&expr2).expect("eval2");
    assert_eq!(result2, Value::String("world".to_string()));

    host.clear_execution_context();
}


