Intent Graph Backup Format
==========================

Purpose
-------
Document the JSON backup format used by `IntentStorage::backup(path)` and `IntentStorage::restore(path)` to persist an IntentGraph snapshot.

Top-level JSON schema (v1.1)
----------------------------
- version: string (e.g., "1.1")
- timestamp: integer (unix seconds)
- intents: object (map from intent_id -> StorableIntent JSON)
- edges: array of Edge JSON objects
- manifest: optional object with metadata (e.g., created_by, source, note)
- rtfs: optional string — RTFS snapshot of the intent graph for human diffing and portability

Example
-------
{
  "version": "1.1",
  "timestamp": 1692796800,
  "intents": {
    "intent_123": { /* StorableIntent fields */ },
    "intent_456": { /* StorableIntent fields */ }
  },
  "edges": [
    { "from": "intent_123", "to": "intent_456", "edge_type": "DependsOn", "metadata": null, "weight": 1.0 }
  ],
  "manifest": { "created_by": "rtfs_compiler", "source": "file_storage", "note": "periodic backup" },
  "rtfs": "(intent-graph (intents (intent {:id \"intent_123\" :goal \"...\" ...})) (edges (edge {:from \"intent_123\" :to \"intent_456\" :type \"DependsOn\"})))\n"
}

Compatibility & Migration
-------------------------
- Version 1.1 adds optional fields `manifest` and `rtfs`.
- Restores are tolerant of unknown and missing optional fields (serde default), enabling forward/backward compatibility with 1.0 and 1.1.
- When introducing new fields into `StorableIntent` or `Edge`, update the `version` and add migration notes here.

Security & Integrity
--------------------
- Backups should be stored with appropriate filesystem permissions.
- Implementations in this repo write to a temporary file and atomically rename it to avoid partial writes (fsync + rename on the same filesystem).
- For tamper detection, consider adding a signed metadata block or a checksum file alongside the backup.

See also
--------
- `rtfs_compiler/src/ccos/intent_storage.rs` — producer/consumer of this format
- `docs/ccos/specs/file_backend.md` — on-disk storage layout for file backend
