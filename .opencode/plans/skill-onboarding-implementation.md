# Skill Onboarding Implementation Plan

**Date**: 2026-02-01
**Status**: Ready for Implementation
**Scope**: Phase 1 - Core Primitives

## Overview

This plan implements the foundational capabilities for multi-step skill onboarding as specified in `docs/new_arch/spec-skill-onboarding.md`. The goal is to enable agents to onboard complex skills with human-in-the-loop steps while maintaining CCOS security guarantees.

## Files to Modify

### 1. `ccos/src/approval/types.rs`

**Add new approval categories:**

```rust
/// Secret write approval - storing a new secret value
/// Value is never logged; only key and scope are tracked
SecretWrite {
    /// Secret key name (e.g., "MOLTBOOK_SECRET")
    key: String,
    /// Scope: "skill" or "global"
    scope: String,
    /// Skill ID for skill-scoped secrets
    #[serde(default, skip_serializing_if = "Option::is_none")]
    skill_id: Option<String>,
    /// Human-readable description of the secret's purpose
    description: String,
},

/// Human action request approval - requires human intervention
/// for onboarding steps like OAuth, email verification, tweet verification
HumanActionRequest {
    /// Action type identifier (e.g., "tweet_verification", "oauth_consent")
    action_type: String,
    /// Short title for the approval UI
    title: String,
    /// Detailed markdown instructions for the human
    instructions: String,
    /// JSON schema for validating the human's response
    required_response_schema: serde_json::Value,
    /// Timeout in hours before the request expires
    timeout_hours: i64,
    /// Skill that needs this human action
    skill_id: String,
    /// Onboarding step identifier
    step_id: String,
},
```

**Extend `ApprovalRequest` struct:**

```rust
pub struct ApprovalRequest {
    // ... existing fields ...
    /// Human-provided response data (for HumanActionRequest completions)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub response: Option<serde_json::Value>,
}
```

**Add Display implementations:**

```rust
ApprovalCategory::SecretWrite { key, .. } => write!(f, "SecretWrite({})", key),
ApprovalCategory::HumanActionRequest { action_type, skill_id, .. } => {
    write!(f, "HumanActionRequest({} for {})", action_type, skill_id)
}
```

### 2. Create `ccos/src/skills/onboarding_capabilities.rs`

This new file will contain all onboarding-related capabilities:

#### Capability: `ccos.secrets.set`

**Purpose**: Store a secret securely with governance approval

**Input Schema:**
```rust
struct SecretSetInput {
    key: String,           // Secret name (e.g., "MOLTBOOK_SECRET")
    value: String,         // Secret value (never logged)
    scope: String,         // "skill" or "global"
    skill_id: Option<String>, // Required if scope is "skill"
    description: String,   // Human-readable purpose
}
```

**Output Schema:**
```rust
struct SecretSetOutput {
    success: bool,
    approval_id: Option<String>, // Present if requires_approval
    message: String,
}
```

**Implementation:**
1. Always creates a `SecretWrite` approval (as per user requirement)
2. Value is never logged or stored in approval
3. On approval, calls `SecretStore::set_local()`
4. For skill scope, prefixes key with `skill.{skill_id}.`
5. Saves to `.ccos/secrets.toml` with restrictive permissions

#### Capability: `ccos.memory.store`

**Purpose**: Store key-value data in working memory for onboarding state

**Input Schema:**
```rust
struct MemoryStoreInput {
    key: String,           // Memory key (e.g., "moltbook.agent_id")
    value: serde_json::Value, // Value to store (JSON-serializable)
    skill_id: Option<String>, // For namespacing
    ttl: Option<u64>,      // Optional TTL in seconds
}
```

**Output Schema:**
```rust
struct MemoryStoreOutput {
    success: bool,
    entry_id: String,      // Full entry ID used
}
```

**Implementation:**
1. Constructs entry ID: `"skill:{skill_id}:{key}"` or `"global:{key}"`
2. Creates `WorkingMemoryEntry` with:
   - `id`: the constructed entry ID
   - `title`: key name
   - `content`: JSON-serialized value
   - `tags`: `["onboarding", "skill:{skill_id}"]`
   - `meta.extra["ttl"]`: TTL if provided
3. Calls `WorkingMemory::append(entry)`
4. Audit logged

#### Capability: `ccos.memory.get`

**Purpose**: Retrieve value from working memory

**Input Schema:**
```rust
struct MemoryGetInput {
    key: String,           // Memory key
    skill_id: Option<String>, // For namespacing
    default: Option<serde_json::Value>, // Default if not found
}
```

**Output Schema:**
```rust
struct MemoryGetOutput {
    value: Option<serde_json::Value>,
    found: bool,
    expired: bool,         // True if entry exists but TTL expired
}
```

**Implementation:**
1. Constructs entry ID: `"skill:{skill_id}:{key}"` or `"global:{key}"`
2. Calls `WorkingMemory::get(&entry_id)`
3. If found:
   - Check TTL in `meta.extra["ttl"]`
   - If expired: return `found: true, expired: true, value: None`
   - Otherwise deserialize content and return
4. If not found:
   - Return `found: false, expired: false, value: default`

#### Capability: `ccos.approval.request_human_action`

**Purpose**: Request human intervention for onboarding steps

**Input Schema:**
```rust
struct RequestHumanActionInput {
    action_type: String,        // "tweet_verification", "email_verification", etc.
    title: String,              // Short UI title
    instructions: String,       // Detailed markdown instructions
    required_response: serde_json::Value, // JSON schema for response
    timeout_hours: Option<i64>, // Default: 24
    skill_id: String,           // Skill needing this action
    step_id: String,            // Onboarding step
}
```

**Output Schema:**
```rust
struct RequestHumanActionOutput {
    approval_id: String,
    status: String,             // "pending"
    expires_at: String,         // ISO 8601 timestamp
}
```

**Implementation:**
1. Creates `ApprovalCategory::HumanActionRequest` with:
   - All input fields
   - Default timeout: 24 hours
   - RiskAssessment: Medium (configurable)
2. Calls `UnifiedApprovalQueue::add(request)`
3. Returns approval_id for polling
4. Audit logged

#### Capability: `ccos.approval.complete`

**Purpose**: Complete a human action with response data

**Input Schema:**
```rust
struct CompleteHumanActionInput {
    approval_id: String,        // ID from request_human_action
    response: serde_json::Value, // Human-provided response
}
```

**Output Schema:**
```rust
struct CompleteHumanActionOutput {
    success: bool,
    validation_errors: Vec<String>, // If response doesn't match schema
    message: String,
}
```

**Implementation:**
1. Retrieves approval via `UnifiedApprovalQueue::get(&approval_id)`
2. Validates it exists and is `HumanActionRequest` category
3. Validates response against `required_response_schema`
4. If valid:
   - Sets approval status to Approved
   - Stores response in `approval.response` field
   - Also stores in WorkingMemory: `key = "approval:{approval_id}:response"`
   - Updates via `UnifiedApprovalQueue::update(&approval)`
5. Returns success/failure with validation errors if any
6. Audit logged

### 3. Update `ccos/src/skills/mod.rs`

Add module export:

```rust
pub mod onboarding_capabilities;
```

### 4. Update `ccos/src/skills/capabilities.rs`

Add registration of onboarding capabilities:

```rust
use crate::skills::onboarding_capabilities;

pub async fn register_skill_capabilities(
    marketplace: Arc<CapabilityMarketplace>,
    skill_mapper: Arc<Mutex<SkillMapper>>,
) -> RuntimeResult<()> {
    // ... existing skill capabilities ...
    
    // Register onboarding capabilities
    onboarding_capabilities::register_onboarding_capabilities(
        marketplace.clone(),
        secret_store,
        working_memory,
        approval_queue,
    ).await?;
    
    Ok(())
}
```

## Dependencies Required

Capabilities need access to:
- `SecretStore` (for `ccos.secrets.set`)
- `WorkingMemory` (for `ccos.memory.store/get`)
- `UnifiedApprovalQueue` (for human action management)

These should be passed during registration or accessed via the CCOS runtime context.

## Testing Strategy

Create integration tests in `tests/integration_tests.rs`:

1. **Test `ccos.secrets.set`:**
   - Call capability with secret
   - Verify SecretWrite approval created
   - Approve and verify secret stored
   - Verify value never appears in logs

2. **Test `ccos.memory.store/get`:**
   - Store value with key
   - Retrieve and verify
   - Test with TTL and expired flag
   - Test default value when not found

3. **Test `ccos.approval.request_human_action`:**
   - Request human action
   - Verify HumanActionRequest approval created
   - Verify instructions present

4. **Test `ccos.approval.complete`:**
   - Complete human action with response
   - Verify response stored in approval
   - Verify response also in WorkingMemory
   - Test schema validation failure

## Implementation Order

1. Add approval categories to `types.rs`
2. Extend `ApprovalRequest` with response field
3. Create `onboarding_capabilities.rs` with all capabilities
4. Update `mod.rs` and `capabilities.rs` for registration
5. Add Display implementations
6. Write integration tests
7. Test end-to-end with Moltbook example flow

## Success Criteria

- [ ] All five capabilities registered and callable
- [ ] Secret values never logged
- [ ] Memory operations use WorkingMemory correctly
- [ ] Human action approvals work end-to-end
- [ ] Schema validation on completion
- [ ] Integration tests pass
- [ ] Expired flag works for TTL entries

## Notes

- Secret scope "skill" prefixes key with skill name for isolation
- Human action response stored in both approval (for audit) and memory (for easy access)
- All capabilities use existing CCOS infrastructure (no new structures)
- Approval system provides governance umbrella for all operations

## Related Files

- `docs/new_arch/spec-skill-onboarding.md` - Full specification
- `ccos/src/secrets/secret_store.rs` - Secret storage
- `ccos/src/working_memory/facade.rs` - Working memory interface
- `ccos/src/approval/unified_queue.rs` - Approval queue
