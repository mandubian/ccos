//! Approval operations - pure logic functions for approval queue management

use crate::discovery::ApprovalQueue;
use rtfs::runtime::error::RuntimeResult;
use super::{ApprovalItem, ApprovalListOutput};
use chrono::Utc;
use serde::Serialize;

/// Information about a conflict when approving a server that already exists
#[derive(Debug, Clone, Serialize)]
pub struct ApprovalConflict {
    pub existing_name: String,
    pub existing_endpoint: String,
    pub existing_version: u32,
    pub existing_tool_count: usize,
    pub existing_approved_at: String,
    pub pending_name: String,
    pub pending_endpoint: String,
}

/// List pending approvals
pub async fn list_pending() -> RuntimeResult<ApprovalListOutput> {
    let queue = ApprovalQueue::new(".");
    let pending = queue.list_pending()?;

    let items: Vec<ApprovalItem> = pending.into_iter().map(|item| {
        ApprovalItem {
            id: item.id,
            server_name: item.server_info.name,
            endpoint: item.server_info.endpoint,
            source: item.source.name(),
            risk_level: format!("{:?}", item.risk_assessment.level),
            goal: item.requesting_goal,
            status: "pending".to_string(),
            requested_at: item.requested_at.to_rfc3339(),
        }
    }).collect();

    Ok(ApprovalListOutput {
        items: items.clone(),
        count: items.len(),
    })
}

/// Check if approving a discovery would conflict with an existing approved server
pub async fn check_approval_conflict(id: String) -> RuntimeResult<Option<ApprovalConflict>> {
    let queue = ApprovalQueue::new(".");
    
    if let Some(existing) = queue.check_approval_conflict(&id)? {
        let pending = queue.get_pending(&id)?;
        if let Some(pending_item) = pending {
            let tool_count = existing.capability_files
                .as_ref()
                .map(|files| files.len())
                .unwrap_or(0);
            
            return Ok(Some(ApprovalConflict {
                existing_name: existing.server_info.name,
                existing_endpoint: existing.server_info.endpoint,
                existing_version: existing.version,
                existing_tool_count: tool_count,
                existing_approved_at: existing.approved_at.to_rfc3339(),
                pending_name: pending_item.server_info.name,
                pending_endpoint: pending_item.server_info.endpoint,
            }));
        }
    }
    
    Ok(None)
}

/// Approve a discovery
pub async fn approve_discovery(id: String, reason: Option<String>) -> RuntimeResult<()> {
    let queue = ApprovalQueue::new(".");
    queue.approve(&id, reason)?;
    Ok(())
}

/// Reject a discovery
pub async fn reject_discovery(id: String, reason: String) -> RuntimeResult<()> {
    let queue = ApprovalQueue::new(".");
    queue.reject(&id, reason)?;
    Ok(())
}

/// Skip a pending item (remove without approving or rejecting)
/// Used when user chooses to keep existing approved server instead of merging
pub async fn skip_pending(id: String) -> RuntimeResult<()> {
    let queue = ApprovalQueue::new(".");
    queue.remove_pending(&id)?;
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
            endpoint: item.server_info.endpoint,
            source: item.source.name(),
            risk_level: format!("{:?}", item.risk_assessment.level), // Risk level still relevant for timeouts?
            goal: item.requesting_goal,
            status: "timeout".to_string(),
            requested_at: item.requested_at.to_rfc3339(),
        }
    }).collect();

    Ok(ApprovalListOutput {
        items: items.clone(),
        count: items.len(),
    })
}
