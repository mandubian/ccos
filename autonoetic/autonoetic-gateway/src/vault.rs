//! Vault for secure credential injection.

use secrecy::ExposeSecret;
use secrecy::SecretString;
use std::collections::HashMap;
use std::path::Path;

/// Vault manages secrets for agents and injects them into tools safely.
pub struct Vault {
    secrets: HashMap<String, SecretString>,
}

impl Vault {
    pub fn new() -> Self {
        Self {
            secrets: HashMap::new(),
        }
    }

    /// Load a secret from the environment or a secure keystore.
    pub fn load_secret(&mut self, key: &str, value: String) {
        self.secrets
            .insert(key.to_string(), SecretString::from(value));
    }

    /// Alias for explicit runtime secret writes.
    pub fn set_secret(&mut self, key: &str, value: String) {
        self.load_secret(key, value);
    }

    /// Retrieve a secret for secure injection (e.g., as an env var to a sandbox).
    ///
    /// The secret is wrapped in `SecretString` to prevent accidental logging.
    /// It must be explicitly exposed with `.expose_secret()` at the boundary.
    pub fn get_secret(&self, key: &str) -> Option<&SecretString> {
        self.secrets.get(key)
    }

    /// Clear all secrets from memory.
    pub fn clear(&mut self) {
        self.secrets.clear();
    }

    /// Load a vault snapshot from JSON on disk.
    pub fn load_from_file(path: &Path) -> anyhow::Result<Self> {
        if !path.exists() {
            return Ok(Self::new());
        }
        let raw = std::fs::read_to_string(path)?;
        let plain: HashMap<String, String> = serde_json::from_str(&raw)?;
        let mut vault = Self::new();
        for (k, v) in plain {
            vault.set_secret(&k, v);
        }
        Ok(vault)
    }

    /// Persist current vault state to JSON on disk.
    pub fn persist_to_file(&self, path: &Path) -> anyhow::Result<()> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let plain: HashMap<String, String> = self
            .secrets
            .iter()
            .map(|(k, v)| (k.clone(), v.expose_secret().to_string()))
            .collect();
        std::fs::write(path, serde_json::to_string_pretty(&plain)?)?;
        Ok(())
    }
}

impl Default for Vault {
    fn default() -> Self {
        Self::new()
    }
}
