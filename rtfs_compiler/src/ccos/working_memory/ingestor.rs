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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ccos::working_memory::backend_inmemory::InMemoryJsonlBackend;
    use crate::ccos::working_memory::QueryParams;

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
}
