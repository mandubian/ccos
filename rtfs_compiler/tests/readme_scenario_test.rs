//! rtfs_compiler/tests/readme_scenario_test.rs

use rtfs_compiler::runtime::values::Value;
use rtfs_compiler::runtime::evaluator::Evaluator;
use rtfs_compiler::runtime::environment::Environment;
use rtfs_compiler::runtime::capability_marketplace::CapabilityMarketplace;
use rtfs_compiler::runtime::capability_registry::CapabilityRegistry;
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
    use rtfs_compiler::ccos::delegation::StaticDelegationEngine;
    use rtfs_compiler::runtime::host::RuntimeHost;
    use rtfs_compiler::ccos::causal_chain::CausalChain;
    use std::sync::Mutex;
    
  let module_registry = StdArc::new(ModuleRegistry::new());
    let delegation_engine = Arc::new(StaticDelegationEngine::new(std::collections::HashMap::new()));
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
        delegation_engine,
        security_context,
        host
    );
    
    let plan_str = "(plan
      :type :rtfs.core:v2.0:plan
      :plan-id \"plan-e5f8-1a3c-6b7d\"
      :intent-ids [\"intent-2a7d-4b8e-9c1f\"]
      :program (do
        (let [
          ;; Step 1: Gather Intelligence Data
          competitor_financials (step \"Gather Financial Data\"
            (call :com.bizdata.eu:v1.financial-report {:topic \"Project Phoenix\"}))
          competitor_technicals (step \"Gather Technical Specs\"
            (call :com.tech-analysis.eu:v1.spec-breakdown {:product \"Project Phoenix\"}))
          
          ;; Step 2: Synthesize the analysis from gathered data
          analysis_doc (step \"Synthesize Analysis\"
            (call :com.local-llm:v1.synthesize
                  {:docs [competitor_financials competitor_technicals]
                   :format :competitive-analysis}))
          
          ;; Step 3: Draft a press release based on the analysis
          press_release (step \"Draft Press Release\"
            (call :com.local-llm:v1.draft-document
                  {:context analysis_doc
                   :style :press-release}))
          
          ;; Step 4: Attempt to notify the team, with a fallback
          notification_result (step \"Notify Product Team\"
            (try
              (call :com.collaboration:v1.slack-post
                    {:channel \"#product-team\"
                     :summary (:key-takeaways analysis_doc)})
              (catch :error/network err
                (call :com.collaboration:v1.send-email
                      {:to \"product-team@example.com\"
                       :subject \"Urgent: Project Phoenix Analysis\"
                       :body (:key-takeaways analysis_doc)}))))
        ]
          ;; Final Step: Return a map that satisfies the intent's :success-criteria
          {
            :analysis-document analysis_doc
            :press-release-draft press_release
            :notification-status (:status notification_result)
          }
        )
      ))
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
        Value::Map(map) => {
            assert!(map.contains_key(&MapKey::Keyword(Keyword("analysis-document".to_string()))));
            assert!(map.contains_key(&MapKey::Keyword(Keyword("press-release-draft".to_string()))));
            assert!(map.contains_key(&MapKey::Keyword(Keyword("notification-status".to_string()))));

            let notification_status = map.get(&MapKey::Keyword(Keyword("notification-status".to_string()))).unwrap();
            // Depending on whether the slack capability is set to fail, this could be either
            assert!(
                *notification_status == Value::Keyword(Keyword(":slack-success".to_string())) ||
                *notification_status == Value::Keyword(Keyword(":email-fallback-success".to_string()))
            );
        }
        _ => panic!("Expected a map as the final result"),
    }
}
