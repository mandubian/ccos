# PR: wt/archive-storage — File-backed content-addressable archive + backups

Branch: wt/archive-storage

Summary
- Adds a deterministic, file-backed ContentAddressableArchive with two-level sharding
  (aa/bb/<hash>.json), an on-disk `index.json` mapping, and robust atomic writes
  (temp-file write + file fsync + rename + parent-directory fsync).
- Adds a hybrid JSON+RTFS backup format (StorageBackupData v1.1) and updates
  `FileStorage` and `InMemoryStorage` to produce atomic backups that include an
  optional human-readable RTFS snapshot and a small manifest.
- Adds an integration test exercising IntentGraph backup & restore using the
  file-backed storage. Adds a sqlite-backed archive scaffold (thread-safe via
  Arc<Mutex<Connection>>) and focused tests for sqlite store/retrieve and
  hash-stability across backends.

Files changed (high level)
- `rtfs_compiler/src/ccos/storage_backends/file_archive.rs` — new/updated file archive
- `rtfs_compiler/src/ccos/intent_storage.rs` — backup format v1.1 + atomic backup writes
- `rtfs_compiler/src/ccos/storage_backends/sqlite_archive.rs` — sqlite scaffold + tests
- `rtfs_compiler/src/tests/intent_storage_tests.rs` — integration test for IntentGraph backup/restore
- `docs/ccos/specs/` — documentation updated/added describing file backend and backup format
- `WORKTREE_COMPLETION_REPORT.md` — completion report for this worktree

What I verified locally
- Focused unit tests for the new file archive and sqlite scaffold passed locally.
- Integration test `intent_graph_backup_and_restore_file_storage` passed in focused runs.
- File-level atomic write and parent-dir fsync implemented and used for index and backups.

Suggested commands to reproduce the focused storage test runs
```bash
cd rtfs_compiler
# file archive unit test
cargo test ccos::storage_backends::file_archive::tests::test_file_archive_store_and_retrieve -- --nocapture
# intent graph backup/restore integration test
cargo test tests::intent_storage_tests::intent_graph_backup_restore_integration::test_intent_graph_backup_and_restore_file_storage -- --nocapture
# sqlite focused tests (if you added sqlite feature dependency)
cargo test ccos::storage_backends::sqlite_archive::tests::test_sqlite_store_and_retrieve -- --nocapture
```

Known issues / CI notes
- The full crate test-suite currently reports unrelated failures in parser/arbiter
  tests on this host; those failures are outside the scope of this storage work
  (they affect other subsystems). The storage-focused tests above passed.
- SQLite implementation is an initial scaffold that serializes DB access via
  `Arc<Mutex<Connection)>` for simplicity; consider adding a connection pool
  (r2d2 or similar) later for performance.

Reviewer guidance
- Review the file archive layout and `index.json` usage first — that's the
  critical change for the archive behavior.
- The backup format is intentionally hybrid: canonical JSON is authoritative for
  restore; the `rtfs` field is optional and human-friendly.
- See `docs/ccos/specs` for a short rationale and operational notes.

Next steps (post-merge)
- Optionally replace sqlite connection locking with a pool for performance.
- Add more migration & cross-backend end-to-end tests if you want DB-backed
  archives in production.

---

Files to include in PR (suggested): full diff in this branch; include the
`WORKTREE_COMPLETION_REPORT.md` and docs under `docs/ccos/specs` for context.
