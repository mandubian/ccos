//! Governed instruction resources (URLs/text/files) for chat autonomy.
//!
//! A "resource" is an untrusted instruction/data artifact (e.g. skill.md, prompt text, docs).
//! The gateway stores resource content in the quarantine store and persists minimal provenance
//! metadata to the causal chain for audit + restart-safe rebuild.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use crate::types::Action;

/// Thread-safe resource store wrapper.
pub type SharedResourceStore = Arc<Mutex<ResourceStore>>;

pub fn new_shared_resource_store() -> SharedResourceStore {
    Arc::new(Mutex::new(ResourceStore::new()))
}

#[derive(Debug, Clone)]
pub struct ResourceRecord {
    pub id: String,
    pub pointer_id: String,
    pub source: String,
    pub content_type: String,
    pub sha256: String,
    pub size_bytes: u64,
    pub created_at_ms: u64,
    pub session_id: Option<String>,
    pub run_id: Option<String>,
    pub step_id: Option<String>,
    pub label: Option<String>,
}

#[derive(Debug, Default)]
pub struct ResourceStore {
    resources: HashMap<String, ResourceRecord>,
    by_session: HashMap<String, Vec<String>>,
    by_run: HashMap<String, Vec<String>>,
}

impl ResourceStore {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn upsert(&mut self, record: ResourceRecord) {
        let id = record.id.clone();
        if let Some(session_id) = record.session_id.clone() {
            self.by_session.entry(session_id).or_default().push(id.clone());
        }
        if let Some(run_id) = record.run_id.clone() {
            self.by_run.entry(run_id).or_default().push(id.clone());
        }
        self.resources.insert(id, record);
    }

    pub fn get(&self, id: &str) -> Option<&ResourceRecord> {
        self.resources.get(id)
    }

    pub fn list_for_session(&self, session_id: &str) -> Vec<ResourceRecord> {
        self.by_session
            .get(session_id)
            .into_iter()
            .flat_map(|ids| ids.iter())
            .filter_map(|id| self.resources.get(id).cloned())
            .collect()
    }

    pub fn list_for_run(&self, run_id: &str) -> Vec<ResourceRecord> {
        self.by_run
            .get(run_id)
            .into_iter()
            .flat_map(|ids| ids.iter())
            .filter_map(|id| self.resources.get(id).cloned())
            .collect()
    }

    pub fn rebuild_from_chain(actions: &[Action]) -> Self {
        let mut store = Self::new();
        for a in actions {
            let Some(fn_name) = &a.function_name else {
                continue;
            };
            if fn_name != "chat.audit.resource.ingest" {
                continue;
            }
            let meta = &a.metadata;
            let get_str = |k: &str| meta.get(k).and_then(|v| v.as_string()).map(|s| s.to_string());
            let get_u64 = |k: &str| {
                meta.get(k).and_then(|v| match v {
                    rtfs::runtime::values::Value::Integer(i) if *i >= 0 => Some(*i as u64),
                    rtfs::runtime::values::Value::String(s) => s.parse::<u64>().ok(),
                    _ => None,
                })
            };

            let Some(resource_id) = get_str("resource_id") else {
                continue;
            };
            let Some(pointer_id) = get_str("pointer_id") else {
                continue;
            };
            let source = get_str("source").unwrap_or_else(|| "unknown".to_string());
            let content_type = get_str("content_type").unwrap_or_else(|| "text/plain".to_string());
            let sha256 = get_str("sha256").unwrap_or_else(|| "unknown".to_string());
            let size_bytes = get_u64("size_bytes").unwrap_or(0);
            let created_at_ms = get_u64("created_at_ms").unwrap_or(a.timestamp.saturating_mul(1000));

            store.upsert(ResourceRecord {
                id: resource_id,
                pointer_id,
                source,
                content_type,
                sha256,
                size_bytes,
                created_at_ms,
                session_id: get_str("session_id"),
                run_id: get_str("run_id"),
                step_id: get_str("step_id"),
                label: get_str("label"),
            });
        }
        store
    }
}

