File Backend Spec
==================

Purpose
-------
Describes the on-disk layout, index format, and operation semantics for the file-based content-addressable archive implemented in `rtfs_compiler::ccos::storage_backends::file_archive::FileArchive`.

On-disk layout
--------------
Base directory (configured per FileArchive) contains:
- `index.json` — mapping from content-hash -> relative file name/path
- `aa/bb/<hash>.json` — sharded two-level layout by first 4 hex characters (fan-out to avoid large dirs)
- `<hash>` — legacy fallback if no index entry exists (for older archives)

Index format (`index.json`)
---------------------------
- JSON object mapping content-hash (string) -> relative path (string)
- Example:
  {
    "3a7f...": "plans/3a7f....json",
    "9b2c...": "intents/9b2c....json"
  }

Why an index? — Rationale
-------------------------
- Allows deterministic, human-friendly layout (group by entity type)
- Enables renaming/migration of files without changing content hashes
- Accelerates listing and size calculation without rescanning all files

Store semantics
---------------
- `store(entity)`:
  - Calculate content hash using entity.content_hash()
  - If index contains hash and file exists, no-op and return hash
  - Else serialize entity to JSON and write to `base_dir/aa/bb/<hash>.json` (atomic write: temp + fsync + rename)
  - Update `index.json` with the mapping and persist index atomically (temp + fsync + rename)

Retrieve semantics
------------------
- `retrieve(hash)`:
  - Look up `index.json` for path; if present read file and deserialize
  - If not present, check `base_dir/<hash>` as fallback

Integrity verification
----------------------
- `verify_integrity()` recomputes hash for each stored file and ensures it matches the mapping in `index.json` (or filename), returning Ok(true) only if all match.

Versioning & Manifest
---------------------
- Backups created by IntentStorage::backup(path) include:
  - `version` (string)
  - `timestamp` (unix seconds)
  - `intents` (map intent id -> StoredIntent JSON)
  - `edges` (list)
  - `manifest` (optional metadata map)

Notes & Limitations
-------------------
- This backend is not optimized for extremely large datasets. For large-scale use, prefer a database backend (sqlite, postgres) or object store backed by index sharding.
- Concurrent writers to the same directory may cause race conditions; callers should coordinate writes or use locking where necessary.
- Atomicity relies on POSIX rename semantics; for extra safety on crash consistency, fsync the containing directory after rename in environments that require it.

See also: `docs/ccos/specs/storage_backend.md`, `docs/ccos/specs/intent_backup_format.md`
