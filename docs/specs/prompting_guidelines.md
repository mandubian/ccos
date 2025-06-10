# Guidelines for Prompting LLMs to Generate RTFS Tasks

This document outlines key considerations for crafting effective prompts when asking a Large Language Model (LLM) to generate code in the RTFS language. The goal is to receive accurate, complete, and well-structured RTFS task definitions.

## 1. Be Explicit About the Goal: RTFS Task Generation

*   **Clearly state the target language:** "Generate an RTFS task definition..." or "Write RTFS code for a task that..."
*   **Specify the top-level structure:** Usually, you'll want a `(task ...)` definition. If you need a module, specify `(module ...)` instead.

## 2. Define the Task Intent Clearly

*   **Provide a concise `:intent`:** Describe the overall goal of the task. This could be a simple string or a structured map.
    *   *Example Prompt:* "Generate an RTFS task with the intent 'Process uploaded user data file and update database'."
    *   *Example Prompt:* "The task intent should be a map like `{:action :analyze-log :source-file "/path/to/log.txt"}`."

## 3. Detail the Execution Plan (`:plan`)

*   **Break down the steps:** List the sequence of actions the task should perform.
*   **Specify logic and control flow:** Mention conditions (`if`), loops (if applicable, e.g., using `loop`/`recur` if supported, or describing iterative tool calls), sequential execution (`do`).
*   **Identify necessary tool calls:** Name the specific tools to be used (e.g., `tool:fetch-data`, `tool:process-image`). Include required arguments.
*   **Describe error handling:**
    *   Specify whether to use `match` for expected results (e.g., `[:ok ...]` / `[:error ...]`) or `try/catch` for runtime exceptions.
    *   Detail how different error conditions should be handled (e.g., "If `tool:fetch-data` returns `[:error {:type :network}]`, log the error and return `nil`.").
*   **Mention concurrency:** If steps can run in parallel, explicitly request the use of `(parallel ...)`.
*   **Specify resource management:** If tools return resource handles (like file handles, database connections, tensor handles), request the use of `(with-resource ...)` for proper cleanup.

## 4. Specify Contracts (`:contracts`)

*   **Input/Output Schemas:** Describe the expected structure and types of the task's input and output data using RTFS type syntax (e.g., `[:map [:user-id :int] [:data :string]]`).
*   **Capabilities Required:** List the necessary capabilities, including tool calls, resource access, or network access. Be specific about constraints if needed.
    *   *Example Prompt:* "The task requires the `tool:send-email:v1` capability and read access to files matching `/data/*.csv`."

## 5. Request Specific RTFS Features

*   If you want the LLM to use specific language constructs like `log-step`, destructuring (`let [{:keys [...]}] ...`), specific type definitions, or module imports, mention them explicitly.

## 6. Provide Context

*   **Available Tools:** List the tools the LLM can assume are available, including their expected input arguments and return types (especially if they return tagged results like `[:ok ...]` or `[:error ...]`).
*   **Data Structures:** Describe any relevant data structures or formats the task will interact with.
*   **Environment:** Mention any relevant environmental context (e.g., "The task runs in an environment where network access is restricted.").

## 6.5. Embedding RTFS Syntax Guidance (Crucial for Custom Language)

Since RTFS is a custom language, the LLM won't know its syntax inherently. You **must** provide syntax guidance directly within the prompt.

*   **Define the Core Structure:** Explicitly state the expected top-level form and its main components.
    *   *Example Prompt Snippet:* "Generate an RTFS `(task ...)` structure. It must contain the keys `:id` (use placeholder 'task-XYZ'), `:intent` (a map), `:plan` (RTFS code), and `:contracts` (a map)."
*   **Provide Syntax Examples:** Show the LLM the exact syntax for RTFS constructs needed for the plan.
    *   *Example Prompt Snippet (for `match`):* "When handling the tool result, use the `match` expression. Its syntax is `(match result-variable [:ok data] (handle-ok data) [:error err] (handle-error err) _ (handle-default))`."
    *   *Example Prompt Snippet (for tool calls):* "Tool calls use the format `(tool:namespace/tool-name arg1 arg2)`."
    *   *Example Prompt Snippet (for `with-resource`):* "For resources needing cleanup (like file handles), use `(with-resource [handle [:resource ResourceType] (tool:open-resource ...)] body...)`."
    *   *Example Prompt Snippet (for `let`):* "Use `(let [var1 value1 var2 value2] ...)` for local bindings."
*   **Specify Data Formats:** Define the structure for complex data like capabilities.
    *   *Example Prompt Snippet:* "Under `:contracts`, list `:capabilities-required` as a vector of maps, like `[{:type :tool-call :tool-name \"tool:read-file\"}]`."
*   **Leverage S-expression Base:** Remind the LLM that RTFS uses S-expressions (Lisp-like syntax), which might help it with basic list/map structures.

By embedding these syntax snippets and structural definitions, you guide the LLM to generate code that conforms to the RTFS specification, even without prior training on it.

## 7. Define the Desired Output Format

*   **Completeness:** Ask for a "complete and valid RTFS task definition".
*   **Placeholders:** Specify if placeholders are acceptable for parts the LLM cannot know (e.g., specific IDs, complex logic details).
*   **Code Blocks:** Request the output be formatted as a code block.

## Example Prompt Structure

```
Generate a complete RTFS task definition for the following:

*   **Intent:** "Fetch weather data for a given city, process it, and save the summary."
*   **Plan:**
    1.  Define a variable 'city' with the value "London".
    2.  Call `tool:fetch-weather` with the city name. This tool returns `[:ok weather-map]` on success or `[:error error-details]` on failure.
    3.  Use `match` to handle the result:
        *   If `[:ok weather-map]`, extract the temperature and conditions. Log them using `tool:log`. Proceed to step 4.
        *   If `[:error error-details]`, log the error message from `error-details` using `tool:log-error` and stop, returning `nil`.
    4.  Call `tool:summarize-weather` with the extracted temperature and conditions.
    5.  Call `tool:save-summary` with the result from the previous step.
    6.  Return the summary.
*   **Contracts:**
    *   Input Schema: None (or `:map` if input binding is expected).
    *   Output Schema: `[:map [:summary :string]]` or `:nil` on error.
    *   Capabilities: `tool:fetch-weather`, `tool:log`, `tool:log-error`, `tool:summarize-weather`, `tool:save-summary`.
*   **Output:** Provide the full `(task ...)` definition in an RTFS code block.
```

By providing detailed and structured prompts following these guidelines, you increase the likelihood of obtaining high-quality, functional RTFS code from an LLM.
