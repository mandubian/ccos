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

```rtfs
;; Type-annotated function - CORRECT
(defn add [a b] : [Integer Integer] Integer
  (+ a b))
```

Or with multi-line formatting:

```rtfs
(defn add 
  [a b] 
  : [Integer Integer] Integer  ; Return type after params
  (+ a b))
```

## Grammar Structure Explained

The grammar says:
```
defn_expr = ( "defn" symbol fn_param_list (COLON type_expr)? expression+ )
```

Breaking it down:
1. `"defn"` - keyword
2. `symbol` - function name (e.g., `add`)
3. `fn_param_list` - parameters in brackets `[a b]`
4. `(COLON type_expr)?` - **optional type after colon** `[:fn [...] ReturnType]`
5. `expression+` - function body

## Type Expression Syntax

From the grammar (lines 150-163), a `type_expr` can be:

### Primitive Types
```rtfs
Integer      ; keyword or symbol
String
Boolean
```

### Function Type (for parameter/return types)
```rtfs
[:fn [Type1 Type2] ReturnType]
```

For a function that takes two Integers and returns an Integer:
```rtfs
[:fn [Integer Integer] Integer]
```

Or using the shorthand `=>`:
```rtfs
[:=> [Integer Integer] Integer]
```

## Correct Examples

### Example 1: Simple Function
```rtfs
(defn add [a b] : [:=> [Integer Integer] Integer]
  (+ a b))
```

### Example 2: String Concatenation
```rtfs
(defn concat-str [s1 s2] : [:=> [String String] String]
  (str s1 s2))
```

### Example 3: With Multiple Params
```rtfs
(defn process-data [data index] : [:=> [Vector Integer] String]
  (get data index))
```

### Example 4: No Type Annotation
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

This means individual parameters CAN have types:

```rtfs
(defn add [a : Integer b : Integer] : Integer
  (+ a b))
```

Or with function type:
```rtfs
(defn add [a : Integer b : Integer] : [:=> [Integer Integer] Integer]
  (+ a b))
```

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
  [a b]
  : [:=> [Integer Integer] Integer]
  (+ a b))
```

Metadata goes AFTER `defn` keyword, type annotation goes AFTER parameters.

## What the Spec Should Show

```rtfs
;; Type-annotated function - CORRECT RTFS SYNTAX
(defn add [a : Integer b : Integer] : [:=> [Integer Integer] Integer]
  (+ a b))

;; Simplified version
(defn add [a b] : [:=> [Integer Integer] Integer]
  (+ a b))

;; Alternative with explicit function type
(defn add [a b]
  : [:fn [Integer Integer] Integer]
  (+ a b))
```

## Summary of Issues

| Aspect | Spec Shows | Grammar Actually Is | 
|--------|-----------|-------------------|
| Type syntax | `{:type {:args [...]}}` | `: type_expr` |
| Type placement | In a metadata map | After parameter list |
| Param list | Not shown | `[a b]` before type |
| Return type | In nested map | Part of function type |

## Impact

- ❌ Code in spec won't parse
- ❌ Type annotations don't validate
- ❌ Examples can't be copied and used directly
- ✅ Actual grammar is simpler and cleaner

The grammar is actually more intuitive than what the spec shows!
