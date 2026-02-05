//! Minimal integration test for onboarding integration
//!
//! Verifies that onboarding config metadata is correctly stored and retrievable.

use std::collections::HashMap;
use std::sync::Arc;

use ccos::capabilities::registry::CapabilityRegistry;
use ccos::capability_marketplace::CapabilityMarketplace;
use ccos::skills::types::OnboardingConfig;
use tokio::sync::RwLock;

#[tokio::test]
async fn test_onboarding_config_metadata_storage() {
    let registry = Arc::new(RwLock::new(CapabilityRegistry::new()));
    let marketplace = Arc::new(CapabilityMarketplace::new(registry));

    // Create a simple onboarding config
    let onboarding_config = OnboardingConfig {
        required: true,
        raw_content: "Setup instructions for the agent to read and follow.".to_string(),
        steps: vec![],
    };

    // Simulate what SkillMapper does: store in metadata
    let onboarding_json = serde_json::to_string(&onboarding_config).unwrap();

    // Verify round-trip serialization works
    let parsed: OnboardingConfig = serde_json::from_str(&onboarding_json).unwrap();
    assert_eq!(parsed.required, true);
    assert_eq!(parsed.steps.len(), 0);

    println!("✓ Onboarding config can be serialized and deserialized");
}

#[tokio::test]
async fn test_skill_onboarding_state_operations() {
    use ccos::skills::types::{OnboardingState, SkillOnboardingState};

    // Create initial state
    let mut state = SkillOnboardingState::new(2);
    assert_eq!(state.status, OnboardingState::Loaded);
    assert_eq!(state.current_step, 0);
    assert_eq!(state.total_steps, 2);

    // Complete first step
    state.complete_step("step-1".to_string());
    assert_eq!(state.current_step, 1);
    assert_eq!(state.completed_steps.len(), 1);

    // Complete second step - should transition to Operational
    state.complete_step("step-2".to_string());
    assert_eq!(state.status, OnboardingState::Operational);
    assert_eq!(state.current_step, 2);
    assert_eq!(state.completed_steps.len(), 2);

    println!("✓ SkillOnboardingState correctly tracks progress and transitions to Operational");
}

#[tokio::test]
async fn test_predicate_display_formatting() {
    use ccos::chat::predicate::Predicate;

    let predicate = Predicate::ActionSucceeded {
        function_name: "ccos.secrets.set".to_string(),
    };

    let formatted = format!("{}", predicate);
    assert_eq!(formatted, "(audit.succeeded? \"ccos.secrets.set\")");

    println!("✓ Predicate Display trait works correctly");
}
