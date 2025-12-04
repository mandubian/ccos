//! Approval operations - pure logic functions for approval queue management

use super::{ApprovalItem, ApprovalListOutput};
use crate::discovery::ApprovalQueue;
use chrono::Utc;
use rtfs::runtime::error::RuntimeResult;
use serde::Serialize;
use std::path::PathBuf;

/// Find the workspace root directory (where capabilities/ should be)
/// Checks for ccos/Cargo.toml (workspace root) or walks up to find capabilities/
fn find_workspace_root() -> PathBuf {
    let current_dir = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));

    // Strategy 1: Check if we're at workspace root (has ccos/Cargo.toml AND capabilities/)
    if current_dir.join("ccos/Cargo.toml").exists() && current_dir.join("capabilities").exists() {
        return current_dir;
    }

    // Strategy 2: Walk up the directory tree to find workspace root
    // Look for a directory that has both ccos/Cargo.toml and capabilities/
    let mut path = current_dir.clone();
    loop {
        if path.join("ccos/Cargo.toml").exists() && path.join("capabilities").exists() {
            return path;
        }
        if let Some(parent) = path.parent() {
            path = parent.to_path_buf();
        } else {
            break;
        }
    }

    // Strategy 3: Walk up to find capabilities/ directory (workspace root indicator)
    let mut path = current_dir.clone();
    loop {
        if path.join("capabilities").exists() {
            return path;
        }
        if let Some(parent) = path.parent() {
            path = parent.to_path_buf();
        } else {
            break;
        }
    }

    // Strategy 4: If we're inside ccos/ directory, go up one level
    if current_dir.join("Cargo.toml").exists() {
        if let Some(parent) = current_dir.parent() {
            if parent.join("capabilities").exists() || parent.join("ccos/Cargo.toml").exists() {
                return parent.to_path_buf();
            }
        }
    }

    // Last resort: use current directory
    current_dir
}

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
    let workspace_root = find_workspace_root();
    let queue = ApprovalQueue::new(&workspace_root);
    let pending = queue.list_pending()?;

    let items: Vec<ApprovalItem> = pending
        .into_iter()
        .map(|item| ApprovalItem {
            id: item.id,
            server_name: item.server_info.name,
            endpoint: item.server_info.endpoint,
            source: item.source.name(),
            risk_level: format!("{:?}", item.risk_assessment.level),
            goal: item.requesting_goal,
            status: "pending".to_string(),
            requested_at: item.requested_at.to_rfc3339(),
        })
        .collect();

    Ok(ApprovalListOutput {
        items: items.clone(),
        count: items.len(),
    })
}

/// Check if approving a discovery would conflict with an existing approved server
pub async fn check_approval_conflict(id: String) -> RuntimeResult<Option<ApprovalConflict>> {
    let workspace_root = find_workspace_root();
    let queue = ApprovalQueue::new(&workspace_root);

    if let Some(existing) = queue.check_approval_conflict(&id)? {
        let pending = queue.get_pending(&id)?;
        if let Some(pending_item) = pending {
            let tool_count = existing
                .capability_files
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
    let workspace_root = find_workspace_root();
    let queue = ApprovalQueue::new(&workspace_root);
    queue.approve(&id, reason)?;
    Ok(())
}

/// Reject a discovery
pub async fn reject_discovery(id: String, reason: String) -> RuntimeResult<()> {
    let workspace_root = find_workspace_root();
    let queue = ApprovalQueue::new(&workspace_root);
    queue.reject(&id, reason)?;
    Ok(())
}

/// Skip a pending item (remove without approving or rejecting)
/// Used when user chooses to keep existing approved server instead of merging
pub async fn skip_pending(id: String) -> RuntimeResult<()> {
    let workspace_root = find_workspace_root();
    let queue = ApprovalQueue::new(&workspace_root);
    queue.remove_pending(&id)?;
    Ok(())
}

/// List timed-out items
pub async fn list_timeout() -> RuntimeResult<ApprovalListOutput> {
    let workspace_root = find_workspace_root();
    let queue = ApprovalQueue::new(&workspace_root);
    let timeout_items = queue.list_timeouts()?;

    let items: Vec<ApprovalItem> = timeout_items
        .into_iter()
        .map(|item| {
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
        })
        .collect();

    Ok(ApprovalListOutput {
        items: items.clone(),
        count: items.len(),
    })
}
