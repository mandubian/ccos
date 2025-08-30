wt/causal-chain-event-stream — Issue #56: SEP-003 Causal Chain Event Stream implementation

## Implementation Status: ✅ COMPLETE

### Phase 1: Event Sink Infrastructure ✅
- Added `CausalChainEventSink` trait for observer pattern
- Implemented `register_event_sink()` and `notify_sinks()` methods in `CausalChain`
- Wired notifications to all append paths (record_result, log_plan_event, etc.)
- Lightweight, non-blocking design preserves causal chain performance

### Phase 2: Working Memory Ingestion Sink ✅
- Implemented `WorkingMemorySink` and `WmIngestionSink` adapters
- Added deterministic `Action` → `ActionRecord` mapping with:
  - Proper timestamp conversion (ms → s)
  - Provider/kind extraction
  - Attestation hash forwarding from metadata
  - Compact content string for idempotent hashing
- Integrated with existing `MemoryIngestor` for automatic tagging

### Phase 3: Wiring and Integration ✅
- Registered WM sink in `CCOSEnvironment::new()` when enabled
- Added environment variable controls: `CCOS_ENABLE_WM_INGESTOR`, `CCOS_WM_MAX_ENTRIES`, `CCOS_WM_MAX_TOKENS`
- Implemented full observability ingestor capability: `observability.ingestor:v1.ingest`
- Supports single, batch, and replay modes with proper error handling

### Phase 4: Testing and Validation ✅
- **Unit Tests**: Event sink registration/notification, action mapping determinism, hash stability
- **Integration Tests**: End-to-end WM ingestion from causal chain events
- **Capability Tests**: Observability ingestor single/batch/replay modes
- **Idempotency Tests**: Replay produces same WM state as online ingestion

### Key Features Implemented
- **Event Stream API**: Clean trait-based observer pattern
- **Automatic WM Ingestion**: Real-time distillation of causal chain actions
- **Replay Capability**: Rebuild WM from full ledger history
- **Tagging System**: Actions tagged with "wisdom", "causal-chain", "distillation", and action-type
- **Security**: Preserves attestation and provenance through the pipeline
- **Performance**: Non-blocking, lightweight observers don't impact causal chain critical sections

### Files Modified/Added
- `rtfs_compiler/src/ccos/event_sink.rs` - Event sink traits
- `rtfs_compiler/src/ccos/causal_chain/mod.rs` - Event sink integration
- `rtfs_compiler/src/ccos/working_memory/ingestor.rs` - WM ingestion sink
- `rtfs_compiler/src/ccos/wm_integration.rs` - Bridge utilities
- `rtfs_compiler/src/runtime/ccos_environment.rs` - Environment wiring
- `rtfs_compiler/tests/working_memory_integration.rs` - End-to-end tests

### Acceptance Criteria Met ✅
- ✅ Causal Chain compiles and behaves identically when no sink registered
- ✅ Appended actions produce WM entries with proper tags ("wisdom", "causal-chain", "distillation")
- ✅ Replay from ledger produces identical WM state (idempotent)
- ✅ Integration tests pass: Context Horizon can query distilled wisdom entries
- ✅ No blocking/async operations in causal chain critical sections
- ✅ Preserves ledger immutability and append-only semantics

### Next Steps
Ready for integration testing with Context Horizon and full CCOS orchestration flow.