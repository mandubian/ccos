You are the "AI" able to answer my instructions in the most precise and detailed way.

Let's work together to create a new programming language that is aimed at being used by you "AI" to generate code following my human instructions in this language.

This language should be the most practical for you as an AI, not necessarily for me as a human to represent everything you need to respond to my instructions. Thus, the language syntax doesn't need to be readable or practical for me, the human. Keep that in mind.

I need this language to be interpretable and runnable on a classic machine.

So you'll have to write a compiler that can either live interpret code and run it, either generate a static executable. You can rely on an existing language to write this compiler.

You can choose the theory you want to build this this language. But I want the code to be predictible and executable code to be robust and deterministic.

Remember that this language is made for YOU, not for me.

---

## Project Plan: AI-Centric Language (ACL)

This plan outlines the steps to design and implement the AI-Centric Language (ACL) and its execution environment (interpreter/compiler).

**Phase 1: Language Design & Specification**

1.  **Define Core Philosophy & Goals:**
    *   **Primary User:** AI code generation.
    *   **Key Properties:** Predictable, Robust, Deterministic, Interpretable/Compilable.
    *   **Paradigm:** Functional core with strong static typing (chosen for predictability and robustness). Immutable data structures preferred. Explicit handling of side effects.
2.  **Specify Syntax:**
    *   **Format:** S-expressions (Lisp-like). Chosen for ease of generation/parsing by AI and homoiconicity (code as data). Example: `(define (factorial n) (if (= n 0) 1 (* n (factorial (- n 1)))))`
    *   **Detailed Syntax Rules:**
        *   **Atoms:** Basic indivisible elements.
            *   `Int`: Arbitrary-precision integers (e.g., `123`, `-42`).
            *   `Float`: IEEE 754 double-precision floating-point numbers (e.g., `3.14`, `-0.5e-10`).
            *   `Bool`: Boolean values (`#t` for true, `#f` for false).
            *   `String`: Double-quoted UTF-8 strings with standard escape sequences (e.g., `"hello\\nworld"`).
            *   `Symbol`: Identifiers starting with a letter or specific symbol (`+`, `-`, `*`, `/`, `=`, `<`, `>`, `?`, `!`, `_`), followed by letters, numbers, or these symbols (e.g., `my-variable`, `list->vector`, `+`, `internal_symbol?`). Symbols are case-sensitive.
            *   `Keyword`: Symbols prefixed with `:` (e.g., `:key`, `:option`). Often used for named arguments or map keys.
            *   `Nil`: Represents the empty list or a null-like value (`nil`).
        *   **Lists (S-expressions):** Parenthesized sequences of atoms or other lists, separated by whitespace (e.g., `(op arg1 arg2)`, `(define x (+ 1 2))`, `(list 1 "two" #t)`). The first element often represents an operation or function call.
        *   **Vectors:** Fixed-size arrays, denoted with square brackets (e.g., `[1 2 3]`, `["a" "b"]`).
        *   **Maps (Dictionaries/Hashmaps):** Key-value stores, denoted with curly braces and alternating keys and values (e.g., `{ :key1 "value1 :key2 100 }`). Keys are typically keywords or strings.
        *   **Comments:** Single-line comments start with `;` and continue to the end of the line. Block comments are not standard in S-expression syntaxes but could be considered if needed (e.g., `#| ... |#`). For simplicity, we'll stick to `;` comments initially.
        *   **Whitespace:** Spaces, tabs, and newlines are generally ignored except for separating tokens. Commas are treated as whitespace.
        *   **Quoting:** Use `'` (quote) to prevent evaluation of an S-expression, treating it as data (e.g., `'(+ 1 2)` evaluates to the list `(+ 1 2)`, not the number `3`). Backquote (`` ` ``), comma (`,`), and comma-at (`,@`) can be included for quasiquoting/unquoting if metaprogramming becomes advanced.
3.  **Design Type System:**
    *   **Core Principles:** Strong, static typing enforced at compile-time (or before interpretation). Type inference should be supported where possible, but explicit type annotations will be required for top-level definitions (functions, global constants) and potentially in ambiguous cases.
    *   **Type Syntax:** Types will be represented using symbols (e.g., `Int`, `String`) or S-expressions for complex types (e.g., `(List Int)`, `(-> Int Int Bool)`).
    *   **Basic Types:**
        *   `Int`: Arbitrary-precision integers.
        *   `Float`: IEEE 754 double-precision floating-point numbers.
        *   `Bool`: Boolean values (`#t`, `#f`).
        *   `String`: UTF-8 encoded strings.
        *   `Symbol`: Represents symbols themselves (often used in metaprogramming).
        *   `Keyword`: Represents keywords (e.g., `:option`).
        *   `Nil`: The type of the `nil` value (often used as the empty list type or a unit-like type).
        *   `Void`: Represents the absence of a return value (for side-effecting functions).
    *   **Composite Types:**
        *   `List<T>`: Immutable singly-linked list containing elements of type `T`. Syntax: `(List T)`. Example: `(List Int)`.
        *   `Vector<T>`: Immutable fixed-size array containing elements of type `T`. Syntax: `(Vector T)`. Example: `(Vector String)`.
        *   `Map<K, V>`: Immutable hash map with keys of type `K` and values of type `V`. Syntax: `(Map K V)`. Example: `(Map Keyword String)`.
        *   `Tuple<T1, T2, ...>`: Fixed-size, ordered collection of elements with potentially different types. Syntax: `(Tuple T1 T2 ...)`. Example: `(Tuple Int String Bool)`.
    *   **Algebraic Data Types (ADTs):** Allow defining custom structured types.
        *   **Enums (Sum Types):** Define a type that can be one of several variants, potentially carrying data.
            ```acl
            ; Example: Option type
            (deftype (Option T) 
              (Some T)
              None)
            
            ; Example: Result type
            (deftype (Result T E)
              (Ok T)
              (Err E))
            ```
        *   **Structs (Product Types):** Define a type that groups several named fields.
            ```acl
            ; Example: User record
            (deftype User
              (struct 
                [id : Int]
                [name : String]
                [active : Bool]))
            ```
            (Note: S-expression syntax for structs might vary; this is one possibility using named fields within a `struct` variant).
    *   **Function Types:** Describe the types of arguments and the return type of a function.
        *   Syntax: `(-> ArgType1 ArgType2 ... ReturnType)`. Example: `(-> Int Int Int)` for a function taking two `Int`s and returning an `Int`. `(-> String Void)` for a function taking a `String` and returning nothing.
    *   **Generics/Parametric Polymorphism:** Allow writing code that works with multiple types.
        *   Type variables (like `T`, `K`, `V` above) will be used in type definitions (`deftype`) and function signatures (`define`).
        *   The type checker will enforce that generic functions are used consistently.
    *   **Type Annotations:** Explicit type information.
        *   Variable bindings: `(let ([x : Int 10]) ...)`
        *   Function parameters/return types: `(define (add [x : Int] [y : Int]) : Int (+ x y))`
4.  **Define Core Language Constructs:**
    *   **Variable Bindings:**
        *   `define`: Top-level definitions (functions or constants). Must have type annotations.
            ```acl
            (define pi : Float 3.14159)
            (define (add [x : Int] [y : Int]) : Int (+ x y))
            ```
        *   `let`: Local bindings within an expression scope. Type annotations are optional if inferable, but recommended for clarity.
            ```acl
            (let ([x : Int 10]
                  [y 20] ; Type Int inferred
                  [msg : String "Result:"]) 
              (print msg (+ x y)))
            ```
        *   Scoping rules: Lexical scoping.
    *   **Function Definition and Application:**
        *   Definition: Using `define` as shown above. Anonymous functions (lambdas) are also essential.
            ```acl
            (define doubler : (-> Int Int) 
              (lambda ([n : Int]) (* n 2)))
            
            ; Shorthand lambda syntax might be considered later, e.g., #(\* % 2)
            ```
        *   Application: Standard Lisp-style function call `(function arg1 arg2 ...)`. Example: `(add 5 3)`, `(doubler 10)`.
    *   **Conditional Logic:**
        *   `if`: Basic conditional expression.
            ```acl
            (if (< x 0) 
                "negative"
                (if (= x 0) 
                    "zero"
                    "positive"))
            ```
            Requires boolean condition, returns the value of the executed branch. Both branches must have compatible types.
        *   `cond`: Multi-branch conditional (syntactic sugar over nested `if` or implemented directly).
            ```acl
            (cond
              [(< x 0) "negative"]
              [(= x 0) "zero"]
              [else    "positive"]) ; else clause is common
            ```
    *   **Pattern Matching:** Powerful construct for deconstructing ADTs and other data structures.
        *   `match`: Expression to match a value against patterns.
            ```acl
            (define (process-option [opt : (Option Int)]) : String
              (match opt
                [(Some n) (string-append "Got number: " (int->string n))]
                [None     "Got nothing"])) 
            
            (define (eval-expr [expr : Expr]) : Int
              (match expr
                [(Literal n) n]
                [(Add e1 e2) (+ (eval-expr e1) (eval-expr e2))]
                [(Sub e1 e2) (- (eval-expr e1) (eval-expr e2))]))
            ```
        *   Patterns can include literals, variables (binding), wildcards (`_`), and constructors for ADTs, lists, vectors, tuples.
    *   **Core Immutable Data Structures and Operations:**
        *   Lists: `(list item1 item2 ...)`, `cons`, `car`, `cdr`, `null?`, `map`, `filter`, `fold`, etc.
        *   Vectors: `(vector item1 item2 ...)`, `vector-ref`, `vector-length`, `vector->list`, `list->vector`.
        *   Maps: `(hash-map :key1 val1 :key2 val2 ...)`, `hash-ref`, `hash-set` (returns new map), `hash-keys`, `hash-values`.
        *   Strings: `string-append`, `substring`, `string-length`, `string->list`, `list->string`.
        *   Operations must preserve immutability (e.g., `hash-set` returns a *new* map).
    *   **Mechanisms for Controlled Side Effects:**
        *   **IO Monad:** Encapsulate side-effecting operations (like printing, file I/O, network calls) within a specific type, likely `(IO T)`. Pure functions cannot perform I/O directly.
            ```acl
            (define (main) : (IO Void)
              (do [(line : String (read-line))
                   (_      (print (string-append "You entered: " line)))]
                  (pure_io void))) ; `do` notation for monadic sequencing
            ```
        *   The runtime/interpreter executes the `IO` actions sequentially.
        *   Requires defining `bind` (>>=) and `return` (pure) for the IO type, potentially hidden behind `do` syntax.
    *   **Module System:** For organizing code into reusable units and controlling visibility.
        *   `module`: Define a module.
        *   `export`: Specify public definitions.
        *   `import`: Use definitions from other modules.
            ```acl
            ; file: math/ops.acl
            (module math/ops
              (export add subtract pi)
              
              (define pi : Float 3.14159)
              (define (add [x : Int] [y : Int]) : Int (+ x y))
              (define (subtract [x : Int] [y : Int]) : Int (- x y))
              (define (internal-helper [z : Int]) : Int (* z z)) ; Not exported
            )
            
            ; file: main.acl
            (module main
              (import math/ops)
              
              (define (run) : (IO Void)
                (print (math/ops:add 5 (math/ops:subtract 10 3))))
            )
            ```
        *   Needs clear rules for module resolution (e.g., filesystem-based).
    *   **Metaprogramming Capabilities:** Leverage S-expressions (code as data).
        *   `quote` (`'`): Prevent evaluation, return data structure.
        *   `eval`: Evaluate a data structure as code (use with caution, requires type checking at runtime or careful handling).
        *   Macros: Code-generation capabilities at compile time. Define syntax transformations.
            ```acl
            (defmacro (unless [condition body ...])
              `(if (not ,condition)
                 (do ,@body)))
            
            (unless (= x 5) (print "x is not 5"))
            ```
            Requires careful design of the macro system (e.g., hygiene).
    *   **Foreign Function Interface (FFI):** Specification for calling code written in other languages (e.g., C, Rust).
        *   Mechanism to declare external functions and their ACL types.
        *   Specify calling conventions and data marshalling.
            ```acl
            ; Declare an external C function
            (extern c_printf : (-> String ...) Int "printf") 
            
            (c_printf "Hello from C, number %d\n" 123)
            ```
        *   Implementation depends heavily on the compiler backend (e.g., LLVM).
5.  **Specify Standard Library:** Define the core set of built-in functions and modules available to all ACL programs. These should be implemented either as primitives in the runtime (Rust) or potentially in ACL itself once the basics are working.
    *   **`core` Module (implicitly imported):**
        *   **Types:** `Int`, `Float`, `Bool`, `String`, `Symbol`, `Keyword`, `Nil`, `Void`, `List`, `Vector`, `Map`, `Tuple`, `IO`, `Option`, `Result` (built-in or defined via `deftype`).
        *   **Control Flow:** `if`, `cond`, `match`, `let`, `lambda`.
        *   **Definitions:** `define`, `deftype`, `defmacro`.
        *   **Evaluation/Metaprogramming:** `quote`, `eval` (potentially restricted).
        *   **Basic Arithmetic:** `+`, `-`, `*`, `/`, `mod` (for `Int` and `Float`).
        *   **Comparison:** `=`, `<`, `>`, `<=`, `>=` (for numbers, potentially other types).
        *   **Boolean Logic:** `and`, `or`, `not`.
        *   **Type Predicates:** `int?`, `float?`, `bool?`, `string?`, `symbol?`, `keyword?`, `nil?`, `list?`, `vector?`, `map?`, `tuple?`, `function?`.
        *   **Type Conversions:** `int->string`, `string->int`, `float->string`, `string->float`, etc.
    *   **`list` Module:**
        *   `(list ...)` constructor.
        *   `cons : (-> T (List T) (List T))`
        *   `car` / `first : (-> (List T) T)` (Error on empty list)
        *   `cdr` / `rest : (-> (List T) (List T))` (Error on empty list)
        *   `null? : (-> (List T) Bool)`
        *   `length : (-> (List T) Int)`
        *   `append : (-> (List T) (List T) (List T))`
        *   `reverse : (-> (List T) (List T))`
        *   `map : (-> (-> A B) (List A) (List B))`
        *   `filter : (-> (-> T Bool) (List T) (List T))`
        *   `foldl` / `reduce : (-> (-> B A B) B (List A) B)`
        *   `foldr`
        *   `nth : (-> Int (List T) (Option T))`
    *   **`vector` Module:**
        *   `(vector ...)` constructor.
        *   `vector-length : (-> (Vector T) Int)`
        *   `vector-ref : (-> (Vector T) Int (Option T))` (or error on out of bounds)
        *   `vector-set : (-> (Vector T) Int T (Vector T))` (returns new vector)
        *   `vector->list : (-> (Vector T) (List T))`
        *   `list->vector : (-> (List T) (Vector T))`
        *   `vector-map`, `vector-filter`, etc.
    *   **`map` Module:**
        *   `(hash-map ...)` constructor.
        *   `hash-ref : (-> (Map K V) K (Option V))`
        *   `hash-set : (-> (Map K V) K V (Map K V))` (returns new map)
        *   `hash-remove : (-> (Map K V) K (Map K V))` (returns new map)
        *   `hash-keys : (-> (Map K V) (List K))`
        *   `hash-values : (-> (Map K V) (List V))`
        *   `hash-contains? : (-> (Map K V) K Bool)`
        *   `hash-size : (-> (Map K V) Int)`
    *   **`string` Module:**
        *   `string-append : (-> String ... String)`
        *   `string-length : (-> String Int)`
        *   `substring : (-> String Int Int String)` (start index, end index)
        *   `string->list : (-> String (List Char))` (Requires `Char` type)
        *   `list->string : (-> (List Char) String)`
        *   `string-split : (-> String String (List String))` (split by delimiter)
        *   `string-join : (-> (List String) String String)` (join with separator)
        *   `string-upcase`, `string-downcase`
    *   **`io` Module:** (Functions return `(IO T)`) 
        *   `print : (-> Any ... (IO Void))` (Prints representation of arguments)
        *   `println : (-> Any ... (IO Void))` (Prints representation + newline)
        *   `read-line : (-> (IO String))`
        *   `read-file : (-> String (IO String))` (Reads entire file)
        *   `write-file : (-> String String (IO Void))` (Writes string to file)
        *   `(pure_io T)` / `return_io : (-> T (IO T))`
        *   `(bind_io (IO A) (-> A (IO B)) (IO B))` (Internal, used by `do`)
    *   **`math` Module:**
        *   `abs`, `sqrt`, `pow`, `log`, `exp`, `sin`, `cos`, `tan`, etc.
        *   Constants like `pi`, `e`.

**Phase 2: Implementation**

6.  **Choose Implementation Language:**
    *   **Selection:** Rust. Chosen for performance, memory safety, strong typing, excellent tooling, and suitability for compiler development (ecosystem includes libraries like `nom`, `pest`, `cranelift`, `llvm-sys`).
7.  **Set up Project Structure:**
    *   Create a Rust project (`cargo new acl_compiler`).
    *   Define modules (parser, ast, typechecker, interpreter, compiler, etc.).
8.  **Develop Interpreter/Compiler (Iterative):**
    *   **Step 8.1: Lexer/Parser:** Implement S-expression parser (e.g., using `nom` or `pest`). Input: ACL code string. Output: Sequence of tokens or initial parse tree.
    *   **Step 8.2: Abstract Syntax Tree (AST):** Define Rust structs/enums representing the ACL's language constructs. Transform parser output into AST.
    *   **Step 8.3: Semantic Analysis & Type Checking:** Implement the type checker. Traverses the AST, verifies type rules, resolves symbols, checks scopes. Annotates AST with type information or reports errors.
    *   **Step 8.4: Interpreter Backend:** Implement a tree-walking interpreter that directly executes the type-checked AST. Provides immediate execution capabilities.
    *   **Step 8.5 (Optional/Future): Intermediate Representation (IR):** Design an IR (e.g., SSA-based) to facilitate optimizations.
    *   **Step 8.6 (Optional/Future): Compiler Backend:** Implement code generation targeting an existing backend like LLVM IR or Cranelift IR (Rust-native). Input: AST or IR. Output: Target code (e.g., LLVM bitcode, machine code).
    *   **Step 8.7: Implement Standard Library:** Write core functions defined in Phase 1, Step 5, in ACL itself or as built-in primitives in the interpreter/compiler.
    *   **Step 8.8: Build Basic Tooling:** Create a simple REPL (Read-Eval-Print Loop) for interactive use.

**Phase 3: Testing & Refinement**

9.  **Develop Test Suite:**
    *   Unit tests for parser, type checker, interpreter, compiler components.
    *   Integration tests using sample ACL programs.
    *   Property-based testing where applicable.
10. **Refine Language & Implementation:**
    *   Iteratively refine the language design based on implementation challenges and usability testing (simulating AI generation).
    *   Optimize performance of the interpreter/compiler.

**Phase 4: Documentation**

11. **Write Language Reference:** Formal specification of syntax, semantics, type system, and standard library.
12. **Document Compiler/Interpreter Architecture:** Explain the internal workings for maintainability.

---
*Next Step: Start executing Phase 1, Step 1 (already partially done by defining the plan) and move to Step 2 (Formalizing S-expression syntax details).*
