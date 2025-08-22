# wt/archive-storage — bootstrap

Planned scope (from issues):
- File-based Storage Backend for Unified Archive (#73)
- Database Storage Backend for Unified Archive (#74)
- Archive Manager Integration with Unified Storage (#75)
- Intent Graph Archive Integration (#76)

Source issue: https://github.com/mandubian/ccos/issues/120

Initial tasks:
- [ ] Define a storage backend trait abstraction and API surface for the Unified Archive.
- [ ] Implement a simple file-based backend with deterministic paths and versioning.
- [ ] Add integration tests that exercise archiving and retrieval of IntentGraph snapshots.
- [ ] Implement a database-backed backend (sqlite) and a migration test harness.

Notes:
Aim for a pluggable storage layer. Keep interfaces small and test-driven; ensure consistent metadata and hash stability across backends.
