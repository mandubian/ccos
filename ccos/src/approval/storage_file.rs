//! File-based implementation of the ApprovalStorage trait.
//!
//! This provides persistent storage for approval requests using the filesystem.
//! Each approval request is stored as a JSON file in a designated directory.

use super::types::{ApprovalCategory, ApprovalFilter, ApprovalRequest, ApprovalStorage};
use async_trait::async_trait;
use rtfs::runtime::error::{RuntimeError, RuntimeResult};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::RwLock;

/// File-based implementation of ApprovalStorage.
///
/// Stores approval requests as individual JSON files in a directory structure.
/// This is the default production storage backend.
pub struct FileApprovalStorage {
    /// Base directory for storing approval requests
    base_path: PathBuf,
    /// In-memory cache of loaded requests (interior mutability for trait compat)
    cache: RwLock<HashMap<String, ApprovalRequest>>,
}

impl FileApprovalStorage {
    /// Create a new FileApprovalStorage with the given base path.
    pub fn new(base_path: PathBuf) -> RuntimeResult<Self> {
        // Ensure base directory exists
        if !base_path.exists() {
            std::fs::create_dir_all(&base_path).map_err(|e| {
                RuntimeError::IoError(format!(
                    "Failed to create approval storage directory: {}",
                    e
                ))
            })?;
        }

        // Create subdirectories for each status
        for status in &["pending", "approved", "rejected", "expired"] {
            let status_path = base_path.join(status);
            if !status_path.exists() {
                std::fs::create_dir_all(&status_path).map_err(|e| {
                    RuntimeError::IoError(format!(
                        "Failed to create approval subdirectory {}: {}",
                        status, e
                    ))
                })?;
            }
        }

        let storage = Self {
            base_path,
            cache: RwLock::new(HashMap::new()),
        };

        // Migration: Move any JSON files from root to appropriate subdirectories
        storage.migrate_legacy_files()?;

        // Load existing requests into cache
        storage.load_all()?;

        Ok(storage)
    }

    /// Get the subdirectory name for a given status
    fn get_status_dir_name(status: &super::types::ApprovalStatus) -> &'static str {
        match status {
            super::types::ApprovalStatus::Pending => "pending",
            super::types::ApprovalStatus::Approved { .. } => "approved",
            super::types::ApprovalStatus::Rejected { .. } => "rejected",
            super::types::ApprovalStatus::Expired { .. } => "expired",
        }
    }

    /// Get the full path for a request based on its ID and status
    fn get_request_path_for_status(
        &self,
        id: &str,
        status: &super::types::ApprovalStatus,
    ) -> PathBuf {
        self.base_path
            .join(Self::get_status_dir_name(status))
            .join(format!("{}.json", id))
    }

    /// Migrate legacy files from root directory to subdirectories
    fn migrate_legacy_files(&self) -> RuntimeResult<()> {
        let entries = std::fs::read_dir(&self.base_path).map_err(|e| {
            RuntimeError::IoError(format!("Failed to read approval storage directory: {}", e))
        })?;

        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_file() && path.extension().map_or(false, |ext| ext == "json") {
                // Try to load the request to determine its status
                match self.load_request_from_file(&path) {
                    Ok(request) => {
                        let new_path =
                            self.get_request_path_for_status(&request.id, &request.status);

                        // Move file
                        if let Err(e) = std::fs::rename(&path, &new_path) {
                            eprintln!(
                                "[APPROVAL_STORAGE] Failed to migrate file {} to {}: {}",
                                path.display(),
                                new_path.display(),
                                e
                            );
                        } else {
                            println!(
                                "[APPROVAL_STORAGE] Migrated {} to {}",
                                path.display(),
                                new_path.display()
                            );
                        }
                    }
                    Err(e) => {
                        eprintln!(
                            "[APPROVAL_STORAGE] Failed to load legacy file {} for migration: {}",
                            path.display(),
                            e
                        );
                    }
                }
            }
        }
        Ok(())
    }

    /// Load all requests from all subdirectories into cache
    fn load_all(&self) -> RuntimeResult<()> {
        let mut cache = self.cache.write().map_err(|_| {
            RuntimeError::IoError("Failed to acquire write lock on cache".to_string())
        })?;

        for status in &["pending", "approved", "rejected", "expired"] {
            let status_dir = self.base_path.join(status);
            if let Ok(entries) = std::fs::read_dir(&status_dir) {
                for entry in entries.flatten() {
                    let path = entry.path();
                    if path.extension().map_or(false, |ext| ext == "json") {
                        match self.load_request_from_file(&path) {
                            Ok(request) => {
                                cache.insert(request.id.clone(), request);
                            }
                            Err(e) => {
                                eprintln!(
                                    "[APPROVAL_STORAGE] Failed to load {}: {}",
                                    path.display(),
                                    e
                                );
                            }
                        }
                    }
                }
            }
        }

        Ok(())
    }

    /// Load a single request from a file
    fn load_request_from_file(&self, path: &PathBuf) -> RuntimeResult<ApprovalRequest> {
        let content = std::fs::read_to_string(path).map_err(|e| {
            RuntimeError::IoError(format!(
                "Failed to read approval file {}: {}",
                path.display(),
                e
            ))
        })?;

        serde_json::from_str(&content).map_err(|e| {
            RuntimeError::IoError(format!(
                "Failed to parse approval file {}: {}",
                path.display(),
                e
            ))
        })
    }

    /// Save a request to disk in the correct subdirectory
    fn save_request(&self, request: &ApprovalRequest) -> RuntimeResult<()> {
        let path = self.get_request_path_for_status(&request.id, &request.status);
        let content = serde_json::to_string_pretty(request).map_err(|e| {
            RuntimeError::IoError(format!("Failed to serialize approval request: {}", e))
        })?;

        std::fs::write(&path, content).map_err(|e| {
            RuntimeError::IoError(format!(
                "Failed to write approval file {}: {}",
                path.display(),
                e
            ))
        })?;

        Ok(())
    }

    /// Delete a request file from disk (checking all possible locations if needed,
    /// but primarily relies on status if known)
    fn delete_request_file(&self, id: &str) -> RuntimeResult<()> {
        // Since we might not know the status when deleting by ID only (if not in cache),
        // we should try to find it in all subdirectories.
        // However, usually `remove` is called with the ID, and we check the cache first.
        // But to be safe and thorough, we check all dirs.

        let mut deleted = false;
        for status in &["pending", "approved", "rejected", "expired"] {
            let path = self.base_path.join(status).join(format!("{}.json", id));
            if path.exists() {
                std::fs::remove_file(&path).map_err(|e| {
                    RuntimeError::IoError(format!(
                        "Failed to delete approval file {}: {}",
                        path.display(),
                        e
                    ))
                })?;
                deleted = true;
            }
        }

        // Also check legacy root for completeness
        let legacy_path = self.base_path.join(format!("{}.json", id));
        if legacy_path.exists() {
            std::fs::remove_file(&legacy_path).map_err(|e| {
                RuntimeError::IoError(format!(
                    "Failed to delete legacy approval file {}: {}",
                    legacy_path.display(),
                    e
                ))
            })?;
        }

        Ok(())
    }

    /// Check if a request matches a filter
    fn matches_filter(request: &ApprovalRequest, filter: &ApprovalFilter) -> bool {
        // Filter by category type
        if let Some(ref category_type) = filter.category_type {
            let request_type = match &request.category {
                ApprovalCategory::ServerDiscovery { .. } => "ServerDiscovery",
                ApprovalCategory::EffectApproval { .. } => "EffectApproval",
                ApprovalCategory::SynthesisApproval { .. } => "SynthesisApproval",
                ApprovalCategory::LlmPromptApproval { .. } => "LlmPromptApproval",
                ApprovalCategory::SecretRequired { .. } => "SecretRequired",
            };
            if request_type != category_type {
                return false;
            }
        }

        // Filter by pending status
        if let Some(pending) = filter.status_pending {
            if pending != request.status.is_pending() {
                return false;
            }
        }

        true
    }
}

#[async_trait]
impl ApprovalStorage for FileApprovalStorage {
    async fn add(&self, request: ApprovalRequest) -> RuntimeResult<()> {
        // Save to disk first
        self.save_request(&request)?;
        // Then add to cache
        let mut cache = self.cache.write().map_err(|_| {
            RuntimeError::IoError("Failed to acquire write lock on cache".to_string())
        })?;
        cache.insert(request.id.clone(), request);
        Ok(())
    }

    async fn update(&self, request: &ApprovalRequest) -> RuntimeResult<()> {
        let old_dir_name = {
            let cache = self.cache.read().map_err(|_| {
                RuntimeError::IoError("Failed to acquire read lock on cache".to_string())
            })?;
            match cache.get(&request.id) {
                Some(r) => Some(Self::get_status_dir_name(&r.status)),
                None => None,
            }
        };

        if let Some(old_dir) = old_dir_name {
            // Check if status changed (directory changed) and if so, remove old file
            let new_dir = Self::get_status_dir_name(&request.status);

            if old_dir != new_dir {
                let old_path = self
                    .base_path
                    .join(old_dir)
                    .join(format!("{}.json", request.id));
                if old_path.exists() {
                    // Best effort cleanup
                    let _ = std::fs::remove_file(&old_path);
                }
            }
        } else {
            return Err(RuntimeError::IoError(format!(
                "Approval request not found: {}",
                request.id
            )));
        }

        // Save to disk (new path)
        self.save_request(request)?;

        // Update cache
        let mut cache = self.cache.write().map_err(|_| {
            RuntimeError::IoError("Failed to acquire write lock on cache".to_string())
        })?;
        cache.insert(request.id.clone(), request.clone());
        Ok(())
    }

    async fn get(&self, id: &str) -> RuntimeResult<Option<ApprovalRequest>> {
        let cache = self.cache.read().map_err(|_| {
            RuntimeError::IoError("Failed to acquire read lock on cache".to_string())
        })?;
        Ok(cache.get(id).cloned())
    }

    async fn list(&self, filter: ApprovalFilter) -> RuntimeResult<Vec<ApprovalRequest>> {
        let cache = self.cache.read().map_err(|_| {
            RuntimeError::IoError("Failed to acquire read lock on cache".to_string())
        })?;
        let mut results: Vec<ApprovalRequest> = cache
            .values()
            .filter(|req| Self::matches_filter(req, &filter))
            .cloned()
            .collect();

        // Sort by requested_at descending (most recent first)
        results.sort_by(|a, b| b.requested_at.cmp(&a.requested_at));

        // Apply limit if specified
        if let Some(limit) = filter.limit {
            results.truncate(limit);
        }

        Ok(results)
    }

    async fn remove(&self, id: &str) -> RuntimeResult<bool> {
        let mut cache = self.cache.write().map_err(|_| {
            RuntimeError::IoError("Failed to acquire write lock on cache".to_string())
        })?;
        if cache.remove(id).is_some() {
            drop(cache);
            self.delete_request_file(id)?;
            Ok(true)
        } else {
            Ok(false)
        }
    }

    async fn check_expirations(&self) -> RuntimeResult<Vec<String>> {
        let now = chrono::Utc::now();
        let cache = self.cache.read().map_err(|_| {
            RuntimeError::IoError("Failed to acquire read lock on cache".to_string())
        })?;
        let mut expired_ids = Vec::new();

        // Find expired requests (those past their expires_at and still pending)
        for req in cache.values() {
            if req.status.is_pending() && req.expires_at < now {
                expired_ids.push(req.id.clone());
            }
        }

        Ok(expired_ids)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::approval::queue::{ApprovalAuthority, RiskAssessment, RiskLevel};
    use crate::approval::{DiscoverySource, ServerInfo};
    use tempfile::tempdir;

    fn create_test_request(id: &str, is_server_discovery: bool) -> ApprovalRequest {
        let category = if is_server_discovery {
            ApprovalCategory::ServerDiscovery {
                source: DiscoverySource::Manual {
                    user: "test".to_string(),
                },
                server_info: ServerInfo {
                    name: "test-server".to_string(),
                    endpoint: "http://localhost:8080".to_string(),
                    description: Some("Test server".to_string()),
                    auth_env_var: None,
                    capabilities_path: None,
                    alternative_endpoints: vec![],
                },
                domain_match: vec!["test".to_string()],
                requesting_goal: None,
                health: None,
                capability_files: None,
            }
        } else {
            ApprovalCategory::EffectApproval {
                capability_id: "test-cap".to_string(),
                effects: vec!["read".to_string()],
                intent_description: "test intent".to_string(),
            }
        };
        let mut request = ApprovalRequest::new(
            category,
            RiskAssessment {
                level: RiskLevel::Low,
                reasons: vec!["test".to_string()],
            },
            24, // expires in 24 hours
            Some("test context".to_string()),
        );
        request.id = id.to_string(); // Override auto-generated ID for testing
        request
    }

    #[tokio::test]
    async fn test_file_storage_add_and_get() {
        let dir = tempdir().unwrap();
        let mut storage = FileApprovalStorage::new(dir.path().to_path_buf()).unwrap();

        let request = create_test_request("test-1", true);
        storage.add(request.clone()).await.unwrap();

        let retrieved = storage.get("test-1").await.unwrap();
        assert!(retrieved.is_some());
        assert_eq!(retrieved.unwrap().id, "test-1");
    }

    #[tokio::test]
    async fn test_file_storage_persistence() {
        let dir = tempdir().unwrap();
        let path = dir.path().to_path_buf();

        // Add a request
        {
            let mut storage = FileApprovalStorage::new(path.clone()).unwrap();
            let request = create_test_request("persist-1", false);
            storage.add(request).await.unwrap();
        }

        // Reload and verify
        {
            let storage = FileApprovalStorage::new(path).unwrap();
            let retrieved = storage.get("persist-1").await.unwrap();
            assert!(retrieved.is_some());
        }
    }

    #[tokio::test]
    async fn test_file_storage_remove() {
        let dir = tempdir().unwrap();
        let mut storage = FileApprovalStorage::new(dir.path().to_path_buf()).unwrap();

        let request = create_test_request("remove-1", true);
        storage.add(request).await.unwrap();

        let removed = storage.remove("remove-1").await.unwrap();
        assert!(removed);

        let retrieved = storage.get("remove-1").await.unwrap();
        assert!(retrieved.is_none());
    }

    #[tokio::test]
    async fn test_file_storage_migration() {
        let dir = tempdir().unwrap();
        let path = dir.path().to_path_buf();

        // 1. Manually create a legacy file in the root
        let request = create_test_request("legacy-1", true); // Status is Pending by default
        let content = serde_json::to_string_pretty(&request).unwrap();
        let legacy_file = path.join("legacy-1.json");
        std::fs::write(&legacy_file, content).unwrap();

        assert!(legacy_file.exists());
        assert!(!path.join("pending").join("legacy-1.json").exists());

        // 2. Initialize storage, which should trigger migration
        let storage = FileApprovalStorage::new(path.clone()).unwrap();

        // 3. Verify file moved
        assert!(!legacy_file.exists());
        let migrated_file = path.join("pending").join("legacy-1.json");
        assert!(migrated_file.exists());

        // 4. Verify we can still get it
        let retrieved = storage.get("legacy-1").await.unwrap();
        assert!(retrieved.is_some());
    }

    #[tokio::test]
    async fn test_file_storage_status_change_move() {
        let dir = tempdir().unwrap();
        let path = dir.path().to_path_buf();
        let storage = FileApprovalStorage::new(path.clone()).unwrap();

        // 1. Add pending request
        let mut request = create_test_request("move-test-1", true);
        storage.add(request.clone()).await.unwrap();

        assert!(path.join("pending").join("move-test-1.json").exists());
        assert!(!path.join("approved").join("move-test-1.json").exists());

        // 2. Approve it
        request.approve(
            ApprovalAuthority::User("tester".to_string()),
            Some("Looks good".to_string()),
        );
        storage.update(&request).await.unwrap();

        // 3. Verify file moved
        assert!(
            path.join("approved").join("move-test-1.json").exists(),
            "Approved file should exist"
        );
        assert!(
            !path.join("pending").join("move-test-1.json").exists(),
            "Pending file should NOT exist"
        );
    }
}
