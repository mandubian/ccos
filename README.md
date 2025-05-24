# RTFS - Reasoning Task Flow Specification

## Origin and Purpose

The RTFS (Readable Text Format for Scripts) project originated from the idea of creating a new programming language specifically designed for Artificial Intelligence (AI) to generate code based on human instructions. The core concept is to provide a language that is highly practical for AI to use, rather than prioritizing human readability or traditional programming paradigms.

RTFS is designed to be interpretable and runnable on classical computing machinery. A key objective in its design is to produce robust and deterministic code, ensuring that programs written in RTFS behave predictably and reliably. While humans can understand and write RTFS, its syntax and structure are optimized for AI generation, aiming to represent everything an AI needs to effectively respond to human instructions for code creation.

## How RTFS Was Built

RTFS stands as a testament to the capabilities of modern Artificial Intelligence. It was built exclusively by an AI Large Language Model (LLM). The development process was iterative, with the LLM generating the language specifications, creating a compiler/interpreter, and then refining RTFS based on testing and further instructions. This project showcases how an LLM can handle complex software engineering tasks, from conceptualization to implementation, to create a novel programming language.

## Main Concepts

At the heart of RTFS is the **`Task` artifact**, which serves as the central and self-contained unit of work. Each `Task` encapsulates all information necessary for its execution, management, and analysis.

Key fields within a `Task` artifact include:

*   **`:id`**: A unique string, typically a UUID, identifying the specific task instance.
*   **`:metadata`**: A map for storing auxiliary information such as creation timestamp, authorship, or source of the task.
*   **`:intent`**: A representation of the task's goal, often derived from human instructions, defining the "what" and "why".
*   **`:contracts`**: A map defining the task's operational interface and requirements. This includes:
    *   `:input-schema`: Specifies the expected structure and types of input data.
    *   `:output-schema`: Defines the guaranteed structure and types of output data upon successful completion.
    *   `:capabilities-required`: Lists any external capabilities, such as tool calls or resource access, needed by the plan.
*   **`:plan`**: The sequence of operations or script in RTFS language that, when executed, aims to fulfill the `:intent` while honoring the `:contracts`.
*   **`:execution-trace`**: An immutable, append-only log detailing significant events during the task's lifecycle, including planning stages, step execution details, errors, and final outcomes.

RTFS is built upon several core principles that guide its design and functionality:

*   **Modularity**: RTFS supports organizing code into reusable units with controlled namespaces and visibility through constructs like `module`, `import`, and `export`, promoting cleaner, more maintainable, and reusable task definitions.
*   **Intent/Plan Separation**: The clear distinction between the high-level goal (captured in `:intent`) and the execution strategy (detailed in `:plan`) allows for greater flexibility. This separation enables diverse planning strategies or dynamic replanning in response to new information, without altering the core objective.
*   **Explicit Effects (Contracts)**: RTFS emphasizes making all potential side-effects explicit. This is primarily achieved through the `:contracts` field (especially `:capabilities-required`) and by designing the `:plan` in a way that tool usage and resource interactions are clearly defined language constructs, rather than hidden operations. This design choice significantly enhances predictability, safety, and the verifiability of task execution.
*   **Traceability**: Comprehensive traceability is ensured by the `:execution-trace`, which provides a detailed and verifiable history of the task's entire lifecycle. This is crucial for debugging, auditing, understanding AI behavior, and ensuring accountability.

## Examples of RTFS

The following examples illustrate some key features and syntax of RTFS. RTFS employs an S-expression syntax, akin to Lisp dialects.

### 1. Basic Control Flow & Variables

This example demonstrates fundamental concepts such as variable definition, conditional logic, and sequential execution.

```lisp
(task :id "task-basic-flow"
  :intent {:description "Demonstrate basic control flow"}
  :plan
  (do
    (def initial-value :int 10) ;; Defines a task-scoped variable
    (let [threshold :int 5      ;; Local binding
          processed-value :int (if (> initial-value threshold)
                                 (* initial-value 2) ;; 'then' branch
                                 (+ initial-value 1))] ;; 'else' branch
      ;; Check the processed value
      (if (= processed-value 20)
        (tool:log "Processing successful, value doubled.")
        (tool:log "Processing resulted in increment."))
      ;; Return the final value
      processed-value)))
```

**Explanation:**
*   `def` is used to define a variable (`initial-value`) scoped to the entire task's plan.
*   `let` creates local bindings (`threshold`, `processed-value`) within its scope.
*   `if` provides conditional branching.
*   `do` allows for sequential execution of multiple expressions; the result of the `do` block is the result of its last expression.
*   `tool:log` is an example of a built-in tool call for logging.

### 2. Error Handling with `match`

RTFS promotes robust error handling by encouraging tools and functions to return "result types" – typically tagged tuples like `[:ok data]` for success or `[:error error-details]` for expected failures. The `match` expression is then used to handle these different outcomes gracefully.

```lisp
(task :id "task-match-error"
  :intent {:description "Fetch data and handle potential errors using match"}
  :plan
  (let [fetch-result (tool:fetch-data "http://example.com/data")] ;; This tool returns [:ok ...] or [:error ...]
    (match fetch-result
      ;; Success case: Destructure the :ok vector
      [:ok data]
      (do
        (tool:log "Data fetched successfully.")
        ;; Further processing on 'data'
        (process-data data))

      ;; Specific error case: Destructure the :error vector and the error map
      [:error {:type :error/network :message msg}]
      (do
        (tool:log (str "Network error fetching data: " msg))
        nil) ;; Return nil or a default value

      ;; Catch-all for other errors
      [:error error-info]
      (do
        (tool:log (str "An unexpected error occurred: " (:message error-info)))
        nil)

      ;; Optional: Default case if result structure is unexpected
      _ ;; Wildcard pattern
      (do
        (tool:log "Unexpected result structure from tool:fetch-data")
        nil))))
```

**Explanation:**
*   `tool:fetch-data` is assumed to return a tagged tuple indicating success or failure.
*   `match` elegantly destructures the `fetch-result`:
    *   If it's `[:ok data]`, `data` is bound to the actual fetched content.
    *   If it's `[:error {:type :error/network ...}]`, `msg` is bound to the error message for network errors.
    *   Other `[:error ...]` structures can be caught and handled.
*   This pattern makes expected alternative outcomes explicit and manageable, improving code clarity and reliability compared to relying solely on exceptions for all error conditions.

### 3. Resource Management with `with-resource`

RTFS provides a `with-resource` construct for managing resources that need explicit acquisition and release, such as file handles or network connections. This ensures resources are properly cleaned up, even if errors occur.

```lisp
(task :id "task-with-resource"
  :intent {:description "Read from one file and write to another using with-resource"}
  :contracts {:capabilities-required [ ;; Request capabilities for file access
                 { :type :resource-access :resource-type "FileHandle" :permissions [:read]
                   :constraints {:path [:= "input.txt"]} }
                 { :type :resource-access :resource-type "FileHandle" :permissions [:write]
                   :constraints {:path [:= "output.txt"]} }
               ]}
  :plan
  (do
    (tool:log "Starting file processing.")
    (with-resource [in-handle [:resource FileHandle] (tool:open-file "input.txt" :mode :read)]
      (with-resource [out-handle [:resource FileHandle] (tool:open-file "output.txt" :mode :write)]
        (loop [line :string (tool:read-line in-handle)]
          (when (not (nil? line)) ;; Check if EOF
            (let [processed-line :string (str "Processed: " line)]
              (tool:write-line out-handle processed-line))
            (recur (tool:read-line in-handle)))))
      ;; out-handle is automatically closed here
      )
    ;; in-handle is automatically closed here
    (tool:log "File processing finished.")))
```

**Explanation:**
*   The `:contracts` field declares the task's need for file access capabilities, specifying allowed paths and permissions. This is part of RTFS's security model.
*   `with-resource` is used to open `input.txt` for reading (binding to `in-handle`) and `output.txt` for writing (binding to `out-handle`).
*   The resource type `[:resource FileHandle]` specifies the kind of resource being managed.
*   Crucially, `in-handle` and `out-handle` are automatically closed when their respective `with-resource` blocks exit, regardless of whether execution was normal or due to an error within the block. This prevents resource leaks.
*   The example uses a `loop`/`recur` construct for iterating through lines in the input file.

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

To run an RTFS file using the reference compiler (once it is fully implemented):
1. Navigate to the compiler directory:
   ```sh
   cd rtfs_compiler
   ```
2. Execute the compiler with the RTFS file as an argument (the exact path to the RTFS file may vary):
   ```sh
   cargo run --release -- ../specs/examples/task_basic_flow.rtfs
   ```
   (Replace `../specs/examples/task_basic_flow.rtfs` with the actual path to your `.rtfs` file.)

This command is expected to parse, validate, and then execute the RTFS task, displaying any outputs or errors generated during its run. Specific command-line options may vary as the compiler evolves.

## Future Directions

RTFS is an actively evolving project with a roadmap focused on enhancing its ecosystem and enabling deeper integration with Artificial Intelligence. Key areas of future development include:

### Compiler Development

Ongoing efforts are focused on building a robust, reference implementation of an RTFS parser and validator, housed within the `RTFS_compiler` project. The primary goals of this compiler are:

*   **Validation:** To meticulously check RTFS code against the language specifications, ensuring syntactic correctness and semantic consistency (including type checks, contract validation, and capability verification).
*   **Execution Engine:** To serve as a core engine capable of interpreting and executing valid RTFS `Task` artifacts.
*   **Standardization:** To serve as a benchmark for any alternative RTFS implementations, promoting interoperability.

This compiler is crucial for ensuring that RTFS code, whether human-written or AI-generated, is reliable and behaves as expected.

### LLM Tooling and Training

A core objective for RTFS is to empower Large Language Models (LLMs) to natively and proficiently generate RTFS code from high-level human instructions or complex goals. To achieve this, we are focused on:

*   **Synthetic Data Generation:** Developing tools and methodologies to create comprehensive and diverse datasets of valid RTFS examples. This involves strategies like template-based generation, grammar-based generation, and using LLMs to bootstrap initial examples which are then validated by the RTFS compiler.
*   **LLM Fine-tuning:** Utilizing these generated datasets to fine-tune specialized LLMs. The aim is to train models that possess a deep understanding of RTFS syntax, semantics, its core principles (like explicit contracts and intent/plan separation), and common usage patterns.

By enabling LLMs to conceptualize and generate solutions directly in RTFS, we anticipate a significant improvement in the reliability, predictability, and safety of AI-driven software development and task automation. This approach leverages RTFS's design for verifiability and explicit effects, making it a more suitable target language for AI code generation than many general-purpose programming languages.

The long-term vision is for RTFS to become a standard, verifiable language that facilitates complex interactions and task delegations between humans and AI agents, or among multiple AI agents.

## Contributing

Contributions are welcome! Please open issues or submit pull requests for improvements or bug fixes. Please refer to any specific `CONTRIBUTING.md` file or project contribution guidelines if available.

## License

This project is licensed under the [Apache License 2.0](LICENSE).
A `LICENSE` file containing the full text of the Apache License 2.0 should reside in the root directory of this repository.
