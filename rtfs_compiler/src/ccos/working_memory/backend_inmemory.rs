//! In-memory + JSONL Working Memory backend.
//!
//! Goals:
//! - Small, testable default backend with append/query/prune/load/flush.
//! - Rebuildable: JSONL append-only persistence for durability, simple load on start.
//! - Indices: id map, time index (BTreeMap), tag index (HashMap).
//!
//! Notes:
//! - Token estimation uses backend.approx_token_count(content) which can be overridden.
//! - JSONL format: one WorkingMemoryEntry per line as JSON.
//!
//! Unit tests are colocated at the bottom of this file.

use crate::ccos::working_memory::backend::{
    QueryParams, QueryResult, WorkingMemoryBackend, WorkingMemoryError,
};
use crate::ccos::working_memory::types::{WorkingMemoryEntry, WorkingMemoryId};
use std::collections::{BTreeMap, HashMap, HashSet};
use std::fs::{File, OpenOptions};
use std::io::{BufRead, BufReader, BufWriter, Write};
use std::path::{Path, PathBuf};

/// Default in-memory + JSONL backend.
///
/// Indexing structures:
/// - by_id: O(1) retrieval
/// - by_time: timestamp -> set of ids for time-window filtering and eviction
/// - by_tag: tag -> set of ids for OR tag filtering
pub struct InMemoryJsonlBackend {
    // Data
    by_id: HashMap<WorkingMemoryId, WorkingMemoryEntry>,
    by_time: BTreeMap<u64, HashSet<WorkingMemoryId>>,
    by_tag: HashMap<String, HashSet<WorkingMemoryId>>,

    // Persistence
    jsonl_path: Option<PathBuf>,

    // Budgets (for prune)
    max_entries_in_memory: Option<usize>,
    max_tokens_in_memory: Option<usize>,

    // Cached total tokens for fast budget checks
    total_tokens_cache: usize,
}

impl InMemoryJsonlBackend {
    /// Create a new backend instance.
    pub fn new<P: Into<Option<PathBuf>>>(
        jsonl_path: P,
        max_entries_in_memory: Option<usize>,
        max_tokens_in_memory: Option<usize>,
    ) -> Self {
        Self {
            by_id: HashMap::new(),
            by_time: BTreeMap::new(),
            by_tag: HashMap::new(),
            jsonl_path: jsonl_path.into(),
            max_entries_in_memory,
            max_tokens_in_memory,
            total_tokens_cache: 0,
        }
    }

    fn index_insert(&mut self, entry: &WorkingMemoryEntry) {
        // by_time
        self.by_time
            .entry(entry.timestamp_s)
            .or_insert_with(HashSet::new)
            .insert(entry.id.clone());
        // by_tag
        for tag in &entry.tags {
            self.by_tag
                .entry(tag.clone())
                .or_insert_with(HashSet::new)
                .insert(entry.id.clone());
        }
        // cache tokens
        self.total_tokens_cache += entry.approx_tokens;
    }

    fn index_remove(&mut self, entry: &WorkingMemoryEntry) {
        if let Some(set) = self.by_time.get_mut(&entry.timestamp_s) {
            set.remove(&entry.id);
            if set.is_empty() {
                self.by_time.remove(&entry.timestamp_s);
            }
        }
        for tag in &entry.tags {
            if let Some(set) = self.by_tag.get_mut(tag) {
                set.remove(&entry.id);
                if set.is_empty() {
                    self.by_tag.remove(tag);
                }
            }
        }
        if entry.approx_tokens <= self.total_tokens_cache {
            self.total_tokens_cache -= entry.approx_tokens;
        } else {
            self.total_tokens_cache = 0;
        }
    }

    fn append_jsonl(&self, entry: &WorkingMemoryEntry) -> Result<(), WorkingMemoryError> {
        if let Some(path) = &self.jsonl_path {
            let file = OpenOptions::new().create(true).append(true).open(path)?;
            let mut writer = BufWriter::new(file);
            let line = serde_json::to_string(entry)?;
            writer.write_all(line.as_bytes())?;
            writer.write_all(b"\n")?;
            writer.flush()?;
        }
        Ok(())
    }

    fn load_jsonl_into_memory(&mut self, path: &Path) -> Result<(), WorkingMemoryError> {
        if !path.exists() {
            return Ok(()); // nothing to load
        }
        let file = File::open(path)?;
        let reader = BufReader::new(file);
        for line in reader.lines() {
            let line = line?;
            if line.trim().is_empty() {
                continue;
            }
            let entry: WorkingMemoryEntry = serde_json::from_str(&line)?;
            self.total_tokens_cache += entry.approx_tokens;
            self.by_id.insert(entry.id.clone(), entry.clone());
            self.index_insert(&entry);
        }
        Ok(())
    }

    fn enforce_budgets(&mut self) -> Result<(), WorkingMemoryError> {
        loop {
            let over_entries = self
                .max_entries_in_memory
                .map(|m| self.by_id.len() > m)
                .unwrap_or(false);
            let over_tokens = self
                .max_tokens_in_memory
                .map(|m| self.total_tokens_cache > m)
                .unwrap_or(false);

            if !(over_entries || over_tokens) {
                break;
            }

            // Evict oldest first using by_time
            let oldest_ts = match self.by_time.keys().next().cloned() {
                Some(ts) => ts,
                None => break,
            };
            let ids = match self.by_time.get_mut(&oldest_ts) {
                Some(set) if !set.is_empty() => {
                    // remove one id from the set
                    let id = set.iter().next().cloned().unwrap();
                    set.remove(&id);
                    if set.is_empty() {
                        self.by_time.remove(&oldest_ts);
                    }
                    id
                }
                _ => {
                    self.by_time.remove(&oldest_ts);
                    continue;
                }
            };

            if let Some(entry) = self.by_id.remove(&ids) {
                self.index_remove(&entry);
            }
        }
        Ok(())
    }
}

impl WorkingMemoryBackend for InMemoryJsonlBackend {
    fn append(&mut self, entry: WorkingMemoryEntry) -> Result<(), WorkingMemoryError> {
        // overwrite behavior: replace existing id if present
        if let Some(old) = self.by_id.insert(entry.id.clone(), entry.clone()) {
            self.index_remove(&old);
        }
        self.index_insert(&entry);
        self.append_jsonl(&entry)?;
        self.enforce_budgets()?;
        Ok(())
    }

    fn get(&self, id: &WorkingMemoryId) -> Result<Option<WorkingMemoryEntry>, WorkingMemoryError> {
        Ok(self.by_id.get(id).cloned())
    }

    fn query(&self, params: &QueryParams) -> Result<QueryResult, WorkingMemoryError> {
        // 1) Collect candidate ids by tags OR semantics
        let mut candidate_ids: Option<HashSet<WorkingMemoryId>> = None;

        if params.tags_any.is_empty() {
            // no tag filter means all ids considered
            let all: HashSet<_> = self.by_id.keys().cloned().collect();
            candidate_ids = Some(all);
        } else {
            for tag in &params.tags_any {
                if let Some(set) = self.by_tag.get(tag) {
                    let set_clone: HashSet<_> = set.iter().cloned().collect();
                    candidate_ids = Some(match candidate_ids {
                        None => set_clone,
                        Some(mut acc) => {
                            acc.extend(set_clone);
                            acc
                        }
                    });
                }
            }
            if candidate_ids.is_none() {
                return Ok(QueryResult { entries: vec![] });
            }
        }

        // 2) Apply time window filter
        let (from, to) = (
            params.from_ts_s.unwrap_or(0),
            params.to_ts_s.unwrap_or(u64::MAX),
        );
        let mut filtered: Vec<WorkingMemoryEntry> = candidate_ids
            .unwrap()
            .into_iter()
            .filter_map(|id| self.by_id.get(&id).cloned())
            .filter(|e| e.timestamp_s >= from && e.timestamp_s <= to)
            .collect();

        // 3) Sort by recency desc
        filtered.sort_by(|a, b| b.timestamp_s.cmp(&a.timestamp_s));

        // 4) Apply limit
        if let Some(limit) = params.limit {
            filtered.truncate(limit);
        }

        Ok(QueryResult { entries: filtered })
    }

    fn prune(
        &mut self,
        max_entries: Option<usize>,
        max_tokens: Option<usize>,
    ) -> Result<(), WorkingMemoryError> {
        if max_entries.is_some() {
            self.max_entries_in_memory = max_entries;
        }
        if max_tokens.is_some() {
            self.max_tokens_in_memory = max_tokens;
        }
        self.enforce_budgets()
    }

    fn load(&mut self) -> Result<(), WorkingMemoryError> {
        // Take ownership of the path first to avoid overlapping borrows.
        let path_opt = self.jsonl_path.clone();
        if let Some(path_buf) = path_opt {
            // Ensure file exists if persistence is requested
            if !path_buf.exists() {
                let _ = OpenOptions::new()
                    .create(true)
                    .append(true)
                    .open(&path_buf)?;
            }

            // Build fresh structures off to the side to avoid partial state on failure
            let mut new_by_id: HashMap<WorkingMemoryId, WorkingMemoryEntry> = HashMap::new();
            let mut new_by_time: BTreeMap<u64, HashSet<WorkingMemoryId>> = BTreeMap::new();
            let mut new_by_tag: HashMap<String, HashSet<WorkingMemoryId>> = HashMap::new();
            let mut new_total_tokens: usize = 0;

            // Load using a temporary backend that indexes into the new structures
            // Reuse the loader by creating a lightweight helper
            {
                // Local reader path
                if path_buf.exists() {
                    let file = File::open(&path_buf)?;
                    let reader = BufReader::new(file);
                    for line in reader.lines() {
                        let line = line?;
                        if line.trim().is_empty() {
                            continue;
                        }
                        let entry: WorkingMemoryEntry = serde_json::from_str(&line)?;
                        new_total_tokens += entry.approx_tokens;

                        // Insert into temp maps
                        let id = entry.id.clone();
                        // by_id
                        new_by_id.insert(id.clone(), entry.clone());
                        // by_time
                        new_by_time
                            .entry(entry.timestamp_s)
                            .or_insert_with(HashSet::new)
                            .insert(id.clone());
                        // by_tag
                        for tag in &entry.tags {
                            new_by_tag
                                .entry(tag.clone())
                                .or_insert_with(HashSet::new)
                                .insert(id.clone());
                        }
                    }
                }
            }

            // Commit new state atomically
            self.by_id = new_by_id;
            self.by_time = new_by_time;
            self.by_tag = new_by_tag;
            self.total_tokens_cache = new_total_tokens;
        }
        Ok(())
    }

    fn flush(&mut self) -> Result<(), WorkingMemoryError> {
        // JSONL is append-only; nothing to do for flush beyond ensuring file exists.
        if let Some(path) = &self.jsonl_path {
            if !path.exists() {
                let _ = OpenOptions::new().create(true).append(true).open(path)?;
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ccos::working_memory::types::{WorkingMemoryEntry, WorkingMemoryMeta};
    use std::fs;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn now_s() -> u64 {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs()
    }

    fn temp_path(name: &str) -> PathBuf {
        let mut p = std::env::temp_dir();
        p.push(format!("wm_test_{}_{}.jsonl", name, now_s()));
        p
    }

    fn mk_entry(id: &str, ts: u64, tags: &[&str]) -> WorkingMemoryEntry {
        WorkingMemoryEntry {
            id: id.to_string(),
            title: format!("title-{}", id),
            content: format!("content-{}", id),
            tags: tags.iter().map(|s| s.to_string()).collect(),
            timestamp_s: ts,
            approx_tokens: 4,
            meta: WorkingMemoryMeta::default(),
        }
    }

    #[test]
    fn test_append_query_prune_load() {
        let path = temp_path("append_query");
        let mut backend = InMemoryJsonlBackend::new(Some(path.clone()), Some(3), Some(20));

        let t0 = now_s();
        backend.append(mk_entry("a", t0 - 30, &["wisdom"])).unwrap();
        backend
            .append(mk_entry("b", t0 - 20, &["wisdom", "x"]))
            .unwrap();
        backend.append(mk_entry("c", t0 - 10, &["y"])).unwrap();

        // Query by tag OR
        let qp = QueryParams::with_tags(["wisdom"]).with_limit(Some(10));
        let res = backend.query(&qp).unwrap();
        assert_eq!(res.entries.len(), 2);
        assert_eq!(res.entries[0].id, "b"); // most recent first
        assert_eq!(res.entries[1].id, "a");

        // Budget prune by entries: add one more triggers eviction of oldest ("a")
        backend.append(mk_entry("d", t0 - 5, &["wisdom"])).unwrap();

        let res2 = backend
            .query(&QueryParams::with_tags(["wisdom"]).with_limit(Some(10)))
            .unwrap();
        let ids: Vec<_> = res2.entries.iter().map(|e| e.id.as_str()).collect();
        assert!(ids.contains(&"b"));
        assert!(ids.contains(&"d"));
        assert!(!ids.contains(&"a")); // evicted

        // Reload from disk and ensure we can read at least what was persisted
        let mut backend2 = InMemoryJsonlBackend::new(Some(path.clone()), Some(10), Some(1000));
        backend2.load().unwrap();
        let res3 = backend2.query(&QueryParams::default()).unwrap();
        assert!(!res3.entries.is_empty());

        // Cleanup
        let _ = fs::remove_file(path);
    }

    #[test]
    fn test_time_window_filtering_and_limit() {
        let mut backend = InMemoryJsonlBackend::new(None, None, None);
        let base = now_s();

        backend.append(mk_entry("a", base - 30, &["w"])).unwrap();
        backend.append(mk_entry("b", base - 20, &["w"])).unwrap();
        backend.append(mk_entry("c", base - 10, &["w"])).unwrap();

        // Time window [base-25, base-5] should return b and c
        let qp = QueryParams::with_tags(["w"]).with_time_window(Some(base - 25), Some(base - 5));
        let res = backend.query(&qp).unwrap();
        let ids: Vec<_> = res.entries.iter().map(|e| e.id.as_str()).collect();
        assert_eq!(ids, vec!["c", "b"]); // recency desc

        // Limit to 1
        let qp2 = QueryParams {
            limit: Some(1),
            ..qp
        };
        let res2 = backend.query(&qp2).unwrap();
        assert_eq!(res2.entries.len(), 1);
        assert_eq!(res2.entries[0].id, "c");
    }
}
