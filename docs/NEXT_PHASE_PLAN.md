# CCOS Next Phase Development Plan

**Status:** Ready for Multi-Agent Execution  
**Date:** 2025-01-XX  
**Prerequisites:** RTFS-CCOS Decoupling Migration (100% Complete ✅)

## Executive Summary

After successful RTFS-CCOS decoupling migration, this plan defines prioritized work streams for multi-agent parallel development. The architecture now supports independent development of RTFS language features and CCOS capabilities.

## Current Architecture State

### ✅ **Completed Infrastructure**
- **RTFS**: Pure functional runtime, completely CCOS-agnostic (147/147 tests passing)
- **CCOS**: Orchestration layer with capability marketplace, delegation engine, causal chain
- **Boundary**: Clean `HostInterface` separation with yield-based control flow
- **Tests**: 147 RTFS tests, 0 ignored (CCOS capability tests moved to CCOS suite)
- **Migration**: All 20+ checklist items complete
- **Stream A**: Pattern matching, type coercion, expression conversion, IR execution - 100% COMPLETE ✅

### ⚠️ **Known Gaps from Documentation**
From `docs/archive/ccos/RTFS_MIGRATION_PLAN.md`:
- ✅ Pattern matching in functions (COMPLETE - Stream A)
- ✅ Type coercion logic (COMPLETE - Stream A)
- ✅ Expression conversion completeness (COMPLETE - Stream A)
- ✅ IR node execution completeness (COMPLETE - Stream A)
- Advanced capability providers (MCP, A2A, RemoteRTFS) - frameworks exist, need implementation
- Standard library I/O functions (read-file, write-file, etc.)

---

## Work Stream Organization

### Stream A: RTFS Language Core (High Priority)
**Assigned Agent:** RTFS Language Specialist  
**Goal:** Complete core RTFS language features for functional completeness  
**Estimated Effort:** 2-3 weeks

#### Task A1: Pattern Matching Implementation ✅ COMPLETE
**Priority:** HIGH | **Effort:** 1 week | **Status:** DONE

**Context:**
- RTFS parser supports `match` expression syntax
- No runtime implementation exists
- Critical for control flow and data destructuring

**Acceptance Criteria:**
1. ✅ Implement `Match` IR node execution in `rtfs/src/runtime/ir_runtime.rs`
2. ✅ Support pattern matching on integers, strings, symbols, vectors, maps
3. ✅ Support `_` wildcard pattern
4. ✅ Support binding patterns with variable destructuring
5. ✅ Add comprehensive tests in `rtfs/tests/test_pattern_matching.rs`
6. ✅ All new tests pass (9 tests passing)
7. ✅ No regression in existing tests

**Files Modified:**
- `rtfs/src/runtime/ir_runtime.rs` - Added `pattern_matches` and `execute_match` logic
- `rtfs/src/ir/converter.rs` - Added `convert_match_pattern` for AST→IR conversion
- `rtfs/tests/test_pattern_matching.rs` - Comprehensive test suite

**Result:** 100% complete. Pattern matching fully functional with vector, map, literal, wildcard, and guard patterns.

#### Task A2: Type Coercion System ✅ COMPLETE
**Priority:** HIGH | **Effort:** 1 week | **Status:** DONE

**Context:**
- Basic type coercion exists in evaluator
- Needs comprehensive coverage
- Critical for ergonomic RTFS programming

**Acceptance Criteria:**
1. ✅ Integer ↔ float coercion verified in arithmetic operations
2. ✅ Proper error handling for incompatible types confirmed
3. ✅ Comprehensive type coercion tests added
4. ✅ All new tests pass (8/8 tests passing)
5. ✅ No regression in existing tests

**Files Modified:**
- `rtfs/src/runtime/secure_stdlib.rs` - Verified existing coercion logic
- `rtfs/tests/test_type_coercion.rs` - Comprehensive test suite (8 tests)

**Result:** 100% complete. Type coercion verified and tested for integer-float conversions in all arithmetic operations.

#### Task A3: Expression Conversion Completeness ✅ COMPLETE
**Priority:** MEDIUM | **Effort:** 0.5 weeks | **Status:** DONE

**Context:**
- AST to IR conversion exists but may be incomplete
- Need validation of edge cases

**Acceptance Criteria:**
1. ✅ Audit `IrConverter::convert_expression` completed
2. ✅ Added `Expression::Deref` conversion (desugared to function call)
3. ✅ All AST expression types verified to convert correctly
4. ✅ No regressions

**Files Modified:**
- `rtfs/src/ir/converter.rs` - Added `Deref` desugaring to `FunctionCall`

**Result:** 100% complete. All AST expression types convert to IR correctly.

#### Task A4: IR Node Execution Audit ✅ COMPLETE
**Priority:** MEDIUM | **Effort:** 0.5 weeks | **Status:** DONE

**Context:**
- IR runtime exists but may have incomplete execution paths
- Need systematic audit

**Acceptance Criteria:**
1. ✅ Audited all `IrNode` variants in `IrRuntime::execute_node`
2. ✅ All variants have proper execution paths
3. ✅ `IrNode::Param` identified as structural (yields Nil when evaluated directly)
4. ✅ All existing tests validate IR execution paths
5. ✅ No regressions

**Files Audited:**
- `rtfs/src/runtime/ir_runtime.rs` - Verified complete execution coverage

**Result:** 100% complete. All IR node variants have proper execution paths.

---

### Stream B: CCOS Standard Capabilities (High Priority)
**Assigned Agent:** CCOS Capabilities Specialist  
**Goal:** Complete standard I/O and basic CCOS capabilities  
**Estimated Effort:** 1-2 weeks

#### Task B1: File I/O Capabilities
**Priority:** HIGH | **Effort:** 1 week

**Context:**
- File operations are CCOS capabilities (not RTFS stdlib)
- Need `file-exists`, `read-file`, `write-file`, `delete-file`
- Should integrate with security context

**Acceptance Criteria:**
1. Implement `ccos.io.read-file` capability
2. Implement `ccos.io.write-file` capability
3. Implement `ccos.io.delete-file` capability (note: `file-exists` likely exists)
4. Add security context validation for file operations
5. Add capability marketplace registration
6. Add CCOS integration tests
7. All tests must pass
8. Update capability documentation

**Files to Create/Modify:**
- `ccos/src/capabilities/providers/local_file_provider.rs` - New file
- `ccos/src/lib.rs` - Register new capabilities
- `ccos/tests/test_file_io_capabilities.rs` - New test file

**References:**
- Look at `ccos/src/capabilities/providers/` for provider patterns
- Look at `ccos/src/capabilities/registry.rs` for registration

#### Task B2: JSON Parsing Capabilities
**Priority:** MEDIUM | **Effort:** 0.5 weeks

**Context:**
- JSON is a common data format
- Should be CCOS capability

**Acceptance Criteria:**
1. Implement `ccos.json.parse` capability
2. Implement `ccos.json.stringify` capability
3. Add marketplace registration
4. Add tests
5. All tests must pass

**Files to Create/Modify:**
- `ccos/src/capabilities/providers/json_provider.rs` - New file
- `ccos/src/lib.rs` - Register capabilities
- `ccos/tests/test_json_capabilities.rs` - New test file

---

### Stream C: CCOS Advanced Capabilities (Medium Priority)
**Assigned Agent:** CCOS Integration Specialist  
**Goal:** Implement advanced capability providers with existing frameworks  
**Estimated Effort:** 2-3 weeks

#### Task C1: MCP GitHub Provider Completion
**Priority:** MEDIUM | **Effort:** 1 week

**Context:**
- MCP framework exists in CCOS
- GitHub MCP needs actual implementation
- Memory shows "Phase 2: 100% COMPLETE AND PRODUCTION READY" for session management
- Need to verify and complete actual GitHub API integration

**Acceptance Criteria:**
1. Verify existing MCP session management code
2. Complete GitHub API capability implementations (if not done)
3. Test real GitHub API calls (list issues, create issue, etc.)
4. Add error handling for API failures
5. Add security context validation
6. All integration tests must pass

**Files to Modify:**
- `ccos/src/capabilities/providers/mcp_github_provider.rs` (if exists)
- `ccos/src/capabilities/mcp_*` - Check MCP framework files
- `ccos/tests/test_github_mcp_integration.rs`

**References:**
- Memory mentions: `session_pool.rs`, `mcp_session_handler.rs`, `marketplace.rs`
- Look for "Phase 2: 100% COMPLETE" MCP work

#### Task C2: Remote RTFS Capability Provider
**Priority:** HIGH | **Effort:** 1.5 weeks

**Context:**
- Framework exists but implementation missing
- Critical for distributed execution
- Should execute RTFS plans on remote hosts via HTTP/RPC

**Acceptance Criteria:**
1. Implement `RemoteRTFSCapability` provider
2. Support plan step serialization to RTFS values
3. Support HTTP execution with security context propagation
4. Add marketplace registration
5. Add comprehensive tests
6. All tests must pass

**Files to Create/Modify:**
- `ccos/src/capabilities/providers/remote_rtfs_provider.rs` - New file
- `ccos/src/lib.rs` - Register capability
- `ccos/tests/test_remote_rtfs_capability.rs` - New test file

**References:**
- Check `docs/archive/ccos/SECURITY_FRAMEWORK.md` (line 968-976)
- Look at existing HTTP capability patterns

#### Task C3: A2A Capability Provider
**Priority:** MEDIUM | **Effort:** 1 week

**Context:**
- Agent-to-agent communication framework exists
- Need actual implementation

**Acceptance Criteria:**
1. Implement `A2ACapability` provider
2. Support inter-agent message passing
3. Add security context validation
4. Add marketplace registration
5. Add tests
6. All tests must pass

**Files to Create/Modify:**
- `ccos/src/capabilities/providers/a2a_provider.rs` - New file (or complete existing)
- `ccos/src/lib.rs` - Register capability
- `ccos/tests/test_a2a_capability.rs` - New test file

---

### Stream D: Infrastructure & Quality (Lower Priority)
**Assigned Agent:** DevOps/QA Specialist  
**Goal:** Production readiness and quality assurance  
**Estimated Effort:** 1-2 weeks

#### Task D1: Performance Benchmarks
**Priority:** LOW | **Effort:** 0.5 weeks

**Acceptance Criteria:**
1. Create benchmarks for RTFS core operations (arithmetic, function calls, etc.)
2. Create benchmarks for CCOS capability execution
3. Run benchmarks before and after major changes
4. Document performance targets
5. Set up CI benchmark regression checks

**Files to Create:**
- `rtfs/benches/core_operations.rs` - New file
- `ccos/benches/capability_execution.rs` - New file
- `docs/PERFORMANCE.md` - New documentation

**References:**
- Use `criterion` crate (already in features)

#### Task D2: Viewer Server Migration
**Priority:** LOW | **Effort:** 1 week

**Context:**
- `viewer_server` currently depends on `rtfs_compiler`
- Already commented out dependency
- Needs update to use `rtfs` + `ccos` crates

**Acceptance Criteria:**
1. Update `viewer_server/src/main.rs` imports from `rtfs_compiler::*` to `ccos::*` and `rtfs::*`
2. Update all viewer server source files
3. Fix compilation errors
4. Test viewer functionality
5. Remove TODO comment

**Files to Modify:**
- `viewer_server/src/main.rs`
- `viewer_server/tests/*.rs`
- Other viewer_server source files

---

## Execution Coordination

### Development Workflow
1. **Parallel Execution**: Streams A, B, C can run in parallel (independent)
2. **Sequential Within Stream**: Tasks within a stream should be completed in order
3. **Integration Points**: All agents coordinate on shared interfaces

### Code Review Process
1. Each task must have comprehensive tests
2. All existing tests must pass (no regressions)
3. Code follows project conventions (from workspace rules)
4. Documentation updated for new features

### Commit Strategy
1. Feature branches for each task
2. Atomic commits (one logical change per commit)
3. Descriptive commit messages
4. User preference: direct commits to main (no PRs based on memory)

### Testing Requirements
- **RTFS**: Must pass all 147 existing tests + new tests
- **CCOS**: All CCOS tests must pass + new capability tests
- **Integration**: End-to-end tests for new capabilities

---

## Success Metrics

### Stream A (RTFS Core) ✅ 100% COMPLETE
- ✅ Pattern matching fully implemented and tested (9 tests)
- ✅ Type coercion comprehensive and tested (8 tests)
- ✅ Expression conversion 100% complete (all AST types)
- ✅ IR execution 100% complete (all IrNode variants)
- ✅ No regression in existing tests
- ✅ **Final Status:** 147/147 RTFS tests passing, 0 ignored

### Stream B (Standard Capabilities)
- ✅ File I/O capabilities working
- ✅ JSON parsing capabilities working
- ✅ All capabilities properly registered
- ✅ Security context integration validated
- ✅ All tests passing

### Stream C (Advanced Capabilities)
- ✅ MCP GitHub integration production-ready
- ✅ Remote RTFS capability working
- ✅ A2A capability working
- ✅ All capabilities security-validated
- ✅ All tests passing

### Stream D (Infrastructure)
- ✅ Benchmarks established and CI-integrated
- ✅ Viewer server migrated
- ✅ No known technical debt

---

## Risk Mitigation

### Technical Risks
- **Breaking Changes**: Mitigated by comprehensive test coverage
- **Performance Regressions**: Mitigated by benchmarks
- **Integration Issues**: Mitigated by clear interface boundaries

### Coordination Risks
- **Merge Conflicts**: Agents work on separate streams
- **Interface Changes**: Coordination required for shared interfaces
- **Timeline Delays**: Streams are independent, can adjust scope

---

## References

### Key Documentation
- Migration Plan: `docs/archive/rtfs-2.0/specs-before-migration/99-rtfs-ccos-decoupling-migration.md` (✅ Complete)
- CCOS Foundation Plan: `docs/archive/ccos/CCOS_FOUNDATION_IMPLEMENTATION_PLAN.md`
- Security Framework: `docs/archive/ccos/SECURITY_FRAMEWORK.md`
- RTFS Migration Tracker: `docs/archive/ccos/RTFS_MIGRATION_PLAN.md`

### Key Code Locations
- RTFS Core: `rtfs/src/runtime/`
- RTFS Tests: `rtfs/tests/`
- CCOS Capabilities: `ccos/src/capabilities/`
- CCOS Tests: `ccos/tests/`
- Workspace Root: `Cargo.toml` (defines workspace members)

---

## Agent Assignment Template

```markdown
**Agent Role:** [RTFS Language Specialist | CCOS Capabilities Specialist | CCOS Integration Specialist | DevOps/QA Specialist]

**Assigned Stream:** [A | B | C | D]

**Current Task:** [Task ID and Name]

**Status:** [Not Started | In Progress | Blocked | Complete]

**Notes:**
- Technical blockers or decisions needed
- Coordination requirements
- Estimated completion
```

---

## Change Log

| Date | Change | Author |
|------|--------|--------|
| 2025-01-XX | Initial plan created | AI Assistant |
| 2025-01-26 | Stream A completed: Pattern matching (9 tests), Type coercion (8 tests), Expression conversion, IR execution audit. All 147 RTFS tests passing, 0 ignored. | AI Assistant |
| | | |

