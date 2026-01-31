use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use base64::Engine as _;
use chrono::{DateTime, Duration, Utc};
use chacha20poly1305::aead::{Aead, KeyInit};
use chacha20poly1305::{XChaCha20Poly1305, XNonce};
use rand::rngs::OsRng;
use rand::RngCore;
use rtfs::runtime::error::{RuntimeError, RuntimeResult};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

pub trait QuarantineStore: Send + Sync {
    fn put_bytes(&self, bytes: Vec<u8>, ttl: Duration) -> RuntimeResult<String>;
    fn get_bytes(&self, pointer_id: &str) -> RuntimeResult<Vec<u8>>;
    fn purge_expired(&self) -> RuntimeResult<usize>;
    fn purge_all(&self) -> RuntimeResult<usize>;
}

/// Minimal in-memory quarantine store (Phase 0/1).
#[derive(Debug)]
pub struct InMemoryQuarantineStore {
    entries: Mutex<HashMap<String, QuarantineEntry>>,
}

#[derive(Debug, Clone)]
struct QuarantineEntry {
    expires_at: DateTime<Utc>,
    bytes: Vec<u8>,
}

impl InMemoryQuarantineStore {
    pub fn new() -> Self {
        Self {
            entries: Mutex::new(HashMap::new()),
        }
    }
}

impl QuarantineStore for InMemoryQuarantineStore {
    fn put_bytes(&self, bytes: Vec<u8>, ttl: Duration) -> RuntimeResult<String> {
        let id = Uuid::new_v4().to_string();
        let now = Utc::now();
        let entry = QuarantineEntry {
            expires_at: now + ttl,
            bytes,
        };
        let mut map = self
            .entries
            .lock()
            .map_err(|_| RuntimeError::Generic("Failed to lock quarantine store".to_string()))?;
        map.insert(id.clone(), entry);
        Ok(id)
    }

    fn get_bytes(&self, pointer_id: &str) -> RuntimeResult<Vec<u8>> {
        let mut map = self
            .entries
            .lock()
            .map_err(|_| RuntimeError::Generic("Failed to lock quarantine store".to_string()))?;
        let Some(entry) = map.get(pointer_id).cloned() else {
            return Err(RuntimeError::Generic(format!(
                "Quarantine pointer not found: {}",
                pointer_id
            )));
        };
        if Utc::now() > entry.expires_at {
            map.remove(pointer_id);
            return Err(RuntimeError::Generic(format!(
                "Quarantine pointer expired: {}",
                pointer_id
            )));
        }
        Ok(entry.bytes)
    }

    fn purge_expired(&self) -> RuntimeResult<usize> {
        let now = Utc::now();
        let mut map = self
            .entries
            .lock()
            .map_err(|_| RuntimeError::Generic("Failed to lock quarantine store".to_string()))?;
        let before = map.len();
        map.retain(|_, entry| entry.expires_at > now);
        Ok(before.saturating_sub(map.len()))
    }

    fn purge_all(&self) -> RuntimeResult<usize> {
        let mut map = self
            .entries
            .lock()
            .map_err(|_| RuntimeError::Generic("Failed to lock quarantine store".to_string()))?;
        let count = map.len();
        map.clear();
        Ok(count)
    }
}

#[derive(Debug, Clone)]
pub struct QuarantineKey([u8; 32]);

impl QuarantineKey {
    pub fn from_base64(key_b64: &str) -> RuntimeResult<Self> {
        let raw = base64::engine::general_purpose::STANDARD
            .decode(key_b64.as_bytes())
            .map_err(|_| RuntimeError::Generic("Invalid base64 quarantine key".to_string()))?;
        let bytes: [u8; 32] = raw
            .as_slice()
            .try_into()
            .map_err(|_| RuntimeError::Generic("Quarantine key must be 32 bytes".to_string()))?;
        Ok(Self(bytes))
    }

    pub fn from_env(env_var: &str) -> RuntimeResult<Self> {
        let key = std::env::var(env_var).map_err(|_| {
            RuntimeError::Generic(format!("Missing quarantine key env var: {}", env_var))
        })?;
        Self::from_base64(&key)
    }

    fn as_bytes(&self) -> &[u8; 32] {
        &self.0
    }
}

#[derive(Debug, Serialize, Deserialize)]
struct StoredBlob {
    created_at: DateTime<Utc>,
    expires_at: DateTime<Utc>,
    nonce_b64: String,
    ciphertext_b64: String,
    version: u8,
}

/// Persistent quarantine store with encryption-at-rest and TTL enforcement.
pub struct FileQuarantineStore {
    root_dir: PathBuf,
    cipher: XChaCha20Poly1305,
    default_ttl: Duration,
}

impl FileQuarantineStore {
    pub fn new(root_dir: PathBuf, key: QuarantineKey, default_ttl: Duration) -> RuntimeResult<Self> {
        fs::create_dir_all(&root_dir).map_err(|e| {
            RuntimeError::Generic(format!("Failed to create quarantine dir: {}", e))
        })?;
        let cipher = XChaCha20Poly1305::new(key.as_bytes().into());
        Ok(Self {
            root_dir,
            cipher,
            default_ttl,
        })
    }

    pub fn purge_expired_in_dir(root_dir: &Path) -> RuntimeResult<usize> {
        let mut removed = 0usize;
        let now = Utc::now();
        let entries = fs::read_dir(root_dir).map_err(|e| {
            RuntimeError::Generic(format!("Failed to read quarantine dir: {}", e))
        })?;
        for entry in entries {
            let entry = entry.map_err(|e| {
                RuntimeError::Generic(format!("Failed to read quarantine dir entry: {}", e))
            })?;
            let path = entry.path();
            if !path.is_file() {
                continue;
            }
            let data = fs::read_to_string(&path).map_err(|e| {
                RuntimeError::Generic(format!("Failed to read quarantine blob: {}", e))
            })?;
            let stored: StoredBlob = serde_json::from_str(&data).map_err(|e| {
                RuntimeError::Generic(format!("Failed to parse quarantine blob: {}", e))
            })?;
            if now > stored.expires_at {
                fs::remove_file(&path).map_err(|e| {
                    RuntimeError::Generic(format!("Failed to remove quarantine blob: {}", e))
                })?;
                removed += 1;
            }
        }
        Ok(removed)
    }

    pub fn purge_all_in_dir(root_dir: &Path) -> RuntimeResult<usize> {
        let mut removed = 0usize;
        let entries = fs::read_dir(root_dir).map_err(|e| {
            RuntimeError::Generic(format!("Failed to read quarantine dir: {}", e))
        })?;
        for entry in entries {
            let entry = entry.map_err(|e| {
                RuntimeError::Generic(format!("Failed to read quarantine dir entry: {}", e))
            })?;
            let path = entry.path();
            if path.is_file() {
                fs::remove_file(&path).map_err(|e| {
                    RuntimeError::Generic(format!("Failed to remove quarantine blob: {}", e))
                })?;
                removed += 1;
            }
        }
        Ok(removed)
    }

    fn entry_path(&self, pointer_id: &str) -> PathBuf {
        self.root_dir.join(format!("{}.json", pointer_id))
    }

    fn encrypt_bytes(&self, bytes: &[u8]) -> RuntimeResult<(String, String)> {
        let mut nonce = [0u8; 24];
        OsRng.fill_bytes(&mut nonce);
        let nonce = XNonce::from(nonce);
        let cipher_text = self
            .cipher
            .encrypt(&nonce, bytes)
            .map_err(|_| RuntimeError::Generic("Failed to encrypt quarantine blob".to_string()))?;
        let nonce_b64 = base64::engine::general_purpose::STANDARD.encode(nonce);
        let ciphertext_b64 = base64::engine::general_purpose::STANDARD.encode(cipher_text);
        Ok((nonce_b64, ciphertext_b64))
    }

    fn decrypt_bytes(&self, nonce_b64: &str, ciphertext_b64: &str) -> RuntimeResult<Vec<u8>> {
        let nonce = base64::engine::general_purpose::STANDARD
            .decode(nonce_b64.as_bytes())
            .map_err(|_| RuntimeError::Generic("Invalid quarantine nonce".to_string()))?;
        let ciphertext = base64::engine::general_purpose::STANDARD
            .decode(ciphertext_b64.as_bytes())
            .map_err(|_| RuntimeError::Generic("Invalid quarantine ciphertext".to_string()))?;
        let nonce_arr: [u8; 24] = nonce
            .as_slice()
            .try_into()
            .map_err(|_| RuntimeError::Generic("Invalid quarantine nonce length".to_string()))?;
        let nonce = XNonce::from(nonce_arr);
        let plaintext = self
            .cipher
            .decrypt(&nonce, ciphertext.as_ref())
            .map_err(|_| RuntimeError::Generic("Failed to decrypt quarantine blob".to_string()))?;
        Ok(plaintext)
    }

    fn read_entry(&self, path: &Path) -> RuntimeResult<StoredBlob> {
        let data = fs::read_to_string(path)
            .map_err(|e| RuntimeError::Generic(format!("Failed to read quarantine blob: {}", e)))?;
        serde_json::from_str(&data)
            .map_err(|e| RuntimeError::Generic(format!("Failed to parse quarantine blob: {}", e)))
    }

    fn write_entry(&self, path: &Path, entry: &StoredBlob) -> RuntimeResult<()> {
        let data = serde_json::to_string(entry)
            .map_err(|e| RuntimeError::Generic(format!("Failed to encode quarantine blob: {}", e)))?;
        fs::write(path, data)
            .map_err(|e| RuntimeError::Generic(format!("Failed to write quarantine blob: {}", e)))
    }
}

impl QuarantineStore for FileQuarantineStore {
    fn put_bytes(&self, bytes: Vec<u8>, ttl: Duration) -> RuntimeResult<String> {
        let id = Uuid::new_v4().to_string();
        let now = Utc::now();
        let effective_ttl = if ttl.num_milliseconds() > 0 { ttl } else { self.default_ttl };
        let expires_at = now + effective_ttl;
        let (nonce_b64, ciphertext_b64) = self.encrypt_bytes(&bytes)?;
        let entry = StoredBlob {
            created_at: now,
            expires_at,
            nonce_b64,
            ciphertext_b64,
            version: 1,
        };
        let path = self.entry_path(&id);
        self.write_entry(&path, &entry)?;
        Ok(id)
    }

    fn get_bytes(&self, pointer_id: &str) -> RuntimeResult<Vec<u8>> {
        let path = self.entry_path(pointer_id);
        if !path.exists() {
            return Err(RuntimeError::Generic(format!(
                "Quarantine pointer not found: {}",
                pointer_id
            )));
        }
        let entry = self.read_entry(&path)?;
        if Utc::now() > entry.expires_at {
            fs::remove_file(&path).map_err(|e| {
                RuntimeError::Generic(format!("Failed to remove expired blob: {}", e))
            })?;
            return Err(RuntimeError::Generic(format!(
                "Quarantine pointer expired: {}",
                pointer_id
            )));
        }
        self.decrypt_bytes(&entry.nonce_b64, &entry.ciphertext_b64)
    }

    fn purge_expired(&self) -> RuntimeResult<usize> {
        Self::purge_expired_in_dir(&self.root_dir)
    }

    fn purge_all(&self) -> RuntimeResult<usize> {
        Self::purge_all_in_dir(&self.root_dir)
    }
}
