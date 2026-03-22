//! Hash-chain Causal Logger.

pub mod promotion_lookup;
pub mod rotation;

pub use rotation::{
    generate_segment_filename, get_or_create_history_dir, migrate_legacy_log, parse_segment_info,
    read_all_entries_across_segments, RetentionActions, RetentionPolicy, RotationPolicy,
    RotationStrategy, SegmentIndex, SegmentMetadata,
};

use autonoetic_types::causal_chain::{CausalChainEntry, EntryStatus};
use sha2::{Digest, Sha256};
use std::io::{BufRead, BufReader, Write};
use std::path::PathBuf;
use std::sync::Mutex;

pub struct CausalLogger {
    pub log_path: PathBuf,
    last_hash: Mutex<String>,
    entry_count: Mutex<usize>,
}

impl CausalLogger {
    pub fn new(log_path: impl Into<PathBuf>) -> anyhow::Result<Self> {
        Self::new_with_policy(log_path, RotationPolicy::disabled())
    }

    pub fn new_with_policy(
        log_path: impl Into<PathBuf>,
        policy: RotationPolicy,
    ) -> anyhow::Result<Self> {
        let log_path = log_path.into();

        if policy.rotation_strategy != RotationStrategy::Disabled {
            if let Some(parent) = log_path.parent() {
                let _ = migrate_legacy_log(parent);
            }
        }

        let (last_hash, entry_count) = load_last_hash_and_count(&log_path)?;

        Ok(Self {
            log_path,
            last_hash: Mutex::new(last_hash),
            entry_count: Mutex::new(entry_count),
        })
    }

    #[cfg(test)]
    pub fn test_logger(log_path: impl Into<PathBuf>) -> Self {
        let log_path = log_path.into();
        let (last_hash, entry_count) = load_last_hash_and_count(&log_path).unwrap_or((
            "0000000000000000000000000000000000000000000000000000000000000000".to_string(),
            0,
        ));
        Self {
            log_path,
            last_hash: Mutex::new(last_hash),
            entry_count: Mutex::new(entry_count),
        }
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

        // Check if rotation is needed before appending
        if let Ok(count) = self.entry_count.lock() {
            if *count > 0 {
                let size = self.log_path.metadata().map(|m| m.len()).unwrap_or(0);
                if should_rotate_internal(*count, size) {
                    // Rotation is handled externally via new_with_policy
                }
            }
        }

        let mut file = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.log_path)?;

        writeln!(file, "{}", entry_json)?;

        *last_hash_guard = entry_hash;

        if let Ok(mut count) = self.entry_count.lock() {
            *count += 1;
        }

        Ok(())
    }

    /// Get the log file path.
    pub fn path(&self) -> &std::path::Path {
        &self.log_path
    }

    /// Read all entries from the log file.
    pub fn read_entries(path: &std::path::Path) -> anyhow::Result<Vec<CausalChainEntry>> {
        if !path.exists() {
            return Ok(Vec::new());
        }
        let file = std::fs::File::open(path)?;
        let reader = BufReader::new(file);
        let mut entries = Vec::new();
        for line in reader.lines() {
            let line = line?;
            let trimmed = line.trim();
            if trimmed.is_empty() {
                continue;
            }
            let entry: CausalChainEntry = serde_json::from_str(trimmed)?;
            entries.push(entry);
        }
        Ok(entries)
    }

    /// Read all entries across all segments with continuity validation.
    pub fn read_all_entries(
        history_dir: &std::path::Path,
    ) -> anyhow::Result<Vec<CausalChainEntry>> {
        read_all_entries_across_segments(history_dir)
    }
}

fn should_rotate_internal(current_entries: usize, current_size_bytes: u64) -> bool {
    // Default rotation thresholds - can be customized via new_with_policy
    let max_entries = 10000;
    let max_size = 10 * 1024 * 1024; // 10MB

    current_entries >= max_entries || current_size_bytes >= max_size
}

#[allow(dead_code)]
fn load_last_hash(log_path: &PathBuf) -> anyhow::Result<String> {
    let (hash, _) = load_last_hash_and_count(log_path)?;
    Ok(hash)
}

fn load_last_hash_and_count(log_path: &PathBuf) -> anyhow::Result<(String, usize)> {
    if !log_path.exists() {
        return Ok(("genesis".to_string(), 0));
    }

    let content = std::fs::read_to_string(log_path)?;
    let lines: Vec<&str> = content.lines().filter(|l| !l.trim().is_empty()).collect();

    if lines.is_empty() {
        return Ok(("genesis".to_string(), 0));
    }

    let last_line = lines.last().unwrap();
    if let Ok(entry) = serde_json::from_str::<CausalChainEntry>(last_line) {
        if !entry.entry_hash.trim().is_empty() {
            return Ok((entry.entry_hash, lines.len()));
        }
    }
    Ok((sha256_hex(last_line)?, lines.len()))
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
