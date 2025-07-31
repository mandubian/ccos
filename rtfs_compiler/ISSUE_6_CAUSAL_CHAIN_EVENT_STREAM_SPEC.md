# ISSUE #6 — Causal Chain Event Stream & Working Memory Ingestor Wiring (Specification)

Status: Proposal (no code changes in this PR)
Scope: rtfs_compiler crate, CCOS Rust implementation
Related CCOS/RTFS specs: SEP-003 (Causal Chain), SEP-009 (Context Horizon Boundaries), SEP-013 (Working Memory Ingestion), 014-step-special-form-design

## Background

Issue #6 covers modularizing Working Memory (WM) and wiring Context Horizon (CH) to retrieve distilled wisdom from WM. The WM module, backends, boundaries, and an ingestor skeleton (SEP-013) are implemented with unit tests. The missing piece is integrating Causal Chain append events into WM ingestion so CH can query distilled wisdom tagged with "wisdom", "causal-chain", and "distillation".

Current code facts:
- Causal Chain (src/ccos/causal_chain.rs) appends actions via various methods (record_result, log_plan_event, log_intent_created, etc.), updates metrics, and maintains provenance.
- WM ingestor (src/ccos/working_memory/ingestor.rs) exposes:
  - derive_entries_from_action(ActionRecord) → WorkingMemoryEntry
  - ingest_action / replay_all
  - Deterministic, non-crypto content hash for idempotency
- WM facade and default backend (in-memory + JSONL) are in place.

This document specifies a minimal, backwards-compatible event streaming mechanism for the Causal Chain and a WM ingestion sink that maps `types::Action` to the ingestor's `ActionRecord`, without altering existing behavior for consumers that do not register any sink.

## Goals

1. Add an explicit registration API to observe Causal Chain append events at runtime.
2. Enable WM to ingest appended actions online (idempotently) and rebuild via replay.
3. Keep changes minimal and non-breaking; zero behavior change if no sink is registered.
4. Preserve deterministic, append-only ledger semantics and integrity verification.
5. Align with CCOS security/audit principles (attestation, provenance, immutable audit).

## Non-Goals

- No change to ledger persistence format, hashing, or integrity verification.
- No change to Action structure or existing log_* APIs beyond optional notifications.
- No inclusion of long-running/async logic inside Causal Chain’s critical sections.

## High-Level Design

Introduce a simple observer (sink) mechanism:
- Define trait `CausalChainEventSink { fn on_action_appended(&self, action: &Action) }`.
- Extend `CausalChain` with an optional list of sinks.
- After a successful append + metrics update, notify all sinks synchronously.
- If no sinks are registered, behavior remains unchanged.

Provide a WM ingestion sink that:
- Maps `ccos::types::Action` to `working_memory::ingestor::ActionRecord`.
- Uses `MemoryIngestor::ingest_action` to append into WM idempotently.
- Stays lightweight and non-blocking.

Offer a replay helper (external to Causal Chain) that:
- Iterates `CausalChain::get_all_actions()`
- Maps each to `ActionRecord`
- Calls `MemoryIngestor::replay_all` to rebuild WM from genesis

## Proposed API Changes (Causal Chain)

File: src/ccos/causal_chain.rs

1) New trait (public):
```rust
pub trait CausalChainEventSink {
    fn on_action_appended(&self, action: &Action);
}
```

2) Extend CausalChain state (non-breaking default):
```rust
#[derive(Debug)]
pub struct CausalChain {
    ledger: ImmutableLedger,
    signing: CryptographicSigning,
    provenance: ProvenanceTracker,
    metrics: PerformanceMetrics,
    event_sinks: Vec<Box<dyn CausalChainEventSink>>, // default empty
}
```

3) Registration and notification helpers:
```rust
impl CausalChain {
    pub fn register_event_sink(&mut self, sink: Box<dyn CausalChainEventSink>) {
        self.event_sinks.push(sink);
    }

    fn notify_sinks(&self, action: &Action) {
        for sink in &self.event_sinks {
            sink.on_action_appended(action);
        }
    }
}
```

4) Call `notify_sinks(action)` at the end of each path that appends and records metrics:
- record_result
- log_plan_event (and wrappers: log_plan_started, log_plan_aborted, log_plan_completed)
- log_intent_created
- log_intent_status_change
- log_intent_relationship_created
- log_intent_archived
- log_intent_reactivated
- log_capability_call
- append (general append path)

Ordering requirement: notify after ledger append and metrics record to ensure the action is fully committed before observers react.

Notes:
- If a sink panics or is slow, it must not compromise ledger integrity or locks. Sinks must be minimal and resilient; heavy work should be deferred by the sink itself.

## Working Memory Ingestion Sink (Adapter)

New file suggestion: src/ccos/working_memory/ingestion_sink.rs (or similar)

Purpose: Bridge Causal Chain actions to WM using the existing `MemoryIngestor` (SEP-013).

Sketch:
```rust
use std::sync::{Arc, Mutex};
use crate::ccos::types::{Action, ActionType};
use crate::ccos::working_memory::{WorkingMemory, MemoryIngestor};
use crate::ccos::working_memory::ingestor::ActionRecord;
use crate::ccos::causal_chain::CausalChainEventSink;
use crate::runtime::values::Value;

pub struct WmIngestionSink {
    wm: Arc<Mutex<WorkingMemory>>,
}

impl WmIngestionSink {
    pub fn new(wm: Arc<Mutex<WorkingMemory>>) -> Self { Self { wm } }

    fn map_action_to_record(action: &Action) -> ActionRecord {
        // Convert millis to seconds for WM ingestor
        let ts_s = action.timestamp / 1000;

        let kind = format!("{:?}", action.action_type);
        let provider = action.function_name.clone();

        // Extract attestation if present
        let attestation_hash = action.metadata.get("signature")
            .and_then(|v| if let Value::String(s) = v { Some(s.clone()) } else { None });

        // Compact content payload for idempotent hashing and human scan
        let args_str   = action.arguments.as_ref().map(|a| format!("{:?}", a)).unwrap_or_default();
        let result_str = action.result.as_ref().map(|r| format!("{:?}", r)).unwrap_or_default();
        let meta_str   = if action.metadata.is_empty() { String::new() } else { format!("{:?}", action.metadata) };
        let content    = format!("args={}; result={}; meta={}", args_str, result_str, meta_str);

        let summary = provider.clone().unwrap_or_else(|| kind.clone());

        ActionRecord {
            action_id: action.action_id.clone(),
            kind,
            provider,
            timestamp_s: ts_s,
            summary,
            content,
            plan_id: Some(action.plan_id.clone()),
            intent_id: Some(action.intent_id.clone()),
            step_id: None, // Optionally populate if a step id is available via metadata
            attestation_hash,
            content_hash: None, // Let MemoryIngestor compute deterministic hash
        }
    }
}

impl CausalChainEventSink for WmIngestionSink {
    fn on_action_appended(&self, action: &Action) {
        if let Ok(mut wm) = self.wm.lock() {
            let record = Self::map_action_to_record(action);
            let _ = MemoryIngestor::ingest_action(&mut wm, &record); // idempotent
        }
    }
}
```

Mapping rules:
- action_id → ActionRecord.action_id
- action_type (enum) → kind (String) via `format!("{:?}", ...)`
- function_name → provider (Some) else None
- timestamp (ms) → timestamp_s = ms / 1000
- summary = provider or kind
- content = compact string with args/result/meta (sufficient for WM hash + human scan)
- plan_id, intent_id copied as Some(...)
- step_id currently None; can populate using metadata e.g. `step_id` if present
- attestation_hash = metadata["signature"] if `Value::String`
- content_hash left None → WM ingestor computes deterministic FNV-like hash

Tags and estimation:
- The WM ingestor itself assigns tags: "wisdom", "causal-chain", "distillation", plus lowercased action kind. No special handling required in the sink.

## Replay Helper (Optional Utility)

Location suggestion: same module as the sink or a CH utility.

```rust
pub fn rebuild_working_memory_from_ledger(
    wm: &mut WorkingMemory,
    chain: &crate::ccos::causal_chain::CausalChain
) -> Result<(), crate::ccos::working_memory::backend::WorkingMemoryError> {
    let mut records = Vec::new();
    for action in chain.get_all_actions().iter() {
        let record = WmIngestionSink::map_action_to_record(action);
        records.push(record);
    }
    MemoryIngestor::replay_all(wm, &records)
}
```

## Concurrency, Performance, and Safety

- Causal Chain is typically wrapped in Arc<Mutex<CausalChain>>. `notify_sinks` must remain small and non-blocking to avoid extending critical sections.
- The WM sink maps and appends into WM; for JSONL persistence, appends are lightweight and idempotent. If contention is observed, the sink can queue internally and batch outside the chain lock.
- Future refinement: provide a non-blocking broadcast option using a channel (e.g., `tokio::sync::broadcast`) so observers consume events outside the Causal Chain lock. This can be added without breaking the sink trait.

## Security and Governance

- Attestation: Preserve the action’s `signature` in the derived `ActionRecord.attestation_hash` to support downstream provenance in WM.
- Zero-trust: The sink should not mutate ledger or Causal Chain state; it only derives secondary records for WM.
- Audit: WM is a distilled recall layer; Causal Chain remains the immutable ground truth for audits.

## Migration Plan (Phased)

Phase 1 (infra):
- Add trait `CausalChainEventSink`.
- Add `event_sinks` field and `register_event_sink` / `notify_sinks`.
- Invoke `notify_sinks` in the listed append paths.

Phase 2 (adapter):
- Implement `WmIngestionSink` and mapping function.
- Add replay helper for rebuild use-cases.

Phase 3 (wiring):
- In CCOS initialization (orchestrator/boot), construct a `WorkingMemory` instance and register a `WmIngestionSink` on the shared `CausalChain` (one-liner registration).

Phase 4 (tests):
- Unit tests for mapping determinism (timestamp conversion, provider/kind, attestation extraction).
- Integration test: run a minimal plan that logs a few actions, verify WM query returns entries tagged with "wisdom"/"causal-chain"/"distillation" and correct recency ordering; ensure idempotency on re-run or replay.

## Test Plan

1. Unit: `map_action_to_record` stable hashing inputs (summary/content formation deterministic given equal inputs).
2. Unit: WM ingestor idempotency already covered; extend with an action mapped from `Action`.
3. Integration: 
   - Initialize `CausalChain`, `WorkingMemory` (InMemoryJsonlBackend), register `WmIngestionSink`.
   - Log several actions (PlanStarted, CapabilityCall, PlanCompleted).
   - Query WM: entries contain the required tags, correct ordering, and plan/intent linkage in meta.
   - Replay path yields identical WM state.

## Open Questions

- Do we want `CausalChainEventSink` to require `Send + Sync + 'static` now? (Can be deferred; initial impls using `Arc<Mutex<...>>` are typically Send/Sync.)
- Should we support deregistration / weak sinks? (Out of scope for Phase 1.)
- Should we promote a broadcast channel variant immediately? (Keep for future refinement if needed.)
- Step ID: can be read from metadata where available (e.g., "step_id") to improve CH reductions.

## Backwards Compatibility

- If no sink is registered, there is no behavioral or ABI change.
- The sink trait and new methods are purely additive.
- Integration is opt-in and isolated to CCOS initialization points.

## Alignment With CCOS Specs

- SEP-003: Causal Chain remains the source of truth; event streaming is an observation mechanism.
- SEP-013: Ingestion is idempotent, tagged appropriately, and stores provenance (attestation hash).
- SEP-009/014: CH can apply Boundaries/ReductionStrategy to WM entries downstream; step special forms continue to be logged, then distilled into WM.

---
This specification is intended for review and tracking within Issue #6 before any code changes. Once approved, implementation can proceed in small, isolated PRs aligned with the phases above.
