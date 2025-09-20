# Remaining mutation primitive inventory (short)

Generated: 2025-09-20

This file lists feature and test files that still reference mutation primitives (atom, swap!, reset!, deref/@, set!). Use this to prioritize deprecation or migration.

- rtfs_compiler/tests/shared/rtfs_files/features/parallel_expressions.rtfs
  - uses: `atom`, `swap!` (concurrent counters, logging inside `parallel` tasks)

- rtfs_compiler/tests/shared/rtfs_files/test_fault_tolerance.rtfs
  - uses: `atom`, `swap!`, `@`/deref (recovery-events atom, swap! to append events)

Notes:
- Several docs/specs and runtime files still mention `Value::Atom` or contain the old stdlib implementations; those are intentionally retained behind `legacy-atoms` feature or for documentation purposes.
- For now, the harness deprecates the two feature files above in non-legacy builds. Consider host-backed primitives or full rewrites for longer-term fixes.

Next Steps (Phase 2 - Feature Gating):
- Add cargo feature `legacy-atoms` (default = on initially)
- Move atom-related code paths behind `#[cfg(feature = "legacy-atoms")]`
- Implement compatibility shims that provide clear error messages when feature is disabled
- Test both feature-enabled and feature-disabled builds
