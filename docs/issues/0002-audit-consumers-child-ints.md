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

## Audit Results

### 1. **IntentBuilder Usage** - `src/builders/intent_builder.rs`

**Lines 32, 50, 124, 130, 320, 322, 427, 429**

**Current Usage**: The `IntentBuilder` class uses `child_intents` for:
- Debug formatting (`.field("child_intents", &self.child_intents)`)
- Cloning during builder operations
- Adding child intent IDs via `.with_child_intent()` and `.with_child_intents()`
- RTFS generation in `.to_rtfs()` method
- Property generation in `.to_intent_definition()`

**Recommendation**: **Keep but add synchronization code** - This is a builder pattern that should maintain the denormalized field for convenience. The builder is used to construct intents before they're stored in the graph, so it doesn't need the authoritative API.

**Action**: No changes needed - this is correct usage.

### 2. **RuntimeIntent Conversion** - `src/ccos/types.rs`

**Lines 502, 557**

**Current Usage**: Used in conversion methods between `RuntimeIntent` and `StorableIntent`:
- `RuntimeIntent::to_storable_intent()` - copies `child_intents` from runtime to storable
- `StorableIntent::to_runtime_intent()` - copies `child_intents` from storable to runtime

**Recommendation**: **Keep but add synchronization code** - These are conversion methods that should preserve the denormalized field during type conversions. The field should be kept in sync with edge storage.

**Action**: No changes needed - this is correct usage.

### 3. **Demo Code** - `examples/arbiter_rtfs_graph_demo_live.rs`

**Line 305** (Comment only)

**Current Usage**: The demo code contains a comment explaining the issue:
```rust
// Also query the authoritative graph API for children to detect
// whether edges exist even when storable.child_intents is empty.
```

**Recommendation**: **Already migrated** - The demo correctly uses the authoritative API `graph_lock.get_child_intents(&root_id)` instead of reading the stale field.

**Action**: No changes needed - this is already correctly implemented.

### 4. **Chat History References** - `chats/*.md`

**Multiple files with historical references**

**Current Usage**: Various chat files contain code snippets and discussions about `child_intents` usage.

**Recommendation**: **No action needed** - These are historical chat logs, not active code.

## Summary of Recommendations

### ✅ **No Changes Required**
1. **IntentBuilder** - Correctly uses `child_intents` as a builder field
2. **RuntimeIntent conversions** - Correctly preserves the field during type conversions  
3. **Demo code** - Already migrated to use authoritative API

### 🔄 **Already Implemented**
- **Issue #1 fix** - Denormalized fields are now synchronized when edges are created/deleted
- **Demo migration** - TUI demo already uses `get_child_intents()` API

## Conclusion

**Status**: ✅ **COMPLETED**

The audit reveals that:
1. **No active code** reads `StorableIntent.child_intents` directly for graph traversal
2. **All consumers** that need real-time graph data already use the authoritative `IntentGraph::get_child_intents()` API
3. **Remaining usage** is in builder patterns and type conversions where the denormalized field is appropriate
4. **Issue #1 fix** ensures the denormalized fields stay synchronized with edge storage

**No further migration work is required.** The codebase is already properly architected to use authoritative APIs for graph operations while maintaining denormalized fields for convenience in builders and type conversions.

## Files Affected

- `src/builders/intent_builder.rs` - ✅ Correct usage (no changes needed)
- `src/ccos/types.rs` - ✅ Correct usage (no changes needed)  
- `examples/arbiter_rtfs_graph_demo_live.rs` - ✅ Already migrated
- `chats/*.md` - ✅ Historical references (no action needed)

Estimate: ✅ **COMPLETED** - No additional work required beyond Issue #1 fix.
