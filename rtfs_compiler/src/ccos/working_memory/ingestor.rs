//! Memory Ingestor skeleton
//!
//! Purpose:
//! - Subscribe to Causal Chain append events and derive Working Memory entries asynchronously.
//! - Provide a rebuild (replay) path to reconstruct Working Memory from the ledger.
//!
//! Notes:
//! - This module intentionally provides a minimal skeleton so it stays small and focused.
//! - Derivation logic is stubbed with simple heuristics and intended to be extended.
//!
//! Unit tests at the bottom validate derivation helpers and idempotency by content hash.

use crate::ccos::working_memory::backend::WorkingMemoryError;
use crate::ccos::working_memory::facade::WorkingMemory;
use crate::ccos::working_memory::types::{WorkingMemoryEntry, WorkingMemoryId, WorkingMemoryMeta};
use crate::ccos::event_sink::CausalChainEventSink;
use crate::ccos::types::Action;
use std::sync::{Arc, Mutex};
use std::collections::HashSet;

/// Minimal action-like record used by the ingestor to derive entries.
/// In real integration, this should map to the Causal Chain Action.
#[derive(Debug, Clone)]
pub struct ActionRecord {
    pub action_id: String,
    pub kind: String,           // e.g., "PlanStarted", "StepCompleted", "CapabilityCall"
    pub provider: Option<String>,
    pub timestamp_s: u64,
    pub summary: String,        // short human-readable summary/title
    pub content: String,        // compact payload or details
    pub plan_id: Option<String>,
    pub intent_id: Option<String>,
    pub step_id: Option<String>,
    pub attestation_hash: Option<String>,
    // Optional explicit content hash; if None, derive from content
    pub content_hash: Option<String>,
}

/// Derived entry plus computed id used for ingestion.
#[derive(Debug, Clone)]
pub struct DerivedEntry {
    pub id: WorkingMemoryId,
    pub entry: WorkingMemoryEntry,
}

pub struct MemoryIngestor;

impl MemoryIngestor {
    /// Derive a WorkingMemoryEntry from an ActionRecord with sane defaults.
    /// Generates:
    /// - tags from action type + "causal-chain" + "distillation"
    /// - meta fields with provenance
    /// - approx_tokens via entry helper
    pub fn derive_entries_from_action(action: &ActionRecord) -> Vec<DerivedEntry> {
        let mut tags: HashSet<String> = HashSet::new();
        tags.insert("causal-chain".to_string());
        tags.insert("distillation".to_string());
        tags.insert("wisdom".to_string());
        tags.insert(action.kind.to_lowercase());

        if let Some(p) = &action.provider {
            tags.insert(p.clone());
        }

        let content_hash = action
            .content_hash
            .clone()
            .unwrap_or_else(|| Self::simple_content_hash(&action.content));

        let meta = WorkingMemoryMeta {
            action_id: Some(action.action_id.clone()),
            plan_id: action.plan_id.clone(),
            intent_id: action.intent_id.clone(),
            step_id: action.step_id.clone(),
            provider: action.provider.clone(),
            attestation_hash: action.attestation_hash.clone(),
            content_hash: Some(content_hash.clone()),
            extra: Default::default(),
        };

        // Compute deterministic id using action id + content hash to allow idempotency
        let id = format!("wm:{}:{}", action.action_id, content_hash);

        let entry = WorkingMemoryEntry::new_with_estimate(
            id.clone(),
            action.summary.clone(),
            action.content.clone(),
            tags.into_iter(),
            action.timestamp_s,
            meta,
        );

        vec![DerivedEntry { id, entry }]
    }

    /// Ingest a single action into the working memory, idempotently.
    /// If an entry with the same id already exists, it will be overwritten with identical data.
    pub fn ingest_action(wm: &mut WorkingMemory, action: &ActionRecord) -> Result<(), WorkingMemoryError> {
        let derived = Self::derive_entries_from_action(action);
        for d in derived {
            wm.append(d.entry)?;
        }
        Ok(())
    }

    /// Replay a batch of actions (e.g., from Causal Chain genesis) to rebuild Working Memory.
    pub fn replay_all(wm: &mut WorkingMemory, actions: &[ActionRecord]) -> Result<(), WorkingMemoryError> {
        for a in actions {
            Self::ingest_action(wm, a)?;
        }
        Ok(())
    }

    /// Very small non-cryptographic content hash for idempotency.
    /// Replace with a cryptographic hash (e.g., SHA-256) for production.
    fn simple_content_hash(s: &str) -> String {
        // Fowler–Noll–Vo (FNV-1a) like simple hash for determinism, not security.
        let mut hash: u64 = 0xcbf29ce484222325;
        for b in s.as_bytes() {
            hash ^= *b as u64;
            hash = hash.wrapping_mul(0x100000001b3);
        }
        format!("{:016x}", hash)
    }
}

/// Event-sink adapter that ingests Causal Chain actions into Working Memory.
/// Keep this sink lightweight: it locks briefly and performs a small append.
pub struct WorkingMemorySink {
    wm: Arc<Mutex<WorkingMemory>>, // guarded for thread-safety; keep lock scope minimal
}

impl std::fmt::Debug for WorkingMemorySink {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("WorkingMemorySink").finish()
    }
}

impl WorkingMemorySink {
    pub fn new(wm: Arc<Mutex<WorkingMemory>>) -> Self {
        Self { wm }
    }

    fn map_action(action: &Action) -> ActionRecord {
        // Prefer function_name when present, otherwise use the action type.
        let summary = action
            .function_name
            .clone()
            .unwrap_or_else(|| format!("{:?}", action.action_type));

        // Compact content string capturing key details without serialization deps.
        let mut content = String::new();
        content.push_str(&format!(
            "type={:?}; plan={}; intent={}; ts={}",
            action.action_type, action.plan_id, action.intent_id, action.timestamp
        ));
        if let Some(fn_name) = &action.function_name {
            content.push_str(&format!("; fn={}", fn_name));
        }
        if let Some(args) = &action.arguments {
            content.push_str(&format!("; args={}", args.len()));
        }
        if let Some(cost) = action.cost {
            content.push_str(&format!("; cost={}", cost));
        }
        if let Some(d) = action.duration_ms {
            content.push_str(&format!("; dur_ms={}", d));
        }

        ActionRecord {
            action_id: action.action_id.clone(),
            kind: format!("{:?}", action.action_type),
            provider: action.function_name.clone(),
            timestamp_s: action.timestamp, // note: upstream may use ms; WM treats as opaque
            summary,
            content,
            plan_id: Some(action.plan_id.clone()),
            intent_id: Some(action.intent_id.clone()),
            step_id: None,
            attestation_hash: action
                .metadata
                .get("signature")
                .and_then(|v| match v { crate::runtime::values::Value::String(s) => Some(s.clone()), _ => None }),
            content_hash: None,
        }
    }
}

impl CausalChainEventSink for WorkingMemorySink {
    fn on_action_appended(&self, action: &Action) {
        // Map the action and ingest; ignore ingestion errors to avoid blocking ledger writes.
        let record = Self::map_action(action);
        if let Ok(mut guard) = self.wm.lock() {
            let _ = MemoryIngestor::ingest_action(&mut *guard, &record);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ccos::working_memory::backend_inmemory::InMemoryJsonlBackend;
    use crate::ccos::working_memory::backend::QueryParams;
    use crate::ccos::causal_chain::CausalChain;
    use crate::ccos::types::{Intent, ActionType};

    fn mk_action(id: &str, ts: u64, kind: &str, content: &str) -> ActionRecord {
        ActionRecord {
            action_id: id.to_string(),
            kind: kind.to_string(),
            provider: Some("demo.provider:v1".to_string()),
            timestamp_s: ts,
            summary: format!("summary-{}", id),
            content: content.to_string(),
            plan_id: Some("plan-1".to_string()),
            intent_id: Some("intent-1".to_string()),
            step_id: Some("step-1".to_string()),
            attestation_hash: None,
            content_hash: None,
        }
    }

    #[test]
    fn test_simple_hash_stability() {
        let h1 = MemoryIngestor::simple_content_hash("abc");
        let h2 = MemoryIngestor::simple_content_hash("abc");
        assert_eq!(h1, h2);
        let h3 = MemoryIngestor::simple_content_hash("abcd");
        assert_ne!(h1, h3);
    }

    #[test]
    fn test_derivation_includes_tags_and_meta() {
        let a = mk_action("a1", 100, "CapabilityCall", "payload");
        let derived = MemoryIngestor::derive_entries_from_action(&a);
        assert_eq!(derived.len(), 1);
        let d = &derived[0];
        assert!(d.entry.tags.contains("causal-chain"));
        assert!(d.entry.tags.contains("distillation"));
        assert!(d.entry.tags.contains("wisdom"));
        assert!(d.entry.tags.contains("capabilitycall")); // lowercased kind
        assert_eq!(d.entry.meta.action_id.as_deref(), Some("a1"));
        assert_eq!(d.entry.meta.plan_id.as_deref(), Some("plan-1"));
        assert!(d.entry.meta.content_hash.is_some());
        assert!(d.entry.approx_tokens >= 1);
    }

    #[test]
    fn test_ingest_and_replay_idempotency() {
        let backend = InMemoryJsonlBackend::new(None, Some(10), Some(10_000));
        let mut wm = WorkingMemory::new(Box::new(backend));

        let a1 = mk_action("a1", 100, "PlanStarted", "c1");
        let a2 = mk_action("a1", 101, "PlanStarted", "c1"); // same content -> same id
        let a3 = mk_action("a2", 102, "StepCompleted", "c2");

        MemoryIngestor::ingest_action(&mut wm, &a1).unwrap();
        MemoryIngestor::ingest_action(&mut wm, &a2).unwrap(); // overwrites same id
        MemoryIngestor::ingest_action(&mut wm, &a3).unwrap();

        let res = wm.query(&QueryParams::default()).unwrap();
        assert_eq!(res.entries.len(), 2);

        // Replaying all should still end up with 2 entries
        MemoryIngestor::replay_all(&mut wm, &[a1.clone(), a2.clone(), a3.clone()]).unwrap();
        let res2 = wm.query(&QueryParams::default()).unwrap();
        assert_eq!(res2.entries.len(), 2);
    }

    #[test]
    fn test_wm_sink_receives_actions_from_causal_chain() {
        // Prepare WM + sink
        let backend = InMemoryJsonlBackend::new(None, Some(100), Some(10_000));
        let wm = WorkingMemory::new(Box::new(backend));
        let wm_arc = Arc::new(Mutex::new(wm));
        let sink = WorkingMemorySink::new(wm_arc.clone());

        // Prepare CausalChain and register sink
        let mut chain = CausalChain::new().unwrap();
        let sink_arc: Arc<dyn CausalChainEventSink> = Arc::new(sink);
        chain.register_event_sink(sink_arc);

        // Append a couple of actions via lifecycle helpers
        let intent = Intent::new("WM sink goal".to_string());
        let a = chain.create_action(intent.clone(), None).unwrap();
        // Record a result to trigger append + notification
        let result = crate::ccos::types::ExecutionResult { success: true, value: crate::runtime::values::Value::Nil, metadata: Default::default() };
        chain.record_result(a.clone(), result).unwrap();

        // Also log a plan lifecycle event, which also notifies sinks
        chain.log_plan_event(&a.plan_id.clone(), &a.intent_id.clone(), ActionType::PlanStarted).unwrap();

        // Inspect WM
        let guard = wm_arc.lock().unwrap();
        let res = guard.query(&QueryParams::default()).unwrap();
        assert!(res.entries.len() >= 2);
        // Ensure tags and content exist
        let first = &res.entries[0];
        assert!(first.tags.contains("causal-chain"));
        assert!(!first.content.is_empty());
    }
}
