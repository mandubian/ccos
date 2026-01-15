# RTFS Argument Normalization Plan (Draft)

## Goal
Allow capabilities to be called with either:
- positional arguments (tuple/vector)
- named arguments (map with keyword keys)

…while using **map-only schemas** as the single source of truth, eliminating the need for verbose `Union(Tuple, Map)` patterns.

## Current Situation
- Capabilities use strict `Arity` checks before type validation.
- Schemas like `write-line` use `TypeExpr::Union` with both `Tuple` and `Map` variants.
- This is verbose, error-prone (can drift out of sync), and requires `Arity::Range` workarounds.

### Before: Current `write-line` Schema
```rust
// Requires Union + Arity::Range(1, 2) awkwardness
TypeExpr::Union(vec![
    // Positional args: (handle line)
    TypeExpr::Tuple(vec![
        TypeExpr::Primitive(PrimitiveType::Int),
        TypeExpr::Primitive(PrimitiveType::String),
    ]),
    // Map args: {:handle <int> :line <string>}
    TypeExpr::Map {
        entries: vec![
            MapTypeEntry { key: Keyword("handle"), value_type: Int, optional: false },
            MapTypeEntry { key: Keyword("line"), value_type: String, optional: false },
        ],
        wildcard: None,
    },
])
```

### After: Map-Only Schema with Auto-Normalization
```rust
// Single source of truth - map schema only
// Positional args (1, "foo") auto-normalized to {:handle 1 :line "foo"}
TypeExpr::Map {
    entries: vec![
        MapTypeEntry { key: Keyword("handle"), value_type: Int, optional: false },
        MapTypeEntry { key: Keyword("line"), value_type: String, optional: false },
    ],
    wildcard: None,
}
```

## Design Options

### Option A: RTFS-Level Normalization (Language Semantics)
Add a runtime rule that, when a function expects a **map input schema**, positional args
can be normalized into a map before validation/execution.

**Proposed rule:**
- If the function has a map schema with required fields in a **declaration order**,
  and the call provides a tuple/vector/list of matching length,
  then map positional args to those fields.
- **Declaration order** = the order fields appear in the `entries` vector of `TypeExpr::Map`.

**Pros**
- Works for any capability/function with map schemas.
- Callers get flexible syntax automatically.
- Single source of truth for schema.

**Cons**
- Changes RTFS runtime semantics (wider impact).
- Needs clear rules for field order and optional fields.
- Potential ambiguity if schema includes wildcard or optional keys.

### Option B: Capability-Level Normalization (Boundary Adapter)
Normalize in the capability layer (registry/provider) and keep RTFS unchanged.

**Pros**
- Contained change, minimal global impact.
- Explicit control per capability.

**Cons**
- Requires per-capability implementation or shared helper.
- Not "pure RTFS" behavior.

### Option C: Hybrid
Add a **helper** in RTFS runtime (library function) but keep default semantics unchanged.
Capabilities can opt into normalization by calling the helper before validation.

## Detailed Proposal (Option B - Immediate Implementation)

### 1) Normalization Helper Function

Add `normalize_args_to_map` in `ccos/src/capabilities/mod.rs`:

```rust
/// Normalize positional arguments to a map based on a TypeExpr::Map schema.
/// 
/// # Rules:
/// - If args is already a single Map value, return it unchanged.
/// - If args is a list/vector AND schema is TypeExpr::Map:
///   - If args length matches number of REQUIRED fields (non-optional entries):
///     - Convert to map `{ :field_i -> arg_i }` following declaration order.
///   - Declaration order = order of entries in MapTypeEntry vector.
/// - Optional fields must be passed via map syntax.
/// - If schema has wildcard entries, normalization is not supported (ambiguous).
/// 
/// # Edge Cases:
/// - Single positional arg → single-field map: `(call cap x)` → `{:field x}`
/// - Wrong arg count → error with expected field names
pub fn normalize_args_to_map(
    args: Vec<Value>,
    schema: &TypeExpr,
) -> Result<Value, RuntimeError>
```

### 2) Field Ordering Semantics

**Declaration Order Rule:**
- Positional args map to fields in the order they appear in `MapTypeEntry.entries`.
- Only **required** fields (where `optional: false`) participate in positional normalization.
- Optional fields must be explicitly passed via map syntax.

Example:
```rust
// Schema: {:handle Int :line String :encoding? String}
// entries = [handle, line, encoding?]
// Required fields in order: [handle, line]

(call write-line 1 "foo")        // → {:handle 1 :line "foo"}
(call write-line {:handle 1 :line "foo" :encoding "utf-8"})  // passthrough
```

### 3) Edge Cases & Rules

| Case | Behavior |
|------|----------|
| Map schema, no wildcard | ✅ Normalization supported |
| Map schema with wildcard | ❌ Error: ambiguous positional mapping |
| Map with optional fields | ✅ Positional maps to required only, optionals via map |
| Single arg → single-field map | ✅ `(cap x)` → `{:field x}` |
| Args already a map | ✅ Passthrough unchanged |
| Wrong positional count | ❌ Error with expected field names |
| Union schemas | ❌ Not normalized (caller must be explicit) |

### 4) Arity Handling

Update capability registration to use:
- `Arity::Fixed(1)` for capabilities with map-only schemas (the single map arg)
- The normalization helper handles both formats before the single-map-arg is passed.

**Alternative:** Use `Arity::Range(N, 1)` where N = number of required fields, but this is less clean.

### 5) Error Reporting

If normalization fails, return explicit error:
```
Expected map with keys [:handle :line] or positional args of length 2
```

## Comprehensive Examples

### Basic Normalization
```clojure
;; Schema: {:handle Int :line String}
(call write-line 1 "hello")              ;; → {:handle 1 :line "hello"} ✅
(call write-line {:handle 1 :line "hi"}) ;; → {:handle 1 :line "hi"}    ✅ passthrough
```

### Single Positional Arg → Single Field
```clojure
;; Schema: {:path String}
(call read-file "/tmp/foo.txt")          ;; → {:path "/tmp/foo.txt"} ✅
(call read-file {:path "/tmp/foo.txt"})  ;; → {:path "/tmp/foo.txt"} ✅

;; Schema: {:key String}
(call get-env "HOME")                    ;; → {:key "HOME"} ✅
```

### Optional Fields (Trailing Only)
```clojure
;; Schema: {:handle Int :line String :encoding? String}
;; Required: [handle, line], Optional: [encoding]

(call write-line 1 "hello")              ;; → {:handle 1 :line "hello"} ✅
(call write-line 1 "hello" "utf-8")      ;; → ERROR: 3 args, expected 2 ❌
(call write-line {:handle 1 :line "hello" :encoding "utf-8"})  ;; ✅

;; Schema: {:url String :method? String :headers? Map}
(call http-fetch "https://api.example.com")  ;; → {:url "..."} ✅
(call http-fetch "url" "POST")               ;; → ERROR: 2 args, expected 1 ❌
```

### Single Map Field Ambiguity
```clojure
;; Schema: {:data Map}  ← field expects a Map value
(call store-data {:foo 1 :bar 2})  ;; AMBIGUOUS!
;; Resolution: if input lacks :data key → normalize to {:data {:foo 1 :bar 2}}
;; If input has :data key → passthrough
```

### Nested Map Args
```clojure
;; Schema: {:config {:host String :port Int}}
(call connect {:host "localhost" :port 8080})  
;; → {:config {:host "localhost" :port 8080}} ✅ (normalized)

(call connect {:config {:host "localhost" :port 8080}})  
;; → passthrough ✅ (has :config key)
```

## Problematic Cases & Resolutions

### 1. ⚠️ Map Field Ambiguity (CRITICAL)
**Problem:** Schema expects single Map field—how to distinguish passthrough from positional?

**Resolution:** Check if input has the expected field key:
- If `{:data ...}` present → passthrough
- If `:data` absent → normalize to `{:data <input>}`

### 2. ⚠️ Optional Fields Between Required
```clojure
;; Schema: {:a Int :b? String :c Int}  ← :b optional BETWEEN required
(call foo 1 2)  ;; Ambiguous: {:a 1 :c 2} or {:a 1 :b 2}?
```
**Resolution:** REJECT normalization. Optional fields must be trailing only.

### 3. ⚠️ Wildcard Maps
```clojure
;; Schema: {:id Int :* Any}
(call foo 1 "extra")  ;; No field name for "extra"
```
**Resolution:** Wildcard schemas reject positional normalization entirely.

### 4. ⚠️ Optional-Only Schema
```clojure
;; Schema: {:timeout? Int :retries? Int}
(call foo)    ;; → {} ✅ (zero args = empty map)
(call foo 5)  ;; → ERROR ❌ (must use map syntax)
```

### 5. ℹ️ Type Mismatch After Normalization
```clojure
(call foo "not-an-int")  ;; Normalized, then TypeValidator catches mismatch
```
**Resolution:** OK—normalization is syntactic; validation catches type errors.

### 6. ℹ️ Union Schemas
```clojure
;; Schema: [:union {:id Int} {:name String}]
(call foo 123)  ;; Which variant? Ambiguous.
```
**Resolution:** Union schemas do NOT support positional normalization.

## Summary: Normalization Rules

| Schema Pattern | Positional? | Notes |
|----------------|-------------|-------|
| Map, required only | ✅ | `(a b)` → `{:f1 a :f2 b}` |
| Map, trailing optionals | ✅ | Required fields only |
| Map, mixed optionals | ❌ | Error: optionals must be trailing |
| Map with wildcard | ❌ | Error: wildcard not supported |
| Optional-only map | ⚠️ | Zero args → `{}`, else error |
| Single map field | ✅ | Check for field key presence |
| Union | ❌ | Not normalized |

## Migration Plan

1. Add `normalize_args_to_map` helper and tests
2. Update `write-line` capability to use map-only schema with normalization
3. Update existing test in `capability_schema_tests.rs` to verify both input styles still work
4. Migrate other applicable capabilities (`write-file`, `put`, etc.)
5. (Future) Consider Option A for RTFS-level semantics if pattern proves successful

## Testing Plan

### Unit Tests (New: `ccos/tests/arg_normalization_tests.rs`)
```
cargo test --package ccos --test arg_normalization_tests
```

- `test_normalize_positional_to_map` - tuple → map normalization
- `test_normalize_map_passthrough` - map input unchanged
- `test_normalize_wrong_length_error` - error on wrong positional count
- `test_normalize_single_arg_single_field` - single arg → single-field map
- `test_normalize_wildcard_schema_error` - wildcard schemas rejected
- `test_normalize_optional_fields_ignored` - only required fields for positional

### Existing Tests (Update: `ccos/tests/capability_schema_tests.rs`)
```
cargo test --package ccos --test capability_schema_tests
```

- Update `write_line_accepts_tuple_and_map_inputs` to verify:
  - Schema is now `TypeExpr::Map` (not Union)
  - Normalization helper converts positional to map
  - Both styles validate correctly

## Recommendation

Start with **Option B** (capability-level adapter) for immediate UX improvement,
then consider **Option A** if consistent RTFS semantics are desired.
