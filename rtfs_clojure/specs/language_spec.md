# RTFS Language Specification (Clojure-Oriented)

This document outlines the design and features of the Reasoning Task Flow Specification (RTFS) language, as implemented in Clojure.

## 1. The `task` Form: The Central Artifact

RTFS revolves around the concept of a `task`, which represents a complete unit of work derived from a human instruction and processed by an AI system. A `task` is a Clojure map containing several key-value pairs that describe the instruction, its interpretation, the plan for execution, and the history of that execution.

**Standard `task` Structure:**

A typical `task` map includes the following keys (as Clojure keywords):

- `:id` (string): Unique identifier for the task instance.
- `:source` (string or keyword): Information about the origin of the task (e.g., `"human-instruction"`, `:system-generated`).
- `:natural-language` (string, optional): The original human instruction text, if applicable.
- `:intent` (map): A structured representation of the semantic goal derived from the instruction. Its structure is domain-specific.
- `:plan` (RTFS expression, usually a list): An executable RTFS S-expression representing the sequence of steps to achieve the `:intent`. This is composed using the core expression forms defined below.
- `:execution-log` (vector of maps): An immutable, append-only vector of maps, where each map represents a stage in the planning or execution lifecycle of the task. It tracks status, agent actions, timestamps, errors, and potentially intermediate results.

The `task` form is the primary data structure passed between different AI components (e.g., instruction parser, planner, executor) and provides a comprehensive record of the work unit.

## 2. Data Types (`Value`)

RTFS supports the following fundamental data types, all of which are native to Clojure:

- **nil**: Represents the absence of a value. Evaluates as false in boolean contexts.
- **boolean**: `true` or `false`.
- **integer**: Arbitrary-precision integers (e.g., `123`, `-456`).
- **float**: 64-bit floating-point numbers (e.g., `3.14`, `-0.5e-10`).
- **string**: UTF-8 encoded strings (e.g., `"hello world"`).
- **symbol**: Used for variable names or symbolic constants (e.g., `x`, `my-variable`).
- **keyword**: Used for named arguments or enumerated values (e.g., `:key`, `:option1`).
- **list**: Ordered, possibly heterogeneous sequence of values, typically used to represent code structure (S-expressions), e.g., `'(1 2 "three")`.
- **vector**: Ordered, possibly heterogeneous sequence of values, optimized for random access, e.g., `[1 2 "three"]`.
- **map**: Associative collection mapping keys to values, e.g., `{:name "Alice" :age 30}`. Map keys are typically keywords or strings, but can also be symbols or numbers.

## 3. Type System and Validation

RTFS is dynamically typed, following Clojure’s philosophy. However, to ensure robustness and reliability (especially for AI-generated code), RTFS leverages Clojure’s `spec` or `malli` libraries to define and validate the structure of tasks, plans, and log entries at runtime.

- **Type Annotations:** Not required, but specs can be used to describe expected shapes and invariants for data structures and function arguments/returns.
- **Validation:** Specs are used to check the structure of tasks, plans, and log entries before execution or at key boundaries.
- **Error Handling:** Validation errors are reported clearly, supporting debugging and safe AI code generation.

## 4. Core Expression Forms (`Expr`)

These are the fundamental building blocks used primarily within the `:plan` field of a `task` to construct executable logic. All forms are written as Clojure S-expressions.

- **Literals:** Numbers, strings, booleans, keywords, and nil are self-evaluating.
  - Examples: `42`, `"hello"`, `true`, `:done`, `nil`

- **Variable Lookup:** A symbol refers to a variable in the current environment.
  - Example: `x`

- **Definition:** Bind a symbol to a value in the current scope.
  - Example: `(def pi 3.14159)`
  - Example: `(defn add [x y] (+ x y))`

- **Let Binding:** Create local variable bindings.
  - Example: `(let [x 1 y 2] (+ x y))`

- **If Expression:** Conditional evaluation.
  - Example: `(if (> x 0) "positive" "non-positive")`

- **Do Block:** Sequentially evaluate a series of expressions, returning the value of the last.
  - Example: `(do (println "Calculating...") (+ 1 2))`

- **Function Application:** Apply a function to arguments.
  - Example: `(+ 1 2)`
  - Example: `((fn [x y] (+ x y)) 3 4)`

- **Lambda (Anonymous Function):**
  - Example: `(fn [x] (* x x))`

- **Parallel and Join:**
  - Example: `(parallel [id1 expr1] [id2 expr2])` ; run expr1 and expr2 in parallel, binding results to id1 and id2
  - Example: `(join id1 id2)` ; wait for parallel steps to complete

- **Log Step:**
  - Example: `(log-step :id step-id expr)` ; log the execution of expr with the given id

- **Tool/Resource Calls:**
  - Example: `(tool:read-file "foo.txt")`
  - Example: `(tool:run-terminal-command "ls -l")`

## 5. Example Task

```clojure
{:id "gh-fetch-bugs-001"
 :source "human-instruction"
 :natural-language "Fetch the list of open issues labeled 'bug' from the rtfs-compiler repo and save their titles to bug_titles.txt"
 :intent {:action :fetch-filter-save
          :data-source {:type :github-issues :repo "rtfs-compiler" :state :open}
          :filter-criteria {:label "bug"}
          :data-to-extract :title
          :destination {:type :file :path "bug_titles.txt"}}
 :plan (do
         (parallel
           [fetch-A (log-step :id "fetch-A" (tool:fetch-data "source-A"))]
           [fetch-B (log-step :id "fetch-B" (tool:fetch-data "source-B"))])
         (let [join-results (join fetch-A fetch-B)]
           (log-step :id "process"
             (process-results (map-ref join-results fetch-A)
                              (map-ref join-results fetch-B)))))
 :execution-log [{:stage 1 :agent "Formulator-AI" :timestamp "2025-04-21T10:00:00Z" :status :planning-requested :plan (:send-planner {})}]}
```

## 6. Evaluation and Execution

- RTFS tasks are interpreted by a Clojure runtime, which evaluates the `:plan` field as Clojure code, manages the environment, and updates the `:execution-log`.
- All side effects (tool/resource calls, mutations) are explicit in the plan and tracked in the log.
- Validation (via spec/malli) is performed before and during execution to ensure safety and correctness.

## 7. Extensibility

- RTFS can be extended with new core forms, macros, and domain-specific libraries using Clojure’s macro system.
- The language is designed to evolve, supporting new types of plans, resources, and integration patterns as needed for AI-driven workflows.
