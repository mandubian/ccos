# Plan: Content-Linked Promotion Gates for Agent Installation

## Problem Statement

The current promotion gate system allows a planner to spawn `specialized_builder` without actual evaluator/auditor validation because:

1. **No content linkage**: `promotion_gate` only carries boolean flags (`evaluator_pass`, `auditor_pass`) and evidence objects, but is not tied to the actual content handle being installed.
2. **No gateway verification**: The gateway doesn't verify that evaluator/auditor actually ran and produced results — it just trusts the boolean flags.
3. **Coder produces artifacts**: The coder can generate multiple files in an artifact, but there's no system to associate promotion evidence with the artifact handle.

## Solution Overview

Create a **Content Promotion Registry** that links promotion status to specific content handles, verified via causal chain audit trail.

```
┌─────────────────────────────────────────────────────────────────────────┐
│  CODER writes content → content.handle = sha256:abc123                  │
│                                    ↓                                     │
│  EVALUATOR validates content sha256:abc123                               │
│    → calls promotion.record(content_handle=sha256:abc123, pass=true)    │
│                                    ↓                                     │
│  AUDITOR validates content sha256:abc123                                │
│    → calls promotion.record(content_handle=sha256:abc123, auditor_pass)  │
│                                    ↓                                     │
│  SPECIALIZED_BUILDER installs sha256:abc123                             │
│    → gateway verifies causal chain: did evaluator/auditor validate?      │
│    → if NOT validated → REJECT install                                   │
│    → if validated → proceed with capability/security analysis           │
└─────────────────────────────────────────────────────────────────────────┘
```

## Implementation Phases

---

### Phase 1: Content Promotion Registry

**Goal**: Create a gateway-side registry that tracks promotion status per content handle.

#### Checkpoint 1.1: Define PromotionRecord types
- [x] **File**: `autonoetic-types/src/promotion.rs` (new)
- [x] Define `PromotionRecord` struct
- [x] Define `Finding` struct with severity, description, evidence
- [x] Define `promotion.record` tool arguments and response types
- [x] Export new types in `autonoetic-types/src/lib.rs`

#### Checkpoint 1.2: Create PromotionStore
- [x] **File**: `autonoetic-gateway/src/runtime/promotion_store.rs` (new)
- [x] `PromotionStore` struct backed by JSON file in gateway state dir
- [x] Methods: `record_promotion`, `get_promotion`, `list_promotions`
- [x] Implement proper error handling
- [x] Thread-safe (use Mutex)

#### Checkpoint 1.3: Implement `promotion.record` tool
- [x] **File**: `autonoetic-gateway/src/runtime/tools_promotion.rs` (new)
- [x] Tool name: `promotion.record`
- [x] Arguments: content_handle, role, pass, findings, summary
- [x] Response: `{ok: true, promotion_record: {...}}`
- [x] Only allowed for `evaluator.default` and `auditor.default` agents
- [x] Records to `PromotionStore`

#### Checkpoint 1.4: Implement `promotion.query` tool
- [x] **File**: `autonoetic-gateway/src/runtime/tools_promotion.rs`
- [x] Tool name: `promotion.query`
- [x] Anyone can query

---

### Phase 2: Causal Chain Verification

**Goal**: Enable the gateway to verify promotion records against the causal chain for tamper evidence.

#### Checkpoint 2.1: CausalChain helper for promotion lookup
- [x] **File**: `autonoetic-gateway/src/causal_chain/promotion_lookup.rs` (new)
- [x] `PromotionLookup::new(causal_log_path: PathBuf)`
- [x] `find_promotion_records(content_handle: &str) -> Vec<CausalChainEntry>`
- [x] `verify_promotion(content_handle: &str, role: &str) -> bool`

#### Checkpoint 2.2: Enhance PromotionStore with causal verification
- [x] Deferred verification to install time (Phase 3)

---

### Phase 3: Gateway Install Verification

**Goal**: Make `agent.install` verify content was validated before allowing install.

#### Checkpoint 3.1: Modify InstallAgentArgs to require content_handle
- [x] **File**: `autonoetic-gateway/src/runtime/tools.rs` (InstallAgentArgs at ~line 2500)
- [x] Add `source_content_handle: Option<String>` to `InstallAgentArgs`
- [x] Add to JSON schema in tool definition

#### Checkpoint 3.2: Update promotion_gate validation
- [x] In `validate_promotion_gate_evidence` (~line 2587)
- [x] Query `PromotionStore` for this handle
- [x] Verify `evaluator_pass: true` and `auditor_pass: true`
- [x] Verify via causal chain that evaluator/auditor actually called `promotion.record`
- [x] If verification fails → REJECT install with clear error message

#### Checkpoint 3.3: Enhanced error messages
- [x] Update error messages to be clear about what failed:
  - `"content sha256:... was not validated by evaluator.default"`
  - `"content sha256:... evaluator passed but auditor.default did not validate"`
  - `"promotion.record call not found in causal chain for content sha256:..."`

---

### Phase 4: Evaluator/Auditor Integration

**Goal**: Update evaluator.default and auditor.default SKILL.md to use `promotion.record`.

#### Checkpoint 4.1: Update evaluator.default SKILL.md
- [x] **File**: `autonoetic/agents/specialists/evaluator.default/SKILL.md`
- [x] Added "Recording Promotion (CRITICAL)" section requiring `promotion.record` call

#### Checkpoint 4.2: Update auditor.default SKILL.md
- [x] **File**: `autonoetic/agents/specialists/auditor.default/SKILL.md`
- [x] Added "Recording Promotion (CRITICAL)" section requiring `promotion.record` call

---

### Phase 5: Planner Workflow Update

**Goal**: Update planner.default SKILL.md to require content-linked promotion.

#### Checkpoint 5.1: Update planner.default SKILL.md agent creation flow
- [x] **File**: `autonoetic/agents/lead/planner.default/SKILL.md`
- [x] Updated Steps 3-4 to include promotion.record call requirement
- [x] Updated Step 6 to include `source_content_handle`
- [x] Added CRITICAL ENFORCEMENT section with strict rules

#### Checkpoint 5.2: Strict enforcement on evaluator/auditor failure
- [x] Added rule: If evaluator or auditor fails, DO NOT proceed to specialized_builder
- [x] Iteration loop: coder fixes → evaluator re-validates → auditor re-audits → specialized_builder

---

### Phase 6: Bootstrap Evaluator/Auditor Configuration

**Goal**: Fix the OPENAI_API_KEY → OPENROUTER_API_KEY issue for evaluator/auditor agents.

#### Checkpoint 6.1: Investigate bootstrap config propagation
- [x] **File**: `autonoetic/src/cli/agent.rs` — `resolve_llm_config` function
- [x] Found hardcoded defaults that apply wrong provider (openai instead of openrouter)
- [x] Evaluator/auditor template falls through to `_` default case

#### Checkpoint 6.2: Fix config propagation to evaluator/auditor
- [x] Added `"evaluator" | "auditor"` case to `resolve_llm_config` using openrouter/google/gemini-3-flash-preview

---

### Phase 7: Testing

**Goal**: Comprehensive test coverage for all components.

#### Checkpoint 7.1: Unit tests
- [x] `PromotionStore` CRUD operations — 6 tests passing
- [x] `promotion.record` tool args validation — in tools_promotion
- [x] `promotion.query` responses — in tools_promotion
- [x] Causal chain promotion lookup — 3 tests passing

#### Checkpoint 7.2: Integration tests
- [x] `tests/promotion_record_e2e.rs`: Full flow - 1 test passing
- [x] `tests/promotion_record_reject.rs`: Rejection scenarios - 4 tests passing
- [x] `tests/promotion_record_evaluator_fail.rs`: Evaluator fails - 2 tests passing

#### Checkpoint 7.3: Gateway verification tests
- [x] Test causal chain verification logic — in promotion_lookup tests
- [x] Test that fake promotion records are detected — in promotion_record_reject tests

---

## Files Created

| File | Status | Purpose |
|------|--------|---------|
| `autonoetic-types/src/promotion.rs` | ✅ Done | PromotionRecord, Finding types |
| `autonoetic-gateway/src/runtime/promotion_store.rs` | ✅ Done | PromotionStore implementation |
| `autonoetic-gateway/src/runtime/tools_promotion.rs` | ✅ Done | promotion.record, promotion.query tools |
| `autonoetic-gateway/src/causal_chain/promotion_lookup.rs` | ✅ Done | Causal chain verification |
| `tests/promotion_record_e2e.rs` | ✅ Done | Integration tests |
| `tests/promotion_record_reject.rs` | ✅ Done | Rejection tests |
| `tests/promotion_record_evaluator_fail.rs` | ✅ Done | Evaluator fail tests |

## Files Modified

| File | Status | Changes |
|------|--------|---------|
| `autonoetic-types/src/promotion.rs` | ✅ Done | PromotionRecord, Finding types |
| `autonoetic-types/src/lib.rs` | ✅ Done | Already exports promotion module |
| `autonoetic-gateway/src/runtime/mod.rs` | ✅ Done | Includes tools_promotion |
| `autonoetic-gateway/src/runtime/promotion_store.rs` | ✅ Done | PromotionStore implementation |
| `autonoetic-gateway/src/runtime/tools_promotion.rs` | ✅ Done | promotion.record, promotion.query tools |
| `autonoetic-gateway/src/causal_chain/promotion_lookup.rs` | ✅ Done | Causal chain verification |
| `autonoetic-gateway/src/runtime/tools.rs` | ✅ Done | InstallAgentArgs + validation |
| `autonoetic/src/cli/agent.rs` | ✅ Done | Fixed evaluator/auditor bootstrap config |
| `autonoetic/agents/specialists/evaluator.default/SKILL.md` | ✅ Done | Added promotion.record |
| `autonoetic/agents/specialists/auditor.default/SKILL.md` | ✅ Done | Added promotion.record |
| `autonoetic/agents/lead/planner.default/SKILL.md` | ✅ Done | Strict workflow |

## Security Considerations

1. **Only evaluator/auditor can record promotion** — verify manifest agent_id in tool handler
2. **Causal chain is append-only** — promotion records cannot be deleted or modified
3. **Content handle must exist** — verify handle exists in content store before recording
4. **Install must reference same content** — `source_content_handle` must match what was validated
5. **No bypass via override_approval_ref** — override_approval_ref bypasses approval, not promotion verification

## Success Metrics

1. ✅ An agent cannot be installed via `specialized_builder` without actual evaluator/auditor validation
2. ✅ Promotion records are verifiable via causal chain
3. ✅ Multi-file artifacts are validated as a unit (all files in artifact must pass)
4. ✅ Failed evaluations prevent installation (even if specialized_builder tries to fake it)
5. ✅ Evaluator/auditor use OPENROUTER_API_KEY (not OPENAI_API_KEY) — **FIXED in bootstrap**
