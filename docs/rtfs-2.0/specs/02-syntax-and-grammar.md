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
(defn sum [args]
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
[:refined :int (> 0) (< 100)] ; int between 1 and 99

;; Optional types
:string? ; equivalent to [:union :string :nil]
```

## Macros and Metaprogramming

Macros enable compile-time code transformation:

```clojure
;; Define a macro
(defmacro when [condition & body]
  `(if ~condition (do ~@body)))

;; Use the macro
(when (> x 0)
  (call :ccos.io/println "positive")
  (* x 2))
```

## Host Integration

RTFS yields to the host for side effects through the `call` primitive:

```clojure
;; Host capability call with keyword syntax
(call :ccos.state.kv/get "my-key")

;; Host capability call with string syntax
(call "ccos.state.kv.get" "my-key")
```

The `call` primitive is the primary mechanism for RTFS to interact with CCOS capabilities. It accepts a capability identifier (as a keyword or string) followed by arguments, and yields control to the host for execution.

## Metadata

Expressions can include metadata:

```clojure
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