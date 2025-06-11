# AST to IR Conversion Design

This document outlines the design for the process of converting the RTFS Abstract Syntax Tree (AST) into the defined Intermediate Representation (IR).

## Core Idea

The conversion process will be a recursive traversal of the AST. For each AST node type, there will be a corresponding function or logic block that transforms it into one or more IR nodes. This process is often referred to as "lowering" or "compilation to IR."

## Key Stages and Considerations

### 1. Symbol Resolution and Environment
-   **AST:** Symbols are represented as `Symbol(String)`.
-   **IR:** Symbols must be resolved to their definitions (e.g., local variables, function parameters, global definitions). An environment (or symbol table) will be maintained during conversion to track these mappings.
-   **Relevant IR Nodes:** `:ir/variable-binding`, `:ir/global-ref`, `:ir/function-ref`, `:ir/variable-ref`.
-   **Process:**
    -   Binding forms (`let`, `fn` parameters, `defn`) add symbols to the current environment.
    -   When a symbol is encountered in an expression, it's looked up in the environment to determine its nature and create the appropriate IR node.

### 2. Literal Conversion
-   **AST:** `Literal` enum (Integer, Float, String, Boolean, Keyword, Nil).
-   **IR:** Direct mapping to `:ir/literal` with specific types (`:ir/integer`, `:ir/float`, `:ir/string`, `:ir/boolean`, `:ir/keyword`, `:ir/nil`).
-   **Process:** Straightforward conversion.

### 3. Expression Conversion (General)
-   Most AST `Expression` variants will have a corresponding IR form or will be desugared into a combination of IR forms.

### 4. Function Calls
-   **AST:** `Expression::FunctionCall { callee, arguments }`
-   **IR:** `:ir/apply`
-   **Process:**
    -   Convert the `callee` expression to its IR form (resolving to a function reference or an expression evaluating to a function).
    -   Convert each argument expression to its IR form.
    -   Construct an `:ir/apply` node.

### 5. Special Forms Conversion

#### `let` Expressions
-   **AST:** `LetExpr { bindings, body }`
-   **IR:** `:ir/let` containing `:bindings` (a vector of `:ir/binding-pair`) and a `:body` (a vector of IR expressions).
-   **Destructuring:** `LetBinding.pattern` (AST `Pattern::Symbol`, `Pattern::VectorDestructuring`, `Pattern::MapDestructuring`) will be translated into appropriate IR destructuring patterns (`:ir/variable-binding`, `:ir/vector-destructuring-pattern`, `:ir/map-destructuring-pattern`) for the left-hand side of the `:ir/binding-pair`. This might involve creating temporary variables and assignments internally if the IR structure requires it for complex destructuring, though the goal is to map directly to the IR's destructuring capabilities.
-   **Process:**
    1.  Create a new scope in the environment.
    2.  For each `LetBinding` in `bindings`:
        *   Convert the `value` expression to its IR form (RHS of `:ir/binding-pair`).
        *   Convert the `pattern` to its IR form (LHS of `:ir/binding-pair`).
        *   Add the new bindings introduced by the pattern to the current scope.
    3.  Convert the `body` expressions to IR within this new scope.
    4.  Construct the `:ir/let` node.

#### `if` Expressions
-   **AST:** `IfExpr { condition, then_branch, else_branch }`
-   **IR:** `:ir/conditional`
-   **Process:** Convert `condition`, `then_branch`, and `else_branch` (if present) to their IR forms.

#### `do` Expressions
-   **AST:** `DoExpr { expressions }`
-   **IR:** `:ir/progn`
-   **Process:** Convert each expression in `expressions` to its IR form and place them in the `:forms` field of the `:ir/progn` node.

#### `fn` Expressions (Anonymous Functions)
-   **AST:** `FnExpr { params, variadic_param, return_type, body }`
-   **IR:** `:ir/lambda`
-   **Process:**
    1.  Create a new scope for the function body.
    2.  Convert `params` (`ParamDef { pattern, type_annotation }`) to `:ir/param` nodes. Destructuring patterns in parameters will be converted to the appropriate `:ir/*-destructuring-pattern` within the `:ir/param`'s `:binding` field.
    3.  Convert `variadic_param` (if any) to an `:ir/rest-param`.
    4.  Convert `return_type` (if any) to an `:ir/type-annotation` for the `:return-type` field.
    5.  Convert the `body` expressions to IR within the new scope for the `:body` field.
    6.  Identify and list captured free variables (symbols used in the body but not defined as parameters or local to the lambda) from the enclosing environment. These become `:ir/captured-binding`s in the `:captures` field of the `:ir/lambda` node.

#### `defn` Expressions (Named Functions)
-   **AST:** `DefnExpr { name, params, variadic_param, return_type, body }`
-   **IR:** `:ir/function-def`
-   **Process:**
    1.  Convert the function `name` (Symbol) to an `:ir/identifier` for the `:id` field.
    2.  The parameters, variadic parameter, return type, and body are converted into an `:ir/lambda` structure as described for `fn` expressions. This lambda becomes the value of the `:lambda` field.
    3.  The new function definition is added to the global or module environment.

#### `def` Expressions (Global/Module-level Definitions)
-   **AST:** `DefExpr { symbol, type_annotation, value }`
-   **IR:** `:ir/variable-def`
-   **Process:**
    1.  Convert `symbol` to an `:ir/identifier` for the `:id` field.
    2.  Convert `type_annotation` (if any) to an `:ir/type-annotation` for the `:type` field.
    3.  Convert `value` expression to its IR form for the `:init-expr` field.
    4.  The new variable definition is added to the global or module environment.

#### `match` Expressions
-   **AST:** `MatchExpr { expression, clauses }`
-   **IR:** `:ir/match`
-   **Process:**
    *   Convert the `expression` to be matched into IR for the `:value-expr` field.
    *   For each `MatchClause { pattern, guard, body }`:
        *   Convert AST `MatchPattern` nodes to their corresponding `:ir/pattern-*` IR nodes (e.g., `:ir/pattern-literal`, `:ir/pattern-variable`, `:ir/pattern-vector`, `:ir/pattern-map`, `:ir/pattern-as`, `:ir/pattern-type-check`). This forms the `:pattern` field of an `:ir/match-clause`.
        *   Convert the optional `guard` expression to IR for the `:guard-expr` field.
        *   Convert the `body` expression to IR for the `:body-expr` field.
        *   Collect these into an `:ir/match-clause` node.
    *   The list of converted clauses forms the `:clauses` field of the `:ir/match` node.

#### `with-resource` Expressions
-   **AST:** `WithResourceExpr { resource_symbol, resource_type, resource_init, body }`
-   **IR:** `:ir/with-resource`
-   **Process:**
    1.  Convert `resource_init` to IR for the `:init-expr` field.
    2.  Create an `:ir/variable-binding` for `resource_symbol` for the `:binding` field.
    3.  Convert `resource_type` to an `:ir/type-annotation` (this might be part of the `:ir/variable-binding`'s type or a separate field if the IR spec dictates).
    4.  Convert the `body` expressions to IR within a new scope where `resource_symbol` is bound, for the `:body` field.

#### `try-catch` Expressions
-   **AST:** `TryCatchExpr { try_body, catch_clauses, finally_body }`
-   **IR:** `:ir/try-catch`
-   **Process:**
    *   Convert `try_body` to a sequence of IR expressions for the `:try-block` field.
    *   For each `CatchClause { pattern, binding, body }`:
        *   Convert AST `CatchPattern` (Keyword, Type, Symbol) to an appropriate `:ir/catch-pattern` (e.g., `:ir/catch-type-pattern`). This forms the `:error-pattern` field of an `:ir/catch-clause`.
        *   Create an `:ir/variable-binding` for `binding` for the `:binding` field of the `:ir/catch-clause`.
        *   Convert `body` to a sequence of IR expressions for the `:body` field of the `:ir/catch-clause`.
    *   The list of converted clauses forms the `:catch-clauses` field.
    *   Convert optional `finally_body` to a sequence of IR expressions for the `:finally-block` field.

#### `parallel` Expressions
-   **AST:** `ParallelExpr { bindings }`
-   **IR:** `:ir/parallel`
-   **Process:**
    *   For each `ParallelBinding { symbol, type_annotation, expression }`:
        *   Convert `expression` to IR.
        *   Create an `:ir/variable-binding` for `symbol`.
        *   Convert `type_annotation` (if any) to an `:ir/type-annotation` (likely part of the variable binding).
        *   Create an `:ir/parallel-binding` node containing the variable binding and the expression IR.
    *   The list of these forms the `:bindings` field of the `:ir/parallel` node.

#### `log-step` Expressions
-   **AST:** `LogStepExpr { level, values, location }`
-   **IR:** `:ir/log-step`
-   **Process:**
    *   Convert `level` (Keyword) to an `:ir/keyword` literal for the `:level` field.
    *   Convert `values` (Vec<Expression>) to a vector of IR expressions for the `:values` field.
    *   Convert `location` (Option<String>) to an optional `:ir/string` literal for the `:location` field.

### 6. Type Expression Conversion
-   **AST:** `TypeExpr` enum.
-   **IR:** `:ir/type-annotation` containing the specific IR type structure (e.g., `:ir/primitive-type`, `:ir/vector-type`, `:ir/map-type`, `:ir/function-type`, `:ir/union-type`, etc., as defined in `ir_spec.md`).
-   **Process:** This will be a recursive conversion mapping AST type structures to their corresponding IR type structures.

### 7. Top-Level Items
-   **AST:** `TopLevel` enum (`Task`, `Module`, `Expression`).
-   **IR:** The root will be an `:ir/rtfs-program` node, containing a list of `:ir/top-level-form`s. These forms can be `:ir/task-def`, `:ir/module-def`, or other top-level definitions/expressions.

#### `TaskDefinition`
-   **AST:** `TaskDefinition { id, source, timestamp, intent, contracts, plan, execution_trace, metadata }`
-   **IR:** `:ir/task-def`
-   **Process:** Convert each field to its corresponding IR form (literals, expressions, identifiers as per `ir_spec.md`).

#### `ModuleDefinition`
-   **AST:** `ModuleDefinition { name, exports, definitions }`
-   **IR:** `:ir/module-def`
-   **Process:**
    1.  Convert `name` to an `:ir/identifier` for the `:id` field.
    2.  Convert `exports` (Vec<Symbol>) to a list of `:ir/identifier`s for the `:exports` field.
    3.  Convert `definitions` (`ModuleLevelDefinition` which can be `Def`, `Defn`, `Import`) into their corresponding IR definitions (`:ir/variable-def`, `:ir/function-def`, `:ir/import-def`) for the `:definitions` field. These definitions occur within the module's scope.

#### `ImportDefinition`
-   **AST:** `ImportDefinition { module_name, alias, only }`
-   **IR:** `:ir/import-def`
-   **Process:** Convert `module_name`, `alias` (if present), and `only` (if present) symbols to `:ir/identifier`s for the respective fields (`:module-id`, `:alias`, `:imports`).

## Data Structures for Conversion

-   **`IrConverter` struct/class:**
    -   Holds the current environment (symbol table).
    -   Contains methods for each AST node type (e.g., `convert_expression`, `convert_let_expr`, `convert_fn_expr`).
-   **Environment/SymbolTable:**
    -   A stack of scopes (e.g., `Vec<HashMap<String, IrBindingInfo>>`).
    -   `IrBindingInfo` could store information about the binding, such as its IR node type (e.g., variable, function) and its declared type.

## Error Handling

-   The conversion process must detect and report errors, such as:
    -   Undefined symbols.
    -   Type mismatches (if performing basic type checking during this stage).
    -   Invalid AST structures that don't conform to expected patterns.
    -   Failures in converting patterns.
-   Errors should be structured and informative.

## Output

-   The primary output will be a root IR node of type `:ir/rtfs-program`, which contains a `:version` string and a list of `:ir/top-level-form`s, as specified in `ir_spec.md`.

## Example Workflow (Simplified for `let x = 10; x + 5`)

**AST (Conceptual):**
```
LetExpr {
    bindings: [
        LetBinding {
            pattern: Pattern::Symbol(Symbol("x")),
            value: Expression::Literal(Literal::Integer(10))
        }
    ],
    body: [
        Expression::FunctionCall {
            callee: Expression::Symbol(Symbol("+")),
            arguments: [
                Expression::Symbol(Symbol("x")),
                Expression::Literal(Literal::Integer(5))
            ]
        }
    ]
}
```

**Resulting IR (Conceptual, following `ir_spec.md`):**
```clojure
[:ir/let
 {:bindings [[:ir/binding-pair ;; First element of the :bindings vector
              [:ir/variable-binding {:id "x" :type [:ir/primitive-type :ir/integer]}] ;; LHS of pair
              [:ir/literal [:ir/integer 10]] ;; RHS of pair
             ]]
  :body [[:ir/apply ;; First element of the :body vector
          [:ir/function-ref {:id "+"}] ;; Operator
          [[:ir/variable-ref {:id "x"}] ;; Operands list
           [:ir/literal [:ir/integer 5]]]]]}]
```
This IR snippet represents a single expression. If it were a top-level form, it would be wrapped accordingly (e.g., within an `:ir/rtfs-program`).
