# ACL Language Specification (Work In Progress)

This document outlines the design and features of the ACL (A Cunning Lisp) language.

## 1. Data Types (`Value`)

ACL supports the following fundamental data types:

*   **`Nil`**: Represents the absence of a value, similar to `null` or `None`. Evaluates to false in boolean contexts. Represented as `nil`.
*   **`Bool`**: Represents boolean values `true` and `false`.
*   **`Int`**: Arbitrary-precision integers (using `BigInt`). Example: `123`, `-456`.
*   **`Float`**: 64-bit floating-point numbers. Example: `3.14`, `-0.5e-10`. Note: Due to the nature of floats, they cannot be used as keys in maps.
*   **`String`**: UTF-8 encoded strings. Example: `"hello world"`.
*   **`Symbol`**: Identifiers used primarily for variable names or symbolic constants. Example: `x`, `my-variable`.
*   **`Keyword`**: Similar to symbols but typically used for named arguments or enumerated values. They evaluate to themselves. Example: `:key`, `:option1`.
*   **`List`**: Ordered, potentially heterogeneous sequence of values, typically used to represent code structure (S-expressions). Example: `'(1 2 "three")`.
*   **`Vector`**: Ordered, potentially heterogeneous sequence of values, optimized for random access. Example: `[1 2 "three"]`.
*   **`Map`**: Associative collection mapping keys to values. Example: `{:name "Alice" :age 30}`.
    *   **Map Keys (`MapKey`)**: Not all `Value` types can be used as map keys. Allowed key types are `Nil`, `Bool`, `Int`, `String`, `Symbol`, and `Keyword`. `Float`, `List`, `Vector`, and `Map` themselves cannot be keys due to hashing/equality constraints.

## 2. Core Expression Forms (`Expr`)

These are the fundamental building blocks for constructing ACL programs:

*   **`Literal(Value)`**: Represents a constant value directly embedded in the code (e.g., `10`, `"hello"`, `true`, `nil`).
*   **`Variable(String)`**: Represents a variable lookup. When evaluated, it returns the value bound to that variable name in the current environment (e.g., `x`).
*   **`Define(String, Box<Expr>)`**: Binds a symbol to the result of an expression in the *current* scope. Typically used for top-level or module-level definitions.
    *   Syntax: `(define <symbol> <value-expression>)`
    *   Example: `(define pi 3.14159)`
*   **`Set(String, Box<Expr>)`**: *(Currently Deferred)* Mutates (changes) the value of an *existing* variable binding. The `!` convention signals mutation/side effects. It's an error to `set!` an undefined variable.
    *   Syntax: `(set! <symbol> <new-value-expression>)`
    *   Example: `(define count 0) (set! count (+ count 1))`
*   **`Let { bindings: Vec<(String, Expr)>, body: Box<Expr> }`**: Creates local variable bindings that are only visible within the `body` expression.
    *   Syntax: `(let ((<sym1> <val1-expr>) (<sym2> <val2-expr>) ...) <body>)`
    *   Example: `(let ((x 1) (y 2)) (+ x y))` ; evaluates to 3
*   **`If { condition: Box<Expr>, then_branch: Box<Expr>, else_branch: Box<Expr> }`**: Conditional evaluation. Evaluates `condition`. If true, evaluates and returns `then_branch`; otherwise, evaluates and returns `else_branch`. ACL requires both branches.
    *   Syntax: `(if <condition-expr> <then-expr> <else-expr>)`
    *   Example: `(if (> x 0) "positive" "non-positive")`
*   **`Do { expressions: Vec<Expr> }`**: Evaluates a sequence of expressions in order. The value of the *last* expression is returned as the result of the `do` block. Useful for sequencing operations with side effects.
    *   Syntax: `(do <expr1> <expr2> ... <last-expr>)`
    *   Example: `(do (print "Calculating...") (+ 1 2))` ; prints "Calculating..." and returns 3
*   **`Lambda { params: Vec<String>, body: Box<Expr> }`**: Creates an anonymous function (closure). Captures the lexical environment where it's defined.
    *   Syntax: `(lambda (<param1> <param2> ...) <body>)`
    *   Example: `(lambda (x) (* x x))` ; a function that squares its argument
*   **`Apply { function: Box<Expr>, arguments: Vec<Expr> }`**: Function application (calling a function). Evaluates `function` to get a function object, evaluates `arguments` to get argument values, then calls the function with the arguments.
    *   Syntax: `(<function-expr> <arg1-expr> <arg2-expr> ...)`
    *   Example: `(define square (lambda (x) (* x x))) (square 5)` ; evaluates to 25
    *   Example: `((lambda (x y) (+ x y)) 3 4)` ; evaluates to 7

*(More features like Cond, Match, Quote, Macros, etc., to be added later.)*
