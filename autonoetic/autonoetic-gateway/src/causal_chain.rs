//! Hash-chain Causal Logger.

use autonoetic_types::causal_chain::{CausalChainEntry, EntryStatus};
use std::io::Write;
use std::path::PathBuf;
use std::sync::Mutex;

pub struct CausalLogger {
    log_path: PathBuf,
    last_hash: Mutex<String>,
}

impl CausalLogger {
    pub fn new(log_path: impl Into<PathBuf>) -> Self {
        Self {
            log_path: log_path.into(),
            last_hash: Mutex::new("genesis".to_string()),
        }
    }

    /// Append a new action to the Causal Chain.
    pub fn log(
        &self,
        actor_id: &str,
        category: &str,
        action: &str,
        status: EntryStatus,
        payload: Option<serde_json::Value>,
    ) -> anyhow::Result<()> {
        let mut last_hash_guard = self.last_hash.lock().unwrap();
        let prev_hash = last_hash_guard.clone();

        let entry = CausalChainEntry {
            timestamp: chrono::Utc::now().to_rfc3339(),
            log_id: uuid::Uuid::new_v4().to_string(),
            actor_id: actor_id.to_string(),
            category: category.to_string(),
            action: action.to_string(),
            target: None,
            status,
            reason: None,
            payload,
            prev_hash: prev_hash.clone(),
        };

        let entry_json = serde_json::to_string(&entry)?;

        // Compute new hash (naive stub — real impl uses sha2)
        use std::hash::{Hash, Hasher};
        let mut hasher = std::collections::hash_map::DefaultHasher::new();
        entry_json.hash(&mut hasher);
        let current_hash = format!("{:x}", hasher.finish());

        // Append to .jsonl
        let mut file = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.log_path)?;

        writeln!(file, "{}", entry_json)?;

        *last_hash_guard = current_hash;
        Ok(())
    }
}
