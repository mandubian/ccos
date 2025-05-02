# RTFS - Language Semantics (Draft)

This document describes the intended runtime behavior and evaluation rules for the standalone RTFS language.

## 1. Evaluation Model

*   **Applicative Order Evaluation:** Arguments to functions and tool calls are typically evaluated *before* the function/tool is invoked. The order of argument evaluation (e.g., left-to-right) should be specified.
*   **Special Forms:** Certain keywords (`if`, `let`, `do`, `fn`, `def`, `with-resource`, `try`, `match`, `parallel`) have specific evaluation rules that deviate from standard function application (e.g., `if` only evaluates one branch).
*   **Immutability:** Core data structures (numbers, strings, bools, lists, vectors, maps) are immutable by default. Mutation is handled through explicit resource operations or potentially dedicated state management constructs (TBD).
*   **Environment Model:** Evaluation occurs within nested lexical environments. `let` and `fn` create new scopes. `def` adds bindings to the current scope.
*   **Handling Effects:** While RTFS does not expose generic monadic operators (like `bind`), it provides specific constructs (`do`, `let`, `try/catch`, `match`, `with-resource`, tool calls) to explicitly manage the sequencing and context of computations involving effects such as errors, resource lifecycles, and I/O. This achieves similar goals of structured effect management found in monadic programming patterns.

## 2. Scoping Rules

*   **Lexical Scoping:** Variables are resolved based on the structure of the code (where they are defined), not the dynamic call stack.
*   **Module Scope:** Each module defines a distinct namespace. Top-level `def` and `defn` forms within a `(module ...)` block create bindings within that module's namespace.
*   **`let`:** Creates local bindings visible only within the `let` block's body. Shadowing is allowed (including shadowing module-level or imported bindings).
*   **`fn`:** Creates a closure, capturing the lexical environment where it was defined (including any imported or module-level bindings visible at that point). Parameters form a new scope within the function body.
*   **`def`:** Inside a module, defines a variable within the module's namespace. Inside a task plan or function body, defines a variable within the current lexical scope. Re-definition is allowed and shadows.
*   **`import`:** Makes symbols from another module's namespace accessible in the current scope (either directly or via an alias). See Section 7 for details.
*   **Resolution Order:** When resolving a symbol, the runtime typically checks: 1) Local bindings (innermost `let`/`fn` scope first), 2) Imported symbols (potentially qualified), 3) Module-level definitions (if within a module), 4) Built-in/standard library symbols. (Exact order needs careful definition, especially regarding qualified vs unqualified imports).
*   **Task Context (`@`):** A special mechanism (e.g., `@intent`, `@id`) provides read-only access to the fields of the containing `task` artifact from within the `:plan`.

## 3. Concurrency (`parallel`)

*   The `(parallel [id1 expr1] [id2 expr2] ...)` form initiates the concurrent evaluation of `expr1`, `expr2`, etc.
*   The runtime environment is responsible for managing the underlying concurrency (e.g., threads, async tasks).
*   The `parallel` form blocks until *all* constituent expressions have completed (either successfully or with an error).
*   **Result:** Upon successful completion of all branches, `parallel` returns a map where keys are the binding identifiers (as keywords, e.g., `:id1`) and values are the results of the corresponding expressions.
*   **Error Handling:** If any expression within `parallel` evaluates to an error (e.g., returns `[:error error-map]` or throws an exception), the evaluation of other branches may continue or be cancelled (runtime dependent), but the overall `parallel` form immediately terminates and propagates the *first* error encountered. It does not wait for all branches to finish if an error occurs.

## 4. Error Handling Semantics

*   **Structured Errors:** Errors are represented as structured data (e.g., maps with `:type`, `:message`, `:details`), not just strings. The specific structure should be defined by the type system.
*   **Propagation:** By default, errors (represented by `[:error error-map]` return values or runtime exceptions) propagate up the call stack, unwinding execution unless caught.
*   **`try/catch`:** Allows catching specific error types (based on structure/type field in the error map) or any error originating from runtime issues or propagated from tool calls. Enables recovery or alternative logic. It does *not* catch standard `[:error ...]` return values unless those are explicitly re-thrown as exceptions (which is not the standard flow).
*   **`Result` Types (Convention):** Functions and tool calls are strongly encouraged (and often required by their contracts/signatures) to signal expected failure modes by returning a tagged result, e.g., `[:ok value]` or `[:error error-map]`. The `match` expression provides the primary, structured way to handle these expected alternative outcomes.
*   **User Errors:** User-defined functions should signal errors by returning `[:error error-map]`, not by using an internal `throw`-like mechanism.

### 4.1 Standard Error Map Structure

When an error occurs (either via `[:error error-map]` return or a runtime exception), the `error-map` should conform to a standard structure to facilitate consistent handling:

```acl
;; Proposed Standard Error Map Structure
{
  :type :keyword ;; Required. A namespaced keyword identifying the error category (e.g., :error/network, :error/validation, :error/tool-failed, :error/resource-unavailable).
  :message :string ;; Required. A human-readable description of the error.
  :details :map?   ;; Optional. A map containing additional context-specific information about the error (e.g., invalid value, failed URL, tool name).
  ;; :stacktrace :vector? ;; Optional. A vector of strings or structured entries representing the call stack at the point of error (if available/configured).
}
```
*   The `:type` keyword is crucial for `catch` blocks and `match` expressions to dispatch on specific error kinds.
*   Tools should return errors conforming to this structure when they fail.

## 5. Resource Management Semantics

*   **Handles:** Resources (files, sockets, etc.) are represented by opaque handles with associated types.
*   **`with-resource`:** Guarantees that a resource's cleanup function (associated with its type) is called when execution leaves the block's scope, whether normally or due to an error. It simulates RAII (Resource Acquisition Is Initialization) or Python's `with` statement.
*   **Ownership/Consumption (Linearity Simulation):**
    *   The handle bound by `with-resource` is considered "owned" within the block.
    *   Functions operating on resources may "consume" the handle (invalidating the original binding) and potentially return a new handle representing the next state, preventing use-after-consumption. This requires runtime checks or potentially more advanced static analysis based on type system rules (see `type_system.md`, `resource_management.md`).

## 6. Tool Call Semantics

*   Tool calls (`tool:namespace/action ...`) invoke external functionality provided by the runtime environment.
*   The runtime is responsible for resolving the tool name, checking required capabilities (see `security_model.md`), marshalling arguments, invoking the tool, and marshalling the result (or error) back into an RTFS value.
*   Tools abstract interactions with both local resources (files, processes) and non-local resources (web APIs, databases, remote services). For resources requiring lifecycle management (like database connections), tools typically return handles intended to be managed using `with-resource`. For stateless interactions (like simple API calls), tools might operate directly on data arguments.
*   Tool signatures (expected arguments, return types, potential errors) should be declaratively available to the RTFS type checker/validator, likely via the `:contracts` mechanism.
*   **Tool Signature Discovery:** The mechanism by which the runtime/type-checker discovers tool signatures (required for type checking plans and validating `:capabilities-required`) is crucial but considered **external** to the core language specification. It depends on the specific runtime environment and tooling implementation (e.g., using a central registry, manifest files, or an introspection protocol).

## 7. Module System Semantics

*   **Definition (`module`):**
    *   A `(module full.namespace.name ...)` form defines a compilation unit and establishes a unique namespace based on the provided `full.namespace.name` (a namespaced identifier).
    *   Top-level `def` and `defn` forms within the `module` block create bindings (vars) associated with this namespace.
    *   Module definitions are typically expected to reside in files whose paths correspond to the namespace (e.g., `my/company/utils.rtfs` for `my.company.utils`), although the exact loading mechanism is runtime-dependent.
*   **Namespaces:**
    *   Namespaces prevent naming collisions between different modules.
    *   Symbols defined within a module can be referred to externally using their fully qualified name: `namespace.name/symbol-name` (e.g., `my.math.utils/add`).
*   **Exports (`:exports`):**
    *   The `(:exports [symbol1 symbol2 ...])` option within a `module` definition declares which symbols defined within that module are intended for public use.
    *   By default, if `:exports` is present, only the listed symbols are considered public. If `:exports` is omitted, the runtime might default to exporting all top-level `def`/`defn` symbols or none (needs a defined default behavior - let's default to **exporting all** if `:exports` is omitted for simplicity).
    *   Accessing non-exported symbols from outside the module (even using the fully qualified name) should ideally result in an error during resolution or type checking.
*   **Imports (`import`):**
    *   The `(import full.namespace.name ...)` form makes symbols from another module available.
    *   `import` forms are typically allowed at the top level of a module or task plan, or potentially at the beginning of `let` or `do` blocks.
    *   **Default Import:** `(import my.math.utils)` - Makes the public symbols of `my.math.utils` available using their fully qualified names (e.g., `my.math.utils/add`).
    *   **Alias (`:as`):** `(import my.math.utils (:as m))` - Makes public symbols available using the alias prefix: `m/add`. This is the preferred way to avoid namespace collisions.
    *   **Selective Import (`:only`):** `(import my.math.utils (:only [add]))` - Makes *only* the specified public symbols (`add`) available directly in the current scope *without* qualification. Use with caution to avoid polluting the local namespace.
    *   **Combined:** `(import my.math.utils (:as m :only [PI]))` - Makes `PI` available directly, and all other public symbols via `m/symbol`.
*   **Loading:**
    *   The runtime environment is responsible for locating, loading, compiling (if necessary), and caching module definitions when an `import` statement is encountered or a fully qualified symbol is used.
    *   The exact mechanism (filesystem search paths, pre-compiled caches, network loading) is runtime-dependent.
    *   Circular dependencies between modules should ideally be detected and result in an error during loading.
*   **Scope of Imports:** Imported symbols (whether qualified by alias/namespace or imported directly via `:only`) are added to the lexical scope where the `import` form appears. They are subject to shadowing by local `let` or parameter bindings.

*(Further details on specific expression semantics will be added as the language definition matures.)*
