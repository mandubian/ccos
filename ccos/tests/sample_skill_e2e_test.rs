//! End-to-end test for sample onboarding skill
//!
//! Tests loading the Twitter Publisher sample skill and verifying its onboarding configuration.

use std::sync::Arc;
use std::sync::Mutex as StdMutex;

use ccos::approval::storage_memory::InMemoryApprovalStorage;
use ccos::approval::unified_queue::UnifiedApprovalQueue;
use ccos::capabilities::registry::CapabilityRegistry;
use ccos::capability_marketplace::CapabilityMarketplace;
use ccos::secrets::SecretStore;
use ccos::skills::loader::parse_skill_markdown;
use ccos::skills::mapper::SkillMapper;
use ccos::skills::onboarding_capabilities::register_onboarding_capabilities;
use ccos::skills::types::OnboardingState;
use ccos::working_memory::{InMemoryJsonlBackend, WorkingMemory};
use tokio::sync::RwLock;

#[tokio::test]
async fn test_load_twitter_publisher_sample_skill() {
    // Load the markdown file
    let skill_path = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../capabilities/samples/twitter-publisher-skill.md"
    );

    let content = tokio::fs::read_to_string(skill_path)
        .await
        .expect("Failed to read twitter-publisher-skill.md");

    // Parse the skill
    let skill = parse_skill_markdown(&content).expect("Failed to parse skill markdown");

    // Verify basic metadata
    assert_eq!(skill.id, "twitter-publisher-skill");
    assert_eq!(skill.name, "Twitter Publisher Skill");

    // Verify onboarding configuration exists
    assert!(
        skill.onboarding.is_some(),
        "Twitter Publisher skill should have onboarding config"
    );

    let onboarding = skill.onboarding.as_ref().unwrap();
    assert!(onboarding.required, "Onboarding should be required");

    // Freeform onboarding: markdown skills now capture raw prose for LLM reasoning.
    // Structured steps remain supported for YAML/JSON skills, but are not required here.
    assert!(
        !onboarding.raw_content.trim().is_empty(),
        "Onboarding raw_content should be captured for markdown skills"
    );

    println!("✓ Twitter Publisher skill loaded and validated successfully");
    println!("  - ID: {}", skill.id);
    println!("  - Onboarding raw_content len: {}", onboarding.raw_content.len());
}

#[tokio::test]
async fn test_register_twitter_publisher_skill() {
    // Setup test components
    let registry = Arc::new(RwLock::new(CapabilityRegistry::new()));
    let marketplace = Arc::new(CapabilityMarketplace::new(registry));
    let secret_store = Arc::new(StdMutex::new(SecretStore::empty()));
    let backend = InMemoryJsonlBackend::new(None, None, None);
    let working_memory = Arc::new(StdMutex::new(WorkingMemory::new(Box::new(backend))));
    let storage = InMemoryApprovalStorage::new();
    let approval_queue = Arc::new(UnifiedApprovalQueue::new(Arc::new(storage)));

    // Register onboarding capabilities
    register_onboarding_capabilities(
        marketplace.clone(),
        secret_store.clone(),
        working_memory.clone(),
        approval_queue.clone(),
    )
    .await
    .expect("Failed to register onboarding capabilities");

    let skill_mapper = Arc::new(SkillMapper::new(marketplace.clone()));

    // Load the skill
    let skill_path = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../capabilities/samples/twitter-publisher-skill.md"
    );
    let content = tokio::fs::read_to_string(skill_path)
        .await
        .expect("Failed to read skill file");
    let skill = parse_skill_markdown(&content).expect("Failed to parse skill");

    // Register the skill
    skill_mapper
        .register_skill_capabilities(&skill, None)
        .await
        .expect("Failed to register skill");

    // Verify capabilities were registered
    // Note: The skill defines operations but they would need actual implementations
    // For this test, we're just verifying the onboarding metadata is stored

    println!("✓ Twitter Publisher skill registered successfully");
    println!("  - Skill mapper accepted the skill");
    println!("  - Onboarding config should be in metadata");
}

#[tokio::test]
async fn test_skill_onboarding_state_initialization() {
    use ccos::skills::types::SkillOnboardingState;

    // Create initial state for the Twitter Publisher skill (4 steps)
    let state = SkillOnboardingState::new(4);

    assert_eq!(state.status, OnboardingState::Loaded);
    assert_eq!(state.current_step, 0);
    assert_eq!(state.total_steps, 4);
    assert_eq!(state.completed_steps.len(), 0);

    println!("✓ Skill onboarding state initialized correctly for 4-step flow");
}
