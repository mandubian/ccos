# RTFS Grammar Syntax Error: Type Annotations in defn

## The Error in Spec

From `03-core-syntax-data-types.md` (lines 285-289):

```rtfs
;; Type-annotated function
(defn add {:type {:args [Integer Integer] :return Integer}}
  [a b]
  (+ a b))
```

❌ **WRONG** - This shows type in a map `{:type {...}}`

## Actual RTFS Grammar (rtfs.pest)

From `rtfs_compiler/src/rtfs.pest` (line 273):

```
defn_expr = { "(" ~ defn_keyword ~ ... ~ symbol ~ ... ~ fn_param_list ~ (COLON ~ type_expr)? ~ expression+ ~ ")" }
```

**Key part**: `(COLON ~ type_expr)?` means type annotation uses `:` directly, not a map.

## Correct Syntax

The COLON syntax allows **both spaced and unspaced** forms (verified by test_type_annotation_whitespace.rs):

### With whitespace (explicit):
```rtfs
(defn add [a : int b : int] : int
  (+ a b))
```

### Without whitespace (shorthand - also valid!):
```rtfs
(defn add [a :int b :int] :int
  (+ a b))
```

Both are valid. The grammar treats `COLON` as optional whitespace-aware, so `: int` and `:int` both parse correctly.

## Grammar Structure Explained

The grammar says:
```
defn_expr = ( "defn" symbol fn_param_list (COLON type_expr)? expression+ )
```

Breaking it down:
1. `"defn"` - keyword
2. `symbol` - function name (e.g., `add`)
3. `fn_param_list` - parameters in brackets `[a b]` (with optional type annotations)
4. `(COLON type_expr)?` - **optional return type after colon**
5. `expression+` - function body

## Type Expression Syntax

From the grammar (lines 113-114):
```
primitive_type = { symbol | keyword }
// Accept both bare symbols (int) and keyword forms (:int) for backward compatibility
```

A `type_expr` can be:

### Primitive Types (Symbols or Keywords)
```rtfs
int           ; symbol (lowercase)
Int           ; symbol (capitalized)
:int          ; keyword
:Int          ; keyword
String        ; symbol
:String       ; keyword
Boolean       ; symbol
:Boolean      ; keyword
```

### Function Type (for parameter/return types)
```rtfs
[:fn [Type1 Type2] ReturnType]
[:=> [Type1 Type2] ReturnType]    ; Shorthand notation
```

For a function that takes two integers and returns an integer:
```rtfs
[:fn [int int] int]
[:=> [int int] int]               ; Shorthand
```

## Correct Examples

### Example 1: Simple Function with Keywords
```rtfs
(defn add [a :int b :int] :int
  (+ a b))
```

### Example 2: With Spaced Types
```rtfs
(defn add [a : int b : int] : int
  (+ a b))
```

### Example 3: With Capitalized Symbol Types
```rtfs
(defn add [a : Int b : Int] : Int
  (+ a b))
```

### Example 4: String Concatenation
```rtfs
(defn concat-str [s1 :String s2 :String] :String
  (str s1 s2))
```

### Example 5: With Function Type
```rtfs
(defn add [a :int b :int] : [:=> [int int] int]
  (+ a b))
```

### Example 6: Mixed (individual params + return type)
```rtfs
(defn multiply
  [x : int y : int]
  : [:fn [int int] int]
  (* x y))
```

### Example 7: No Type Annotation
```rtfs
(defn add [a b]
  (+ a b))
```

## Parameter Types Within fn_param_list

From grammar (lines 267, 270):

```
fn_param_list = { "[" ~ param_def* ~ "]" }
param_def = { binding_pattern ~ (COLON ~ type_expr)? }
```

Individual parameters CAN have optional types using COLON:

### Spaced version:
```rtfs
[a : int b : int]
```

### Unspaced version:
```rtfs
[a :int b :int]
```

Both work! The grammar is whitespace-agnostic around the COLON.

## Also: Metadata vs Types

From grammar (line 273):
```
defn_expr = { "(" ~ defn_keyword ~ metadata* ~ symbol ~ fn_param_list ~ (COLON ~ type_expr)? ~ expression+ ~ ")" }
```

**Metadata** (different from types) uses `^` prefix:

```rtfs
;; With metadata AND type annotation
(defn add 
  ^:delegation :local
  [a :int b :int]
  :int
  (+ a b))
```

Metadata goes AFTER `defn` keyword, type annotation goes AFTER parameters.

## What the Spec Should Show

```rtfs
;; Type-annotated function - CORRECT RTFS SYNTAX
(defn add [a :int b :int] :int
  (+ a b))

;; Alternative with spaces
(defn add [a : int b : int] : int
  (+ a b))

;; With function type signature
(defn add [a :int b :int] : [:=> [int int] int]
  (+ a b))

;; With metadata
(defn add
  ^:delegation :local
  [a :int b :int]
  :int
  (+ a b))
```

## Summary of Issues

| Aspect | Spec Shows | Grammar Actually Is | 
|--------|-----------|-------------------|
| Type syntax | `{:type {:args [...]}}` | `: type_expr` (COLON followed by type) |
| Type placement | In a metadata map | After parameter list |
| Param list | Not shown | `[a b]` or `[a :type b :type]` before return type |
| Return type | In nested map | After COLON following params |
| Whitespace | Not addressed | Optional (both `:int` and `: int` work) |
| Type forms | Not shown | Symbols (int, Int) OR Keywords (:int, :Int) |

## Impact

- ❌ Code in spec won't parse
- ❌ Type annotations don't validate
- ❌ Examples can't be copied and used directly
- ✅ Actual grammar is simpler and cleaner
- ✅ More flexible (symbols and keywords both work)

The grammar is actually more intuitive than what the spec shows!

## Key Insights

1. **Whitespace flexibility**: The pest grammar treats `COLON ~ type_expr` allowing whitespace, so both `:int` and `: int` work
2. **Type flexibility**: Both symbol forms (`int`, `Int`) and keyword forms (`:int`, `:Int`) are accepted
3. **Simpler than spec**: Actual syntax is cleaner than the nested map shown in the spec
4. **Test verified**: Confirmed by `test_type_annotation_whitespace.rs` test cases