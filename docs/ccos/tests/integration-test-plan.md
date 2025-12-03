# Integration Testing Plan for CCOS CLI Discovery

This document outlines the integration tests required to verify the end-to-end flow of the CCOS CLI discovery, approval, and introspection features.

## Test Scenarios

### 1. Discovery Flow
- **Goal-Driven Discovery**:
  - `ccos discover goal "..."` should find relevant servers from registries.
  - Results should be filtered by score threshold.
  - Interactive mode should prompt for selection.
  - Non-interactive mode should queue top results automatically.

- **Direct Server Discovery**:
  - `ccos discover server "name"` or `ccos server introspect "name"` should work.

### 2. Approval Queue Management
- **Pending List**: `ccos approval pending` should list queued items.
- **Approval**: `ccos approval approve <id>` should:
  - Move server to approved list.
  - Move capability files from `pending/` to `approved/`.
  - Update `capabilities_path` in server info.
- **Rejection**: `ccos approval reject <id>` should move to rejected list.

### 3. Edge Cases (The "Refinement" Phase)
- **Duplicate Discovery**:
  - Discovering a server already in `pending` should update the existing entry (extend expiration), not duplicate it.
- **Re-discovering Approved Server**:
  - Discovering a server already in `approved` should:
    - Prompt user (in interactive mode): Merge vs Skip.
    - If merged: Remove from `approved`, move capabilities to `pending`, add to `pending` list.
- **Authentication**:
  - Missing token should prompt user or fail gracefully with instructions.
  - Valid token should succeed.

### 4. Capability Persistence
- **Serialization**:
  - Saved `.rtfs` files should follow canonical schema.
  - `:input-schema` and `:output-schema` must be inside the capability map.
  - Unknown outputs should be `:any`, not `nil`.

## Implementation Strategy

Create a new integration test file `ccos/tests/cli_discovery_test.rs` using `assert_cmd` to run the CLI binary against a temporary test directory.

```rust
use assert_cmd::Command;
use predicates::prelude::*;
use std::fs;

#[test]
fn test_discovery_flow() {
    // Setup temp dir for config/capabilities
    // ...

    // 1. Discover
    let mut cmd = Command::cargo_bin("ccos").unwrap();
    cmd.arg("discover").arg("goal").arg("test goal")
       .assert()
       .success();

    // Verify pending.json content
    // ...

    // 2. Approve
    // ...
}
```

