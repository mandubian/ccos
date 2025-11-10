# RTFS 2.0 Intermediate Representation (IR) and Compilation

This document describes the RTFS Intermediate Representation (IR), a typed, canonical representation of RTFS programs, and the process of compiling the Abstract Syntax Tree (AST) into this IR.

## 1. Introduction to the IR

The RTFS IR is a low-level, explicit representation of an RTFS program that is designed to be easy for the runtime to interpret. It resolves symbols, makes scopes explicit, and canonicalizes the program structure.

### 1.1. Design Goals

- **Explicitness:** All variable references are resolved, and scopes are clearly defined.
- **Simplicity:** The IR has a small number of node types, making it easy to interpret.
- **Typed:** Every node in the IR has a type, allowing for type checking and optimization.
- **Canonical:** The IR represents the program in a standard, unambiguous way.

## 2. IR Node Structure

The core of the IR is the `IrNode` enum, which represents all the different kinds of nodes in the IR.

### 2.1. `IrNode` Enum

The `IrNode` enum has the following variants:

- **`Program`:** The root of the IR, containing a list of top-level forms.
- **`Literal`:** Represents a literal value, such as an integer, string, or boolean.
- **`VariableBinding`:** Represents a variable binding, with a name, type, and unique ID.
- **`VariableRef`:** Represents a reference to a variable, with the ID of the binding it refers to.
- **`ResourceRef`:** Represents a reference to a resource.
- **`QualifiedSymbolRef`:** Represents a reference to a symbol in another module.
- **`VariableDef`:** Represents a variable definition (`def`).
- **`Apply`:** Represents a function call.
- **`Lambda`:** Represents a function definition (`fn`).
- **`Param`:** Represents a function parameter.
- **`If`:** Represents an `if` expression.
- **`Let`:** Represents a `let` expression.
- **`Do`:** Represents a `do` block.
- **`Match`:** Represents a `match` expression.
- **`TryCatch`:** Represents a `try`/`catch` expression.
- **`Parallel`:** Represents a `parallel` block.
- **`WithResource`:** Represents a `with-resource` block.
- **`LogStep`:** Represents a `log-step` expression.
- **`Step`:** Represents a `step` block.
- **`Module`:** Represents a module definition.
- **`FunctionDef`:** Represents a function definition (`defn`).
- **`Import`:** Represents an `import` statement.
- **`Task`:** Represents a task definition.
- **`DiscoverAgents`:** Represents a `discover-agents` expression.
- **`Vector`:** Represents a vector literal.
- **`Map`:** Represents a map literal.
- **`Destructure`:** Represents a destructuring operation.

### 2.2. `IrType` Enum

The `IrType` enum represents the types of values in the IR:

- **`Int`, `Float`, `String`, `Bool`, `Nil`, `Keyword`, `Symbol`:** Primitive types.
- **`Any`, `Never`:** Top and bottom types.
- **`Vector`, `List`, `Tuple`, `Map`:** Collection types.
- **`Function`:** Function types.
- **`Union`, `Intersection`:** Composite types.
- **`Resource`:** Resource types.
- **`LiteralValue`:** A type that represents a single literal value.
- **`TypeRef`:** A reference to a type alias.

## 3. AST to IR Compilation

The `IrConverter` is responsible for converting the AST into the IR. This process involves several steps:

### 3.1. Scope Management

The `IrConverter` uses a scope stack to manage symbol resolution. When a new scope is entered (e.g., in a `let` or `fn` expression), a new scope is pushed onto the stack. When the scope is exited, the scope is popped from the stack.

### 3.2. Symbol Resolution

When the converter encounters a symbol, it looks it up in the current scope. If the symbol is found, it creates a `VariableRef` node with the ID of the binding. If the symbol is not found, it can either create a dynamic variable reference to be resolved at runtime or, in strict mode, return an `UndefinedSymbol` error.

### 3.3. Type Inference

The converter performs type inference to determine the type of each node in the IR. It uses a `TypeContext` to store type aliases and constraints.

### 3.4. Special Form Conversion

The converter has special handlers for each of the special forms in the RTFS language, such as `if`, `let`, `fn`, and `def`. These handlers are responsible for converting the special form into the appropriate IR nodes.

### 3.5. Built-in Functions

The converter adds the built-in functions to the global scope, so they can be called from anywhere in the program.

## 4. Macro expansion

Before the `IrConverter` runs, RTFS performs a dedicated top-level macro expansion pass. The key properties of this pass are:

- It runs on the AST produced by the parser and expands top-level macro definitions and top-level macro invocations until a fixed point is reached.
- The expansion pass returns both the expanded AST and a `MacroExpander` registry instance that contains the macros discovered during expansion.
- The expander replaces quasiquote/unquote artifacts with concrete AST nodes so the `IrConverter` never sees macro-templating artifacts.
- The compiler captures the `MacroExpander` instance and forwards it to any runtime evaluators that will execute AST fragments, ensuring compile-time and runtime share the same macro registry.

This ordering guarantees the IR is generated from the fully-expanded AST and prevents macro-templating artifacts from leaking into the lower-level representation.
