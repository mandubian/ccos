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

## 13. Task Interacting with an Agent Profile

These examples illustrate how an RTFS task can declare dependencies on capabilities provided by an external agent (defined via an `agent-profile`) and then invoke those capabilities, including streaming.

### 13.1 Agent Profile Definition Example

First, let's define an agent profile for a hypothetical "Polyglot Agent" that offers translation services.

```acl
;; File: polyglot-agent-profile.rtfs
(agent-profile :id "polyglot-agent-v1"
  :metadata {
    :name "Polyglot Translation Agent"
    :version "1.2.0"
    :description "Provides text translation and continuous translation feed services."
    :owner "linguistics-inc"
  }

  :capabilities [
    { :capability-id "translate-text-batch-v1.2"
      :description "Translates a batch of texts from a source language to a target language."
      :type :task
      :input-schema [:map 
                       [:texts [:vector :string]]
                       [:source-language :keyword]
                       [:target-language :keyword]]
      :output-schema [:map 
                        [:translations [:vector :string]]
                        [:detected-source-language :keyword?]]
      :annotations { :cost :medium :batch-size-limit 100 }
    }
    { :capability-id "live-translation-feed-v1.0"
      :description "Provides a continuous stream of translations for incoming text chunks."
      :type :stream-source
      :input-schema [:map ;; Parameters to start the stream
                       [:source-language :keyword]
                       [:target-language :keyword]
                       [:filter-topics [:vector :string]?]]
      :output-schema [:map ;; Schema for each item in the output stream
                        [:original-chunk :string]
                        [:translated-chunk :string]
                        [:timestamp :string]]
      :annotations { :real-time true }
    }
  ]

  :communication-endpoints [
    { :endpoint-id "main-jsonrpc"
      :protocol :json-rpc
      :transport :http
      :uri "https://api.polyglot-agent.example.com/rpc"
      :details {
        :http-methods [:POST]
        :authentication { :type :oauth2 :flow :client-credentials }
      }
      :provides-capabilities ["translate-text-batch-v1.2"]
    }
    { :endpoint-id "stream-ws"
      :protocol :websocket
      :transport :wss
      :uri "wss://stream.polyglot-agent.example.com/feed"
      :details {
        :message-format :json
        :authentication { :type :bearer-token }
        :stream-options {
          :subscription-message-schema [:map 
                                         [:action :subscribe]
                                         [:feed-id :string] ;; e.g., "live-translation-feed-v1.0"
                                         [:params [:map [:source-language :keyword] [:target-language :keyword]]]]
          :unsubscription-message-schema [:map [:action :unsubscribe] [:feed-id :string]]
        }
      }
      :provides-capabilities ["live-translation-feed-v1.0"]
    }
  ]

  :discovery-mechanisms [
    { :type :registry
      :registry-uri "https://rtfs-registry.example.com/agents"
      :registration-id "polyglot-agent-v1"
    }
  ]
)
```

### 13.2 Task Requiring and Invoking a Standard Agent Capability

This task needs to translate a document using the `translate-text-batch-v1.2` capability from the `polyglot-agent-v1`.

```acl
(task :id "task-translate-doc"
  :intent {
    :action :translate-document
    :document-content "Hello world. This is an example."
    :target-language :es
  }
  :contracts {
    :input-schema [:map [:document-content :string] [:target-language :keyword]]
    :output-schema [:map [:translated-document :string]]
    :requires [ ;; Declaring dependency on an external capability
      { :capability-id "polyglot-agent-v1/translate-text-batch-v1.2" ;; Fully qualified ID: <agent-id>/<capability-id>
        ;; Optional: :agent-profile-uri "./polyglot-agent-profile.rtfs" (if known locally)
        :alias translator ;; Local alias for use in the plan
        :timeout-ms 5000
        :retry-policy { :max-attempts 3 :delay-ms 1000 }
      }
    ]
  }
  :plan
  (do
    (tool:log "Starting document translation.")
    (let [content :string (:document-content @input) ;; Assuming @input is populated based on :input-schema
          target-lang :keyword (:target-language @input)]
      
      ;; Invoke the aliased capability
      (let [translation-result :map (invoke translator
                                        ;; Arguments map, matching the capability's :input-schema
                                        { :texts [content] 
                                          :source-language :en ;; Assuming source is English
                                          :target-language target-lang }
                                        ;; Optional: Override invocation options from :contracts/:requires
                                        { :timeout-ms 7000 })]
        
        (match translation-result
          ;; Assuming 'invoke' itself returns a result structure or propagates errors
          ;; For simplicity, let's assume direct success or failure from invoke
          ;; A more robust invoke might return [:ok actual-output] | [:error details]
          
          ;; If invoke returns the direct output-schema of the capability on success:
          { :translations translated-texts }
          (do
            (tool:log "Translation successful.")
            { :translated-document (first translated-texts) })
          
          ;; If invoke can return an error structure (e.g. if it wraps the call)
          [:error err-details]
          (do
            (tool:log-error (str "Translation failed: " err-details))
            { :translated-document "[Translation Failed]" })

          ;; Fallback if the structure is unexpected (should align with how 'invoke' behaves)
          _ 
          (do
            (tool:log-error (str "Unexpected result from translation service: " translation-result))
            { :translated-document "[Unexpected Translation Result]" }
          )
        )
      )
    )
  )
)
```

### 13.3 Task Consuming a Stream from an Agent

This task connects to the `live-translation-feed-v1.0` capability to process a stream of translations.

```acl
(task :id "task-live-translate-feed-consumer"
  :intent {
    :action :monitor-live-translations
    :source-language :en
    :target-language :fr
    :duration-seconds 60
  }
  :contracts {
    :requires [
      { :capability-id "polyglot-agent-v1/live-translation-feed-v1.0"
        :alias live-translator
      }
    ]
  }
  :plan
  (do
    (tool:log "Starting to consume live translation feed.")
    (let [source-lang :keyword (:source-language @intent)
          target-lang :keyword (:target-language @intent)
          monitoring-duration :int (:duration-seconds @intent)]

      ;; Consume the stream provided by the 'live-translator' capability
      (consume-stream live-translator
        ;; Parameters to initiate the stream, matching capability's :input-schema
        { :source-language source-lang 
          :target-language target-lang }
        
        ;; Handler block for each item from the stream
        ;; 'item' will be a map matching the capability's :output-schema for stream items
        { item => 
          (do
            (tool:log (str "Received live translation: " (:original-chunk item) " -> " (:translated-chunk item)))
            ;; Further processing of 'item'
            (tool:store-translation item)) ;; Example: store each translated item
        }
        
        ;; Optional: Stream-level options
        { :on-error (fn [err] (tool:log-error (str "Stream error: " err)))
          :on-complete (fn [] (tool:log "Live translation stream completed."))
          :timeout-ms (* monitoring-duration 1000) ;; Overall timeout for the consumption
        }
      )
      (tool:log "Finished consuming live translation feed (or timed out).")))
)
```

## 14. End-to-End Example: Human Request to MCP Tool Call and Logging

This example demonstrates a complete flow: a human request is translated into an RTFS task intent, an RTFS task is defined to handle this intent by calling an external MCP (Model Context Protocol) agent, and finally, a simulated execution log shows the interaction.

### 14.1 Human Request

"What's the current temperature in Paris in Celsius?"

### 14.2 Generated RTFS Task Intent

An RTFS-compatible system (e.g., an orchestrator or a higher-level AI) would parse the human request into a structured task intent:

```acl
{
  :action :get-current-weather
  :location "Paris"
  :units "Celsius"
  :context {
    :original-request "What's the current temperature in Paris in Celsius?"
    :user-id "user-7742"
    :session-id "session-b3f9-4a1c"
    :preferred-language "en-US"
  }
}
```

### 14.3 Hypothetical MCP Weather Agent

Assume an MCP agent exists that provides weather information. Its (simplified) profile and relevant tool might look like this:

*   **Agent Profile ID:** `weather-mcp-agent-profile-v1`
*   **Tool ID (within Agent Profile):** `get-current-temperature`
*   **MCP Tool Input Schema (JSON Schema):**
    ```json
    {
      "type": "object",
      "properties": {
        "city": { "type": "string", "description": "The city name." },
        "units": { "type": "string", "enum": ["Celsius", "Fahrenheit"], "default": "Celsius" }
      },
      "required": ["city"]
    }
    ```
*   **MCP Tool Output Schema (JSON Schema):**
    ```json
    {
      "type": "object",
      "properties": {
        "city": { "type": "string" },
        "temperature": { "type": "number" },
        "units": { "type": "string" },
        "condition": { "type": "string", "description": "e.g., Sunny, Cloudy" },
        "humidity": { "type": "number", "description": "Percentage" }
      },
      "required": ["city", "temperature", "units", "condition"]
    }
    ```
This agent profile would be discoverable through an agent registry.

### 14.4 RTFS Task Definition

The RTFS task to handle the intent and call the MCP agent:

```acl
(task :id "fetch-city-temperature-mcp"
  :version "1.0.0"
  :intent-schema { ;; Schema for the intent this task can handle
    :action [:val :get-current-weather]
    :location :string
    :units [:enum "Celsius" "Fahrenheit"]
    :context :map? ;; Optional context map
  }
  :contracts {
    :input-schema { ;; Task's own input, derived from intent
      :city :string
      :temperature-units [:enum "Celsius" "Fahrenheit"]
    }
    :output-schema { ;; Task's own output
      :city :string
      :current-temperature :float
      :units :string
      :weather-condition :string
      :details :string
    }
    :requires [
      { ;; Declare dependency on the MCP tool
        :capability-id "weather-mcp-agent-profile-v1/get-current-temperature"
        :alias weather-service ;; Local alias for the plan
        :protocol :mcp ;; Explicitly state it's an MCP tool
        ;; Input schema for the *required* capability (matches MCP tool's input)
        :input-schema [:map 
                         [:city :string] 
                         [:units [:enum "Celsius" "Fahrenheit"]?]]
        ;; Output schema for the *required* capability (matches MCP tool's output)
        :output-schema [:map
                         [:city :string]
                         [:temperature :number]
                         [:units :string]
                         [:condition :string]
                         [:humidity :number?]]
        :timeout-ms 10000
        :retry-policy { :max-attempts 2 :delay-ms 500 }
      }
    ]
  }
  :plan
  (do
    (tool:log (str "Preparing to fetch weather for city: " (:city @input) 
                   " in " (:temperature-units @input) " units."))

    ;; Invoke the MCP weather service
    (let [mcp-params {:city (:city @input) :units (:temperature-units @input)}
          weather-result (invoke weather-service mcp-params)]
      
      (match weather-result
        ;; Success case: MCP tool returned data matching its output schema
        { :city city-name 
          :temperature temp 
          :units units-val 
          :condition cond 
          :humidity hum? } ;; humidity is optional
        (do
          (tool:log (str "Successfully received weather data from MCP agent for " city-name))
          ;; Construct the task's output based on the MCP response
          { :city city-name
            :current-temperature (to-float temp) ;; Ensure float type
            :units units-val
            :weather-condition cond
            :details (str "Current weather in " city-name " is " temp " " units-val ", " cond 
                          (if hum? (str ", with " hum? "% humidity.") "."))
          })

        ;; Error case from 'invoke' (e.g., timeout, network issue, MCP error response)
        [:error error-details]
        (do
          (tool:log-error (str "Error calling MCP weather service: " error-details))
          ;; Return a structured error output matching task's :output-schema (partially)
          { :city (:city @input)
            :current-temperature -999.0 ;; Indicate error
            :units (:temperature-units @input)
            :weather-condition "Unknown"
            :details (str "Failed to retrieve weather. Error: " (:message error-details "N/A"))
          })
        
        ;; Fallback for unexpected structure from 'invoke' (should be rare)
        _
        (do
          (tool:log-error (str "Unexpected response structure from MCP weather service: " weather-result))
          { :city (:city @input)
            :current-temperature -999.0
            :units (:temperature-units @input)
            :weather-condition "Error"
            :details "Unexpected error during weather data retrieval."
          })
      )
    )
  )
)
```

### 14.5 Simulated Execution Log

The RTFS runtime would produce a detailed execution log. This log can be represented in various formats. Below are examples in RTFS format (native to the RTFS ecosystem) and JSON format (common for interoperability).

#### 14.5.1 RTFS Format

This format uses RTFS data structures (vectors of maps with keyword keys) for the log entries.
It's important to note that for fields like `:payload` in `:agent-request` and `:agent-response` events that represent data for external protocols (like MCP, which uses JSON), the RTFS representation within the log aims to accurately reflect the structure of that external data. Thus, string keys from JSON are preserved as string keys in these specific nested maps, rather than being converted to keywords, to maintain fidelity with the external protocol. Other parts of the log use keywords as is idiomatic for RTFS.

```acl
;; Vector of log events
[
  { ;; Event 1: task-start
    :event-type :task-start
    :timestamp "2025-06-08T10:30:01.100Z"
    :task-id "fetch-city-temperature-mcp"
    :task-version "1.0.0"
    :invocation-id "inv-xyz-789"
    :intent {
      :action :get-current-weather
      :location "Paris"
      :units "Celsius"
      :context {
        :original-request "What's the current temperature in Paris in Celsius?"
        :user-id "user-7742"
        :session-id "session-b3f9-4a1c"
        :preferred-language "en-US"
      }
    }
    :input {
      :city "Paris"
      :temperature-units "Celsius"
    }
  }
  { ;; Event 2: log-message
    :event-type :log-message
    :timestamp "2025-06-08T10:30:01.105Z"
    :invocation-id "inv-xyz-789"
    :level :info
    :message "Preparing to fetch weather for city: Paris in Celsius units."
  }
  { ;; Event 3: log-step-start
    :event-type :log-step-start
    :timestamp "2025-06-08T10:30:01.110Z"
    :invocation-id "inv-xyz-789"
    :step-id "invoke-weather-service-1"
    :step-type :agent-call
    :details {
      :alias "weather-service"
      :capability-id "weather-mcp-agent-profile-v1/get-current-temperature"
      :protocol :mcp
    }
  }
  { ;; Event 4: agent-request
    :event-type :agent-request
    :timestamp "2025-06-08T10:30:01.115Z"
    :invocation-id "inv-xyz-789"
    :step-id "invoke-weather-service-1"
    :protocol :mcp
    :direction :request
    :target-agent-id "weather-mcp-agent-profile-v1"
    :target-tool-id "get-current-temperature"
    :payload { ;; RTFS map representing the JSON payload sent to MCP. String keys are preserved.
      "city" "Paris"
      "units" "Celsius"
    }
  }
  { ;; Event 5: agent-response
    :event-type :agent-response
    :timestamp "2025-06-08T10:30:01.950Z"
    :invocation-id "inv-xyz-789"
    :step-id "invoke-weather-service-1"
    :protocol :mcp
    :direction :response
    :source-agent-id "weather-mcp-agent-profile-v1"
    :source-tool-id "get-current-temperature"
    :status :success
    :payload { ;; RTFS map representing the JSON payload received from MCP. String keys are preserved.
      "city" "Paris"
      "temperature" 22.5
      "units" "Celsius"
      "condition" "Partly Cloudy"
      "humidity" 65
    }
  }
  { ;; Event 6: log-message
    :event-type :log-message
    :timestamp "2025-06-08T10:30:01.955Z"
    :invocation-id "inv-xyz-789"
    :level :info
    :message "Successfully received weather data from MCP agent for Paris"
  }
  { ;; Event 7: log-step-end
    :event-type :log-step-end
    :timestamp "2025-06-08T10:30:01.960Z"
    :invocation-id "inv-xyz-789"
    :step-id "invoke-weather-service-1"
    :status :success
    :duration-ms 850
    :result { ;; The internal RTFS result of the (invoke ...) expression
      :city "Paris"
      :temperature 22.5
      :units "Celsius"
      :condition "Partly Cloudy"
      :humidity 65
    }
  }
  { ;; Event 8: task-end
    :event-type :task-end
    :timestamp "2025-06-08T10:30:01.970Z"
    :invocation-id "inv-xyz-789"
    :status :success
    :duration-ms 870
    :output {
      :city "Paris"
      :current-temperature 22.5
      :units "Celsius"
      :weather-condition "Partly Cloudy"
      :details "Current weather in Paris is 22.5 Celsius, Partly Cloudy, with 65% humidity."
    }
  }
]
```

#### 14.5.2 JSON Format

This format is useful for interoperability with systems that primarily consume JSON.

```json
[
  {
    "event-type": "task-start",
    "timestamp": "2025-06-08T10:30:01.100Z",
    "task-id": "fetch-city-temperature-mcp",
    "task-version": "1.0.0",
    "invocation-id": "inv-xyz-789",
    "intent": {
      "action": "get-current-weather",
      "location": "Paris",
      "units": "Celsius",
      "context": {
        "original-request": "What's the current temperature in Paris in Celsius?",
        "user-id": "user-7742",
        "session-id": "session-b3f9-4a1c",
        "preferred-language": "en-US"
      }
    },
    "input": {
      "city": "Paris",
      "temperature-units": "Celsius"
    }
  },
  {
    "event-type": "log-message",
    "timestamp": "2025-06-08T10:30:01.105Z",
    "invocation-id": "inv-xyz-789",
    "level": "info",
    "message": "Preparing to fetch weather for city: Paris in Celsius units."
  },
  {
    "event-type": "log-step-start",
    "timestamp": "2025-06-08T10:30:01.110Z",
    "invocation-id": "inv-xyz-789",
    "step-id": "invoke-weather-service-1", 
    "step-type": "agent-call",
    "details": {
      "alias": "weather-service",
      "capability-id": "weather-mcp-agent-profile-v1/get-current-temperature",
      "protocol": "mcp"
    }
  },
  {
    "event-type": "agent-request",
    "timestamp": "2025-06-08T10:30:01.115Z",
    "invocation-id": "inv-xyz-789",
    "step-id": "invoke-weather-service-1",
    "protocol": "mcp",
    "direction": "request",
    "target-agent-id": "weather-mcp-agent-profile-v1",
    "target-tool-id": "get-current-temperature",
    "payload": { 
      "city": "Paris",
      "units": "Celsius"
    }
  },
  {
    "event-type": "agent-response",
    "timestamp": "2025-06-08T10:30:01.950Z",
    "invocation-id": "inv-xyz-789",
    "step-id": "invoke-weather-service-1",
    "protocol": "mcp",
    "direction": "response",
    "source-agent-id": "weather-mcp-agent-profile-v1",
    "source-tool-id": "get-current-temperature",
    "status": "success", 
    "payload": { 
      "city": "Paris",
      "temperature": 22.5,
      "units": "Celsius",
      "condition": "Partly Cloudy",
      "humidity": 65
    }
  },
  {
    "event-type": "log-message",
    "timestamp": "2025-06-08T10:30:01.955Z",
    "invocation-id": "inv-xyz-789",
    "level": "info",
    "message": "Successfully received weather data from MCP agent for Paris"
  },
  {
    "event-type": "log-step-end",
    "timestamp": "2025-06-08T10:30:01.960Z",
    "invocation-id": "inv-xyz-789",
    "step-id": "invoke-weather-service-1",
    "status": "success",
    "duration-ms": 850,
    "result": { 
      "city": "Paris",
      "temperature": 22.5,
      "units": "Celsius",
      "condition": "Partly Cloudy",
      "humidity": 65
    }
  },
  {
    "event-type": "task-end",
    "timestamp": "2025-06-08T10:30:01.970Z",
    "invocation-id": "inv-xyz-789",
    "status": "success",
    "duration-ms": 870,
    "output": {
      "city": "Paris",
      "current-temperature": 22.5,
      "units": "Celsius",
      "weather-condition": "Partly Cloudy",
      "details": "Current weather in Paris is 22.5 Celsius, Partly Cloudy, with 65% humidity."
    }
  }
]
```

This comprehensive example covers the lifecycle from a user's need to a structured task execution involving external MCP communication, and how such an interaction would be logged for observability and debugging.

## 15. Agent Discovery and Dynamic Invocation

This section illustrates the complete workflow of defining an agent, how it might be discovered, and how an RTFS task can dynamically find and invoke its capabilities.

### 15.1 Example Agent Profile: Weather Reporter

This agent profile defines a hypothetical "Weather Reporter Agent" that provides current weather information.

```acl
;; File: weather-reporter-agent-profile.rtfs
(agent-profile :id "weather-reporter-agent-v1"
  :metadata {
    :name "Weather Reporter Agent"
    :version "1.0.0"
    :description "Provides current weather information for a given location."
    :owner "weather-services-inc"
    :discovery-tags [:weather :forecast :location-based "real-time-data"]
  }

  :capabilities [
    { :capability-id "get-current-weather-v1.0"
      :description "Fetches the current weather for a specified city."
      :type :task
      :input-schema [:map
                       [:city :string]
                       [:units [:enum :celsius :fahrenheit]? {:default :celsius}]]
      :output-schema [:map
                        [:city :string]
                        [:temperature :float]
                        [:condition :string]
                        [:humidity :float?]
                        [:wind-speed :float?]
                        [:units :keyword]]
      :annotations { :cost :low :data-freshness "up-to-the-minute" }
    }
  ]

  :communication-endpoints [
    { :endpoint-id "main-jsonrpc"
      :protocol :json-rpc
      :transport :http
      :uri "https://api.weather-reporter.example.com/rpc"
      :details {
        :http-methods [:POST]
        :authentication { :type :api-key :header "X-API-Key" }
      }
      :provides-capabilities ["get-current-weather-v1.0"]
    }
  ]

  :discovery-mechanisms [
    { :type :registry
      :registry-uri "https://rtfs-registry.example.com/agents"
      :registration-id "weather-reporter-agent-v1" ;; ID used for registration
    }
    { :type :well-known-uri ;; For direct discovery if the agent hosts its own profile
      :uri "httpsis://api.weather-reporter.example.com/.well-known/rtfs-agent-profile"
    }
  ]
)
```

This profile includes:
*   Basic metadata and `:discovery-tags` for searchability.
*   A single capability `get-current-weather-v1.0` with defined input/output schemas.
*   A communication endpoint detailing how to reach the agent.
*   Discovery mechanisms, including a pointer to a central registry and a well-known URI for direct discovery. The `:uri` from the `:well-known-uri` mechanism would be used to populate `agent_card.agent_profile_uri` if discovered this way.

### 15.2 Conceptual Agent Card

When the "Weather Reporter Agent" registers with a discovery service (like the one at `https://rtfs-registry.example.com/agents`), or when its `agent-profile` is fetched and processed, an `agent_card` is generated. This card is a summary used in discovery results.

Based on the profile above and the definitions in `agent_discovery.md`, the `agent_card` for `weather-reporter-agent-v1` would look something like this (represented in JSON for clarity, though RTFS structures would be used internally):

```json
{
  "agent_id": "weather-reporter-agent-v1",
  "agent_profile_uri": "httpsis://api.weather-reporter.example.com/.well-known/rtfs-agent-profile", // or URI from registry
  "name": "Weather Reporter Agent",
  "version": "1.0.0",
  "description": "Provides current weather information for a given location.",
  "discovery_tags": ["weather", "forecast", "location-based", "real-time-data"],
  "capabilities_summary": [
    {
      "capability_id": "get-current-weather-v1.0",
      "description": "Fetches the current weather for a specified city.",
      "type": "task"
      // Schemas might be URIs or summarized further depending on registry policy
    }
  ],
  "communication_summary": [
    {
      "endpoint_id": "main-jsonrpc",
      "protocol": "json-rpc",
      "transport": "http",
      "uri": "httpsis://api.weather-reporter.example.com/rpc"
      // Other details like auth might be included or require fetching the full profile
    }
  ],
  "provider_id": "weather-services-inc", // from :owner
  "last_updated": "2025-06-09T10:00:00Z" // Added by the registry
  // Other fields like :rank, :score, :trust_level might be added by the registry
}
```
**Note:** The exact structure and content of the `agent_card` are defined by the `agent_discovery.md` specification and can be influenced by the registry's specific implementation (e.g., how it summarizes capabilities or adds metadata like `last_updated` or `rank`). The `agent_profile_uri` would typically point to where the full `agent-profile.rtfs` can be retrieved.

### 15.3 Task: Discovering and Invoking the Weather Agent

This RTFS task demonstrates how to discover the Weather Reporter Agent and then invoke its capability.

It shows two approaches for requirements:
1.  **Static Requirement (Commented Out):** If the agent and its capability are known beforehand, it can be declared directly in `:contracts :requires`.
2.  **Dynamic Discovery:** The task uses `(discover-agents ...)` to find suitable agents.

```acl
(task :id "task-dynamic-weather-report"
  :intent {
    :action :get-weather-dynamically
    :city "London"
    :preferred-units :celsius
  }
  :contracts {
    :input-schema [:map [:city :string] [:preferred-units :keyword]]
    :output-schema [:map [:report :string] [:error :string?]]
    ;; --- Static Requirement Example (if agent is known) ---
    ;; This shows how one might require the capability if its details were fixed.
    ;; For dynamic discovery, this specific block might be omitted or be more general.
    #_(:requires [ 
      { :capability-id "weather-reporter-agent-v1/get-current-weather-v1.0"
        :alias static-weather-service
        :timeout-ms 3000
      }
    ])
    ;; --- Capabilities needed for the discovery and invocation process itself ---
    :capabilities-required [
      { :type :tool-call :tool-name "tool:log" } 
      ;; Implicit: discover-agents special form, invoke special form
    ]
  }
  :plan
  (do
    (tool:log (str "Attempting to discover a weather agent for city: " (:city @input)))

    ;; 1. Discover agents that can provide weather information
    (let [discovery-results :vector 
          (discover-agents 
            :discovery-tags [:weather "real-time-data"] ;; Search by tags
            :capability-constraints { ;; Further filter by capability properties (conceptual)
              :type :task 
              ;; :input-schema-contains { :city :string } ;; More advanced query
            }
            :limit 5 ;; Get up to 5 results
            ;; :discovery-query { ;; Alternative: Free-form query for advanced registries
            ;;   "metadata.owner": "weather-services-inc"
            ;; }
            )]
      
      (tool:log (str "Discovery returned " (count discovery-results) " agent(s)."))

      (if (empty? discovery-results)
        (do
          (tool:log "No suitable weather agent found.")
          { :report "N/A" :error "Failed to discover a weather service." })
        
        (do
          ;; 2. Select an agent from the results (e.g., the first one)
          ;; In a real scenario, might involve ranking or selection logic
          (let [selected-agent-card :map (first discovery-results)
                agent-id :string (:agent_id selected-agent-card) 
                ;; Assuming capability ID is known or also discoverable from card
                capability-id :string "get-current-weather-v1.0" 
                ;; Endpoint URI could also be extracted if needed for :endpoint-override
                ;; endpoint-uri :string (get-in selected-agent-card [:communication_summary 0 :uri]) 
                ]
            
            (tool:log (str "Selected agent: " agent-id 
                           " (Profile: " (:agent_profile_uri selected-agent-card) ")"))

            ;; 3. Invoke the capability on the discovered agent
            (tool:log (str "Invoking " capability-id " on agent " agent-id 
                           " for city: " (:city @input)))
            
            (let [invoke-params {:city (:city @input) :units (:preferred-units @input)}
                  ;; Use :agent-id-override to target the dynamically discovered agent
                  weather-response (invoke capability-id ;; Base capability ID
                                     invoke-params
                                     { :agent-id-override agent-id 
                                       ;; :endpoint-override endpoint-uri ;; If needed
                                       :timeout-ms 4500 })]

              ;; 4. Process the response
              (match weather-response
                { :city c :temperature temp :condition cond :units u }
                (do
                  (let [report-string (str "Weather in " c ": " temp " " u ", " cond)]
                    (tool:log (str "Successfully retrieved weather: " report-string))
                    { :report report-string }))
                
                [:error err-details]
                (do
                  (tool:log-error (str "Error invoking weather service on " agent-id ": " err-details))
                  { :report "N/A" :error (str "Error from " agent-id ": " (:message err-details "Unknown error")) })
                
                _ ;; Default case for unexpected response structure
                (do
                  (tool:log-error (str "Unexpected response from " agent-id ": " weather-response))
                  { :report "N/A" :error (str "Unexpected response from " agent-id) })
              )
            )
          )
        )
      )
    )
  )
)
```

This example demonstrates:
*   An `agent-profile` for a `weather-reporter-agent-v1`.
*   A conceptual `agent_card` that would be generated from this profile.
*   An RTFS task that:
    *   Includes a commented-out example of a static `:contracts :requires` for comparison.
    *   Uses `(discover-agents ...)` with `:discovery-tags` to find suitable agents.
    *   Selects an agent from the discovery results (here, simply the first one).
    *   Extracts the `:agent_id` from the chosen `agent_card`.
    *   Uses `(invoke ...)` with the `:agent-id-override` option to call the desired capability on the dynamically selected agent.
    *   Processes the response from the `invoke` call.

This flow shows how an RTFS task can adapt to available services at runtime, rather than being hardcoded to a specific agent instance, by leveraging the agent discovery mechanism.
The `:contracts :requires` section can be used to declare a *need* for a certain type of capability (e.g., by specifying `:capability-type` or `:discovery-tags` within a requirement entry, if the spec evolves to support that directly in `:requires`), which the runtime could then attempt to satisfy via discovery if a specific `:capability-id` isn't statically resolvable. For now, the example uses `discover-agents` explicitly in the plan.



