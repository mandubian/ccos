---
name: RTFS Grammar
description: Learn RTFS (Reason about The Functional Spec) - the pure functional language for CCOS agents
---

# RTFS Grammar Reference

RTFS is a **pure functional language** using S-expression syntax, designed for LLM agents. All side effects are delegated to the host via `(call ...)`.

## Quick Reference

### Core Syntax
```clojure
;; Lists = code/function calls
(+ 1 2 3)                       ; => 6
(if (> x 0) "positive" "zero")

;; Vectors = ordered data
[1 2 3 4]                       ; literal vector
(get [10 20 30] 1)              ; => 20

;; Maps = key-value data
{:name "Alice" :age 30}         ; map literal
(get {:a 1 :b 2} :a)            ; => 1
```

### Literals
```clojure
42                              ; integer
3.14                            ; float
"hello world"                   ; string (UTF-8)
true / false                    ; booleans
nil                             ; null/empty
:keyword                        ; self-evaluating keyword
:my.ns/qualified                ; namespaced keyword
2026-01-28T10:00:00Z            ; ISO 8601 timestamp
```

### Variable Binding
```clojure
(def pi 3.14159)                ; global definition

(let [x 1 y (+ x 2)]            ; lexical scoping
  (* x y))                      ; => 3

;; Destructuring
(let [[a b] [1 2]] (+ a b))                    ; vector => 3
(let [{:keys [name age]} {:name "Al" :age 30}] 
  name)                                         ; map => "Al"
```

### Functions
```clojure
(fn [x] (* x x))                ; anonymous function
(defn add [x y] (+ x y))        ; named function
(defn sum [& args]              ; variadic (rest args)
  (reduce + 0 args))
```

### Control Flow
```clojure
(if (> x 0) "positive" "non-positive")

(do                             ; sequencing
  (call "ccos.io.log" "step1")
  (call "ccos.io.log" "step2")
  42)                           ; returns last expr

(match value                    ; pattern matching
  0 "zero"
  [x y] (str "pair: " x y)
  {:name n} (str "hi " n)
  _ "other")
```

## The Host Boundary (CRITICAL)

RTFS is **pure** - it cannot perform side effects directly. All effects (I/O, network, state) must go through the host via `(call ...)`:

```clojure
;; Pure code (no call needed)
(+ 1 2 3)                              ; arithmetic
(map inc [1 2 3])                      ; data transformation
(filter even? [1 2 3 4])               ; filtering

;; Effectful code (REQUIRES call)
(call "ccos.io.log" "Hello")           ; I/O
(call "ccos.network.http-fetch" url)   ; network  
(call "ccos.io.read-file" "/path")     ; file system
(call "ccos.state.kv.get" :key)        ; state access
```

### Built-in Capabilities
```clojure
;; Data transformation
(call "ccos.json.parse" "{\"a\": 1}")      ; => {:a 1}
(call "ccos.json.stringify" {:a 1})        ; => "{\"a\":1}"

;; System
(call "ccos.system.get-env" "PATH")        ; => "/usr/bin:..."
(call "ccos.system.current-time")          ; => timestamp

;; I/O
(call "ccos.io.log" "message")
(call "ccos.io.read-file" "/tmp/foo")
(call "ccos.io.write-file" "/tmp/foo" "bar")

;; State
(call "ccos.state.kv.put" :key "value")
(call "ccos.state.kv.get" :key)
```

## Type Expressions

RTFS supports gradual typing with schemas:

```clojure
;; Primitive types
:int :float :string :bool :nil :any

;; Collection types
[:vector :int]                          ; vector of ints
[:tuple :string :int]                   ; fixed tuple
[:map [:name :string] [:age :int]]      ; map schema

;; Function types
[:fn [:int :int] :int]                  ; (int, int) -> int

;; Union & Optional
[:union :int :string :nil]              ; one of these types
:string?                                ; sugar for [:union :string :nil]

;; Refined types (constraints)
[:and :int [:> 0]]                      ; positive int
[:and :int [:>= 0] [:< 100]]            ; int in [0, 100)
[:and :string [:min-length 1] [:max-length 255]]
```

## Capability Definition

Capabilities are the core building blocks:

```clojure
(capability "my-tool.fetch-data"
  :description "Fetches data from API"
  :input-schema [:map
    [:id :string]
    [:limit [:and :int [:> 0]]]]
  :output-schema [:map [:data [:vector :any]]]
  :effects [:network]
  :implementation (fn [inputs]
    (let [url (str "https://api.example.com/" (:id inputs))]
      (call "ccos.network.http-fetch" {:url url}))))
```

## Common Patterns

### Multi-step workflow
```clojure
(let [weather (call "weather.get" {:city "Paris"})
      price   (call "crypto.get-price" {:symbol "BTC"})
      fact    (call "catfact.random" {})]
  {:weather weather :btc price :cat fact})
```

### Error handling with match
```clojure
(match (call "api.fetch" {:id "123"})
  {:error e} (call "ccos.io.log" (str "Failed: " e))
  {:data d}  (process-data d)
  _          (call "ccos.io.log" "Unknown response"))
```

## Resources

- Full specs: `docs/rtfs-2.0/specs/`
- REPL guide: `docs/rtfs-2.0/guides/repl-guide.md`
- Type checking: `docs/rtfs-2.0/guides/type-checking-guide.md`
