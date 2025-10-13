# RTFS 2.0: Pattern Matching and Destructuring

## 1. Overview

RTFS provides sophisticated pattern matching through **destructuring**, allowing direct binding of nested data structures to local variables. This feature enables concise, declarative code that extracts multiple values from complex data structures in a single operation.

Destructuring is supported in:
- `let` bindings
- Function parameters (`defn`, `fn`)
- Loop constructs (where applicable)

## 2. Vector Destructuring

Vector destructuring binds elements of a vector to individual symbols by position.

### Basic Vector Destructuring

```rtfs
;; Bind each element to a symbol
(let [[a b c] [1 2 3]]
  (+ a b c))  ; => 6

;; Fewer symbols than elements (ignores extras)
(let [[x y] [1 2 3 4]]
  (+ x y))    ; => 3

;; More symbols than elements (binds extras to nil)
(let [[a b c d] [1 2]]
  [a b c d])  ; => [1 2 nil nil]
```

### Rest Parameters

The `&` symbol captures remaining elements as a list:

```rtfs
;; Capture rest of vector
(let [[head & tail] [1 2 3 4 5]]
  [head tail])  ; => [1 (2 3 4 5)]

;; Rest with specific bindings
(let [[first second & others] [1 2 3 4 5]]
  [first second others])  ; => [1 2 (3 4 5)]

;; Only rest (ignore first elements)
(let [[& rest] [1 2 3 4]]
  rest)  ; => (1 2 3 4)
```

### Nested Vector Destructuring

Vectors can be nested arbitrarily deep:

```rtfs
;; Simple nesting
(let [[[a b] c] [[1 2] 3]]
  [a b c])  ; => [1 2 3]

;; Complex nesting with rest
(let [[[x & xs] y & ys] [[1 2 3] 4 5 6]]
  [x xs y ys])  ; => [1 (2 3) 4 (5 6)]

;; Matrix-like structures
(let [[[a b] [c d]] [[1 2] [3 4]]]
  (+ a b c d))  ; => 10
```

## 3. Map Destructuring

Map destructuring binds values from maps using keys, supporting both keyword and explicit key specifications.

### Keyword Key Destructuring

The `:keys` directive binds map values using keyword keys:

```rtfs
;; Basic keyword destructuring
(let [{:keys [name age]} {:name "Alice" :age 30}]
  (str name " is " age))  ; => "Alice is 30"

;; Multiple keys
(let [{:keys [x y z]} {:x 1 :y 2 :z 3 :w 4}]
  (+ x y z))  ; => 6

;; Missing keys bind to nil
(let [{:keys [present missing]} {:present "here"}]
  [present missing])  ; => ["here" nil]
```

### Explicit Key Binding

Specify exact key-value bindings:

```rtfs
;; String keys
(let [{"name" n "age" a} {"name" "Bob" "age" 25}]
  [n a])  ; => ["Bob" 25]

;; Mixed key types
(let [{42 answer :pi pi-val} {42 "life" :pi 3.14}]
  [answer pi-val])  ; => ["life" 3.14]
```

### Combined Destructuring

Mix `:keys` with explicit bindings:

```rtfs
;; Keys with explicit bindings
(let [{:keys [name] "id" user-id :role role}
      {:name "Alice" "id" 123 :role :admin}]
  [name user-id role])  ; => ["Alice" 123 :admin]
```

### :as Binding

Capture the entire original map with `:as`:

```rtfs
;; Bind destructured values and whole map
(let [{:keys [x y] :as point} {:x 10 :y 20 :z 30}]
  [x y point])  ; => [10 20 {:x 10 :y 20 :z 30}]

;; :as with explicit keys
(let [{"name" n :as user} {"name" "Alice" "age" 30}]
  [n user])  ; => ["Alice" {"name" "Alice" "age" 30}]
```

### Nested Map Destructuring

Maps can contain other maps:

```rtfs
;; Nested map destructuring
(let [{:keys [user profile]}
      {:user {:name "Alice" :id 123}
       :profile {:role :admin :level 5}}]
  [(:name user) (:role profile)])  ; => ["Alice" :admin]

;; Deep nesting with :keys
(let [{:keys [data]}
      {:data {:user {:name "Bob" :stats {:score 100}}}}]
  (-> data :user :stats :score))  ; => 100
```

## 4. Wildcard Patterns

The underscore `_` ignores values without binding them:

```rtfs
;; Ignore specific positions
(let [[_ important _] [1 42 3]]
  important)  ; => 42

;; Ignore in map destructuring
(let [{:keys [name _]} {:name "Alice" :temp "ignored"}]
  name)  ; => "Alice"

;; Multiple wildcards
(let [[a _ _ c] [1 2 3 4]]
  [a c])  ; => [1 4]
```

## 5. Function Parameter Destructuring

Destructuring works in function definitions and anonymous functions:

### defn with Destructuring

```rtfs
;; Vector parameter destructuring
(defn add-coords [[x y]]
  (+ x y))

(add-coords [3 4])  ; => 7

;; Map parameter destructuring
(defn greet {:keys [name title]}
  (str title " " name))

(greet {:name "Alice" :title "Dr."})  ; => "Dr. Alice"

;; Mixed parameters
(defn process-user [id {:keys [name email]}]
  {:id id :name name :email email})

(process-user 123 {:name "Bob" :email "bob@example.com"})
; => {:id 123 :name "Bob" :email "bob@example.com"}
```

### Anonymous Function Destructuring

```rtfs
;; Vector destructuring in fn
(map (fn [[x y]] (+ x y)) [[1 2] [3 4] [5 6]])
; => [3 7 11]

;; Map destructuring in fn
(filter (fn [{:keys [active]}] active)
        [{:name "Alice" :active true}
         {:name "Bob" :active false}])
; => [{:name "Alice" :active true}]
```

### Variadic Functions with Destructuring

```rtfs
;; Fixed params with destructuring, rest as list
(defn make-pairs [prefix & items]
  (map (fn [item] [prefix item]) items))

(make-pairs :data 1 2 3)
; => [[:data 1] [:data 2] [:data 3]]

;; Note: & rest cannot be destructured directly
;; (defn bad [& [a b]] ...)  ; Invalid - rest must be a symbol
```

## 6. Advanced Patterns

### Conditional Destructuring

Destructuring can be combined with conditionals:

```rtfs
;; Destructure and check conditions
(defn safe-divide [[n d]]
  (if (and d (not (= d 0)))
      (/ n d)
      :division-by-zero))

(safe-divide [10 2])  ; => 5
(safe-divide [10 0])  ; => :division-by-zero
```

### Recursive Destructuring

Complex nested structures:

```rtfs
;; Tree-like structures
(defn tree-sum [[value & children]]
  (+ value
     (if children
         (apply + (map tree-sum children))
         0)))

(tree-sum [1 [2] [3 [4] [5]]])  ; => 15
```

## 7. Error Handling

Destructuring failures produce specific errors:

```rtfs
;; Type mismatch
(let [[a b] "not a vector"] ...)  ; TypeError: expected vector

;; Invalid pattern
(let [[:invalid] [1 2 3]] ...)   ; PatternError: invalid destructuring pattern

;; Missing required keys (if enforced)
(let [{:keys [required]} {}] ...) ; KeyError: missing required key
```

## 8. Implementation Details

### Compilation Strategy

Destructuring is compiled into explicit binding operations:

```rtfs
;; Source: (let [[a b] vec] body)
;; Compiles to:
(let [temp vec]
  (let [a (get temp 0)
        b (get temp 1)]
    body))
```

### Performance Characteristics

- **Vector destructuring**: O(k) where k is number of bindings
- **Map destructuring**: O(k) where k is number of keys looked up
- **Nested destructuring**: Linear in total number of bindings

### Memory Usage

- No additional memory overhead for simple destructuring
- Nested patterns may create intermediate temporary bindings
- Wildcards (`_`) are optimized away during compilation

## 9. Best Practices

### Readability

```rtfs
;; Good: Clear intent
(defn process-point [{:keys [x y] :as point}]
  (validate-point point)
  (calculate-distance x y))

;; Avoid: Overly complex destructuring
(defn complex-fn [[a [b [c d]] {:keys [e f]} & rest]]
  ...)
```

### Error Handling

```rtfs
;; Good: Validate inputs
(defn safe-process [data]
  (if (vector? data)
      (let [[a b c] data]
        (process-triple a b c))
      (error "Expected vector of 3 elements")))
```

### Performance

```rtfs
;; Good: Extract only needed values
(let [{:keys [name]} user]  ; Only extract name
  (println name))

;; Avoid: Extract everything then ignore
(let [{:keys [name age email] :as user} user]  ; Redundant :as
  (println name))
```

## 10. Integration with Other Features

### With Macros

Destructuring works seamlessly with macro-generated code:

```rtfs
(defmacro with-config [bindings & body]
  `(let [{:keys ~bindings} (load-config)]
     ~@body))

(with-config [host port]
  (connect host port))
```

### With Type System

Type annotations work with destructuring:

```rtfs
(defn process {:type {:args [Integer {:name String :value Integer}] :return String}}
  [id {:keys [name value]}]
  (str "Item " id ": " name " = " value))
```

This comprehensive destructuring system enables concise, readable code while maintaining RTFS's functional purity and safety guarantees.