# ACL Language Motivation

This document describes the rationale behind the design choices and goals for the **AI-Centric Language (ACL)**.

## 1. Purpose and Primary Use Case

ACL is **not** a general-purpose programming language. It is a **specialized language** designed primarily for **AI systems** (like GitHub Copilot) to represent and execute tasks derived from human instructions.

**Core Use Case:** An AI translates a human instruction into a structured ACL **`task`**. This `task` representation is the central concept of ACL and typically includes:

1.  **Context/Instruction:** Metadata about the original request (e.g., natural language text).
2.  **Semantic Intent:** A formal representation of the goal derived from the instruction (the "what" and "why").
3.  **Execution Plan:** A detailed, executable sequence of steps to achieve the intent (the "how"). This plan integrates:
    *   Internal ACL logic (data manipulation, control flow).
    *   Calls to external **tools** (APIs, CLIs, IDE functions) via runtime primitives.
    *   Management of complex **resources** (e.g., tensors, files) via opaque handles and controlled operations.

**Key Goals:**

*   **AI-Centric Representation:** Provide a practical and efficient format for AI systems to generate, manipulate, and understand tasks. The structure prioritizes AI needs over human readability.
*   **Instruction -> Plan -> Execution:** Serve as the bridge transforming high-level human instructions into verifiable, executable plans.
*   **Robustness & Predictability:** Enable the generation and execution of reliable, deterministic plans through features like strong typing (planned), a functional core, and explicit management of side effects (tools, resources).
*   **Inter-AI Communication:** Offer a potential standard format for exchanging structured tasks (intent + plan) between different AI systems.
*   **Executable Specification:** The ACL `task` itself acts as an executable specification of the work to be done.

ACL is designed *for AI* to structure its understanding and execution of user requests.

## 2. Architectural Choices

*   **Implementation Language (Rust):** Chosen for its safety, performance, expressive type system (well-suited for ASTs and type checking), and ecosystem (parsing libraries, potential compiler backends). These contribute to building a reliable foundation for the language.
*   **Core Representation (AST):** A well-defined Abstract Syntax Tree (`ast.rs`) is central. This decouples parsing from execution/compilation and provides a structured format that AI can potentially target or analyze.
*   **Initial Execution Model (Interpreter):** Starting with an interpreter (`interpreter.rs`) allows for faster iteration on language semantics. A compiler (`compiler.rs`) is a future goal for performance.

## 3. Language Design Philosophy

*   **Lisp-Family Syntax (S-expressions):** Adopted for its syntactic uniformity and **homoiconicity** (code represented as data). This structure is relatively easy for AI systems to parse, generate, and manipulate programmatically.
*   **Functional Core with Strong Typing:** Emphasizes immutability and uses a strong, static type system (as planned) to enhance predictability and robustness, which are crucial for reliable AI code generation.
*   **Controlled Side Effects:** Mechanisms (like an IO Monad, as planned) will be used to manage side effects explicitly, making program behavior easier for an AI (and humans) to reason about.
*   **Explicitness:** Conventions (like `!` for mutation, if mutation is eventually allowed in controlled ways) can help clarify intent, even in AI-generated code.
*   **Pragmatism:** Design choices will balance theoretical purity with the practical needs of creating a usable system for AI generation, potentially simplifying some aspects compared to traditional Lisps if it aids the core goal.
*   **Extensibility:** The design aims for modularity to allow future additions (macros, advanced types, standard library extensions) relevant to AI code generation tasks.
