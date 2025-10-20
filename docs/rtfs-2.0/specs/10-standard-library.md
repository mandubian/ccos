# RTFS 2.0 Standard Library

This document provides a comprehensive reference for the RTFS 2.0 standard library, which contains **only pure, deterministic functions** with **no side effects**.

## 1. Core Philosophy

The standard library is designed with the following principles in mind:

- **Pure Functions Only:** All functions are pure and referentially transparent, making them safe and predictable.
- **No Effects:** RTFS is a no-effect language - all effects must be delegated to the host through capabilities.
- **Immutable Data:** All data structures are immutable - no mutation functions are provided.
- **Consistency:** The library provides a consistent and idiomatic set of functions for common tasks.
- **Extensibility:** The library is designed to be extensible with custom functions and capabilities.

## 2. Function Categories

The standard library is organized into the following categories:

- **Arithmetic:** Functions for mathematical operations.
- **Comparison:** Functions for comparing values.
- **Boolean Logic:** Functions for logical operations.
- **String Manipulation:** Functions for working with strings.
- **Collection Manipulation:** Functions for working with vectors, lists, and maps (read-only operations).
- **Type Predicates:** Functions for checking the type of a value.
- **CCOS Capabilities:** Functions for interacting with the CCOS through the host boundary.

**Note:** All effectful operations (I/O, file system, network, state mutation) are **not** part of the standard library. These must be accessed through CCOS capabilities using the `(call :capability ...)` syntax.

## 3. Arithmetic Functions

| Function | Signature | Description |
|---|---|---|
| `+` | `(-> :int :int ... :int)` | Adds two or more numbers. |
| `-` | `(-> :int :int :int)` | Subtracts two numbers. |
| `*` | `(-> :int :int ... :int)` | Multiplies two or more numbers. |
| `/` | `(-> :int :int :int)` | Divides two numbers. |
| `mod` | `(-> :int :int :int)` | Returns the remainder of a division. |
| `inc` | `(-> :int :int)` | Increments a number by 1. |
| `dec` | `(-> :int :int)` | Decrements a number by 1. |
| `max` | `(-> :any ... :any)` | Returns the largest of one or more numbers. |
| `min` | `(-> :any ... :any)` | Returns the smallest of one or more numbers. |
| `even?` | `(-> :int :bool)` | Returns `true` if a number is even. |
| `odd?` | `(-> :int :bool)` | Returns `true` if a number is odd. |
| `zero?` | `(-> :int :bool)` | Returns `true` if a number is zero. |
| `pos?` | `(-> :int :bool)` | Returns `true` if a number is positive. |
| `neg?` | `(-> :int :bool)` | Returns `true` if a number is negative. |

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
| `substring` | `(-> :string :int :string)` | Returns a substring of a string. |
| `string-contains` | `(-> :string :string :bool)` | Returns `true` if a string contains another string. |

## 7. Collection Manipulation Functions

**Note:** All collection functions are **pure and immutable**. They return new collections rather than modifying existing ones.

| Function | Signature | Description |
|---|---|---|
| `vector` | `(-> ... :vector)` | Creates a new vector. |
| `map` | `(-> :function :collection :collection)` | Applies a function to each element of a collection. |
| `map-indexed` | `(-> :function :collection :collection)` | Applies a function to each element of a collection, with the index. |
| `filter` | `(-> :function :collection :collection)` | Returns a new collection with only the elements that satisfy a predicate. |
| `reduce` | `(-> :function :collection :any)` | Reduces a collection to a single value using a function. |
| `sort` | `(-> :collection :collection)` | Sorts a collection. |
| `sort-by` | `(-> :function :collection :collection)` | Sorts a collection by a key function. |
| `distinct` | `(-> :collection :collection)` | Returns a new collection with duplicate values removed. |
| `frequencies` | `(-> :collection :map)` | Returns a map of the frequencies of the elements in a collection. |
| `contains?` | `(-> :collection :any :bool)` | Returns `true` if a collection contains an element. |
| `keys` | `(-> :map :vector)` | Returns a vector of the keys in a map. |
| `vals` | `(-> :map :vector)` | Returns a vector of the values in a map. |
| `get` | `(-> :map :any :any)` | Returns the value for a key in a map, or a default value. |
| `get-in` | `(-> :map :vector :any)` | Returns the value at a nested path in a map. |
| `cons` | `(-> :any :collection :collection)` | Adds an element to the beginning of a collection. |
| `concat` | `(-> ... :collection)` | Concatenates two or more collections. |
| `first` | `(-> :collection :any)` | Returns the first element of a collection. |
| `rest` | `(-> :collection :collection)` | Returns all but the first element of a collection. |
| `nth` | `(-> :collection :int :any)` | Returns the element at a given index in a collection. |
| `count` | `(-> :collection :int)` | Returns the number of elements in a collection. |
| `empty?` | `(-> :collection :bool)` | Returns `true` if a collection is empty. |
| `range` | `(-> :int :int :int :vector)` | Returns a vector of numbers in a given range. |
| `hash-map` | `(-> ... :map)` | Creates a new map. |
| `take` | `(-> :int :collection :collection)` | Returns the first n elements of a collection. |
| `drop` | `(-> :int :collection :collection)` | Returns all but the first n elements of a collection. |
| `last` | `(-> :collection :any)` | Returns the last element of a collection. |
| `reverse` | `(-> :collection :collection)` | Returns a new collection with elements in reverse order. |
| `numbers` | `(-> :int :int :vector)` | Returns a vector of numbers from start to end. |

**Removed Functions:** The following functions have been removed as they are effectful (mutate data structures):
- `assoc`, `dissoc`, `conj`, `remove`, `update`

For data structure mutations, use CCOS capabilities like `(call :ccos.state.kv/put ...)` or other appropriate state management capabilities.

## 8. Type Predicate Functions

| Function | Signature | Description |
|---|---|---|
| `nil?` | `(-> :any :bool)` | Returns `true` if a value is `nil`. |
| `bool?` | `(-> :any :bool)` | Returns `true` if a value is a boolean. |
| `int?` | `(-> :any :bool)` | Returns `true` if a value is an integer. |
| `float?` | `(-> :any :bool)` | Returns `true` if a value is a float. |
| `number?` | `(-> :any :bool)` | Returns `true` if a value is a number. |
| `string?` | `(-> :any :bool)` | Returns `true` if a value is a string. |
| `fn?` | `(-> :any :bool)` | Returns `true` if a value is a function. |
| `symbol?` | `(-> :any :bool)` | Returns `true` if a value is a symbol. |
| `keyword?` | `(-> :any :bool)` | Returns `true` if a value is a keyword. |
| `vector?` | `(-> :any :bool)` | Returns `true` if a value is a vector. |
| `map?` | `(-> :any :bool)` | Returns `true` if a value is a map. |
| `type-name` | `(-> :any :string)` | Returns the type of a value as a string. |

## 9. Effectful Operations (Not in Standard Library)

**Important:** RTFS 2.0 standard library contains **no effectful functions**. All operations that interact with the outside world must be accessed through CCOS capabilities.

### Common Effectful Operations via Capabilities

| Operation | Capability Call | Description |
|---|---|---|
| File I/O | `(call :ccos.io.file-exists "path")` | Check if file exists |
| | `(call :ccos.io.open-file "path")` | Open file for reading |
| | `(call :ccos.io.read-line file-handle)` | Read line from file |
| | `(call :ccos.io.write-line file-handle "content")` | Write line to file |
| | `(call :ccos.io.close-file file-handle)` | Close file |
| HTTP Requests | `(call :ccos.network.http-fetch "url")` | Fetch content from URL |
| Logging | `(call :ccos.io.log "message")` | Log message |
| | `(call :ccos.io.print "message")` | Print without newline |
| | `(call :ccos.io.println "message")` | Print with newline |
| System Info | `(call :ccos.system.get-env "VAR")` | Get environment variable |
| | `(call :ccos.system.current-time)` | Get current time |
| | `(call :ccos.system.current-timestamp-ms)` | Get current timestamp |
| Data Serialization | `(call :ccos.data.parse-json "json-string")` | Parse JSON string |
| | `(call :ccos.data.serialize-json value)` | Serialize to JSON |
| State Management | `(call :ccos.state.kv/get "key")` | Get value from key-value store |
| | `(call :ccos.state.kv/put "key" value)` | Put value in key-value store |
| | `(call :ccos.state.counter/inc "counter")` | Increment counter |
| | `(call :ccos.state.event/append "stream" event)` | Append to event stream |
| Human Interaction | `(call :ccos.agent.ask-human "prompt")` | Ask human for input |

### Why No Effectful Functions in Standard Library?

1. **Purity**: RTFS remains a pure functional language
2. **Security**: All effects are mediated through CCOS governance
3. **Auditability**: Every effect is tracked in the causal chain
4. **Testability**: Pure functions are easily testable
5. **Composability**: Pure functions can be safely combined

## 10. CCOS Capability Functions

The only way to perform effectful operations in RTFS is through the `call` function, which delegates to CCOS capabilities.

| Function | Signature | Description |
|---|---|---|
| `call` | `(-> :keyword ... :any)` | Calls a CCOS capability. This is the **only** way to perform effectful operations. |

### Capability Call Examples

```clojure
;; System operations
(call :ccos.system.get-env "PATH")
(call :ccos.system.current-time)

;; I/O operations  
(call :ccos.io.log "Processing data...")
(call :ccos.io.file-exists "/path/to/file")

;; Network operations
(call :ccos.network.http-fetch "https://api.example.com/data")

;; State management
(call :ccos.state.kv/get "user:123")
(call :ccos.state.kv/put "user:123" user-data)

;; Agent operations
(call :ccos.agent.discover-agents {:capabilities ["database" "api"]})
(call :ccos.agent.ask-human "Please confirm the operation")

;; Data operations
(call :ccos.data.parse-json json-string)
(call :ccos.data.serialize-json data)
```

### Capability Execution Flow

1. **RTFS Evaluation**: `(call :capability args...)` is evaluated
2. **Host Call Generation**: Creates `HostCall` with capability ID and arguments
3. **CCOS Governance**: Security validation, policy enforcement, audit logging
4. **Provider Execution**: Capability is executed by appropriate provider
5. **Result Return**: Result is returned to RTFS for continuation

This architecture ensures RTFS remains **pure** while enabling **secure, auditable** interaction with the external world.
