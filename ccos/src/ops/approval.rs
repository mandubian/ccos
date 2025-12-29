//! Approval operations - pure logic functions for approval queue management

use super::{ApprovalItem, ApprovalListOutput};
use crate::approval::{
    storage_file::FileApprovalStorage, ApprovalAuthority, ApprovalCategory, ApprovalRequest,
    UnifiedApprovalQueue,
};
use crate::utils::fs::find_workspace_root;
use rtfs::runtime::error::{RuntimeError, RuntimeResult};
use serde::Serialize;
use std::sync::Arc;

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

/// Create a unified approval queue with file storage
fn create_queue() -> RuntimeResult<UnifiedApprovalQueue<FileApprovalStorage>> {
    let workspace_root = find_workspace_root();
    let storage_path = workspace_root.join("capabilities/servers/approvals");
    let storage = Arc::new(FileApprovalStorage::new(storage_path)?);
    Ok(UnifiedApprovalQueue::new(storage))
}

/// Helper to extract server info from ApprovalRequest
fn extract_server_item(request: &ApprovalRequest) -> Option<ApprovalItem> {
    if let ApprovalCategory::ServerDiscovery {
        source,
        server_info,
        requesting_goal,
        ..
    } = &request.category
    {
        Some(ApprovalItem {
            id: request.id.clone(),
            server_name: server_info.name.clone(),
            endpoint: server_info.endpoint.clone(),
            source: source.name(),
            risk_level: format!("{:?}", request.risk_assessment.level),
            goal: requesting_goal.clone(),
            status: if request.status.is_pending() {
                "pending".to_string()
            } else {
                "resolved".to_string()
            },
            requested_at: request.requested_at.to_rfc3339(),
        })
    } else {
        None
    }
}

/// List pending approvals
pub async fn list_pending() -> RuntimeResult<ApprovalListOutput> {
    let queue = create_queue()?;
    let pending = queue.list_pending_servers().await?;

    let items: Vec<ApprovalItem> = pending.iter().filter_map(extract_server_item).collect();

    Ok(ApprovalListOutput {
        items: items.clone(),
        count: items.len(),
    })
}

/// Check if approving a discovery would conflict with an existing approved server
pub async fn check_approval_conflict(id: String) -> RuntimeResult<Option<ApprovalConflict>> {
    let queue = create_queue()?;

    // Get the pending request
    let pending = queue.get(&id).await?;
    if pending.is_none() {
        return Ok(None);
    }
    let pending_item = pending.unwrap();

    // Get pending server info
    let (pending_name, pending_endpoint) =
        if let ApprovalCategory::ServerDiscovery { server_info, .. } = &pending_item.category {
            (server_info.name.clone(), server_info.endpoint.clone())
        } else {
            return Ok(None);
        };

    // Check approved servers for conflicts
    let approved = queue.list_approved_servers().await?;
    for approved_item in approved {
        if let ApprovalCategory::ServerDiscovery {
            server_info,
            health,
            capability_files,
            ..
        } = &approved_item.category
        {
            if server_info.name == pending_name || server_info.endpoint == pending_endpoint {
                let tool_count = capability_files.as_ref().map(|f| f.len()).unwrap_or(0);

                let (approved_at, version) =
                    if let crate::approval::ApprovalStatus::Approved { at, .. } =
                        &approved_item.status
                    {
                        (
                            at.to_rfc3339(),
                            health.as_ref().map(|h| h.version).unwrap_or(1),
                        )
                    } else {
                        continue;
                    };

                return Ok(Some(ApprovalConflict {
                    existing_name: server_info.name.clone(),
                    existing_endpoint: server_info.endpoint.clone(),
                    existing_version: version,
                    existing_tool_count: tool_count,
                    existing_approved_at: approved_at,
                    pending_name,
                    pending_endpoint,
                }));
            }
        }
    }

    Ok(None)
}

/// Approve a discovery
pub async fn approve_discovery(id: String, reason: Option<String>) -> RuntimeResult<()> {
    let queue = create_queue()?;
    queue
        .approve_server(&id, ApprovalAuthority::User("cli".to_string()), reason)
        .await
}

/// Reject a discovery
pub async fn reject_discovery(id: String, reason: String) -> RuntimeResult<()> {
    let queue = create_queue()?;
    queue
        .reject(&id, ApprovalAuthority::User("cli".to_string()), reason)
        .await
}

/// Skip a pending item (remove without approving or rejecting)
/// Used when user chooses to keep existing approved server instead of merging
pub async fn skip_pending(id: String) -> RuntimeResult<()> {
    let queue = create_queue()?;
    queue.remove(&id).await?;
    Ok(())
}

/// List timed-out items
pub async fn list_timeout() -> RuntimeResult<ApprovalListOutput> {
    let queue = create_queue()?;
    // Check expirations first
    queue.check_expirations().await?;

    // List all non-pending items and filter for expired
    let all = queue
        .list(crate::approval::ApprovalFilter::default())
        .await?;
    let timeout_items: Vec<ApprovalItem> = all
        .iter()
        .filter(|r| matches!(r.status, crate::approval::ApprovalStatus::Expired { .. }))
        .filter_map(extract_server_item)
        .map(|mut item| {
            item.status = "timeout".to_string();
            item
        })
        .collect();

    Ok(ApprovalListOutput {
        items: timeout_items.clone(),
        count: timeout_items.len(),
    })
}
