//! Approval operations - pure logic functions for approval queue management

use crate::discovery::ApprovalQueue;
use rtfs::runtime::error::RuntimeResult;
use super::{ApprovalItem, ApprovalListOutput};
use chrono::Utc;

/// List pending approvals
pub async fn list_pending() -> RuntimeResult<ApprovalListOutput> {
    let queue = ApprovalQueue::new(".");
    let pending = queue.list_pending()?;

    let items: Vec<ApprovalItem> = pending.into_iter().map(|item| {
        ApprovalItem {
            id: item.id,
            server_name: item.server_info.name,
            status: "pending".to_string(),
            requested_at: item.requested_at.to_rfc3339(),
        }
    }).collect();

    Ok(ApprovalListOutput {
        items: items.clone(),
        count: items.len(),
    })
}

/// Approve a discovery
pub async fn approve_discovery(id: String) -> RuntimeResult<()> {
    let queue = ApprovalQueue::new(".");
    queue.approve(&id, None)?;
    Ok(())
}

/// Reject a discovery
pub async fn reject_discovery(id: String) -> RuntimeResult<()> {
    let queue = ApprovalQueue::new(".");
    queue.reject(&id, "Rejected via CLI".to_string())?;
    Ok(())
}

/// List timed-out items
pub async fn list_timeout() -> RuntimeResult<ApprovalListOutput> {
    let queue = ApprovalQueue::new(".");
    let timeout_items = queue.list_timeouts()?;

    let items: Vec<ApprovalItem> = timeout_items.into_iter().map(|item| {
        ApprovalItem {
            id: item.id,
            server_name: item.server_info.name,
            status: "timeout".to_string(),
            requested_at: item.requested_at.to_rfc3339(),
        }
    }).collect();

    Ok(ApprovalListOutput {
        items: items.clone(),
        count: items.len(),
    })
}