# RTFS 2.0: Type System

## 1. Type System Overview

RTFS uses a simple, dynamic type system with optional type annotations. Types are checked at runtime with some compile-time inference.

### Core Principles

- **Dynamic Typing**: Runtime type checking with flexibility
- **Optional Annotations**: Type hints for clarity and documentation
- **Simple Types**: Basic types for data and functions

## 2. Basic Types

### Primitive Types

```rtfs
;; Boolean
true   ; Bool
false  ; Bool

;; Numbers
42     ; Int
3.14   ; Float

;; Strings
"Hello"  ; String

;; Symbols and Keywords
'symbol  ; Symbol
:keyword ; Keyword

;; Nil
nil     ; Nil
```

### Collection Types

```rtfs
;; Vectors
[1 2 3]        ; Vector of Int
["a" "b" "c"]  ; Vector of String

;; Maps
{:name "Alice" :age 30}  ; Map

;; Lists
'(1 2 3)       ; List
```

### Function Types

```rtfs
;; Simple function
(fn [x] (* x 2))  ; Function

;; Multiple parameters
(fn [a b] (+ a b))  ; Function with two parameters
```

## 3. Type Annotations

### Variable Annotations

```rtfs
;; Optional type annotation
(def x 42)  ; Int inferred

;; Function parameters
(defn add [a b]
  (+ a b))
```

## 4. Runtime Type Checking

### Type Predicates

```rtfs
;; Check types at runtime
(int? 42)      ; true
(string? "hi") ; true
(vector? [1 2]) ; true

;; Type assertions
(defn safe-add [a b]
  (if (and (number? a) (number? b))
    (+ a b)
    (error "Arguments must be numbers")))
```

This simple type system provides basic type safety and documentation while maintaining RTFS's dynamic and flexible nature.