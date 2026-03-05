//! Tier 2 Memory Object — Gateway-substrate persistent memory.

use serde::{Deserialize, Serialize};

/// Visibility scope for a memory entry.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum MemoryVisibility {
    #[default]
    Private,
    Global,
}

/// A single Tier 2 memory object stored in the Gateway substrate.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemoryObject {
    pub key: String,
    pub value: String,
    pub owner: String,
    #[serde(default)]
    pub visibility: MemoryVisibility,
    pub created_at: String,
    pub updated_at: String,
    // Embedding vector omitted from the Rust type for now;
    // it will live in the storage backend.
}
