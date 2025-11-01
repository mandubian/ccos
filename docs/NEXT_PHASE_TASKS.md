# Next Phase Development Tasks - Quick Reference

**Plan:** See `docs/NEXT_PHASE_PLAN.md` for full context  
**Status:** Ready for agent assignment

---

## Stream A: RTFS Language Core (High Priority) ✅ 100% COMPLETE

### A1: Pattern Matching Implementation ✅ COMPLETE
**Files:** `rtfs/src/runtime/ir_runtime.rs`, `rtfs/tests/test_pattern_matching.rs`  
**Acceptance:** Match on int/string/symbol/vector/map, binding patterns, wildcard, tests pass  
**Status:** 9 tests passing, full implementation complete

### A2: Type Coercion System ✅ COMPLETE
**Files:** `rtfs/src/runtime/values.rs`, `rtfs/src/runtime/evaluator.rs`, `rtfs/tests/test_type_coercion.rs`  
**Acceptance:** int↔float, string↔symbol, error handling, tests pass  
**Status:** 8 tests passing, full implementation verified

### A3: Expression Conversion Completeness ✅ COMPLETE
**Files:** `rtfs/src/ir/converter.rs`, `rtfs/tests/test_converter_completeness.rs`  
**Acceptance:** 100% AST→IR coverage, tests pass  
**Status:** Deref conversion added, all expressions verified

### A4: IR Node Execution Audit ✅ COMPLETE
**Files:** `rtfs/src/runtime/ir_runtime.rs`, `rtfs/tests/test_ir_execution_completeness.rs`  
**Acceptance:** 100% execution path coverage, tests pass  
**Status:** All IrNode variants verified, complete coverage

**Stream A Result:** 147/147 RTFS tests passing, 0 ignored. CCOS-dependent tests removed from RTFS suite.

---

## Stream B: CCOS Standard Capabilities (High Priority) ✅ 100% COMPLETE

### B0: Restore CCOS streaming runtime ✅ COMPLETE
**Files:** `ccos/src/streaming/runtime.rs`, `ccos/src/streaming/mod.rs`
**Acceptance:** Streaming runtime restored inside CCOS and re-exported; integration tests updated to import `ccos::streaming`.
**Status:** Completed (streaming types re-exported and tests updated).

### B1: File I/O Capabilities ✅ COMPLETE
**Files:** `ccos/src/capabilities/providers/local_file_provider.rs`, `ccos/tests/test_file_io_capabilities.rs`  
**Acceptance:** read-file, write-file, delete-file, file-exists, security validation, tests pass  
**Status:** ✅ Implementation complete, 2 tests passing

### B2: JSON Parsing Capabilities ✅ COMPLETE
**Files:** `ccos/src/capabilities/providers/json_provider.rs`, `ccos/tests/test_json_capabilities.rs`  
**Acceptance:** parse/stringify, pretty-print, marketplace registration, legacy aliases, tests pass  
**Status:** ✅ Implementation complete, 2 tests passing

**Stream B Result:** All capabilities implemented and tested. 4/4 tests passing.

---

## Stream C: CCOS Advanced Capabilities (Medium Priority) ✅ 100% COMPLETE

### C1: MCP GitHub Provider Completion ℹ️ SKIPPED
**Files:** `ccos/src/capabilities/providers/github_mcp.rs` (existing)
**Status:** ⏭️ Skipped - MCP wrapper in capabilities + mcp_introspector provides MCP endpoint support
**Note:** Existing MCP infrastructure is sufficient for GitHub and other MCP services

### C2: Remote RTFS Capability Provider ✅ COMPLETE
**Files:** `ccos/src/capabilities/providers/remote_rtfs_provider.rs`, `ccos/tests/test_remote_rtfs_capability.rs`  
**Acceptance:** Remote execution, serialization, security propagation, tests pass  
**Status:** ✅ Implementation complete, 5 tests passing

### C3: A2A Capability Provider ✅ COMPLETE
**Files:** `ccos/src/capabilities/providers/a2a_provider.rs`, `ccos/tests/test_a2a_capability.rs`  
**Acceptance:** Agent communication, security validation, tests pass  
**Status:** ✅ Implementation complete, 5 tests passing

**Stream C Result:** All required capabilities implemented and tested. 10/10 tests passing.

---

## Stream D: Infrastructure & Quality (Partial Complete)

### D1: Performance Benchmarks ✅ COMPLETE
**Files:** `rtfs/benches/core_operations.rs`, `ccos/benches/capability_execution.rs`, `docs/BENCHMARKS.md`
**Acceptance:** Benchmarks created, CI integrated, targets documented  
**Status:** ✅ Complete - Benchmarks compile and run successfully
**Benchmarks Created:**
- RTFS: Parsing, evaluation, pattern matching, stdlib (4 benchmark groups)
- CCOS: Capability registration, execution, security validation, serialization (4 benchmark groups)
- Documentation: Performance targets, baseline metrics, CI integration guide

### D2: Viewer Server Migration ℹ️ LOW
**Files:** `viewer_server/src/main.rs`, `viewer_server/tests/*.rs`  
**Acceptance:** Migrated from rtfs_compiler to rtfs+ccos, tests pass  
**Estimated:** 1 week

### D3: Workspace compile triage ✅ COMPLETE
**Files:** workspace-wide (ccos, rtfs, rtfs_compiler)
**Acceptance:** Investigate and fix workspace compilation blockers surfaced after the CCOS/RTFS separation — examples include missing `plan_archive`, private field access, and type mismatches. Goal is to reach a clean `cargo check -p ccos` and stabilize CI for subsequent tasks.
**Status:** ✅ Complete - `cargo check -p ccos` and `cargo check -p rtfs` both pass with 0 errors
**Fixes Applied:**
- Fixed plan_archive module resolution
- Fixed Host trait method signatures
- Removed context_manager references (RTFS/CCOS separation)
- Fixed IsolationLevel conversion
- Added CCOS public accessor methods
- Fixed CapabilityRegistry type mismatches (27+ errors resolved)
- Fixed binary import paths (rtfs_ccos_repl, resolve_deps)
- Fixed test infrastructure (24+ errors resolved)

---

## Priority Legend

- ⚠️ HIGH: Critical for core functionality
- ℹ️ MEDIUM: Important but not blocking
- ℹ️ LOW: Quality of life improvements

## Testing Requirements (All Tasks)

- **No Regressions**: All existing tests must pass
- **New Tests**: Comprehensive tests for new functionality
- **Integration**: End-to-end validation where applicable

## Assignment Instructions

1. Pick a stream (A, B, C, or D)
2. Start with first task in stream
3. Complete task acceptance criteria
4. Verify all tests pass
5. Move to next task in stream

## Questions?

See full plan: `docs/NEXT_PHASE_PLAN.md`

