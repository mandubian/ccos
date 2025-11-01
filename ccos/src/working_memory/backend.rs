//! Backend trait and error types for Working Memory.
//!
//! Responsibilities:
//! - Define a minimal storage-agnostic API for Working Memory operations.
//! - Keep interfaces small and focused for easier testing and alternate backends.
//!
//! Unit tests at the bottom validate trait object usage and basic QueryParams behavior.

use crate::working_memory::types::{WorkingMemoryEntry, WorkingMemoryId};
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use thiserror::Error;

/// Parameters for querying Working Memory backends.
///
/// Semantics:
/// - tags_any: OR semantics across provided tags. Empty means no tag filtering.
/// - from_ts_s/to_ts_s: inclusive time window. None means unbounded on that side.
/// - limit: max number of entries to return (backend may return fewer).
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct QueryParams {
    pub tags_any: HashSet<String>,
    pub from_ts_s: Option<u64>,
    pub to_ts_s: Option<u64>,
    pub limit: Option<usize>,
}

impl QueryParams {
    pub fn with_tags<I, T>(tags: I) -> Self
    where
        I: IntoIterator<Item = T>,
        T: Into<String>,
    {
        let mut qp = Self::default();
        qp.tags_any = tags.into_iter().map(|t| t.into()).collect();
        qp
    }

    pub fn with_time_window(mut self, from_ts_s: Option<u64>, to_ts_s: Option<u64>) -> Self {
        self.from_ts_s = from_ts_s;
        self.to_ts_s = to_ts_s;
        self
    }

    pub fn with_limit(mut self, limit: Option<usize>) -> Self {
        self.limit = limit;
        self
    }
}

/// Result container for queries. Entries are typically sorted by recency (desc).
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct QueryResult {
    pub entries: Vec<WorkingMemoryEntry>,
}

/// Error type for Working Memory backends.
#[derive(Debug, Error)]
pub enum WorkingMemoryError {
    #[error("IO error: {0}")]
    Io(String),
    #[error("Serialization error: {0}")]
    Serde(String),
    #[error("Invalid data: {0}")]
    Invalid(String),
    #[error("Not implemented: {0}")]
    NotImplemented(String),
    #[error("Other: {0}")]
    Other(String),
}

impl From<std::io::Error> for WorkingMemoryError {
    fn from(e: std::io::Error) -> Self {
        WorkingMemoryError::Io(e.to_string())
    }
}
impl From<serde_json::Error> for WorkingMemoryError {
    fn from(e: serde_json::Error) -> Self {
        WorkingMemoryError::Serde(e.to_string())
    }
}

/// Minimal storage-agnostic backend API.
///
/// Notes:
/// - Backends must be Send + Sync to allow concurrent access behind Arcs.
/// - approx_token_count: allows backends to expose their own estimator if needed.
pub trait WorkingMemoryBackend: Send + Sync {
    fn append(&mut self, entry: WorkingMemoryEntry) -> Result<(), WorkingMemoryError>;
    fn get(&self, id: &WorkingMemoryId) -> Result<Option<WorkingMemoryEntry>, WorkingMemoryError>;
    fn query(&self, params: &QueryParams) -> Result<QueryResult, WorkingMemoryError>;
    fn prune(
        &mut self,
        max_entries: Option<usize>,
        max_tokens: Option<usize>,
    ) -> Result<(), WorkingMemoryError>;
    fn load(&mut self) -> Result<(), WorkingMemoryError>;
    fn flush(&mut self) -> Result<(), WorkingMemoryError>;
    fn approx_token_count(&self, content: &str) -> usize {
        // Default heuristic; backends can override.
        (content.len() / 4).max(1)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct NoopBackend;
    impl WorkingMemoryBackend for NoopBackend {
        fn append(&mut self, _entry: WorkingMemoryEntry) -> Result<(), WorkingMemoryError> {
            Ok(())
        }
        fn get(
            &self,
            _id: &WorkingMemoryId,
        ) -> Result<Option<WorkingMemoryEntry>, WorkingMemoryError> {
            Ok(None)
        }
        fn query(&self, _params: &QueryParams) -> Result<QueryResult, WorkingMemoryError> {
            Ok(QueryResult { entries: vec![] })
        }
        fn prune(
            &mut self,
            _max_entries: Option<usize>,
            _max_tokens: Option<usize>,
        ) -> Result<(), WorkingMemoryError> {
            Ok(())
        }
        fn load(&mut self) -> Result<(), WorkingMemoryError> {
            Ok(())
        }
        fn flush(&mut self) -> Result<(), WorkingMemoryError> {
            Ok(())
        }
    }

    #[test]
    fn test_query_params_builders() {
        let qp = QueryParams::with_tags(["a", "b"])
            .with_time_window(Some(10), Some(20))
            .with_limit(Some(5));

        assert!(qp.tags_any.contains("a"));
        assert!(qp.tags_any.contains("b"));
        assert_eq!(qp.from_ts_s, Some(10));
        assert_eq!(qp.to_ts_s, Some(20));
        assert_eq!(qp.limit, Some(5));
    }

    #[test]
    fn test_trait_object_usage() {
        let mut backend: Box<dyn WorkingMemoryBackend> = Box::new(NoopBackend);
        // basic calls should not panic
        backend.load().unwrap();
        backend.flush().unwrap();
        let res = backend.query(&QueryParams::default()).unwrap();
        assert!(res.entries.is_empty());
    }
}
