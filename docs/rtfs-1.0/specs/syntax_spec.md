# RTFS - Standalone Language Syntax Specification (Draft)

This document outlines a proposed custom syntax for the Reasoning Task Flow Specification (RTFS) language, designed to be independent of any specific host language like Clojure.

## 1. Design Principles

*   **Clarity & Readability:** Syntax should be understandable by both humans and AI agents. Keywords should be explicit. Use of S-expressions aids structural clarity.
*   **Expressiveness:** Must naturally represent all core RTFS concepts: tasks, intents, plans, sequential/parallel execution, tool calls, logging, contracts, security primitives, resource management, error handling.
*   **Unambiguity & Parsability:** The grammar should be well-defined (formally specified later, e.g., EBNF) and reasonably easy to parse reliably. S-expression base helps.
*   **Extensibility:** Allow for future additions via namespaces and potentially a macro system (defined later).
*   **Data-Centric & Homoiconic:** RTFS code *is* data (S-expressions), making it easy for AI to generate, analyze, and manipulate plans. The `Task` artifact remains the central data structure.
*   **Strong Guarantees:** Incorporate features for static analysis and runtime safety (types, resource management, contracts).

## 2. Proposed Syntax Sketch

*(Note: This is a preliminary sketch using S-expression-like structure with specific keywords and type annotations. Details like schema definition syntax within contracts need further refinement.)*

### 2.1. Top-Level Task Definition

```acl
;; Define a task artifact using the 'task' keyword
(task :id "task-uuid-1234" ;; String literal ID
  :source "human-chat:v2" ;; Namespaced keyword for source type
  :timestamp "2025-04-28T10:00:00Z" ;; ISO 8601 string

  ;; Semantic goal derived from source
  :intent { ;; Standard map literal for intent structure
    :action :summarize-document
    :document-url "http://example.com/report.pdf"
    :max-length 500 ;; Integer literal
  }

  ;; Explicit contracts (populated by planner, validated by runtime)
  :contracts { ;; Map containing contract definitions
    ;; Input schema definition using detailed type syntax (keyword keys)
    :input-schema [:map
                     [:document-content :string]
                     [:user-prefs [:map [:language :string?]]]] ;; Optional language
    ;; Output schema definition (keyword keys)
    :output-schema [:map
                      [:summary [:and :string [:min-length 10]]] ;; Refined string type
                      [:word-count [:and :int [:>= 0]]] ;; Non-negative integer
                      [:language :string]]
    ;; List of required external capabilities using structured format
    :capabilities-required [
      { :type :tool-call :tool-name "tool:fetch-url:v1" }
      { :type :tool-call :tool-name "tool:extract-text" }
      { :type :tool-call :tool-name "tool:summarize" }
      { :type :resource-access :resource-type "FileHandle" :permissions [:write] }
    ]
  }

  ;; The executable plan
  :plan (do ;; Sequential execution block
          ;; Access task context using '@' prefix
          (let [url :string (:document-url @intent) ;; Accessing @intent
                max-len :int (:max-length @intent)]
            ;; Resource management block: ensures file is closed
            ;; Explicit ResourceType annotation (FileHandle)
            (with-resource [output-file FileHandle (tool:open-file "summary.txt" :mode :write)]
              ;; Tool call with named arguments
              (let [doc-content :string (tool:fetch-url url)
                    text :string (tool:extract-text doc-content)
                    summary-result :map (tool:summarize text :max-length max-len)]
                 ;; Write to the resource handle
                 (tool:write-line output-file (:summary summary-result))
                 ;; Return a map matching the output contract
                 { :summary (:summary summary-result)
                   :word-count (:word-count summary-result)
                   :language "en" } ;; Example literal value
                 ;; 'output-file' handle is automatically consumed/closed here
                 ))))

  ;; Execution Trace (Log) - Append-only list of signed events
  :execution-trace [ ;; List literal for trace
    (log-entry ;; Specific keyword for log entries
      :timestamp "2025-04-28T09:59:50Z"
      :agent "parser-agent:v1.2"
      :event :task-created
      :details {:source "human-chat:v2"} ;; Details specific to the event
      :previous-entry-hash nil ;; Hash of previous entry (nil for first)
      ;; Detailed signature block
      :signature {
        :key-id "parser-key-01"
        :algo :ed25519
        :value "base64..."
      }
    )
    (log-entry
      :timestamp "2025-04-28T10:05:00Z"
      :agent "planner-agent:v3.0"
      :event :plan-generated
      :details { ;; Include hash/reference to generated plan/contracts
        :plan-hash "sha256-abc..."
        :contracts-hash "sha256-def..."
      }
      :previous-entry-hash "sha256-of-first-entry..." ;; Link to previous entry
      :signature {
        :key-id "planner-key-05"
        :algo :ed25519
        :value "base64..."
      }
    )
    (log-entry ;; Example of logging a step execution
      :timestamp "2025-04-28T10:06:15Z"
      :agent "executor-runtime:v0.9"
      :event :step-executed
      :step-id "fetch-url-step" ;; Corresponds to (log-step :id ...)
      :result {:status :success :value "<content bytes hash>"} ;; Or {:status :error ...}
      :previous-entry-hash "sha256-of-plan-entry..."
      :signature {
        :key-id "executor-key-01"
        :algo :ed25519
        :value "base64..."
      }
    )
    ;; ... more entries ...
  ]
)
```

### 2.2. Plan Language Constructs

```acl
;; --- Basic Values & Variables ---

;; Literals
42          ;; int
3.14        ;; float
"hello"     ;; string
true        ;; bool
false       ;; bool
nil         ;; nil type
:my-keyword ;; keyword
[1 2 "a"]   ;; vector literal (list in some Lisps, vector in Clojure/RTFS)
{:a 1 :b 2} ;; map literal

;; Variable Definition (within lexical scope)
;; Type annotations are optional (gradual typing) but encouraged
(def my-var :int 10)
(def inferred-var "hello") ;; Type inferred if possible by analyzer

;; Variable Lookup
my-var

;; Let Binding (lexical scope)
(let [x :int 1 ;; Optional type annotation
      y (+ x 2) ;; Type potentially inferred
      z :string "result"]
  (str z ": " y)) ;; Returns "result: 3"

### Array Types with Shapes

RTFS supports typed arrays with specified element types and shapes. This is crucial for handling structured data, especially in tool calls involving tensors or collections.

**Syntax:**

`[:array element-type dimensions]`

*   `element-type`: An RTFS type keyword (e.g., `:int`, `:float`, `:string`, `:map`) or a nested type definition.
*   `dimensions`: A vector defining the shape of the array.
    *   Integer literals for fixed dimensions (e.g., `[3]`, `[10 20]`).
    *   `?` for an unknown or variable dimension size.
    *   `*` (or `?*` or just `?`) can represent a variable number of elements in a 1D array or the outermost dimension.

**Examples:**

```acl
;; A 1D array (vector) of 5 integers
(def my-vector [:array :int [5]])

;; A 2D array (matrix) of floats, 3 rows, 4 columns
(def my-matrix [:array :float [3 4]])

;; A 1D array of strings, unknown length
(def string-list [:array :string [?]]) ;; or [*]

;; A 3D array, first two dimensions unknown, third is 3 (e.g., a batch of RGB images)
(def image-batch [:array :ubyte [? ? 3]])

;; An array of maps
(def user-list [:array [:map [:id :int] [:name :string]] [?]])
```

Array shapes are used for static analysis where possible and runtime validation. Tools that produce or consume arrays are expected to declare their array type signatures in their contracts.

### Type Refinements and Predicates

RTFS allows base types to be refined with predicates to create more specific and constrained types. This is particularly useful in defining schemas for task inputs, outputs, and capability contracts, enhancing data validation and system robustness.

**Syntax:**

The primary way to define a refined type is using the `[:and ...]` form:

`[:and base-type predicate1 predicate2 ...]`

*   `base-type`: The underlying RTFS type (e.g., `:int`, `:string`, `:map`).
*   `predicateN`: An expression or keyword representing a constraint on the base type.

Common Predicates:

*   For `:int` / `:float`:
    *   `[:> value]`, `[:>= value]`, `[:< value]`, `[:<= value]`, `[:= value]`, `[:!= value]`
    *   `[:in-range min max]` (inclusive or exclusive depending on definition)
*   For `:string`:
    *   `[:min-length len]`, `[:max-length len]`, `[:length len]`
    *   `[:matches-regex "pattern"]`
    *   `[:is-url]`, `[:is-email]` (conceptual, could be stdlib predicates)
*   For `:array` / `:vector`:
    *   `[:min-count count]`, `[:max-count count]`, `[:count count]`
    *   `[:non-empty]` (equivalent to `[:min-count 1]`)
*   For `:map`:
    *   `[:has-key :key-name]`
    *   `[:required-keys [:key1 :key2]]`

**Examples:**

```acl
;; An integer greater than 0
(def positive-int [:and :int [:> 0]])

;; A string that must be a valid email format and have a max length
(def email-string [:and :string [:matches-regex "^.+@.+\\\\..+$"] [:max-length 255]])

;; A non-empty array of positive integers
(def non-empty-positive-int-array [:array [:and :int [:> 0]] [:non-empty]]) ;; or [:min-count 1]

;; A map requiring specific keys
(def user-record-spec
  [:map
   [:id [:and :int [:> 0]]]
   [:username [:and :string [:min-length 3]]]
   [:status [:enum :active :inactive :pending]] ;; Enum as a form of refinement
   [:optional-prefs :map?]]) ;; Optional key with a map type or nil
```

**Other Type Constructs related to Refinement:**

*   **`[:enum value1 value2 ...]`**: Defines a type whose value must be one of the specified literal values (often keywords or strings).
    ```acl
    (def traffic-light-color [:enum :red :yellow :green])
    ```

*   **`[:one-of type1 type2 ...]`** (Union Type): Defines a type whose value can be of any of the specified types.
    ```acl
    (def id-type [:one-of :int :string]) ;; ID can be an integer or a string
    ```

These refined types are used extensively in `:input-schema` and `:output-schema` definitions within task contracts and agent capability descriptions.

;; --- Control Flow ---

;; Conditional (else branch is mandatory for type stability)
(if (> x 0)
  "positive" ;; then branch expression
  "non-positive") ;; else branch expression

;; Sequential Execution
(do
  (tool:log "Starting")
  (def temp (tool:step1 "input"))
  (tool:step2 temp)) ;; Result of 'do' is result of last expression
```

### Function Definition (`fn`, `defn`)

Defines anonymous (`fn`) or named (`defn`) functions.

```acl
;; Anonymous function adding two numbers
(fn [a :int b :int] :int (+ a b))

;; Named function with type hints and docstring (conceptual)
(defn greet
  "Returns a greeting string."
  [name :string] :string
  (str "Hello, " name "!"))

;; Function with variadic arguments
(defn sum-all [first :number & rest :number] :number
  (reduce + first rest)) ;; Assuming 'reduce' exists

;; Function accepting a map for named/optional parameters using destructuring
(defn configure-widget
  "Configures a widget based on options map."
  [{:keys [width height] ;; Required keys
    :or {color :blue debug false} ;; Optional keys with defaults
    :as options}] ;; Bind the whole map to 'options'
  :map ;; Return type (e.g., the configuration applied)

  (do
    (tool:log (str "Configuring widget. Width=" width ", Height=" height ", Color=" color))
    (when debug (tool:log (str "Full options: " options)))
    ;; ... actual configuration logic ...
    options)) ;; Return the effective options

;; Calling the function with destructuring
(configure-widget {:width 100 :height 50}) ;; Uses default color :blue
(configure-widget {:width 150 :height 75 :color :red :debug true})


;; --- Functions ---

;; Function Definition (Lambda)
;; Type annotations for params and return value are optional but recommended
(fn [x :int y :string] :bool ;; -> returns bool
  (and (> x 0) (!= y "")))

;; Named Function Definition (Syntactic sugar for def + fn)
(defn process [data :map] :map
  (assoc data :processed true))

;; Function/Tool Application
(process {:id 1})
(tool:perform-action "data" :mode :safe :retries 3)
((fn [x] (* x x)) 10) ;; Applying an inline lambda

;; --- Concurrency ---

;; Parallel Execution
;; Executes expressions concurrently. Returns a map of results keyed by binding ID (e.g., :res-a).
;; Blocks until all parallel tasks complete. If any task errors,
;; the first error encountered is propagated immediately.
(parallel
  [res-a :string (tool:fetch "source-a")] ;; Optional type annotation for result
  [res-b :int (tool:long-computation 42)])
;; Returns map like { :res-a "content-a", :res-b 1764 } on success

;; Join (Alternative/Legacy? 'parallel' block implies join)
;; If needed: (join task-a task-b) - Semantics TBD if separate from 'parallel'

;; --- Resource Management (Simulating Linear Types) ---

;; Acquire and use resource within a scope, ensuring release.
;; 'handle' is bound within the block. 'tool:open-file' returns a resource handle.
;; 'FileHandle' is the explicit resource type annotation.
;; 'tool:close-file' is the implicitly called cleanup function associated with the handle type.
(with-resource [handle FileHandle (tool:open-file "data.txt")]
  (tool:read-line handle)) ;; Use the handle

;; Explicit consumption (if needed for specific protocols)
;; 'consume' keyword signifies the value cannot be used afterwards in this scope.
;; (let [final-data (consume (process-resource handle))]) ;; Semantics TBD - REMOVED during consistency review

;; --- Error Handling (Structured Errors) ---

;; Example: Using a Result type convention with 'match'
;; Assume tool calls return [:ok value] or [:error error-map]
(let [result (tool:risky-operation)]
  (match result ;; Pattern matching on the result structure
    [:ok data] (do ;; Match success case
                 (tool:log "Success!")
                 data) ;; Extract data
    [:error {:type err-type :message msg :as err-info}] ;; Match error case, destructure map
           (do ;; Handle error
             (tool:log-error (str "Error type: " err-type ", Msg: " msg))
             nil) ;; Return nil on error
    _ (do ;; Default catch-all pattern (optional)
          (tool:log "Unknown result structure")
          nil)))

;; Alternative: Try/Catch block
(try
  (tool:might-fail)
  ;; Catch specific error type using keyword from standard error map
  (catch :error/network err
    (tool:log "Network failed:" (:message err))
    (fallback-value))
  ;; Catch based on a more general type (if type system supports it)
  ;; (catch ResourceError err ...)
  ;; Catch any other error (use :any or a variable)
  (catch other-err
    (tool:log "Unknown error:" (:message other-err))
    (default-value)))

;; --- Logging & Security ---

;; Log Step Execution (associates log entry with expression evaluation)
(log-step :id "unique-step-identifier"
  (tool:complex-calculation x y))

;; Signatures are part of the :execution-trace structure (see Task definition)

;; --- Namespacing & Modules ---

;; Tool calls use namespaces
(tool:namespace/action ...)
(resource:type/operation ...)

;; Module definition/import (Conceptual Syntax)
;; Define a module, optionally exporting specific symbols
(module my.utils
  :exports [helper-fn] ;; Only export helper-fn
  (def private-var 10)
  (defn helper-fn [x] (* x private-var)))

;; Import symbols from another module
(import my.utils :as u) ;; Import all exported symbols under alias 'u'
(import other.module :only [specific-func]) ;; Import only specific-func
;; (import another.module :refer :all) ;; Import all exported symbols directly (use with caution)

;; Usage after import
(u/helper-fn 5)
(specific-func)

;; --- Tool Calls involving External Data (e.g., Tensors) ---

;; Load a tensor (returns a handle, or could return the array directly if small enough)
;; Option 1: Using Resource Handle
(def my-tensor-handle [:resource TensorHandle] (tool:load-tensor "data.npy"))

;; Option 2: Using First-Class Array Type (if supported by tool/runtime)
(def my-tensor [:array :float [? ? 3]] (tool:load-tensor-direct "image.png")) ;; Shape might be partially known

;; Perform an operation using a tool (takes/returns handles or arrays)
;; Using Handles:
(def result-handle [:resource TensorHandle] (tool:tensor-add my-tensor-handle 5.0))

;; Using Arrays (Tool signature would need to specify expected shapes):
;; (def result-tensor [:array :float [? ? 3]] (tool:tensor-add my-tensor 5.0))

;; Run inference (takes model/tensor handles or arrays, returns handle or array)
(with-resource [model [:resource ModelHandle] (tool:load-model "model.onnx")]
  ;; Using handle for input, array for output
  (let [output [:array :float [? 10]] (tool:run-inference model my-tensor-handle)]
    (tool:log (str "Inference output shape: " (tool:array-shape output)))))

;; --- Tool Calls involving Non-Local Resources ---

;; Stateless API Call
(def api-result (tool:call-web-api "http://example.com/status" :method :get))

;; Database Interaction with Handle
(with-resource [db-conn DatabaseConnectionHandle (tool:connect-db "connection-string")]
  (let [users (tool:query db-conn "SELECT id FROM users WHERE active = true")]
    (process-user-ids users)))
```

### Special Forms

These have unique evaluation rules and syntax defined directly by the language semantics.

*   `if`
*   `let`
*   `do`
*   `fn`
*   `def`
*   `defn`
*   `match`
*   `try`, `catch`, `finally`
*   `parallel`
*   `with-resource`
*   `log-step`
*   `module`
*   `import`
*   `task` (Top-level definition form)
*   `agent-profile` (Top-level definition form)
*   `invoke` (Agent capability invocation)
*   `consume-stream` (Agent stream consumption)
*   `produce-to-stream` (Agent stream production)
*   `discover-agents` (New special form for agent discovery)

### Keywords

Used as identifiers (often in maps or for specific options).

*   `:id`
*   `:metadata`
*   `:intent`
*   `:contracts`
*   `:input-schema`
*   `:output-schema`
*   `:capabilities-required`
*   `:plan`
*   `:execution-trace`
*   `:timestamp`
*   `:agent`
*   `:event`
*   `:details`
*   `:previous-entry-hash`
*   `:signature`
*   `:key-id`
*   `:algo`
*   `:value`
*   `:type`
*   `:tool-call`
*   `:tool-name`
*   `:constraints`
*   `:args`
*   `:resource-access`
*   `:resource-type`
*   `:permissions`
*   `:network-access`
*   `:host`
*   `:port`
*   `:protocols`
*   `:ok`
*   `:error`
*   `:message`
*   `:exports`
*   `:as`
*   `:only`
*   *(Standard error types like `:error/runtime`, `:error/network`, etc.)*
*   *(Modes like `:read`, `:write`, `:append`)*
*   *(Enum values defined in schemas, e.g., `:success`, `:failure`)*

### Task Context Access

Special syntax for accessing fields from the containing task artifact within the `:plan`:

*   `@id` - Access the task's `:id` field
*   `@intent` - Access the task's `:intent` field  
*   `@metadata` - Access the task's `:metadata` field
*   `@contracts` - Access the task's `:contracts` field
*   `@input` - Access the task's input data (assuming it has been populated according to the task's `:input-schema` by the runtime or a preceding step). The exact mechanism of how `@input` is populated needs to be defined in `language_semantics.md`.

Example usage:
```acl
;; Within a task's :plan
(let [url (:document-url @intent)     ;; Access intent field
      task-id @id                    ;; Access task ID
      input-data @input]             ;; Access the whole input data map
  (tool:log (str "Processing " url " for task " task-id " with input: " input-data)))
```

## 3. Next Steps

*   **Refine Formal Grammar:** Enhance `grammar_spec.md` with more detail and stricter EBNF rules, covering edge cases and precise identifier/literal definitions.
*   **Formalize Schemas & Constraints:** Provide rigorous definitions for the predicate language used in `[:and]` types (`type_system.md`) and the constraint language for capabilities (`security_model.md`).
*   **Detail Type Inference:** Elaborate on type inference rules and interactions between typed and untyped code in `type_system.md`. Specify rules for generics and polymorphism if needed.
*   **Consistency Review:** Perform a thorough review across all specification documents (`core_concepts.md`, `syntax_spec.md`, `grammar_spec.md`, `type_system.md`, `language_semantics.md`, `ir_spec.md`, `resource_management.md`, `security_model.md`, `examples.md`, `stdlib_spec.md`) to ensure alignment and resolve ambiguities.
*   **Refine Standard Library/Tools:** Expand and detail the functions and tool interfaces defined in `stdlib_spec.md`, including precise error conditions and edge cases.
*   **Implementation Planning:** Begin designing the architecture for the RTFS toolchain (parser, IR generator, type checker, runtime/interpreter).
*   **Develop Examples:** Create more complex and diverse examples demonstrating various language features and use cases. See [`examples.md`](./examples.md) for detailed examples.

## 6. Agent Profile Syntax

An `agent-profile` is a top-level RTFS artifact that describes an AI agent, its capabilities, and how to interact with it. It is represented as a map with specific keyword-based keys.

```lisp
(agent-profile :id "polyglot-agent-v1" ;; Matches example in examples.md
  :metadata {
    :name "Polyglot Translation Agent"
    :version "1.2.0"
    :description "An agent specialized in translating text between multiple languages."
    :owner "owner-identifier-or-contact"
    :tags [:translator :nlp :multilingual] ;; Optional tags for categorization
    :created-at "2025-06-07T10:00:00Z"
    :updated-at "2025-06-07T10:00:00Z"
    ;; ... other relevant metadata ...
  }

  :capabilities [
    { :capability-id "summarize-text-v1"
      :description "Summarizes a given text document."
      :type :task ;; Could also be :tool, :service, :stream-source, :stream-sink
      ;; For :task type, this could point to a full RTFS task definition or schema
      :input-schema [:map [:text string] [:max-length int?]]
      :output-schema [:map [:summary string]]
      :annotations { :idempotent true :cost :low }
    }
    { :capability-id "translate-text-v2"
      :description "Translates text between languages."
      :type :task
      :input-schema [:map [:text string] [:source-lang keyword] [:target-lang keyword]]
      :output-schema [:map [:translated-text string]]
      :annotations { :cost :medium }
    }
    { :capability-id "live-data-feed-v1"
      :description "Provides a continuous stream of live market data."
      :type :stream-source ;; Indicates this capability produces a stream
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
    ;; ... more capabilities ...
  ]

  :communication-endpoints [
    { :endpoint-id "jsonrpc-http"
      :protocol :json-rpc
      :transport :http
      :uri "https://agent.example.com/api/v1/jsonrpc"
      :details { ;; Protocol-specific details
        :http-methods [:POST]
        :authentication { :type :api-key :header "X-Auth-Token" }
      }
      :provides-capabilities ["summarize-text-v1" "translate-text-v2"] ;; List of capability-ids served by this endpoint
    }
    { :endpoint-id "websocket-stream"
      :protocol :websocket
      :transport :ws
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
    ;; ... more endpoints ...
  ]

  :discovery-mechanisms [
    { :type :well-known-uri
      :uri "/.well-known/rtfs-agent-profile.json" ;; Relative to the agent's base URI
      :format :json ;; The format this profile is available in at the URI (could be :rtfs)
    }
    { :type :registry
      :registry-uri "https://registry.example.com/agents"
      :registration-id "unique-agent-identifier-string"
    }
    ;; ... other discovery methods ...
  ]

  ;; Optional section for declaring compatibility with other protocols like A2A or MCP
  :interoperability {
    :a2a-compatibility { ;; Agent2Agent Protocol
      :agent-card-uri "https://agent.example.com/a2a/agent-card.json"
      :task-endpoint "https://agent.example.com/a2a/tasks"
    }
    :mcp-compatibility { ;; Model Context Protocol
      :server-capabilities-uri "https://agent.example.com/mcp/capabilities"
      ;; Further details on how MCP tools map to RTFS capabilities
      :tool-mappings [
        { :mcp-tool-name "mcp/summarize"
          :rtfs-capability-id "summarize-text-v1"
        }
      ]
    }
  }
)
```

### Key Sections of `agent-profile`:

*   **`:id`** (`string`): A globally unique identifier for the agent instance.
*   **`:metadata`** (`:map`): General information about the agent profile itself (name, version, description, ownership, timestamps, tags).
*   **`:capabilities`** (`[:vector :map]`): A list of capabilities the agent offers. Each capability map includes:
    *   `:capability-id` (`string`): A unique ID for the capability within this agent.
    *   `:description` (`string`): Human-readable description.
    *   `:type` (`keyword`): The nature of the capability (e.g., `:task`, `:tool`, `:service`, `:stream-source`, `:stream-sink`).
    *   `:input-schema` (RTFS Type Schema): Defines the input structure for invoking this capability or, for `:stream-sink`, the type of elements it consumes.
    *   `:output-schema` (RTFS Type Schema): Defines the output structure or, for `:stream-source`, the type of individual elements in the stream.
    *   `:annotations` (`:map?`): Optional metadata about the capability (e.g., cost, idempotency, rate limits).
*   **`:communication-endpoints`** (`[:vector :map]`): Describes how to access the agent's capabilities. Each endpoint map includes:
    *   `:endpoint-id` (`string`): A unique ID for this endpoint.
    *   `:protocol` (`keyword`): The communication protocol (e.g., `:json-rpc`, `:websocket`, `:http-rest`, `:rtfs-native`, `:grpc`).
    *   `:transport` (`keyword`): The transport layer (e.g., `:http`, `:https`, `:ws`, `:wss`, `:stdio`).
    *   `:uri` (`string`): The access URI for this endpoint.
    *   `:details` (`:map?`): Protocol and transport-specific configuration (e.g., HTTP methods, authentication details, message formats, `stream-options` for streaming protocols).
    *   `:provides-capabilities` (`[:vector string]`): A list of `capability-id`s exposed through this endpoint.
*   **`:discovery-mechanisms`** (`[:vector :map]`): How this agent profile can be discovered by others. Each mechanism map includes:
    *   `:type` (`keyword`): The discovery method (e.g., `:well-known-uri`, `:registry`, `:dns-srv`).
    *   Other fields specific to the discovery type (e.g., `:uri`, `:format` for well-known; `:registry-uri`, `:registration-id` for registry). An entry of type `:well-known-uri` is typically used by the agent to populate the `agent_profile_uri` field in its `agent_card`, resolving any relative URI to an absolute one. For more details on agent discovery mechanisms and the `agent_card` structure, refer to [`agent_discovery.md`](./agent_discovery.md).
*   **`:interoperability`** (`:map?`): Optional section detailing compatibility with specific external protocols like A2A or MCP.
    *   Keys are protocol names (e.g., `:a2a-compatibility`, `:mcp-compatibility`).
    *   Values are maps containing protocol-specific information (e.g., endpoint URIs, capability mappings).

## 7. Task Interaction with Agent Profile (Syntax Summary)

This section summarizes the syntax related to how a task declares dependencies and invokes capabilities from an agent profile. Refer to `examples.md` for complete, runnable examples.

### 7.1. Declaring Required Capabilities in Task Contracts

Within a `task` definition, the `:contracts` map can include a `:requires` key. Each entry in the `:requires` vector is a map describing a needed capability. This map includes fields for identifying the capability, aliasing it for use in the plan, providing static discovery hints (like a direct profile URI or agent ID), specifying dynamic discovery parameters (like tags or custom queries for an agent registry), and setting default invocation behaviors.

The map for each required capability can contain fields such as:
```acl
(task :id "my-consumer-task"
  :contracts {
    :requires [
      { ;; Capability Identification and Aliasing
        :capability-id "namespace/capability-name" ; Required: ID, name, or pattern for the capability.
        :alias local-alias-for-plan ; Required: Symbol used in the plan to refer to this requirement.
        :version-constraint ">=1.0 <2.0"? ; Optional: Semantic version constraint for the capability.

        ;; Static Discovery Hints & Direct Addressing
        ;; If :agent-profile-uri is provided and valid, it may be used directly, bypassing broader discovery.
        :agent-profile-uri "uri/to/specific/profile.rtfs"? ; Optional: Direct URI to a specific agent's profile.
        :agent-id "specific-agent-uuid"? ; Optional: Constrain discovery to a specific agent ID or pattern if broader discovery is used.

        ;; Dynamic Discovery Parameters (used if broader discovery via a registry is invoked, e.g., by discover-agents form or by the runtime resolving an alias)
        :discovery-tags [:vector :keyword]? ; Optional: Tags to filter agents by (e.g., [:translator :nlp]). These keywords are typically matched against string representations of tags in `agent_card` structures (see [`agent_discovery.md`](./agent_discovery.md)) or registry queries.
        :discovery-query :map? ; Optional: Arbitrary key-value parameters for advanced registry queries. These parameters are passed to the discovery mechanism (e.g., an agent registry) and can be used to match against various fields in an `agent_card` (such as metadata, communication protocols, or custom properties defined in the `agent_card`'s metadata; see [`agent_discovery.md`](./agent_discovery.md)) or to perform free-text searches. (e.g., {:protocol_preference :json-rpc, :region \"us-east-1\", :text_search \"image processing\", \"custom.agent_card.property\": \"value\"}).

        ;; Invocation Behavior Overrides (defaults for invocations using the alias)
        :optional false? ; Optional, default false. If true, task can proceed if capability not found/failed.
        :timeout-ms 5000? ; Optional: Default timeout in milliseconds for invoking this capability.
        :retry-policy { :max-attempts 3 :delay-ms 1000 }? ; Optional: Default retry policy for this capability.
      }
      ;; ... more required capabilities ...
    ]
    ;; ... other contract fields like :input-schema, :output-schema ...
  }
  :plan (...)
)
```

### 7.2. Invoking a Standard Capability in a Plan

Use the `invoke` special form:

```acl
(invoke local-alias-for-plan ;; Alias from :requires
  { ; Map of arguments matching the capability's :input-schema
    :arg1 value1
    :arg2 value2
  }
  [ ; Optional map of invocation-specific options (overrides defaults)
    { :timeout-ms 10000 }
  ]
)
```

### 7.3. Consuming a Stream Capability in a Plan

Use the `consume-stream` special form:

```acl
(consume-stream local-alias-for-streaming-capability ;; Alias from :requires
  { ; Map of parameters to initiate the stream, matching capability's :input-schema
    :param1 value1
  }
  { item-binding => ;; Binding for each item from the stream
    ;; Body expressions to process the item-binding
    (tool:process-stream-item item-binding)
  }
  [ ; Optional map of stream-level options
    { :on-error (fn [err] (tool:log-error err))
      :on-complete (fn [] (tool:log "Stream done."))
      :timeout-ms 60000
    }
  ]
)
```

### 7.4. Producing to a Stream Capability in a Plan

Use the `produce-to-stream` special form:

```acl
(produce-to-stream local-alias-for-stream-sink-capability ;; Alias from :requires
  item-to-send ;; Expression evaluating to the item, matching capability's :input-schema
  [ ; Optional map of produce-specific options
    { :ack-timeout-ms 2000 }
  ]
)
```

### 7.5. Discovering Agents in a Plan (New Section)

To dynamically find agents based on criteria, the `discover-agents` special form can be used within a task's plan. This form interacts with an agent discovery mechanism (e.g., a registry) and returns information about matching agents.

**Syntax:**

```acl
(discover-agents
  ;; Discovery Criteria Map (required)
  { ;; At least one of these criteria must be provided.
    ;; These fields mirror those in the :requires contract but are used for explicit discovery calls.
    :capability-id "namespace/capability-name"? ; Optional: ID, name, or pattern for a capability the agent must offer.
    :version-constraint ">=1.0 <2.0"? ; Optional: Version constraint for the :capability-id.
    :agent-id "specific-agent-uuid"? ; Optional: Discover a specific agent by its ID.
    :discovery-tags [:vector :keyword]? ; Optional: Tags to filter agents by (e.g., [:translator :nlp]). These keywords are typically matched against string representations of tags in agent_card structures or registry queries.
    :discovery-query :map? ; Optional: Arbitrary key-value parameters for advanced registry queries. These parameters are passed to the discovery mechanism (e.g., an agent registry) and can be used to match against various fields in an agent_card or to perform free-text searches. (e.g., {:name_contains "translator", :text_search "document analysis", :min_rating 4.0, "service_level": "premium"}).
    :limit :int? ; Optional: Maximum number of agent profiles to return.
  }
  ;; Options Map (optional)
  [
    { :registry-uri "uri/to/specific/registry"? ; Optional: URI of the agent registry to query. Defaults to runtime-configured registry.
      :timeout-ms 10000? ; Optional: Timeout for the discovery operation.
      :cache-policy :keyword? ; Optional: e.g., :use-cache, :no-cache, :refresh-cache
    }
  ]
)
```

**Key Aspects:**

*   **Criteria Map:** This map specifies what kind of agent(s) to search for. It can include:
    *   `:capability-id`: Find agents offering a specific capability (optionally with `:version-constraint`).
    *   `:agent-id`: Find a specific agent by its unique ID.
    *   `:discovery-tags`: A vector of keywords used to match against tags in agent profiles.
    *   `:discovery-query`: A map for more complex or custom query parameters supported by the registry.
    *   `:limit`: An optional integer to limit the number of results.
*   **Options Map:** This optional map can control aspects of the discovery process:
    *   `:registry-uri`: Specify a particular agent registry. If omitted, a default registry configured in the runtime environment is used.
    *   `:timeout-ms`: Timeout for the discovery request.
    *   `:cache-policy`: Hint for how to use cached discovery results.
*   **Return Value:** The `discover-agents` form returns a vector of `agent_card` structures (as defined in [`agent_discovery.md`](./agent_discovery.md)). These cards provide comprehensive information about discovered agents, including their ID, capabilities, and communication endpoints. If no agents are found, it returns an empty vector. On error (e.g., registry unavailable), it returns an error map. The detailed semantics, including how returned `agent_card`s can be used with forms like `invoke`, are specified in `language_semantics.md`.

**Usage Example:**

```acl
(let [discovered-translators (discover-agents
                               {:capability-id "vendor/translate"
                                :discovery-tags [:nlp :english :french]
                                :limit 5}
                               [{:timeout-ms 5000}])]
  (if (empty? discovered-translators)
    (tool:log "No suitable translation agents found.")
    (let [best-translator (first discovered-translators) ;; Simplified selection
          ;; Assume 'best-translator' is a map containing enough info to be used as a capability-target
          ;; or that we can construct a temporary :requires-like structure for invoke.
          ;; For now, let's assume it returns a structure that 'invoke' can understand, perhaps by returning
          ;; a map that includes a resolvable :agent-id and the specific :capability-id found.
          ]
      (tool:log (str "Found translator: " (:id best-translator)))
      ;; Further logic to invoke the discovered agent would go here.
      ;; This might involve dynamically constructing the capability-target for 'invoke'
      ;; or the runtime providing a way to invoke directly using the discovered agent handle.
      ))
)
```

This new special form provides a direct way for tasks to perform dynamic discovery, complementing the more static declarations in the `:requires` contract.
