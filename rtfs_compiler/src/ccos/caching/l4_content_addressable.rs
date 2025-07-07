//! L4 Content-Addressable RTFS: Bytecode-level caching and reuse
//! 
//! This layer caches compiled RTFS bytecode and intermediate representations
//! for reuse across different execution contexts.

use std::time::SystemTime;
use uuid::Uuid;
use lazy_static::lazy_static;
use std::sync::{Arc, RwLock};
use std::collections::HashMap;
use sha2::{Digest, Sha256};

lazy_static! {
    /// Global in-process L4 cache client (prototype).
    pub static ref GLOBAL_L4_CLIENT: L4CacheClient = L4CacheClient::new();
}

/// Metadata stored alongside each RTFS bytecode module.
/// Mirrors the database schema described in the L4 cache specification.
#[derive(Debug, Clone)]
pub struct RtfsModuleMetadata {
    pub id: Uuid,
    /// Vector embedding representing the task semantics.
    pub semantic_embedding: Vec<f32>,
    /// Stable hash of the function signature / interface.
    pub interface_hash: String,
    /// Pointer to the blob in storage (e.g. S3 key).
    pub storage_pointer: String,
    /// Validation status (e.g. "Verified", "Pending").
    pub validation_status: String,
    /// Cryptographic signature for integrity verification.
    pub signature: String,
    pub creation_timestamp: SystemTime,
    pub last_used_timestamp: SystemTime,
}

impl RtfsModuleMetadata {
    pub fn new(semantic_embedding: Vec<f32>, interface_hash: String, storage_pointer: String) -> Self {
        let now = SystemTime::now();
        Self {
            id: Uuid::new_v4(),
            semantic_embedding,
            interface_hash,
            storage_pointer,
            validation_status: "Pending".to_string(),
            signature: String::new(),
            creation_timestamp: now,
            last_used_timestamp: now,
        }
    }
}

/// Simple stub client – in a real implementation this would talk to S3 / Postgres.
#[derive(Debug, Default)]
pub struct L4CacheClient {
    // In-memory store for prototyping
    index: Arc<RwLock<Vec<RtfsModuleMetadata>>>, // simplistic – no pagination
    /// In-memory blob store keyed by storage pointer. Prototype only – replace with S3 or other storage in production.
    blob_store: Arc<RwLock<HashMap<String, Vec<u8>>>>,
}

impl Clone for L4CacheClient {
    fn clone(&self) -> Self {
        Self {
            index: Arc::clone(&self.index),
            blob_store: Arc::clone(&self.blob_store),
        }
    }
}

impl L4CacheClient {
    pub fn new() -> Self {
        Self {
            index: Arc::new(RwLock::new(Vec::new())),
            blob_store: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    /// Store an arbitrary blob and return its content-address (SHA-256 hex).
    /// If the blob already exists, this is a no-op and returns the existing hash.
    pub fn store_blob(&self, bytes: Vec<u8>) -> Result<String, String> {
        let mut hasher = Sha256::new();
        hasher.update(&bytes);
        let hash = format!("{:x}", hasher.finalize());

        let mut store = self.blob_store.write().map_err(|e| e.to_string())?;
        store.entry(hash.clone()).or_insert(bytes);
        Ok(hash)
    }

    /// Publish a compiled RTFS module together with its bytecode.
    ///
    /// Returns the storage pointer where the blob was stored (for now an in-memory key).
    pub fn publish_module(
        &self,
        bytecode: Vec<u8>,
        mut metadata: RtfsModuleMetadata,
    ) -> Result<String, String> {
        // Store blob first to get its content hash
        let object_key = self.store_blob(bytecode)?;

        metadata.storage_pointer = object_key.clone();
        metadata.validation_status = "Verified".to_string();

        // Store metadata
        self.index.write().unwrap().push(metadata);
        Ok(object_key)
    }

    /// Retrieve a previously published bytecode blob.
    pub fn get_blob(&self, storage_pointer: &str) -> Option<Vec<u8>> {
        self.blob_store.read().ok().and_then(|s| s.get(storage_pointer).cloned())
    }

    /// Query for an existing module by interface hash and optional semantic embedding.
    /// This is a naive linear search; production code would use a vector DB / ANN index.
    pub fn query(
        &self,
        interface_hash: &str,
        semantic_embedding: Option<&[f32]>,
        similarity_threshold: f32,
    ) -> Option<RtfsModuleMetadata> {
        let index = self.index.read().unwrap();
        let mut best: Option<(f32, &RtfsModuleMetadata)> = None;
        for meta in index.iter() {
            if meta.interface_hash != interface_hash {
                continue;
            }
            if let Some(query_emb) = semantic_embedding {
                let sim = cosine_similarity(query_emb, &meta.semantic_embedding);
                if sim >= similarity_threshold {
                    if let Some((best_sim, _)) = best {
                        if sim > best_sim {
                            best = Some((sim, meta));
                        }
                    } else {
                        best = Some((sim, meta));
                    }
                }
            } else {
                // No semantic check – first exact interface match wins
                return Some(meta.clone());
            }
        }
        best.map(|(_, m)| m.clone())
    }
}

/// Compute cosine similarity between two equal-length vectors
fn cosine_similarity(a: &[f32], b: &[f32]) -> f32 {
    if a.len() != b.len() || a.is_empty() {
        return 0.0;
    }
    let dot: f32 = a.iter().zip(b).map(|(x, y)| x * y).sum();
    let norm_a: f32 = a.iter().map(|x| x * x).sum::<f32>().sqrt();
    let norm_b: f32 = b.iter().map(|x| x * x).sum::<f32>().sqrt();
    if norm_a == 0.0 || norm_b == 0.0 {
        0.0
    } else {
        dot / (norm_a * norm_b)
    }
}

// Existing minimal cache struct kept for compatibility; now wraps the client.
#[derive(Debug, Default)]
pub struct L4ContentAddressableCache {
    client: L4CacheClient,
}

impl L4ContentAddressableCache {
    pub fn new() -> Self {
        Self { client: L4CacheClient::new() }
    }

    pub fn client(&self) -> &L4CacheClient {
        &self.client
    }
} 