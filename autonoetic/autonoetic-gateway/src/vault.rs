//! Vault for secure credential injection.

use secrecy::SecretString;
use std::collections::HashMap;

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
}

impl Default for Vault {
    fn default() -> Self {
        Self::new()
    }
}
