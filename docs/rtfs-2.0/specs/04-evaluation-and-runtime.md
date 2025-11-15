# RTFS 2.0 Evaluation and Runtime

## Evaluation Model

RTFS 2.0 uses a **hybrid compile-time/runtime evaluation model** where:

- **Compile-time**: Macro expansion, type checking, optimization
- **Runtime**: Pure functional evaluation with host yielding for effects

Important detail (macro integration): the compile-time phase includes a dedicated top-level macro expansion pass that runs before IR conversion or runtime evaluation. The compiler captures the `MacroExpander` instance that was populated during top-level expansion and forwards it to any runtime evaluators. Evaluator construction requires a `MacroExpander` to be provided so runtime evaluators share the same macro registry produced at compile-time.

Example (conceptual):

```rust
let (expanded_ast, macro_expander) = expand_top_levels(parsed_ast);
// later, when constructing an evaluator for AST execution
let eval = Evaluator::new(ctx, host_iface, module_registry, macro_expander.clone());
```

## Expression Evaluation

All RTFS constructs are expressions that evaluate to values:

### Atomic Values
Literals and symbols evaluate directly:

```clojure
42        ; => 42
"hello"   ; => "hello"
:keyword  ; => :keyword
x         ; => value bound to x
```

### Collections
Collections evaluate their elements:

```clojure
[1 2 3]           ; => [1 2 3]
{:a 1 :b 2}       ; => {:a 1 :b 2}
(func arg1 arg2)  ; => result of calling func
```

### Special Forms
Special forms have specialized evaluation rules:

```clojure
;; if - conditional evaluation
(if true "yes" "no")  ; => "yes"
(if false "yes" "no") ; => "no"

;; let - sequential binding
(let [x 1
      y (+ x 1)]  ; x is bound before y
  (+ x y))        ; => 3

;; do - sequential evaluation (effects via call)
(do
  (call :ccos.io/println "first")   ; effect via host boundary
  (call :ccos.io/println "second")  ; effect via host boundary
  42)                               ; => 42
```

## Function Application

Functions are first-class values applied via list syntax:

```clojure
;; Direct application
(+ 1 2 3)  ; => 6

;; Higher-order functions
(map (fn [x] (* x 2)) [1 2 3])  ; => [2 4 6]

;; Anonymous functions
((fn [x] (* x x)) 5)  ; => 25
```

## Scoping and Environments

RTFS uses **lexical scoping** with immutable bindings:

### Global Scope
Definitions create global bindings:

```clojure
(def answer 42)  ; global binding
```

### Local Scope
`let` creates nested lexical scopes:

```clojure
(def x 1)

(let [x 2        ; shadows global x
      y (+ x 1)] ; y = 3
  (+ x y))       ; => 5

x                 ; => 1 (global unchanged)
```

### Function Scope
Functions capture their definition environment:

```clojure
(def multiplier 2)

(defn make-multiplier [factor]
  (fn [x] (* x factor multiplier)))

(def double (make-multiplier 2))
(double 5)  ; => 20 (5 * 2 * 2)
```

## Host Boundary and Yielding

RTFS maintains **strict effect homogeneity** for LLM usability. ALL effects, including basic I/O, yield control to the CCOS host through the `call` special form:

### Execution Outcomes

Evaluation returns one of:

- `ExecutionOutcome::Complete(value)` - Pure computation finished
- `ExecutionOutcome::RequiresHost(call)` - ANY effect yields to host via `call`

### Effect Uniformity

For true LLM-native usability, RTFS treats all effects uniformly:

```clojure
;; Pure computation - no host boundary crossing
(+ 1 2 3)  ; => 6

;; ALL effects go through call - completely uniform
(call :ccos.io/println "Hello")    ; I/O effect
(call :ccos.state.kv/get "key")    ; External API effect
(call :ccos.user.ask "Input?")     ; User interaction effect
```

### Design Rationale

**Homogeneity Principle**: An effect is an effect. No exceptions for "basic" vs "external" operations.

- **LLM Predictability**: One uniform mechanism for all effects
- **Security Consistency**: All effects subject to governance
- **Language Simplicity**: No need to distinguish tool functions vs capabilities
- **Auditability**: Complete causal chain for every effect

This architecture prioritizes **LLM usability** and **security uniformity** over syntactic convenience.

## Runtime Strategies

RTFS supports multiple evaluation strategies:

### Tree Walking
Direct AST interpretation with recursive evaluation.

### IR (Intermediate Representation)
Compile to optimized bytecode for better performance.

### Hybrid
IR with fallback to tree walking for dynamic features.

## Error Handling

RTFS provides structured error handling:

```clojure
;; Try-catch for exceptions
(try
  (/ 1 0)
  (catch :division-by-zero e
    "caught division by zero"))

;; Resource management through host
(call :ccos.resource/with-managed-file "data.txt"
  (fn [file] (call :ccos.fs/read-lines file)))
```

## Concurrency Model

RTFS is single-threaded with **host-mediated concurrency**. All concurrent operations are handled through CCOS capabilities:

```clojure
;; Concurrent operations via host
(call :ccos.concurrent/parallel
  [(task1) (task2) (task3)])

;; Async operations via host
(let [future (call :ccos.async/compute-heavy-task data)]
  (do-other-work)
  (call :ccos.async/await future))
```

## Type Checking

RTFS performs **runtime type validation**:

```clojure
;; Type annotations
(defn add [x:int y:int]:int
  (+ x y))

;; Runtime type checking
(validate {:name :string :age :int}
         {:name "Alice" :age 30})
```

## Performance Characteristics

- **Pure functions**: Memoizable, parallelizable
- **Host yielding**: Controlled side effects
- **Structural sharing**: Efficient immutable data
- **Lazy evaluation**: On-demand computation where beneficial

## Runtime Architecture

```
RTFS Program
    ↓
Parser → AST
    ↓
Type Checker → Validated AST
    ↓
Evaluator → ExecutionOutcome
    ↓
Complete(Value) | RequiresHost(HostCall)
                      ↓
                   CCOS Governance
                      ↓
                 Capability Execution
                      ↓
                   Result → RTFS
```

This architecture ensures **security**, **auditability**, and **composability** while maintaining **performance** and **expressiveness**.</content>
<parameter name="filePath">/home/mandubian/workspaces/mandubian/ccos/docs/rtfs-2.0/specs-new/02-evaluation-and-runtime.md