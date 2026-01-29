use ccos::budget::{BudgetLimits, BudgetPolicies, ExhaustionPolicy};
use ccos::ccos_core::CCOS;
use ccos::config::types::{AgentConfig, BudgetConfig, PolicyConfig};
use ccos::governance_kernel::SemanticJudgePolicy;
use ccos::intent_graph::config::IntentGraphConfig;
use ccos::types::{Plan, PlanBody};
use rtfs::runtime::security::RuntimeContext;
use rtfs::runtime::values::Value;
use std::collections::HashMap;

#[tokio::test]
async fn test_budget_exhaustion_approval_required() {
    std::env::set_var("CCOS_ALLOW_STUB_PROVIDER", "1");
    // 1. Setup a policy with a very strict budget (1 step allowed)
    // and an ApprovalRequired exhaustion policy.
    let mut policies = HashMap::new();

    let budget_config = BudgetConfig {
        limits: BudgetLimits {
            steps: 1, // Max 1 step
            ..Default::default()
        },
        policies: BudgetPolicies {
            steps: ExhaustionPolicy::ApprovalRequired,
            ..Default::default()
        },
    };

    policies.insert(
        "test_policy".to_string(),
        PolicyConfig {
            risk_tier: "low".to_string(),
            requires_approvals: 0,
            budgets: budget_config.clone(),
        },
    );

    let mut agent_config = AgentConfig::default();
    agent_config.governance.policies = policies;

    // 2. Initialize CCOS with this config
    let ccos = CCOS::new_with_agent_config_and_configs_and_debug_callback(
        IntentGraphConfig::default(),
        None,
        Some(agent_config),
        None,
    )
    .await
    .expect("Failed to initialize CCOS");

    ccos.governance_kernel
        .set_semantic_judge_policy(SemanticJudgePolicy {
            enabled: false,
            fail_open: true,
            risk_threshold: 1.0,
        });

    // 3. Create a plan that calls a capability twice
    // Since we only allow 1 step, the second call should trigger exhaustion.
    // We use a simple echo call.
    let rtfs_code = r#"
        (do
            (call "ccos.io.println" "first call")
            (call "ccos.io.println" "second call"))
    "#;

    let plan = Plan {
        plan_id: "test_plan".to_string(),
        body: PlanBody::Rtfs(rtfs_code.to_string()),
        ..Plan::default()
    };

    // 4. Execute the plan with "execution_mode": "test_policy"
    let mut context = RuntimeContext::full();
    context.cross_plan_params.insert(
        "execution_mode".to_string(),
        Value::String("test_policy".to_string()),
    );

    let result = ccos
        .governance_kernel
        .execute_plan_governed(plan, &context)
        .await;

    // 5. Verify the result
    match result {
        Ok(exec_res) => {
            // It should be reported as non-successful (since it's not complete) but with a Paused status in metadata
            assert!(
                !exec_res.success,
                "Execution should be reported as paused (success: false)"
            );
            assert_eq!(
                exec_res.metadata.get("status").and_then(|v| v.as_string()),
                Some("paused"),
                "Plan status should be 'paused' in metadata"
            );
            assert!(
                exec_res.metadata.contains_key("checkpoint_id"),
                "Result should contain a checkpoint_id"
            );

            let checkpoint_id = exec_res
                .metadata
                .get("checkpoint_id")
                .unwrap()
                .as_string()
                .unwrap();
            println!("Plan paused with checkpoint: {}", checkpoint_id);
        }
        Err(e) => {
            panic!("Execution failed with error instead of pausing: {}", e);
        }
    }
}
