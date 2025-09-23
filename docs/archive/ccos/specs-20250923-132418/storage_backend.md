Storage Backend Spec
=====================

Purpose
-------
This document describes the storage backend API surface used by CCOS and the design constraints for pluggable backends (file, sqlite, S3, etc.). It documents the existing traits and the minimal contract a backend must satisfy.

Scope
-----
- Content-addressable immutable storage for archivable entities (plans, actions, checkpoints).
- Intent graph persistence (intents, edges) with backup/restore and health checks.
- Deterministic on-disk layout and stable content hashes across backends.

Core Contracts
--------------
1) Archivable
- Rust trait: `ccos::storage::Archivable`
- Requirements: Debug + Clone + Serialize + for<'de> Deserialize<'de> + Send + Sync
- Methods:
  - `fn content_hash(&self) -> String` — sha256(json(self)) by default
  - `fn entity_id(&self) -> String`
  - `fn entity_type(&self) -> &'static str`

2) ContentAddressableArchive<T: Archivable>
- Rust trait: `ccos::storage::ContentAddressableArchive<T>`
- Responsibilities:
  - `fn store(&self, entity: T) -> Result<String, String>` — store and return content-hash
  - `fn retrieve(&self, hash: &str) -> Result<Option<T>, String>`
  - `fn exists(&self, hash: &str) -> bool`
  - `fn stats(&self) -> ArchiveStats`
  - `fn verify_integrity(&self) -> Result<bool, String>`
  - `fn list_hashes(&self) -> Vec<String>`

3) IntentStorage
- Rust trait: `ccos::intent_storage::IntentStorage`
- Responsibilities (async): store/get/update/delete intents, store/get/delete edges, backup/restore, health_check
- Backup format documented separately (see `intent_backup_format.md`)

Design Constraints
------------------
- Content-addressing must be stable across backends: `content_hash()` must compute identical hashes for logically identical entities across implementations.
- Backends must be thread-safe and suitable for use behind trait objects.
- Avoid introducing additional un-audited fields that affect hashing without a migration plan.

Extensibility
-------------
- Backends should register under `ccos::storage_backends` and implement the above traits.
- Backends that provide both content-addressable and graph persistence (for example a sqlite-backed implementation) may provide multiple types implementing the distinct traits but should document how they are wired into `StorageFactory` or initialization code.

Audit & Verification
--------------------
- Implementations should provide `verify_integrity()` which checks stored bytes against recomputed hashes and returns Ok(true) when consistent.
- Backends should persist a minimal manifest (timestamp, version, implementation id) when writing backups.

Compatibility & Migration
-------------------------
- When changing the JSON serialization that affects content hashes, a migration note must be added to `docs/ccos/specs` and tests added to validate the change. The CRC/hash computation must remain stable across minor code refactors.

See also
--------
- `docs/ccos/specs/file_backend.md`
- `docs/ccos/specs/intent_backup_format.md`

Hybrid Backups and Operational Guidance
---------------------------------------
- Backup format: this repo uses a hybrid JSON+RTFS backup (v1.1). The JSON is the canonical, machine-parseable snapshot; the embedded `rtfs` string is a human-readable RTFS snapshot useful for diffs and portability across toolchains.
- Atomic writes: producers should write backups and index files using a temporary file in the same directory, fsync the file, then rename to the final path. Optionally fsync the directory on platforms where that's meaningful.
- Index behavior: file-based backends should persist an `index.json` mapping `content-hash -> relative-path`. Implementations should update the in-memory index before persisting and ensure the persistence is atomic.
- Hash stability: implementors must not change the default `content_hash()` behavior (sha256 over serde_json::to_string(self)) without a documented migration and test suite validating cross-backend stability.

StorageFactory wiring (note)
---------------------------
- `StorageFactory` should remain the single entrypoint for configurable backends. When adding a sqlite or DB-backed implementation, provide a clear migration path and a `StorageFactory::migrate_from_file(path)` helper that can consume existing `index.json` or backup files and import entities deterministically.
