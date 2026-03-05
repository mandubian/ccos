//! Agent Memory Tier 1 and Tier 2.

use rusqlite::Connection;
use std::path::{Path, PathBuf};

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

/// Tier 2 Memory: Long-term indexed storage (SQLite stub).
pub struct Tier2Memory {
    conn: Connection,
}

impl Tier2Memory {
    pub fn new(agent_dir: &Path) -> anyhow::Result<Self> {
        let db_path = agent_dir.join("memory.db");
        let conn = Connection::open(&db_path)?;

        conn.execute(
            "CREATE TABLE IF NOT EXISTS memories (
                id TEXT PRIMARY KEY,
                content TEXT NOT NULL,
                timestamp DATETIME DEFAULT CURRENT_TIMESTAMP
            )",
            (),
        )?;

        Ok(Self { conn })
    }

    pub fn remember(&self, id: &str, content: &str) -> anyhow::Result<()> {
        self.conn.execute(
            "INSERT OR REPLACE INTO memories (id, content) VALUES (?1, ?2)",
            (id, content),
        )?;
        Ok(())
    }

    pub fn recall(&self, id: &str) -> anyhow::Result<String> {
        let mut stmt = self
            .conn
            .prepare("SELECT content FROM memories WHERE id = ?1")?;
        let content: String = stmt.query_row([id], |row| row.get(0))?;
        Ok(content)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_tier1_memory() {
        let temp = tempfile::tempdir().unwrap();
        let mem = Tier1Memory::new(temp.path()).unwrap();

        mem.write_file("notes.txt", "hello world").unwrap();
        assert_eq!(mem.read_file("notes.txt").unwrap(), "hello world");
        assert!(mem.write_file("../out.txt", "hacker").is_err());
    }

    #[test]
    fn test_tier2_memory() {
        let temp = tempfile::tempdir().unwrap();
        let mem = Tier2Memory::new(temp.path()).unwrap();

        mem.remember("fact_1", "The sky is blue").unwrap();
        assert_eq!(mem.recall("fact_1").unwrap(), "The sky is blue");
        assert!(mem.recall("fact_2").is_err());
    }
}
