# RTFS 2.0 Standard Library

This document provides a comprehensive reference for the RTFS 2.0 standard library, which includes a wide range of functions for pure computation and impure side-effects.

## 1. Core Philosophy

The standard library is designed with the following principles in mind:

- **Purity:** Most functions are pure and referentially transparent, making them safe and predictable.
- **Host Boundary:** Impure functions that interact with the outside world are clearly separated and managed by the CCOS host.
- **Consistency:** The library provides a consistent and idiomatic set of functions for common tasks.
- **Extensibility:** The library is designed to be extensible with custom functions and capabilities.

## 2. Function Categories

The standard library is organized into the following categories:

- **Arithmetic:** Functions for mathematical operations.
- **Comparison:** Functions for comparing values.
- **Boolean Logic:** Functions for logical operations.
- **String Manipulation:** Functions for working with strings.
- **Collection Manipulation:** Functions for working with vectors, lists, and maps.
- **Type Predicates:** Functions for checking the type of a value.
- **Tooling:** Impure functions for interacting with the file system, network, and other external resources.
- **CCOS Capabilities:** Functions for interacting with the CCOS.

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

| Function | Signature | Description |
|---|---|---|
| `vector` | `(-> ... :vector)` | Creates a new vector. |
| `map` | `(-> :function :collection :collection)` | Applies a function to each element of a collection. |
| `map-indexed` | `(-> :function :collection :collection)` | Applies a function to each element of a collection, with the index. |
| `filter` | `(-> :function :collection :collection)` | Returns a new collection with only the elements that satisfy a predicate. |
| `reduce` | `(-> :function :collection :any)` | Reduces a collection to a single value using a function. |
| `remove` | `(-> :function :collection :collection)` | Returns a new collection with the elements that do not satisfy a predicate. |
| `sort` | `(-> :collection :collection)` | Sorts a collection. |
| `sort-by` | `(-> :function :collection :collection)` | Sorts a collection by a key function. |
| `distinct` | `(-> :collection :collection)` | Returns a new collection with duplicate values removed. |
| `frequencies` | `(-> :collection :map)` | Returns a map of the frequencies of the elements in a collection. |
| `contains?` | `(-> :collection :any :bool)` | Returns `true` if a collection contains an element. |
| `keys` | `(-> :map :vector)` | Returns a vector of the keys in a map. |
| `vals` | `(-> :map :vector)` | Returns a vector of the values in a map. |
| `get` | `(-> :map :any :any)` | Returns the value for a key in a map, or a default value. |
| `get-in` | `(-> :map :vector :any)` | Returns the value at a nested path in a map. |
| `assoc` | `(-> :map :any :any :map)` | Associates a value with a key in a map. |
| `dissoc` | `(-> :map :any :map)` | Dissociates a key from a map. |
| `conj` | `(-> :collection :any :collection)` | Adds an element to a collection. |
| `cons` | `(-> :any :collection :collection)` | Adds an element to the beginning of a collection. |
| `concat` | `(-> ... :collection)` | Concatenates two or more collections. |
| `first` | `(-> :collection :any)` | Returns the first element of a collection. |
| `rest` | `(-> :collection :collection)` | Returns all but the first element of a collection. |
| `nth` | `(-> :collection :int :any)` | Returns the element at a given index in a collection. |
| `count` | `(-> :collection :int)` | Returns the number of elements in a collection. |
| `empty?` | `(-> :collection :bool)` | Returns `true` if a collection is empty. |
| `range` | `(-> :int :int :int :vector)` | Returns a vector of numbers in a given range. |
| `hash-map` | `(-> ... :map)` | Creates a new map. |

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

## 9. Tooling Functions

These functions are impure and interact with the outside world. They are managed by the CCOS host and require appropriate capabilities.

| Function | Signature | Description |
|---|---|---|
| `tool/open-file` | `(-> :string :string)` | Reads the content of a file. |
| `tool/http-fetch` | `(-> :string :string)` | Fetches content from a URL. |
| `tool/log` | `(-> ... :nil)` | Prints arguments to the console. |
| `tool/time-ms` | `(-> :int)` | Returns the current time in milliseconds. |
| `file-exists?` | `(-> :string :bool)` | Checks if a file exists. |
| `get-env` | `(-> :string :string)` | Gets an environment variable. |
| `tool/serialize-json` | `(-> :any :string)` | Serializes an RTFS value to a JSON string. |
| `tool/parse-json` | `(-> :string :any)` | Parses a JSON string into an RTFS value. |
| `println` | `(-> ... :nil)` | Prints arguments to the console with a newline. |
| `thread/sleep` | `(-> :int :nil)` | Sleeps for a given number of milliseconds. |
| `read-lines` | `(-> :string :vector)` | Reads all lines from a file. |

## 10. CCOS Capability Functions

| Function | Signature | Description |
|---|---|---|
| `call` | `(-> :keyword ... :any)` | Calls a CCOS capability. |
| `discover-agents` | `(-> :map :vector)` | Discovers agents based on a set of criteria. |
| `task-coordination` | `(-> ... :any)` | Coordinates a task between multiple agents. |
| `establish-system-baseline` | `(-> ... :map)` | Establishes a system baseline. |
