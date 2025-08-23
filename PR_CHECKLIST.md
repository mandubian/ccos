# PR Checklist — wt/archive-storage

- [ ] Review file archive implementation in `rtfs_compiler/src/ccos/storage_backends/file_archive.rs`.
- [ ] Verify index persistence semantics (look at `index.json` format and save/load mechanics).
- [ ] Validate atomic write behavior (temp-file + fsync + rename + parent-dir fsync).
- [ ] Check `StorageBackupData v1.1` format in `rtfs_compiler/src/ccos/intent_storage.rs` and ensure `rtfs` is optional.
- [ ] Run focused storage tests (see `PR_DESCRIPTION.md` for commands).
- [ ] Review sqlite scaffold (`rtfs_compiler/src/ccos/storage_backends/sqlite_archive.rs`) and accompanying tests.
- [ ] Review docs in `docs/ccos/specs` and `WORKTREE_COMPLETION_REPORT.md`.
