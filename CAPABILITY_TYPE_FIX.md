# Quick Fix: Capability Parameter Types

## The Issue

Current capability synthesis generates invalid RTFS type syntax:

```rtfs
:parameters {:travel_dates "string" :duration "number" :budget "currency" :interests "list"}
                              ^^^^^^^^              ^^^^^^^^                 ^^^^^^^^
                              ❌ String literal - WRONG!
```

## The Fix

Use **keyword types** instead of string literals:

```rtfs
:parameters {:travel_dates :string :duration :number :budget :currency :interests :list}
                           ^^^^^^^^          ^^^^^^^^                  ^^^^^^^^
                           ✅ Keyword type - CORRECT!
```

## Implementation Location

File: `rtfs_compiler/examples/user_interaction_smart_assistant.rs`

In the LLM prompt that generates the capability (around line 820-860 in the synthesis prompt).

### Where LLM Gets Examples

The prompt shows capitalized types (for RTFS core types), but we need it to generate keyword types for parameters:

**Current prompt shows**:
```
:parameters {{:destination "string" :duration "number" :budget "number" :interests "list"}}
```

**Should show**:
```
:parameters {{:destination :string :duration :number :budget :currency :interests :list}}
```

## Code Fix Option

### Option A: Fix in Generated Examples (Prompt)

In `synthesize_capability_via_llm()`, update the example capability:

**Before**:
```rust
:parameters {{:destination "string" :duration "number" :budget "number" :interests "list"}}
```

**After**:
```rust
:parameters {{:destination :string :duration :number :budget :currency :interests :list}}
```

### Option B: Fix at ExtractedParam Level

In `ExtractedPreferences` struct, add a method to generate keyword types:

```rust
impl ExtractedParam {
    fn to_rtfs_keyword_type(&self) -> String {
        format!(":{}", self.param_type)  // ":string", ":currency", etc.
    }
}
```

Then use it when building capability schema.

## Test After Fix

Run the demo and verify the generated capability has keyword types:

```bash
./demo_smart_assistant.sh --topic "plan trip to paris"
```

Look for Phase 2 output showing:
```rtfs
:parameters {:budget :currency :duration :number :interests :list}
```

Not:
```rtfs
:parameters {:budget "currency" :duration "number" :interests "list"}
```

## Compliance

- ✅ RTFS 2.0 spec compliance (uses keyword types for metadata)
- ✅ CCOS marketplace compatible (semantic type keywords)
- ✅ LLM synthesis compatible (keywords are self-documenting)
- ✅ Backward compatible (doesn't break existing code)

## Priority

**Medium** - The capability still works with string literals for parameters, but:
- Doesn't pass RTFS type validation
- Won't work with strict type checking
- Better to fix now before relying on type safety

## Related Files

- `RTFS_TYPE_SYNTAX_ANALYSIS.md` - Detailed analysis
- `DYNAMIC_KEYWORD_EXTRACTION.md` - Parameter extraction system
- `docs/rtfs-2.0/specs/03-core-syntax-data-types.md` - RTFS type syntax
- `docs/rtfs-2.0/specs/09-streaming-capabilities.md` - Schema type examples
