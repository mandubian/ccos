# RTFS Project - Next Steps

This document outlines the immediate and near-term priorities for the RTFS project based on the current state of specifications and development.

## Key Development Areas:

1.  **Implement the Standard Library (`specs/stdlib_spec.md`)**:
    *   **Task**: Develop and integrate the functions and modules defined in `stdlib_spec.md` into the Rust-based runtime.
    *   **Importance**: Essential for making RTFS practical and reducing verbosity in task definitions.
    *   **Status**: Specification exists; implementation pending.

2.  **Develop the Agent Discovery Registry & Test `(discover-agents ...)` (`specs/agent_discovery.md`)**:
    *   **Task**:
        *   Implement a prototype Agent Discovery Registry (can be simple initially).
        *   Ensure the RTFS runtime's `(discover-agents ...)` special form can correctly query this registry.
        *   Implement the `agent-profile` to `agent_card` conversion logic for agent registration.
    *   **Importance**: Core to enabling dynamic multi-agent interactions.
    *   **Status**: Detailed specification exists; implementation pending.

3.  **Flesh out and Implement Resource Management (`specs/resource_management.md`)**:
    *   **Task**: Implement the resource lifecycle management (e.g., for files, network connections) as defined in `resource_management.md` and ensure constructs like `with-resource` function correctly in the runtime.
    *   **Importance**: Crucial for task safety, predictability, and preventing resource leaks.
    *   **Status**: Specification exists; implementation pending.

4.  **Advance the LLM Training Plan (`specs/rtfs_llm_training_plan.md`)**:
    *   **Task**:
        *   Begin compiling a corpus of RTFS tasks.
        *   Refine the Intermediate Representation (IR) for optimal LLM consumption.
        *   Experiment with fine-tuning or few-shot prompting for RTFS generation.
        *   Update `specs/prompting_guidelines.md` based on findings.
    *   **Importance**: Strategic goal to enable LLMs to natively generate RTFS.
    *   **Status**: Plan exists; execution pending.

5.  **Build More Comprehensive Examples (`specs/examples.md`)**:
    *   **Task**: As the language and runtime mature, create more complex and diverse examples showcasing various RTFS features and use cases.
    *   **Importance**: Aids in testing, provides learning material, and contributes to the LLM training corpus.
    *   **Status**: Initial examples exist; ongoing effort required.

6.  **Implement and Test the Security Model (`specs/security_model.md`)**:
    *   **Task**: Implement the security features outlined, including contract validation, permission checks, and ensuring the cryptographic integrity of the `:execution-trace`.
    *   **Importance**: Paramount for safe and trustworthy execution of AI-generated tasks.
    *   **Status**: Specification exists; implementation pending.

## Documentation & General Tasks:

*   **Complete "TBD" Sections in `README.md`**: Fill in the Installation and Usage sections.
*   **Continuous Specification Refinement**: Review and improve all documents in `specs/` for clarity, completeness, and consistency.
*   **Expand Test Suite**: Develop a comprehensive test suite for the RTFS parser, runtime, and standard library.
