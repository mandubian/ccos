/// Unified Storage Abstraction for CCOS
///
/// This module provides a content-addressable, immutable storage system
/// with integrity verification and consistent interfaces across all CCOS entities.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::fmt::Debug;
use sha2::{Digest, Sha256};
use serde::{Deserialize, Serialize};

/// Core trait for entities that can be archived in our unified storage system.
/// 
/// This trait enables content-addressable storage with automatic integrity verification.
/// All archived entities must be serializable to ensure thread-safety and persistence.
pub trait Archivable: Debug + Clone + Serialize + for<'de> Deserialize<'de> {
    /// Generate a content hash for this entity
    fn content_hash(&self) -> String {
        let serialized = serde_json::to_string(self).unwrap_or_default();
        let mut hasher = Sha256::new();
        hasher.update(serialized.as_bytes());
        format!("{:x}", hasher.finalize())
    }
    
    /// Unique identifier for this entity
    fn entity_id(&self) -> String;
    
    /// Human-readable description for debugging
    fn entity_type(&self) -> &'static str;
}

/// Thread-safe, content-addressable archive for immutable entities.
/// 
/// This trait provides the core storage interface used by all CCOS archives.
/// Implementations must be thread-safe and support concurrent access.
pub trait ContentAddressableArchive<T: Archivable> {
    /// Store an entity, returning its content hash
    fn store(&self, entity: T) -> Result<String, String>;
    
    /// Retrieve an entity by content hash
    fn retrieve(&self, hash: &str) -> Result<Option<T>, String>;
    
    /// Check if an entity exists by content hash
    fn exists(&self, hash: &str) -> bool;
    
    /// Delete an entity by content hash
    fn delete(&self, hash: &str) -> Result<(), String>;
    
    /// Get storage statistics
    fn stats(&self) -> ArchiveStats;
    
    /// Verify integrity of stored data
    fn verify_integrity(&self) -> Result<bool, String>;
    
    /// List all stored hashes
    fn list_hashes(&self) -> Vec<String>;
}

/// Statistics about archive storage usage
#[derive(Debug, Clone)]
pub struct ArchiveStats {
    pub total_entities: usize,
    pub total_size_bytes: usize,
    pub oldest_timestamp: Option<u64>,
    pub newest_timestamp: Option<u64>,
}

/// Thread-safe in-memory implementation of content-addressable archive.
/// 
/// Uses Arc<Mutex<...>> for thread safety. Suitable for testing and 
/// single-process scenarios.
#[derive(Debug)]
pub struct InMemoryArchive<T: Archivable> {
    storage: Arc<Mutex<HashMap<String, T>>>,
    metadata: Arc<Mutex<HashMap<String, EntityMetadata>>>,
}

#[derive(Debug, Clone)]
struct EntityMetadata {
    stored_at: u64,
    size_bytes: usize,
}

impl<T: Archivable> Default for InMemoryArchive<T> {
    fn default() -> Self {
        Self::new()
    }
}

impl<T: Archivable> InMemoryArchive<T> {
    pub fn new() -> Self {
        Self {
            storage: Arc::new(Mutex::new(HashMap::new())),
            metadata: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    /// Get the approximate size in bytes of stored data
    pub fn size_bytes(&self) -> usize {
        let metadata = self.metadata.lock().unwrap();
        metadata.values().map(|meta| meta.size_bytes).sum()
    }
}

impl<T: Archivable> ContentAddressableArchive<T> for InMemoryArchive<T> {
    fn store(&self, entity: T) -> Result<String, String> {
        let hash = entity.content_hash();
        
        // Calculate size for statistics
        let size_bytes = serde_json::to_string(&entity)
            .map_err(|e| format!("Serialization error: {}", e))?
            .len();
        
        let metadata = EntityMetadata {
            stored_at: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs(),
            size_bytes,
        };
        
        // Store with thread safety
        {
            let mut storage = self.storage.lock()
                .map_err(|_| "Storage lock poisoned".to_string())?;
            storage.insert(hash.clone(), entity);
        }
        
        {
            let mut meta = self.metadata.lock()
                .map_err(|_| "Metadata lock poisoned".to_string())?;
            meta.insert(hash.clone(), metadata);
        }
        
        Ok(hash)
    }
    
    fn retrieve(&self, hash: &str) -> Result<Option<T>, String> {
        let storage = self.storage.lock()
            .map_err(|_| "Storage lock poisoned".to_string())?;
        Ok(storage.get(hash).cloned())
    }
    
    fn exists(&self, hash: &str) -> bool {
        self.storage.lock()
            .map(|storage| storage.contains_key(hash))
            .unwrap_or(false)
    }
    
    fn delete(&self, hash: &str) -> Result<(), String> {
        // Remove from storage
        {
            let mut storage = self.storage.lock()
                .map_err(|_| "Storage lock poisoned".to_string())?;
            storage.remove(hash);
        }
        
        // Remove from metadata
        {
            let mut metadata = self.metadata.lock()
                .map_err(|_| "Metadata lock poisoned".to_string())?;
            metadata.remove(hash);
        }
        
        Ok(())
    }
    
    fn stats(&self) -> ArchiveStats {
        let storage = self.storage.lock().unwrap_or_else(|_| panic!("Storage lock poisoned"));
        let metadata = self.metadata.lock().unwrap_or_else(|_| panic!("Metadata lock poisoned"));
        
        let total_entities = storage.len();
        let total_size_bytes = metadata.values().map(|m| m.size_bytes).sum();
        
        let timestamps: Vec<u64> = metadata.values().map(|m| m.stored_at).collect();
        
        ArchiveStats {
            total_entities,
            total_size_bytes,
            oldest_timestamp: timestamps.iter().min().copied(),
            newest_timestamp: timestamps.iter().max().copied(),
        }
    }
    
    fn verify_integrity(&self) -> Result<bool, String> {
        let storage = self.storage.lock()
            .map_err(|_| "Storage lock poisoned".to_string())?;
        
        for (hash, entity) in storage.iter() {
            let computed_hash = entity.content_hash();
            if computed_hash != *hash {
                return Ok(false);
            }
        }
        
        Ok(true)
    }
    
    fn list_hashes(&self) -> Vec<String> {
        self.storage.lock()
            .map(|storage| storage.keys().cloned().collect())
            .unwrap_or_default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ccos::storage_backends::file_archive::FileArchive;
    use tempfile::tempdir;
    
    #[derive(Debug, Clone, Serialize, Deserialize)]
    struct TestEntity {
        id: String,
        name: String,
        value: i32,
    }
    
    impl Archivable for TestEntity {
        fn entity_id(&self) -> String {
            self.id.clone()
        }
        
        fn entity_type(&self) -> &'static str {
            "TestEntity"
        }
    }
    
    #[test]
    fn test_basic_storage() {
        let archive = InMemoryArchive::<TestEntity>::new();
        
        let entity = TestEntity {
            id: "test-1".to_string(),
            name: "Test Entity".to_string(),
            value: 42,
        };
        
        // Store entity
        let hash = archive.store(entity.clone()).expect("Failed to store entity");
        assert!(!hash.is_empty());
        
        // Retrieve entity
        let retrieved = archive.retrieve(&hash).expect("Failed to retrieve entity");
        assert!(retrieved.is_some());
        let retrieved = retrieved.unwrap();
        
        assert_eq!(retrieved.id, entity.id);
        assert_eq!(retrieved.name, entity.name);
        assert_eq!(retrieved.value, entity.value);
    }
    
    #[test]
    fn test_content_addressing() {
        let archive = InMemoryArchive::<TestEntity>::new();
        
        let entity1 = TestEntity {
            id: "test-1".to_string(),
            name: "Test Entity".to_string(),
            value: 42,
        };
        
        let entity2 = TestEntity {
            id: "test-1".to_string(),
            name: "Test Entity".to_string(),
            value: 42,
        };
        
        // Same content should produce same hash
        let hash1 = archive.store(entity1).expect("Failed to store entity1");
        let hash2 = archive.store(entity2).expect("Failed to store entity2");
        
        assert_eq!(hash1, hash2);
        
        // Only one entity should be stored (deduplicated by content)
        let stats = archive.stats();
        assert_eq!(stats.total_entities, 1);
    }
    
    #[test]
    fn test_integrity_verification() {
        let archive = InMemoryArchive::<TestEntity>::new();
        
        let entity = TestEntity {
            id: "test-1".to_string(),
            name: "Test Entity".to_string(),
            value: 42,
        };
        
        archive.store(entity).expect("Failed to store entity");
        
        let integrity_ok = archive.verify_integrity().expect("Failed to verify integrity");
        assert!(integrity_ok);
    }
    
    #[test]
    fn test_statistics() {
        let archive = InMemoryArchive::<TestEntity>::new();
        
        let entity1 = TestEntity {
            id: "test-1".to_string(),
            name: "Test Entity 1".to_string(),
            value: 42,
        };
        
        let entity2 = TestEntity {
            id: "test-2".to_string(),
            name: "Test Entity 2".to_string(),
            value: 100,
        };
        
        archive.store(entity1).expect("Failed to store entity1");
        archive.store(entity2).expect("Failed to store entity2");
        
        let stats = archive.stats();
        assert_eq!(stats.total_entities, 2);
        assert!(stats.total_size_bytes > 0);
        assert!(stats.oldest_timestamp.is_some());
        assert!(stats.newest_timestamp.is_some());
    }

    #[test]
    fn test_hash_stability_across_backends() {
        // Same entity should produce the same content hash in memory and on disk
        let entity = TestEntity {
            id: "x".to_string(),
            name: "Stable".to_string(),
            value: 7,
        };

        // In-memory
        let mem = InMemoryArchive::<TestEntity>::new();
        let h1 = mem.store(entity.clone()).expect("mem store");

        // File-backed
        let dir = tempdir().unwrap();
        let file = FileArchive::new(dir.path()).expect("file archive");
        let h2 = <FileArchive as ContentAddressableArchive<TestEntity>>::store(&file, entity)
            .expect("file store");

        assert_eq!(h1, h2);
    }
}
