# GitHub Issues Status Report - mandubian/ccos

**Date:** 2 janvier 2026 (Last Updated)

## Summary
This document summarizes the current state of GitHub issues and pull requests for the `mandubian/ccos` project, identifying items that can be closed and those requiring further work.

---

## 1. Recently Closed Issues
The following issues have been closed as they are completed.

| Issue # | Title | Reason for Closing | Closed Date |
|---------|-------|--------------------|-------------|
| #173 | [Phase 6] Expose CLI as Governed Native Capabilities | ✅ **CLOSED** - Closed on 2025-12-27. Implementation verified in `ccos/src/ops/` and `NativeCapabilityProvider`. | 2025-12-27 |
| #157 | Defmacro in RTFS | ✅ **CLOSED** - Closed on 2025-12-27. Implementation completed and merged via PR #158. | 2025-12-27 |
| #116 | Orchestrator: IntentGraph status | Verified `orchestrator.rs` updates `IntentGraph` after execution. | - |
| #102 | Provenance logging | Verified `ccos/src/causal_chain/provenance.rs` exists and is used. | - |

---

## 2. Pull Requests
| PR # | Title | Status | Notes |
|------|-------|--------|-------|
| #179 | test(rtfs): Comprehensive RTFS robustness test suite | ✅ **MERGED** (2025-12-31) | Successfully merged. Implements comprehensive robustness testing, fuzzing, and error formatting. Directly addresses issues #53 and #46. |

---

## 3. Issues Requiring Work
The following issues are active and represent the current development roadmap.

### High Priority / Core Architecture
| Issue # | Title | Description | Status |
|---------|-------|-------------|--------|
| #178 | Planner: lightweight adapters | Bridge incompatible tool I/O with tiny RTFS adapters. | Open |
| #177 | Planner: synthesis-or-queue | Synthesize capabilities for missing data/format or queue for implementation. | Open |
| #176 | Planner: iterative refinement | Iterative refinement and opportunistic safe execution for grounding. | Open |
| #166 | Move AgentConfig from RTFS to CCOS | ✅ **COMPLETED** (2026-01-05) - Commit 73c8172. All types migrated, imports updated, backwards compatibility maintained. | Completed |
| #163 | Sandbox/isolation strategy | Implement isolation for plan execution (microVMs/containers). | Open |
| #117 | GovernanceKernel: constitutional validation | Implement logic to validate plans against constitutional rules. | Partial |
| #103 | Pluggable remote prompt stores | Support Git/HTTP backends for prompt management. | Open |
| #101 | Enhance plan prompts | Prompt engineering for better planning. | Open |

### CLI & UX
| Issue # | Title | Description |
|---------|-------|-------------|
| #174 | [CLI UX] Add Interactive Mode | Add interactive selection and better semantic filtering to the CLI. |
| #167 | [Umbrella] CCOS CLI: Unified Tool | Umbrella issue for CLI development (Phases 7 and 8 remaining). |

### RTFS Language & Stability
| Issue # | Title | Description |
|---------|-------|-------------|
| #153 | Fix import options parsing | Fix `:as` and `:only` keywords in RTFS parser. |
| #127 | Stabilize function_expressions | Fix failures in `test_function_expressions_feature`. |
| #149 | RTFS Mutation Migration | Phase 2: Feature gate legacy atoms. |
| #146-#138 | RTFS 2.0 Features | Implementation of lazy evaluation, function literals, comprehensions, etc. |

### Storage & Observability
| Issue # | Title | Description |
|---------|-------|-------------|
| #164 | Storage/Archives vs CausalChain | Ensure archives complement rather than replace the CausalChain. |
| #162 | Bring CausalChain up to date | Update CausalChain to capture all relevant events from newer modules. |
| #137-#135 | Observability Enhancements | Cached aggregates, WM ingest latency, and ActionType counters. |
| #103 | Pluggable remote prompt stores | Support Git/HTTP backends for prompt management. |

### Older Architectural Issues (Audit)
| Issue # | Title | Description | Status |
|---------|-------|-------------|--------|
| #99 | Track defensive test-runtime fixes | Review temporary shims in `wip/ir-step-params-binding`. | Open |
| #95 | Negative & Edge-Case Tests | Augment test suite with explicit negative scenarios for delegation. | Open |
| #92 | Wire Task Protocol into Orchestrator | Implement execution path for structured delegated tasks. | Open |
| #91 | AST-Based Capability Extraction | Replace tokenizer-based preflight with AST walk. | Open |
| #88 | Thread-safe Host + Parallel steps | Implement true parallel execution for `step-parallel`. | Open |
| #77 | L4 Content-Addressable RTFS Cache | Integrate bytecode caching with unified storage. | Open |
| #66-#63 | Agent Isolation & MicroVMs | Connect to actual MicroVM providers (Firecracker, gVisor). | Open |
| #55 | CCOS RTFS Library for Intent Graph | Create RTFS bindings for Intent Graph functions. | Open |
| #53 | RTFS Unit Test Stabilization | Resolve remaining unit test failures (addressed by PR #179). | Open |
| #46 | Fuzz Testing for Parser | Implement fuzz testing (addressed by PR #179). | Open |

---

## 4. Next Steps

### ✅ Completed
1. ✅ **Close #173 and #157** on GitHub - **DONE** (both closed on 2025-12-27)
2. ✅ **Review and Merge PR #179** - **DONE** (merged on 2025-12-31)

### 🔄 In Progress / Next Actions
3. **Prioritize Planner Grounding Fixes** (#176, #177, #178) as they are critical for autonomous agent reliability.
   - All three issues are currently **OPEN** and ready for work
   - Reference plan: `docs/drafts/iterative-planner-synthesis-plan.md`

### 📊 Status Summary
- **Closed Issues**: #173, #157 (completed)
- **Completed Migrations**: #166 (AgentConfig migration - commit 73c8172)
- **Merged PRs**: #179 (completed)
- **Active High-Priority Issues**: #176, #177, #178 (3 issues)
