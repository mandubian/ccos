# RTFS Primitive Types: Clarification

## The Confusion: "Integer" vs "integer"

**Question**: Is `Integer` a primitive type?

**Answer**: No! `Integer` is just a **symbol**, not a primitive type.

## Three Different Concepts

### 1. Runtime Primitive Types (What Actually Exists)

These are the actual runtime types, from `Value::get_type()`:

```
integer         ; 64-bit integers
float           ; 64-bit floats
boolean         ; true/false
string          ; UTF-8 text
nil             ; null/empty
vector          ; [1 2 3]
list            ; (1 2 3)
map             ; {:key value}
symbol          ; my-symbol
keyword         ; :my-keyword
timestamp       ; #timestamp("...")
uuid            ; #uuid("...")
resource-handle ; #resource-handle("...")
function        ; #<function>
error           ; #<error: ...>
```

**These are lowercase and represent actual runtime values.**

### 2. Grammar Primitive Types (What Parser Accepts)

From `rtfs.pest` (line 113-114):

```
primitive_type = { symbol | keyword }
// Accept both bare symbols (int) and keyword forms (:int)
```

**This means ANY symbol or keyword is accepted as a type!**

Examples:
- `int` ✅
- `Int` ✅
- `Integer` ✅
- `MyCustomType` ✅
- `:int` ✅
- `:Integer` ✅
- `whatever-i-want` ✅

All valid!

### 3. Type Annotations (Hints in Code)

Type annotations are **optional** hints written by programmers:

```rtfs
;; All of these are valid:
(defn add [a : integer b : integer] : integer ...)    ; Runtime type
(defn add [a : int b : int] : int ...)                ; Common shorthand
(defn add [a : Integer b : Integer] : Integer ...)    ; Capitalized convention
(defn add [a : Num b : Num] : Num ...)                ; Custom convention
(defn add [a : Int32 b : Int32] : Int32 ...)          ; Specific naming
```

**They're all just symbols** - the grammar and runtime don't care which convention you use.

## Why the Confusion?

The spec was showing examples like:

```rtfs
(assert-type x Integer)    ; Integer with capital I
(cast-to String value)     ; String with capital S
```

But the actual runtime has lowercase types:

```rtfs
(assert-type x integer)    ; What runtime expects
(cast-to string value)     ; Correct lowercase
```

## The Key Insight

**Type annotations in RTFS are not about pre-defined types.**

Instead, they're about:
1. **Documentation** - Help humans understand intent
2. **Hints** - For IDEs and tools
3. **Convention** - Teams can choose their own naming
4. **Custom domains** - Use domain-specific types

The system is completely flexible!

## Best Practices

### Use Runtime Type Names When Asserting

```rtfs
;; Runtime type assertions - use lowercase
(assert-type x integer)
(assert-type items vector)
(assert-type config map)
```

### Use Convention for Annotations

```rtfs
;; For type hints - use your convention
(defn process [data : Data] : Result ...)
(defn calculate [x : Number y : Number] : Number ...)
(defn merge [a : Map b : Map] : Map ...)
```

### Be Consistent Within Projects

Pick a convention and stick with it:
- Option 1: Lowercase (matches runtime)
- Option 2: Capitalized (looks traditional)
- Option 3: Domain-specific (e.g., `HttpRequest`, `DatabaseResult`)

Example team convention:
```rtfs
;; Consistent capitalization for type hints
(defn fetch [url : String] : HttpResponse ...)
(defn parse [data : String] : JsonValue ...)
(defn save [record : DatabaseRecord] : Operation ...)

;; But use runtime types for assertions
(assert-type response map)
(assert-type data string)
```

## Summary

| Concept | What It Is | Examples | Notes |
|---------|-----------|----------|-------|
| **Runtime types** | Actual values | `integer`, `string`, `vector`, `map` | Lowercase, from `Value::get_type()` |
| **Grammar primitive_type** | Parser rule | ANY symbol or keyword | Completely flexible |
| **Type annotations** | Hints/documentation | Use your convention | Optional and flexible |
| **Integer** | Just a symbol | Capital-I `Integer` is a symbol | Not special, not pre-defined |

The beauty of RTFS: **You define what type names mean in your domain!**

## Corrections to Specs

Specs that said `Integer` as a primitive type have been corrected to:
1. Show lowercase runtime types as primary examples
2. Explain that any symbol works
3. Show the flexibility and conventions
4. Link to actual runtime types in `Value::get_type()`

This prevents confusion and embraces RTFS's dynamic, flexible nature.



