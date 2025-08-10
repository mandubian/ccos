//! Checkpoint Archive: stores serialized execution contexts for resume

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

use super::storage::{Archivable, ContentAddressableArchive, InMemoryArchive};

/// Persisted checkpoint record
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CheckpointRecord {
    pub checkpoint_id: String,
    pub plan_id: String,
    pub intent_id: String,
    pub serialized_context: String,
    pub created_at: u64,
    pub metadata: HashMap<String, String>,
}

impl Archivable for CheckpointRecord {
    fn entity_id(&self) -> String { self.checkpoint_id.clone() }
    fn entity_type(&self) -> &'static str { "CheckpointRecord" }
}

/// In-memory checkpoint archive
#[derive(Debug, Default)]
pub struct CheckpointArchive {
    storage: InMemoryArchive<CheckpointRecord>,
    // Direct index by human-facing checkpoint_id (e.g., "cp-<hash>")
    id_index: std::sync::Arc<std::sync::Mutex<std::collections::HashMap<String, CheckpointRecord>>>,
}

impl CheckpointArchive {
    pub fn new() -> Self { Self { storage: InMemoryArchive::new(), id_index: Default::default() } }

    pub fn store(&self, record: CheckpointRecord) -> Result<String, String> {
        let id = record.checkpoint_id.clone();
        {
            let mut idx = self.id_index.lock().map_err(|_| "index lock".to_string())?;
            idx.insert(id.clone(), record.clone());
        }
        // Also store in content-addressed archive for stats/integrity
        let _ = self.storage.store(record)?;
        Ok(id)
    }

    pub fn get_by_id(&self, id: &str) -> Option<CheckpointRecord> {
        self.id_index.lock().ok().and_then(|m| m.get(id).cloned())
    }
}


