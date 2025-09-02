Title: Audit consumers for `StorableIntent.child_intents` usage and migrate to authoritative API

Summary

Several consumers read `StorableIntent.child_intents` directly. Since the denormalized `child_intents` field may be stale, we should audit all code and tests that rely on it and update them to call `IntentGraph::get_child_intents()` where appropriate.

Goal

- Find all usages of `child_intents` and determine whether they should be updated to query the authoritative API.
- Prioritize UI consumers (demos) and public APIs.
- Create follow-up fixes to update callers and tests.

Acceptance criteria

- A list of locations (files + line ranges) where `child_intents` is read.
- For each location, an explicit recommendation: "Migrate to `get_child_intents()`" or "Keep but add synchronization code to ensure denormalized field is maintained".
- A small PR to migrate the TUI demo has already been applied; follow-ups prioritized.

Files to search

- `**/*.rs` for `.child_intents` occurrences

Estimate: small audit (a few hours) and several small PRs for fixes.
