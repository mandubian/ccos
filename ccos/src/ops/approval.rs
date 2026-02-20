//! Approval operations - pure logic functions for approval queue management

use super::{ApprovalItem, ApprovalListOutput};
use crate::approval::{
    storage_file::FileApprovalStorage, ApprovalAuthority, ApprovalCategory, ApprovalRequest,
    UnifiedApprovalQueue,
};
// removed unused find_workspace_root import
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

use std::path::PathBuf;

/// Create a unified approval queue with file storage
fn create_queue(storage_path: PathBuf) -> RuntimeResult<UnifiedApprovalQueue<FileApprovalStorage>> {
    let storage = Arc::new(FileApprovalStorage::new(storage_path)?);
    Ok(UnifiedApprovalQueue::new(storage))
}

/// Convert ApprovalRequest to ApprovalItem for CLI output
fn to_approval_item(request: &ApprovalRequest) -> ApprovalItem {
    use super::ApprovalType;

    let (approval_type, title, description, source, goal) = match &request.category {
        ApprovalCategory::ServerDiscovery {
            source,
            server_info,
            requesting_goal,
            ..
        } => (
            ApprovalType::ServerDiscovery,
            server_info.name.clone(),
            server_info.endpoint.clone(),
            source.name(),
            requesting_goal.clone(),
        ),
        ApprovalCategory::EffectApproval {
            capability_id,
            intent_description,
            ..
        } => (
            ApprovalType::Effect,
            capability_id.clone(),
            intent_description.clone(),
            "planner".to_string(),
            None,
        ),
        ApprovalCategory::LlmPromptApproval { prompt, .. } => (
            ApprovalType::LlmPrompt,
            "LLM Prompt Approval".to_string(),
            if prompt.len() > 100 {
                format!("{}...", &prompt[..100])
            } else {
                prompt.clone()
            },
            "agent".to_string(),
            None,
        ),
        ApprovalCategory::SynthesisApproval { capability_id, .. } => (
            ApprovalType::Synthesis,
            format!("Synthesis Approval: {}", capability_id),
            "Capability synthesis".to_string(),
            "planner".to_string(),
            None,
        ),
        ApprovalCategory::SecretRequired {
            capability_id,
            secret_type,
            description,
        } => (
            ApprovalType::Effect, // Reuse Effect type for secret requests
            format!("Secret Required: {}", capability_id),
            format!("{}: {}", secret_type, description),
            "capability".to_string(),
            None,
        ),
        ApprovalCategory::BudgetExtension {
            plan_id, dimension, ..
        } => (
            ApprovalType::Budget,
            format!("Budget Extension: {}", dimension),
            format!("Plan {} requested budget extension", plan_id),
            "runtime".to_string(),
            None,
        ),
        ApprovalCategory::ChatPolicyException {
            kind,
            session_id,
            run_id,
        } => (
            ApprovalType::Effect, // reuse Effect type for now (policy exception gate)
            format!("Chat Policy Exception: {}", kind),
            format!("Session {} / Run {}", session_id, run_id),
            "chat".to_string(),
            None,
        ),
        ApprovalCategory::ChatPublicDeclassification {
            session_id,
            run_id,
            transform_capability_id,
            verifier_capability_id,
            constraints,
        } => (
            ApprovalType::Effect, // reuse Effect type for now (public declass gate)
            "Chat Public Declassification".to_string(),
            format!(
                "Session {} / Run {} — transform={} verifier={} constraints={}",
                session_id, run_id, transform_capability_id, verifier_capability_id, constraints
            ),
            "chat".to_string(),
            None,
        ),
        ApprovalCategory::SecretWrite { key, scope, .. } => (
            ApprovalType::Effect, // reuse Effect type for secrets
            format!("Secret Write: {}", key),
            format!("Store secret '{}' with scope '{}'", key, scope),
            "onboarding".to_string(),
            None,
        ),
        ApprovalCategory::HumanActionRequest {
            action_type,
            title,
            instructions,
            skill_id,
            step_id,
            ..
        } => (
            ApprovalType::Effect, // reuse Effect type for human actions
            format!("Human Action: {}", title),
            format!(
                "{} for skill {} step {}: {}",
                action_type,
                skill_id,
                step_id,
                if instructions.len() > 100 {
                    format!("{}...", &instructions[..100])
                } else {
                    instructions.clone()
                }
            ),
            "onboarding".to_string(),
            None,
        ),
        ApprovalCategory::HttpHostApproval {
            host,
            port,
            requesting_url,
            reason,
            ..
        } => (
            ApprovalType::Effect, // reuse Effect type for HTTP host approvals
            format!("HTTP Host Approval: {}", host),
            format!(
                "{}:{} for {} - {}",
                host,
                port.map_or("default".to_string(), |p| p.to_string()),
                requesting_url,
                reason
            ),
            "network".to_string(),
            None,
        ),
        ApprovalCategory::PackageApproval { package, runtime } => (
            ApprovalType::Effect, // reuse Effect type for package approvals
            format!("Package Approval: {}", package),
            format!("{}: {}", runtime, package),
            "sandbox".to_string(),
            None,
        ),
        ApprovalCategory::SandboxNetwork {
            capability_id,
            allowed_hosts,
        } => (
            ApprovalType::Effect, // reuse Effect type for sandbox network
            format!("Sandbox Network: {}", capability_id),
            format!("Allowed hosts: {}", allowed_hosts.join(", ")),
            "sandbox".to_string(),
            None,
        ),
    };

    ApprovalItem {
        id: request.id.clone(),
        approval_type,
        title,
        description,
        risk_level: format!("{:?}", request.risk_assessment.level),
        source,
        goal,
        status: if request.status.is_pending() {
            "pending".to_string()
        } else if request.status.is_approved() {
            "approved".to_string()
        } else if request.status.is_rejected() {
            "rejected".to_string()
        } else if request.status.is_expired() {
            "expired".to_string()
        } else {
            "resolved".to_string()
        },
        requested_at: request.requested_at.to_rfc3339(),
    }
}

/// List all pending approvals across all categories
pub async fn list_pending(storage_path: PathBuf) -> RuntimeResult<ApprovalListOutput> {
    let queue = create_queue(storage_path)?;
    let pending = queue.list_pending().await?;

    let items: Vec<ApprovalItem> = pending.iter().map(to_approval_item).collect();

    Ok(ApprovalListOutput {
        items: items.clone(),
        count: items.len(),
    })
}

/// Check if approving a discovery would conflict with an existing approved server
pub async fn check_approval_conflict(
    storage_path: PathBuf,
    id: String,
) -> RuntimeResult<Option<ApprovalConflict>> {
    let queue = create_queue(storage_path)?;

    // Get the pending request
    let pending = queue.get(&id).await?;
    if pending.is_none() {
        return Ok(None);
    }
    let pending_item = pending.unwrap();

    // Only discovery requests have conflicts with existing servers
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

/// Approve a discovery or any other approval request
pub async fn approve_discovery(
    storage_path: PathBuf,
    id: String,
    reason: Option<String>,
) -> RuntimeResult<()> {
    approve_request(storage_path, id, reason).await
}

/// Reject a discovery or any other approval request
pub async fn reject_discovery(
    storage_path: PathBuf,
    id: String,
    reason: String,
) -> RuntimeResult<()> {
    reject_request(storage_path, id, reason).await
}

/// Approve any approval request
pub async fn approve_request(
    storage_path: PathBuf,
    id: String,
    reason: Option<String>,
) -> RuntimeResult<()> {
    let queue = create_queue(storage_path)?;

    // Fetch the request to determine how to approve it
    let request = queue
        .get(&id)
        .await?
        .ok_or_else(|| RuntimeError::Generic(format!("Approval request {} not found", id)))?;

    match &request.category {
        ApprovalCategory::ServerDiscovery { .. } => {
            queue
                .approve_server(&id, ApprovalAuthority::User("cli".to_string()), reason)
                .await
        }
        _ => {
            queue
                .approve(&id, ApprovalAuthority::User("cli".to_string()), reason)
                .await
        }
    }
}

/// Reject any approval request
pub async fn reject_request(
    storage_path: PathBuf,
    id: String,
    reason: String,
) -> RuntimeResult<()> {
    let queue = create_queue(storage_path)?;
    queue
        .reject(&id, ApprovalAuthority::User("cli".to_string()), reason)
        .await
}

/// Skip a pending item (remove without approving or rejecting)
pub async fn skip_pending(storage_path: PathBuf, id: String) -> RuntimeResult<()> {
    let queue = create_queue(storage_path)?;
    queue.remove(&id).await?;
    Ok(())
}

/// List items that have timed out
pub async fn list_timeout(storage_path: PathBuf) -> RuntimeResult<ApprovalListOutput> {
    let queue = create_queue(storage_path)?;
    // Check expirations first
    queue.check_expirations().await?;

    // List all non-pending items and filter for expired
    let all = queue
        .list(crate::approval::ApprovalFilter::default())
        .await?;
    let timeout_items: Vec<ApprovalItem> = all
        .iter()
        .filter(|r| matches!(r.status, crate::approval::ApprovalStatus::Expired { .. }))
        .map(to_approval_item)
        .collect();

    Ok(ApprovalListOutput {
        items: timeout_items.clone(),
        count: timeout_items.len(),
    })
}
