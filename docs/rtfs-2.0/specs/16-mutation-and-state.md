# RTFS 2.0 — Mutation and State Model

Status: Draft (pending full test coverage)
Last updated: 2025-08-22

This document specifies RTFS’s immutability-by-default model and the only sanctioned ways to introduce mutability: lexical rebinding via `set!` and shared mutable references via Atoms (`atom`, `deref`, `reset!`, `swap!`).

## 1. Immutability by Default

- All RTFS values (numbers, strings, keywords, vectors, maps, booleans, nil) are immutable.
- Library functions such as `assoc`, `dissoc`, `conj`, `merge`, `update`, `map`, `filter`, `reduce` return new values; inputs are never mutated in place.
- Implementations MAY use structural sharing internally; this does not change the observable semantics.

## 2. Environments and Bindings

- `let` creates immutable lexical bindings: a symbol bound to a value snapshot.
- `set!` is a special form that REBINDS an existing symbol in the CURRENT lexical frame.
  - If the symbol exists in the current frame, it is rebound.
  - If it does not exist in the current frame, `set!` creates the binding in that frame (shadowing any outer binding of the same name).
  - `set!` does not edit outer frames. To affect an outer binding from an inner scope, use an Atom (see below).
  - Return value: `nil`.

Example:

```clojure
(let [x 1]
  (do
    (set! x 2) ; rebinds x in this frame
    x)) ; => 2
```

## 3. Closures and Capture Semantics

- Closures capture the VALUES of outer bindings at the time the closure is created (traditional lexical capture).
- Rebinding a symbol with `set!` in an inner scope does not mutate the captured value inside an already-created closure.
- To share evolving state across closures/scopes, use an Atom.

## 4. Mutable References (Atoms)

Atoms provide a shared, mutable reference cell that holds a value. They are the only way to share evolving state across scopes.

### 4.1 Construction and Operations

- `(atom v)` → returns a new Atom containing value `v`.
- `(deref a)` → returns the current value inside atom `a`.
- Reader sugar: `@a` (dereference) is RESERVED for future use. Today, `@` is used for resource/context references (e.g., `@plan-id`). Until reader sugar is introduced in grammar, prefer explicit `(deref a)`.
- `(reset! a v)` → sets the atom `a` to `v`, returns `v`.
- `(swap! a f & args)` → reads current value `cur`, computes `new = f(cur, args...)`, stores `new` in `a`, returns `new`.

### 4.2 Types and Equality

- The Atom itself is a distinct runtime type (e.g., `Value::Atom`).
- Equality:
  - Atom identity equality compares reference identity (two different atoms containing equal values are NOT equal as atoms).
  - To compare contained values, deref first: `(= (deref a1) (deref a2))`.

### 4.3 Error Modes

- `deref`: type error if argument is not an atom.
- `reset!`: type error if first argument is not an atom.
- `swap!`: type error if first arg is not an atom or if `f` is not callable; propagate any error thrown by `f`.

### 4.4 Concurrency Semantics

- Current runtimes evaluate user code single-threaded; `swap!` behaves as a read–modify–write primitive.
- When parallel evaluation is enabled (e.g., `(parallel ...)`), `swap!` MUST be atomic per atom instance. Implementations SHOULD serialize updates on a per-atom basis to avoid lost updates.
- No global transaction semantics are implied by Atoms.

### 4.5 Module-level ("Global") Atoms

- If a runtime supports module-level top forms that establish bindings, an Atom bound at module scope acts as shared state for all functions within that module for the module’s lifetime.
- Such usage is allowed but SHOULD be minimized; prefer passing state explicitly or scoping atoms within a plan step to ease reasoning and testing.
- Module-level atoms follow the same semantics as any atom (deref/reset!/swap!), including atomicity requirements under parallel evaluation.

## 5. Interaction with Control Forms

- `dotimes`/`for` bodies are evaluated in new inner frames per iteration. Variables mutated with `set!` inside a body affect only the current frame unless they were created in the same frame.
- Use Atoms to accumulate results across iterations when a pure reduction is impractical.

Examples:

```clojure
; Accumulate with an Atom
(let [sum (atom 0)]
  (dotimes [i 5]
    (swap! sum + i))
  (deref sum)) ; => 10

; Prefer pure reduce when possible
(reduce + [0 1 2 3 4]) ; => 10
```

## 6. Guidance and Best Practices

- Prefer pure transformations (return new values) when feasible.
- Use `set!` for simple local rebinding in the current frame (e.g., loop counters, scratch bindings).
- Use Atoms for shared, evolving state across closures/scopes, or when later atomicity under parallelism will be required.
- Avoid using Atoms as implicit global state; prefer passing values explicitly or returning new values.

## 7. Audit and Governance Considerations

- Atom operations mutate in-memory runtime state only; they do not perform external side effects. External effects must go through capabilities and/or `(step ...)` to be auditable.
- Plans must not rely on hidden external I/O within `swap!` functions; those must call capabilities instead.

## 8. Examples

### 8.1 Counter captured by a closure

```clojure
(let [c (atom 0)
      inc! (fn [] (swap! c inc))]
  (do (inc!) (inc!) (deref c))) ; => 2
```

### 8.2 Shared map updated across steps

```clojure
(let [state (atom {:hits 0})]
  (dotimes [i 3]
    (swap! state update :hits inc))
  (deref state)) ; => {:hits 3}
```

### 8.3 Local rebinding vs shared reference

```clojure
(let [x 1]
  (do (set! x 2) x)) ; => 2  ; local rebinding

(let [x (atom 1)
      bump (fn [] (swap! x inc))]
  (do (bump) (bump) (deref x))) ; => 3 ; shared reference across calls
```

---

This specification is normative for RTFS 2.0 runtimes. Any runtime providing `set!` or Atom primitives must conform to the semantics described above.
