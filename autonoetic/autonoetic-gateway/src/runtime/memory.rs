//! Agent Memory Tier 1 and Tier 2 with provenance tracking.

use autonoetic_types::memory::MemoryObject;
use rusqlite::{params, Connection};
use std::path::{Path, PathBuf};
use std::sync::Arc;

/// Tier 1 Memory: Working state directory (`state/`).
/// Flat files for the agent's immediate situational awareness.
pub struct Tier1Memory {
    state_dir: PathBuf,
}

impl Tier1Memory {
    pub fn new(agent_dir: &Path) -> anyhow::Result<Self> {
        let state_dir = agent_dir.join("state");
        std::fs::create_dir_all(&state_dir)?;
        Ok(Self { state_dir })
    }

    pub fn write_file(&self, filename: &str, content: &str) -> anyhow::Result<()> {
        // Basic path traversal prevention
        if filename.contains('/') || filename.contains('\\') || filename.contains("..") {
            anyhow::bail!("Invalid filename mapping");
        }
        std::fs::write(self.state_dir.join(filename), content)?;
        Ok(())
    }

    pub fn read_file(&self, filename: &str) -> anyhow::Result<String> {
        let path = self.state_dir.join(filename);
        if !path.exists() {
            anyhow::bail!("File not found in Tier 1 memory");
        }
        Ok(std::fs::read_to_string(path)?)
    }
}

/// Tier 2 Memory: Gateway-managed long-term storage with provenance tracking.
///
/// This is the gateway-owned source of truth for durable facts and cross-agent recall.
/// All memory records include full provenance (writer, source, timestamps, content hash).
pub struct Tier2Memory {
    conn: Arc<Connection>,
    /// The agent ID that is currently using this memory instance.
    current_agent_id: String,
}

impl Tier2Memory {
    /// Creates a new Tier2Memory instance connected to the gateway-managed database.
    ///
    /// # Arguments
    /// * `gateway_dir` - Path to the gateway directory (contains memory.db)
    /// * `agent_id` - The ID of the agent using this memory instance
    pub fn new(gateway_dir: &Path, agent_id: &str) -> anyhow::Result<Self> {
        let db_path = gateway_dir.join("memory.db");
        let conn = Connection::open(&db_path)?;

        // Create the provenance-aware memories table
        conn.execute(
            "CREATE TABLE IF NOT EXISTS memories (
                memory_id TEXT PRIMARY KEY,
                scope TEXT NOT NULL,
                owner_agent_id TEXT NOT NULL,
                writer_agent_id TEXT NOT NULL,
                source_type TEXT NOT NULL DEFAULT 'agent_write',
                source_ref TEXT NOT NULL,
                created_at TEXT NOT NULL,
                updated_at TEXT NOT NULL,
                content TEXT NOT NULL,
                content_hash TEXT NOT NULL,
                confidence REAL,
                tags TEXT,
                lineage TEXT,
                visibility TEXT NOT NULL DEFAULT 'private',
                allowed_agents TEXT
            )",
            [],
        )?;

        // Create index for scope-based queries
        conn.execute(
            "CREATE INDEX IF NOT EXISTS idx_memories_scope ON memories(scope)",
            [],
        )?;

        // Create index for owner-based queries
        conn.execute(
            "CREATE INDEX IF NOT EXISTS idx_memories_owner ON memories(owner_agent_id)",
            [],
        )?;

        // Create index for visibility-based queries
        conn.execute(
            "CREATE INDEX IF NOT EXISTS idx_memories_visibility ON memories(visibility)",
            [],
        )?;

        Ok(Self {
            conn: Arc::new(conn),
            current_agent_id: agent_id.to_string(),
        })
    }

    /// Stores a new memory record or updates an existing one.
    ///
    /// # Arguments
    /// * `memory_id` - Unique identifier for the memory
    /// * `scope` - Scope/namespace for organizing memory
    /// * `owner_agent_id` - Agent that owns this memory
    /// * `source_ref` - Reference to causal chain entry or session
    /// * `content` - The content to store
    pub fn remember(
        &self,
        memory_id: &str,
        scope: &str,
        owner_agent_id: &str,
        source_ref: &str,
        content: &str,
    ) -> anyhow::Result<MemoryObject> {
        let memory = MemoryObject::new(
            memory_id.to_string(),
            scope.to_string(),
            owner_agent_id.to_string(),
            self.current_agent_id.clone(),
            source_ref.to_string(),
            content.to_string(),
        );

        self.save_memory(&memory)
    }

    /// Saves a MemoryObject to the database.
    pub fn save_memory(&self, memory: &MemoryObject) -> anyhow::Result<MemoryObject> {
        let tags_json = serde_json::to_string(&memory.tags)?;
        let lineage_json = serde_json::to_string(&memory.lineage)?;
        let allowed_agents_json = serde_json::to_string(&memory.allowed_agents)?;

        self.conn.execute(
            "INSERT OR REPLACE INTO memories (
                memory_id, scope, owner_agent_id, writer_agent_id, source_type, source_ref,
                created_at, updated_at, content, content_hash, confidence, tags, lineage,
                visibility, allowed_agents
            ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15)",
            params![
                memory.memory_id,
                memory.scope,
                memory.owner_agent_id,
                memory.writer_agent_id,
                serde_json::to_string(&memory.source_type)?,
                memory.source_ref,
                memory.created_at,
                memory.updated_at,
                memory.content,
                memory.content_hash,
                memory.confidence,
                tags_json,
                lineage_json,
                serde_json::to_string(&memory.visibility)?,
                allowed_agents_json,
            ],
        )?;

        Ok(memory.clone())
    }

    /// Recalls a memory by its ID.
    ///
    /// Enforces visibility/ACL checks based on the current agent.
    pub fn recall(&self, memory_id: &str) -> anyhow::Result<MemoryObject> {
        let memory: MemoryObject = self
            .conn
            .prepare("SELECT * FROM memories WHERE memory_id = ?1")?
            .query_row(
                params![memory_id],
                |row| -> rusqlite::Result<MemoryObject> {
                    let source_type_str: String = row.get(4)?;
                    let tags_str: String = row.get(11)?;
                    let lineage_str: String = row.get(12)?;
                    let visibility_str: String = row.get(13)?;
                    let allowed_agents_str: String = row.get(14)?;

                    Ok(MemoryObject {
                        memory_id: row.get(0)?,
                        scope: row.get(1)?,
                        owner_agent_id: row.get(2)?,
                        writer_agent_id: row.get(3)?,
                        source_type: serde_json::from_str(&source_type_str).map_err(|e| {
                            rusqlite::Error::FromSqlConversionFailure(
                                0,
                                rusqlite::types::Type::Text,
                                e.to_string().into(),
                            )
                        })?,
                        source_ref: row.get(5)?,
                        created_at: row.get(6)?,
                        updated_at: row.get(7)?,
                        content: row.get(8)?,
                        content_hash: row.get(9)?,
                        confidence: row.get(10)?,
                        tags: serde_json::from_str(&tags_str).map_err(|e| {
                            rusqlite::Error::FromSqlConversionFailure(
                                0,
                                rusqlite::types::Type::Text,
                                e.to_string().into(),
                            )
                        })?,
                        lineage: serde_json::from_str(&lineage_str).map_err(|e| {
                            rusqlite::Error::FromSqlConversionFailure(
                                0,
                                rusqlite::types::Type::Text,
                                e.to_string().into(),
                            )
                        })?,
                        visibility: serde_json::from_str(&visibility_str).map_err(|e| {
                            rusqlite::Error::FromSqlConversionFailure(
                                0,
                                rusqlite::types::Type::Text,
                                e.to_string().into(),
                            )
                        })?,
                        allowed_agents: serde_json::from_str(&allowed_agents_str).map_err(|e| {
                            rusqlite::Error::FromSqlConversionFailure(
                                0,
                                rusqlite::types::Type::Text,
                                e.to_string().into(),
                            )
                        })?,
                    })
                },
            )?;

        // Enforce visibility check
        if !memory.is_readable_by(&self.current_agent_id) {
            anyhow::bail!(
                "Memory '{}' is not accessible to agent '{}'",
                memory_id,
                self.current_agent_id
            );
        }

        Ok(memory)
    }

    /// Searches memories by scope and optional query terms.
    ///
    /// Returns memories that match the scope and are visible to the current agent.
    pub fn search(&self, scope: &str, query: Option<&str>) -> anyhow::Result<Vec<MemoryObject>> {
        let mut sql = String::from("SELECT * FROM memories WHERE scope = ?1");

        if let Some(_q) = query {
            sql.push_str(" AND content LIKE ?2");
        }

        sql.push_str(" ORDER BY updated_at DESC");

        let mut stmt = self.conn.prepare(&sql)?;

        let mut rows = if let Some(q) = query {
            let search_term = format!("%{}%", q);
            stmt.query(params![scope, search_term])?
        } else {
            stmt.query(params![scope])?
        };

        let mut results = Vec::new();
        while let Some(row) = rows.next()? {
            let memory_id: String = row.get(0)?;

            // Only include memories visible to current agent
            // Propagate errors for debugging DB/serde issues
            match self.recall(&memory_id) {
                Ok(memory) => results.push(memory),
                Err(e) => {
                    // Log the error for debugging but don't fail the entire search
                    tracing::warn!(
                        "Failed to recall memory '{}' during search: {}",
                        memory_id,
                        e
                    );
                    // Continue with other memories
                }
            }
        }

        Ok(results)
    }

    /// Shares a memory with specific agents.
    ///
    /// Requires the current agent to be the owner or writer.
    pub fn share_with(
        &self,
        memory_id: &str,
        target_agents: Vec<String>,
    ) -> anyhow::Result<MemoryObject> {
        let memory = self.recall(memory_id)?;

        // Only owner or writer can share
        if memory.owner_agent_id != self.current_agent_id
            && memory.writer_agent_id != self.current_agent_id
        {
            anyhow::bail!("Only the owner or writer can share a memory");
        }

        let updated = memory.share_with(target_agents);
        self.save_memory(&updated)?;

        Ok(updated)
    }

    /// Makes a memory globally visible.
    pub fn make_global(&self, memory_id: &str) -> anyhow::Result<MemoryObject> {
        let memory = self.recall(memory_id)?;

        // Only owner can make global
        if memory.owner_agent_id != self.current_agent_id {
            anyhow::bail!("Only the owner can make a memory global");
        }

        let updated = memory.make_global();
        self.save_memory(&updated)?;

        Ok(updated)
    }

    /// Lists all scopes available to the current agent.
    /// Only returns scopes where the agent has at least one visible memory.
    pub fn list_scopes(&self) -> anyhow::Result<Vec<String>> {
        // A memory is visible if:
        // 1. visibility = '"global"', OR
        // 2. visibility = '"private"' AND (owner_agent_id = current_agent_id OR writer_agent_id = current_agent_id), OR
        // 3. visibility = '"shared"' AND (owner_agent_id = current_agent_id OR writer_agent_id = current_agent_id OR current_agent_id is in allowed_agents)
        // Note: visibility is stored as JSON string (e.g., '"private"')
        let mut stmt = self.conn.prepare(LIST_SCOPES_SQL)?;
        let mut rows = stmt.query(params![&self.current_agent_id])?;

        let mut scopes = Vec::new();
        while let Some(row) = rows.next()? {
            let scope: String = row.get(0)?;
            scopes.push(scope);
        }

        Ok(scopes)
    }

    /// Lists all memories owned by the current agent.
    pub fn list_memories(&self) -> anyhow::Result<Vec<MemoryObject>> {
        let mut stmt = self.conn.prepare(
            "SELECT memory_id FROM memories WHERE owner_agent_id = ?1 ORDER BY created_at DESC",
        )?;
        let mut rows = stmt.query(params![&self.current_agent_id])?;

        let mut memories = Vec::new();
        while let Some(row) = rows.next()? {
            let memory_id: String = row.get(0)?;
            match self.recall(&memory_id) {
                Ok(memory) => memories.push(memory),
                Err(e) => {
                    tracing::warn!("Failed to recall memory '{}' during list: {}", memory_id, e);
                }
            }
        }

        Ok(memories)
    }
}

const LIST_SCOPES_SQL: &str = r#"
    SELECT DISTINCT scope FROM memories
    WHERE json_extract(visibility, '$') = 'global'
       OR (json_extract(visibility, '$') = 'private'
           AND (owner_agent_id = ?1 OR writer_agent_id = ?1))
       OR (json_extract(visibility, '$') = 'shared'
           AND (owner_agent_id = ?1 OR writer_agent_id = ?1
                OR (allowed_agents IS NOT NULL
                    AND json_valid(allowed_agents)
                    AND ?1 IN (SELECT value FROM json_each(allowed_agents)))))
    ORDER BY scope
"#;

#[cfg(test)]
mod tests {
    use super::*;
    use autonoetic_types::memory::{MemorySourceType, MemoryVisibility};

    #[test]
    fn test_tier1_memory() {
        let temp = tempfile::tempdir().unwrap();
        let mem = Tier1Memory::new(temp.path()).unwrap();

        mem.write_file("notes.txt", "hello world").unwrap();
        assert_eq!(mem.read_file("notes.txt").unwrap(), "hello world");
        assert!(mem.write_file("../out.txt", "hacker").is_err());
    }

    #[test]
    fn test_tier2_memory_basic() {
        let temp = tempfile::tempdir().unwrap();
        let mem = Tier2Memory::new(temp.path(), "agent-1").unwrap();

        let memory = mem
            .remember(
                "fact_1",
                "general",
                "agent-1",
                "session:test:turn:1",
                "The sky is blue",
            )
            .unwrap();

        assert_eq!(memory.memory_id, "fact_1");
        assert_eq!(memory.content, "The sky is blue");
        assert_eq!(memory.owner_agent_id, "agent-1");
        assert_eq!(memory.visibility, MemoryVisibility::Private);

        // Verify content hash is set
        assert!(!memory.content_hash.is_empty());
    }

    #[test]
    fn test_tier2_memory_recall() {
        let temp = tempfile::tempdir().unwrap();
        let mem = Tier2Memory::new(temp.path(), "agent-1").unwrap();

        mem.remember(
            "fact_1",
            "general",
            "agent-1",
            "session:test:turn:1",
            "The sky is blue",
        )
        .unwrap();

        let recalled = mem.recall("fact_1").unwrap();
        assert_eq!(recalled.content, "The sky is blue");

        // Non-existent memory should fail
        assert!(mem.recall("fact_2").is_err());
    }

    #[test]
    fn test_tier2_memory_visibility_private() {
        let temp = tempfile::tempdir().unwrap();
        let mem1 = Tier2Memory::new(temp.path(), "agent-1").unwrap();
        let mem2 = Tier2Memory::new(temp.path(), "agent-2").unwrap();

        mem1.remember(
            "fact_1",
            "general",
            "agent-1",
            "session:test:turn:1",
            "Private fact",
        )
        .unwrap();

        // agent-1 can read its own memory
        assert!(mem1.recall("fact_1").is_ok());

        // agent-2 cannot read agent-1's private memory
        assert!(mem2.recall("fact_1").is_err());
    }

    #[test]
    fn test_tier2_memory_sharing() {
        let temp = tempfile::tempdir().unwrap();
        let mem1 = Tier2Memory::new(temp.path(), "agent-1").unwrap();
        let mem2 = Tier2Memory::new(temp.path(), "agent-2").unwrap();

        mem1.remember(
            "fact_1",
            "general",
            "agent-1",
            "session:test:turn:1",
            "Shared fact",
        )
        .unwrap();

        // Share with agent-2
        mem1.share_with("fact_1", vec!["agent-2".to_string()])
            .unwrap();

        // Now agent-2 can read it
        let recalled = mem2.recall("fact_1").unwrap();
        assert_eq!(recalled.content, "Shared fact");
        assert_eq!(recalled.visibility, MemoryVisibility::Shared);
        assert!(recalled.allowed_agents.contains(&"agent-2".to_string()));
    }

    #[test]
    fn test_tier2_memory_global() {
        let temp = tempfile::tempdir().unwrap();
        let mem1 = Tier2Memory::new(temp.path(), "agent-1").unwrap();
        let mem2 = Tier2Memory::new(temp.path(), "agent-2").unwrap();

        mem1.remember(
            "fact_1",
            "general",
            "agent-1",
            "session:test:turn:1",
            "Global fact",
        )
        .unwrap();

        // Make global
        mem1.make_global("fact_1").unwrap();

        // All agents can read it
        assert!(mem1.recall("fact_1").is_ok());
        assert!(mem2.recall("fact_1").is_ok());
    }

    #[test]
    fn test_tier2_memory_search() {
        let temp = tempfile::tempdir().unwrap();
        let mem = Tier2Memory::new(temp.path(), "agent-1").unwrap();

        mem.remember(
            "fact_1",
            "weather",
            "agent-1",
            "session:test:turn:1",
            "Paris is sunny",
        )
        .unwrap();

        mem.remember(
            "fact_2",
            "weather",
            "agent-1",
            "session:test:turn:2",
            "London is rainy",
        )
        .unwrap();

        mem.remember(
            "fact_3",
            "geography",
            "agent-1",
            "session:test:turn:3",
            "Paris is in France",
        )
        .unwrap();

        // Search by scope
        let results = mem.search("weather", None).unwrap();
        assert_eq!(results.len(), 2);

        // Search by scope and query
        let results = mem.search("weather", Some("Paris")).unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].memory_id, "fact_1");
    }

    #[test]
    fn test_tier2_memory_provenance() {
        let temp = tempfile::tempdir().unwrap();
        let mem = Tier2Memory::new(temp.path(), "agent-1").unwrap();

        let memory = mem
            .remember(
                "fact_1",
                "general",
                "agent-1",
                "session:abc123:turn:5",
                "Important fact",
            )
            .unwrap();

        // Verify provenance fields
        assert_eq!(memory.writer_agent_id, "agent-1");
        assert_eq!(memory.source_ref, "session:abc123:turn:5");
        assert_eq!(memory.source_type, MemorySourceType::AgentWrite);
        assert!(!memory.created_at.is_empty());
        assert!(!memory.updated_at.is_empty());
        assert!(!memory.content_hash.is_empty());
    }
}
