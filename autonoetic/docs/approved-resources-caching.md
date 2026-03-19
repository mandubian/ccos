# Plan: Approved Sandbox Exec Replay Cache

## Problem Statement

Currently, every `sandbox.exec` call that contains network access patterns requires a new approval, even if the agent has already been approved to execute the same code. This creates:

1. **Approval fatigue** - users start auto-approving everything
2. **Redundant friction** - same code = repeated prompts
3. **Poor UX** - legitimate repeated operations are blocked

## Solution

Cache approved sandbox exec fingerprints so identical future executions skip creating new approval requests.

## Scope

Phase 1 covers only `sandbox.exec` with network detection. Phase 2 (not in this plan) will add TTL, capability-diff rules, and revocation UX.

## Design

### Trust Model: Fingerprint-Only

**Reuse key = SHA256(agent_id + normalized_remote_targets + code_fingerprint)**

- `agent_id`: the agent requesting execution
- `normalized_remote_targets`: sorted list of concrete hosts detected in code
- `code_fingerprint`: SHA256 of the **analyzed script content**, not the raw command

**The fingerprint must use the same `code_to_analyze` payload that `RemoteAccessAnalyzer::analyze_code()` receives.** The raw command (e.g., `python3 fetch.py`) may reference external files, so the cache key must be based on the actual script content that was analyzed. Two calls are identical only if the analyzed code content is identical.

**Host-wide approval (approve all URLs on a host) is a separate approval class for Phase 2.**

### Single Source of Truth

Store only in gateway-owned location. No agent-local files.

```
{gateway_root}/scheduler/approvals/
├── pending/                   # Existing: pending ApprovalRequest
├── approved/                  # Existing: approved ApprovalDecision
├── rejected/                  # Existing: rejected ApprovalDecision
└── exec_cache/                # NEW: approved exec fingerprints
    └── {fingerprint}.json     # One file per cached approval
```

### Cache Record

```json
{
  "fingerprint": "sha256:abc123...",
  "agent_id": "my.agent",
  "remote_targets": ["api.example.com", "status.github.com"],
  "code_fingerprint": "sha256:def456...",
  "code_snippet": "import requests\nrequests.get('https://api.example.com')",
  "approval_request_id": "apr-xyz789",
  "approved_at": "2026-03-18T12:00:00Z",
  "approved_by": "operator",
  "last_used_at": "2026-03-18T14:00:00Z"
}
```

Note: `remote_targets` contains only **concrete hosts** extracted from URL literals (e.g., `api.example.com` from `https://api.example.com/data`).

**Cache only when ALL remote access evidence is concrete.** Import statements and function calls are treated as opaque indicators of network access intent and prevent caching even when concrete targets also exist. This ensures that code like `import requests; requests.get("https://api.example.com")` (which produces import + url_literal patterns) is NOT cached, because the import could also make other network calls at runtime.

## Phase 1: Implementation [COMPLETE]

### 1.1: Types

**File**: `autonoetic-gateway/src/runtime/approved_exec_cache.rs` (new in gateway, not types)

- [x] `ApprovedExecCache` struct
- [x] `ApprovedExecEntry` struct (matches cache record above)
- [x] `compute_fingerprint(agent_id, targets, code_to_analyze) -> String` — uses the same `code_to_analyze` that `RemoteAccessAnalyzer::analyze_code()` receives
- [x] `normalize_targets(patterns) -> Vec<String>` — extract hosts from `DetectedPattern`, sort/deduplicate, strip paths
- [x] `has_concrete_targets(patterns) -> bool` — returns true only if ALL patterns are concrete (url_literal or ip_address); returns false if ANY pattern is opaque (import or function_call), even when concrete targets also exist

### 1.2: Store

**File**: `autonoetic-gateway/src/runtime/approved_exec_cache.rs`

- [x] `ApprovedExecCache::new(gateway_dir: &Path) -> Self`
- [x] `record(&self, entry: ApprovedExecEntry) -> Result<()>`
- [x] `find(&self, fingerprint: &str) -> Option<ApprovedExecEntry>`
- [x] `update_last_used(&self, fingerprint: &str) -> Result<()>`

### 1.3: Integration

**File**: `autonoetic-gateway/src/runtime/tools.rs` (modify `sandbox_exec`)

The write point is explicitly **after successful execution with a valid approval_ref**:

1. Agent calls `sandbox.exec` with code containing network patterns
2. Gateway creates approval request, returns `approval_required: true, request_id: apr-xxx`
3. Operator approves via gateway API
4. Agent retries with `approval_ref: apr-xxx`
5. Gateway validates `approval_ref`, executes successfully
6. **Gateway writes to exec_cache** (only if concrete targets exist and execution succeeded)
7. Future identical calls skip to step 7 directly

**In the initial path (where approval is created):**

- [x] Run `RemoteAccessAnalyzer::analyze_code()` to get detected patterns
- [x] If `!has_concrete_targets(detected_patterns)`, proceed with existing approval flow (do not cache)
- [x] Compute fingerprint using `code_to_analyze`
- [x] If cache hit, skip approval creation, execute immediately
- [x] If cache miss, proceed with existing approval flow

**In the retry path (where approval_ref is validated):**

- [x] Validate `approval_ref`
- [x] Execute
- [x] On success: record to cache (only if concrete targets exist)
- [x] On failure: do not record

### 1.4: No New Tool

Do NOT add `resource.revoke` as a runtime tool. Revocation is handled via Phase 2 gateway CLI.

## Files to Create/Modify

### New Files

| File | Purpose |
|------|---------|
| `autonoetic-gateway/src/runtime/approved_exec_cache.rs` | Cache store + types |

### Files to Modify

| File | Changes |
|------|---------|
| `autonoetic-gateway/src/runtime/mod.rs` | Include new module |
| `autonoetic-gateway/src/runtime/tools.rs` | Check/write cache in sandbox_exec |

## Testing

### Integration tests

- [x] `tests/approved_exec_cache_integration.rs`: Cache record/find/persistence tests
- [x] `tests/approved_exec_cache_integration.rs`: has_concrete_targets tests (unit + integration)
- [x] `tests/approved_exec_cache_integration.rs`: normalize_targets tests
- [x] `tests/approved_exec_cache_integration.rs`: compute_fingerprint tests
- [x] `tests/approved_exec_cache_integration.rs`: Cache full cycle test (miss → record → hit → miss for different code)
- [x] `tests/approved_exec_cache_integration.rs`: Opaque targets never cached test

## Security Considerations

1. **Fingerprint is a cache key, not a security boundary** - no secrets involved
2. **Same agent, same analyzed code, same concrete targets** required for reuse
3. **All remote evidence must be concrete to cache** - if any detected pattern is opaque (imports, dynamic URLs), approval is always required; this prevents partial concrete hosts from masking unchecked opaque access
4. **Original approval still creates audit trail** - linked via `approval_request_id`
5. **Capability changes not tracked in Phase 1** - deferred to Phase 2

## Out of Scope for Phase 1

- TTL / expiry (Phase 2)
- Capability change detection (Phase 2)
- Host-wide approval class (Phase 2)
- Revocation CLI/API (Phase 2)

## Success Metrics

1. Identical exec called twice → second call skips approval creation
2. Different code → new approval required
3. Original approval linked in causal chain
4. No performance regression

---

## Appendix: Phase 2 (Future)

These features are deferred to Phase 2:

### TTL / Expiry
```yaml
resource_approval:
  ttl_secs: 86400  # 24 hours, None = no expiry
```

### Capability Change Detection
- Store capabilities snapshot at approval time
- If capabilities changed significantly, require re-approval

### Host-Wide Approval
- Separate approval class: "approve all URLs on host X"
- More convenient but broader trust

### Revocation
- Gateway CLI: `autonoetic approvals revoke-cache <fingerprint>`
- Or API endpoint for management UI
