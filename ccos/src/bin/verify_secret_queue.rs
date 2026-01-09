use ccos::approval::{
    storage_memory::InMemoryApprovalStorage,
    types::{ApprovalCategory, ApprovalStatus},
    unified_queue::UnifiedApprovalQueue,
};
use std::sync::Arc;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let storage = Arc::new(InMemoryApprovalStorage::new());
    let aq = UnifiedApprovalQueue::new(storage);

    println!("Testing add_secret_approval...");

    let approval_id = aq
        .add_secret_approval(
            "test.capability".to_string(),
            "TEST_SECRET".to_string(),
            "Description for test secret".to_string(),
            24,
        )
        .await?;

    println!("Created secret approval: {}", approval_id);

    let pending = aq.list_pending_secrets().await?;
    assert_eq!(pending.len(), 1);

    if let ApprovalCategory::SecretRequired {
        capability_id,
        secret_type,
        ..
    } = &pending[0].category
    {
        assert_eq!(capability_id, "test.capability");
        assert_eq!(secret_type, "TEST_SECRET");
        println!("Verified secret approval in queue.");
    } else {
        panic!("Wrong category!");
    }

    // Test deduplication
    let second_id = aq
        .add_secret_approval(
            "test.capability".to_string(),
            "TEST_SECRET".to_string(),
            "Another description".to_string(),
            24,
        )
        .await?;

    assert_eq!(approval_id, second_id);
    let pending = aq.list_pending_secrets().await?;
    assert_eq!(pending.len(), 1);
    println!("Verified deduplication works.");

    println!("Verification successful!");
    Ok(())
}
