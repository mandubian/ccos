# RTFS - Detailed Examples

This document provides various examples demonstrating the usage of RTFS language features based on the specifications.

## 1. Basic Control Flow & Variables

```acl
(task :id "task-basic-flow"
  :intent {:description "Demonstrate basic control flow"}
  :plan
  (do
    (def initial-value :int 10)
    (let [threshold :int 5
          processed-value :int (if (> initial-value threshold)
                                 (* initial-value 2) ;; 'then' branch
                                 (+ initial-value 1))] ;; 'else' branch
      ;; Check the processed value
      (if (= processed-value 20)
        (tool:log "Processing successful, value doubled.")
        (tool:log "Processing resulted in increment."))
      ;; Return the final value
      processed-value)))
```
*   Uses `def` to define a task-scoped variable.
*   Uses `let` for local bindings within the `do` block.
*   Demonstrates `if` for conditional logic.
*   Uses `do` for sequential execution. The result of `do` is the result of its last expression.

## 2. Error Handling: `match` with Result Types

This is the preferred way to handle *expected* alternative outcomes from functions or tool calls. Assume `tool:fetch-data` returns `[:ok data-map]` or `[:error error-map]`.

```acl
(task :id "task-match-error"
  :intent {:description "Fetch data and handle potential errors using match"}
  :plan
  (let [fetch-result (tool:fetch-data "http://example.com/data")]
    (match fetch-result
      ;; Success case: Destructure the :ok vector
      [:ok data]
      (do
        (tool:log "Data fetched successfully.")
        ;; Further processing on 'data'
        (process-data data))

      ;; Specific error case: Destructure the :error vector and the error map
      [:error {:type :error/network :message msg}]
      (do
        (tool:log (str "Network error fetching data: " msg))
        ;; Return a default value or signal failure
        nil)

      ;; Another specific error case
      [:error {:type :error/not-found}]
      (do
        (tool:log "Data not found at URL.")
        nil)

      ;; Catch-all for other errors (destructuring the map)
      [:error error-info]
      (do
        (tool:log (str "An unexpected error occurred: " (:message error-info)))
        (tool:report-error error-info) ;; Maybe report it
        nil)

      ;; Optional: Default case if result structure is unexpected (not :ok or :error)
      _ ;; Wildcard pattern
      (do
        (tool:log "Unexpected result structure from tool:fetch-data")
        nil))))
```
*   Uses `match` to handle the tagged vector returned by `tool:fetch-data`.
*   Demonstrates destructuring for both the `[:ok ...]` and `[:error ...]` cases.
*   Shows matching on specific error `:type`s within the error map.
*   Includes a catch-all `[:error error-info]` case and a final wildcard `_` case.

## 3. Error Handling: `try/catch` for Runtime Exceptions

Use `try/catch` primarily for unexpected runtime errors or exceptions propagated directly from tool integrations (less common for well-behaved tools that use result types).

```acl
(task :id "task-try-catch"
  :intent {:description "Demonstrate try/catch for runtime issues"}
  :plan
  (try
    ;; Code that might cause a runtime issue (e.g., division by zero if not checked)
    ;; Or a tool call that might throw an exception instead of returning [:error ...]
    (let [divisor :int (tool:get-divisor)]
       (if (= divisor 0)
         (tool:log "Divisor is zero, skipping division.") ;; Avoid runtime error
         (let [result (/ 10 divisor)]
           (tool:log (str "Result: " result)))))

    ;; Catch a specific category of error based on the standard error map :type
    (catch :error/runtime err ;; 'err' binds the full error map
      (do
        (tool:log (str "Caught a runtime error: " (:message err)))
        ;; Perform cleanup or return a default
        :runtime-error-occurred))

    ;; Catch any other error (variable pattern implies :any)
    (catch other-error
      (do
        (tool:log (str "Caught an unexpected exception: " (:message other-error)))
        :unknown-exception))

    ;; Optional: Finally block always executes
    (finally
      (tool:log "Exiting try/catch block."))))
```
*   Uses `try` to wrap potentially problematic code.
*   Uses `catch` with a keyword pattern (`:error/runtime`) to catch specific error types based on the `:type` field in the error map.
*   Uses `catch` with a variable pattern (`other-error`) to catch any other error/exception and bind it.
*   Includes an optional `finally` block for cleanup actions.

## 4. Resource Management: `with-resource`

Ensures resources like file handles are properly acquired and released.

```acl
(task :id "task-with-resource"
  :intent {:description "Read from one file and write to another using with-resource"}
  :contracts {:capabilities-required [ ;; Request capability to read/write files
                 { :type :resource-access :resource-type "FileHandle" :permissions [:read]
                   :constraints {:path [:= "input.txt"]} } ;; Constraint: only input.txt
                 { :type :resource-access :resource-type "FileHandle" :permissions [:write]
                   :constraints {:path [:= "output.txt"]} } ;; Constraint: only output.txt
               ]}
  :plan
  (do
    (tool:log "Starting file processing.")
    ;; Acquire input file handle (read mode) - Use correct resource type syntax
    (with-resource [in-handle [:resource FileHandle] (tool:open-file "input.txt" :mode :read)]
      ;; Acquire output file handle (write mode) - Use correct resource type syntax
      (with-resource [out-handle [:resource FileHandle] (tool:open-file "output.txt" :mode :write)]
        ;; Read line by line from input
        (loop [line :string (tool:read-line in-handle)]
          (when (not (nil? line)) ;; Check if EOF reached
            ;; Process and write to output
            (let [processed-line :string (str "Processed: " line)]
              (tool:write-line out-handle processed-line))
            ;; Recur for next line
            (recur (tool:read-line in-handle)))))
      ;; out-handle is automatically closed here
      )
    ;; in-handle is automatically closed here
    (tool:log "File processing finished.")))
```
*   Uses nested `with-resource` to manage input and output file handles.
*   Specifies the expected resource type (`FileHandle`).
*   Handles are automatically closed when the `with-resource` block exits, even if errors occur within the block (assuming `try/catch` is used appropriately at a higher level if needed).
*   Includes capability requests with constraints on file paths.

## 5. Concurrency: `parallel`

Executes independent operations concurrently and collects results in a map.

```acl
(task :id "task-parallel"
  :intent {:description "Fetch data from multiple sources in parallel"}
  :contracts {:capabilities-required [ ;; Request capability for multiple tool calls
                 { :type :tool-call :tool-name "tool:fetch" }
               ]}
  :plan
  (do
    (tool:log "Starting parallel fetch.")
    ;; Define sources
    (let [source-a :string "http://example.com/sourceA"
          source-b :string "http://example.com/sourceB"]
      ;; Execute fetches in parallel
      (let [results :map (parallel ;; Result is a map {:data-a ..., :data-b ...}
                           [data-a :string (tool:fetch source-a)] ;; Binding ID becomes map key
                           [data-b :string (tool:fetch source-b)])]
        ;; Process the combined results (available once both fetches complete)
        (tool:log (str "Fetched A: " (:data-a results)))
        (tool:log (str "Fetched B: " (:data-b results)))
        ;; Combine or further process results
        (combine-data (:data-a results) (:data-b results))))))
```
*   Uses `parallel` to run two `tool:fetch` calls concurrently.
*   The binding identifiers (`data-a`, `data-b`) become the keys in the resulting map.
*   The `let` block containing `parallel` waits until all parallel operations complete before proceeding.
*   Error handling: If any parallel branch fails, the `parallel` form immediately fails, propagating the first error encountered. Consider wrapping individual parallel branches in `try/catch` or using `match` if fine-grained error handling per branch is needed.

## 6. Complex Task Definition (Contracts, Types, Context)

Illustrates more detailed contracts, schema definitions with predicates, and task context access.

```acl
(task :id "task-complex-contracts"
  :intent { ;; Structured intent
    :action :process-user-data
    :user-id :int 123
    :options [:map [:notify? :bool] [:threshold :float]]
  }
  :contracts {
    ;; Input schema: Expects user data map
    :input-schema [:map
                     [:user-id :int]
                     [:data [:map
                              [:name :string]
                              [:email [:and :string [:string-contains "@"]]]? ;; Optional email, basic check
                              [:values [:vector :float]]]]]
    ;; Output schema: Guarantees a status and optional message
    :output-schema [:map
                      [:status [:enum :success :failure]] ;; Status must be one of these keywords
                      [:message :string?]] ;; Optional message
    ;; Capabilities: Requires specific tool and constrained network access
    :capabilities-required [
      { :type :tool-call :tool-name "tool:analyze-data:v2" }
      { :type :network-access :host "internal-stats.local" :port 8080 }
    ]
  }
  :plan
  (do
    (tool:log (str "Starting processing for user: " (:user-id @intent))) ;; Access @intent

    ;; Validate input against schema (runtime might do this implicitly based on contract)
    ;; (validate-input input :input-schema) ;; Conceptual validation step

    (let [user-data :map (:data input) ;; Assuming 'input' holds data matching :input-schema
          threshold :float (:threshold (:options @intent)) ;; Access nested intent data
          analysis-result :map (tool:analyze-data (:values user-data) :threshold threshold)]

      (if (:critical analysis-result)
        (do
          (when (:notify? (:options @intent)) ;; Check boolean option in intent
             (tool:send-alert "Critical value detected" (:user-id @intent)))
          ;; Return failure status matching output schema
          { :status :failure :message "Critical value detected during analysis." })
        ;; Return success status matching output schema
        { :status :success }))))
```
*   Defines detailed `:input-schema` and `:output-schema` using map, vector, optional (`?`), refinement (`:and`), and enum (`[:enum ...]`) types.
*   Specifies `:capabilities-required` including tool calls and constrained network access.
*   Accesses task-level data using `@intent`.
*   Structures the return value to match the `:output-schema`.

## 7. Logging Specific Steps: `log-step`

Wraps an expression to ensure its execution (start, end, result/error) is explicitly logged in the execution trace.

```acl
(task :id "task-log-step"
  :intent {:description "Demonstrate logging a specific step"}
  :plan
  (do
    (let [input-val 10]
      ;; Log the execution of this specific calculation
      (log-step :id "critical-calculation-1"
        (let [intermediate (* input-val 5)]
          ;; ... more complex logic ...
          (/ intermediate 2))) ;; The result of this expression is logged

      ;; Other steps proceed as normal
      (tool:another-operation))))
```
*   The `log-step` form wraps the expression `(/ intermediate 2)`.
*   The runtime ensures that log entries corresponding to the start and end (including result or error) of evaluating the wrapped expression, tagged with `"critical-calculation-1"`, are added to the `:execution-trace`.

## 8. Module Definition and Usage

Illustrates defining a simple module and importing it with an alias.

```acl
;; File: my/math.rtfs
(module my.math
  (:exports [add]) ;; Export only the 'add' function

  (def private-pi 3.14159) ;; Not exported

  (defn add [x :number y :number] :number
    "Adds two numbers."
    (+ x y))

  (defn subtract [x :number y :number] :number ;; Not exported
    "Subtracts two numbers."
    (- x y))
)

;; File: main.rtfs (or within a task plan)
(do
  ;; Import the 'my.math' module and alias it as 'm'
  (import my.math :as m)

  ;; Call the exported function using the alias and namespace separator '/'
  (let [result (m/add 5 3)]
    (tool:print (str "Result of m/add: " result))) ;; Output: Result of m/add: 8

  ;; Attempting to call a non-exported function would fail (at compile/load time or runtime)
  ;; (m/subtract 10 2) ; Error: 'subtract' is not exported from my.math
  ;; (m/private-pi)    ; Error: 'private-pi' is not exported from my.math
)
```

This example shows:
*   Defining a module with a namespaced name (`my.math`).
*   Using `:exports` to control public visibility.
*   Importing a module with an alias (`:as m`).
*   Calling an exported function using the `alias/symbol` syntax (`m/add`).

## 9. Function Definition with Named Parameters (Map Destructuring)

Demonstrates defining a function that accepts named/optional arguments using map destructuring in its parameter list.

```acl
(task :id "task-named-params"
  :intent {:description "Show function definition with named parameters via destructuring"}
  :plan
  (do
    ;; Define the function
    (defn create-user
      "Creates a user record. Requires username, allows optional email and age with defaults."
      ;; Parameter list uses map destructuring
      [{:keys [username email age] ;; Specify keys to extract
        :or {age 18 email "default@example.com"} ;; Provide defaults for age and email
        :as user-data}] ;; Bind the whole input map to user-data
      ;; Type hint for the input map (optional but good practice)
      : [:map [:username :string] [:email :string?] [:age :int?]]
      ;; Return type
      :map

      (do
        (tool:log (str "Creating user: " username
                       ", Email: " email
                       ", Age: " age))
        ;; Simulate creating and returning the user record
        ;; Using 'user-data' which contains the original input map
        (assoc user-data :id (tool:generate-id) :created-at (tool:current-time))))

    ;; Call the function
    (let [user1 :map (create-user {:username "Alice"}) ;; Uses default email and age
          user2 :map (create-user {:username "Bob" :email "bob@host.com"}) ;; Uses default age
          user3 :map (create-user {:username "Charlie" :age 30 :email "charlie@site.net"}) ;; Provides all
          ]
      (tool:log (str "Created User 1: " user1))
      (tool:log (str "Created User 2: " user2))
      (tool:log (str "Created User 3: " user3))
      ;; Return the last user created
      user3)))
```
*   The `create-user` function takes one argument: a map.
*   The parameter list `[{:keys [...] :or {...} :as ...}]` destructures this map.
*   `:keys [username email age]` binds local variables `username`, `email`, `age` to the values of the corresponding keys (`:username`, `:email`, `:age`) in the input map.
*   `:or {age 18 email "..."}` provides default values if `:age` or `:email` keys are missing from the input map.
*   `:as user-data` binds the original, complete input map to the variable `user-data`.
*   Type hints can be provided for the destructured map argument itself.
*   Function calls simply pass a map literal containing the desired key-value pairs.

## 10. Secure Remote Tensor Manipulation

This example shows how to load a tensor from a trusted remote source, perform an operation on it using tools, and manage it securely via resource handles and capabilities.

```acl
(task :id "task-remote-tensor"
  :intent {:description "Load a remote tensor, add 5.0 to it, and get its summary."
           :source-uri "trusted-repo://models/tensor-a.bin"
           :scalar-value 5.0}
  :contracts {
    :input-schema [:map] ;; No specific input data needed for this example
    :output-schema [:map ;; Expecting a summary map
                      [:shape [:vector :int]]
                      [:dtype :keyword]
                      [:mean :float]]
    :capabilities-required [
      ;; Capability to load tensors, constrained to specific URI scheme
      { :type :tool-call
        :tool-name "tool:load-remote-tensor"
        :constraints {:args {:source-uri [:string-starts-with "trusted-repo://"]}} }
      ;; Capability to perform scalar multiplication
      { :type :tool-call
        :tool-name "tool:tensor-scalar-multiply" }
      ;; Capability to get tensor summary
      { :type :tool-call
        :tool-name "tool:get-tensor-summary" }
      ;; Implicit capability to manage TensorHandle resources via with-resource
      ;; (May require explicit { :type :resource-access :resource-type "TensorHandle" } depending on runtime)
    ]
  }
  :plan
  (do
    (tool:log "Starting remote tensor manipulation.")
    (let [source :string (:source-uri @intent)
          scalar :float (:scalar-value @intent)]

      ;; Use with-resource to manage the lifecycle of tensor handles
      ;; Handles are automatically released when the block exits
      (with-resource [original-handle [:resource TensorHandle] (tool:load-remote-tensor source)]
        (tool:log (str "Loaded tensor from: " source))

        (with-resource [result-handle [:resource TensorHandle] (tool:tensor-scalar-multiply original-handle scalar)]
          (tool:log (str "Performed scalar multiplication: " scalar))

          ;; Get summary of the resulting tensor
          (let [summary :map (tool:get-tensor-summary result-handle)]
            (tool:log (str "Result summary: " summary))
            ;; Return a map matching the output schema
            { :shape (:shape summary)
              :dtype (:dtype summary)
              :mean (:mean summary) })
          ;; result-handle is released here
          )
        ;; original-handle is released here
        ))
    ;; Return value of the 'do' block is the result of the last expression (the summary map)
    ))
```
*   Uses `[:resource TensorHandle]` to represent the remote tensor.
*   Calls `tool:load-remote-tensor`, `tool:tensor-scalar-multiply`, and `tool:get-tensor-summary` which operate on handles.
*   Uses nested `with-resource` to ensure the external resources associated with the handles are eventually released by the runtime (calling the appropriate cleanup logic, like `tool:release-tensor`, implicitly).
*   Declares specific `:capabilities-required` in `:contracts`, including a constraint on the allowed source URI for loading.
*   The runtime verifies these capabilities before allowing the tool calls.

## 11. Pattern Matching: `match`

Selects a code branch based on the structure of a value.

```acl
(task :id "task-match"
  :intent {:description "Demonstrate pattern matching with match"}
  :plan
  (do
    (let [value :any [:config {:port 8080 :host "localhost"}]]
      (match value
        ;; Match a specific vector structure
        [:config {:port p :host h}] (tool:log (str "Config found: Host=" h ", Port=" p))
        ;; Match any vector starting with :data
        [:data d] (tool:log (str "Data found: " d))
        ;; Match a specific keyword
        :error (tool:log "An error keyword was encountered.")
        ;; Default case using wildcard _
        _ (tool:log (str "Unknown value structure: " value))))))
```
*   Uses `match` to inspect the structure of `value`.
*   Demonstrates literal matching (`:error`), structural matching with binding (`[:config {:port p :host h}]`), and wildcard (`_`).

## 12. Handling Results with `match`

Demonstrates using `match` to handle the standard `Result<T>` pattern (`[:ok Value]` or `[:error ErrorMap]`) returned by fallible tools.

```acl
(task :id "task-handle-result"
  :intent {:description "Parse JSON and handle potential success or failure using match"}
  :contracts {:capabilities-required [{:type :tool-call :tool-name "tool:parse-json"}]}
  :plan
  (do
    (let [json-string :string "{\"name\": \"RTFS\", \"version\": 0.1}"
          ;; tool:parse-json returns [:union [:tuple [:val :ok] :any] [:tuple [:val :error] ErrorMap]]
          parse-result (tool:parse-json json-string)]

      (match parse-result
        ;; Success case: Match the [:ok value] tuple and bind 'value'
        [:ok value]
        (do
          (tool:log (str "JSON parsed successfully: " value))
          ;; Further processing using 'value'
          (get value :version)) ;; Example: return the version

        ;; Error case: Match the [:error error-map] tuple and bind 'err-map'
        [:error err-map]
        (do
          (tool:log-error (str "JSON parsing failed: Type=" (get err-map :type) ", Msg=" (get err-map :message)))
          ;; Return a default value or signal failure
          nil)

        ;; Optional: A wildcard case for unexpected results (though Result<T> should cover all)
        _
        (do
          (tool:log-error (str "Unexpected result from tool:parse-json: " parse-result))
          nil)
        ))))
```
*   Calls `tool:parse-json`, which returns a `Result<:any>`.
*   Uses `match` to destructure the result.
*   The first clause `[:ok value]` handles successful parsing, binding the parsed data structure to `value`.
*   The second clause `[:error err-map]` handles parsing errors, binding the standard `ErrorMap` to `err-map`.
*   Demonstrates accessing the parsed value or the error details within the respective clauses.



