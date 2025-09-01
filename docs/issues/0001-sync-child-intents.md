Title: Sync StorableIntent.child_intents when edges are created

Summary

Currently the authoritative edge storage is updated when edges are created (via `create_edge`/`storage.create_edge`), but the denormalized `StorableIntent.child_intents` field on stored intents is not updated. This causes UIs and consumers that rely on `st.child_intents` to see stale/empty child lists (observed in `arbiter_rtfs_graph_demo_live.rs`).

Goal

Ensure `StorableIntent.child_intents` is updated when edges are created so the denormalized field remains consistent with edge storage.

Acceptance criteria

- When `IntentGraph::create_edge` (or underlying storage) persists an edge, the parent and/or child `StorableIntent` records are updated to include the relationship in `child_intents`/`parent_intent` fields as appropriate.
- Tests added/updated in `rtfs_compiler` that exercise storing an intent, creating an edge, and reading back `get_all_intents_sync()` to verify `child_intents` contains the expected id(s).
- Changes do not introduce deadlocks in the current-thread runtime (use existing runtime handles / don't double-lock mutexes).

Notes

- Alternative approach: prefer canonical, authoritative APIs (`get_child_intents()`) and migrate consumers to that model instead of maintaining denormalized lists. Pros/cons should be evaluated.
- Implementers should search for places that call `storage.create_edge(...)` and ensure updates to stored intents are done atomically with edge insertion if possible.

Files likely affected

- `src/ccos/intent_graph/core.rs`
- `src/ccos/intent_graph/storage.rs`
- Tests in `src/ccos/intent_graph/tests.rs` and `tests/intent_graph_dependency_tests.rs`

Estimate: small (1-2 days) to implement and test.
