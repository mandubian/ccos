//! SKILL.md Cryptographic Verification.
//!
//! Uses Ed25519 to sign and verify Agent Manifests to prevent tampering
//! and Knowledge Poisoning.

use ed25519_dalek::{Signature, Signer, SigningKey, Verifier, VerifyingKey};

pub struct ManifestSigner {
    key: SigningKey,
}

impl ManifestSigner {
    /// Creates a signer from a 32-byte secret key.
    pub fn new(secret_bytes: &[u8; 32]) -> Self {
        Self {
            key: SigningKey::from_bytes(secret_bytes),
        }
    }

    /// Generates a base64 encoded Ed25519 signature for the given content.
    pub fn sign(&self, content: &str) -> String {
        let signature = self.key.sign(content.as_bytes());
        use base64::{engine::general_purpose::STANDARD, Engine as _};
        STANDARD.encode(signature.to_bytes())
    }
}

pub struct ManifestVerifier;

impl ManifestVerifier {
    /// Verifies that the base64 signature matches the content and public key bytes.
    pub fn verify(
        public_bytes: &[u8; 32],
        content: &str,
        signature_b64: &str,
    ) -> anyhow::Result<bool> {
        let vk = VerifyingKey::from_bytes(public_bytes)
            .map_err(|e| anyhow::anyhow!("Invalid public key: {}", e))?;

        use base64::{engine::general_purpose::STANDARD, Engine as _};
        let sig_bytes = STANDARD.decode(signature_b64)?;
        if sig_bytes.len() != 64 {
            return Ok(false);
        }

        let mut sig_arr = [0u8; 64];
        sig_arr.copy_from_slice(&sig_bytes);
        let signature = Signature::from_bytes(&sig_arr);

        Ok(vk.verify(content.as_bytes(), &signature).is_ok())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_sign_and_verify() {
        let secret = [1u8; 32];
        let signer = ManifestSigner::new(&secret);

        let content = "The quick brown fox jumps over the lazy agent.";
        let sig_b64 = signer.sign(content);

        // Derive public key for verification
        let public_bytes = signer.key.verifying_key().to_bytes();

        assert!(ManifestVerifier::verify(&public_bytes, content, &sig_b64).unwrap());

        // Test tampering
        let tampered_content = "The quick brown fox jumps over the evil agent.";
        assert!(!ManifestVerifier::verify(&public_bytes, tampered_content, &sig_b64).unwrap());
    }
}
