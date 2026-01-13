# RTFS 2.0 Syntax and Grammar

## Overview

RTFS 2.0 uses a **homoiconic s-expression syntax** where code and data share the same representation. All constructs are expressions that evaluate to values.

## Core Syntax Elements

### Literals

RTFS supports rich literal types:

```clojure
;; Primitive types
42                    ; integer
3.14                  ; float
"hello world"         ; string
true                  ; boolean
nil                   ; null value

;; Extended types
:keyword              ; keyword
timestamp             ; ISO 8601 timestamp strings
uuid                  ; UUID strings
resource://handle     ; resource handles
```

### Symbols and Identifiers

Symbols represent names and follow these rules:

```clojure
;; Valid symbols
foo
my-function
my.namespace/function
com.example:v1.0/api
+ - * / = < > ! ?
```

### Collections

RTFS provides three collection types:

```clojure
;; Lists - code and function calls
(func arg1 arg2)

;; Vectors - ordered sequences
[1 2 3 4]

;; Maps - key-value associations
{:key "value" :count 42}
```

## Special Forms

Special forms are built-in constructs that cannot be implemented as functions:

### Variable Binding
```clojure
;; let - lexical scoping
(let [x 1
      y (+ x 2)]
  (* x y))

;; def - global definitions
(def pi 3.14159)

;; defn - function definitions
(defn add [x y]
  (+ x y))

;; defn in let body - local function definitions with closures
;; This is the correct pattern for defining functions within let expressions
(let [value 42]
  (defn helper [x] (+ x value))  ; Function defined in let body
  (helper 8))                    ; Returns 50, captures 'value' as closure
```

**Important Note**: `defn` must be used in the **body** of `let` expressions, not in the bindings. This pattern enables proper lexical scoping and closure creation:

```clojure
;; ✅ CORRECT - defn in let body
(let [outer-var 10]
  (defn use-outer [] outer-var)  ; Creates closure over outer-var
  (use-outer))                   ; Returns 10

;; ❌ INCORRECT - defn in let bindings (will not work)
(let [(defn bad-fn [] 42)        ; This syntax is invalid
      value 5]
  (bad-fn))
```

### Control Flow
```clojure
;; if - conditional
(if (> x 0)
  "positive"
  "non-positive")

;; do - sequencing
(do
  (call :ccos.io/println "first")
  (call :ccos.io/println "second")
  42)

;; match - pattern matching
(match value
  0 "zero"
  n (str "number: " n)
  _ "other")
```

### Functions
```clojure
;; fn - anonymous functions
(fn [x] (* x x))

;; Variadic functions
(defn sum [& args]
  (reduce + 0 args))
```

## Pattern Matching and Destructuring

### Destructuring Patterns

RTFS supports rich destructuring in bindings:

```clojure
;; Vector destructuring
(let [[x y z] [1 2 3]]
  (+ x y z))

;; Map destructuring
(let [{:keys [name age] :as person} {:name "Alice" :age 30}]
  (str name " is " age " years old"))

;; Nested destructuring
(let [[[x y] z] [[1 2] 3]]
  (+ x y z))
```

### Match Patterns

Match expressions use patterns for conditional logic:

```clojure
(match data
  ;; Literal patterns
  0 "zero"
  1 "one"

  ;; Type patterns
  (:int n) (str "integer: " n)

  ;; Collection patterns
  [x y] (str "pair: " x "," y)
  {:name n :age a} (str n " is " a " years old")

  ;; Wildcard
  _ "other")
```

## Type Expressions

RTFS includes a structural type system:

```clojure
;; Primitive types
:int :float :string :bool :nil

;; Collection types
[:vector :int]        ; vector of integers
[:tuple :string :int] ; tuple type
[:map {:name :string :age :int}] ; map type

;; Function types
[:fn [:int :int] :int] ; function taking two ints, returning int

;; Union types
[:union :int :string]  ; either int or string

;; Refined types
[:and :int [:> 0] [:< 100]] ; int between 1 and 99

;; Optional types
:string? ; equivalent to [:union :string :nil]
```

## Macros and Metaprogramming

RTFS 2.0 provides a full macro system for compile-time code transformation:

```clojure
;; Macro definition
(defmacro when [condition & body]
  `(if ~condition (do ~@body)))

;; Macro usage
(when (> x 0)
  (call :ccos.io/println "positive")
  (* x 2))
```

The macro system includes:
- **`defmacro`**: Define custom syntax transformations
- **Quasiquote** (`` ` ``): Quote code with selective unquoting
- **Unquote** (`~`): Evaluate expressions within quasiquote
- **Unquote-splicing** (`~@`): Splice sequences into quasiquote

Implementation: `compiler/expander.rs` with full `MacroExpander` support including quasiquote level tracking and variadic parameters.

## Host Integration

RTFS yields to the host for side effects through the `call` primitive:

```clojure
;; Host capability call with keyword syntax
(call :ccos.state.kv/get "my-key")

;; Host capability call with string syntax
(call "ccos.state.kv.get" "my-key")
```

The `call` primitive is the primary mechanism for RTFS to interact with CCOS capabilities. It accepts a capability identifier (as a keyword or string) followed by arguments, and yields control to the host for execution.

## Advanced Patterns and Meta-Planner Usage

### Recursive Decomposition with defn in let

The meta-planner pattern demonstrates advanced usage of `defn` within `let` expressions for recursive decomposition:

```clojure
;; Meta-planner style recursive decomposition
(let [goal "resolve complex intent"
      max-depth 5]
  
  ;; Define recursive resolver function in let body
  (defn resolve-or-decompose [intent depth]
    (if (<= depth 0)
      {:resolved false :error "Max recursion depth reached" :intent intent}
      
      ;; Base case: simple intent can be resolved directly
      (if (simple-intent? intent)
        {:resolved true :intent intent :depth depth}
        
        ;; Recursive case: decompose complex intent
        (let [sub-intents (decompose intent)]
          {:resolved false
           :intent intent
           :sub-intents (map #(resolve-or-decompose % (- depth 1)) sub-intents)
           :depth depth}))))
  
  ;; Use the function with initial parameters
  (let [root-intent {:description goal :id "root" :complexity 8}]
    (resolve-or-decompose root-intent max-depth)))
```

This pattern shows:
- **Lexical scoping**: The function captures `max-depth` from the outer scope
- **Recursive decomposition**: The function calls itself with decremented depth
- **Closure creation**: Inner `let` bindings are accessible to the function
- **Structured data**: Returns maps with detailed resolution information

### Multiple Function Definitions

You can define multiple functions in the same `let` body:

```clojure
(let [data [1 2 3 4 5]
      threshold 3]
  
  (defn filter-above [items limit]
    (filter #(> % limit) items))
  
  (defn sum-squares [items]
    (reduce + (map #(* % %) items)))
  
  ;; Use both functions
  (let [filtered (filter-above data threshold)
        result (sum-squares filtered)]
    {:filtered filtered :sum result}))
```

### Function Factories

Functions can create and return other functions (higher-order functions):

```clojure
(let [base 10]
  (defn create-multiplier [factor]
    (fn [x] (* x factor base)))
  
  (let [double (create-multiplier 2)
        triple (create-multiplier 3)]
    (+ (double 5) (triple 2))))  ; Returns 10 + 60 = 70
```

## Metadata

Expressions can include metadata using the `^{...}` syntax.

**Implementation Note:**
- **Runtime Hints (`^{:runtime.* ...}`):** ✅ **Supported**. Metadata on expressions (especially `call`) keys starting with `runtime.` (e.g., `:runtime.timeout`) are successfully parsed and propagated to the CCOS Host as `CallMetadata`.
- **Definition Metadata (`defn ^{...}`):** ⚠️ **Partial/Dropped**. While the parser accepts metadata on definitions (like `:doc` or `:private`), the current IR compiler **drops** this metadata, meaning it is not available at runtime or for introspection.

```clojure
;; ✅ SUPPORTED: Runtime hints propagated to Host
^{:runtime.timeout 500
  :runtime.idempotent true}
(call "http.get" {:url "..."})

;; ⚠️ PARTIAL: Parsed but currently dropped by compiler
^{:doc "A simple function"
  :author "Alice"}
(defn greet [name]
  (str "Hello, " name))
```

## Grammar Rules

The RTFS grammar is defined in `rtfs.pest` with the following precedence:

1. Special forms and literals
2. Collections (lists, vectors, maps)
3. Symbols and identifiers
4. Metadata and other modifiers

This syntax design enables RTFS to be both **readable for humans** and **manipulable as data** for metaprogramming.</content>
<parameter name="filePath">/home/mandubian/workspaces/mandubian/ccos/docs/rtfs-2.0/specs-new/01-syntax-and-grammar.md