# ACL Language Motivation

This document describes the rationale behind the design choices and goals for the **AI-Centric Language (ACL)**.

## 1. Purpose and Primary Use Case

ACL is **not** a general-purpose programming language. It is a **specialized language** designed primarily for **AI systems** (like GitHub Copilot) to represent and execute tasks derived from human instructions.

**Core Use Case:** An AI translates a human instruction into a structured ACL **`task`**. This `task` representation is the central concept and artifact of ACL. It encapsulates the entire lifecycle of fulfilling the instruction, typically including:

1.  **Context/Instruction:** Metadata about the original request (e.g., natural language text).
2.  **Semantic Intent (`:intent`):** A formal, structured representation of the goal derived from the instruction (the "what" and "why").
3.  **Execution Plan (`:plan`):** A detailed, executable sequence of steps (ACL code) designed to achieve the intent (the "how"). This plan integrates:
    *   Internal ACL logic (data manipulation, control flow).
    *   Calls to external **tools** (APIs, CLIs, IDE functions) via runtime primitives.
    *   Management of complex **resources** (e.g., tensors, files) via opaque handles and controlled operations (planned, potentially via monadic control).
4.  **Execution Log (`:execution-log`):** An immutable record tracking the evolution of the plan and the status of its execution (as detailed in `language_prospections.md`).

**Key Goals:**

*   **AI-Centric Representation:** Provide a practical and efficient format (`task`) for AI systems to generate, manipulate, and understand work units.
*   **Instruction -> Task -> Execution:** Serve as the bridge transforming high-level human instructions into verifiable, executable `task` objects containing both intent and plan.
*   **Robustness & Predictability:** Enable the generation and execution of reliable plans through features like strong typing (planned), a functional core, and explicit management of side effects (tools, resources via monadic control).
*   **Inter-AI Communication:** Offer a potential standard format (`task`) for exchanging structured work units (intent + plan + history) between different AI systems or agents.
*   **Executable Specification:** The ACL `task` itself acts as an executable specification of the work to be done, including its history and status.

ACL is designed *for AI* to structure its understanding, planning, and execution of user requests within a single, comprehensive `task` object.

## 2. Architectural Choices

*   **Implementation Language (Rust):** Chosen for its safety, performance, expressive type system (well-suited for ASTs and type checking), and ecosystem (parsing libraries, potential compiler backends). These contribute to building a reliable foundation for the language, crucial for AI-generated code.
*   **Core Representation (AST):** A well-defined Abstract Syntax Tree (`ast.rs`) is central. This decouples parsing from execution/compilation and provides a structured format that AI can potentially target or analyze.
*   **Initial Execution Model (Interpreter):** Starting with an interpreter (`interpreter.rs`) allows for faster iteration on language semantics. A compiler (`compiler.rs`) is a future goal for performance.

## 3. Language Design Philosophy

*   **Lisp-Family Syntax (S-expressions):** Adopted for its syntactic uniformity and **homoiconicity** (code represented as data). This structure is relatively easy for AI systems to parse, generate, and manipulate programmatically.
*   **Functional Core with Strong Typing:** Emphasizes immutability and uses a strong, static type system (as planned, see `plan.md`) to enhance predictability and robustness, which are crucial for reliable AI code generation.
*   **Controlled Side Effects:** Mechanisms (like an IO Monad, as planned in `plan.md`) will be used to manage side effects (like tool calls or resource interactions) explicitly, making program behavior easier for an AI (and humans) to reason about.
*   **Explicitness:** Conventions (like `!` for mutation, e.g., `set!`) are adopted from Scheme to clarify intent, even in AI-generated code, highlighting operations that modify state.
*   **Pragmatism:** Design choices will balance theoretical purity with the practical needs of creating a usable system for AI generation, potentially simplifying some aspects compared to traditional Lisps if it aids the core goal.
*   **Extensibility:** The design aims for modularity to allow future additions (macros, advanced types, standard library extensions) relevant to AI code generation tasks.
