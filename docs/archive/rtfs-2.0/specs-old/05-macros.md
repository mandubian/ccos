# RTFS 2.0: Macros

## 1. What are Macros?

Macros are a powerful feature of RTFS that enable **syntactic abstraction**. They are functions that run at **compile time**, transforming RTFS code (data) into different RTFS code before it is evaluated.

This allows you to create new language constructs, simplify repetitive code patterns, and design more expressive APIs without changing the core RTFS evaluator.

**Key Idea**: Macros are functions that take code as input and return new code as output.

## 2. The Problem Macros Solve

Consider a common pattern: `if (not condition) ...`. You might want to create a more readable `(unless ...)` form.

Without macros, you would have to define a function:

```rtfs
(def unless-fn (fn (condition then-expr)
  (if (not condition) then-expr nil)))

;; This does NOT work as expected!
(unless-fn (= 2 2) (call :fs.delete "/"))
```

The problem is that `unless-fn` is a regular function, so its arguments are evaluated *before* the function is called. In the example above, `(call :fs.delete "/")` is executed regardless of the condition, which is disastrous.

Macros solve this by transforming the code *before* evaluation.

## 3. Defining and Using Macros

Macros are defined using the `defmacro` special form.

**Syntax**: `(defmacro macro-name (param1 param2 ...) body...)`

The `body` of the macro should return a new s-expression, which will then be evaluated in place of the original macro call.

### Example: Implementing `unless`

```rtfs
(defmacro unless (condition then-expr)
  ;; The body of the macro returns a new piece of code.
  ;; We use 'quote' (or the '`' reader macro) to construct the code.
  `(if (not ,condition) ,then-expr nil))
```

Let's break down how this works when you call it:

1.  **Code**: `(unless (= 2 2) (call :fs.delete "/"))`
2.  **Compile Time**: The compiler sees `unless`, which is a macro. It calls the `unless` macro function.
    -   The `condition` parameter is bound to the *unevaluated* code `(= 2 2)`.
    -   The `then-expr` parameter is bound to the *unevaluated* code `(call :fs.delete "/")`.
3.  **Macro Expansion**: The macro's body is executed. It constructs a new list:
    -   `(if (not (= 2 2)) (call :fs.delete "/") nil)`
4.  **Evaluation**: The compiler replaces the original `(unless ...)` form with the new, expanded code. The evaluator then runs this new `if` expression. Since `(not (= 2 2))` is false, the dangerous `delete` call is never evaluated.

## 4. Quoting and Splicing: Building Code

Constructing code inside a macro is so common that RTFS provides special syntax to make it easier.

### `quote` (or `'`)

`quote` prevents evaluation, returning the form as data.

```rtfs
(quote (a b c))  ;; returns the list (a b c)
'(a b c)         ;; equivalent to above
```

### `quasiquote` (or `` ` ``)

`quasiquote` (also known as "syntax quote") is like `quote`, but it allows you to selectively evaluate and insert parts of the expression. It's the most common tool for writing macros.

-   Within a `quasiquote`d expression, `unquote` (or `,`) evaluates a single expression and inserts its result.
-   `unquote-splicing` (or `,@`) evaluates an expression that must result in a list, and inserts the elements of that list directly into the surrounding list.

### Example: A Simple Macro

Let's create a macro `when` that executes code only when a condition is true.

**Desired behavior**:

```rtfs
;; We want this:
(when (= 2 2) (println "Math works!"))

;; To expand into this:
(if (= 2 2) (println "Math works!") nil)
```

**Implementation**:

```rtfs
(defmacro when (condition & body)
  ;; The body of the macro returns a new piece of code.
  ;; We use 'quasiquote' to construct the code.
  `(if ,condition (do ,@body) nil))
```

This demonstrates the power of `quasiquote`, `unquote`, and `unquote-splicing` to precisely construct new code.

## 5. Macros and the Host Boundary

Macros are a key tool for creating user-friendly interfaces for Host capabilities.

A Host might provide a set of low-level, verbose capabilities for interacting with a database:

-   `:db.connect`
-   `:db.prepare-statement`
-   `:db.bind-param`
-   `:db.execute`
-   `:db.close`

Writing this sequence manually would be tedious and error-prone. A macro `(with-db-tx ...)` could be written to abstract this entire sequence away.

```rtfs
;; User-friendly macro
(with-db-tx [conn {:host "localhost"}]
  (query conn "SELECT * FROM users WHERE id = ?" 1))

;; Expands at compile time into a series of (call ...) forms
(let [conn (call :db.connect {:host "localhost"})]
  (do
    (let [stmt (call :db.prepare-statement conn "SELECT * FROM users WHERE id = ?")]
      (call :db.bind-param stmt 1)
      (call :db.execute stmt))
    (call :db.close conn)))
```

This keeps the RTFS core language pure and minimal. The complexity is handled by compile-time code generation, not by adding new features to the evaluator. The runtime engine only ever sees the final, expanded sequence of `(call ...)` forms, which it yields to the Host as usual.

## 6. Macros vs. Lazy Evaluation: A Deliberate Choice

Macros provide control over *when* and *if* code is evaluated. Another paradigm for this is **lazy evaluation**, a runtime strategy where expressions are only computed when their results are actually needed. While powerful, laziness is deliberately excluded from RTFS in favor of an **eager (or strict) evaluation** model, with macros as the sole tool for syntactic control.

This choice is fundamental to RTFS's role in an AI orchestration system.

### The Problem with Laziness: Unpredictable Side Effects

In a lazy language, it is extremely difficult to reason about when a side effect will occur. Consider this hypothetical lazy code:

```rtfs
;; In a lazy system, when does the file actually get opened?
(let [file (call :fs.open "/data.txt")]
  ...
  ;; The call might only execute here, when 'file' is first used.
  (process-file file))
```

This "time bomb" effect is unacceptable for a system like CCOS, which must govern and audit actions in a predictable, sequential order. The RTFS guarantee is that a `(call ...)` form yields to the Host *at the moment it is evaluated*. Eager evaluation provides this essential predictability.

### Why Eager Evaluation + Macros is the Right Model for RTFS

1.  **Predictable Governance**: With eager evaluation, the sequence of host calls is explicit and follows the code's structure. This allows the Host to reliably audit, approve, or deny actions in real-time.
2.  **Simpler Runtime**: An eager evaluator is significantly simpler, more performant, and easier to verify than a lazy one that must manage a complex graph of unevaluated "thunks."
3.  **Clear Performance Model**: Laziness can lead to unexpected performance issues and memory consumption ("space leaks"). An eager model has a straightforward performance profile that is easier for developers and AI models to reason about.

Macros give RTFS the best of both worlds: the ability to create sophisticated syntactic abstractions and control evaluation, without sacrificing the core predictability of an eager runtime.
