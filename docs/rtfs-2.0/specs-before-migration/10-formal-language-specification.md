# RTFS 2.0 Formal Language Specification

**Status:** Stable  
**Version:** 2.0.0  
**Date:** July 2025  
**Implementation:** Complete

## 1. Introduction

This document provides the complete formal specification for the RTFS 2.0 language, including syntax, semantics, and standard library. RTFS 2.0 is a functional programming language designed specifically for AI task execution within the CCOS (Cognitive Computing Operating System) framework.

## 2. Language Overview

RTFS 2.0 is a Lisp-like functional language with the following characteristics:

- **Functional**: All expressions are pure functions with no side effects
- **Capability-Centric**: Execution is based on discoverable capabilities
- **Type-Safe**: Comprehensive type system with compile-time validation
- **Security-First**: Built-in security features and attestation
- **Streaming**: Native support for streaming data processing
- **CCOS-Integrated**: Designed to work seamlessly with CCOS components

## 3. Grammar Specification

### 3.1 Lexical Structure

#### 3.1.1 Tokens

```ebnf
/* Whitespace */
whitespace = " " | "\t" | "\n" | "\r";

/* Comments */
line-comment = ";" { any-character } "\n";
block-comment = "#|" { any-character } "|#";

/* Literals */
string = '"' { string-character } '"';
number = integer | float;
integer = ["-"] digit { digit };
float = ["-"] digit { digit } "." digit { digit } [exponent];
exponent = ("e" | "E") ["+" | "-"] digit { digit };
boolean = "true" | "false";
nil = "nil";

/* Keywords */
keyword = ":" identifier;

/* Identifiers */
identifier = letter { letter | digit | "-" | "_" };
namespaced-identifier = identifier { "." identifier } "/" identifier;
versioned-identifier = identifier { "." identifier } ":" version "/" identifier;
version = "v" digit { digit } { "." digit { digit } };

/* Special tokens */
left-paren = "(";
right-paren = ")";
left-bracket = "[";
right-bracket = "]";
left-brace = "{";
right-brace = "}";
```

#### 3.1.2 Special Characters

```ebnf
/* Special forms */
special-forms = "let" | "if" | "fn" | "do" | "match" | "try" | "with-resource";

/* Capability keywords */
capability-keywords = "capability" | "provider" | "attestation";

/* Type keywords */
type-keywords = "string" | "number" | "boolean" | "null" | "array" | "vector" | "map" | "union" | "optional";
```

### 3.2 Syntactic Structure

#### 3.2.1 Expressions

```ebnf
/* Primary expressions */
expression = literal | symbol | keyword | list | vector | map | capability-call;

/* Literals */
literal = string | number | boolean | nil | keyword;

/* Symbols */
symbol = identifier | namespaced-identifier | versioned-identifier;

/* Lists (function calls and special forms) */
list = left-paren expression { expression } right-paren;

/* Vectors */
vector = left-bracket { expression } right-bracket;

/* Maps */
map = left-brace { key-value-pair } right-brace;
key-value-pair = expression expression;

/* Map type (braced) annotation */
/* A braced map type may be used in type annotations and schema-like literals.
  Example: [:map { :host :string :port :int }] represents a map with keys
  :host (string) and :port (int).
*/

/* Capability calls */
capability-call = left-paren "capability" keyword expression right-paren;
```

#### 3.2.2 Special Forms

```ebnf
/* Let binding */
let-form = left-paren "let" left-bracket binding-pair { binding-pair } right-bracket expression right-paren;
binding-pair = symbol expression;

/* Conditional */
if-form = left-paren "if" expression expression [expression] right-paren;

/* Function definition */
fn-form = left-paren "fn" [symbol] left-bracket { symbol } right-bracket expression right-paren;

/* Sequential execution */
do-form = left-paren "do" { expression } right-paren;

/* Pattern matching */
match-form = left-paren "match" expression { match-clause } right-paren;
match-clause = pattern expression;
pattern = literal | symbol | left-bracket { pattern } right-bracket | left-brace { key-pattern } right-brace;
key-pattern = keyword pattern;

/* Error handling */
try-form = left-paren "try" expression { catch-clause } right-paren;
catch-clause = left-pracket "catch" symbol symbol expression right-pracket;

/* Resource management */
with-resource-form = left-paren "with-resource" left-bracket binding-pair { binding-pair } right-bracket expression right-paren;
```

## 4. Semantic Specification

### 4.1 Evaluation Model

RTFS 2.0 uses a strict, eager evaluation model with the following characteristics:

#### 4.1.1 Evaluation Rules

1. **Literals**: Self-evaluating
2. **Symbols**: Resolved in current environment
3. **Keywords**: Self-evaluating
4. **Lists**: Special form or function application
5. **Vectors**: Evaluated element-wise
6. **Maps**: Evaluated key-value-wise
7. **Capability calls**: Resolved and executed

#### 4.1.2 Environment Model

```clojure
;; Environment structure
{:bindings {:symbol value}
 :parent environment
 :capabilities {:capability-id capability}
 :context {:user-id string
           :security-level keyword
           :resource-limits map}}
```

### 4.2 Special Form Semantics

#### 4.2.1 Let Expressions

```clojure
(let [x 10 y 20] (+ x y))
;; Evaluates to: 30

;; Multiple bindings
(let [x 10
      y (* x 2)
      z (+ x y)]
  z)
;; Evaluates to: 30
```

**Semantics:**
1. Evaluate all binding expressions in order
2. Create new environment with bindings
3. Evaluate body expression in new environment
4. Return body expression value

#### 4.2.2 Conditional Expressions

```clojure
(if (> x 10) "big" "small")
;; Evaluates to: "big" if x > 10, "small" otherwise

(if-let [value (get-value)] value "default")
;; Evaluates to: value if get-value returns non-nil, "default" otherwise
```

**Semantics:**
1. Evaluate condition expression
2. If condition is truthy, evaluate then-expression
3. If condition is falsy, evaluate else-expression (if provided)
4. Return evaluated expression value

#### 4.2.3 Function Definitions

```clojure
;; Anonymous function
(fn [x y] (+ x y))

;; Named function
(fn add [x y] (+ x y))

;; Recursive function
(fn factorial [n]
  (if (= n 0)
    1
    (* n (factorial (- n 1)))))
```

**Semantics:**
1. Create function object with parameters and body
2. Function captures current environment (closure)
3. When called, create new environment with parameter bindings
4. Evaluate body in new environment
5. Return body expression value

#### 4.2.4 Do Blocks

```clojure
(do
  (println "Step 1")
  (println "Step 2")
  42)
;; Evaluates to: 42
```

**Semantics:**
1. Evaluate all expressions in order
2. Return value of last expression
3. All intermediate expressions are evaluated for side effects

#### 4.2.5 Step Execution with Logging

```clojure
(step "step-name"
  (let [data (fetch-data)]
    (process-data data)))
```

**Semantics:**
1. **Before**: Log `PlanStepStarted` action to the Causal Chain
2. **During**: Evaluate body expression in current environment
3. **After**: Log `PlanStepCompleted` or `PlanStepFailed` action with result
4. Return body expression value

**CCOS Integration:**
- Automatically logs step execution to the Causal Chain
- Provides audit trail for plan execution
- Enables step-level monitoring and debugging
- Maintains execution context for error handling

**Example:**
```clojure
(step "fetch-sales-data"
  (let [data (call :com.acme.db:v1.0:sales-query
                   {:query "SELECT * FROM sales WHERE quarter = 'Q2-2025'"})]
    data))

(step "analyze-data"
  (let [summary (call :com.openai:v1.0:data-analysis
                      {:data sales-data
                       :analysis-type :quarterly-summary})]
    summary))
```

#### 4.2.6 Pattern Matching

```clojure
(match value
  0 "zero"
  1 "one"
  n (str "number: " n))

(match data
  {:type "user" :id id} (str "User: " id)
  {:type "admin" :id id} (str "Admin: " id)
  _ "Unknown")
```

**Semantics:**
1. Evaluate match expression
2. Try each pattern in order
3. If pattern matches, evaluate corresponding expression
4. Return evaluated expression value
5. If no pattern matches, error

#### 4.2.7 Error Handling

```clojure
(try
  (risky-operation)
  (catch Exception e
    (handle-error e))
  (finally
    (cleanup)))
```

**Semantics:**
1. Evaluate try expression
2. If exception occurs, evaluate catch expressions
3. Always evaluate finally expression
4. Return try expression value or catch expression value

#### 4.2.8 Resource Management

```clojure
(with-resource [file (open-file "data.csv")]
  (with-resource [db (connect-database)]
    (process-data file db)))
```

**Semantics:**
1. Evaluate resource expressions
2. Create new environment with resource bindings
3. Evaluate body expression
4. Automatically cleanup resources
5. Return body expression value

### 4.3 Capability Execution

#### 4.3.1 Capability Call Syntax

```clojure
[:capability :data.process
 {:input {:data user-data
          :operations [:clean :validate :transform]}
  :provider :local
  :attestation "sha256:abc123..."}]
```

#### 4.3.2 Capability Resolution

1. **Discovery**: Find capability in marketplace
2. **Validation**: Verify attestation and permissions
3. **Resolution**: Select appropriate provider
4. **Execution**: Execute capability with input
5. **Validation**: Verify output against schema
6. **Recording**: Log execution to causal chain

## 5. Type System

### 5.1 Type Expressions

```clojure
;; Primitive types
:string
:number
:boolean
:null

;; Complex types
[:array :number]
[:vector :string]
[:map :string :number]
[:union :string :number]
[:optional :string]

;; Map types with braced syntax
[:map {:host :string :port :int}]
[:map {:name :string :age :int :active :bool}]

;; Custom types
[:struct {:name :string
          :age :number
          :active :boolean}]

[:enum "red" "green" "blue"]
```

### 5.2 Type Checking

#### 5.2.1 Type Inference

```clojure
;; Automatic type inference
(let [x 10]           ; x : number
  (let [y "hello"]    ; y : string
    (+ x 5)))         ; result : number

;; Function type inference
(fn add [x y]         ; x : number, y : number
  (+ x y))            ; result : number
```

#### 5.2.2 Type Validation

```clojure
;; Runtime type checking
(defn process-data [data]
  {:input-schema {:data [:array :number]
                  :operations [:vector :keyword]}
   :output-schema {:result [:map {:processed [:array :number]
                                 :metadata [:map]}]}
   :capabilities-required [:data.process]})
```

### 5.3 Schema Validation

All RTFS 2.0 objects must conform to their schemas:

```clojure
;; Validate capability input
(validate-schema input-data capability-input-schema)

;; Validate capability output
(validate-schema output-data capability-output-schema)
```

## 6. Standard Library

### 6.1 Core Functions

#### 6.1.1 Arithmetic Functions

```clojure
(+ x y)           ; Addition
(- x y)           ; Subtraction
(* x y)           ; Multiplication
(/ x y)           ; Division
(mod x y)         ; Modulo
(inc x)           ; Increment
(dec x)           ; Decrement
(abs x)           ; Absolute value
(min x y)         ; Minimum
(max x y)         ; Maximum
```

#### 6.1.2 Comparison Functions

```clojure
(= x y)           ; Equality
(not= x y)        ; Inequality
(< x y)           ; Less than
(<= x y)          ; Less than or equal
(> x y)           ; Greater than
(>= x y)          ; Greater than or equal
(zero? x)         ; Zero check
(pos? x)          ; Positive check
(neg? x)          ; Negative check
```

#### 6.1.3 Logical Functions

```clojure
(and x y)         ; Logical AND
(or x y)          ; Logical OR
(not x)           ; Logical NOT
(true? x)         ; True check
(false? x)        ; False check
(nil? x)          ; Nil check
```

### 6.2 Collection Functions

#### 6.2.1 Vector Functions

```clojure
(count coll)      ; Count elements
(empty? coll)     ; Empty check
(first coll)      ; First element
(rest coll)       ; Rest of elements
(nth coll n)      ; Nth element
(conj coll x)     ; Conjoin element
(vec coll)        ; Convert to vector
(vector x y z)    ; Create vector
```

#### 6.2.2 Map Functions

```clojure
(get map key)     ; Get value by key
(assoc map key val) ; Associate key-value
(dissoc map key)  ; Dissociate key
(keys map)        ; Get all keys
(vals map)        ; Get all values
(merge map1 map2) ; Merge maps
(select-keys map keys) ; Select specific keys
```

#### 6.2.3 Sequence Functions

```clojure
(map f coll)      ; Apply function to each element
(filter pred coll) ; Filter elements
(reduce f init coll) ; Reduce collection
(take n coll)     ; Take first n elements
(drop n coll)     ; Drop first n elements
(range n)         ; Create range
(repeat n x)      ; Repeat element n times
```

### 6.3 String Functions

```clojure
(str x y z)       ; Convert to string
(str/split s sep) ; Split string
(str/join sep coll) ; Join collection
(str/upper-case s) ; Convert to uppercase
(str/lower-case s) ; Convert to lowercase
(str/trim s)      ; Trim whitespace
(str/replace s pattern replacement) ; Replace pattern
(str/starts-with? s prefix) ; Check prefix
(str/ends-with? s suffix) ; Check suffix
```

### 6.4 Type Functions

```clojure
(type x)          ; Get type of value
(instance? type x) ; Check instance type
(cast type x)     ; Cast to type
(validate schema x) ; Validate against schema
(conform schema x) ; Conform to schema
```

### 6.5 Capability Functions

```clojure
(capability/discover pattern) ; Discover capabilities
(capability/execute cap-id input) ; Execute capability
(capability/validate cap-id input) ; Validate input
(capability/attest cap-id) ; Verify attestation
(capability/provenance cap-id) ; Get provenance
```

### 6.6 Resource Functions

```clojure
(resource/open uri) ; Open resource
(resource/close resource) ; Close resource
(resource/read resource) ; Read from resource
(resource/write resource data) ; Write to resource
(resource/exists? uri) ; Check if resource exists
(resource/size uri) ; Get resource size
```

### 6.7 Security Functions

```clojure
(security/verify-attestation attestation) ; Verify attestation
(security/check-permissions user-id permissions) ; Check permissions
(security/hash data) ; Hash data
(security/sign data key) ; Sign data
(security/verify-signature data signature key) ; Verify signature
```

## 7. Error Handling

### 7.1 Error Types

```clojure
;; Runtime errors
:type-error       ; Type mismatch
:capability-error ; Capability execution error
:resource-error   ; Resource access error
:security-error   ; Security violation
:validation-error ; Schema validation error
:network-error    ; Network communication error
```

### 7.2 Error Handling Patterns

```clojure
;; Pattern matching for errors
(match (try (risky-operation) (catch e e))
  {:type :capability-error :message msg} (handle-capability-error msg)
  {:type :security-error :message msg} (handle-security-error msg)
  {:type :network-error :message msg} (handle-network-error msg)
  _ (handle-unknown-error))

;; Error recovery
(try
  (primary-operation)
  (catch :capability-error e
    (fallback-operation))
  (catch :security-error e
    (request-permissions))
  (finally
    (cleanup)))
```

## 8. Performance Characteristics

### 8.1 Execution Performance

- **Local capability execution**: < 1ms overhead
- **HTTP capability execution**: < 100ms overhead (network dependent)
- **Schema validation**: < 1ms per object
- **Type checking**: < 0.1ms per expression
- **Capability discovery**: < 10ms (cached)

### 8.2 Memory Usage

- **Expression evaluation**: Minimal overhead
- **Environment creation**: ~100 bytes per binding
- **Capability metadata**: ~1KB per capability
- **Schema validation**: ~10KB per schema

### 8.3 Optimization Features

- **Expression caching**: Frequently used expressions are cached
- **Lazy evaluation**: Supported for streaming operations
- **Type inference caching**: Inferred types are cached
- **Capability result caching**: Capability results can be cached

## 9. Security Features

### 9.1 Capability Security

```clojure
;; Capability attestation verification
(security/verify-attestation capability-attestation)

;; Permission checking
(security/check-permissions user-id required-permissions)

;; Input validation
(validate-schema input-data capability-input-schema)

;; Output validation
(validate-schema output-data capability-output-schema)
```

### 9.2 Resource Security

```clojure
;; Resource access control
(with-resource [file (resource/open uri :read-only)]
  (process-file file))

;; Secure resource cleanup
(with-resource [db (database/connect :secure)]
  (process-data db))
```

### 9.3 Execution Security

```clojure
;; Execution context validation
(validate-execution-context context)

;; Resource limit enforcement
(enforce-resource-limits limits)

;; Capability permission enforcement
(enforce-capability-permissions permissions)
```

## 10. Implementation Notes

### 10.1 Parser Implementation

The RTFS 2.0 parser is implemented using the Pest parsing library:

```rust
// Parser grammar (rtfs.pest)
expression = _{ 
    literal | keyword | symbol | 
    list | vector | map | capability_call 
}

list = { "(" ~ expression* ~ ")" }
vector = { "[" ~ expression* ~ "]" }
map = { "{" ~ (expression ~ expression)* ~ "}" }
capability_call = { "(" ~ "capability" ~ keyword ~ expression ~ ")" }
```

### 10.2 Runtime Implementation

The RTFS 2.0 runtime provides:

- **Expression evaluator**: Evaluates RTFS expressions
- **Environment manager**: Manages variable bindings and scope
- **Capability resolver**: Resolves and executes capabilities
- **Type checker**: Performs type checking and validation
- **Security enforcer**: Enforces security policies
- **Resource manager**: Manages resource lifecycle

### 10.3 Integration with CCOS

RTFS 2.0 integrates with CCOS through:

- **Orchestrator**: Executes RTFS plans
- **Global Function Mesh**: Resolves capability requests
- **Causal Chain**: Records execution events
- **Intent Graph**: Stores intent information
- **Capability Marketplace**: Discovers capabilities

## 11. Examples

### 11.1 Simple Data Processing

```clojure
;; Process user data
(let [user-data {:name "John" :age 30 :active true}
      processed-data (-> user-data
                        (assoc :processed true)
                        (assoc :timestamp (now)))]
  [:capability :data.save
   {:input {:data processed-data
            :format :json
            :location "users/processed"}}])
```

### 11.2 Error Handling

```clojure
;; Robust data processing with error handling
(try
  (let [data (load-data "input.csv")
        processed (process-data data)
        result (save-data processed "output.json")]
    {:status :success :result result})
  (catch :validation-error e
    {:status :error :message "Invalid data format"})
  (catch :capability-error e
    {:status :error :message "Processing failed"}))
```

### 11.3 Resource Management

```clojure
;; Secure file processing
(with-resource [input-file (file/open "input.txt" :read)]
  (with-resource [output-file (file/open "output.txt" :write)]
    (let [content (file/read input-file)
          processed (process-content content)]
      (file/write output-file processed))))
```

### 11.4 Capability Composition

```clojure
;; Compose multiple capabilities
[:capability :workflow.execute
 {:steps [[:capability :data.load {:source "input.csv"}]
          [:capability :data.process {:operations [:clean :validate]}]
          [:capability :data.analyze {:method :statistical}]
          [:capability :data.save {:destination "results.json"}]]}]
```

## 12. Conclusion

This specification defines the complete RTFS 2.0 language, providing a solid foundation for AI task execution within the CCOS framework. The language combines functional programming principles with capability-based execution, comprehensive type safety, and security-first design to create a powerful and safe environment for AI-driven workflows.

---

**Note**: This specification is complete and ready for implementation. All features described are designed to work seamlessly with the CCOS architecture and provide the foundation for safe, aligned, and intelligent cognitive computing. 