# RTFS 2.0: Evaluation and Scoping

## 1. The Evaluation Model

RTFS uses a simple, recursive evaluation model. When an s-expression is evaluated, the following rules apply:

1.  **Atoms**:
    -   Numbers, strings, booleans, keywords, and `nil` evaluate to themselves.
    -   A **symbol** is treated as a variable lookup. The evaluator searches the current scope for a value bound to that symbol. If found, it returns the value. If not, it's an error.

2.  **Collections**:
    -   Vectors (`[]`) and maps (`{}`) are evaluated by evaluating each of their elements or key-value pairs. The result is a new vector or map containing the evaluated results.

3.  **Lists (`()`)**:
    -   An empty list `()` evaluates to an empty list.
    -   A non-empty list is treated as a **function call** or a **special form**.

### Function Calls vs. Special Forms

-   **Special Forms**: The first element of the list is a symbol that identifies a special form (e.g., `let`, `if`, `fn`). These forms have their own unique evaluation rules and do not follow the standard function call procedure. For example, `if` only evaluates one of its branches.
-   **Function Calls**: If the first element is not a special form, the list is a function call. The evaluation proceeds as follows:
    1.  Evaluate the first element of the list to get the function to be called.
    2.  Evaluate all subsequent elements (the arguments) from left to right.
    3.  Apply the function to the evaluated arguments.

### Example Evaluation

Consider the expression `(+ (* 2 3) 5)`.

1.  The evaluator sees a list, so it treats it as a function call.
2.  It evaluates the first element, `+`, which resolves to the addition function.
3.  It evaluates the second element, `(* 2 3)`:
    -   This is another list (a function call).
    -   It evaluates `*` to get the multiplication function.
    -   It evaluates `2`, which is `2`.
    -   It evaluates `3`, which is `3`.
    -   It applies the multiplication function to `2` and `3`, resulting in `6`.
4.  It evaluates the third element, `5`, which is `5`.
5.  Finally, it applies the addition function to the results `6` and `5`, producing the final result `11`.

## 2. Scoping: Lexical Scoping

RTFS uses **lexical scoping**. This means that the scope of a variable is determined by its location in the source code, not by the runtime call stack. When a function is created, it "closes over" the environment in which it was defined.

This is most clearly demonstrated by the `let` special form and function definitions.

### The `let` Special Form

`let` creates a new lexical scope and binds symbols to values within that scope.

**Syntax**: `(let [binding1 value1 binding2 value2 ...] body...)`

-   The first argument is a vector of symbol-value pairs.
-   The bindings are established within a new, temporary scope.
-   The `body` expressions are evaluated in this new scope.
-   The result of the `let` form is the result of the last expression in the `body`.

```rtfs
(let [x 10 y 20]
  (+ x y)) ;; Evaluates to 30

;; The symbols x and y do not exist outside the let block.
;; (+ x y) -> Error: symbol 'x' not found.
```

Bindings are evaluated sequentially, so a later binding can refer to an earlier one:

```rtfs
(let [x 10
      y (* x 2)] ;; y is bound to 20
  y)
```

### The `def` Special Form

`def` binds a symbol to a value in the **current (usually global) scope**. It is used for defining top-level variables and functions.

**Syntax**: `(def symbol value)`

```rtfs
(def pi 3.14)
(def my-message "hello")
```

### The `fn` Special Form

`fn` creates a function (a closure).

**Syntax**: `(fn (param1 param2 ...) body...)`

-   The first argument is a vector of symbols that will be the function's parameters.
-   The `body` is a sequence of expressions that will be evaluated when the function is called.

When an `fn` is evaluated, it captures the current lexical scope. When the function is later called, a new scope is created for its parameters, but this new scope also has access to the captured scope.

```rtfs
(let [prefix "LOG: "]
  ;; 'logger' is a closure that captures the 'prefix' variable.
  (def logger (fn (message)
                (str prefix message))))

;; Even though 'prefix' is out of scope here, 'logger' still has access to it.
(logger "An event occurred.") ;; Returns "LOG: An event occurred."
```

## 3. The Environment

The "environment" is the data structure that maps symbols to values. It is a chain of scopes.

-   **Global Scope**: The outermost scope, containing all top-level definitions.
-   **Lexical Scopes**: Created by `let` blocks and function calls. Each lexical scope points to its parent scope.

When looking up a symbol, the evaluator checks the current scope first. If the symbol is not found, it checks the parent scope, and so on, up to the global scope. This is how closures work.

## 4. Summary of Core Special Forms

| Special Form | Syntax                               | Description                                                              |
|--------------|--------------------------------------|--------------------------------------------------------------------------|
| `let`        | `(let [bindings] body...)`           | Creates a new lexical scope with local bindings.                         |
| `def`        | `(def symbol value)`                 | Binds a symbol to a value in the current scope.                          |
| `fn`         | `(fn (params) body...)`              | Creates a function (a closure) that captures the current environment.    |
| `if`         | `(if condition then-expr else-expr)` | Evaluates `then-expr` if `condition` is true, otherwise evaluates `else-expr`. |
| `do`         | `(do expr1 expr2 ...)`               | Evaluates a sequence of expressions and returns the result of the last one. |
| `quote`      | `(quote form)` or `'form`            | Prevents evaluation of a form, returning it as data.                     |
