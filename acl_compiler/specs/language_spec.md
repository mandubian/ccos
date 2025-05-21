# RTFS Language Specification (Work In Progress)

This document outlines the design and features of the RTFS (Reasoning Task Flow Specification) language.

## 1. The `task` Form: The Central Artifact

RTFS revolves around the concept of a `task`, which represents a complete unit of work derived from a human instruction and processed by an AI system. A `task` is typically represented as an RTFS **Map** containing several key-value pairs that describe the instruction, its interpretation, the plan for execution, and the history of that execution.

**Standard `task` Structure:**

While flexible, a typical `task` map includes the following keys (using Keywords):

*   **`:id` (String):** A unique identifier for the task instance.
*   **`:source` (Value):** Information about the origin of the task (e.g., `"human-instruction"`, `"system-generated"`).
*   **`:natural-language` (String, Optional):** The original human instruction text, if applicable.
*   **`:intent` (Value, typically Map):** A structured representation of the semantic goal derived from the instruction. Its specific structure depends on the domain and the AI's interpretation capabilities. (See `language_prospections.md` for examples).
*   **`:plan` (Expr, typically List/Do):** An executable RTFS expression representing the sequence of steps to achieve the `:intent`. This is composed using the Core Expression Forms defined below.
*   **`:execution-log` (List):** An immutable, append-only list of Maps, where each map represents a stage in the planning or execution lifecycle of the task. It tracks status, agent actions, timestamps, errors, and potentially intermediate results. (See `language_prospections.md` for detailed structure and examples).

The `task` form serves as the primary data structure passed between different AI components (e.g., instruction parser, planner, executor) and provides a comprehensive record of the work unit.

## 2. Data Types (`Value`)

RTFS supports the following fundamental data types:

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

## 3. Type System

RTFS employs a **strong, static type system** designed to enhance predictability and robustness, especially for AI-generated code. Type checking is performed before execution (either at compile-time or pre-interpretation).

*   **Type Syntax:**
    *   **Basic Types:** Represented by symbols: `Int`, `Float`, `Bool`, `String`, `Symbol`, `Keyword`, `Nil`, `Void` (for functions with no return value).
    *   **Composite Types:** Represented using S-expressions:
        *   `List<T>`: `(List T)` (e.g., `(List Int)`)
        *   `Vector<T>`: `(Vector T)` (e.g., `(Vector String)`)
        *   `Map<K, V>`: `(Map K V)` (e.g., `(Map Keyword Bool)`)
        *   `Tuple<T1, T2, ...>`: `(Tuple T1 T2 ...)` (e.g., `(Tuple Int String)`)
    *   **Function Types:** `(-> ArgType1 ArgType2 ... ReturnType)` (e.g., `(-> Int Int Int)`, `(-> String Void)`).
    *   **Algebraic Data Types (ADTs - Planned):** Custom types defined using `deftype` (syntax TBD, see `plan.md`).
*   **Type Annotations:** While type inference is supported where unambiguous, explicit annotations are required for top-level definitions and recommended for clarity.
    *   **Variables:** `(let ([<symbol> : <Type> <value-expr>]) ...)`
    *   **Function Parameters & Return:** `(define (<name> [<param1> : <Type1>] ...) : <ReturnType> <body>)`
*   **Generics (Parametric Polymorphism - Planned):** Type variables (e.g., `T`, `K`, `V`) allow defining functions and types that operate on multiple types (e.g., `(List T)`, `(define (identity [x : T]) : T x)`).
*   **Evaluation Strategy:** RTFS uses **strict (eager) evaluation** by default. Laziness is not a standard feature.

## 4. Core Expression Forms (`Expr`)

These are the fundamental building blocks used primarily within the `:plan` field of a `task` to construct executable logic:

*   **`Literal(Value)`**: Represents a constant value directly embedded in the code (e.g., `10`, `"hello"`, `true`, `nil`).
*   **`Variable(String)`**: Represents a variable lookup. When evaluated, it returns the value bound to that variable name in the current environment (e.g., `x`). The type checker ensures the variable exists and is used according to its type.
*   **`Define(String, Box<Expr>)`**: Binds a symbol to the result of an expression in the *current* scope. Requires a type annotation for the defined symbol.
    *   Syntax: `(define <symbol> : <Type> <value-expression>)` or for functions `(define (<name> [<param> : <Type>]...) : <ReturnType> <body>)`
    *   Example: `(define pi : Float 3.14159)`
    *   Example: `(define (add [x : Int] [y : Int]) : Int (+ x y))`
*   **`Set(String, Box<Expr>)`**: *(Currently Deferred)* Mutates (changes) the value of an *existing* variable binding. Type checker ensures the new value's type matches the variable's declared type.
    *   Syntax: `(set! <symbol> <new-value-expression>)`
    *   Example: `(define count : Int 0) (set! count (+ count 1))`
*   **`Let { bindings: Vec<(String, Expr)>, body: Box<Expr> }`**: Creates local variable bindings. Types can be inferred or explicitly annotated.
    *   Syntax: `(let ((<sym1> <val1-expr>) ; Inferred type
                  (<sym2> : <Type> <val2-expr>) ; Annotated type
                  ...) 
                 <body>)`
    *   Example: `(let ((x 1) (y : Int 2)) (+ x y))` ; evaluates to 3
*   **`If { condition: Box<Expr>, then_branch: Box<Expr>, else_branch: Box<Expr> }`**: Conditional evaluation. Type checker ensures `condition` is `Bool`, and both `then_branch` and `else_branch` have compatible types. The type of the `if` expression is the unified type of the branches.
    *   Syntax: `(if <condition-expr> <then-expr> <else-expr>)`
    *   Example: `(if (> x 0) "positive" "non-positive")` ; Type is String
*   **`Do { expressions: Vec<Expr> }`**: Evaluates a sequence of expressions. Type checker verifies each expression. The type of the `do` block is the type of the *last* expression.
    *   Syntax: `(do <expr1> <expr2> ... <last-expr>)`
    *   Example: `(do (print "Calculating...") (+ 1 2))` ; Type is Int (from `+`)
*   **`Lambda { params: Vec<String>, body: Box<Expr> }`**: Creates an anonymous function (closure). Type annotations on parameters and return type (implicitly or explicitly via context) determine the function's type.
    *   Syntax: `(lambda ([<param1> : <Type1>] ...) : <ReturnType> <body>)` or `(lambda (<param1> <param2> ...) <body>)` where types might be inferred.
    *   Example: `(lambda ([x : Int]) : Int (* x x))` ; Type is `(-> Int Int)`
*   **`Apply { function: Box<Expr>, arguments: Vec<Expr> }`**: Function application. Type checker verifies that `function` evaluates to a function type and that the `arguments`' types match the function's parameter types. The type of the `Apply` expression is the function's return type.
    *   Syntax: `(<function-expr> <arg1-expr> <arg2-expr> ...)`
    *   Example: `(define square : (-> Int Int) (lambda ([x : Int]) (* x x))) (square 5)` ; Type is Int
    *   Example: `((lambda ([x : Int] [y : Int]) : Int (+ x y)) 3 4)` ; Type is Int

*(More features like Cond, Match, Quote, Macros, etc., to be added later.)*
