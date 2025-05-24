# RTFS - Reasoning Task Flow Specification

_(or Read The F\*\*\*ing Spec)_

## Meta-Teaser: RTFS in its Own Words

```lisp
(task :id "rtfs-readme-teaser-v1"
  :metadata {
    :purpose "Briefly showcase RTFS by defining its essence in its own format."
    :target "README.md Introduction"
    :timestamp "2025-05-25T11:00:00Z" ;; Current date
  }

  :intent "To be a self-describing artifact of the RTFS language, illustrating its core structure and principles for AI-driven task execution. This very code is an example of RTFS."

  :contracts {
    :input "None; this task is a self-contained demonstration."
    :output "The understanding of RTFS imparted by reading this structure."
    :capabilities-needed [
      "ai:generate-and-understand-rtfs" ;; Conceptual capability
      "system:log-execution-securely"   ;; Conceptual capability
    ]
  }

  :plan (do
          ;; RTFS is designed for AI:
          (def ai-centricity "Core Idea: A language for AI agents to define, run, and track tasks.")

          ;; RTFS is verifiable:
          (def verifiable-code "Structure: 'intent' (what to do), 'contracts' (rules), 'plan' (how to do it).")

          ;; RTFS is traceable:
          (def traceable-execution "Audit: ':execution-trace' (see below) logs all actions for clarity and security.")

          ;; RTFS is data:
          (def code-is-data "Syntax: S-expressions, making RTFS code easy for AI to generate and process.")

          ;; This plan\\'s 'execution' is you reading and understanding this!
          (tool:display-message "Welcome to RTFS! This is how it works, in its own terms.")
        )

  :execution-trace [
    { :event :task-instantiated
      :timestamp "2025-05-25T11:00:01Z"
      :agent "RTFS-Documentation-Bot"
      :details "This RTFS task was generated to serve as a concise, self-explanatory teaser."
      :security { :hash-of-previous "nil" :current-hash "abc123xyz789" :method "sha256-chain" }
    }
    ;; Further entries would log the 'execution' of the plan by an RTFS runtime.
  ]
)
```

This document outlines the fundamental concepts, design principles, and technical approach behind the **Reasoning Task Flow Specification (RTFS)** language.

## Project Rationale: An Author's Perspective (also reshaped by the LLM :D)

While existing AI communication protocols like Anthropic's MCP and Google'ss A2A are significant, they appear to offer limited scope for a truly AI-native language. Current approaches don't seem to fully harness a Large Language Model's (LLM) capacity to articulate complex intents and processes derived from human instructions. Furthermore, the distinction between application-to-AI and AI-to-AI communication can be seen as artificial, as both often stem from initial directives (human or AI) and lead to AI-driven interactions.

This project, RTFS, explores an alternative: a language where LLMs have significantly shaped the design, making it primarily for AI use. The central idea is to enable an AI to generate a self-contained structure representing a unit of work. This structure, referred to in RTFS as a `task`, is envisioned to include:

- The original instruction (from a human or another AI).
- The AI\\\'s inferred intent, considering available tools.
- A detailed plan to execute the work.
- A complete, verifiable execution trace.

This artifact (the RTFS `task`), being both data and code, should be self-explanatory, clearly defining both the objective (\"what\") and the method (\"how\"). Key principles guiding this design are verifiability, safety, traceability, robustness, and determinism. The aim is to make it straightforward for AIs to generate, parse, and execute complex operations.

RTFS also serves as an experiment to explore the capabilities of LLMs in language design, particularly when the language is intended for the AI's own use with minimal human steering. Although the LLM's initial design choices (favoring types, effects, and immutability) aligned with principles I personally support and encouraged, the development process has been an insightful journey into this collaborative and ambitious approach to creating a new language.

## Project Genesis: The Bootstrap Prompt

The RTFS project was initiated with the following prompt, which has guided its core philosophy:

> You are the "AI" able to answer my instructions in the most precise and detailed way.
>
> Let's work together to create a new programming language that is aimed at being used by you "AI" to generate code following my human instructions in this language.
>
> This language should be the most practical for you as an AI, not necessarily for me as a human to represent everything you need to respond to my instructions. Thus, the language syntax doesn't need to be readable or practical for me, the human. Keep that in mind.
>
> I need this language to be interpretable and runnable on a classic machine.
>
> So you'll have to write a compiler that can either live interpret code and run it, either generate a static executable. You can rely on an existing language to write this compiler.
>
> You can choose the theory you want to build this this language. But I want the code to be predictible and executable code to be robust and deterministic.
>
> Remember that this language is made for YOU, not for me.

This initial directive, along with subsequent design discussions and refinements (many of which are archived in the `chats/` directory), led to the creation of RTFS. RTFS was conceived and initially designed by a Large Language Model (mostly using Github Copilot with Gemini 2.5 Pro a lot, Claude 3.7 and GPT4.1 a bit and a tiny bit of Claude 4 Sonnet) with the primary goal of creating a language optimized for AI systems to define, execute, trace, and verify complex tasks, directly addressing the needs outlined in the prompt.

## 1. Purpose and Core Motivations (Shaped by the Genesis Prompt)

Stemming from the bootstrap prompt, RTFS is a specialized language primarily designed to be **generated by AI systems** in response to human instructions. Its core purpose is to serve as a practical and effective medium for AI to structure, represent, and execute tasks, even if its syntax and constructs are not optimized for direct human authoring.

**Key Motivations & Goals (aligned with the Genesis Prompt):**

- **AI-Centric Generation:** The language must be easy for AI to generate, parse, and manipulate. Human readability and syntactic convenience for humans are secondary concerns to AI practicality.
- **Bridging Human Instructions to Executable Code:** RTFS aims to be the target language an AI uses to translate high-level human requests into concrete, runnable plans.
- **Interpretability & Compilability:** RTFS code must be executable on standard computing machinery, either through an interpreter or by compilation to a static executable.
- **Predictability, Robustness, Determinism:** The language and its execution model are designed to ensure that tasks run predictably and reliably, producing deterministic outcomes where specified.
- **Verifiability & Safety:** Incorporate features like a strong type system, explicit contracts (input/output schemas, capabilities), and explicit effect handling to allow validation of AI-generated plans and increase trust in their execution.
- **Traceability & Auditability:** Provide a detailed, immutable record of planning and execution (`:execution-trace`) for debugging, auditing AI behavior, understanding decision-making processes, and ensuring accountability.
- **Portability:** Define the language independently of any specific runtime to allow for diverse implementations (e.g., in Rust, Python, JavaScript).
- **Expressiveness for AI Tasks:** Capture common patterns in AI task execution, including sequential/parallel flow, tool usage, resource management, and error handling.

## 2. Design Principles (Informed by LLM Insights)

The design of RTFS was guided by an LLM's understanding of what would make a language effective for AI-to-AI interaction and for human oversight of AI operations.

- **Data-Centrism & Homoiconicity:**
  RTFS uses an S-expression syntax, meaning **code is data**. This choice, made by the LLM, makes RTFS expressions inherently easy for other AI systems to generate, parse, analyze, and transform. The `task` artifact itself is a data structure.

  ```lisp
  ;; Simple plan illustrating data-like code
  (do
    (tool:log "Starting task...")
    (let [x 10] (* x 2)))
  ```

````

- **Verifiability & Safety (Types & Contracts):**
  To enhance the reliability of AI-generated plans, RTFS includes a type system (both static and dynamic checks) and explicit contracts. Tasks define their `:input-schema`, `:output-schema`, and `:capabilities-required`. This allows for upfront validation and runtime checks, ensuring that tasks operate within expected boundaries.

  ```lisp
  (task :id "sample-task"
    :contracts {
      :input-schema [:map [:data :string]]
      :output-schema [:map [:result :int]]
      :capabilities-required [{:type :tool-call :tool-name "tool:process"}]
    }
    :plan (;; ... plan using input.data and tool:process ...)
  )
  ```

- **Explicit Effects & Resource Management:**
  All interactions with the external world (tool calls, I/O, etc.) are explicit operations within the `:plan`. RTFS provides constructs like `with-resource` to manage the lifecycle of resources (e.g., file handles, network connections, tensor objects) safely and predictably. This explicitness is crucial for understanding and controlling AI behavior.

  ```lisp
  (with-resource [file-handle FileHandle (tool:open-file "data.txt" :mode :read)]
    (tool:read-line file-handle))
  ```

  Complex data types like **tensors or ML models** can be represented either as first-class array types (`[:array :float [100 100 3]]`) or managed as opaque resource handles (`[:resource TensorHandle]`) via `with-resource`.

- **Traceability & Auditability:**
  Every `task` artifact includes an `:execution-trace`, an immutable, append-only log of significant events. To enhance security, this trace leverages cryptographic techniques like hashing to link entries and ensure data integrity. This makes the execution record verifiable, tamper-evident, and trustworthy, which is crucial for debugging, auditing AI behavior, understanding decision-making, and ensuring accountability.

- **Modularity & Reusability:**
  RTFS supports `module`, `import`, and `export` mechanisms, allowing developers and AI agents to create and share libraries of reusable functions and task components, promoting a more organized and efficient workflow development.

## 3. The `Task` Artifact

The **`task`** is the central, self-contained artifact in RTFS. It represents a complete unit of work and encapsulates:

- **`:id`**: A unique identifier.
- **`:metadata`**: Information about the task's origin, creation time, etc.
- **`:intent`**: The semantic goal of the task.
- **`:contracts`**: Defines the task's interface, including input/output schemas and required capabilities.
- **`:plan`**: The executable RTFS code defining the steps to achieve the intent.
- **`:execution-trace`**: An immutable log of execution events.

This structured artifact ensures that all necessary information for understanding, validating, executing, and auditing a task is contained within a single unit.

## 4. Technical Approach & LLM Generation

Bringing RTFS to life involves a toolchain designed to process, execute, and ultimately, help LLMs generate RTFS code more effectively.

- **Parser and Intermediate Representation (IR):**
  RTFS source text (S-expressions) is processed by a **Parser** into an Abstract Syntax Tree (AST) or directly into a canonical, structured **Intermediate Representation (IR)**. This IR is crucial for:

  - **Validation:** Type checking, contract validation, and security checks.
  - **Execution:** The runtime/interpreter executes the IR.
  - **Optimization:** Future compilers could optimize the IR.
  - **LLM Training:** The structured nature of the IR is ideal for training LLMs.

  **Implementation Choice (Rust):**
  While RTFS's S-expression syntax might suggest a Clojure-based parser for easier initial development (given Clojure's LISP nature and native handling of S-expressions), a full **Rust implementation** was chosen for the core RTFS tooling (parser, validator, and eventual runtime/compiler). This decision was driven by the need for:

  - **High Performance:** Critical for a language intended for frequent generation and processing by AI systems.
  - **Robustness & Safety:** Rust's static typing, ownership model, and borrow checker offer strong compile-time guarantees, leading to more reliable and secure tooling. This is paramount when executing AI-generated code.
  - **Fine-Grained Control:** Rust provides detailed control over system resources, memory, and execution flow, essential for optimization and complex integrations.
  - **Long-term Maintainability & Ecosystem:** Rust offers a strong foundation for building performant, standalone tools and benefits from a mature ecosystem for systems programming.

  Although a Clojure script-based approach could have simplified initial S-expression handling, it was deemed less fitting for the project's core requirements of performance, safety, and control. The choice of Rust underscores a commitment to building a highly reliable and efficient foundation for AI task execution.

- **Enabling Native LLM Generation:**
  A core goal is to leverage the IR and a growing corpus of RTFS `task` artifacts to fine-tune Large Language Models. The aim is for LLMs to "learn" the structure, semantics, and common patterns of RTFS, enabling them to natively generate correct, efficient, and safe RTFS plans. This creates a virtuous cycle: an LLM-designed language that other LLMs can master.

## 4.1. Example: From Human Instruction to RTFS Task

To illustrate how RTFS bridges human instructions and AI execution, consider the following:

**Human Instruction:**

"Please summarize the latest news article I uploaded about renewable energy, make it about 150 words, and save it as 'renewable_energy_summary.txt'."

**Corresponding RTFS `Task` (Conceptual):**

An AI system, upon receiving this instruction, would aim to generate an RTFS `task` artifact similar to this:

```lisp
(task :id "task-uuid-6789"
  :metadata {
    :source "human-chat-ui:v1.2"
    :user-id "user-abc-123"
    :timestamp "2025-05-24T14:30:00Z" ;; Assuming current date from context
    :original-instruction "Please summarize the latest news article I uploaded about renewable energy, make it about 150 words, and save it as \'renewable_energy_summary.txt\'."
  }

  :intent {
    :action :summarize-document-and-save
    ;; How the document is identified/accessed would depend on the system's capabilities
    :document-source { :type :user-upload :identifier "latest_news_article.pdf" }
    :summary-constraints { :target-word-count 150 }
    :output-destination { :type :file :filename "renewable_energy_summary.txt" }
  }

  :contracts {
    ;; Input schema for the :plan, assuming the document content is made available
    :input-schema [:map
                     [:document-content :string]
                     [:target-word-count :int]
                     [:output-filename :string]]
    :output-schema [:map
                      [:summary-text :string]
                      [:actual-word-count :int]
                      [:file-path :string]]
    :capabilities-required [
      { :type :tool-call :tool-name "ai:text-summarizer:v2" } ;; Example tool
      { :type :tool-call :tool-name "sys:file-writer:v1" }    ;; Example tool
      { :type :resource-access :resource-type "UserUploadedFile" :permissions [:read] }
    ]
  }

  :plan (do
          ;; The plan assumes the runtime or a pre-processing step makes
          ;; 'document-content-string', 'desired-word-count', and 'target-filename'
          ;; available in its initial scope, based on the :intent and system capabilities.

          (let [summary-result :map (ai:text-summarizer document-content-string
                                                        :max-words desired-word-count)
                final-summary :string (:text summary-result)
                words-generated :int (:count summary-result)]

            (sys:file-writer target-filename final-summary)

            ;; Return a map that adheres to the :output-schema
            { :summary-text final-summary
              :actual-word-count words-generated
              :file-path target-filename }))

  :execution-trace [] ;; Initially empty, populated by the runtime
)
```

This example demonstrates how the human's goal is captured in `:intent`, the task's operational boundaries are set in `:contracts`, and the steps to achieve the goal are laid out in `:plan`. The AI's role is to accurately translate the nuances of the human request into these structured RTFS components.

## 4.2. Example: Remote Tool Call and Execution Trace

This example illustrates how an RTFS task can call a remote tool (e.g., another AI model) and how the `:execution-trace` logs these interactions.

**Human Instruction:**

"Analyze the sentiment of the following customer feedback: \'The new interface is amazing and so easy to use!\' and tell me if it\'s positive, negative, or neutral."

**Corresponding RTFS `Task` (Conceptual):**

```acl
(task :id "task-uuid-1011"
  :metadata {
    :source "human-voice-assistant:v3.1"
    :user-id "user-xyz-789"
    :timestamp "2025-05-24T16:00:00Z"
    :original-instruction "Analyze the sentiment of the following customer feedback: \'The new interface is amazing and so easy to use!\' and tell me if it\'s positive, negative, or neutral."
  }

  :intent {
    :action :analyze-text-sentiment
    :text-to-analyze "The new interface is amazing and so easy to use!"
    :desired-output [:enum "positive" "negative" "neutral"]
  }

  :contracts {
    :input-schema [:map [:feedback-text :string]]
    :output-schema [:map
                     [:analyzed-text :string]
                     [:sentiment :string] ; Should ideally be a more specific enum type
                     [:confidence :float?]] ; Optional confidence score
    :capabilities-required [
      { :type :tool-call
        :tool-name "remote-sentiment-analyzer:v1"
        ;; Optionally, specify expected schema for the tool call itself
        :tool-input-schema [:map [:text-content :string]]
        :tool-output-schema [:map [:sentiment :string] [:confidence :float]]
      }
    ]
  }

  :plan (do
          ;; Assume \'feedback-text-input\' is provided by the runtime based on :intent
          (let [analysis-result :map (remote-sentiment-analyzer:v1
                                        {:text-content feedback-text-input})
                sentiment-value :string (:sentiment analysis-result)
                confidence-score :float? (:confidence analysis-result)]

            { :analyzed-text feedback-text-input
              :sentiment sentiment-value
              :confidence confidence-score }))

  ;; Execution trace will be built up by the runtime
  :execution-trace []
)
```

**Evolution of the `:execution-trace`:**

The `:execution-trace` is an append-only log. Here’s how it might look at different stages:

**1. After Task Creation and Planning:**

```lisp
:execution-trace [
  { :timestamp "2025-05-24T16:00:05Z"
    :agent "rtfs-planner-agent:v2.2"
    :event :task-created-and-planned
    :details {
      :task-id "task-uuid-1011"
      :intent-hash "sha256-intent-hash..."
      :plan-hash "sha256-plan-hash..."
      :contracts-hash "sha256-contracts-hash..."
    }
    :previous-entry-hash nil
    :signature { :key-id "planner-key-01" :algo :ed25519 :value "sig-planner..." }
  }
]
```

**2. Before Calling the Remote Tool:**

The runtime logs the intent to call the tool.

```lisp
:execution-trace [
  ;; ... previous entry ...
  { :timestamp "2025-05-24T16:00:10Z"
    :agent "rtfs-executor-runtime:v1.5"
    :event :tool-call-start
    :step-id "sentiment-analysis-step" ; If plan uses (log-step)
    :details {
      :tool-name "remote-sentiment-analyzer:v1"
      :parameters { :text-content "The new interface is amazing and so easy to use!" }
    }
    :previous-entry-hash "sha256-of-previous-entry..."
    :signature { :key-id "executor-key-01" :algo :ed25519 :value "sig-executor-pre-call..." }
  }
]
```

**3. After Receiving a Successful Response from the Remote Tool:**

The runtime logs the response.

```lisp
:execution-trace [
  ;; ... previous entries ...
  { :timestamp "2025-05-24T16:00:12Z" ; Assuming a 2-second response time
    :agent "rtfs-executor-runtime:v1.5" ; Logged by local executor
    :event :tool-call-completed
    :step-id "sentiment-analysis-step"
    :details {
      :tool-name "remote-sentiment-analyzer:v1"
      :status :success
      :response { :sentiment "positive" :confidence 0.98 }
      ;; Optionally, the remote tool itself could add signed entries to a sub-trace
      ;; or the response could be signed by the remote tool.
      :remote-signature { :key-id "remote-analyzer-key-77" :value "sig-remote..." }
    }
    :previous-entry-hash "sha256-of-tool-call-start-entry..."
    :signature { :key-id "executor-key-01" :algo :ed25519 :value "sig-executor-post-call-success..." }
  }
]
```

**4. Alternative: If the Remote Tool Call Fails:**

```lisp
:execution-trace [
  ;; ... previous entries (up to tool-call-start) ...
  { :timestamp "2025-05-24T16:00:15Z"
    :agent "rtfs-executor-runtime:v1.5"
    :event :tool-call-completed
    :step-id "sentiment-analysis-step"
    :details {
      :tool-name "remote-sentiment-analyzer:v1"
      :status :error
      :error-info {
        :error-code "service-unavailable"
        :message "The sentiment analyzer service is currently down for maintenance."
        :retryable false
      }
    }
    :previous-entry-hash "sha256-of-tool-call-start-entry..."
    :signature { :key-id "executor-key-01" :algo :ed25519 :value "sig-executor-post-call-failure..." }
  }
]
```

**5. After Task Completion (assuming successful tool call):**

The runtime logs the final result of the task.

```lisp
:execution-trace [
  ;; ... previous entries ...
  { :timestamp "2025-05-24T16:00:13Z"
    :agent "rtfs-executor-runtime:v1.5"
    :event :task-completed
    :details {
      :task-id "task-uuid-1011"
      :final-status :success
      :result {
        :analyzed-text "The new interface is amazing and so easy to use!"
        :sentiment "positive"
        :confidence 0.98
      }
      :result-hash "sha256-of-result-map..."
    }
    :previous-entry-hash "sha256-of-tool-call-completed-entry..."
    :signature { :key-id "executor-key-01" :algo :ed25519 :value "sig-executor-task-complete..." }
  }
]
```

This detailed trace allows for comprehensive auditing of the task\'s execution, including all interactions with external systems.

## 5. Key Specification Documents

For more detailed information, please refer to the following specification documents:

- [Core Concepts](./specs/core_concepts.md)
- [Syntax Specification](./specs/syntax_spec.md)
- [Grammar Specification](./specs/grammar_spec.md)
- [Type System](./specs/type_system.md)
- [Language Semantics](./specs/language_semantics.md)
- [Examples](./specs/examples.md)
- [Standard Library (Partial)](./specs/stdlib_spec.md)
- [Security Model](./specs/security_model.md)
- [Resource Management](./specs/resource_management.md)

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

```

```
````
