//! Working Memory facade wrapping a pluggable backend.
//!
//! Purpose:
//! - Provide a stable, simple API for CCOS components (e.g., Context Horizon).
//! - Hide backend details behind a small surface (append/get/query/prune/load/flush).
//!
//! Notes:
//! - Keep this file small; heavy logic remains in backends.
//! - Unit tests cover basic flows and error propagation.

use crate::ccos::working_memory::backend::{
    QueryParams, QueryResult, WorkingMemoryBackend, WorkingMemoryError,
};
use crate::ccos::working_memory::types::{WorkingMemoryEntry, WorkingMemoryId};

/// Thin facade over a boxed WorkingMemoryBackend implementation.
pub struct WorkingMemory {
    backend: Box<dyn WorkingMemoryBackend>,
}

impl WorkingMemory {
    /// Create a new facade from any backend.
    pub fn new(backend: Box<dyn WorkingMemoryBackend>) -> Self {
        Self { backend }
    }

    /// Append or replace an entry.
    pub fn append(&mut self, entry: WorkingMemoryEntry) -> Result<(), WorkingMemoryError> {
        self.backend.append(entry)
    }

    /// Get an entry by id.
    pub fn get(
        &self,
        id: &WorkingMemoryId,
    ) -> Result<Option<WorkingMemoryEntry>, WorkingMemoryError> {
        self.backend.get(id)
    }

    /// Query entries by tags/time/limit.
    pub fn query(&self, params: &QueryParams) -> Result<QueryResult, WorkingMemoryError> {
        self.backend.query(params)
    }

    /// Enforce budgets by pruning oldest entries until constraints are met.
    pub fn prune(
        &mut self,
        max_entries: Option<usize>,
        max_tokens: Option<usize>,
    ) -> Result<(), WorkingMemoryError> {
        self.backend.prune(max_entries, max_tokens)
    }

    /// Load entries from persistent storage (if supported).
    pub fn load(&mut self) -> Result<(), WorkingMemoryError> {
        self.backend.load()
    }

    /// Flush state to persistent storage (if supported).
    pub fn flush(&mut self) -> Result<(), WorkingMemoryError> {
        self.backend.flush()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ccos::working_memory::backend_inmemory::InMemoryJsonlBackend;
    use crate::ccos::working_memory::types::{WorkingMemoryEntry, WorkingMemoryMeta};
    use std::time::{SystemTime, UNIX_EPOCH};

    fn now_s() -> u64 {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs()
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
    fn test_facade_basic_flow() {
        let backend = InMemoryJsonlBackend::new(None, Some(10), Some(1000));
        let mut wm = WorkingMemory::new(Box::new(backend));

        let t0 = now_s();
        wm.append(mk_entry("a", t0 - 2, &["w", "x"])).unwrap();
        wm.append(mk_entry("b", t0 - 1, &["w"])).unwrap();

        let res = wm
            .query(&QueryParams::with_tags(["w"]).with_limit(Some(10)))
            .unwrap();
        let ids: Vec<_> = res.entries.iter().map(|e| e.id.as_str()).collect();
        assert_eq!(ids, vec!["b", "a"]);

        let got = wm.get(&"a".to_string()).unwrap().unwrap();
        assert_eq!(got.id, "a");
    }

    #[test]
    fn test_facade_prune() {
        let backend = InMemoryJsonlBackend::new(None, Some(1), Some(1000));
        let mut wm = WorkingMemory::new(Box::new(backend));

        let t0 = now_s();
        wm.append(mk_entry("a", t0 - 3, &["w"])).unwrap();
        wm.append(mk_entry("b", t0 - 2, &["w"])).unwrap();
        wm.append(mk_entry("c", t0 - 1, &["w"])).unwrap();

        // after appends with budget=1, only most recent should remain
        let res = wm
            .query(&QueryParams::with_tags(["w"]).with_limit(Some(10)))
            .unwrap();
        let ids: Vec<_> = res.entries.iter().map(|e| e.id.as_str()).collect();
        assert_eq!(ids, vec!["c"]);
    }
}
