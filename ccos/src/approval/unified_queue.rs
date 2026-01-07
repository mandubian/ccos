//! Unified Approval Queue
//!
//! A generic approval queue that works with the `ApprovalStorage` trait
//! for backend-agnostic storage. Replaces the legacy file-based ApprovalQueue.

use super::queue::{ApprovalAuthority, DiscoverySource, RiskAssessment, ServerInfo};
use super::types::{
    ApprovalCategory, ApprovalFilter, ApprovalRequest, ApprovalStatus, ApprovalStorage,
    ServerHealthTracking,
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
}

impl<S: ApprovalStorage> Clone for UnifiedApprovalQueue<S> {
    fn clone(&self) -> Self {
        Self {
            storage: Arc::clone(&self.storage),
        }
    }
}

impl<S: ApprovalStorage> UnifiedApprovalQueue<S> {
    /// Create a new unified approval queue with the given storage backend
    pub fn new(storage: Arc<S>) -> Self {
        Self { storage }
    }

    // ========================================================================
    // Generic Operations (work with any ApprovalCategory)
    // ========================================================================

    /// Add a new approval request
    pub async fn add(&self, request: ApprovalRequest) -> RuntimeResult<String> {
        let id = request.id.clone();
        self.storage.add(request).await?;
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
                self.storage.update(&request).await
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

        request.reject(by, reason);
        self.storage.update(&request).await
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
    pub async fn add_server_discovery(
        &self,
        source: DiscoverySource,
        server_info: ServerInfo,
        domain_match: Vec<String>,
        risk_assessment: RiskAssessment,
        requesting_goal: Option<String>,
        expires_in_hours: i64,
    ) -> RuntimeResult<String> {
        let request = ApprovalRequest::new(
            ApprovalCategory::ServerDiscovery {
                source,
                server_info,
                domain_match,
                requesting_goal,
                health: None,
                capability_files: None,
            },
            risk_assessment,
            expires_in_hours,
            None,
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
            let server_id = crate::utils::fs::sanitize_filename(&server_info.name);

            // Get workspace root for file paths
            let workspace_root = crate::utils::fs::get_workspace_root();
            let pending_dir = workspace_root
                .join("capabilities/servers/pending")
                .join(&server_id);
            let approved_dir = workspace_root
                .join("capabilities/servers/approved")
                .join(&server_id);

            if pending_dir.exists() {
                // Create approved directory structure
                if let Err(e) =
                    std::fs::create_dir_all(approved_dir.parent().unwrap_or(&approved_dir))
                {
                    eprintln!("[CCOS] Warning: Failed to create approved directory: {}", e);
                }

                // Remove existing approved dir if it exists
                if approved_dir.exists() {
                    let _ = std::fs::remove_dir_all(&approved_dir);
                }

                // Move from pending to approved
                if let Err(e) = std::fs::rename(&pending_dir, &approved_dir) {
                    eprintln!(
                        "[CCOS] Warning: Failed to move capabilities from pending to approved: {}",
                        e
                    );
                } else {
                    eprintln!(
                        "[CCOS] Moved capabilities from {} to {}",
                        pending_dir.display(),
                        approved_dir.display()
                    );

                    // Collect all RTFS files from the approved directory
                    let mut files = Vec::new();
                    fn collect_rtfs_files(
                        dir: &std::path::Path,
                        files: &mut Vec<String>,
                        base: &std::path::Path,
                    ) {
                        if let Ok(entries) = std::fs::read_dir(dir) {
                            for entry in entries.flatten() {
                                let path = entry.path();
                                if path.is_dir() {
                                    collect_rtfs_files(&path, files, base);
                                } else if path.extension().map_or(false, |ext| ext == "rtfs") {
                                    if let Ok(rel) = path.strip_prefix(base) {
                                        files.push(rel.to_string_lossy().to_string());
                                    }
                                }
                            }
                        }
                    }
                    collect_rtfs_files(&approved_dir, &mut files, &approved_dir);

                    if !files.is_empty() {
                        *capability_files = Some(files);
                    }
                }
            }
        }

        request.approve(by, reason);
        self.storage.update(&request).await
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
            if let ApprovalCategory::ServerDiscovery { ref mut health, .. } = updated.category {
                *health = Some(ServerHealthTracking::default());
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
                domain_match: domain_match.clone(),
                risk_assessment: self.risk_assessment.clone(),
                requested_at: self.requested_at,
                expires_at: self.expires_at,
                requesting_goal: requesting_goal.clone(),
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
                    domain_match: domain_match.clone(),
                    risk_assessment: self.risk_assessment.clone(),
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
                domain_match: pd.domain_match.clone(),
                requesting_goal: pd.requesting_goal.clone(),
                health: None,
                capability_files: None,
            },
            risk_assessment: pd.risk_assessment.clone(),
            requested_at: pd.requested_at,
            expires_at: pd.expires_at,
            status: ApprovalStatus::Pending,
            context: None,
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
            24,
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
