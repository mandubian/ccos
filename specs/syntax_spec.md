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

### Built-in Functions

Standard functions available in the global scope.

*   `+`, `-`, `*`, `/`
*   `=`, `!=`, `>`, `<`, `>=`, `<=`
*   `and`, `or`, `not`
*   `str`, `string-length`, `substring`
*   `get`, `assoc`, `dissoc`, `count`, `conj`
*   `vector`, `map`
*   `int?`, `float?`, `number?`, `string?`, `bool?`, `nil?`, `map?`, `vector?`, `keyword?`, `symbol?`, `fn?`

### Tool Interfaces (Examples)

Namespaced identifiers for standard runtime-provided tools.

*   `tool:log`, `tool:log-error`
*   `tool:open-file`, `tool:read-line`, `tool:write-line`, `tool:close-file`
*   `tool:print`, `tool:read-input`
*   `tool:get-env`, `tool:current-time`
*   `tool:http-fetch`
*   `tool:parse-json`, `tool:serialize-json`
*   `tool:load-remote-tensor`, `tool:tensor-elementwise-add`, etc.

## 3. Next Steps

*   **Refine Formal Grammar:** Enhance `grammar_spec.md` with more detail and stricter EBNF rules, covering edge cases and precise identifier/literal definitions.
*   **Formalize Schemas & Constraints:** Provide rigorous definitions for the predicate language used in `[:and]` types (`type_system.md`) and the constraint language for capabilities (`security_model.md`).
*   **Detail Type Inference:** Elaborate on type inference rules and interactions between typed and untyped code in `type_system.md`. Specify rules for generics and polymorphism if needed.
*   **Consistency Review:** Perform a thorough review across all specification documents (`core_concepts.md`, `syntax_spec.md`, `grammar_spec.md`, `type_system.md`, `language_semantics.md`, `ir_spec.md`, `resource_management.md`, `security_model.md`, `examples.md`, `stdlib_spec.md`) to ensure alignment and resolve ambiguities.
*   **Refine Standard Library/Tools:** Expand and detail the functions and tool interfaces defined in `stdlib_spec.md`, including precise error conditions and edge cases.
*   **Implementation Planning:** Begin designing the architecture for the RTFS toolchain (parser, IR generator, type checker, runtime/interpreter).
*   **Develop Examples:** Create more complex and diverse examples demonstrating various language features and use cases. See [`examples.md`](./examples.md) for detailed examples.
