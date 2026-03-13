//! Causal Chain Rotation Module.
//!
//! Provides segmentation of causal logs by date and size while preserving
//! hash-chain continuity across rotated segments.

use crate::CausalLogger;
use autonoetic_types::causal_chain::CausalChainEntry;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::fs;
use std::path::{Path, PathBuf};

const SEGMENT_PREFIX: &str = "causal_chain-";
const INDEX_FILENAME: &str = "segments.json";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RotationPolicy {
    pub max_entries_per_segment: Option<usize>,
    pub max_segment_size_bytes: Option<u64>,
    pub rotation_strategy: RotationStrategy,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum RotationStrategy {
    Size,
    Date,
    SizeOrDate,
    Disabled,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RetentionPolicy {
    pub max_age_days: Option<u32>,
    pub max_total_size_mb: Option<u64>,
    pub compress_after_days: Option<u32>,
    pub delete_after_days: Option<u32>,
}

impl Default for RetentionPolicy {
    fn default() -> Self {
        Self {
            max_age_days: Some(90),
            max_total_size_mb: Some(1024), // 1GB
            compress_after_days: Some(30),
            delete_after_days: None,
        }
    }
}

impl RetentionPolicy {
    pub fn disabled() -> Self {
        Self {
            max_age_days: None,
            max_total_size_mb: None,
            compress_after_days: None,
            delete_after_days: None,
        }
    }
}

impl Default for RotationPolicy {
    fn default() -> Self {
        Self {
            max_entries_per_segment: Some(10000),
            max_segment_size_bytes: Some(10 * 1024 * 1024), // 10MB
            rotation_strategy: RotationStrategy::SizeOrDate,
        }
    }
}

impl RotationPolicy {
    pub fn disabled() -> Self {
        Self {
            max_entries_per_segment: None,
            max_segment_size_bytes: None,
            rotation_strategy: RotationStrategy::Disabled,
        }
    }

    pub fn should_rotate(&self, current_entries: usize, current_size_bytes: u64) -> bool {
        if self.rotation_strategy == RotationStrategy::Disabled {
            return false;
        }

        if let Some(max_entries) = self.max_entries_per_segment {
            if current_entries >= max_entries {
                return true;
            }
        }

        if let Some(max_size) = self.max_segment_size_bytes {
            if current_size_bytes >= max_size {
                return true;
            }
        }

        false
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SegmentMetadata {
    pub segment_id: String,
    pub filename: String,
    pub first_entry_timestamp: String,
    pub first_entry_hash: String,
    pub prev_segment_hash: Option<String>,
    pub entry_count: usize,
    pub file_size_bytes: u64,
    pub created_at: String,
    pub is_compressed: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SegmentIndex {
    pub history_dir: PathBuf,
    pub segments: Vec<SegmentMetadata>,
    pub current_segment_filename: String,
    pub total_entry_count: usize,
    pub genesis_hash: String,
}

impl SegmentIndex {
    pub fn new(history_dir: impl Into<PathBuf>) -> Self {
        Self {
            history_dir: history_dir.into(),
            segments: Vec::new(),
            current_segment_filename: format!("{}current.jsonl", SEGMENT_PREFIX),
            total_entry_count: 0,
            genesis_hash: "genesis".to_string(),
        }
    }

    pub fn load(history_dir: &Path) -> anyhow::Result<Option<Self>> {
        let index_path = history_dir.join(INDEX_FILENAME);
        if !index_path.exists() {
            return Ok(None);
        }
        let content = fs::read_to_string(&index_path)?;
        let index: SegmentIndex = serde_json::from_str(&content)?;
        Ok(Some(index))
    }

    pub fn save(&self) -> anyhow::Result<()> {
        let index_path = self.history_dir.join(INDEX_FILENAME);
        fs::create_dir_all(&self.history_dir)?;
        let content = serde_json::to_string_pretty(self)?;
        fs::write(index_path, content)?;
        Ok(())
    }

    pub fn discover_segments(&self) -> Vec<PathBuf> {
        let mut segments = Vec::new();
        if let Ok(entries) = fs::read_dir(&self.history_dir) {
            for entry in entries.flatten() {
                let path = entry.path();
                if let Some(name) = path.file_name() {
                    let name = name.to_string_lossy();
                    if name.starts_with(SEGMENT_PREFIX) && name.ends_with(".jsonl") {
                        segments.push(path);
                    }
                }
            }
        }
        segments.sort();
        segments
    }

    pub fn add_segment(&mut self, metadata: SegmentMetadata) {
        self.total_entry_count += metadata.entry_count;
        self.segments.push(metadata);
    }
}

pub fn generate_segment_filename() -> String {
    let now = chrono::Utc::now();
    let uuid = uuid::Uuid::new_v4().to_string()[..8].to_string();
    format!(
        "{}{}-{:04}.jsonl",
        SEGMENT_PREFIX,
        now.format("%Y-%m-%d"),
        uuid
    )
}

pub fn parse_segment_info(filename: &str) -> Option<(String, String)> {
    if !filename.starts_with(SEGMENT_PREFIX) || !filename.ends_with(".jsonl") {
        return None;
    }
    let stem = filename
        .strip_prefix(SEGMENT_PREFIX)?
        .strip_suffix(".jsonl")?;

    // Format is YYYY-MM-DD-XXXXXXXX.jsonl - split on the last dash before uuid
    if let Some(idx) = stem.rfind('-') {
        let date = stem[..idx].to_string();
        let uuid = stem[idx + 1..].to_string();
        if !date.is_empty() && !uuid.is_empty() {
            return Some((date, uuid));
        }
    }
    None
}

pub fn compute_continuity_hash(
    last_entry: &CausalChainEntry,
    prev_segment_last_hash: Option<&str>,
) -> String {
    let mut hasher = Sha256::new();
    hasher.update(last_entry.entry_hash.as_bytes());
    if let Some(prev) = prev_segment_last_hash {
        hasher.update(prev.as_bytes());
    }
    format!("{:x}", hasher.finalize())
}

pub fn get_or_create_history_dir(agent_dir: &Path) -> PathBuf {
    let history_dir = agent_dir.join("history");
    if !history_dir.exists() {
        fs::create_dir_all(&history_dir).ok();
    }
    history_dir
}

pub fn migrate_legacy_log(history_dir: &Path) -> anyhow::Result<Option<String>> {
    let legacy_path = history_dir.join("causal_chain.jsonl");
    if !legacy_path.exists() {
        return Ok(None);
    }

    let content = fs::read_to_string(&legacy_path)?;
    let lines: Vec<&str> = content.lines().filter(|l| !l.trim().is_empty()).collect();
    if lines.is_empty() {
        fs::rename(
            &legacy_path,
            history_dir.join("causal_chain-legacy-empty.jsonl"),
        )?;
        return Ok(None);
    }

    let last_line = lines.last().unwrap();
    let last_entry: CausalChainEntry = serde_json::from_str(last_line)?;
    let last_hash = last_entry.entry_hash.clone();

    let segment_filename = generate_segment_filename();
    let new_path = history_dir.join(&segment_filename);
    fs::rename(&legacy_path, &new_path)?;

    Ok(Some(last_hash))
}

pub fn read_all_entries_across_segments(
    history_dir: &Path,
) -> anyhow::Result<Vec<CausalChainEntry>> {
    let index = SegmentIndex::load(history_dir)?;

    let mut all_entries = Vec::new();
    let mut prev_segment_hash: Option<String> = None;

    if let Some(idx) = index {
        for seg in &idx.segments {
            let seg_path = history_dir.join(&seg.filename);
            if seg_path.exists() {
                let entries = CausalLogger::read_entries(&seg_path)?;
                for entry in entries {
                    if let Some(prev) = &prev_segment_hash {
                        if entry.prev_hash != *prev {
                            anyhow::bail!(
                                "Hash chain broken at segment {}: expected prev_hash {}, got {}",
                                seg.filename,
                                prev,
                                entry.prev_hash
                            );
                        }
                    }
                    prev_segment_hash = Some(entry.entry_hash.clone());
                    all_entries.push(entry);
                }
            }
        }

        let current_path = history_dir.join(&idx.current_segment_filename);
        if current_path.exists() {
            let entries = CausalLogger::read_entries(&current_path)?;
            for entry in entries {
                if let Some(prev) = &prev_segment_hash {
                    if entry.prev_hash != *prev {
                        anyhow::bail!(
                            "Hash chain broken in current segment: expected prev_hash {}, got {}",
                            prev,
                            entry.prev_hash
                        );
                    }
                }
                prev_segment_hash = Some(entry.entry_hash.clone());
                all_entries.push(entry);
            }
        }
    } else {
        let legacy_path = history_dir.join("causal_chain.jsonl");
        if legacy_path.exists() {
            all_entries = CausalLogger::read_entries(&legacy_path)?;
        }
    }

    Ok(all_entries)
}

#[allow(dead_code)]
pub fn compress_segment(segment_path: &Path) -> anyhow::Result<PathBuf> {
    anyhow::bail!("Compression requires flate2 crate - add to Cargo.toml dependencies");
}

#[allow(dead_code)]
pub fn decompress_segment(compressed_path: &Path) -> anyhow::Result<PathBuf> {
    anyhow::bail!("Decompression requires flate2 crate - add to Cargo.toml dependencies");
}

pub fn apply_retention_policy(
    history_dir: &Path,
    policy: &RetentionPolicy,
) -> anyhow::Result<RetentionActions> {
    let mut actions = RetentionActions::default();

    let index = match SegmentIndex::load(history_dir)? {
        Some(idx) => idx,
        None => return Ok(actions),
    };

    let now = chrono::Utc::now();

    for seg in &index.segments {
        let created = chrono::DateTime::parse_from_rfc3339(&seg.created_at)
            .map(|dt| dt.with_timezone(&chrono::Utc))
            .unwrap_or(now);
        let age_days = (now - created).num_days() as u32;

        // Check compression
        if let Some(compress_after) = policy.compress_after_days {
            if age_days >= compress_after && !seg.is_compressed {
                actions.segments_to_compress.push(seg.filename.clone());
            }
        }

        // Check deletion
        if let Some(delete_after) = policy.delete_after_days {
            if age_days >= delete_after {
                actions.segments_to_delete.push(seg.filename.clone());
            }
        }
    }

    Ok(actions)
}

#[derive(Debug, Default)]
pub struct RetentionActions {
    pub segments_to_compress: Vec<String>,
    pub segments_to_delete: Vec<String>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn test_rotation_policy_size() {
        let policy = RotationPolicy {
            max_entries_per_segment: Some(100),
            max_segment_size_bytes: Some(1000),
            rotation_strategy: RotationStrategy::Size,
        };

        assert!(!policy.should_rotate(50, 500));
        assert!(policy.should_rotate(100, 500));
        assert!(policy.should_rotate(50, 1000));
    }

    #[test]
    fn test_rotation_policy_disabled() {
        let policy = RotationPolicy::disabled();
        assert!(!policy.should_rotate(1000000, 1_000_000_000));
    }

    #[test]
    fn test_parse_segment_info() {
        let (date, uuid) = parse_segment_info("causal_chain-2026-03-13-abc12345.jsonl").unwrap();
        assert_eq!(date, "2026-03-13");
        assert_eq!(uuid, "abc12345");
    }

    #[test]
    fn test_migrate_legacy_log() {
        let temp = tempdir().unwrap();
        let history_dir = temp.path().join("history");
        fs::create_dir_all(&history_dir).unwrap();

        let legacy_path = history_dir.join("causal_chain.jsonl");
        fs::write(&legacy_path, r#"{"timestamp":"2026-01-01T00:00:00Z","log_id":"test","actor_id":"a","session_id":"s","event_seq":1,"category":"c","action":"a","status":"SUCCESS","prev_hash":"genesis","entry_hash":"abc123"}"#).unwrap();

        let last_hash = migrate_legacy_log(&history_dir).unwrap();
        assert!(last_hash.is_some());
        assert!(legacy_path.exists() == false);
    }
}
