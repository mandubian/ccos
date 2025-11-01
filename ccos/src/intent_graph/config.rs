//! Configuration types for Intent Graph

use super::super::intent_storage::StorageConfig;
use std::path::PathBuf;

/// Configuration for Intent Graph storage backend
#[derive(Debug, Clone)]
pub struct IntentGraphConfig {
    pub storage_config: StorageConfig,
}

impl Default for IntentGraphConfig {
    fn default() -> Self {
        Self {
            storage_config: StorageConfig::InMemory,
        }
    }
}

impl IntentGraphConfig {
    pub fn with_file_storage(path: PathBuf) -> Self {
        Self {
            storage_config: StorageConfig::File { path },
        }
    }

    pub fn with_file_archive_storage(base_dir: PathBuf) -> Self {
        Self {
            storage_config: StorageConfig::FileArchive { base_dir },
        }
    }

    pub fn with_in_memory_storage() -> Self {
        Self {
            storage_config: StorageConfig::InMemory,
        }
    }

    pub fn to_storage_config(&self) -> StorageConfig {
        self.storage_config.clone()
    }
}
