# RTFS 2.0: Complete Language Features Overview

This document provides a comprehensive overview of all RTFS 2.0 language features, organized by category. RTFS is a pure functional, homoiconic language designed for safe, verifiable computation within the CCOS (Cognitive-Causal Orchestration System) framework.

## 1. Core Language Constructs

### Data Types and Literals
- **Atoms**: Integers, floats, strings, symbols, keywords, booleans, timestamps, UUIDs, resource handles, nil
- **Collections**: Lists `()`, vectors `[]`, maps `{}`
- **Functions**: First-class functions with lexical scoping

### Special Forms (implemented in RTFS 2.0)
- **Definition**: `def`, `defn`, `defstruct` – bind values, functions, and struct-like types to symbols
- **Control Flow**: `if`, `match`, `do`, `try`/`catch`/`finally`, `for`
- **Binding**: `let` – lexical variable binding with destructuring
- **Functions**: `fn` (with `λ` as an alias) – function definition and anonymous functions

## 2. Advanced Pattern Matching and Destructuring

RTFS supports sophisticated pattern matching through destructuring, allowing direct binding of nested data structures.

### Vector Destructuring
```rtfs
;; Basic vector destructuring
(let [[a b c] [1 2 3]]
  (+ a b c))  ; => 6

;; Nested destructuring with rest
(let [[head & tail] [1 2 3 4]]
  [head (count tail)])  ; => [1 3]

;; Function parameters with destructuring
(defn add-coords [[x y]]
  (+ x y))
```

### Map Destructuring
```rtfs
;; Keyword key destructuring
(let [{:keys [name age]} {:name "Alice" :age 30}]
  (str name " is " age))  ; => "Alice is 30"

;; Explicit key binding
(let [{:name n :age a} {:name "Bob" :age 25}]
  [n a])  ; => ["Bob" 25]

;; Mixed destructuring with :as
(let [{:keys [x y] :as point} {:x 10 :y 20 :z 30}]
  [x y point])  ; => [10 20 {:x 10 :y 20 :z 30}]
```

### Wildcard Patterns
```rtfs
;; Ignore values with underscore
(let [[_ important _] [1 42 3]]
  important)  ; => 42
```

## 3. Continuation-Passing and Host Boundary

RTFS implements a unique continuation model where all external effects are mediated through the host environment.

### Host Calls and Continuations
```rtfs
;; Basic host capability call
(call :fs.read "/path/to/file")

;; Continuation with callback
(call :async.compute
  (fn [result] (println "Result:" result)))
```

### Execution Model
- Pure RTFS evaluation produces either a final value or `ExecutionOutcome::RequiresHost`
- Host processes the request and resumes execution with the result
- This creates a secure boundary where RTFS cannot perform side effects directly

### Streaming Capabilities
RTFS supports incremental data processing through host-mediated streaming:

```rtfs
;; Stream processing via host capabilities
(call :stream.process
  {:source :file-stream
   :processor (fn [chunk] (transform chunk))
   :on-complete (fn [result] (finalize result))})
```

## 4. Macro System (design, not yet implemented)

RTFS 2.0 currently **does not implement a user-facing `defmacro` form or quasiquote syntax** in the shipped grammar.
The macro system described in `08-macro-system.md` is a **design target** for future versions and should be treated as aspirational.

- Macros today are effectively handled by CCOS- or compiler-level transformations, not by RTFS code using `defmacro`.
- Examples using `defmacro`, backtick (`` ` ``), `~`, or `~@` are **conceptual** and will not parse in the current RTFS 2.0 implementation.

When generating RTFS code today, prefer **functions and higher-order combinators** instead of relying on macros.

## 5. Module and Namespace System

RTFS supports modular code organization with explicit imports and exports via the `module` special form defined in `rtfs.pest`.

### Module Definition (current syntax)
```rtfs
;; Module declaration
(module my.app/math
  (:exports [add multiply])

  (import my.app/core :as core)

  (defn add [a b] (core/+ a b))
  (defn multiply [a b] (core/* a b)))
```

### Import/Export
- **Exports**: Use `(:exports [sym1 sym2 ...])` inside `module` to declare public symbols.
- **Imports**: Use `(import some.module :as alias)` or `(import some.module :only [sym1 sym2])`.
- Namespace-qualified symbols (e.g., `math/sqrt`) follow the rules in `rtfs.pest` and `12-module-system.md`.

## 6. Type System and Validation

RTFS includes a structural type system for runtime safety.

### Type Annotations
```rtfs
;; Function type annotation
(defn add {:type {:args [Integer Integer] :return Integer}}
  [a b]
  (+ a b))
```

### Runtime Type Checking
- Optional type validation during execution
- Structural typing for maps and collections
- Type-driven dispatch capabilities

## 7. Concurrency and Parallel Processing

RTFS supports structured concurrency through host-mediated parallelism.

### Parallel Execution
```rtfs
;; Parallel processing
(call :parallel.map
  (fn [item] (expensive-computation item))
  [1 2 3 4 5])
```

### Coordination Primitives
- Host-managed thread pools
- Structured concurrency with cancellation
- Deterministic execution ordering

## 8. Error Handling and Recovery

RTFS provides comprehensive error handling with structured error types.

### Error Types
- **RuntimeError**: Evaluation errors, type mismatches
- **HostError**: Capability execution failures
- **ValidationError**: Type and constraint violations

### Error Handling
```rtfs
;; Try-catch style error handling
(try
  (risky-operation)
  (catch RuntimeError e
    (handle-error e)))
```

## 9. Security Model

RTFS enforces security through the host boundary and capability system.

### Capability-Based Security
- All external operations require explicit host capabilities
- Fine-grained permission system
- Audit trail of all side effects

### Sandboxing
- Pure functional core prevents direct system access
- Host-mediated I/O operations
- Memory and execution limits enforced by host

## 10. Performance and Optimization

RTFS includes several performance-oriented design choices.

### Compilation and Evaluation
- **IR (Intermediate Representation)**: Optional compilation target described in `12-ir-and-compilation.md`
- **Tree-walking evaluator**: Simple, direct execution of the AST

### Memory Management
- Immutable data structures reduce copying overhead
- Structural sharing for persistent data
- Host-managed resource pooling for external resources

## 11. Interoperability Features

RTFS is designed for seamless integration with host systems.

### Foreign Function Interface
```rtfs
;; Call host functions
(call :host.invoke
  {:module "math"
   :function "sqrt"
   :args [16.0]})
```

### Data Format Conversion
- Automatic JSON/RTFS conversion
- Protocol buffer integration
- Custom serialization formats

## 12. Development and Debugging Tools

RTFS provides comprehensive tooling for development.

### REPL and Interactive Development
- Interactive evaluation environment
- Incremental compilation
- Hot reloading capabilities

### Debugging Support
- Source map generation
- Stack trace correlation
- Performance profiling hooks

## 13. Advanced Language Features (conceptual / future work)

Some features described in earlier RTFS 2.0 drafts are **not implemented in the current runtime** and should be treated as future design directions:

- **General `eval` on arbitrary RTFS code**
- **User-defined macros via `defmacro` and macro inspection (`macroexpand`)**
- **Clojure-style lazy sequences via `lazy-seq`**

Instead:
- Treat RTFS programs themselves as data structures (lists, maps, symbols) using standard literals.
- Use CCOS streaming and host capabilities (see `09-streaming-capabilities.md`) to model lazy or incremental computation.

## Summary

RTFS 2.0 combines the elegance of Lisp with modern language features, all while maintaining strict purity and security through the host boundary. The language provides:

- **Safety**: Host-mediated effects prevent direct system access
- **Expressiveness**: Macros, destructuring, and advanced patterns
- **Performance**: Optimized compilation and execution
- **Interoperability**: Seamless integration with host environments
- **Verifiability**: Pure functional core enables formal verification

This comprehensive feature set makes RTFS suitable for complex AI agent coordination, secure computation, and verifiable system integration within the CCOS framework.