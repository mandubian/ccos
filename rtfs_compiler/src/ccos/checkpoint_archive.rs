//! Checkpoint Archive: stores serialized execution contexts for resume

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

use super::storage::{Archivable, ContentAddressableArchive, InMemoryArchive};
use std::fs;
use std::path::PathBuf;

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
    // Optional durable directory for persistence (Developer Preview)
    durable_dir: Option<PathBuf>,
}

impl CheckpointArchive {
    pub fn new() -> Self { Self { storage: InMemoryArchive::new(), id_index: Default::default(), durable_dir: None } }

    /// Enable durable persistence to a directory. Files are named by checkpoint_id with .json extension.
    pub fn with_durable_dir(mut self, dir: impl Into<PathBuf>) -> Self {
        let dir = dir.into();
        let _ = fs::create_dir_all(&dir);
        self.durable_dir = Some(dir);
        self
    }

    pub fn store(&self, record: CheckpointRecord) -> Result<String, String> {
        let id = record.checkpoint_id.clone();
        {
            let mut idx = self.id_index.lock().map_err(|_| "index lock".to_string())?;
            idx.insert(id.clone(), record.clone());
        }
        // Also store in content-addressed archive for stats/integrity
        let _ = self.storage.store(record)?;

        // Optionally persist to disk
        if let Some(dir) = &self.durable_dir {
            let mut path = dir.clone();
            path.push(format!("{}.json", id));
            let json = serde_json::to_string_pretty(&self.get_by_id(&id).ok_or_else(|| "not found after insert".to_string())?)
                .map_err(|e| format!("serde: {}", e))?;
            fs::write(&path, json).map_err(|e| format!("write: {}", e))?;
        }
        Ok(id)
    }

    pub fn get_by_id(&self, id: &str) -> Option<CheckpointRecord> {
        self.id_index.lock().ok().and_then(|m| m.get(id).cloned())
    }

    /// Attempt to load a checkpoint by id from disk if not in memory. No-op if durable_dir is None.
    pub fn load_from_disk(&self, id: &str) -> Option<CheckpointRecord> {
        let dir = self.durable_dir.as_ref()?;
        let mut path = dir.clone();
        path.push(format!("{}.json", id));
        let bytes = fs::read(&path).ok()?;
        serde_json::from_slice::<CheckpointRecord>(&bytes).ok()
    }
}


