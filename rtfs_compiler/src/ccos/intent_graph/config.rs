//! Configuration types for Intent Graph

use super::super::intent_storage::StorageConfig;
use std::path::PathBuf;

/// Configuration for Intent Graph storage backend
#[derive(Debug, Clone)]
pub struct IntentGraphConfig {
    pub storage_path: Option<PathBuf>,
}

impl Default for IntentGraphConfig {
    fn default() -> Self {
        Self {
            storage_path: None,
        }
    }
}

impl IntentGraphConfig {
    pub fn with_file_storage(path: PathBuf) -> Self {
        Self {
            storage_path: Some(path),
        }
    }

    pub fn with_in_memory_storage() -> Self {
        Self {
            storage_path: None,
        }
    }
    
    pub fn to_storage_config(&self) -> StorageConfig {
        match &self.storage_path {
            Some(path) => StorageConfig::File { path: path.clone() },
            None => StorageConfig::InMemory,
        }
    }
}
