use super::super::types::Action;
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use uuid::Uuid;

/// Cryptographic signing for actions
#[derive(Debug)]
pub struct CryptographicSigning {
    // In a full implementation, this would use proper PKI
    pub signing_key: String,
    pub verification_keys: HashMap<String, String>,
}

impl CryptographicSigning {
    pub fn new() -> Self {
        Self {
            signing_key: format!("key-{}", Uuid::new_v4()),
            verification_keys: HashMap::new(),
        }
    }

    pub fn sign_action(&self, action: &Action) -> String {
        let mut hasher = Sha256::new();
        hasher.update(self.signing_key.as_bytes());
        hasher.update(action.action_id.as_bytes());
        hasher.update(action.timestamp.to_string().as_bytes());
        format!("{:x}", hasher.finalize())
    }

    pub fn verify_signature(&self, action: &Action, signature: &str) -> bool {
        let expected_signature = self.sign_action(action);
        signature == expected_signature
    }

    pub fn add_verification_key(&mut self, key_id: String, public_key: String) {
        self.verification_keys.insert(key_id, public_key);
    }
}
