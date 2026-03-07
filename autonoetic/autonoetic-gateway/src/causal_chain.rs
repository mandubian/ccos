//! Hash-chain Causal Logger.

use autonoetic_types::causal_chain::{CausalChainEntry, EntryStatus};
use sha2::{Digest, Sha256};
use std::io::Write;
use std::path::PathBuf;
use std::sync::Mutex;

pub struct CausalLogger {
    log_path: PathBuf,
    last_hash: Mutex<String>,
}

impl CausalLogger {
    pub fn new(log_path: impl Into<PathBuf>) -> anyhow::Result<Self> {
        let log_path = log_path.into();
        let last_hash = load_last_hash(&log_path)?;
        Ok(Self {
            log_path,
            last_hash: Mutex::new(last_hash),
        })
    }

    /// Append a new action to the Causal Chain.
    pub fn log(
        &self,
        actor_id: &str,
        session_id: &str,
        turn_id: Option<&str>,
        event_seq: u64,
        category: &str,
        action: &str,
        status: EntryStatus,
        payload: Option<serde_json::Value>,
    ) -> anyhow::Result<()> {
        let mut last_hash_guard = self
            .last_hash
            .lock()
            .map_err(|_| anyhow::anyhow!("causal logger mutex poisoned"))?;
        let prev_hash = last_hash_guard.clone();
        let payload_hash = payload_hash(&payload)?;

        let timestamp = chrono::Utc::now().to_rfc3339();
        let log_id = uuid::Uuid::new_v4().to_string();
        let entry_hash = compute_entry_hash(
            &timestamp,
            &log_id,
            actor_id,
            session_id,
            turn_id,
            event_seq,
            category,
            action,
            &status,
            payload_hash.as_deref(),
            &prev_hash,
        )?;

        let entry = CausalChainEntry {
            timestamp,
            log_id,
            actor_id: actor_id.to_string(),
            session_id: session_id.to_string(),
            turn_id: turn_id.map(|v| v.to_string()),
            event_seq,
            category: category.to_string(),
            action: action.to_string(),
            target: None,
            status,
            reason: None,
            payload,
            payload_hash,
            prev_hash: prev_hash.clone(),
            entry_hash: entry_hash.clone(),
        };

        let entry_json = serde_json::to_string(&entry)?;

        // Append to .jsonl
        let mut file = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.log_path)?;

        writeln!(file, "{}", entry_json)?;

        *last_hash_guard = entry_hash;
        Ok(())
    }
}

fn load_last_hash(log_path: &PathBuf) -> anyhow::Result<String> {
    if !log_path.exists() {
        return Ok("genesis".to_string());
    }
    let content = std::fs::read_to_string(log_path)?;
    let Some(last_line) = content.lines().rev().find(|l| !l.trim().is_empty()) else {
        return Ok("genesis".to_string());
    };
    if let Ok(entry) = serde_json::from_str::<CausalChainEntry>(last_line) {
        if !entry.entry_hash.trim().is_empty() {
            return Ok(entry.entry_hash);
        }
    }
    sha256_hex(last_line)
}

fn payload_hash(payload: &Option<serde_json::Value>) -> anyhow::Result<Option<String>> {
    let Some(payload) = payload else {
        return Ok(None);
    };
    let encoded = serde_json::to_string(payload)?;
    Ok(Some(sha256_hex(&encoded)?))
}

fn compute_entry_hash(
    timestamp: &str,
    log_id: &str,
    actor_id: &str,
    session_id: &str,
    turn_id: Option<&str>,
    event_seq: u64,
    category: &str,
    action: &str,
    status: &EntryStatus,
    payload_hash: Option<&str>,
    prev_hash: &str,
) -> anyhow::Result<String> {
    let canonical = serde_json::json!({
        "timestamp": timestamp,
        "log_id": log_id,
        "actor_id": actor_id,
        "session_id": session_id,
        "turn_id": turn_id,
        "event_seq": event_seq,
        "category": category,
        "action": action,
        "status": status,
        "payload_hash": payload_hash,
        "prev_hash": prev_hash
    });
    let encoded = serde_json::to_string(&canonical)?;
    sha256_hex(&encoded)
}

fn sha256_hex(input: &str) -> anyhow::Result<String> {
    let mut hasher = Sha256::new();
    hasher.update(input.as_bytes());
    let digest = hasher.finalize();
    Ok(format!("{:x}", digest))
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn test_logger_reloads_last_hash_across_instances() {
        let temp = tempdir().expect("tempdir should create");
        let path = temp.path().join("causal_chain.jsonl");

        let logger = CausalLogger::new(&path).expect("logger should init");
        logger
            .log(
                "agent-a",
                "session-1",
                Some("turn-000001"),
                1,
                "lifecycle",
                "wake",
                EntryStatus::Success,
                Some(serde_json::json!({"k":"v"})),
            )
            .expect("first log should append");

        let content = std::fs::read_to_string(&path).expect("log should read");
        let first: CausalChainEntry =
            serde_json::from_str(content.lines().next().expect("first line should exist"))
                .expect("entry should parse");

        let logger2 = CausalLogger::new(&path).expect("second logger should init");
        logger2
            .log(
                "agent-a",
                "session-1",
                Some("turn-000001"),
                2,
                "lifecycle",
                "hibernate",
                EntryStatus::Success,
                None,
            )
            .expect("second log should append");

        let content = std::fs::read_to_string(&path).expect("log should read");
        let second: CausalChainEntry =
            serde_json::from_str(content.lines().nth(1).expect("second line should exist"))
                .expect("entry should parse");
        assert_eq!(second.prev_hash, first.entry_hash);
    }
}
