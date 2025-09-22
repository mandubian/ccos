# RTFS 2.0: The Standard Library

The RTFS standard library consists of a set of pure, built-in functions that operate on RTFS data structures. These functions do not require a Host and are guaranteed to be available in any compliant RTFS environment. They form the core toolkit for data manipulation.

## 1. Mathematical Operations

These functions operate on `Integer` types.

-   `(+ a b ...)`: Returns the sum of all arguments.
-   `(- a b ...)`: If one argument, negates it. Otherwise, subtracts subsequent arguments from the first.
-   `(* a b ...)`: Returns the product of all arguments.
-   `(/ a b)`: Returns the integer division of `a` by `b`.
-   `(% a b)`: Returns the remainder of `a` divided by `b`.

**Example:**

```rtfs
(+ 10 20 5)   ;; => 35
(- 100 10)    ;; => 90
(- 50)        ;; => -50
(* 2 3 4)     ;; => 24
(/ 10 3)      ;; => 3
(% 10 3)      ;; => 1
```

## 2. Comparison and Equality

These functions perform comparisons and return a `Boolean` (`true` or `false`).

-   `(= a b)`: Returns `true` if `a` and `b` are structurally equal, `false` otherwise. Works on all types.
-   `(> a b)`: Returns `true` if `a` is greater than `b`. (Numeric)
-   `(< a b)`: Returns `true` if `a` is less than `b`. (Numeric)
-   `(>= a b)`: Returns `true` if `a` is greater than or equal to `b`. (Numeric)
-   `(<= a b)`: Returns `true` if `a` is less than or equal to `b`. (Numeric)
-   `(not x)`: Returns `true` if `x` is `false` or `nil`, `false` otherwise.

**Example:**

```rtfs
(= 1 1)              ;; => true
(= "hello" "hello")  ;; => true
(= [1 2] [1 2])      ;; => true
(= [1 2] [1 3])      ;; => false

(> 10 5)             ;; => true
(< 10 5)             ;; => false
(not true)           ;; => false
(not nil)            ;; => true
```

## 3. String Manipulation

-   `(str a b ...)`: Concatenates all arguments into a single string. Non-string arguments are converted to their string representation.
-   `(len s)`: Returns the length of a string `s`.

**Example:**

```rtfs
(str "Hello, " "world!") ;; => "Hello, world!"
(str "Value: " 42)       ;; => "Value: 42"
(len "abcde")            ;; => 5
```

## 4. List and Vector Manipulation

These functions are fundamental for working with ordered collections. Most of these functions work on both lists and vectors.

-   `(list a b ...)`: Creates a new list containing the arguments.
-   `(vector a b ...)`: Creates a new vector containing the arguments.
-   `(len coll)`: Returns the number of elements in a list or vector.
-   `(first coll)`: Returns the first element of a list or vector. Returns `nil` if the collection is empty.
-   `(rest coll)`: Returns a new list or vector containing all but the first element. Returns an empty collection if the input is empty or has one element.
-   `(cons item coll)`: "Constructs" a new list by adding `item` to the front of `coll`.
-   `(concat coll1 coll2 ...)`: Concatenates multiple lists or vectors into a single new list.
-   `(map f coll)`: Applies a function `f` to each element of `coll` and returns a new list of the results.
-   `(filter f coll)`: Returns a new list containing only the elements of `coll` for which `(f element)` returns a truthy value (not `false` or `nil`).

**Example:**

```rtfs
(len [10 20 30])      ;; => 3
(first '(a b c))     ;; => 'a
(rest '(a b c))      ;; => '(b c)
(cons 'a '(b c))     ;; => '(a b c)
(concat [1 2] [3 4]) ;; => '(1 2 3 4)

(def my-list '(1 2 3))
(map (fn (x) (* x 10)) my-list) ;; => '(10 20 30)
(filter (fn (x) (= 0 (% x 2))) my-list) ;; => '(2)
```

## 5. Map Manipulation

-   `(map key1 val1 key2 val2 ...)`: Creates a new map.
-   `(get m key)`: Retrieves the value associated with `key` in map `m`. Returns `nil` if the key is not found.
-   `(assoc m key val)`: "Associates" a key-value pair. Returns a *new* map with the new key-value pair added or updated.
-   `(dissoc m key)`: "Dissociates" a key. Returns a *new* map with the key removed.
-   `(keys m)`: Returns a list of the keys in map `m`.
-   `(vals m)`: Returns a list of the values in map `m`.

**Example:**

```rtfs
(def my-map {:name "Eve" :level 5})

(get my-map :name)         ;; => "Eve"
(get my-map :inventory)    ;; => nil

(assoc my-map :level 6)    ;; => {:name "Eve" :level 6}
(dissoc my-map :name)      ;; => {:level 5}

(keys my-map)              ;; => '(:name :level)
(vals my-map)              ;; => '("Eve" 5)
```

## 6. Type Predicates

-   `(list? x)`: Returns `true` if `x` is a list.
-   `(vector? x)`: Returns `true` if `x` is a vector.
-   `(map? x)`: Returns `true` if `x` is a map.
-   `(symbol? x)`: Returns `true` if `x` is a symbol.
-   `(keyword? x)`: Returns `true` if `x` is a keyword.
-   `(string? x)`: Returns `true` if `x` is a string.
-   `(number? x)`: Returns `true` if `x` is a number.
-   `(boolean? x)`: Returns `true` if `x` is a boolean.
-   `(nil? x)`: Returns `true` if `x` is `nil`.
-   `(fn? x)`: Returns `true` if `x` is a function.
