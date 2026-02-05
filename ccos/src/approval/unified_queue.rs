//! Unified Approval Queue
//!
//! A generic approval queue that works with the `ApprovalStorage` trait
//! for backend-agnostic storage. Replaces the legacy file-based ApprovalQueue.

use super::queue::{ApprovalAuthority, DiscoverySource, RiskAssessment, RiskLevel, ServerInfo};
use super::types::{
    ApprovalCategory, ApprovalConsumer, ApprovalFilter, ApprovalRequest, ApprovalStatus,
    ApprovalStorage, ServerHealthTracking,
};
use chrono::Utc;
use rtfs::runtime::error::{RuntimeError, RuntimeResult};
use std::sync::Arc;

/// Unified approval queue using trait-based storage
///
/// This replaces the legacy `ApprovalQueue` with a storage-agnostic implementation.
/// All approval types (server discovery, effects, synthesis, LLM) are handled uniformly.
pub struct UnifiedApprovalQueue<S: ApprovalStorage> {
    storage: Arc<S>,
    consumers: Arc<tokio::sync::RwLock<Vec<Arc<dyn ApprovalConsumer>>>>,
}

impl<S: ApprovalStorage> Clone for UnifiedApprovalQueue<S> {
    fn clone(&self) -> Self {
        Self {
            storage: Arc::clone(&self.storage),
            consumers: Arc::clone(&self.consumers),
        }
    }
}

impl<S: ApprovalStorage> UnifiedApprovalQueue<S> {
    /// Create a new unified approval queue with the given storage backend
    pub fn new(storage: Arc<S>) -> Self {
        Self {
            storage,
            consumers: Arc::new(tokio::sync::RwLock::new(Vec::new())),
        }
    }

    /// Add a consumer for approval lifecycle events
    pub async fn add_consumer(&self, consumer: Arc<dyn ApprovalConsumer>) {
        let mut consumers = self.consumers.write().await;
        consumers.push(consumer);
    }

    // ========================================================================
    // Generic Operations (work with any ApprovalCategory)
    // ========================================================================

    /// Add a new approval request
    pub async fn add(&self, request: ApprovalRequest) -> RuntimeResult<String> {
        let id = request.id.clone();
        self.storage.add(request.clone()).await?;

        // Notify consumers
        let consumers = self.consumers.read().await;
        for consumer in consumers.iter() {
            consumer.on_approval_requested(&request).await;
        }

        Ok(id)
    }

    /// Get an approval request by ID
    pub async fn get(&self, id: &str) -> RuntimeResult<Option<ApprovalRequest>> {
        self.storage.get(id).await
    }

    /// List approval requests with optional filter
    pub async fn list(&self, filter: ApprovalFilter) -> RuntimeResult<Vec<ApprovalRequest>> {
        self.storage.list(filter).await
    }

    /// Update an existing approval request
    pub async fn update(&self, request: &ApprovalRequest) -> RuntimeResult<()> {
        self.storage.update(request).await
    }

    /// List all pending approval requests
    pub async fn list_pending(&self) -> RuntimeResult<Vec<ApprovalRequest>> {
        self.storage.list(ApprovalFilter::pending()).await
    }

    /// List pending requests of a specific category
    pub async fn list_pending_by_category(
        &self,
        category_type: &str,
    ) -> RuntimeResult<Vec<ApprovalRequest>> {
        self.storage
            .list(ApprovalFilter {
                category_type: Some(category_type.to_string()),
                status_pending: Some(true),
                ..Default::default()
            })
            .await
    }

    /// Approve a request
    pub async fn approve(
        &self,
        id: &str,
        by: ApprovalAuthority,
        reason: Option<String>,
    ) -> RuntimeResult<()> {
        let request =
            self.storage.get(id).await?.ok_or_else(|| {
                RuntimeError::Generic(format!("Approval request not found: {}", id))
            })?;

        match request.category {
            ApprovalCategory::ServerDiscovery { .. } => self.approve_server(id, by, reason).await,
            _ => {
                let mut request = request;
                request.approve(by, reason);
                self.storage.update(&request).await?;

                // Notify consumers
                let consumers = self.consumers.read().await;
                for consumer in consumers.iter() {
                    consumer.on_approval_resolved(&request).await;
                }
                Ok(())
            }
        }
    }

    /// Reject a request
    pub async fn reject(
        &self,
        id: &str,
        by: ApprovalAuthority,
        reason: String,
    ) -> RuntimeResult<()> {
        let mut request =
            self.storage.get(id).await?.ok_or_else(|| {
                RuntimeError::Generic(format!("Approval request not found: {}", id))
            })?;

        // If it's a server discovery, move artifacts to rejected
        if let ApprovalCategory::ServerDiscovery {
            ref server_info, ..
        } = request.category
        {
            let _ = self.move_server_directory(&server_info.name, "pending", "rejected");
        }

        request.reject(by, reason);
        self.storage.update(&request).await?;

        // Notify consumers
        let consumers = self.consumers.read().await;
        for consumer in consumers.iter() {
            consumer.on_approval_resolved(&request).await;
        }
        Ok(())
    }

    /// Remove a request
    pub async fn remove(&self, id: &str) -> RuntimeResult<bool> {
        self.storage.remove(id).await
    }

    /// Remove a pending request (alias for remove, for backward compatibility)
    pub async fn remove_pending(&self, id: &str) -> RuntimeResult<bool> {
        self.storage.remove(id).await
    }

    /// Check and expire timed-out requests
    pub async fn check_expirations(&self) -> RuntimeResult<Vec<String>> {
        self.storage.check_expirations().await
    }

    // ========================================================================
    // Server Discovery Specific Operations
    // ========================================================================

    /// Add a server discovery request
    ///
    /// Deduplication logic based on `server_id`:
    /// - If **pending** with same server_id exists → update in place (bump version)
    /// - If **approved** with same server_id exists → create NEW pending with version+1
    /// - If **rejected/expired** with same server_id → create fresh pending
    pub async fn add_server_discovery(
        &self,
        source: DiscoverySource,
        server_info: ServerInfo,
        domain_match: Vec<String>,
        risk_assessment: RiskAssessment,
        requesting_goal: Option<String>,
        expires_in_hours: i64,
    ) -> RuntimeResult<String> {
        let new_server_id = crate::utils::fs::sanitize_filename(&server_info.name);

        // Find existing approvals for this server_id
        let all = self.list(Default::default()).await?;
        let mut existing_pending: Option<ApprovalRequest> = None;
        let mut existing_approved: Option<ApprovalRequest> = None;
        let mut highest_version: u32 = 0;

        for r in all {
            if let ApprovalCategory::ServerDiscovery {
                server_id: Some(ref sid),
                version,
                ..
            } = &r.category
            {
                if sid == &new_server_id {
                    let ver = version.unwrap_or(1);
                    if ver > highest_version {
                        highest_version = ver;
                    }

                    if r.status.is_pending() {
                        existing_pending = Some(r);
                    } else if r.status.is_approved() {
                        existing_approved = Some(r);
                    }
                }
            } else if let ApprovalCategory::ServerDiscovery {
                ref server_info, ..
            } = &r.category
            {
                // Legacy: derive server_id from name
                let derived_id = crate::utils::fs::sanitize_filename(&server_info.name);
                if derived_id == new_server_id {
                    if r.status.is_pending() {
                        existing_pending = Some(r);
                    } else if r.status.is_approved() {
                        existing_approved = Some(r);
                    }
                }
            }
        }

        // Case 1: Pending exists → return existing ID (don't create duplicate pending)
        if let Some(pending) = existing_pending {
            // Return the existing pending ID - user should approve that one
            return Ok(pending.id);
        }

        // Case 2: Approved exists → create NEW pending with version+1
        let new_version = if existing_approved.is_some() {
            highest_version + 1
        } else {
            1
        };

        let request = ApprovalRequest::new(
            ApprovalCategory::ServerDiscovery {
                source,
                server_info,
                server_id: Some(new_server_id),
                version: Some(new_version),
                domain_match,
                requesting_goal,
                health: None,
                capability_files: None,
            },
            risk_assessment,
            expires_in_hours,
            if existing_approved.is_some() {
                Some(format!("Re-introspection (version {})", new_version))
            } else {
                None
            },
        );
        self.add(request).await
    }

    /// List pending server discoveries
    pub async fn list_pending_servers(&self) -> RuntimeResult<Vec<ApprovalRequest>> {
        self.list_pending_by_category("ServerDiscovery").await
    }

    /// List approved servers
    pub async fn list_approved_servers(&self) -> RuntimeResult<Vec<ApprovalRequest>> {
        self.storage
            .list(ApprovalFilter {
                category_type: Some("ServerDiscovery".to_string()),
                status_pending: Some(false),
                ..Default::default()
            })
            .await
            .map(|requests| {
                requests
                    .into_iter()
                    .filter(|r| matches!(r.status, ApprovalStatus::Approved { .. }))
                    .collect()
            })
    }

    /// Approve a server with optional health initialization
    /// Also moves capability files from pending/ to approved/
    /// If approving a new version, archives the old version first
    pub async fn approve_server(
        &self,
        id: &str,
        by: ApprovalAuthority,
        reason: Option<String>,
    ) -> RuntimeResult<()> {
        let mut request =
            self.storage.get(id).await?.ok_or_else(|| {
                RuntimeError::Generic(format!("Server discovery not found: {}", id))
            })?;

        // Get server_id and version for archiving logic
        let (server_id, new_version) = if let ApprovalCategory::ServerDiscovery {
            ref server_id,
            ref version,
            ref server_info,
            ..
        } = request.category
        {
            let sid = server_id
                .clone()
                .unwrap_or_else(|| crate::utils::fs::sanitize_filename(&server_info.name));
            (sid, version.unwrap_or(1))
        } else {
            return Err(RuntimeError::Generic(
                "Not a server discovery request".to_string(),
            ));
        };

        // Archive any existing approved version with the same server_id
        if new_version > 1 {
            if let Ok(all) = self.list_approved_servers().await {
                for old in all {
                    if let ApprovalCategory::ServerDiscovery {
                        server_id: Some(ref sid),
                        version,
                        ..
                    } = old.category
                    {
                        if sid == &server_id && old.id != id {
                            let old_version = version.unwrap_or(1);
                            // Archive the old approval
                            crate::ccos_println!(
                                "Archiving previous version {} of server {}",
                                old_version,
                                server_id
                            );

                            // Move old files to archived/ under a versioned directory name.
                            //
                            // NOTE: `move_server_directory()` derives the directory name from the
                            // provided `name` by sanitizing it, so we cannot pass a synthetic
                            // name like "{server_id}__v{old_version}" and expect it to find
                            // approved/{server_id}. Instead, explicitly rename approved/{server_id}
                            // -> archived/{server_id}__v{old_version}.
                            let _ = self.archive_approved_server_dir(&server_id, old_version);

                            // Update old approval status to superseded
                            let mut old_req = old;
                            old_req.status = ApprovalStatus::Superseded {
                                by_version: new_version,
                                at: Utc::now(),
                            };
                            let _ = self.storage.update(&old_req).await;
                        }
                    }
                }
            }
        }

        // Initialize health tracking for newly approved servers
        if let ApprovalCategory::ServerDiscovery { ref mut health, .. } = request.category {
            if health.is_none() {
                *health = Some(ServerHealthTracking::default());
            }
        }

        // Move capability files from pending to approved
        if let ApprovalCategory::ServerDiscovery {
            ref server_info,
            ref mut capability_files,
            ..
        } = request.category
        {
            // Try moving from pending -> approved
            if let Ok(Some(files)) =
                self.move_server_directory(&server_info.name, "pending", "approved")
            {
                *capability_files = Some(files);
            } else {
                // FALLBACK: If not in pending, maybe it was already rejected (duplicate case)
                // Try moving from rejected -> approved
                if let Ok(Some(files)) =
                    self.move_server_directory(&server_info.name, "rejected", "approved")
                {
                    *capability_files = Some(files);
                }
            }

            // Create an approval link file in the approved directory (if it exists).
            // This is used by the versioning + filesystem-sync tooling to correlate artifacts
            // with approval IDs.
            let workspace_root = crate::utils::fs::get_workspace_root();
            let approved_dir = workspace_root
                .join("capabilities/servers/approved")
                .join(&server_id);
            if approved_dir.exists() {
                self.write_approval_link_files(&approved_dir, &request.id, &server_id, new_version);
            }
        }

        request.approve(by, reason);
        self.storage.update(&request).await?;

        // Notify consumers
        let consumers = self.consumers.read().await;
        for consumer in consumers.iter() {
            consumer.on_approval_resolved(&request).await;
        }
        Ok(())
    }

    /// Archive the current approved server directory into `archived/` with a version suffix.
    fn archive_approved_server_dir(&self, server_id: &str, old_version: u32) -> RuntimeResult<()> {
        let workspace_root = crate::utils::fs::get_workspace_root();
        let approved_dir = workspace_root
            .join("capabilities/servers/approved")
            .join(server_id);
        if !approved_dir.exists() {
            return Ok(());
        }

        let archived_name = format!("{}__v{}", server_id, old_version);
        let archived_dir = workspace_root
            .join("capabilities/servers/archived")
            .join(archived_name);

        if let Some(parent) = archived_dir.parent() {
            std::fs::create_dir_all(parent).map_err(|e| {
                RuntimeError::IoError(format!(
                    "Failed to create directory {}: {}",
                    parent.display(),
                    e
                ))
            })?;
        }

        if archived_dir.exists() {
            let _ = std::fs::remove_dir_all(&archived_dir);
        }

        std::fs::rename(&approved_dir, &archived_dir).map_err(|e| {
            RuntimeError::IoError(format!(
                "Failed to archive server directory from {} to {}: {}",
                approved_dir.display(),
                archived_dir.display(),
                e
            ))
        })?;

        Ok(())
    }

    /// Write approval link metadata file into a server directory.
    ///
    /// Canonical filename: `approval_link.json`.
    fn write_approval_link_files(
        &self,
        dir: &std::path::Path,
        approval_id: &str,
        server_id: &str,
        version: u32,
    ) {
        let link_data = serde_json::json!({
            "approval_id": approval_id,
            "server_id": server_id,
            "version": version,
            "created_at": chrono::Utc::now().to_rfc3339()
        });

        if let Ok(content) = serde_json::to_string_pretty(&link_data) {
            let _ = std::fs::write(dir.join("approval_link.json"), &content);
        }
    }

    /// Update health metrics for an approved server
    pub async fn update_server_health(&self, id: &str, success: bool) -> RuntimeResult<()> {
        let mut request = self
            .storage
            .get(id)
            .await?
            .ok_or_else(|| RuntimeError::Generic(format!("Server not found: {}", id)))?;

        if let ApprovalCategory::ServerDiscovery { ref mut health, .. } = request.category {
            let tracking = health.get_or_insert_with(ServerHealthTracking::default);
            tracking.total_calls += 1;
            if success {
                tracking.last_successful_call = Some(Utc::now());
                tracking.consecutive_failures = 0;
            } else {
                tracking.total_errors += 1;
                tracking.consecutive_failures += 1;
            }
            self.storage.update(&request).await
        } else {
            Err(RuntimeError::Generic(
                "Cannot update health on non-server request".to_string(),
            ))
        }
    }

    /// Check if a server should be dismissed based on health metrics
    pub async fn should_dismiss_server(&self, id: &str) -> RuntimeResult<bool> {
        let request = self.storage.get(id).await?;
        if let Some(req) = request {
            if let ApprovalCategory::ServerDiscovery { health, .. } = &req.category {
                if let Some(h) = health {
                    return Ok(h.should_dismiss());
                }
            }
        }
        Ok(false)
    }

    /// Get server info from an approval request
    pub fn extract_server_info(request: &ApprovalRequest) -> Option<&ServerInfo> {
        if let ApprovalCategory::ServerDiscovery { server_info, .. } = &request.category {
            Some(server_info)
        } else {
            None
        }
    }

    /// Update capability files for an approved server
    pub async fn update_approved_server_capabilities(
        &self,
        server_id: &str,
        files: Vec<String>,
    ) -> RuntimeResult<()> {
        let approved = self.list_approved_servers().await?;
        let found = approved.iter().find(|r| {
            if let ApprovalCategory::ServerDiscovery { server_info, .. } = &r.category {
                r.id == server_id || server_info.name == server_id
            } else {
                false
            }
        });

        if let Some(request) = found {
            let mut updated = request.clone();
            if let ApprovalCategory::ServerDiscovery {
                ref mut capability_files,
                ..
            } = updated.category
            {
                *capability_files = Some(files);
            }
            self.storage.update(&updated).await
        } else {
            Err(RuntimeError::Generic(format!(
                "Server '{}' not found in approved list",
                server_id
            )))
        }
    }
}

/// Result of filesystem synchronization
#[derive(Debug, Default)]
pub struct FilesystemSyncReport {
    /// IDs of approval records created for orphaned pending directories
    pub created_pending: Vec<String>,
    /// IDs of approval records created for orphaned approved directories
    pub created_approved: Vec<String>,
    /// Warnings about inconsistencies (e.g., approval says pending but files in approved/)
    pub warnings: Vec<String>,
}

impl<S: ApprovalStorage> UnifiedApprovalQueue<S> {
    // ========================================================================
    // Filesystem Synchronization
    // ========================================================================

    /// Scan pending/ and approved/ directories and reconcile with approval registry.
    /// Creates approval records for orphaned server directories.
    pub async fn sync_with_filesystem(&self) -> RuntimeResult<FilesystemSyncReport> {
        let workspace_root = crate::utils::fs::get_workspace_root();
        let pending_dir = workspace_root.join("capabilities/servers/pending");
        let approved_dir = workspace_root.join("capabilities/servers/approved");

        let mut report = FilesystemSyncReport::default();

        // Get all existing ServerDiscovery approvals for lookup
        let all_approvals = self
            .list(ApprovalFilter {
                category_type: Some("ServerDiscovery".to_string()),
                ..Default::default()
            })
            .await?;

        // Build a set of known server_ids
        let mut known_server_ids: std::collections::HashSet<String> =
            std::collections::HashSet::new();
        for req in &all_approvals {
            if let ApprovalCategory::ServerDiscovery {
                server_id: Some(ref sid),
                ..
            } = req.category
            {
                known_server_ids.insert(sid.clone());
            } else if let ApprovalCategory::ServerDiscovery {
                ref server_info, ..
            } = req.category
            {
                // Derive server_id from name for legacy approvals
                known_server_ids.insert(crate::utils::fs::sanitize_filename(&server_info.name));
            }
        }

        // 1. Scan pending directories for orphans
        if pending_dir.exists() {
            if let Ok(entries) = std::fs::read_dir(&pending_dir) {
                for entry in entries.flatten() {
                    let path = entry.path();
                    if !path.is_dir() {
                        continue;
                    }
                    let server_id = entry.file_name().to_string_lossy().to_string();

                    // Skip if already known
                    if known_server_ids.contains(&server_id) {
                        continue;
                    }

                    // Try to create approval from server.rtfs
                    if let Some(approval) =
                        Self::create_approval_from_server_directory(&path, false)
                    {
                        let id = approval.id.clone();
                        self.storage.add(approval).await?;
                        report.created_pending.push(id);
                        known_server_ids.insert(server_id);
                    }
                }
            }
        }

        // 2. Scan approved directories for orphans
        if approved_dir.exists() {
            if let Ok(entries) = std::fs::read_dir(&approved_dir) {
                for entry in entries.flatten() {
                    let path = entry.path();
                    if !path.is_dir() {
                        continue;
                    }
                    let server_id = entry.file_name().to_string_lossy().to_string();

                    // Skip if already known
                    if known_server_ids.contains(&server_id) {
                        // Check for inconsistency: approval says pending but files are in approved/
                        for req in &all_approvals {
                            if let ApprovalCategory::ServerDiscovery {
                                server_id: Some(ref sid),
                                ..
                            } = req.category
                            {
                                if sid == &server_id && req.status.is_pending() {
                                    report.warnings.push(format!(
                                        "{}: approval says pending, but files are in approved/",
                                        server_id
                                    ));
                                }
                            }
                        }
                        continue;
                    }

                    // Try to create synthetic approved approval from server.rtfs
                    if let Some(approval) = Self::create_approval_from_server_directory(&path, true)
                    {
                        let id = approval.id.clone();
                        self.storage.add(approval).await?;
                        report.created_approved.push(id.clone());
                        known_server_ids.insert(server_id.clone());

                        // Write approval_link.json immediately for the new recovery
                        let link_path = path.join("approval_link.json");
                        let link_data = serde_json::json!({
                            "approval_id": id,
                            "created_at": chrono::Utc::now().to_rfc3339(),
                            "migrated": true,
                            "recovered": true
                        });
                        if let Ok(content) = serde_json::to_string_pretty(&link_data) {
                            let _ = std::fs::write(link_path, content);
                        }
                    }
                }
            }
        }

        // 3. Backfill missing approval_link.json for existing approved servers
        for req in &all_approvals {
            if let ApprovalCategory::ServerDiscovery {
                server_id,
                server_info,
                ..
            } = &req.category
            {
                // Determine directory name (server_id or sanitized name)
                let dir_name = server_id
                    .clone()
                    .unwrap_or_else(|| crate::utils::fs::sanitize_filename(&server_info.name));

                let approved_path = approved_dir.join(&dir_name);
                if approved_path.exists() && approved_path.is_dir() {
                    let link_path = approved_path.join("approval_link.json");
                    if !link_path.exists() {
                        let link_data = serde_json::json!({
                            "approval_id": req.id,
                            "created_at": chrono::Utc::now().to_rfc3339(),
                            "migrated": true
                        });
                        if let Ok(content) = serde_json::to_string_pretty(&link_data) {
                            if std::fs::write(&link_path, content).is_ok() {
                                crate::ccos_println!(
                                    "Migrated: Created approval_link.json for server {}",
                                    dir_name
                                );
                            }
                        }
                    }
                }
            }
        }

        Ok(report)
    }

    /// Create an ApprovalRequest from a server directory's server.rtfs file
    fn create_approval_from_server_directory(
        dir: &std::path::Path,
        is_approved: bool,
    ) -> Option<ApprovalRequest> {
        let server_rtfs = dir.join("server.rtfs");
        if !server_rtfs.exists() {
            return None;
        }

        let content = std::fs::read_to_string(&server_rtfs).ok()?;

        // Extract name from server.rtfs
        let name = Self::extract_rtfs_field(&content, "name")
            .or_else(|| dir.file_name().map(|f| f.to_string_lossy().to_string()))
            .unwrap_or_else(|| "unknown".to_string());

        // Extract endpoint
        let endpoint = Self::extract_rtfs_field(&content, "endpoint").unwrap_or_default();

        // Extract description
        let description = Self::extract_rtfs_field(&content, "description");

        // Extract auth_env_var
        let auth_env_var = Self::extract_rtfs_field(&content, "auth_env_var");

        // Extract version from server.rtfs (default to 1 for legacy servers without version)
        let version = Self::extract_rtfs_field(&content, "version")
            .and_then(|v| v.parse::<u32>().ok())
            .unwrap_or(1);

        let server_id = crate::utils::fs::sanitize_filename(&name);

        let server_info = ServerInfo {
            name: name.clone(),
            endpoint,
            description,
            auth_env_var,
            capabilities_path: Some(dir.to_string_lossy().to_string()),
            alternative_endpoints: vec![],
            capability_files: None,
        };

        let status = if is_approved {
            ApprovalStatus::Approved {
                by: ApprovalAuthority::Auto,
                reason: Some(
                    "Created from orphaned approved directory by filesystem_sync".to_string(),
                ),
                at: Utc::now(),
            }
        } else {
            ApprovalStatus::Pending
        };

        Some(ApprovalRequest {
            id: uuid::Uuid::new_v4().to_string(),
            category: ApprovalCategory::ServerDiscovery {
                source: DiscoverySource::Manual {
                    user: "filesystem_sync".to_string(),
                },
                server_info,
                server_id: Some(server_id),
                version: Some(version),
                domain_match: vec!["recovered".to_string()],
                requesting_goal: None,
                health: None,
                capability_files: None,
            },
            risk_assessment: RiskAssessment {
                level: RiskLevel::Low,
                reasons: vec!["Recovered from filesystem".to_string()],
            },
            requested_at: Utc::now(),
            expires_at: Utc::now() + chrono::Duration::hours(168), // 1 week
            status,
            context: Some("Created by filesystem sync".to_string()),
            metadata: std::collections::HashMap::new(),
            response: None,
        })
    }

    /// Extract a quoted field value from RTFS content
    fn extract_rtfs_field(content: &str, field: &str) -> Option<String> {
        // Look for :field "value" or "field" "value" patterns
        let patterns = [format!(":{}\\s+\"", field), format!("\"{}\"\\s+\"", field)];

        for pattern in patterns {
            if let Ok(re) = regex::Regex::new(&format!("{}([^\"]*)", pattern)) {
                if let Some(cap) = re.captures(content) {
                    return cap.get(1).map(|m| m.as_str().to_string());
                }
            }
        }
        None
    }

    // ========================================================================
    // Effect Approval Specific Operations
    // ========================================================================

    /// Add an effect approval request
    pub async fn add_effect_approval(
        &self,
        capability_id: String,
        effects: Vec<String>,
        intent_description: String,
        risk_assessment: RiskAssessment,
        expires_in_hours: i64,
    ) -> RuntimeResult<String> {
        let request = ApprovalRequest::new(
            ApprovalCategory::EffectApproval {
                capability_id,
                effects,
                intent_description,
            },
            risk_assessment,
            expires_in_hours,
            None,
        );
        self.add(request).await
    }

    /// List pending effect approvals
    pub async fn list_pending_effects(&self) -> RuntimeResult<Vec<ApprovalRequest>> {
        self.list_pending_by_category("EffectApproval").await
    }

    // ========================================================================
    // Budget Extension Approval Operations
    // ========================================================================

    /// Add a budget extension approval request
    pub async fn add_budget_extension(
        &self,
        plan_id: String,
        intent_id: String,
        dimension: String,
        requested_additional: f64,
        consumed: u64,
        limit: u64,
        risk_assessment: RiskAssessment,
        expires_in_hours: i64,
        context: Option<String>,
    ) -> RuntimeResult<String> {
        let request = ApprovalRequest::new(
            ApprovalCategory::BudgetExtension {
                plan_id,
                intent_id,
                dimension,
                requested_additional,
                consumed,
                limit,
            },
            risk_assessment,
            expires_in_hours,
            context,
        );
        self.add(request).await
    }

    /// List pending budget extension approvals
    pub async fn list_pending_budget_extensions(&self) -> RuntimeResult<Vec<ApprovalRequest>> {
        self.list_pending_by_category("BudgetExtension").await
    }

    // ========================================================================
    // LLM Prompt Approval Specific Operations
    // ========================================================================

    /// Add an LLM prompt approval request
    pub async fn add_llm_prompt_approval(
        &self,
        prompt: String,
        risk_reasons: Vec<String>,
        risk_assessment: RiskAssessment,
        expires_in_hours: i64,
    ) -> RuntimeResult<String> {
        let request = ApprovalRequest::new(
            ApprovalCategory::LlmPromptApproval {
                prompt,
                risk_reasons,
            },
            risk_assessment,
            expires_in_hours,
            None,
        );
        self.add(request).await
    }

    /// List pending LLM prompt approvals
    pub async fn list_pending_llm_prompts(&self) -> RuntimeResult<Vec<ApprovalRequest>> {
        self.list_pending_by_category("LlmPromptApproval").await
    }

    // ========================================================================
    // Synthesis Approval Specific Operations
    // ========================================================================

    /// Add a synthesis approval request
    pub async fn add_synthesis_approval(
        &self,
        capability_id: String,
        generated_code: String,
        is_pure: bool,
        risk_assessment: RiskAssessment,
        expires_in_hours: i64,
    ) -> RuntimeResult<String> {
        let request = ApprovalRequest::new(
            ApprovalCategory::SynthesisApproval {
                capability_id,
                generated_code,
                is_pure,
            },
            risk_assessment,
            expires_in_hours,
            None,
        );
        self.add(request).await
    }

    /// List pending synthesis approvals
    pub async fn list_pending_syntheses(&self) -> RuntimeResult<Vec<ApprovalRequest>> {
        self.list_pending_by_category("SynthesisApproval").await
    }

    // ========================================================================
    // Secret Approval Specific Operations
    // ========================================================================

    /// Add a secret approval request
    pub async fn add_secret_approval(
        &self,
        capability_id: String,
        secret_name: String,
        description: String,
        expires_in_hours: i64,
    ) -> RuntimeResult<String> {
        // Check if a pending request already exists for this secret/capability
        let pending = self.list_pending_secrets().await?;
        for req in pending {
            if let ApprovalCategory::SecretRequired {
                capability_id: cid,
                secret_type,
                ..
            } = &req.category
            {
                if cid == &capability_id && secret_type == &secret_name {
                    return Ok(req.id);
                }
            }
        }

        let request = ApprovalRequest::new(
            ApprovalCategory::SecretRequired {
                capability_id,
                secret_type: secret_name,
                description,
            },
            RiskAssessment {
                level: RiskLevel::Medium,
                reasons: vec!["Capability requires external service credentials".to_string()],
            },
            expires_in_hours,
            None,
        );
        self.add(request).await
    }

    /// List pending secret approvals
    pub async fn list_pending_secrets(&self) -> RuntimeResult<Vec<ApprovalRequest>> {
        self.list_pending_by_category("SecretRequired").await
    }

    /// Update a pending server entry in place
    pub async fn update_pending_server(&self, request: &ApprovalRequest) -> RuntimeResult<()> {
        self.storage.update(request).await
    }

    /// Dismiss an approved server (move to rejected status)
    pub async fn dismiss_server(&self, name: &str, reason: String) -> RuntimeResult<()> {
        let approved = self.list_approved_servers().await?;
        let found = approved.iter().find(|r| {
            if let ApprovalCategory::ServerDiscovery { server_info, .. } = &r.category {
                server_info.name == name || r.id == name
            } else {
                false
            }
        });

        if let Some(request) = found {
            let mut updated = request.clone();
            updated.status = ApprovalStatus::Rejected {
                by: ApprovalAuthority::User("cli".to_string()),
                reason,
                at: chrono::Utc::now(),
            };

            // Move artifacts to rejected
            if let ApprovalCategory::ServerDiscovery {
                ref server_info, ..
            } = updated.category
            {
                let _ = self.move_server_directory(&server_info.name, "approved", "rejected");
            }

            self.storage.update(&updated).await?;
            Ok(())
        } else {
            Err(RuntimeError::Generic(format!(
                "Server '{}' not found in approved list",
                name
            )))
        }
    }

    /// Retry a dismissed server (move back to approved with reset health)
    pub async fn retry_server(&self, name: &str) -> RuntimeResult<()> {
        // Get all requests and find the dismissed one
        let filter = ApprovalFilter::default();
        let all_requests = self.storage.list(filter).await?;

        // Also check rejected
        let found = all_requests.iter().find(|r| {
            if let ApprovalCategory::ServerDiscovery { server_info, .. } = &r.category {
                (server_info.name == name || r.id == name)
                    && matches!(r.status, ApprovalStatus::Rejected { .. })
            } else {
                false
            }
        });

        if let Some(request) = found {
            let mut updated = request.clone();
            // Reset to approved with cleared health
            updated.status = ApprovalStatus::Approved {
                by: ApprovalAuthority::User("cli".to_string()),
                reason: Some("Retry after dismiss".to_string()),
                at: chrono::Utc::now(),
            };
            // Reset health tracking
            if let ApprovalCategory::ServerDiscovery {
                ref mut health,
                ref mut capability_files,
                ref server_info,
                ..
            } = updated.category
            {
                *health = Some(ServerHealthTracking::default());

                // Move artifacts back from rejected to approved
                if let Ok(Some(files)) =
                    self.move_server_directory(&server_info.name, "rejected", "approved")
                {
                    *capability_files = Some(files);
                }
            }
            self.storage.update(&updated).await?;
            Ok(())
        } else {
            Err(RuntimeError::Generic(format!(
                "Server '{}' not found in dismissed list",
                name
            )))
        }
    }

    /// Helper to move server artifacts between subdirectories (e.g. pending -> approved)
    fn move_server_directory(
        &self,
        name: &str,
        from_subdir: &str,
        to_subdir: &str,
    ) -> RuntimeResult<Option<Vec<String>>> {
        let server_id = crate::utils::fs::sanitize_filename(name);
        let workspace_root = crate::utils::fs::get_workspace_root();

        let from_dir = workspace_root
            .join("capabilities/servers")
            .join(from_subdir)
            .join(&server_id);
        let to_dir = workspace_root
            .join("capabilities/servers")
            .join(to_subdir)
            .join(&server_id);

        if from_dir.exists() {
            crate::ccos_println!("Moving server from {:?} to {:?}", from_dir, to_dir);
            // Create target parent directory structure
            if let Some(parent) = to_dir.parent() {
                std::fs::create_dir_all(parent).map_err(|e| {
                    RuntimeError::IoError(format!(
                        "Failed to create directory {}: {}",
                        parent.display(),
                        e
                    ))
                })?;
            }

            // Remove existing target dir if it exists
            if to_dir.exists() {
                let _ = std::fs::remove_dir_all(&to_dir);
            }

            // Move
            crate::ccos_println!("Renaming {:?} -> {:?}", from_dir, to_dir);
            std::fs::rename(&from_dir, &to_dir).map_err(|e| {
                RuntimeError::IoError(format!(
                    "Failed to move server directory from {} to {}: {}",
                    from_dir.display(),
                    to_dir.display(),
                    e
                ))
            })?;

            // Collect RTFS files
            crate::ccos_println!("Collecting files from {:?}", to_dir);
            let mut files = Vec::new();

            fn collect_rtfs_files_recursive(
                dir: &std::path::Path,
                files: &mut Vec<String>,
                base: &std::path::Path,
            ) {
                if let Ok(entries) = std::fs::read_dir(dir) {
                    for entry in entries.flatten() {
                        let path = entry.path();
                        if path.is_dir() {
                            collect_rtfs_files_recursive(&path, files, base);
                        } else if path.extension().map_or(false, |ext| ext == "rtfs") {
                            if let Ok(rel) = path.strip_prefix(base) {
                                files.push(rel.to_string_lossy().to_string());
                            }
                        }
                    }
                }
            }

            collect_rtfs_files_recursive(&to_dir, &mut files, &to_dir);
            crate::ccos_println!("Collected {} files", files.len());
            if !files.is_empty() {
                return Ok(Some(files));
            }
        }
        if !from_dir.exists() {
            crate::ccos_eprintln!("Source directory {:?} does NOT exist", from_dir);
        }
        Ok(None)
    }
}

// ========================================================================
// Legacy Type Compatibility - for gradual migration
// ========================================================================

use super::queue::{ApprovedDiscovery, PendingDiscovery};

impl ApprovalRequest {
    /// Convert this ApprovalRequest to legacy PendingDiscovery (for compatibility)
    pub fn to_pending_discovery(&self) -> Option<PendingDiscovery> {
        if let ApprovalCategory::ServerDiscovery {
            source,
            server_info,
            domain_match,
            requesting_goal,
            ..
        } = &self.category
        {
            Some(PendingDiscovery {
                id: self.id.clone(),
                source: source.clone(),
                server_info: server_info.clone(),
                domain_match: !domain_match.is_empty(), // Convert Vec<String> to bool
                risk_assessment: Some(self.risk_assessment.clone()), // Convert RA to Option<RA>
                requested_at: self.requested_at,
                expires_at: self.expires_at,
                requesting_goal: requesting_goal.clone(),
                capability_files: None, // Will need to be handled if crucial, but Option makes `None` safe
            })
        } else {
            None
        }
    }

    /// Convert this ApprovalRequest to legacy ApprovedDiscovery (for compatibility)
    pub fn to_approved_discovery(&self) -> Option<ApprovedDiscovery> {
        if let ApprovalCategory::ServerDiscovery {
            source,
            server_info,
            server_id: _,
            version: _,
            domain_match,
            requesting_goal,
            health,
            capability_files,
        } = &self.category
        {
            if let ApprovalStatus::Approved { by, reason, at } = &self.status {
                let health_data = health.as_ref();
                Some(ApprovedDiscovery {
                    id: self.id.clone(),
                    source: source.clone(),
                    server_info: server_info.clone(),
                    domain_match: !domain_match.is_empty(), // Convert Vec<String> to bool
                    risk_assessment: Some(self.risk_assessment.clone()),
                    requesting_goal: requesting_goal.clone(),
                    approved_at: *at,
                    approved_by: by.clone(),
                    approval_reason: reason.clone(),
                    capability_files: capability_files.clone(),
                    version: health_data.map(|h| h.version).unwrap_or(1),
                    last_successful_call: health_data.and_then(|h| h.last_successful_call),
                    consecutive_failures: health_data.map(|h| h.consecutive_failures).unwrap_or(0),
                    total_calls: health_data.map(|h| h.total_calls).unwrap_or(0),
                    total_errors: health_data.map(|h| h.total_errors).unwrap_or(0),
                })
            } else {
                None
            }
        } else {
            None
        }
    }

    /// Create from a legacy PendingDiscovery
    pub fn from_pending_discovery(pd: &PendingDiscovery) -> Self {
        ApprovalRequest {
            id: pd.id.clone(),
            category: ApprovalCategory::ServerDiscovery {
                source: pd.source.clone(),
                server_info: pd.server_info.clone(),
                server_id: Some(crate::utils::fs::sanitize_filename(&pd.server_info.name)),
                version: Some(1),
                domain_match: if pd.domain_match {
                    vec!["legacy_match".to_string()]
                } else {
                    vec![]
                }, // Convert bool to Vec
                requesting_goal: pd.requesting_goal.clone(),
                health: None,
                capability_files: pd.capability_files.clone(),
            },
            risk_assessment: pd
                .risk_assessment
                .clone()
                .unwrap_or_else(|| RiskAssessment {
                    level: RiskLevel::Medium,
                    reasons: vec!["Legacy request - no assessment".to_string()],
                }), // Handle Option
            requested_at: pd.requested_at,
            expires_at: pd.expires_at,
            status: ApprovalStatus::Pending,
            context: None,
            metadata: std::collections::HashMap::new(),
            response: None,
        }
    }
}

// ========================================================================
// Helper for suggesting auth env var (moved from legacy ApprovalQueue)
// ========================================================================

/// Suggest environment variable name for authentication token based on server name
pub fn suggest_auth_env_var(server_name: &str) -> String {
    let (namespace, is_web_api) = if server_name.starts_with("web/") {
        let parts: Vec<&str> = server_name.split('/').collect();
        if parts.len() >= 3 {
            (parts[2], true)
        } else if parts.len() == 2 {
            (parts[1], true)
        } else {
            (server_name, true)
        }
    } else if server_name.contains("/mcp") || server_name.ends_with("-mcp") {
        if let Some(slash_pos) = server_name.find('/') {
            (&server_name[..slash_pos], false)
        } else {
            (server_name, false)
        }
    } else if server_name.starts_with("apis.guru/") {
        let parts: Vec<&str> = server_name.split('/').collect();
        if parts.len() >= 2 {
            (parts[1], true)
        } else {
            (server_name, true)
        }
    } else {
        if let Some(slash_pos) = server_name.find('/') {
            (&server_name[..slash_pos], false)
        } else {
            (server_name, false)
        }
    };

    let normalized = namespace.replace('-', "_").to_uppercase();

    if is_web_api {
        format!("{}_API_KEY", normalized)
    } else {
        format!("{}_MCP_TOKEN", normalized)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::approval::queue::RiskLevel;
    use crate::approval::storage_memory::InMemoryApprovalStorage;

    fn create_test_queue() -> UnifiedApprovalQueue<InMemoryApprovalStorage> {
        let storage = Arc::new(InMemoryApprovalStorage::new());
        UnifiedApprovalQueue::new(storage)
    }

    #[tokio::test]
    async fn test_add_and_get() {
        let queue = create_test_queue();

        let expiry_hours = 24; // Define expiry_hours
        let request = ApprovalRequest::new(
            ApprovalCategory::EffectApproval {
                capability_id: "test.cap".to_string(),
                effects: vec!["read".to_string()],
                intent_description: "Test intent".to_string(),
            },
            RiskAssessment {
                level: RiskLevel::Low,
                reasons: vec![],
            },
            expiry_hours,
            None,
        );

        let id = queue.add(request).await.unwrap();
        let retrieved = queue.get(&id).await.unwrap();
        assert!(retrieved.is_some());
    }

    #[tokio::test]
    async fn test_approve_flow() {
        let queue = create_test_queue();

        let request = ApprovalRequest::new(
            ApprovalCategory::EffectApproval {
                capability_id: "test.cap".to_string(),
                effects: vec!["network".to_string()],
                intent_description: "Test".to_string(),
            },
            RiskAssessment {
                level: RiskLevel::Medium,
                reasons: vec!["network access".to_string()],
            },
            24,
            None,
        );

        let id = queue.add(request).await.unwrap();

        // Should be pending
        let pending = queue.list_pending().await.unwrap();
        assert_eq!(pending.len(), 1);

        // Approve
        queue
            .approve(
                &id,
                ApprovalAuthority::Auto,
                Some("Test approved".to_string()),
            )
            .await
            .unwrap();

        // Should no longer be pending
        let pending = queue.list_pending().await.unwrap();
        assert!(pending.is_empty());
    }

    #[tokio::test]
    async fn test_server_discovery_flow() {
        let queue = create_test_queue();

        let id = queue
            .add_server_discovery(
                DiscoverySource::Manual {
                    user: "test".to_string(),
                },
                ServerInfo {
                    name: "test-server".to_string(),
                    endpoint: "http://localhost:8080".to_string(),
                    description: Some("Test server".to_string()),
                    auth_env_var: None,
                    capabilities_path: None,
                    alternative_endpoints: vec![],
                    capability_files: None,
                },
                vec!["test".to_string()],
                RiskAssessment {
                    level: RiskLevel::Low,
                    reasons: vec![],
                },
                None,
                24,
            )
            .await
            .unwrap();

        // List pending servers
        let pending = queue.list_pending_servers().await.unwrap();
        assert_eq!(pending.len(), 1);

        // Approve
        queue
            .approve_server(
                &id,
                ApprovalAuthority::User("admin".to_string()),
                Some("Approved".to_string()),
            )
            .await
            .unwrap();

        // Should be approved now
        let approved = queue.list_approved_servers().await.unwrap();
        assert_eq!(approved.len(), 1);
    }

    #[test]
    fn test_suggest_auth_env_var() {
        assert_eq!(
            suggest_auth_env_var("github/github-mcp"),
            "GITHUB_MCP_TOKEN"
        );
        assert_eq!(
            suggest_auth_env_var("web/api/openweathermap"),
            "OPENWEATHERMAP_API_KEY"
        );
    }

    #[tokio::test]
    async fn test_llm_prompt_approval_flow() {
        let queue = create_test_queue();

        let id = queue
            .add_llm_prompt_approval(
                "Write code to delete all files".to_string(),
                vec![
                    "destructive operation".to_string(),
                    "file system access".to_string(),
                ],
                RiskAssessment {
                    level: RiskLevel::High,
                    reasons: vec!["Potentially destructive".to_string()],
                },
                24,
            )
            .await
            .unwrap();

        // List pending LLM prompts
        let pending = queue.list_pending_llm_prompts().await.unwrap();
        assert_eq!(pending.len(), 1);

        // Check that it has the right prompt
        if let ApprovalCategory::LlmPromptApproval {
            prompt,
            risk_reasons,
        } = &pending[0].category
        {
            assert!(prompt.contains("delete"));
            assert_eq!(risk_reasons.len(), 2);
        } else {
            panic!("Expected LlmPromptApproval category");
        }

        // Approve it
        queue
            .approve(
                &id,
                ApprovalAuthority::User("admin".to_string()),
                Some("Reviewed and approved".to_string()),
            )
            .await
            .unwrap();

        // Should no longer be pending
        let pending = queue.list_pending_llm_prompts().await.unwrap();
        assert_eq!(pending.len(), 0);
    }

    #[tokio::test]
    async fn test_synthesis_approval_flow() {
        let queue = create_test_queue();

        let id = queue
            .add_synthesis_approval(
                "generated.my_capability".to_string(),
                "(defn my-capability [x] (+ x 1))".to_string(),
                true, // is_pure
                RiskAssessment {
                    level: RiskLevel::Low,
                    reasons: vec!["Auto-generated code".to_string()],
                },
                24,
            )
            .await
            .unwrap();

        // List pending syntheses
        let pending = queue.list_pending_syntheses().await.unwrap();
        assert_eq!(pending.len(), 1);

        // Check that it has the right capability ID
        if let ApprovalCategory::SynthesisApproval {
            capability_id,
            is_pure,
            ..
        } = &pending[0].category
        {
            assert_eq!(capability_id, "generated.my_capability");
            assert!(is_pure);
        } else {
            panic!("Expected SynthesisApproval category");
        }

        // Reject it
        queue
            .reject(
                &id,
                ApprovalAuthority::User("reviewer".to_string()),
                "Code quality issues".to_string(),
            )
            .await
            .unwrap();

        // Should no longer be pending
        let pending = queue.list_pending_syntheses().await.unwrap();
        assert_eq!(pending.len(), 0);
    }
}
