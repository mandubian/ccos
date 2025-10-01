//! Tests for preflight capability validation logic.
//!
//! Ensures that capability identifiers mentioned inside string literals do not
//! cause false positives (regression test for naive tokenizer that picked up
//! punctuation and string content).

use rtfs_compiler::ccos::{CCOS, types::{Plan, PlanBody, PlanLanguage, PlanStatus}};
use rtfs_compiler::runtime::security::{RuntimeContext, SecurityLevel};
use std::sync::Arc;

#[tokio::test]
async fn plan_with_capabilities_only_in_string_is_valid() {
    let ccos = Arc::new(CCOS::new().await.expect("init ccos"));
    let plan_src = r#"(do
  (step "List Capabilities"
    (call :ccos.echo {:message "Available capabilities: :ccos.echo, :ccos.math.add, :ccos.user.ask"}))
)"#;
    let plan = Plan {
        plan_id: "test-plan".into(),
        name: None,
        intent_ids: vec![],
        language: PlanLanguage::Rtfs20,
        body: PlanBody::Rtfs(plan_src.to_string()),
        status: PlanStatus::Pending,
        created_at: 0,
        metadata: Default::default(),
        input_schema: None,
        output_schema: None,
        policies: Default::default(),
        capabilities_required: vec![],
        annotations: Default::default(),
    };
    // Use permissive security context; we only care about preflight scanning
    let ctx = RuntimeContext { security_level: SecurityLevel::Controlled, ..RuntimeContext::pure() };
    // Preflight should succeed (ccos.echo is invoked; others are only in string)
    ccos.preflight_validate_capabilities(&plan).await.expect("preflight should ignore string mentions");
}

#[tokio::test]
async fn plan_with_real_add_invocation_passes() {
    let ccos = Arc::new(CCOS::new().await.expect("init ccos"));
    let plan_src = r#"(do
  (step "Add Numbers" (call :ccos.math.add {:args (list 1 2 3)}))
)"#;
    let plan = Plan {
        plan_id: "test-plan-2".into(),
        name: None,
        intent_ids: vec![],
        language: PlanLanguage::Rtfs20,
        body: PlanBody::Rtfs(plan_src.to_string()),
        status: PlanStatus::Pending,
        created_at: 0,
        metadata: Default::default(),
        input_schema: None,
        output_schema: None,
        policies: Default::default(),
        capabilities_required: vec![],
        annotations: Default::default(),
    };
    let ctx = RuntimeContext { security_level: SecurityLevel::Controlled, ..RuntimeContext::pure() };
    ccos.preflight_validate_capabilities(&plan).await.expect("preflight should accept real capability invocation");
}
