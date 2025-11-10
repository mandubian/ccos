# RTFS 2.0 Macro System

## Overview

RTFS macros are a hygienic-ish, template-oriented code-transformation facility built on top of quasiquote/unquote/unquote-splicing. Macros allow programs to generate and transform RTFS ASTs before they are evaluated or compiled to IR.

This document describes the runtime- and compiler-visible behavior added in the defmacro/expander evolution:

- `defmacro` for defining macros (supports fixed and variadic parameter lists)
- Quasiquote (`), unquote (~) and unquote-splicing (~@) semantics for templating
- A dedicated top-level expansion pass that runs before AST evaluation and before IR conversion
- A persistent, shared `MacroExpander` registry that is captured by the compiler and injected into runtime evaluators (mandatory at construction)
- Replacement of temporary unquote/quasiquote artifacts before IR conversion so IR never sees macro-templating artifacts

## Macro Definition

Use `defmacro` to define a macro. Macros receive AST nodes as arguments (not evaluated values), and must return an AST node or list of nodes.

```rtfs
;; Simple macro - fixed arity
(defmacro when [condition body]
  `(if ~condition ~body))

;; Variadic macro - tail collects remaining args into a list
(defmacro make-list [& items]
  `(list ~@items))

;; Usage
(when (> x 0)
  (println "positive"))

(make-list 1 2 3) ; -> expands to (list 1 2 3)
```

Notes:
- Macro parameters receive AST fragments. Use quasiquote/unquote to construct new AST fragments inside the macro body.
- Variadic parameter (leading `&`) binds the remaining arguments as an AST list value available for splicing via `~@`.

## Quasiquote, Unquote and Splicing (behavioral details)

Quasiquote, unquote and unquote-splicing work as templating primitives. Key points:

- Backtick (`) creates a template AST (a mostly-quoted structure).
- Unquote (~) evaluates its expression in the macro expansion phase and inserts the resulting AST node in place.
- Unquote-splicing (~@) evaluates to an AST list, and the elements of that list are spliced into the surrounding list.

Examples:

```rtfs
(let [x 42]
  `(println ~x))      ; -> (println 42) as AST

(let [args '(x y z)]
  `(fn [~@args] body)) ; -> (fn [x y z] body)
```

Implementation note (reader/expander interplay): the parser produces AST nodes representing quasiquotes and unquotes; the macro expander resolves these into concrete AST nodes during expansion. Any temporary 'unquote artifact' nodes are removed/replaced before the IR converter runs.

## The Top-level Expansion Pass

To keep macro expansion deterministic and decoupled from runtime evaluation, RTFS performs a dedicated top-level expansion pass early in the compilation/execution pipeline. The canonical flow is:

1. Parse source → AST
2. Top-level expansion: `expand_top_levels(ast)`
   - Walk top-level forms and expand `defmacro` forms into the `MacroExpander` registry
   - Expand macro invocations found in top-level position using the current registry
   - Repeat until a fixed point is reached for top-level expansion
   - Replace quasiquote/unquote artifacts produced during expansion with concrete AST nodes
3. The expansion pass returns the expanded AST plus the `MacroExpander` instance (registry)
4. Type checking / IR conversion runs on the expanded AST (no macro-templating artifacts remain)
5. The compiler collects the `MacroExpander` and injects it into any runtime evaluators created for AST execution

Why top-level expansion?
- Ensures macros that define other macros (or rely on previously defined macros) are visible in the same compilation unit
- Guarantees that IR conversion never sees macro-templating artifacts
- Allows the compiler to capture and persist a MacroExpander instance to be shared with runtime evaluators

## MacroExpander (registry) and runtime integration

RTFS uses a persistent `MacroExpander` registry object:

- Stores macro definitions (`MacroDef`) keyed by symbol
- Supports variadic macro parameters and proper binding of the rest parameter as a list AST
- Provides an API to expand top-level forms and to expand arbitrary AST fragments

Important integration rule: the `MacroExpander` instance is captured by the compiler during top-level expansion and must be injected into any runtime `Evaluator` that will execute AST forms. Injection is mandatory at `Evaluator` construction time to ensure runtime and compile-time macro definitions share the same registry and to prevent divergence between compile-time and runtime expansion behavior.

Example (conceptual):

```rust
// compiler pipeline (concept)
let (expanded_ast, macro_expander) = expand_top_levels(parsed_ast);
// pass expanded_ast into the rest of compilation

// when creating an AST evaluator for running code at runtime:
let eval = Evaluator::new(context, host_iface, module_registry, macro_expander.clone());
```

The concrete APIs in the implementation require the `MacroExpander` to be present at evaluator construction (no optional default). For backwards compatibility the compiler can supply `MacroExpander::default()` when none is otherwise captured, but the injection point is mandatory.

## Hygiene and IR cleanliness

RTFS does not attempt full hygienic macro transformations in this release, but several measures reduce accidental capture:

- The expander replaces temporary unquote/quasiquote artifacts with concrete AST nodes before IR conversion so the IR remains clean and deterministic.
- Macro authors should be conservative with generated symbol names; a future change may introduce explicit hygiene mechanisms if demand requires it.

## Error reporting and diagnostics

- Macro expansion errors are reported with source locations where possible and include the current macro expansion stack to help debugging recursive or faulty expansions.
- If a macro returns invalid AST (type or shape), the IR converter will surface a detailed error pointing to the expanded location.

## Examples

```rtfs
;; Variadic macro example
(defmacro make-pair [& items]
  ;; items is an AST list of arguments; ~@items will splice them
  `(list ~@items))

(make-pair 1 2 3) ; -> expands to (list 1 2 3)

;; Macro that defines another macro (order matters; top-level expansion ensures visibility)
(defmacro def-constant [name value]
  `(def ~name ~value))

(def-constant pi 3.1415)
pi ; -> 3.1415
```

## Migration notes (for integrators)

- The evaluator/runtime API now requires a `MacroExpander` at construction. Update any code that constructs an `Evaluator` to provide the captured or default expander. Example in Rust:

```rust
// before: Evaluator::new(ctx, host, registry)
// after:  Evaluator::new(ctx, host, registry, MacroExpander::default())
```

- Prefer capturing the real `MacroExpander` produced by the compiler's `expand_top_levels` pass and forwarding it to runtime evaluators to keep compile-time and runtime macro state consistent.

---

This section complements the language-level documentation in `02-syntax-and-grammar.md` and the compilation notes in `12-ir-and-compilation.md` (see the "Macro expansion" subsection added there).

<parameter name="filePath">/home/mandubian/workspaces/mandubian/ccos/docs/rtfs-2.0/specs-new/05-macro-system.md