# RTFS Language Motivation

This document describes the rationale behind the design choices and goals for the **Reasoning Task Flow Specification (RTFS)**.

## 1. Purpose and Primary Use Case

RTFS is a **specialized, data-centric language** designed for **AI systems** to represent, plan, and execute tasks derived from human instructions. It is not a general-purpose programming language, but a structured, symbolic, and executable format for AI-driven work.

**Core Use Case:**
An AI translates a human instruction into a structured RTFS **`task`**. The `task` is the central artifact of RTFS, encapsulating the entire lifecycle of fulfilling the instruction. A `task` typically includes:

- **`:id`**: Unique identifier for the task.
- **`:source`**: Origin of the task (e.g., "human-instruction").
- **`:natural-language`**: The original user request (optional).
- **`:intent`**: A structured, semantic representation of the goal (the "what" and "why").
- **`:plan`**: An executable sequence of RTFS expressions (the "how"), including logic, control flow, and tool/resource calls.
- **`:execution-log`**: An immutable, append-only record of planning and execution steps, supporting traceability, parallelism, and error handling.

RTFS is designed for AI to structure its understanding, planning, and execution of user requests within a single, comprehensive `task` object.

## 2. Key Concepts

- **Task as Artifact**: The `task` is a self-contained, serializable, and inspectable object representing the full lifecycle of an AI-driven work unit.
- **Intent and Plan**: RTFS separates the semantic goal (`:intent`) from the executable plan (`:plan`), supporting both reasoning and action.
- **Execution Log**: The `:execution-log` tracks all planning and execution events, enabling traceability, debugging, and parallel/concurrent workflows.
- **Parallelism and Join**: RTFS natively supports parallel execution and synchronization (e.g., `(parallel ...)`, `(join ...)`), as well as explicit logging of steps.
- **Tool and Resource Integration**: RTFS plans can call external tools, APIs, and manage resources (files, tensors, etc.) in a controlled, explicit way.
- **Data-Driven, Symbolic, and Functional**: RTFS leverages Clojure’s strengths for symbolic computation, data manipulation, and functional programming.

## 3. Architectural Choices

- **Implementation Language (Clojure):**
  - Chosen for its Lisp heritage (S-expressions), dynamic development (REPL), functional focus, and seamless Java interop.
  - Clojure’s data structures and symbolic processing are ideal for AI-centric code generation and manipulation.
- **Core Representation (AST):**
  - The RTFS AST is defined using Clojure records and data structures (`ast.clj`), making it easy to generate, analyze, and transform tasks and plans.
- **Interpreter-First Model:**
  - RTFS is interpreted (see `parser.clj`, `core.clj`), enabling rapid iteration and experimentation. Compilation is a possible future direction.
- **Specification and Validation:**
  - Clojure’s `spec` or `malli` libraries are used to define and validate the structure of tasks, plans, and log entries, ensuring robustness and safety, especially for AI-generated code.

## 4. Language Design Philosophy

- **Lisp-Family Syntax (S-expressions):**
  - RTFS uses uniform, homoiconic S-expressions, making code and data interchangeable and easy for AI to generate and manipulate.
- **Functional Core with Specification:**
  - Immutability by default; side effects are explicit and controlled.
  - Dynamic typing is augmented by runtime validation/specification for reliability.
- **Explicit Side Effects and Resource Management:**
  - All tool/resource calls and mutations are explicit in the plan, supporting reasoning, debugging, and reproducibility.
- **Parallelism and Logging:**
  - Native constructs for parallel execution, joining, and step-level logging, supporting advanced workflows and traceability.
- **Pragmatism and Extensibility:**
  - RTFS balances theoretical soundness with practical needs for AI code generation, leveraging Clojure’s macro system and extensibility for future growth.

RTFS is a language for **reasoning, planning, and executing AI-driven tasks**, designed to be robust, inspectable, and extensible, with Clojure as its foundation.
