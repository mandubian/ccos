//! Test Approval Workflow Example
//!
//! Demonstrates the UnifiedApprovalQueue API for all approval categories:
//! - Server Discovery approvals
//! - LLM Prompt approvals
//! - Synthesis (generated code) approvals
//! - Effect (capability) approvals
//!
//! Run: cargo run --example test_approval_workflow

use ccos::approval::{
    storage_memory::InMemoryApprovalStorage, ApprovalAuthority, ApprovalCategory, RiskAssessment,
    RiskLevel, UnifiedApprovalQueue,
};
use std::sync::Arc;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("╔═══════════════════════════════════════════════════════════════╗");
    println!("║         UnifiedApprovalQueue Workflow Test                    ║");
    println!("╚═══════════════════════════════════════════════════════════════╝\n");

    // Use in-memory storage for testing
    let storage = Arc::new(InMemoryApprovalStorage::new());
    let queue = UnifiedApprovalQueue::new(storage);

    // ========================================================================
    // Test 1: LLM Prompt Approval
    // ========================================================================
    println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
    println!("📝 TEST 1: LLM Prompt Approval");
    println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━\n");

    let llm_id = queue
        .add_llm_prompt_approval(
            "Write code to delete all files in /tmp".to_string(),
            vec![
                "destructive operation".to_string(),
                "file system access".to_string(),
            ],
            RiskAssessment {
                level: RiskLevel::High,
                reasons: vec!["Potentially destructive file operation".to_string()],
            },
            24,
        )
        .await?;

    println!("✅ Queued LLM prompt for approval");
    println!("   ID: {}", llm_id);

    // List pending LLM prompts
    let pending_llm = queue.list_pending_llm_prompts().await?;
    println!("\n📋 Pending LLM prompts: {}", pending_llm.len());
    for req in &pending_llm {
        if let ApprovalCategory::LlmPromptApproval {
            prompt,
            risk_reasons,
        } = &req.category
        {
            println!("   Prompt: \"{}...\"", &prompt[..50.min(prompt.len())]);
            println!("   Risk reasons: {:?}", risk_reasons);
            println!("   Risk level: {:?}", req.risk_assessment.level);
        }
    }

    // Approve it
    queue
        .approve(
            &llm_id,
            ApprovalAuthority::User("admin".to_string()),
            Some("Reviewed - safe in test context".to_string()),
        )
        .await?;
    println!("\n✅ Approved LLM prompt");

    // Verify no longer pending
    let pending_llm = queue.list_pending_llm_prompts().await?;
    println!(
        "📋 Pending LLM prompts after approval: {}",
        pending_llm.len()
    );

    // ========================================================================
    // Test 2: Synthesis Approval
    // ========================================================================
    println!("\n━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
    println!("🔧 TEST 2: Synthesis (Generated Code) Approval");
    println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━\n");

    let generated_code = r#"
(defn my-capability [input]
  "A generated RTFS capability"
  (let [result (+ (:x input) (:y input))]
    {:sum result}))
"#;

    let synth_id = queue
        .add_synthesis_approval(
            "generated.math.add_numbers".to_string(),
            generated_code.to_string(),
            true, // is_pure
            RiskAssessment {
                level: RiskLevel::Low,
                reasons: vec!["Auto-generated capability".to_string()],
            },
            24,
        )
        .await?;

    println!("✅ Queued synthesis for approval");
    println!("   ID: {}", synth_id);
    println!("   Capability: generated.math.add_numbers");

    // List pending syntheses
    let pending_synth = queue.list_pending_syntheses().await?;
    println!("\n📋 Pending syntheses: {}", pending_synth.len());
    for req in &pending_synth {
        if let ApprovalCategory::SynthesisApproval {
            capability_id,
            is_pure,
            ..
        } = &req.category
        {
            println!("   Capability: {}", capability_id);
            println!("   Is pure: {}", is_pure);
            println!("   Risk level: {:?}", req.risk_assessment.level);
        }
    }

    // Reject this one
    queue
        .reject(
            &synth_id,
            ApprovalAuthority::User("reviewer".to_string()),
            "Code needs refinement".to_string(),
        )
        .await?;
    println!("\n❌ Rejected synthesis");

    // Verify no longer pending
    let pending_synth = queue.list_pending_syntheses().await?;
    println!(
        "📋 Pending syntheses after rejection: {}",
        pending_synth.len()
    );

    // ========================================================================
    // Test 3: Effect Approval
    // ========================================================================
    println!("\n━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
    println!("⚡ TEST 3: Effect (Capability) Approval");
    println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━\n");

    let effect_id = queue
        .add_effect_approval(
            "mcp.github.delete_repo".to_string(),
            vec!["delete".to_string(), "write".to_string()],
            "Delete the test repository".to_string(),
            RiskAssessment {
                level: RiskLevel::High,
                reasons: vec!["Destructive operation".to_string()],
            },
            24,
        )
        .await?;

    println!("✅ Queued effect approval");
    println!("   ID: {}", effect_id);
    println!("   Capability: mcp.github.delete_repo");
    println!("   Effects: delete, write");

    // List pending effects
    let pending_effects = queue.list_pending_effects().await?;
    println!("\n📋 Pending effect approvals: {}", pending_effects.len());
    for req in &pending_effects {
        if let ApprovalCategory::EffectApproval {
            capability_id,
            effects,
            intent_description,
        } = &req.category
        {
            println!("   Capability: {}", capability_id);
            println!("   Effects: {:?}", effects);
            println!("   Intent: {}", intent_description);
        }
    }

    // Approve it
    queue
        .approve(
            &effect_id,
            ApprovalAuthority::User("admin".to_string()),
            Some("Approved for test".to_string()),
        )
        .await?;
    println!("\n✅ Approved effect");

    // ========================================================================
    // Summary
    // ========================================================================
    println!("\n━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
    println!("📊 SUMMARY");
    println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━\n");

    println!("✅ LLM Prompt Approval: queued → approved");
    println!("❌ Synthesis Approval: queued → rejected");
    println!("✅ Effect Approval: queued → approved");
    println!("\n🎉 All approval workflow tests completed successfully!");

    Ok(())
}
