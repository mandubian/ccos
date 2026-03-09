//! Tier 2 Memory Object — Gateway-substrate persistent memory with provenance tracking.

use serde::{Deserialize, Serialize};

/// Visibility scope for a memory entry.
#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum MemoryVisibility {
    #[default]
    Private,
    Shared,
    Global,
}

/// Source type for a memory record (tracks origin of the fact).
#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum MemorySourceType {
    #[default]
    AgentWrite,
    ToolOutput,
    IngestedEvent,
    ScheduledAction,
    Manual,
}

/// Lineage entry tracks the ancestry of a memory record.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemoryLineageEntry {
    pub source_memory_id: String,
    pub operation: String,
    pub agent_id: String,
    pub timestamp: String,
}

/// A single Tier 2 memory object stored in the Gateway substrate with full provenance.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemoryObject {
    /// Unique identifier for this memory record.
    pub memory_id: String,

    /// Scope/namespace for organizing memory (e.g., "facts", "preferences", "context").
    pub scope: String,

    /// Agent that owns this memory record (typically the agent that created it).
    pub owner_agent_id: String,

    /// Agent that wrote/updated this record (for tracking cross-agent sharing).
    pub writer_agent_id: String,

    /// Type of source that created this record.
    #[serde(default)]
    pub source_type: MemorySourceType,

    /// Reference to the causal chain entry, session ID, or other origin artifact.
    /// Format: "session:<session_id>:turn:<turn_id>" or "causal:<log_id>".
    pub source_ref: String,

    /// ISO 8601 timestamp when the record was created.
    pub created_at: String,

    /// ISO 8601 timestamp when the record was last updated.
    pub updated_at: String,

    /// The actual content/value of the memory.
    pub content: String,

    /// SHA-256 hash of the content for integrity verification.
    pub content_hash: String,

    /// Optional confidence score (0.0-1.0) for the fact's reliability.
    #[serde(default)]
    pub confidence: Option<f64>,

    /// Optional tags for categorization and filtering.
    #[serde(default)]
    pub tags: Vec<String>,

    /// Optional lineage tracking for derived/transformed memories.
    #[serde(default)]
    pub lineage: Vec<MemoryLineageEntry>,

    /// Visibility/ACL for controlling access and sharing.
    #[serde(default)]
    pub visibility: MemoryVisibility,

    /// Optional list of agent IDs explicitly granted access (when visibility=Shared).
    #[serde(default)]
    pub allowed_agents: Vec<String>,
}

impl MemoryObject {
    /// Creates a new MemoryObject with required fields.
    pub fn new(
        memory_id: String,
        scope: String,
        owner_agent_id: String,
        writer_agent_id: String,
        source_ref: String,
        content: String,
    ) -> Self {
        use sha2::{Digest, Sha256};
        let mut hasher = Sha256::new();
        hasher.update(content.as_bytes());
        let content_hash = hex::encode(hasher.finalize());

        let now = chrono::Utc::now().to_rfc3339();

        Self {
            memory_id,
            scope,
            owner_agent_id,
            writer_agent_id,
            source_type: MemorySourceType::default(),
            source_ref,
            created_at: now.clone(),
            updated_at: now,
            content,
            content_hash,
            confidence: None,
            tags: Vec::new(),
            lineage: Vec::new(),
            visibility: MemoryVisibility::default(),
            allowed_agents: Vec::new(),
        }
    }

    /// Updates the content and returns a new MemoryObject with updated timestamps and hash.
    pub fn update_content(mut self, new_content: String, writer_agent_id: String) -> Self {
        use sha2::{Digest, Sha256};
        let mut hasher = Sha256::new();
        hasher.update(new_content.as_bytes());

        self.content = new_content;
        self.content_hash = hex::encode(hasher.finalize());
        self.writer_agent_id = writer_agent_id;
        self.updated_at = chrono::Utc::now().to_rfc3339();

        self
    }

    /// Shares this memory with specific agents.
    pub fn share_with(mut self, agents: Vec<String>) -> Self {
        self.visibility = MemoryVisibility::Shared;
        self.allowed_agents = agents;
        self.updated_at = chrono::Utc::now().to_rfc3339();
        self
    }

    /// Makes this memory globally visible.
    pub fn make_global(mut self) -> Self {
        self.visibility = MemoryVisibility::Global;
        self.allowed_agents = Vec::new();
        self.updated_at = chrono::Utc::now().to_rfc3339();
        self
    }

    /// Checks if an agent is allowed to read this memory.
    pub fn is_readable_by(&self, agent_id: &str) -> bool {
        match self.visibility {
            MemoryVisibility::Private => {
                self.owner_agent_id == agent_id || self.writer_agent_id == agent_id
            }
            MemoryVisibility::Shared => {
                self.owner_agent_id == agent_id
                    || self.writer_agent_id == agent_id
                    || self.allowed_agents.contains(&agent_id.to_string())
            }
            MemoryVisibility::Global => true,
        }
    }

    /// Checks if an agent is allowed to write/update this memory.
    pub fn is_writable_by(&self, agent_id: &str) -> bool {
        self.owner_agent_id == agent_id || self.writer_agent_id == agent_id
    }
}
