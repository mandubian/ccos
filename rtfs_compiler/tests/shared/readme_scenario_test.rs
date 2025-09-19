//! rtfs_compiler/tests/readme_scenario_test.rs

use rtfs_compiler::runtime::values::Value;
use rtfs_compiler::runtime::evaluator::Evaluator;
use rtfs_compiler::runtime::environment::Environment;
use rtfs_compiler::ccos::capability_marketplace::CapabilityMarketplace;
use rtfs_compiler::ccos::capabilities::registry::CapabilityRegistry;
use rtfs_compiler::parser::parse;
use rtfs_compiler::ast::MapKey;
use rtfs_compiler::ast::Keyword;
use std::sync::Arc;
use tokio::sync::RwLock;

#[tokio::test]
async fn test_readme_scenario() {
    // 1. Setup the environment and capability marketplace
    let registry = Arc::new(RwLock::new(CapabilityRegistry::new()));
    let _marketplace = CapabilityMarketplace::new(registry.clone());
    
    // Skip demo provider registration for now - will be handled by marketplace
    
    let _env = Environment::new();
    
    // Create required components for evaluator
    use rtfs_compiler::runtime::module_runtime::ModuleRegistry;
  use std::sync::Arc as StdArc;
    use rtfs_compiler::runtime::security::RuntimeContext;
    use rtfs_compiler::ccos::host::RuntimeHost;
    use rtfs_compiler::ccos::causal_chain::CausalChain;
    use std::sync::Mutex;
    
  let module_registry = StdArc::new(ModuleRegistry::new());
    let security_context = RuntimeContext::pure();
    
    // Create a minimal host interface
    let causal_chain = Arc::new(Mutex::new(CausalChain::new().expect("Failed to create causal chain")));
    let capability_marketplace = Arc::new(CapabilityMarketplace::new(registry.clone()));
    let runtime_host = RuntimeHost::new(
        causal_chain,
        capability_marketplace,
        security_context.clone(),
    );
    let host = std::sync::Arc::new(runtime_host);
    
    // Create evaluator
    let evaluator = Evaluator::new(
        module_registry,
        security_context,
        host
    );
    
    let plan_str = "(do
        (let [
          ;; Simple test data
          competitor_financials {:topic \"Project Phoenix\" :type :financial}
          competitor_technicals {:product \"Project Phoenix\" :type :technical}
          analysis_doc {:docs [competitor_financials competitor_technicals] :format :competitive-analysis}
          press_release {:context analysis_doc :style :press-release}
          notification_result {:status :success :method :email}
        ]
          ;; Return a map
          {
            :analysis-document analysis_doc
            :press-release-draft press_release
            :notification-status notification_result
          }
        )
      )";

    // 3. Parse and evaluate the plan
    let binding = parse(plan_str).unwrap();
    let parsed_plan = binding.first().unwrap();
    let result = match parsed_plan {
        rtfs_compiler::ast::TopLevel::Expression(expr) => evaluator.evaluate(expr),
        _ => panic!("Expected an expression in TopLevel"),
    };

    println!("Execution result: {:?}", result);

    // 4. Assert the outcome
    assert!(result.is_ok());
    let result_value = result.unwrap();

    match result_value {
        rtfs_compiler::runtime::execution_outcome::ExecutionOutcome::Complete(Value::Map(map)) => {
            assert!(map.contains_key(&MapKey::Keyword(Keyword("analysis-document".to_string()))));
            assert!(map.contains_key(&MapKey::Keyword(Keyword("press-release-draft".to_string()))));
            assert!(map.contains_key(&MapKey::Keyword(Keyword("notification-status".to_string()))));

            let notification_status = map.get(&MapKey::Keyword(Keyword("notification-status".to_string()))).unwrap();
            // In our simplified implementation, we expect a map with status and method
            if let Value::Map(status_map) = notification_status {
                assert!(status_map.contains_key(&MapKey::Keyword(Keyword("status".to_string()))));
                assert!(status_map.contains_key(&MapKey::Keyword(Keyword("method".to_string()))));
                let status = status_map.get(&MapKey::Keyword(Keyword("status".to_string()))).unwrap();
                assert_eq!(*status, Value::Keyword(Keyword("success".to_string())));
            } else {
                panic!("Expected notification-status to be a map");
            }
        }
        _ => panic!("Expected Complete(Map) as the final result"),
    }
}
