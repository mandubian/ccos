# ACL Language Prospections

This document captures forward-looking ideas, design considerations, and discussions about the potential evolution and deeper philosophy of the AI-Centric Language (ACL).

## 1. Convergence of Natural Language and Executable Code

**Concept:** Explore ACL not just as a target for AI code generation, but as a richer intermediate representation that captures both:
    a.  The semantic **intent** derived from a human's natural language instruction.
    b.  The procedural **execution plan** (executable ACL code) needed to fulfill that intent, along with its execution history and status.

The AI system translates natural language into this structured ACL `task` representation.

**Alignment with Current ACL Design:**

*   **S-expressions & Homoiconicity:** Ideal for embedding structured metadata (intent) alongside code and execution history within a unified data structure.
*   **Data Structures (`Value`):** Suitable for representing intent, code (`:plan`), and log metadata.
*   **Functional Core & Predictability:** Makes the executable `:plan` easier for AI to generate reliably.
*   **Static Typing (Planned):** Can enforce structure on intent, plans, and log entries.

**Structural Implementation:**

*   **`task` Form:** The central structure containing `:intent` and `:execution-log`.
*   **`:intent` Block:** Captures the structured semantic goal derived from NL.
*   **`:execution-log`:** An immutable list of stages tracking planning evolution and execution status. Each stage contains metadata and a `:plan` (an `Expr`).
*   **Standard Library:** Includes primitives for actions relevant to developer instructions (file I/O, AST manipulation, terminal commands, workflow commands like `:send-planner`), managed via controlled side effects.

**Example Sketch (GitHub Issue Fetching - Initial Task):**

```acl
(task ;; Top-level form representing the instruction and its execution plan
  :id "gh-fetch-bugs-001" ;; Unique identifier for the task
  :source "human-instruction"
  :natural-language "Fetch the list of open issues for the 'acl-compiler' repository on GitHub, extract the titles of issues labeled 'bug', and save them to a file named 'bug_titles.txt'."

  :intent { ;; Structured representation of the goal
    :action :fetch-filter-save
    :data-source { :type :github-issues :repo "acl-compiler" :state :open }
    :filter-criteria { :label "bug" }
    :data-to-extract :title
    :destination { :type :file :path "bug_titles.txt" }
  }

  :execution-log [ ;; Initial log with planning request
    { :stage 1
      :agent "Formulator-AI"
      :timestamp "..."
      :plan (:send-planner {}) ;; Request planning
    }
  ]
)
```

## 2. Integration with Tools

**Concept:** ACL execution plans should seamlessly incorporate calls to external tools (local CLIs, APIs, IDE functions) as primitive operations.

**Compatibility:** This fits perfectly with the ACL-as-plan idea. The "how" (execution steps) can include these tool calls.

**Implementation:**

*   **Representing Tool Calls:** Define specific ACL functions/primitives (e.g., `(tool:read-file path)`, `(tool:run-terminal-command cmd)`).
*   **Standard Library:** Include these primitives with defined types.
*   **Runtime Role:** The interpreter/compiler executes these calls, interacting with the OS/environment.
*   **Data Flow:** Marshal tool results back into ACL `Value` types.
*   **Controlled Side Effects:** Tool calls are inherently side effects and must be managed within the planned monadic context (e.g., `IO`).

**Benefits:**

*   **Unified Plan:** Single ACL structure for internal logic and external actions.
*   **Leverage Capabilities:** Use existing powerful tools.
*   **Transparency:** Plan explicitly shows tool usage.

## 3. Managing Mutable/Remote Resources (e.g., Tensors)

**Challenge:** Balancing the need for efficient mutation of large resources (like tensors, GPU buffers) with ACL's goal of predictability, while handling resource lifecycles and potential remoteness.

**Proposed Approach: Opaque Handles + Controlled Operations (within IO/Resource Monad)**

1.  **Opaque Resource Handles:** Use an immutable ACL `Value::ResourceHandle { id: ResourceId, type_tag: String }` that refers to the actual resource managed *outside* the core ACL value system by the runtime.
2.  **Runtime Resource Management:** The Rust runtime manages the lifecycle (allocation, deallocation) and location (CPU, GPU, remote) of the actual resource data, mapping `ResourceId`s to it.
3.  **Controlled Primitive Operations:** Define specific, built-in ACL functions that operate on handles. Operations causing mutation or side effects (allocation, mutation, deallocation, data transfer) return results within the `IO` (or specialized `Resource`) monad.
    *   Examples: `(alloc-tensor shape dtype) -> (IO TensorHandle)`, `(free-resource! handle) -> (IO Void)`, `(tensor-add! result-h t1-h t2-h) -> (IO Void)`, `(tensor-read handle indices) -> (IO Value)`, `(tensor-send handle target-device) -> (IO TensorHandle)`.
4.  **Monadic Sequencing:** Use `(do ...)` to ensure effectful operations occur in the correct order, confining mutation and side effects.

**Benefits:**

*   Preserves predictability in the pure parts of ACL.
*   Abstracts resource location and management details behind handles.
*   Provides explicit control over resource lifecycles and side effects.
*   Structured enough for AI generation.

**Example Snippet (Conceptual Tensor Ops):**

```acl
(do
  [handle1 : TensorHandle (alloc-tensor [100 100] :float32)]
  [handle2 : TensorHandle (alloc-tensor [100 100] :float32)]
  [handle_sum : TensorHandle (alloc-tensor [100 100] :float32)]

  (_ : Void (tensor-fill! handle1 1.0))
  (_ : Void (tensor-fill! handle2 2.0))

  ;; Perform mutation via primitive
  (_ : Void (tensor-add! handle_sum handle1 handle2))

  ;; Read a value (potentially copies)
  [val : Float (tensor-read handle_sum [50 50])]
  (_ : Void (print (string-append "Value: " (float->string val))))

  ;; Clean up
  (_ : Void (free-resource! handle1))
  (_ : Void (free-resource! handle2))
  (_ : Void (free-resource! handle_sum))

  (pure_io :success)
)
```

## 4. Multi-Agent Workflows, Execution Tracking, and Parallelism

**Concept:** Extend the immutable log approach to support detailed execution status tracking and parallel execution within tasks, enabling complex, traceable workflows.

**Approach: Execution Log + Structured Plans + Status Updates**

1.  **Structured Plans (`:plan` field):** Introduce ACL constructs within the plan to manage control flow:
    *   `(do step1 step2 ...)`: Sequential execution.
    *   `(parallel [step-id-1 plan-expr-1] [step-id-2 plan-expr-2] ...)`: Defines steps that can run concurrently, each identified by a unique `step-id`.
    *   `(join step-id-1 step-id-2 ...)`: Synchronization point; waits for specified parallel steps to complete.
    *   `(log-step :id <id> <expr>)`: Optionally wrap expressions to ensure their start/completion is explicitly logged.
    *   `(result <step-id>)`: Hypothetical construct to access the result of a previously completed step (identified by its ID) from the execution log.

2.  **Execution Log (`:execution-log` field):** An append-only list tracking planning *and* execution state transitions.
    *   **Planning Stages:** Record the evolution of the `:plan` itself (e.g., initial planning request `(:send-planner ...)` followed by the planner's generated plan).
    *   **Execution Stages:** When the executor processes a step (identified by `step-id` or position):
        *   Append a stage marking the step as `:status :running`.
        *   Upon completion, append another stage referencing the 'running' stage, marking it `:status :completed` with a `:result`, or `:status :failed` with an `:error`.

3.  **Executor Role:**
    *   Interprets the `:plan` from the latest *planning* stage in the log.
    *   Appends new log entries reflecting the execution status of individual steps.
    *   Manages concurrency for `(parallel ...)` blocks.
    *   Uses the log to resolve `(join ...)` dependencies and potentially `(result ...)` calls.

**Example Structure:**

```acl
(task
  :intent { ... }
  :execution-log [
    { :stage 1 :agent "Formulator" :plan (:send-planner {}) } ;; Planning request

    { :stage 2 :agent "Planner" :derived-from 1 ;; Planning result
      :plan (do
              (parallel
                ["fetch-A" (tool:fetch-data "source-A")]
                ["fetch-B" (tool:fetch-data "source-B")])
              (join "fetch-A" "fetch-B")
              (log-step :id "process" (process-results (result "fetch-A") (result "fetch-B"))))
    }

    ;; --- Execution Phase --- starts interpreting plan from stage 2
    { :stage 3 :agent "Executor" :derived-from 2 :executing-step "fetch-A" :status :running }
    { :stage 4 :agent "Executor" :derived-from 2 :executing-step "fetch-B" :status :running }
    { :stage 5 :agent "Executor" :derived-from 3 :executed-step "fetch-A" :status :completed :result "<data-A>" }
    { :stage 6 :agent "Executor" :derived-from 4 :executed-step "fetch-B" :status :failed :error {:type ...} }
    { :stage 7 :agent "Executor" :derived-from 2 :executed-step "join" :status :failed :error {:reason ...} }
    ;; ... execution potentially stops or handles error ...
  ])
```

**Advantages:**

*   **Full Traceability:** Complete planning and execution history.
*   **Handles Parallelism:** Explicit representation and tracking.
*   **Status Tracking:** Fine-grained status recorded immutably.
*   **Immutability:** Log is append-only.
*   **Resilience:** Potential for resuming tasks by analyzing the log.

**Challenges:**

*   **Log Verbosity:** Requires careful management or summarization.
*   **Executor Complexity:** Needs to handle concurrency, log interpretation, and state management.
*   **Result Propagation:** Accessing results (`(result ...)`) needs a robust mechanism tied to the log.

This approach provides a comprehensive framework for representing and tracking complex, potentially parallel AI tasks within ACL.
