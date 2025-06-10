# RTFS - Standard Library & Tool Interfaces (Preliminary Draft)

This document outlines a preliminary set of built-in functions and standard tool interfaces expected to be available in an RTFS environment.

## 1. Built-in Functions (Core Operations)

These functions are typically pure, operate on basic data types, and form the core computational building blocks. Special forms (`if`, `let`, `do`, `fn`, `def`, `defn`, `match`, `try`, `catch`, `finally`, `parallel`, `with-resource`, `log-step`) are defined in the language semantics and grammar, not listed here.

### 1.1. Arithmetic

*   `+`
    *   Signature: `[:=> [:cat :number :number+] [:* :number] :number]` (Takes one or more numbers, returns a number)
    *   Description: Adds numbers. Type promotion rules apply (e.g., int + float -> float).
*   `-`
    *   Signature: `[:=> [:cat :number :number*] [:* :number] :number]` (Takes one or more numbers, returns a number)
    *   Description: Subtracts subsequent numbers from the first, or negates if only one argument.
*   `*`
    *   Signature: `[:=> [:cat :number :number+] [:* :number] :number]` (Takes one or more numbers, returns a number)
    *   Description: Multiplies numbers.
*   `/`
    *   Signature: `[:=> [:cat :number :number+] [:* :number] :number]` (Takes one or more numbers, returns a number)
    *   Description: Divides the first number by subsequent numbers. Result is typically float unless specific integer division is provided elsewhere. Division by zero results in a runtime error (e.g., `{:type :error/arithmetic :message "Division by zero"}`).

### 1.2. Comparison

*   `=`
    *   Signature: `[:=> [:cat :any :any+] [:* :any] :bool]` (Takes one or more values, returns boolean)
    *   Description: Returns `true` if all arguments are equal, `false` otherwise. Defines equality for primitives and structural equality for collections.
*   `!=`
    *   Signature: `[:=> [:cat :any :any] :bool]` (Takes two values, returns boolean)
    *   Description: Returns `true` if arguments are not equal, `false` otherwise.
*   `>`
    *   Signature: `[:=> [:cat :comparable :comparable] :bool]` (Takes two comparable values, returns boolean)
    *   Description: Returns `true` if the first argument is greater than the second. `:comparable` includes numbers, strings.
*   `<`
    *   Signature: `[:=> [:cat :comparable :comparable] :bool]`
    *   Description: Returns `true` if the first argument is less than the second.
*   `>=`
    *   Signature: `[:=> [:cat :comparable :comparable] :bool]`
    *   Description: Returns `true` if the first argument is greater than or equal to the second.
*   `<=`
    *   Signature: `[:=> [:cat :comparable :comparable] :bool]`
    *   Description: Returns `true` if the first argument is less than or equal to the second.

### 1.3. Boolean Logic

*   `and`
    *   Signature: `[:=> [:cat :any*] [:* :any] :any]` (Takes zero or more arguments)
    *   Description: Evaluates arguments left-to-right. Returns the first `false` or `nil` value encountered, or the value of the last argument if all are truthy. Returns `true` if no arguments. Short-circuits.
*   `or`
    *   Signature: `[:=> [:cat :any*] [:* :any] :any]` (Takes zero or more arguments)
    *   Description: Evaluates arguments left-to-right. Returns the first truthy value encountered, or the value of the last argument if all are falsey (`false` or `nil`). Returns `nil` if no arguments. Short-circuits.
*   `not`
    *   Signature: `[:=> [:cat :any] :bool]`
    *   Description: Returns `true` if the argument is `false` or `nil`, `false` otherwise.

### 1.4. String Manipulation

*   `str`
    *   Signature: `[:=> [:cat :any*] [:* :any] :string]` (Takes zero or more arguments)
    *   Description: Concatenates the string representations of its arguments into a single string.
*   `string-length`
    *   Signature: `[:=> [:cat :string] :int]`
    *   Description: Returns the number of characters in the string.
*   `substring`
    *   Signature: `[:=> [:cat :string :int :int?] :string]` (String, start-index, optional end-index)
    *   Description: Returns a substring based on start and optional end indices.

### 1.5. Collection Manipulation

*   `get`
    *   Signature: `[:=> [:cat [:union :map :vector] :any :any?] [:union :any :nil]]` (Collection, key/index, optional default)
    *   Description: Retrieves the value associated with a key in a map or an index in a vector. Returns default value or `nil` if key/index not found. Does not signal error for missing keys/indices.
*   `assoc`
    *   Signature: `[:=> [:cat :map :any :any :any*] :map]` or `[:=> [:cat :vector :int :any] :vector]`
    *   Description: Associates values with keys in a map or updates an index in a vector. Returns a *new* collection (persistent data structures). Takes map/vector, key/index, value, and optionally more key/value pairs for maps.
*   `dissoc`
    *   Signature: `[:=> [:cat :map :any+] :map]` (Map, key1, key2...)
    *   Description: Returns a new map without the specified keys.
*   `count`
    *   Signature: `[:=> [:cat [:or :map :vector :string]] :int]`
    *   Description: Returns the number of elements/entries/characters in the collection/string.
*   `conj`
    *   Signature: `[:=> [:cat :vector :any+] :vector]` or `[:=> [:cat :map [:vector :any :any]+] :map]`
    *   Description: Adds elements to a vector (typically at the end) or key-value pairs (as `[:key :val]`) to a map. Returns a *new* collection.
*   `vector`
    *   Signature: `[:=> [:cat :any*] :vector]`
    *   Description: Creates a new vector containing the arguments.
*   `map`
    *   Signature: `[:=> [:cat [:vector :any :any]*] :map]` (Takes zero or more key-value pair vectors) or `[:=> [:cat :any*] :map]` (Takes alternating keys and values) - *Syntax TBD, prefer explicit pairs?*
    *   Description: Creates a new map from the arguments.

### 1.6. Type Predicates

*   `int?` : `[:=> [:cat :any] :bool]`
*   `float?` : `[:=> [:cat :any] :bool]`
*   `number?` : `[:=> [:cat :any] :bool]`
*   `string?` : `[:=> [:cat :any] :bool]`
*   `bool?` : `[:=> [:cat :any] :bool]`
*   `nil?` : `[:=> [:cat :any] :bool]`
*   `map?` : `[:=> [:cat :any] :bool]`
*   `vector?` : `[:=> [:cat :any] :bool]`
*   `keyword?` : `[:=> [:cat :any] :bool]`
*   `symbol?` : `[:=> [:cat :any] :bool]`
*   `fn?` : `[:=> [:cat :any] :bool]`

## 2. Standard Tool Interfaces

These represent capabilities typically provided by the RTFS runtime environment. They often involve side effects or interaction with the external world. They follow the `namespace:action` naming convention and require appropriate capability requests in the task's `:contracts`.

**Note on Handle Consumption:** Unless explicitly stated otherwise (or if the tool's purpose is inherently finalization, like `tool:close-file`), standard tools operating on resource handles (e.g., `tool:read-line`, `tool:write-line`, `tool:get-tensor-summary`) do **not** consume the handle. The handle remains valid for subsequent operations within its `with-resource` scope. Consumption primarily occurs automatically when `with-resource` exits.

### 2.1. Logging

*   `tool:log`
    *   Signature: `[:=> [:cat :string] :nil]`
    *   Capability: `{ :type :tool-call :tool-name "tool:log" }` (or similar granularity)
    *   Description: Logs a general informational message to the runtime's standard log output.
*   `tool:log-error`
    *   Signature: `[:=> [:cat :string :map?] :nil]` (Message, optional error details map)
    *   Capability: `{ :type :tool-call :tool-name "tool:log-error" }`
    *   Description: Logs an error message, potentially with structured details.

### 2.2. Basic File I/O

*   `tool:open-file`
    *   Signature: `[:=> [:cat :string :keyword] [:union [:tuple [:val :ok] [:resource FileHandle]] [:tuple [:val :error] ErrorMap]]]` (Path, Mode (`:read`, `:write`, `:append`))
    *   Capability: `{ :type :resource-access :resource-type "FileHandle" :permissions [...] :constraints {:path [...]}}`
    *   Description: Opens a file. Returns `[:ok FileHandle]` on success or `[:error ErrorMap]` on failure (e.g., file not found, permission denied). Best used within `with-resource`.
*   `tool:read-line`
    *   Signature: `[:=> [:cat [:resource FileHandle]] [:union [:tuple [:val :ok] [:union :string :nil]] [:tuple [:val :error] ErrorMap]]]`
    *   Capability: Implicitly covered by the capability granting access to the `FileHandle`.
    *   Description: Reads a line from an open `FileHandle`. Returns `[:ok string]` for a line, `[:ok nil]` at EOF, or `[:error ErrorMap]` if the handle is invalid or a read error occurs.
*   `tool:write-line`
    *   Signature: `[:=> [:cat [:resource FileHandle] :string] [:union [:tuple [:val :ok] :nil] [:tuple [:val :error] ErrorMap]]]`
    *   Capability: Implicitly covered by the capability granting access to the `FileHandle`.
    *   Description: Writes a string (appending a newline) to an open `FileHandle`. Returns `[:ok nil]` on success or `[:error ErrorMap]` on failure (e.g., invalid handle, I/O error).
*   `tool:close-file`
    *   Signature: `[:=> [:cat [:resource FileHandle]] [:union [:tuple [:val :ok] :nil] [:tuple [:val :error] ErrorMap]]]`
    *   Capability: Implicitly covered.
    *   Description: Closes the file handle. Returns `[:ok nil]` or `[:error ErrorMap]`. **Consumes** the provided `FileHandle`. **Note:** This is typically handled automatically by `with-resource` and should rarely be called directly.

### 2.3. Console I/O (Optional - Environment Dependent)

*   `tool:print`
    *   Signature: `[:=> [:cat :any*] :nil]`
    *   Capability: `{ :type :tool-call :tool-name "tool:print" }`
    *   Description: Prints string representations of arguments to standard output.
*   `tool:read-input`
    *   Signature: `[:=> [:cat :string?] [:or :string :nil]]` (Optional prompt message)
    *   Capability: `{ :type :tool-call :tool-name "tool:read-input" }`
    *   Description: Reads a line of text from standard input.

### 2.4. Environment & Time

*   `tool:get-env`
    *   Signature: `[:=> [:cat :string] [:union [:tuple [:val :ok] [:union :string :nil]] [:tuple [:val :error] ErrorMap]]]` (Variable name)
    *   Capability: `{ :type :tool-call :tool-name "tool:get-env" :constraints {:variable-name [...]}}`
    *   Description: Retrieves the value of an environment variable. Returns `[:ok string]` if found, `[:ok nil]` if not found but access was permitted, or `[:error ErrorMap]` if access was denied or another error occurred.
*   `tool:current-time`
    *   Signature: `[:=> [:cat] :string]` (Or a specific timestamp type)
    *   Capability: `{ :type :tool-call :tool-name "tool:current-time" }`
    *   Description: Returns the current system time, likely as an ISO 8601 string.

### 2.5. HTTP Client

*   `tool:http-fetch`
    *   Signature: `[:=> [:cat :string :map?] [:union [:tuple [:val :ok] [:map [:status :int] [:headers :map] [:body :string]]] [:tuple [:val :error] ErrorMap]]]` (URL, optional options map e.g., {:method :POST :headers {...} :body "..."})
    *   Capability: `{ :type :network-access :protocols [:http :https] :host [...] :port [...] }` (Constraints on host/port/protocol)
    *   Description: Performs an HTTP request (defaulting to GET). Returns `[:ok ResponseMap]` on success (containing status, headers, body as string) or `[:error ErrorMap]` on failure (e.g., network error, DNS error, invalid URL, timeout).

### 2.6. Data Serialization

*   `tool:parse-json`
    *   Signature: `[:=> [:cat :string] [:union [:tuple [:val :ok] :any] [:tuple [:val :error] ErrorMap]]]` (JSON string)
    *   Capability: `{ :type :tool-call :tool-name \"tool:parse-json\" }`
    *   Description: Parses a JSON string into corresponding RTFS data structures (maps, vectors, strings, numbers, booleans, nil). Returns `[:ok value]` or `[:error ErrorMap]` (e.g., invalid JSON syntax).
*   `tool:serialize-json`
    *   Signature: `[:=> [:cat :any] [:union [:tuple [:val :ok] :string] [:tuple [:val :error] ErrorMap]]]` (RTFS value)
    *   Capability: `{ :type :tool-call :tool-name \"tool:serialize-json\" }`
    *   Description: Serializes an RTFS value (maps, vectors, strings, numbers, booleans, nil) into a JSON string. Returns `[:ok json-string]` or `[:error ErrorMap]` (e.g., value contains non-serializable types like functions or resources).

### 2.7. Tensor/Array Operations (Illustrative)

These tools operate on tensor/array data, often represented by resource handles (`[:resource TensorHandle]`) for large or remote data, or potentially directly on `[:array ...]` types for smaller, local data. Capabilities would constrain which operations and data sources are allowed.

*   `tool:load-remote-tensor`
    *   Signature: `[:=> [:cat :string :map?] [:union [:tuple [:val :ok] [:resource TensorHandle]] [:tuple [:val :error] ErrorMap]]]` (Source URI, optional options map)
    *   Capability: `{ :type :tool-call :tool-name \"tool:load-remote-tensor\" :constraints {:args {:source-uri [:string-starts-with \"trusted-prefix://\"]}} }` (Example constraint)
    *   Description: Loads tensor data from a remote source. Returns `[:ok TensorHandle]` or `[:error ErrorMap]`.
*   `tool:tensor-elementwise-add`
    *   Signature: `[:=> [:cat [:resource TensorHandle] [:resource TensorHandle]] [:union [:tuple [:val :ok] [:resource TensorHandle]] [:tuple [:val :error] ErrorMap]]]` (Handle A, Handle B)
    *   Capability: `{ :type :tool-call :tool-name \"tool:tensor-elementwise-add\" }`
    *   Description: Performs element-wise addition. Returns `[:ok ResultHandle]` or `[:error ErrorMap]` (e.g., shape mismatch, resource error).
*   `tool:tensor-scalar-multiply`
    *   Signature: `[:=> [:cat [:resource TensorHandle] :number] [:union [:tuple [:val :ok] [:resource TensorHandle]] [:tuple [:val :error] ErrorMap]]]` (Handle, Scalar)
    *   Capability: `{ :type :tool-call :tool-name \"tool:tensor-scalar-multiply\" }`
    *   Description: Multiplies the tensor by a scalar. Returns `[:ok ResultHandle]` or `[:error ErrorMap]`.
*   `tool:get-tensor-summary`
    *   Signature: `[:=> [:cat [:resource TensorHandle]] [:union [:tuple [:val :ok] :map] [:tuple [:val :error] ErrorMap]]]` (Handle)
    *   Capability: `{ :type :tool-call :tool-name \"tool:get-tensor-summary\" }`
    *   Description: Returns a summary map. Returns `[:ok map]` or `[:error ErrorMap]` (e.g., invalid handle).
*   `tool:release-tensor`
    *   Signature: `[:=> [:cat [:resource TensorHandle]] [:union [:tuple [:val :ok] :nil] [:tuple [:val :error] ErrorMap]]]` (Handle)
    *   Capability: Implicitly covered by handle ownership or specific release capability.
    *   Description: Releases the external resources associated with the tensor handle. Returns `[:ok nil]` or `[:error ErrorMap]`. **Consumes** the provided `TensorHandle`. **Note:** Typically handled automatically by `with-resource`.

*(This list is preliminary and needs further refinement regarding specific function behaviors, error handling, type details, and the set of included items. Assumes `ErrorMap` is defined as in `type_system.md`)*
