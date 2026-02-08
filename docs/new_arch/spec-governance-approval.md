# Governance Approval Flow for CCOS Capabilities

## Goal
Implement a multi-stage approval flow for capabilities that have side effects (e.g., network, IO), ensuring an administrator can review and approve them before they are usable by agents.

## User Review Required
> [!IMPORTANT]
> This change introduces a "Pending Approval" state. New capabilities with `Effectful` side effects (e.g., network access) will be **BLOCKED** until an administrator manually approves them.

## Implementation Details

### 1. Capability Types
#### [MODIFY] [capability_marketplace/types.rs](file:///home/mandubian/workspaces/mandubian/ccos/ccos/src/capability_marketplace/types.rs)
- Added `ApprovalStatus` enum: `Pending`, `Approved`, `Revoked`, `AutoApproved`.
- Added `approval_status` field to `CapabilityManifest`.
- `EffectType::Effectful` capabilities default to `Pending` status.

### 2. Marketplace Gating & Persistence
#### [MODIFY] [capability_marketplace/marketplace.rs](file:///home/mandubian/workspaces/mandubian/ccos/ccos/src/capability_marketplace/marketplace.rs)
- **Runtime Persistence**: Integrated `RuntimeApprovalStore` (from `approval/runtime_state.rs`) to persist approval states to disk (JSON).
- **Enforcement**: In `execute_capability_with_metadata`, the marketplace checks `RuntimeApprovalStore` first, then falls back to `manifest.approval_status`.
    - If status is `Pending` or `Revoked` for an `Effectful` capability, execution is blocked with `RuntimeError`.
- **Management Methods**:
    - `configure_approval_store(path)`: Loads approvals from file.
    - `update_approval_status(id, status)`: Updates and saves status.
    - `get_effective_approval_status(id)`: Resolves current status.
    - `list_capabilities()`: Returns all registered capabilities.

#### Configuration
- The approval store path is configurable via `agent_config.storage.approvals_dir`.
- Defaults to `~/.ccos/approvals.json` (or similar depending on workspace root).

### 3. Administrative Interface (`ccos-admin`)
#### [NEW] [bin/ccos_admin.rs](file:///home/mandubian/workspaces/mandubian/ccos/ccos/src/bin/ccos_admin.rs)
A CLI tool to manage capability approvals.

**Usage:**
```bash
cargo run --bin ccos_admin -- <COMMAND>
```

**Commands:**
- `list-pending`: Lists capabilities with `Pending` status.
- `list-all`: Lists all capabilities with their current status.
- `approve <id>`: Sets status to `Approved`.
- `reject <id>`: Sets status to `Revoked`.

### 4. Integration
- `ccos-mcp` initializes the `CapabilityMarketplace` with the configured approval store path.
- Demo binaries (`resolve-deps`, `sandbox-hardened-demo`) initialize manifests with explicit `approval_status`.

## Verification

### Automated Tests
- `ccos` library compiles successfully with new types and marketplace logic.
- `ccos-admin` binary compiles successfully.
- `test_sandboxed_capability` passes.

### Manual Verification
- Verified `ccos-admin` commands against the runtime store.
- Validated that `Pending` capabilities are correctly identified.
