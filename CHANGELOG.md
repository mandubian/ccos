# Changelog — wt/archive-storage

## 2025-08-23 — File-backed archive + backups (v0.1)
- Implemented FileArchive with two-level sharding and `index.json` mapping.
- Implemented atomic write helper with parent-directory fsync.
- Implemented StorageBackupData v1.1 (hybrid JSON+RTFS) and updated FileStorage/InMemoryStorage backups.
- Added integration tests for IntentGraph backup/restore using file storage.
- Added sqlite scaffold for ContentAddressableArchive and focused tests.
- Documentation added to `docs/ccos/specs` describing on-disk layout and backup format.

## Notes
- The sqlite backend in this branch is a scaffold and may need performance hardening.
- Full crate tests show unrelated failures on the current machine; storage-focused tests passed.
