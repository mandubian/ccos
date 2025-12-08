//! Capability Versioning System
//!
//! Provides version tracking and rollback support for synthesized capabilities,
//! enabling safe AI self-programming with recovery from failures.

use std::collections::{HashMap, VecDeque};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::RwLock;
use std::time::{SystemTime, UNIX_EPOCH};

/// Maximum versions to keep per capability
const DEFAULT_MAX_VERSIONS: usize = 10;

/// Tracks capability file versions for rollback support
#[derive(Debug)]
pub struct CapabilityVersionStore {
    /// Storage directory for version backups
    storage_dir: PathBuf,
    /// In-memory version history (capability_id -> versions)
    versions: RwLock<HashMap<String, VecDeque<CapabilityVersion>>>,
    /// Maximum versions to retain per capability
    max_versions: usize,
}

/// A single version snapshot of a capability
#[derive(Debug, Clone)]
pub struct CapabilityVersion {
    /// Version identifier (timestamp-based)
    pub version_id: String,
    /// Capability ID this version belongs to
    pub capability_id: String,
    /// Original file path
    pub original_path: PathBuf,
    /// Backup file path (in version store)
    pub backup_path: PathBuf,
    /// Timestamp when version was created
    pub created_at: u64,
    /// Reason for versioning (synthesis, update, etc.)
    pub reason: String,
    /// Whether this version has been rolled back
    pub rolled_back: bool,
}

/// Result of a rollback operation
#[derive(Debug)]
pub enum RollbackResult {
    /// Successfully rolled back to version
    Success {
        version_id: String,
        restored_path: PathBuf,
    },
    /// No previous version available
    NoPreviousVersion { capability_id: String },
    /// Rollback failed
    Failed {
        capability_id: String,
        error: String,
    },
}

impl CapabilityVersionStore {
    /// Create a new version store with the given storage directory
    pub fn new(storage_dir: PathBuf) -> std::io::Result<Self> {
        // Ensure storage directory exists
        fs::create_dir_all(&storage_dir)?;

        Ok(Self {
            storage_dir,
            versions: RwLock::new(HashMap::new()),
            max_versions: DEFAULT_MAX_VERSIONS,
        })
    }

    /// Create a new version store with custom max versions
    pub fn with_max_versions(storage_dir: PathBuf, max_versions: usize) -> std::io::Result<Self> {
        fs::create_dir_all(&storage_dir)?;

        Ok(Self {
            storage_dir,
            versions: RwLock::new(HashMap::new()),
            max_versions,
        })
    }

    /// Create a version snapshot before modifying a capability
    pub fn create_version(
        &self,
        capability_id: &str,
        original_path: &Path,
        reason: &str,
    ) -> std::io::Result<CapabilityVersion> {
        // Generate version ID using nanoseconds for uniqueness
        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default();
        let nanos = timestamp.as_nanos() as u64;
        let millis = timestamp.as_millis() as u64;
        let version_id = format!("{}_{}", capability_id.replace('/', "_"), nanos);

        // Create backup directory for this capability
        let cap_dir = self.storage_dir.join(capability_id.replace('/', "_"));
        fs::create_dir_all(&cap_dir)?;

        // Copy file to backup location
        let backup_path = cap_dir.join(format!("{}.rtfs", version_id));
        if original_path.exists() {
            fs::copy(original_path, &backup_path)?;
        } else {
            // Create empty placeholder if original doesn't exist yet
            fs::write(&backup_path, "")?;
        }

        let version = CapabilityVersion {
            version_id: version_id.clone(),
            capability_id: capability_id.to_string(),
            original_path: original_path.to_path_buf(),
            backup_path,
            created_at: millis,
            reason: reason.to_string(),
            rolled_back: false,
        };

        // Add to in-memory history
        if let Ok(mut versions) = self.versions.write() {
            let history = versions
                .entry(capability_id.to_string())
                .or_insert_with(VecDeque::new);

            history.push_front(version.clone());

            // Prune old versions
            while history.len() > self.max_versions {
                if let Some(old) = history.pop_back() {
                    // Clean up old backup file
                    let _ = fs::remove_file(&old.backup_path);
                }
            }
        }

        Ok(version)
    }

    /// Rollback a capability to its previous version
    pub fn rollback(&self, capability_id: &str) -> RollbackResult {
        let version = {
            let versions = match self.versions.read() {
                Ok(v) => v,
                Err(e) => {
                    return RollbackResult::Failed {
                        capability_id: capability_id.to_string(),
                        error: format!("Lock error: {}", e),
                    }
                }
            };

            match versions.get(capability_id) {
                Some(history) if history.len() > 1 => {
                    // Get the previous version (index 1, not 0 which is current)
                    history.get(1).cloned()
                }
                Some(history) if !history.is_empty() => {
                    // Only one version - rollback to it
                    history.front().cloned()
                }
                _ => None,
            }
        };

        match version {
            Some(v) => {
                // Restore the backup
                if v.backup_path.exists() {
                    match fs::copy(&v.backup_path, &v.original_path) {
                        Ok(_) => {
                            // Mark as rolled back
                            if let Ok(mut versions) = self.versions.write() {
                                if let Some(history) = versions.get_mut(capability_id) {
                                    if let Some(current) = history.front_mut() {
                                        current.rolled_back = true;
                                    }
                                }
                            }

                            RollbackResult::Success {
                                version_id: v.version_id,
                                restored_path: v.original_path,
                            }
                        }
                        Err(e) => RollbackResult::Failed {
                            capability_id: capability_id.to_string(),
                            error: format!("Failed to restore: {}", e),
                        },
                    }
                } else {
                    RollbackResult::Failed {
                        capability_id: capability_id.to_string(),
                        error: "Backup file not found".to_string(),
                    }
                }
            }
            None => RollbackResult::NoPreviousVersion {
                capability_id: capability_id.to_string(),
            },
        }
    }

    /// Rollback to a specific version ID
    pub fn rollback_to_version(&self, capability_id: &str, version_id: &str) -> RollbackResult {
        let version = {
            let versions = match self.versions.read() {
                Ok(v) => v,
                Err(e) => {
                    return RollbackResult::Failed {
                        capability_id: capability_id.to_string(),
                        error: format!("Lock error: {}", e),
                    }
                }
            };

            versions
                .get(capability_id)
                .and_then(|history| history.iter().find(|v| v.version_id == version_id).cloned())
        };

        match version {
            Some(v) if v.backup_path.exists() => match fs::copy(&v.backup_path, &v.original_path) {
                Ok(_) => RollbackResult::Success {
                    version_id: v.version_id,
                    restored_path: v.original_path,
                },
                Err(e) => RollbackResult::Failed {
                    capability_id: capability_id.to_string(),
                    error: format!("Failed to restore: {}", e),
                },
            },
            Some(_) => RollbackResult::Failed {
                capability_id: capability_id.to_string(),
                error: format!("Backup for version {} not found", version_id),
            },
            None => RollbackResult::NoPreviousVersion {
                capability_id: capability_id.to_string(),
            },
        }
    }

    /// Get version history for a capability
    pub fn get_history(&self, capability_id: &str) -> Vec<CapabilityVersion> {
        self.versions
            .read()
            .ok()
            .and_then(|v| v.get(capability_id).cloned())
            .map(|d| d.into_iter().collect())
            .unwrap_or_default()
    }

    /// Get latest version for a capability
    pub fn get_latest(&self, capability_id: &str) -> Option<CapabilityVersion> {
        self.versions
            .read()
            .ok()
            .and_then(|v| v.get(capability_id)?.front().cloned())
    }

    /// Check if a capability has versions
    pub fn has_versions(&self, capability_id: &str) -> bool {
        self.versions
            .read()
            .ok()
            .map(|v| v.contains_key(capability_id))
            .unwrap_or(false)
    }

    /// Get count of versions for a capability
    pub fn version_count(&self, capability_id: &str) -> usize {
        self.versions
            .read()
            .ok()
            .and_then(|v| v.get(capability_id).map(|h| h.len()))
            .unwrap_or(0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::env::temp_dir;

    fn test_store() -> CapabilityVersionStore {
        let dir = temp_dir().join(format!("cap_versions_test_{}", std::process::id()));
        CapabilityVersionStore::new(dir).unwrap()
    }

    #[test]
    fn test_create_version() {
        let store = test_store();
        let test_file = temp_dir().join("test_cap.rtfs");
        fs::write(&test_file, "(capability test :implementation (fn [] nil))").unwrap();

        let version = store
            .create_version("test/cap", &test_file, "initial synthesis")
            .unwrap();

        assert!(version.backup_path.exists());
        assert_eq!(version.capability_id, "test/cap");
        assert_eq!(version.reason, "initial synthesis");

        // Cleanup
        let _ = fs::remove_file(&test_file);
        let _ = fs::remove_dir_all(&store.storage_dir);
    }

    #[test]
    fn test_rollback() {
        use std::time::Instant;
        let unique_id = format!("{:?}", Instant::now()).replace(['.', ':'], "_");
        let dir = temp_dir().join(format!("cap_versions_rollback_{}", unique_id));
        let store = CapabilityVersionStore::new(dir.clone()).unwrap();
        let test_file = dir.join("test_rollback.rtfs");

        // Create v1
        fs::write(&test_file, "version 1").unwrap();
        let v1 = store
            .create_version("rollback/test", &test_file, "v1")
            .unwrap();

        // Create v2 (overwrites file)
        fs::write(&test_file, "version 2").unwrap();
        let _v2 = store
            .create_version("rollback/test", &test_file, "v2")
            .unwrap();

        // Verify current file is v2
        assert_eq!(fs::read_to_string(&test_file).unwrap(), "version 2");

        // Verify v1 backup has correct content
        assert_eq!(fs::read_to_string(&v1.backup_path).unwrap(), "version 1");

        // Test rollback using specific version ID
        let result = store.rollback_to_version("rollback/test", &v1.version_id);
        assert!(
            matches!(result, RollbackResult::Success { .. }),
            "rollback failed: {:?}",
            result
        );

        // Verify v1 content restored
        assert_eq!(fs::read_to_string(&test_file).unwrap(), "version 1");

        // Cleanup
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn test_version_pruning() {
        let dir = temp_dir().join(format!("cap_versions_prune_{}", std::process::id()));
        let store = CapabilityVersionStore::with_max_versions(dir.clone(), 3).unwrap();
        let test_file = temp_dir().join("test_prune.rtfs");

        // Create 5 versions
        for i in 1..=5 {
            fs::write(&test_file, format!("version {}", i)).unwrap();
            store
                .create_version("prune/test", &test_file, &format!("v{}", i))
                .unwrap();
        }

        // Should only have 3 versions
        assert_eq!(store.version_count("prune/test"), 3);

        // Cleanup
        let _ = fs::remove_file(&test_file);
        let _ = fs::remove_dir_all(&dir);
    }
}
