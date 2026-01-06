# RTFS 2.0: Core Syntax and Data Types

## 1. Fundamental Syntax: S-Expressions

RTFS is built on **s-expressions** (symbolic expressions), the fundamental building blocks of Lisp-family languages. An s-expression is recursively defined as either an **atom** or a **list** of s-expressions.

### Atoms

Atoms are indivisible values that serve as the leaves of the syntax tree:

- **Integers**: Whole numbers with arbitrary precision
  ```rtfs
  42
  -100
  0
  ```

- **Strings**: UTF-8 encoded text sequences enclosed in double quotes
  ```rtfs
  "hello world"
  "line 1\nline 2"
  "\"quoted\" text"
  ```

- **Symbols**: Identifiers used for variables, functions, and keywords
  ```rtfs
  x
  my-variable
  function-name
  + - * / = > <
  ```

- **Keywords**: Special symbols starting with `:` used as map keys and identifiers
  ```rtfs
  :name
  :fs.read
  :async.compute
  ```

- **Booleans**: Logical true/false values
  ```rtfs
  true
  false
  ```

- **Nil**: Represents absence of value or empty collections
  ```rtfs
  nil
  ```

### Lists

Lists are ordered sequences of s-expressions enclosed in parentheses `()`. Lists serve dual purposes:

1. **Data structures**: Ordered collections of values
   ```rtfs
   (1 2 3)
   ("hello" 42 my-symbol :keyword)
   (a (b c) (d (e)))
   ```

2. **Function calls**: The first element is the function/operator, remaining elements are arguments
   ```rtfs
   (+ 1 2 3)        ; => 6
   (println "hello") ; Side effect via host call
   (if (> x 0) x (- x)) ; Conditional
   ```

## 2. Collection Types

RTFS provides three primary collection types, each optimized for different use cases.

### Vectors: Indexed Sequences

Vectors `[]` are ordered, indexed collections providing O(1) random access:

```rtfs
;; Basic vector
[1 2 3 4 5]

;; Nested structures
[1 [2 3] 4]

;; Mixed types
["string" 42 :keyword [1 2 3]]

;; Vector operations
(count [1 2 3])     ; => 3
(get [10 20 30] 1)  ; => 20
(conj [1 2] 3)      ; => [1 2 3]
```

### Maps: Key-Value Associations

Maps `{}` associate keys with values, supporting any RTFS value as keys:

```rtfs
;; Keyword keys (most common)
{:name "Alice" :age 30 :active true}

;; String keys
{"first-name" "Bob" "last-name" "Smith"}

;; Mixed key types
{42 "the answer" :pi 3.14159 [1 2] "vector key"}

;; Nested maps
{:user {:id 123 :profile {:name "Alice" :email "alice@example.com"}}}

;; Map operations
(get {:a 1 :b 2} :a)           ; => 1
(assoc {:a 1} :b 2)            ; => {:a 1 :b 2}
(dissoc {:a 1 :b 2} :a)        ; => {:b 2}
(keys {:a 1 :b 2})             ; => (:a :b)
(vals {:a 1 :b 2})             ; => (1 2)
```

### Lists: Sequential Access

Lists `()` are linked sequences optimized for sequential access and functional operations:

```rtfs
;; Basic list
(1 2 3 4 5)

;; List operations
(first (1 2 3))     ; => 1
(rest (1 2 3))      ; => (2 3)
(cons 0 (1 2 3))    ; => (0 1 2 3)
(conj (1 2) 3)      ; => (1 2 3)
```

## 3. Special Forms

Special forms are built-in syntactic constructs that cannot be implemented as regular functions. They have special evaluation rules.

### Definition Forms

```rtfs
;; Value binding
(def pi 3.14159)
(def my-list (1 2 3 4 5))

;; Function definition
(defn square [x] (* x x))
(defn add [a b] (+ a b))

;; Variable binding with lexical scope
(let [x 10 y 20]
  (+ x y))  ; => 30
```

### Control Flow

```rtfs
;; Conditional execution
(if (> x 0)
    (println "positive")
    (println "non-positive"))

;; Multi-way conditional
(cond
  (> x 100) "large"
  (> x 10)  "medium"
  :else     "small")

;; Logical operators (short-circuiting)
(and (> x 0) (< x 100))  ; true if x in (0, 100)
(or (= x 0) (= x 1))     ; true if x is 0 or 1
(not (= x 0))            ; true if x is not 0
```

### Function Forms

```rtfs
;; Anonymous functions
(fn [x] (* x x))
(fn [a b] (+ a b))

;; Lambda (alternative syntax)
(lambda [x] (* x x))

;; Higher-order functions
(map (fn [x] (* x x)) [1 2 3 4])  ; => [1 4 9 16]
(filter (fn [x] (> x 0)) [-1 0 1 2])  ; => [1 2]
(reduce (fn [acc x] (+ acc x)) 0 [1 2 3 4])  ; => 10
```

## 4. Evaluation Model

RTFS uses **applicative order evaluation** (eager evaluation) by default:

1. **Atoms** evaluate to themselves
2. **Lists** are evaluated as function calls:
   - Evaluate all elements
   - Apply first element (function) to remaining elements (arguments)

### Special Evaluation Rules

- **Quote** prevents evaluation: `'(+ 1 2)` remains as the list `(+ 1 2)`
- **Quasiquote** allows selective evaluation: `` `(list ,x ,@y) ``
- **Special forms** have custom evaluation rules

### Example Evaluation

```rtfs
;; Expression: (+ (* 2 3) 4)
;; Step 1: Evaluate arguments
;;   (* 2 3) => 6
;;   4 => 4
;; Step 2: Apply + to 6 and 4
;; Result: 10
```

## 5. Comments and Documentation

```rtfs
;; Single-line comments
(+ 1 2) ;; Inline comment

;; Multi-line comments not supported
;; Use multiple single-line comments

;; Documentation strings (convention)
(defn add
  "Adds two numbers together"
  [a b]
  (+ a b))
```

## 6. Literals and Constants

RTFS supports various literal forms for common values:

```rtfs
;; Numbers
42      ; integer
-3.14   ; floating point (if supported)

;; Booleans
true
false

;; Strings with escapes
"hello\nworld"
"tab:\there"
"quote: \"hello\""

;; Collections
[]      ; empty vector
{}      ; empty map
()      ; empty list (nil)

;; Keywords (self-evaluating)
:keyword
:namespace/keyword
```

## 7. Symbol Resolution

Symbols are resolved through lexical scoping with the following precedence:

1. **Local bindings** (let, function parameters)
2. **Module-level definitions** (def, defn)
3. **Imported symbols** (qualified or aliased)
4. **Built-in functions** and special forms
5. **Host capabilities** (via call mechanism)

### Namespace Qualification

```rtfs
;; Qualified access
(math/sqrt 16)    ; sqrt from math module
(io/read-file)    ; read-file from io module

;; Aliased imports
(import [math :as m])
(m/sqrt 16)       ; same as math/sqrt
```

## 8. Type System Integration

While RTFS has a dynamic type system, it supports optional type annotations. Type annotations in RTFS are **just symbols** - any symbol or keyword can be used as a type hint. The grammar accepts them all.

**Runtime primitive types** (from `Value::get_type()`):
- `integer`, `float`, `boolean`, `string`
- `vector`, `list`, `map`
- `symbol`, `keyword`, `nil`
- `timestamp`, `uuid`, `resource-handle`, `function`, `error`

**Type annotation examples**:

```rtfs
;; With lowercase runtime types
(defn add [a : integer b : integer] : integer
  (+ a b))

;; With capitalized convention (also valid, just symbols)
(defn add [a : Integer b : Integer] : Integer
  (+ a b))

;; With keyword types
(defn add [a :int b :int] :int
  (+ a b))

;; Custom type symbols (any symbol works)
(defn process [x : CustomType] : MyResult
  (do-something x))
```

**⚠️ Note**: `assert-type` and `cast-to` functions shown in earlier drafts are **not implemented**. Type annotations are optional and act as documentation/hints. Runtime type checking is performed by the `TypeValidator` at configurable levels (basic/standard/strict).

## 9. Error Conditions

Common syntax and evaluation errors:

```rtfs
;; Undefined symbol
undefined-symbol  ; RuntimeError: UndefinedSymbol

;; Wrong number of arguments
(+ 1)            ; RuntimeError: ArityError

;; Type mismatch
(+ "hello" 42)   ; RuntimeError: TypeError

;; Invalid syntax
(+ 1 2          ; ParseError: Unmatched parentheses
```

## 10. Implementation Notes

### Memory Model
- All data structures are immutable
- Reference counting or garbage collection
- No explicit memory management

### Performance Characteristics
- O(1) atom evaluation
- O(n) list traversal
- O(log n) map operations (hash-based)
- O(1) vector random access

### Thread Safety
- Immutable data structures are thread-safe
- Host boundary provides concurrency control
- No shared mutable state within RTFS

This core syntax provides the foundation for all RTFS programs, enabling the construction of complex, composable systems while maintaining safety and predictability.