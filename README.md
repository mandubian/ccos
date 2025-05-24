# RTFS - Core Concepts

This document outlines the fundamental concepts and motivations behind the standalone Reasoning Task Flow Specification (RTFS) language.

## 1. Purpose: A Language for AI Task Execution

RTFS is a specialized language designed for **AI systems** to represent, plan, execute, and track tasks, often derived from human instructions or complex goals. It serves as a structured, verifiable, and portable format for defining and managing AI-driven workflows.

**Key Goals:**
*   **AI-Centric:** Designed for generation, manipulation, and interpretation by AI agents (planners, executors).
*   **Verifiability:** Incorporate features (types, contracts, security) to allow validation and increase trust in AI-generated plans.
*   **Traceability:** Provide a detailed, immutable record of planning and execution for debugging, auditing, and analysis.
*   **Portability:** Define the language independently to allow implementations (interpreters, transpilers) in various environments (Rust, Python, JS, Clojure, etc.).
*   **Expressiveness:** Capture common patterns in AI task execution, including sequential/parallel flow, tool usage, resource management, and error handling.

## 2. The `Task` Artifact

The **`task`** is the central, self-contained artifact in RTFS. It represents a complete unit of work and encapsulates its entire lifecycle. A `task` is typically represented as a structured map or record containing the following standard fields:

*   **`:id`** (`:string`)
    *   A unique identifier for this specific task instance (e.g., a UUID).
*   **`:metadata`** (`:map`)
    *   A map containing information about the task's origin, creation time, involved agents, etc. Common keys might include `:source` (string or keyword indicating origin), `:timestamp` (ISO 8601 string), `:agent-id` (string).
*   **`:intent`** (`:any`, typically `:map`)
    *   A structured, semantic representation of the task's goal (the "what" and "why"). The exact structure is application-dependent but should capture the core objective derived from the source. This guides the planning process.
*   **`:contracts`** (`:map`)
    *   A map defining the task's interface and requirements, typically generated or refined by a planner. Contains the following keys:
        *   **`:input-schema`** (`:map` or type schema): Defines the expected structure and types of input data required by the `:plan`. See `type_system.md`.
        *   **`:output-schema`** (`:map` or type schema): Defines the guaranteed structure and types of the output data upon successful completion of the `:plan`. See `type_system.md`.
        *   **`:capabilities-required`** (`[:vector :map]`): A vector of maps, where each map describes an external capability (tool call, resource access, network access) needed by the `:plan`. See `security_model.md`.
*   **`:plan`** (RTFS expression, e.g., `[:list ...]`)
    *   An executable expression in the RTFS language defining the steps (the "how") to achieve the `:intent`, respecting the `:contracts`. This is the code that the RTFS runtime executes. See `syntax_spec.md`, `language_semantics.md`.
*   **`:execution-trace`** (`[:vector :map]`)
    *   An immutable, append-only vector of maps, where each map represents a significant event (`log-entry`) during the task's lifecycle (creation, planning, step execution, errors, completion). Entries often include cryptographic signatures for authenticity and integrity. See `security_model.md`.

This structured artifact ensures that all necessary information for understanding, validating, executing, and auditing a task is contained within a single unit.

### 2.1. Future Consideration: Task Composability

While the current specification defines a `task` as a self-contained unit of work represented by an artifact, a common requirement in workflow systems is **task composability** – allowing one task to invoke or trigger another, potentially building complex graphs or workflows.

Directly calling a `task` artifact like a function is not supported, as tasks encapsulate more than just executable code (including intent, contracts, history). However, task composability could be achieved in the future through a dedicated standard tool, for example:

*   **`tool:run-task` (Hypothetical):**
    *   **Purpose:** Instruct the RTFS runtime to schedule and execute another task, identified perhaps by its ID or a definition reference.
    *   **Interaction:** This tool would likely interact with the runtime environment rather than executing the sub-task's plan directly within the caller's context.
    *   **Interface:** It would need to handle passing input data (conforming to the sub-task's `:input-schema`), potentially receiving output data (conforming to the sub-task's `:output-schema`), and managing errors.
    *   **Considerations:** Key design aspects would include:
        *   Synchronous vs. Asynchronous execution.
        *   Mapping of input/output data.
        *   Capability delegation or granting for the sub-task.
        *   Error propagation from the sub-task.
        *   Handling potential circular dependencies.

Introducing such a tool would allow building sophisticated workflows by orchestrating multiple task artifacts, while maintaining the core definition of a task as a comprehensive record of work. This remains a potential area for future extension.

## 3. Core Principles

*   **Modules:** Provide a mechanism (`module`, `import`, `export`) for organizing code into reusable units with controlled namespaces and visibility.
*   **Intent/Plan Separation:** Clearly distinguishing the goal (`:intent`) from the execution strategy (`:plan`) allows for flexible planning and replanning.
RTFS aims to be the lingua franca for describing, executing, and tracking complex, verifiable tasks performed by collaborating AI agents.pulate. The `task` itself is a data structure.
*   **Explicit Effects:** Side effects (tool calls, resource interactions, I/O) are explicit constructs within the `:plan`, not hidden. Complex data types like **tensors or ML models** can be represented either as first-class array types with optional dimension information (`[:array :float [100 100 3]]`) for static analysis, or managed as opaque resource handles (`[:resource TensorHandle]`) via resource management constructs, with operations performed by explicit tool calls.
## 4. Implementation Architecture (Conceptual) (including optional array dimensions), contracts, explicit resource management, and security primitives aim to make AI-generated plans safer and more reliable.
*   **Traceability:** The `:execution-trace` provides a detailed, verifiable history of the task's progression.
Bringing RTFS to life requires a toolchain capable of processing and executing `task` artifacts according to the defined specifications. A typical implementation would involve the following core components:
    *   **`match` for Expected Alternatives:** Functions and tools are encouraged to return tagged results (e.g., `[:ok value]` or `[:error error-map]`) to signal *expected* alternative outcomes or standard failure modes. The `match` expression is the primary way to handle these structured, predictable results.
*   **Parser:**catch` for Exceptions:** The `try/catch` construct is intended for handling *unexpected* runtime errors (e.g., division by zero, resource exhaustion) or exceptions propagated from underlying systems or tool integrations that don't conform to the tagged result convention. It handles exceptional control flow rather than standard alternative results.
    *   **Input:** RTFS source text (likely S-expression based).
    *   **Output:** An initial Abstract Syntax Tree (AST) or directly the Intermediate Representation (IR).ed by collaborating AI agents.
    *   **Reference:** Adheres to `grammar_spec.md`.
## 4. Implementation Architecture (Conceptual)
*   **IR Generator (if AST is separate):**
    *   **Input:** Parser-generated AST.in capable of processing and executing `task` artifacts according to the defined specifications. A typical implementation would involve the following core components:
    *   **Output:** The canonical, structured Intermediate Representation (IR).
    *   **Reference:** Produces structures defined in `ir_spec.md`.
    *   **Input:** RTFS source text (likely S-expression based).
*   **Type Checker / Validator:**stract Syntax Tree (AST) or directly the Intermediate Representation (IR).
    *   **Input:** The generated IR.rammar_spec.md`.
    *   **Process:** Traverses the IR, performs type inference, checks explicit type annotations, validates `:contracts` (input/output schemas), and ensures adherence to type rules. May annotate the IR with resolved types.
    *   **Output:** Validated (and potentially type-annotated) IR, or static error reports.
    *   **Reference:** Implements rules from `type_system.md`.
    *   **Output:** The canonical, structured Intermediate Representation (IR).
*   **Runtime / Interpreter:**s structures defined in `ir_spec.md`.
    *   **Input:** Validated, typed IR.
    *   **Process:** Executes the `:plan` according to the evaluation rules. Manages execution state (environments, call stack), handles control flow (`if`, `do`, `match`, `try/catch`), manages resource lifecycles (`with-resource`), orchestrates concurrency (`parallel`), interacts with the Tool Integration Layer, and appends to the `:execution-trace`.
    *   **Output:** Task result (or error) and updated `:execution-trace`.
    *   **Reference:** Follows semantics defined in `language_semantics.md` and `resource_management.md`.es `:contracts` (input/output schemas), and ensures adherence to type rules. May annotate the IR with resolved types.
    *   **Output:** Validated (and potentially type-annotated) IR, or static error reports.
*   **Tool Integration Layer:**ts rules from `type_system.md`.
    *   **Interface:** Provides a mechanism for the Runtime to discover available external tools, retrieve their signatures (for validation by the Type Checker), and invoke them.
    *   **Function:** Handles marshalling data between RTFS types and the tool's expected format, capability checking (via the Security Subsystem), and error translation.
    *   **Input:** Validated, typed IR.
*   **Security Subsystem:**es the `:plan` according to the evaluation rules. Manages execution state (environments, call stack), handles control flow (`if`, `do`, `match`, `try/catch`), manages resource lifecycles (`with-resource`), orchestrates concurrency (`parallel`), interacts with the Tool Integration Layer, and appends to the `:execution-trace`.
    *   **Function:** Works with the Runtime and Tool Integration Layer to check `:capabilities-required` against granted permissions before allowing tool calls or resource access.
    *   **Reference:** Implements the model described in `security_model.md`.nd `resource_management.md`.

*   **(Optional) Compiler / Transpiler:**
    *   **Input:** Validated, typed IR.nism for the Runtime to discover available external tools, retrieve their signatures (for validation by the Type Checker), and invoke them.
    *   **Output:** Code in a target language (e.g., Rust, Python, Clojure, WebAssembly) or machine code.ity checking (via the Security Subsystem), and error translation.
    *   **Function:** Translates RTFS semantics into equivalent constructs in the target environment.
*   **Security Subsystem:**
This modular architecture allows for different implementations (e.g., a Rust-based runtime, a Clojure-based transpiler) while adhering to the common RTFS language specification.ss.
    *   **Reference:** Implements the model described in `security_model.md`.

*   **(Optional) Compiler / Transpiler:**
    *   **Input:** Validated, typed IR.
    *   **Output:** Code in a target language (e.g., Rust, Python, Clojure, WebAssembly) or machine code.
    *   **Function:** Translates RTFS semantics into equivalent constructs in the target environment.

This modular architecture allows for different implementations (e.g., a Rust-based runtime, a Clojure-based transpiler) while adhering to the common RTFS language specification.

## Installation (TBD)

1. Clone the repository:
    ```sh
    git clone https://github.com/yourusername/rtfs-ai.git
    cd rtfs-ai
    ```
2. Build the project (requires [Rust](https://www.rust-lang.org/tools/install)):
    ```sh
    cd rtfs_compiler
    cargo build --release
    ```

## Usage

Run the main binary:
```sh
cargo run --release
```
[Add usage examples or command-line options here.]

## Contributing

Contributions are welcome! Please open issues or submit pull requests for improvements or bug fixes.

## License

This project is licensed under the [Apache License 2.0](LICENSE).