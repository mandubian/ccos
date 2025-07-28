# RTFS 2.0 Secure Standard Library Specification

## Overview

The RTFS Secure Standard Library provides a comprehensive set of pure, deterministic functions that are safe to execute in any context without security concerns. This library implements the requirements specified in Issue #51, expanding the RTFS secure standard library with additional pure functions.

## Design Principles

- **Pure Functions**: All functions are pure and deterministic
- **Security**: No dangerous operations (file I/O, network access, system calls)
- **Composability**: Functions can be combined to build complex operations
- **Type Safety**: Strong type checking and error handling
- **Performance**: Efficient implementations for common operations

## Function Categories

### 1. Essential Math Functions

#### `abs`
- **Purpose**: Returns the absolute value of a number
- **Signature**: `(abs number) -> number`
- **Examples**:
  ```rtfs
  (abs -5)     ; => 5
  (abs 3.14)   ; => 3.14
  (abs 0)      ; => 0
  ```

#### `mod`
- **Purpose**: Returns the remainder of division
- **Signature**: `(mod dividend divisor) -> number`
- **Examples**:
  ```rtfs
  (mod 7 3)    ; => 1
  (mod 10 2)   ; => 0
  (mod -7 3)   ; => -1
  ```

#### `sqrt`
- **Purpose**: Returns the square root of a number
- **Signature**: `(sqrt number) -> number`
- **Examples**:
  ```rtfs
  (sqrt 16)    ; => 4.0
  (sqrt 2)     ; => 1.4142135623730951
  (sqrt 0)     ; => 0.0
  ```

#### `pow`
- **Purpose**: Returns a number raised to a power
- **Signature**: `(pow base exponent) -> number`
- **Examples**:
  ```rtfs
  (pow 2 3)    ; => 8
  (pow 5 2)    ; => 25
  (pow 2 0.5)  ; => 1.4142135623730951
  ```

### 2. String Utilities

#### `string-upper`
- **Purpose**: Converts a string to uppercase
- **Signature**: `(string-upper string) -> string`
- **Examples**:
  ```rtfs
  (string-upper "hello")     ; => "HELLO"
  (string-upper "World")     ; => "WORLD"
  (string-upper "123")       ; => "123"
  ```

#### `string-lower`
- **Purpose**: Converts a string to lowercase
- **Signature**: `(string-lower string) -> string`
- **Examples**:
  ```rtfs
  (string-lower "WORLD")     ; => "world"
  (string-lower "Hello")     ; => "hello"
  (string-lower "123")       ; => "123"
  ```

#### `string-trim`
- **Purpose**: Removes leading and trailing whitespace from a string
- **Signature**: `(string-trim string) -> string`
- **Examples**:
  ```rtfs
  (string-trim "  hi  ")     ; => "hi"
  (string-trim "hello")      ; => "hello"
  (string-trim "  ")         ; => ""
  ```

### 3. Collection Utilities

#### `reverse`
- **Purpose**: Reverses a vector or string
- **Signature**: `(reverse collection) -> collection`
- **Examples**:
  ```rtfs
  (reverse [1 2 3])          ; => [3 2 1]
  (reverse "hello")          ; => "olleh"
  (reverse [])               ; => []
  ```

#### `last`
- **Purpose**: Returns the last element of a vector or string
- **Signature**: `(last collection) -> element`
- **Examples**:
  ```rtfs
  (last [1 2 3])             ; => 3
  (last "hello")             ; => "o"
  (last [])                  ; => nil
  ```

#### `take`
- **Purpose**: Returns the first n elements of a collection
- **Signature**: `(take count collection) -> collection`
- **Examples**:
  ```rtfs
  (take 2 [1 2 3 4])         ; => [1 2]
  (take 3 "hello")           ; => "hel"
  (take 0 [1 2 3])           ; => []
  ```

#### `drop`
- **Purpose**: Returns all elements after the first n elements
- **Signature**: `(drop count collection) -> collection`
- **Examples**:
  ```rtfs
  (drop 2 [1 2 3 4])         ; => [3 4]
  (drop 2 "hello")           ; => "llo"
  (drop 0 [1 2 3])           ; => [1 2 3]
  ```

#### `distinct`
- **Purpose**: Returns a collection with duplicate elements removed
- **Signature**: `(distinct collection) -> collection`
- **Examples**:
  ```rtfs
  (distinct [1 2 2 3])       ; => [1 2 3]
  (distinct "hello")         ; => "helo"
  (distinct [])              ; => []
  ```

### 4. Functional Predicates

#### `every?`
- **Purpose**: Returns true if the predicate is true for all elements
- **Signature**: `(every? predicate collection) -> boolean`
- **Examples**:
  ```rtfs
  (every? (fn [x] (> x 0)) [1 2 3])     ; => true
  (every? (fn [x] (> x 0)) [-1 2 3])    ; => false
  (every? (fn [c] (= c (string-upper c))) "HELLO") ; => true
  ```

#### `some?`
- **Purpose**: Returns true if the predicate is true for at least one element
- **Signature**: `(some? predicate collection) -> boolean`
- **Examples**:
  ```rtfs
  (some? (fn [x] (> x 0)) [-1 -2 3])    ; => true
  (some? (fn [x] (> x 0)) [-1 -2 -3])   ; => false
  (some? (fn [c] (= c (string-upper c))) "Hello") ; => true
  ```

## Implementation Status

### ✅ Completed Functions
- **Phase 1**: All essential math functions (`abs`, `mod`, `sqrt`, `pow`)
- **Phase 2**: All string utilities (`string-upper`, `string-lower`, `string-trim`)
- **Phase 3**: All collection utilities (`reverse`, `last`, `take`, `drop`, `distinct`)

### ⚠️ Partially Implemented Functions
- **Phase 4**: Functional predicates (`every?`, `some?`)
  - **Status**: Implemented in the main evaluator
  - **Issue**: Requires IR runtime support for `BuiltinWithContext` functions
  - **Workaround**: Functions work in the main evaluator but not in the IR runtime

## Error Handling

All functions include comprehensive error handling:

- **Arity Mismatch**: Functions validate the correct number of arguments
- **Type Errors**: Functions validate argument types and provide clear error messages
- **Bounds Checking**: Collection functions handle empty collections and out-of-bounds access
- **Edge Cases**: Functions handle edge cases like empty strings, empty vectors, etc.

## Performance Characteristics

- **Time Complexity**: All functions are optimized for common use cases
- **Space Complexity**: Functions minimize memory allocation where possible
- **Lazy Evaluation**: Functional predicates use short-circuit evaluation
- **Immutable**: All functions return new values without modifying inputs

## Security Considerations

- **Pure Functions**: No side effects or external dependencies
- **Deterministic**: Same inputs always produce same outputs
- **No I/O**: No file system, network, or system call access
- **Memory Safe**: No buffer overflows or memory leaks
- **Type Safe**: Strong type checking prevents runtime errors

## Usage Examples

### Basic Math Operations
```rtfs
;; Calculate the hypotenuse of a right triangle
(defn hypotenuse [a b]
  (sqrt (+ (pow a 2) (pow b 2))))

(hypotenuse 3 4)  ; => 5.0
```

### String Processing
```rtfs
;; Normalize a string for comparison
(defn normalize-string [s]
  (string-trim (string-lower s)))

(normalize-string "  Hello World  ")  ; => "hello world"
```

### Collection Processing
```rtfs
;; Get unique elements from a list
(defn unique-elements [coll]
  (distinct coll))

(unique-elements [1 2 2 3 3 4])  ; => [1 2 3 4]
```

### Functional Programming
```rtfs
;; Check if all numbers are positive
(defn all-positive? [numbers]
  (every? (fn [x] (> x 0)) numbers))

(all-positive? [1 2 3])   ; => true
(all-positive? [-1 2 3])  ; => false
```

## Future Enhancements

### Planned Additions
- Additional math functions (trigonometric, logarithmic)
- More string manipulation functions
- Advanced collection operations
- Pattern matching utilities

### IR Runtime Support
- Complete implementation of `BuiltinWithContext` functions in IR runtime
- Performance optimizations for functional predicates
- Compile-time optimizations for common patterns

## Testing

All functions are thoroughly tested with:
- Unit tests for individual functions
- Integration tests for function combinations
- Edge case testing for error conditions
- Performance benchmarks for optimization

## References

- **Issue #51**: Original requirements for expanding the secure standard library
- **RTFS 2.0 Language Features**: Core language specification
- **RTFS 2.0 Grammar Extensions**: Syntax and grammar rules
- **RTFS 2.0 Native Type System**: Type system specification 