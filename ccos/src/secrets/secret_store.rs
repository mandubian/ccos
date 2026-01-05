//! Secret store implementation with layered resolution
//!
//! Resolution order (higher priority first):
//! 1. Local project secrets (.ccos/secrets.toml)
//! 2. Environment variables (global user scope)

use rtfs::runtime::error::{RuntimeError, RuntimeResult};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;

/// Secrets file format
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
struct SecretsFile {
    #[serde(default)]
    secrets: HashMap<String, String>,
    #[serde(default)]
    mappings: HashMap<String, String>,
}

/// Layered secret store with local file and env var fallback
pub struct SecretStore {
    /// Path to local secrets file (.ccos/secrets.toml)
    local_path: Option<PathBuf>,
    /// In-memory cache of local secrets
    local_secrets: HashMap<String, String>,
    /// Mappings from expected secret name to actual provider name (e.g., ENV var)
    mappings: HashMap<String, String>,
}

impl SecretStore {
    /// Create a new SecretStore, loading from the given project directory
    pub fn new(project_dir: Option<PathBuf>) -> RuntimeResult<Self> {
        let local_path = project_dir.map(|p| p.join(".ccos").join("secrets.toml"));
        let (local_secrets, mappings) = if let Some(ref path) = local_path {
            Self::load_from_file(path)?
        } else {
            (HashMap::new(), HashMap::new())
        };

        Ok(Self {
            local_path,
            local_secrets,
            mappings,
        })
    }

    /// Load secrets from a TOML file
    fn load_from_file(
        path: &PathBuf,
    ) -> RuntimeResult<(HashMap<String, String>, HashMap<String, String>)> {
        if !path.exists() {
            return Ok((HashMap::new(), HashMap::new()));
        }

        let content = fs::read_to_string(path)
            .map_err(|e| RuntimeError::IoError(format!("Failed to read secrets file: {}", e)))?;

        let file: SecretsFile = toml::from_str(&content)
            .map_err(|e| RuntimeError::IoError(format!("Failed to parse secrets file: {}", e)))?;

        Ok((file.secrets, file.mappings))
    }

    /// Get a secret by name. Resolution order: local file → env var
    pub fn get(&self, name: &str) -> Option<String> {
        // Apply mapping if it exists
        let name_to_lookup = self.mappings.get(name).map(|s| s.as_str()).unwrap_or(name);

        // 1. Check local secrets (bundled with plan)
        if let Some(val) = self.local_secrets.get(name_to_lookup) {
            return Some(val.clone());
        }
        // 2. Fall back to env var (global user scope)
        std::env::var(name_to_lookup).ok()
    }

    /// Check if a secret is available (without revealing value)
    pub fn has(&self, name: &str) -> bool {
        self.get(name).is_some()
    }

    /// Store a secret in the local project file
    pub fn set_local(&mut self, name: &str, value: String) -> RuntimeResult<()> {
        self.local_secrets.insert(name.to_string(), value);
        self.save()
    }

    /// Set a mapping in the local project file
    pub fn set_mapping(&mut self, expected_name: &str, actual_name: String) -> RuntimeResult<()> {
        self.mappings.insert(expected_name.to_string(), actual_name);
        self.save()
    }

    /// Save secrets to the local file
    pub fn save(&self) -> RuntimeResult<()> {
        let path = self.local_path.as_ref().ok_or_else(|| {
            RuntimeError::Generic("No local path configured for secrets".to_string())
        })?;

        // Ensure parent directory exists
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).map_err(|e| {
                RuntimeError::IoError(format!("Failed to create secrets directory: {}", e))
            })?;
        }

        let file = SecretsFile {
            secrets: self.local_secrets.clone(),
            mappings: self.mappings.clone(),
        };

        let content = toml::to_string_pretty(&file)
            .map_err(|e| RuntimeError::IoError(format!("Failed to serialize secrets: {}", e)))?;

        fs::write(path, &content)
            .map_err(|e| RuntimeError::IoError(format!("Failed to write secrets file: {}", e)))?;

        // Set restrictive permissions on Unix
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let perms = fs::Permissions::from_mode(0o600);
            fs::set_permissions(path, perms).map_err(|e| {
                RuntimeError::IoError(format!("Failed to set file permissions: {}", e))
            })?;
        }

        Ok(())
    }

    /// Remove a secret from local storage
    pub fn remove(&mut self, name: &str) -> RuntimeResult<bool> {
        let existed = self.local_secrets.remove(name).is_some();
        if existed {
            self.save()?;
        }
        Ok(existed)
    }

    /// List all known secret names (from local file, not env vars)
    pub fn list_local(&self) -> Vec<&str> {
        self.local_secrets.keys().map(|s| s.as_str()).collect()
    }

    /// List all mapped secret names
    pub fn list_mappings(&self) -> Vec<&str> {
        self.mappings.keys().map(|s| s.as_str()).collect()
    }

    /// Get mapping for a secret name
    pub fn get_mapping(&self, name: &str) -> Option<&String> {
        self.mappings.get(name)
    }

    /// Extract secrets needed for specific secret names (for plan export)
    pub fn export_secrets(&self, names: &[String]) -> HashMap<String, String> {
        let mut exported = HashMap::new();
        for name in names {
            if let Some(value) = self.get(name) {
                exported.insert(name.clone(), value);
            }
        }
        exported
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn test_env_var_fallback() {
        std::env::set_var("TEST_SECRET_XYZ", "test_value");
        let store = SecretStore::new(None).unwrap();
        assert_eq!(store.get("TEST_SECRET_XYZ"), Some("test_value".to_string()));
        std::env::remove_var("TEST_SECRET_XYZ");
    }

    #[test]
    fn test_local_overrides_env() {
        std::env::set_var("TEST_SECRET_ABC", "env_value");

        let dir = tempdir().unwrap();
        let mut store = SecretStore::new(Some(dir.path().to_path_buf())).unwrap();
        store
            .set_local("TEST_SECRET_ABC", "local_value".to_string())
            .unwrap();

        assert_eq!(
            store.get("TEST_SECRET_ABC"),
            Some("local_value".to_string())
        );

        std::env::remove_var("TEST_SECRET_ABC");
    }

    #[test]
    fn test_persistence() {
        let dir = tempdir().unwrap();
        let path = dir.path().to_path_buf();

        // Create and save
        {
            let mut store = SecretStore::new(Some(path.clone())).unwrap();
            store
                .set_local("PERSIST_SECRET", "persisted".to_string())
                .unwrap();
        }

        // Reload and verify
        {
            let store = SecretStore::new(Some(path)).unwrap();
            assert_eq!(store.get("PERSIST_SECRET"), Some("persisted".to_string()));
        }
    }

    #[test]
    fn test_mapping_functionality() {
        std::env::set_var("ACTUAL_KEY", "mapped_value");
        let dir = tempdir().unwrap();
        let mut store = SecretStore::new(Some(dir.path().to_path_buf())).unwrap();

        // Map EXPECTED_KEY to ACTUAL_KEY
        store
            .set_mapping("EXPECTED_KEY", "ACTUAL_KEY".to_string())
            .unwrap();

        // Should find ACTUAL_KEY value when asking for EXPECTED_KEY
        assert_eq!(store.get("EXPECTED_KEY"), Some("mapped_value".to_string()));

        std::env::remove_var("ACTUAL_KEY");
    }
}
