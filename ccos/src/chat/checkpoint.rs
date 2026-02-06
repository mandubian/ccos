use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Captured state of an agent's execution
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Checkpoint {
    pub id: String,
    pub run_id: String,
    pub action_id: Option<String>,
    /// Serialized environment (variables, state)
    pub env: serde_json::Value,
    /// Instruction pointer/position in the causal chain or IR
    pub ir_pos: usize,
    /// Pending yield/approval status
    pub pending_yield: Option<String>,
    pub created_at: DateTime<Utc>,
    pub metadata: HashMap<String, String>,
}

impl Checkpoint {
    pub fn new(run_id: String, env: serde_json::Value, ir_pos: usize) -> Self {
        Self {
            id: format!("ckpt-{}", uuid::Uuid::new_v4()),
            run_id,
            action_id: None,
            env,
            ir_pos,
            pending_yield: None,
            created_at: Utc::now(),
            metadata: HashMap::new(),
        }
    }
}

pub trait CheckpointStore: Send + Sync {
    fn store_checkpoint(&self, checkpoint: Checkpoint) -> Result<(), String>;
    fn get_checkpoint(&self, checkpoint_id: &str) -> Result<Option<Checkpoint>, String>;
    fn list_checkpoints_for_run(&self, run_id: &str) -> Result<Vec<Checkpoint>, String>;
}

pub struct InMemoryCheckpointStore {
    checkpoints: std::sync::Mutex<HashMap<String, Checkpoint>>,
}

impl InMemoryCheckpointStore {
    pub fn new() -> Self {
        Self {
            checkpoints: std::sync::Mutex::new(HashMap::new()),
        }
    }
}

impl CheckpointStore for InMemoryCheckpointStore {
    fn store_checkpoint(&self, checkpoint: Checkpoint) -> Result<(), String> {
        self.checkpoints
            .lock()
            .unwrap()
            .insert(checkpoint.id.clone(), checkpoint);
        Ok(())
    }

    fn get_checkpoint(&self, checkpoint_id: &str) -> Result<Option<Checkpoint>, String> {
        Ok(self.checkpoints.lock().unwrap().get(checkpoint_id).cloned())
    }

    fn list_checkpoints_for_run(&self, run_id: &str) -> Result<Vec<Checkpoint>, String> {
        let ckpts = self.checkpoints.lock().unwrap();
        Ok(ckpts
            .values()
            .filter(|c| c.run_id == run_id)
            .cloned()
            .collect())
    }
}
