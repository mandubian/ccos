# Capability Marketplace Worktree Bootstrap

Worktree: wt/capability-marketplace — planned — Capability Marketplace: initialize with discovered capabilities at startup (#118), [CCOS] Dynamic capability discovery (Capability System) (#19), Agent Marketplace: Create marketplace with isolation requirements (#67)

Source issue: https://github.com/mandubian/ccos/issues/120

## Goal
Stand up a Capability Marketplace that:
- Initializes at startup with discovered capabilities (scan/registry bootstrap) (#118)
- Supports dynamic capability discovery/registration lifecycle (#19)
- Paves the way for an Agent Marketplace with isolation policies/enforcement (#67)

## Quick start
- Repo worktree path: this directory
- Build/tests (in rtfs_compiler/):
  - cargo test --test integration_tests -- --nocapture --test-threads 1
- Suggested workflow:
  1) Add/extend capability registry and marketplace APIs
  2) Implement startup discovery/bootstrap wiring
  3) Add integration tests asserting marketplace pre-population and dynamic discovery
  4) Wire isolation requirements hooks (policy surface; enforcement later if out-of-scope)

## Code pointers
- Runtime marketplace broker: `rtfs_compiler/src/runtime/capability_marketplace/`
- Capability registry: `rtfs_compiler/src/runtime/capability_registry.rs`
- Host + CCOS integration hooks: `rtfs_compiler/src/runtime/host.rs` and `src/ccos/` modules
- Delegation/agent discovery context: `src/ccos/delegation.rs`, `src/agent/registry.rs`
- Causal chain for auditing: `src/ccos/causal_chain.rs`
- Tests entry: `rtfs_compiler/tests/integration_tests.rs`

## Minimal milestone plan
1) Bootstrap on startup
   - On runtime init, load built-in capabilities from registry
   - Add discovery providers (static, file-based manifest, optional network placeholder)
   - Expose a read API to list capabilities with metadata
2) Dynamic discovery
   - Add capability provider interface; allow registering/unregistering at runtime
   - Emit audit entries on changes (CausalChain)
3) Isolation surface
   - Define capability isolation policies (namespaces, allow/deny lists, resource constraints)
   - Validate calls via CapabilityMarketplace broker before invocation
4) Tests
   - Integration: marketplace initializes with N capabilities
   - Dynamic: registering a new capability makes it invokable
   - Security: denied capability fails with explicit error and audit event

## Definition of done
- Marketplace pre-populates on startup and is queryable
- Dynamic add/remove works with audit
- Isolation policy check enforces allow/deny
- Tests are green and CI-safe (no external network/file deps by default)
- Minimal docs updated in `docs/` with capability lifecycle notes

## Notes
- Keep side effects via marketplace only (see CCOS/RTFS capability rules)
- Prefer small, incremental PRs with integration tests
