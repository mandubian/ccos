# RTFS 2.0 Standard Library

**Implementation Status**: ✅ **Fully Implemented** (verified against `rtfs` crate)

This document provides a comprehensive reference for the RTFS 2.0 standard library. The standard library is divided into two layers:

1.  **Secure Standard Library (`secure_stdlib.rs`)**: A minimal, pure, deterministic core that can be executed in any context.
2.  **Extended Standard Library (`stdlib.rs`)**: Includes the secure core plus additional pure utilities (JSON, complex collection helpers) and the `call` interface for host capabilities.

## Implementation Verification

The functions listed below have been **fully verified** against the implementation in `rtfs/src/runtime/secure_stdlib.rs` and `rtfs/src/runtime/stdlib.rs`:

| Category | Status | Notes |
|----------|--------|-------|
| **Arithmetic** | ✅ **Implemented** | Includes `+`, `-`, `*`, `/`, `inc`, `dec`, `max`, `min`, `factorial`, `abs`, `sqrt`, `pow`, etc. |
| **Comparison** | ✅ **Implemented** | Includes `=`, `!=`, `>`, `<`, `>=`, `<=` |
| **Boolean Logic** | ✅ **Implemented** | Includes `and`, `or`, `not` |
| **String Functions** | ✅ **Implemented** | Includes `str`, `length`, `substring`, `contains?`, `starts-with?`, `string-join`, `upper`, `lower`, `trim`, and full Regex support |
| **Collection Functions** | ✅ **Implemented** | `map`, `filter`, `reduce`, `apply`, `sort`, `sort-by`, `distinct`, `frequencies`, `get`, `get-in`, `assoc`, `dissoc`, `conj`, `first`, `rest`, `nth`, `count`, `empty?`, `range`, `numbers`, `take`, `drop`, `last`, `reverse`, `group-by`, `keys`, `vals` |
| **Type Predicates** | ✅ **Implemented** | `nil?`, `bool?`, `int?`, `float?`, `number?`, `string?`, `fn?`, `symbol?`, `keyword?`, `vector?`, `map?`, `type-name` |
| **JSON Support** | ✅ **Implemented** | `parse-json`, `serialize-json` (pure functions) |
| **Host Interface** | ✅ **Implemented** | `call` function for CCOS capabilities |

## 1. Core Philosophy

The standard library is designed with the following principles in mind:

- **Pure Functions Only:** All functions (except `call`) are pure and referentially transparent.
- **No Effects:** RTFS kernel is an effect-free language. All side effects must be explicitly delegated via `(call :capability ...)`.
- **Layered Security:** The `SecureStandardLibrary` provides a "safe mode" that excludes even host-delegation (`call`).
- **Immutable Data:** All operations on collections return new immutable values.
- **Consistency:** Provides an idiomatic set of functions familiar to Clojure/Lisp developers.

## 2. Library Layers

### 2.1 Secure Standard Library (Core)
The **Secure Standard Library** (`rtfs/src/runtime/secure_stdlib.rs`) is the foundation of the RTFS environment. It contains only functions that are guaranteed to:
- Have no external dependencies.
- Be perfectly deterministic.
- Have no path to effect delegation.

### 2.2 Standard Library (Full Environment)
The **Standard Library** (`rtfs/src/runtime/stdlib.rs`) is the default environment provided to RTFS plans. It composes the Secure core with:
- **`call`**: The gateway to CCOS capabilities.
- **JSON Utilities**: `parse-json` and `serialize-json`.
- **Complex Helpers**: Functions like `map-indexed`, `frequencies`, and `sort-by` which may use evaluator context.

---

## 3. Arithmetic Functions

| Function | Signature | Description |
|---|---|---|
| `+` | `(-> :number ... :number)` | Adds numbers. |
| `-` | `(-> :number ... :number)` | Subtracts numbers. |
| `*` | `(-> :number ... :number)` | Multiplies numbers. |
| `/` | `(-> :number :number :number)` | Divides numbers. |
| `mod` | `(-> :int :int :int)` | Returns the remainder of division. |
| `inc` | `(-> :number :number)` | Increments by 1. |
| `dec` | `(-> :number :number)` | Decrements by 1. |
| `max` | `(-> :number ... :number)` | Returns the largest number. |
| `min` | `(-> :number ... :number)` | Returns the smallest number. |
| `abs` | `(-> :number :number)` | Returns absolute value. |
| `sqrt` | `(-> :number :float)` | Returns square root. |
| `pow` | `(-> :number :number :number)` | Returns base raised to power. |
| `factorial` | `(-> :int :int)` | Returns factorial of n. |
| `even?` | `(-> :int :bool)` | `true` if even. |
| `odd?` | `(-> :int :bool)` | `true` if odd. |
| `zero?` | `(-> :number :bool)` | `true` if zero. |
| `pos?` | `(-> :number :bool)` | `true` if positive. |
| `neg?` | `(-> :number :bool)` | `true` if negative. |

## 4. Comparison Functions

| Function | Signature | Description |
|---|---|---|
| `=` | `(-> :any :any :bool)` | Returns `true` if two values are equal. |
| `!=` | `(-> :any :any :bool)` | Returns `true` if two values are not equal. |
| `>` | `(-> :any :any :bool)` | Returns `true` if the first value is greater than the second. |
| `<` | `(-> :any :any :bool)` | Returns `true` if the first value is less than the second. |
| `>=` | `(-> :any :any :bool)` | Returns `true` if the first value is greater than or equal to the second. |
| `<=` | `(-> :any :any :bool)` | Returns `true` if the first value is less than or equal to the second. |

## 5. Boolean Logic Functions

| Function | Signature | Description |
|---|---|---|
| `and` | `(-> :any ... :any)` | Returns the first falsy value, or the last truthy value. |
| `or` | `(-> :any ... :any)` | Returns the first truthy value, or the last falsy value. |
| `not` | `(-> :any :bool)` | Returns the boolean opposite of a value. |

## 6. String Manipulation Functions

| Function | Signature | Description |
|---|---|---|
| `str` | `(-> :any ... :string)` | Concatenates all arguments into a string. |
| `string-length` | `(-> :string :int)` | Returns the length of a string. |
| `substring` | `(-> :string :int :int :string)` | Returns a substring of a string (start, end). |
| `string-contains?` | `(-> :string :string :bool)` | Returns `true` if a string contains another string. |
| `starts-with?` | `(-> :string :string :bool)` | Returns `true` if string starts with prefix. |
| `string-join` | `(-> :string :vector :string)` | Joins vector of strings with separator. |
| `string-upper` | `(-> :string :string)` | Converts string to uppercase. |
| `string-lower` | `(-> :string :string)` | Converts string to lowercase. |
| `string-trim` | `(-> :string :string)` | Trims whitespace from start/end. |
| `re-matches` | `(-> :string :string :any)` | Returns full match or nil. |
| `re-find` | `(-> :string :string :any)` | Returns first match or nil. |
| `re-seq` | `(-> :string :string :vector)` | Returns vector of all matches. |
| `parse-int` | `(-> :string :int)` | Parses string to integer. |
| `parse-float` | `(-> :string :float)` | Parses string to float. |
| `int` | `(-> :any :int)` | Coerces value to integer. |
| `float` | `(-> :any :float)` | Coerces value to float. |

## 7. Collection Manipulation Functions

**Note:** All collection functions are **pure and immutable**. They return new collections rather than modifying existing ones.

| Function | Signature | Description |
|---|---|---|
| `vector` | `(-> ... :vector)` | Creates a new vector. |
| `hash-map` | `(-> ... :map)` | Creates a new map. |
| `map` | `(-> :function :collection :collection)` | Applies a function to each element. |
| `map-indexed` | `(-> :function :collection :collection)` | Applies function to index and element. |
| `filter` | `(-> :function :collection :collection)` | Returns elements satisfying predicate. |
| `reduce` | `(-> :function :any? :collection :any)` | Reduces collection to single value. |
| `apply` | `(-> :function :any* :vector :any)` | Calls function with arguments. |
| `sort` | `(-> :collection :collection)` | Sorts a collection. |
| `sort-by` | `(-> :function :collection :collection)` | Sorts collection by key function. |
| `distinct` | `(-> :collection :collection)` | Removes duplicate values. |
| `frequencies` | `(-> :collection :map)` | Returns map of element frequencies. |
| `group-by` | `(-> :function :collection :map)` | Groups elements by key function. |
| `contains?` | `(-> :collection :any :bool)` | `true` if collection contains element. |
| `keys` | `(-> :map :vector)` | Returns vector of map keys. |
| `vals` | `(-> :map :vector)` | Returns vector of map values. |
| `get` | `(-> :any :any :any?)` | Returns value for key, or optional default. |
| `get-in` | `(-> :any :vector :any?)` | Returns value at nested path. |
| `assoc` | `(-> :collection :any ... :collection)` | Returns new collection with associations. |
| `dissoc` | `(-> :map :keyword ... :map)` | Returns new map with keys removed. |
| `update` | `(-> :map :any :fn ... :map)` | Updates value at key by applying function. |
| `remove` | `(-> :fn :collection :collection)` | Returns elements NOT satisfying predicate. |
| `cons` | `(-> :any :collection :collection)` | Adds element to beginning. |
| `conj` | `(-> :collection :any ... :collection)` | Appends elements to collection (vector-optimized). |
| `concat` | `(-> ... :collection)` | Concatenates collections. |
| `first` | `(-> :collection :any)` | Returns first element. |
| `rest` | `(-> :collection :collection)` | Returns all but first element. |
| `last` | `(-> :collection :any)` | Returns last element. |
| `nth` | `(-> :collection :int :any)` | Returns element at index. |
| `count` | `(-> :collection :int)` | Returns number of elements. |
| `empty?` | `(-> :collection :bool)` | `true` if collection is empty. |
| `range` | `(-> :int? :int :int? :vector)` | Returns range of numbers (start, end, step). |
| `numbers` | `(-> :int :int :vector)` | Returns numbers from start to end (inclusive). |
| `take` | `(-> :int :collection :collection)` | Returns first n elements. |
| `drop` | `(-> :int :collection :collection)` | Returns all but first n elements. |
| `reverse` | `(-> :collection :collection)` | Returns elements in reverse order. |
| `merge` | `(-> :map ... :map)` | Merges multiple maps. |
| `find` | `(-> :map :any :any)` | Returns [key value] pair or nil. |

## 8. Type Predicate Functions

| Function | Signature | Description |
|---|---|---|
| `nil?` | `(-> :any :bool)` | `true` if value is `nil`. |
| `bool?` | `(-> :any :bool)` | `true` if value is a boolean. |
| `int?` | `(-> :any :bool)` | `true` if value is an integer. |
| `float?` | `(-> :any :bool)` | `true` if value is a float. |
| `number?` | `(-> :any :bool)` | `true` if value is a number (int or float). |
| `string?` | `(-> :any :bool)` | `true` if value is a string. |
| `fn?` | `(-> :any :bool)` | `true` if value is a function/lambda. |
| `symbol?` | `(-> :any :bool)` | `true` if value is a symbol. |
| `keyword?` | `(-> :any :bool)` | `true` if value is a keyword. |
| `vector?` | `(-> :any :bool)` | `true` if value is a vector. |
| `map?` | `(-> :any :bool)` | `true` if value is a map. |
| `type-name` | `(-> :any :string)` | Returns type name as string (e.g., ":int", ":vector"). |

## 9. JSON & Data Functions

These functions are part of the **Standard Library** (not the Secure Core) but are **pure**.

| Function | Signature | Description |
|---|---|---|
| `parse-json` | `(-> :string :any)` | Parses JSON string to RTFS value. |
| `serialize-json` | `(-> :any :string)` | Serializes RTFS value to JSON string. |

---

## 10. Host Delegation & Capabilities

RTFS kernel is strictly effect-free. All side effects (I/O, network, state, etc.) are performed via the `call` function.

| Function | Signature | Description |
|---|---|---|
| `call` | `(-> :keyword ... :any)` | Invokes a CCOS capability. |

### Why `call`?
1. **Governance:** CCOS intercepts every `call` to check permissions and budgets.
2. **Auditability:** Every host interaction is recorded in the Causal Chain.
3. **Homoiconicity:** Plans remain pure data; only their *execution* in a CCOS host produces effects.
4. **Abstraction:** RTFS code doesn't care if a capability is provided by a local tool, a remote agent, or a MicroVM.

### Common Capability Examples

```clojure
;; I/O
(call :ccos.io.log "Message")
(call :ccos.io.read-file "data.txt")

;; Network
(call :ccos.network.http-fetch "url")

;; JSON (Alternative to pure helpers for symmetry)
(call :ccos.json.parse json-str)
(call :ccos.json.stringify val)
```
