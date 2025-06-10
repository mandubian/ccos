# RTFS Agent Communication Gap Analysis with A2A and MCP

This document analyzes the compatibility of RTFS agent interaction mechanisms (as defined in `grammar_spec.md`, `syntax_spec.md`, `language_semantics.md`, and `security_model.md`) with the A2A (Agent2Agent) and MCP (Model Context Protocol) protocols.

## 1. Core RTFS Agent Interaction Concepts

Before diving into A2A and MCP, let's summarize the RTFS mechanisms:

*   **Agent Profile Declaration:** Tasks can declare dependencies on external agent profiles using the `:contracts` section, specifying an `agent-profile-id`.
    ```clojure
    :contracts {:profile-alias {:agent-profile-id "some-unique-agent-profile-name"}}
    ```
*   **Capability Requirement:** Tasks declare required capabilities from an agent profile.
    ```clojure
    :requires [:profile-alias/capability-name]
    ; or with version/constraints
    :requires [{:capability :profile-alias/data-analyzer :version "1.2.0" :constraints {...}}]
    ```
*   **Invocation:**
    *   Standard call: `(invoke :profile-alias/capability-name {:param1 value1})`
    *   Streaming consumption: `(consume-stream :profile-alias/streaming-capability {:param1 value1})`
    *   Producing to a stream: `(produce-to-stream :profile-alias/ingestion-capability {:param1 value1} data-source-expr)`
*   **Security:** The RTFS runtime is responsible for resolving agent profiles and managing credentials. The `security_model.md` introduces an `:agent-capability-access` permission type.
    ```clojure
    {:type :agent-capability-access
     :agent-profile-id "some-unique-agent-profile-name"
     :capability-id "capability-name" ; or "*"
     :invocation-types [:invoke :consume-stream] ; etc.
     :constraints {:params {:param-name PredicateSchema}}}
    ```
*   **Error Handling:** Standardized error types like `:error/agent.unavailable`, `:error/agent.capability-not-found`, `:error/agent.invocation-failed`, `:error/agent.stream-failed`.

## 2. A2A (Agent2Agent) Protocol Compatibility

A2A is designed for interoperability between disparate AI agent systems, using JSON-RPC 2.0 over HTTP(S) and SSE for streaming.

### 2.1. Agent and Capability Discovery

*   **A2A:** Uses `AgentCard` (typically at `/.well-known/agent.json`) which contains `name`, `description`, `url`, `capabilities` (streaming, push), `authentication`, and a list of `AgentSkill` objects. Each `AgentSkill` has an `id`, `name`, `description`, `inputModes`, `outputModes`.
*   **RTFS:**
    *   The `agent-profile-id` in RTFS could conceptually map to an A2A agent's identity, perhaps derived from its `AgentCard` URL or a registered name.
    *   The RTFS runtime would be responsible for discovering the A2A agent (e.g., fetching its `AgentCard`).
    *   RTFS's `:requires [:profile-alias/capability-name]` would map to an `AgentSkill.id` or `AgentSkill.name` from the A2A `AgentCard`. Version constraints in RTFS would need to be checked against `AgentCard.version` or skill-specific versioning if available (A2A `AgentSkill` doesn't explicitly list a version per skill, but `AgentCard.version` exists for the agent).
*   **Gap/Considerations:**
    *   RTFS needs a clear mechanism for how an `agent-profile-id` resolves to an A2A agent's `AgentCard` URL. This could be a registry or a convention.
    *   Mapping RTFS capability constraints (e.g., on parameters) to A2A `AgentSkill` input/output modes or other metadata needs to be defined. A2A `AgentSkill` has `inputModes` and `outputModes` (e.g., "text", "file") but not detailed schema constraints per se in the summary.

### 2.2. Invocation Mechanisms

*   **A2A:**
    *   Standard call: `tasks/send` method (request/response). Parameters are in `TaskSendParams` (includes `message` with `parts`). Result is a `Task` object.
    *   Streaming: `tasks/sendSubscribe` method (request/stream). Parameters similar. Results are streamed as `TaskStatusUpdateEvent` or `TaskArtifactUpdateEvent` via SSE.
*   **RTFS:**
    *   `invoke :profile-alias/capability-name {...params}`: This maps well to A2A's `tasks/send`. The RTFS runtime would construct the `TaskSendParams.message` using the provided RTFS params, converting them to A2A `Part`s (TextPart, FilePart, DataPart). The response `Task.artifacts` or `Task.status.message` would be converted back to RTFS types.
    *   `consume-stream :profile-alias/streaming-capability {...params}`: This maps well to A2A's `tasks/sendSubscribe`. The RTFS runtime would handle the SSE connection and adapt the stream of `TaskStatusUpdateEvent` and `TaskArtifactUpdateEvent` into a stream consumable by the RTFS task. The `Artifact.parts` would be the primary source of data.
    *   `produce-to-stream :profile-alias/ingestion-capability {...params} data-source`: A2A is primarily defined from the perspective of a client *sending* a task to an agent and potentially receiving a stream *from* the agent. A2A's `tasks/send` and `tasks/sendSubscribe` involve sending `Message` objects which can contain `FilePart` (with `bytes` or `uri`) or `DataPart`. If the `data-source-expr` in RTFS produces a sequence of data, the RTFS runtime could potentially make multiple `tasks/send` calls if the A2A capability is designed to accept data incrementally, or it might need to buffer and send if the A2A capability expects a single message. A2A's `append: true` for `Artifact` parts in streaming updates is about the agent *appending* to an artifact, not the client streaming input. This RTFS pattern might require specific A2A agent skill design that can accept streamed input, perhaps via multiple `tasks/send` calls correlated by `sessionId`, or a long-lived connection if supported by a specific A2A skill (though not standard A2A RPC).
*   **Gap/Considerations:**
    *   **Data Mapping:** RTFS types need a defined mapping to A2A `Part` types (e.g., RTFS records to `DataPart`, RTFS blobs/files to `FilePart`, RTFS text to `TextPart`).
    *   **`produce-to-stream`:** This is the most complex to map. If the A2A agent skill doesn't explicitly support receiving a stream of data from the client for a single conceptual "task", this might be problematic. The A2A protocol itself focuses on the agent streaming *back* to the client. An A2A agent *could* be designed to receive a sequence of `tasks/send` requests for a long-running ingestion, using `sessionId` for correlation.

### 2.3. Data Exchange Formats

*   **A2A:** Uses JSON. `Message` objects contain `Part[]`. `Part` can be `TextPart`, `FilePart` (with `bytes` or `uri`), or `DataPart` (structured JSON). `Artifacts` also contain `Part[]`.
*   **RTFS:** Has its own rich type system.
*   **Gap/Considerations:**
    *   A robust mapping layer is needed in the RTFS runtime to convert between RTFS types and A2A's `Part` structure. This includes handling complex RTFS types, records, variants, streams, etc., and serializing/deserializing them into appropriate `DataPart` (JSON), `TextPart`, or `FilePart` representations.
    *   For `FilePart`, RTFS needs to decide whether to use `bytes` (base64 encoded) or `uri`. If `uri`, accessibility of the URI by the A2A agent is a concern.

### 2.4. Security and Authentication

*   **A2A:** `AgentCard` can specify `authentication` schemes (e.g., API keys, OAuth, JWT). The A2A spec mentions JWT for push notifications.
*   **RTFS:** The `security_model.md` states the runtime manages credentials. The `:agent-capability-access` permission controls which tasks can call which agent capabilities.
*   **Compatibility:**
    *   The RTFS runtime, when resolving an `agent-profile-id` and discovering an A2A agent, would need to inspect the `AgentCard.authentication` section.
    *   Based on the required authentication, the runtime would use its managed credentials (associated with the `agent-profile-id` or the task's execution context) to authenticate with the A2A agent.
    *   This aligns well, as RTFS centralizes credential management and access control, while A2A defines how agents advertise their auth requirements.
*   **Gap/Considerations:**
    *   The RTFS runtime needs to support various authentication mechanisms that A2A agents might require (as listed in their `AgentCard`).

### 2.5. Error Handling

*   **A2A:** Uses standard JSON-RPC error codes (e.g., `-32601 MethodNotFoundError`) and A2A specific error codes (e.g., `-32001 TaskNotFoundError`, `-32005 ContentTypeNotSupportedError`). Errors have `code`, `message`, and optional `data`.
*   **RTFS:** Defines errors like `:error/agent.unavailable`, `:error/agent.capability-not-found`, `:error/agent.invocation-failed`.
*   **Compatibility:**
    *   The RTFS runtime should map A2A error responses to its standardized error types. For example:
        *   A2A connection errors or HTTP errors before a JSON-RPC response -> RTFS `:error/agent.unavailable`.
        *   A2A `-32601 MethodNotFoundError` (if skill maps to method) or if a skill ID is not found after fetching `AgentCard` -> RTFS `:error/agent.capability.not-found`.
        *   Other A2A errors during task processing (e.g., `failed` task state, or specific error codes) -> RTFS `:error/agent.invocation-failed` or `:error/agent.stream-failed`, potentially with more specific subtypes or carrying the A2A error details.
*   **Gap/Considerations:**
    *   A detailed mapping from A2A error codes and `Task.status` (when `failed`) to RTFS error types should be established.

## 3. MCP (Model Context Protocol) Compatibility

MCP is a JSON-RPC based protocol for client-server communication, often where the "server" provides resources, tools, or prompts to a "client" (which might be an LLM-based application).

### 3.1. Agent/Service and Capability Discovery

*   **MCP:**
    *   Servers advertise capabilities through `InitializeResult.capabilities`.
    *   Specific "capabilities" like tools are listed via `tools/list` (returns `ListToolsResult` with `Tool[]`). Each `Tool` has `toolID`, `description`, `parameters` (JSON schema).
    *   Resources are listed via `resources/list` (returns `ListResourcesResult` with `Resource[]`).
    *   Prompts via `prompts/list`.
*   **RTFS:**
    *   An `agent-profile-id` in RTFS would map to an MCP server instance.
    *   RTFS `:requires [:profile-alias/capability-name]` could map to:
        *   An MCP `Tool.toolID`.
        *   A specific resource URI pattern that the task intends to `resources/read`.
        *   A prompt ID from `prompts/list`.
    *   The RTFS runtime would connect to the MCP server, perform initialization, and then use methods like `tools/list` to verify the existence and schema of the required "capability".
*   **Gap/Considerations:**
    *   RTFS `agent-profile-id` needs to resolve to an MCP server endpoint.
    *   The "capability-name" in RTFS needs a convention to distinguish between MCP tools, resources, or prompts if they share a similar naming space or if an agent profile exposes multiple types. E.g., `tool:myTool`, `resource:/path/to/data`.
    *   MCP `Tool.parameters` are defined by JSON Schema. RTFS type system and constraints need to be compatible or mappable to JSON Schema for validation before calling.

### 3.2. Invocation Mechanisms

*   **MCP:**
    *   Tool invocation: `tools/call` method. Params: `CallToolRequest` (includes `toolID`, `arguments`). Result: `CallToolResult`.
    *   Resource reading: `resources/read` method. Params: `ReadResourceRequest` (includes `uri`). Result: `ReadResourceResult` (includes `ResourceContents`).
    *   MCP also has `prompts/get` for prompts.
    *   Streaming: MCP has `resources/subscribe` for `ResourceUpdatedNotification` and general `ProgressNotification` for long-running requests. It doesn't seem to have a direct equivalent to A2A's `tasks/sendSubscribe` for a continuous stream of arbitrary data chunks from a single "tool call" or "resource read" itself, but rather notifications about changes or progress.
*   **RTFS:**
    *   `invoke :profile-alias/capability-name {...params}`:
        *   If `capability-name` refers to an MCP tool, this maps to `tools/call`. RTFS params become `CallToolRequest.arguments`.
        *   If `capability-name` refers to a resource, this could map to `resources/read`. Params might specify the URI or sub-parts.
    *   `consume-stream :profile-alias/streaming-capability {...params}`:
        *   This is less direct. If the "streaming capability" refers to being notified of resource updates, it could map to `resources/subscribe`, and the RTFS runtime would adapt the `ResourceUpdatedNotification` stream.
        *   If it refers to progress of a long-running operation, `ProgressNotification` (associated with a `progressToken` from an initial request) could be used.
        *   MCP doesn't seem to offer a generic "stream of data" from a tool call in the same way A2A does with SSE events carrying `Artifact` parts. `CallToolResult` is a single response.
    *   `produce-to-stream :profile-alias/ingestion-capability {...params} data-source`:
        *   MCP does not seem to have a standard mechanism for a client to stream data *to* a server-side tool or resource for ingestion within a single RPC call.
        *   This might require multiple `tools/call` or other custom methods if the MCP server/tool is designed to accept data incrementally. Or, similar to A2A, data might need to be passed as a `BlobResourceContents` or via a URI if the tool can read it.
*   **Gap/Considerations:**
    *   **`consume-stream`:** Mapping to MCP is not straightforward for arbitrary data streams. It fits better for update notifications (`ResourceUpdatedNotification`) or progress updates. If an MCP tool needs to return a large dataset, it would likely do so in a single `CallToolResult` or by writing to a resource that the client then reads.
    *   **`produce-to-stream`:** Similar to A2A, this is challenging. MCP is generally client-pull or server-push for notifications, not client-push for streaming data *into* a standard tool call.
    *   **Data Mapping:** RTFS types to JSON arguments for `tools/call` (respecting the tool's JSON Schema for parameters) and RTFS types from `CallToolResult` or `ResourceContents`.

### 3.3. Data Exchange Formats

*   **MCP:** JSON. `Tool` parameters are defined by JSON Schema. `ResourceContents` can be `TextResourceContents` or `BlobResourceContents`. `CallToolResult` contains `result` (any JSON).
*   **RTFS:** Own type system.
*   **Gap/Considerations:**
    *   Robust mapping between RTFS types and JSON, respecting JSON Schemas provided by MCP tools.
    *   Handling of binary data (RTFS blob vs. `BlobResourceContents` or base64 encoding within JSON).

### 3.4. Security and Authentication

*   **MCP:** The provided schema is less explicit on application-level authentication beyond the `initialize` handshake. It relies on the underlying transport (e.g., HTTPS) for secure communication. `ClientCapabilities` and `ServerCapabilities` might play a role, but specific auth mechanisms (like API keys, tokens in headers) are not detailed in the core JSON-RPC message structures for every call.
*   **RTFS:** Runtime manages credentials.
*   **Compatibility:**
    *   The RTFS runtime would need to handle any authentication required by the MCP server at the transport level or during/after the `initialize` handshake (e.g., passing tokens as part of JSON-RPC requests if the MCP server defines such custom extensions, or via HTTP headers).
*   **Gap/Considerations:**
    *   MCP's flexibility means authentication methods can vary. The RTFS runtime needs to be adaptable or assume common patterns (e.g., bearer tokens in HTTP headers). The `agent-profile-id` configuration in RTFS would need to store or reference these credentials.

### 3.5. Error Handling

*   **MCP:** Uses standard JSON-RPC error codes. For `tools/call`, it specifies that errors *originating from the tool itself* SHOULD be reported inside the `CallToolResult` object (e.g., `isError: true`), not as protocol-level errors, so the LLM can see them. Protocol-level errors are for issues like `METHOD_NOT_FOUND`.
*   **RTFS:** Standardized agent-related errors.
*   **Compatibility:**
    *   RTFS runtime should map MCP JSON-RPC errors to RTFS errors (e.g., `METHOD_NOT_FOUND` to `:error/agent.capability-not-found`).
    *   For `tools/call`, the runtime needs to inspect `CallToolResult` for tool-specific errors and map them to `:error/agent.invocation-failed`, including the error details from the result.
*   **Gap/Considerations:**
    *   Distinguishing between MCP protocol errors and tool-execution errors within `CallToolResult` is important for correct mapping to RTFS error semantics.

## 4. Summary of Gaps and Potential RTFS Refinements

**Update (2025-06-09):** Many of the discovery-related gaps identified below (particularly in 4.1.1, 4.2, and 4.3 regarding agent/capability discovery and resolution) have been substantially addressed by the introduction of the `(discover-agents ...)` special form, the `agent_card` data structure, the agent discovery protocol detailed in `agent_discovery.md`, and refinements to the `(invoke ...)` special form (e.g., `:agent-id-override`). These additions provide a formal mechanism for dynamic agent discovery and invocation within RTFS.

### 4.1. Common Gaps (A2A & MCP)

1.  **Agent/Service Endpoint Resolution:** RTFS `agent-profile-id` needs a clear, robust mechanism to resolve to actual network endpoints and discovery documents (A2A `AgentCard` URL, MCP server address). This could involve a runtime-configurable registry or service discovery protocol. *(Partially addressed by `agent_discovery.md` and `discover-agents` which formalize the interaction with such registries/mechanisms).*
2.  **`produce-to-stream` Pattern:** This RTFS feature is not directly supported by standard A2A or MCP call patterns, which are more client-request/server-response or server-stream-to-client. Supporting this may require:
    *   The external agent/tool to be specifically designed to accept streamed input (e.g., via multiple calls, WebSockets, or other non-standard JSON-RPC mechanisms).
    *   The RTFS runtime to buffer the stream and send it as a single data blob if the agent capability expects that (losing the "streaming" benefit for the agent).
    *   Clarification in RTFS specs on how this should behave with agents not supporting client-side streaming input.
3.  **Data Type Mapping:** A comprehensive specification for mapping between RTFS's rich type system and the JSON-based structures of A2A (`Part`s) and MCP (JSON Schema for tool params, `ResourceContents`) is crucial. This includes complex types, collections, blobs, and error states.
4.  **Capability Constraint Mapping:** RTFS allows specifying `:constraints` on required capabilities. Mapping these to A2A's `inputModes`/`outputModes` or MCP's JSON Schema for tool parameters needs to be well-defined. The runtime would need to perform pre-flight checks or rely on the agent to enforce them.

### 4.2. A2A-Specific Considerations

*   **Skill Versioning:** A2A `AgentSkill` doesn't have individual versions in the summary. RTFS version constraints would likely apply to the `AgentCard.version`.
*   **Task Lifecycle:** A2A has a detailed task lifecycle (`TaskStatus`, `TaskState`). RTFS `invoke` and `consume-stream` are synchronous from the task's perspective (the stream itself is asynchronous, but the call to start consuming it is blocking until the stream is established or fails). The RTFS runtime would manage the A2A task lifecycle interaction.

### 4.3. MCP-Specific Considerations

*   **`consume-stream` Mapping:** This is less direct for MCP. It fits best for `ResourceUpdatedNotification` or `ProgressNotification`. For general data streaming from a "tool", MCP seems to favor a single large response or writing to a resource. RTFS might need to clarify expectations for `consume-stream` with MCP-like agents.
*   **Tool Error Reporting:** RTFS runtime must correctly handle MCP's dual error reporting (protocol errors vs. in-result errors for tools).
*   **Authentication:** MCP is less prescriptive on application-level auth in its core schema. RTFS runtime will need to be flexible.

### 4.4. Potential RTFS Refinements/Clarifications

1.  **Agent Profile Resolution:** Detail how `agent-profile-id`s are mapped to concrete agent endpoints and discovery mechanisms.
2.  **Streaming Semantics:**
    *   Clarify behavior of `produce-to-stream` when the target agent capability doesn't natively support input streaming.
    *   For `consume-stream`, define how different underlying streaming mechanisms (A2A SSE, MCP notifications) are abstracted.
3.  **Data Mapping Rules:** Provide guidelines or even a standard library for RTFS-to-JSON (and vice-versa) conversions, especially for A2A `Part`s and MCP JSON Schemas.
4.  **Error Mapping Details:** Offer a more detailed mapping table from common A2A/MCP errors to RTFS error types.
5.  **Capability Constraint Enforcement:** Specify whether the RTFS runtime attempts to validate constraints client-side (if possible) or if it's purely the agent's responsibility.

### 4.5 Detailed Data Type Mapping Considerations (RTFS to/from JSON)

A robust and well-defined mapping between RTFS\'s type system and JSON (used by A2A `DataPart` and for MCP parameters/results) is critical for interoperability. The RTFS runtime or an adapter layer is responsible for these conversions.

**General Principles:**

*   **Fidelity vs. Simplicity:** Strive for high fidelity where possible, but acknowledge that some RTFS type distinctions might be lost in JSON (e.g., different integer sizes all becoming JSON `number`).
*   **Bi-directionality:** Mappings should ideally be bi-directional, though perfect symmetry might not always be achievable.
*   **Configuration/Customization:** Allow for potential customization of serialization/deserialization for specific RTFS types or agent interactions.

**Mapping Strategies for Common RTFS Type Categories:**

1.  **Primitive Types:**
    *   **RTFS `Integer`, `Float`:** Map to JSON `number`.
        *   *Consideration:* JSON numbers are typically IEEE 754 double-precision. If RTFS supports arbitrary-precision integers or specific float types, precision loss can occur. For very large integers, string representation in JSON might be necessary.
    *   **RTFS `String`:** Map to JSON `string`.
    *   **RTFS `Boolean`:** Map to JSON `boolean` (`true`, `false`).
    *   **RTFS `Nil` (or equivalent):** Map to JSON `null`.
    *   **RTFS `Symbol`, `Keyword`:** Map to JSON `string`.
        *   *Consideration:* A convention might be needed to distinguish them from regular strings if necessary on deserialization, e.g., prefixing or wrapping in a simple object `{"type": "symbol", "value": "foo"}`. For many use cases, treating them as strings is sufficient.

2.  **Collection Types:**
    *   **RTFS `List`, `Vector`, `Sequence`:** Map to JSON `array`. Order is preserved.
    *   **RTFS `Map`, `Record` (struct-like):** Map to JSON `object`.
        *   *Keys:* RTFS map keys (if not already strings, e.g., keywords or symbols) must be converted to JSON string keys. This conversion must be consistent.
        *   *Fields:* Record field names (often keywords or symbols in Lisp-like languages) become JSON object keys (strings).

3.  **Algebraic Data Types:**
    *   **RTFS `Variant` / `Tagged Union` (e.g., `(Either String Integer)`):**
        *   **Strategy 1 (Wrapped object):** `{"type": "VariantCaseName", "value": ...value...}`. E.g., `{"type": "Left", "value": "error"}` or `{"type": "Right", "value": 123}`. This is explicit and good for discriminated unions.
        *   **Strategy 2 (Case as key):** `{"VariantCaseName": ...value...}`. E.g., `{"Left": "error"}`. This can be more concise but might be ambiguous if `value` can also be an object.
        *   The choice depends on clarity and compatibility with how A2A/MCP agents expect structured data.

4.  **Binary Data / Blobs:**
    *   **General JSON:** Typically Base64 encoded string: `{"data": "SGVsbG8gd29ybGQ="}`.
    *   **A2A:**
        *   Use `FilePart` with `bytes` (Base64 string) or `uri`.
        *   Small binary data could be embedded as Base64 in a `DataPart` if appropriate.
    *   **MCP:**
        *   `BlobResourceContents` often implies Base64.
        *   Can be passed as a Base64 string within a JSON parameter.
        *   URIs pointing to binary data are also common.
    *   *Consideration:* The RTFS runtime needs to handle the Base64 encoding/decoding.

5.  **Streams:**
    *   Streams are not directly representable as a single static JSON value.
    *   **When RTFS consumes a stream (A2A/MCP -> RTFS):** The adapter receives individual items/events (e.g., A2A SSE events, MCP notifications) and converts each item to an RTFS type before passing it to the RTFS stream processing logic.
    *   **When RTFS produces a stream (RTFS -> A2A/MCP):**
        *   **A2A:** Each item from the RTFS stream is converted to an appropriate A2A `Part` (often a `DataPart` containing JSON, or `TextPart`) and sent as part of an A2A streaming event.
        *   **MCP:** If an MCP tool is defined to return a "stream," it might be represented as a JSON array in the `CallToolResult`. For true streaming, MCP relies on mechanisms like `ProgressNotification` or `ResourceUpdatedNotification`, where each notification payload would be a JSON object representing a stream item.
    *   *Consideration:* The mapping of individual stream elements follows the rules for other RTFS types.

6.  **Dates, Times, Durations:**
    *   Map to JSON `string` using ISO 8601 format (e.g., `"2023-10-26T07:40:00Z"` for datetime, `"P3Y6M4DT12H30M5S"` for duration). This is a widely accepted standard.

7.  **Custom/Opaque Types:**
    *   If an RTFS type has a canonical string representation, that can be used.
    *   Otherwise, they might be treated as opaque blobs (serialized, possibly Base64 encoded) if they need to be round-tripped.
    *   If they are only sent one way to an agent that understands their internal structure (e.g., via a custom schema), that schema dictates the JSON format.

8.  **Type Refinements and Predicates (e.g., `(Integer :min 0 :max 100)`):**
    *   These primarily affect schema generation and validation, not the wire format of the data itself (which would still be a JSON `number`).
    *   **When RTFS calls MCP:** If RTFS knows the JSON Schema for an MCP tool\'s parameters (which might include constraints like `minimum`, `maximum`, `pattern`), it can perform client-side validation before sending.
    *   **When RTFS exposes capabilities (e.g., as an MCP tool):** The RTFS adapter should generate a JSON Schema for its parameters, translating RTFS type refinements into corresponding JSON Schema validation keywords.

**Specifics for A2A `Part` Structure:**

*   The RTFS runtime/adapter decides how to package data into A2A `Message.parts`:
    *   Simple text: `TextPart`.
    *   Structured data (records, lists, variants, primitives other than simple text): Serialize to JSON string, then embed in `DataPart.data` (with `contentType: "application/json"`).
    *   Binary data: `FilePart` (using `bytes` for embedded Base64 or `uri`).
    *   The choice can be guided by the `inputModes` or `outputModes` declared in the A2A `AgentSkill`.

**Specifics for MCP JSON Schema:**

*   **RTFS as MCP Client:** When calling an MCP tool, RTFS values are converted to JSON according to the rules above. The structure must conform to the JSON Schema provided by the tool in `Tool.parameters`. The RTFS runtime might perform validation against this schema before sending.
*   **RTFS as MCP Server:** When RTFS exposes an RTFS task as an MCP tool, the adapter layer must generate a JSON Schema for the task\'s input parameters and output. This schema is derived from the RTFS types and any refinements. For example:
    *   RTFS `(Record :name String :age (Integer :min 0))`
    *   JSON Schema:
        ```json
        {
          "type": "object",
          "properties": {
            "name": {"type": "string"},
            "age": {"type": "integer", "minimum": 0}
          },
          "required": ["name", "age"]
        }
        ```

**Potential Challenges:**

*   **Loss of Type Information:** JSON\'s simpler type system (e.g., only one `number` type) means some RTFS type distinctions might be lost.
*   **Cycles in Data Structures:** Standard JSON does not support object cycles. If RTFS data can contain cycles, they must be broken or represented using a convention (e.g., by-reference using IDs) if they need to be transmitted. This is often complex.
*   **Error Handling for (De)serialization:** Robust error handling is needed for cases where RTFS data cannot be serialized to JSON or incoming JSON does not conform to the expected structure for deserialization into RTFS types.

A clear specification or library within the RTFS ecosystem for these mappings would be highly beneficial for consistent and reliable agent communication.

This analysis suggests that while RTFS agent interaction concepts are generally compatible with A2A and MCP, the RTFS runtime will play a critical role as an adaptation layer. The main challenges lie in the `produce-to-stream` pattern and the specifics of data type and constraint mapping.

## 5. A2A/MCP Calling RTFS (RTFS as a Server)

The discussion above focuses on RTFS tasks calling external A2A/MCP agents. This section considers the reverse: an external A2A client or MCP client calling into an RTFS system. For this to be feasible without excessive effort, the RTFS system would need to expose an A2A or MCP-compliant server interface. This typically involves building an **adapter layer** on top of the RTFS runtime.

### 5.1. General Requirements for RTFS as a Server

1.  **HTTP/JSON-RPC Infrastructure:**
    *   An HTTP server capable of handling POST requests.
    *   A JSON-RPC 2.0 request processing pipeline to parse incoming requests and formulate responses.

2.  **RTFS Runtime Integration:**
    *   A mechanism for the adapter layer to discover available RTFS tasks that can be exposed as capabilities.
    *   An API to programmatically invoke these RTFS tasks, pass parameters (translated from A2A/MCP format), and retrieve their results or subscribe to their output streams.
    *   A way to manage the lifecycle of RTFS task executions initiated by external calls.

3.  **Data Marshalling:**
    *   Converting incoming A2A `Message.parts` or MCP JSON arguments into the data types expected by the target RTFS tasks.
    *   Converting results or errors from RTFS task execution back into the A2A `Task` object / streaming events or MCP `CallToolResult` / `ReadResourceResult` formats.

### 5.2. RTFS as an A2A Agent

To act as an A2A agent, the RTFS server adapter would need to:

*   **Implement Agent Discovery:**
    *   Serve an `AgentCard` (e.g., at `/.well-known/agent.json`). This card would describe the RTFS system as an A2A agent and list its "skills," which would correspond to specific, exposable RTFS tasks.
    *   The `AgentCard` would also specify supported authentication mechanisms.
*   **Implement A2A JSON-RPC Methods:**
    *   `tasks/send`: For invoking RTFS tasks that operate in a request/response manner. The adapter would map the incoming `TaskSendParams` to an RTFS task invocation and return a `Task` object representing the outcome.
    *   `tasks/sendSubscribe`: If some RTFS tasks can produce a stream of results, this method would be implemented to handle the subscription and stream back `TaskStatusUpdateEvent` or `TaskArtifactUpdateEvent` via Server-Sent Events (SSE).
*   **Handle A2A Task Lifecycle:** Manage the state of tasks as understood by A2A (e.g., `pending`, `running`, `completed`, `failed`).

### 5.3. RTFS as an MCP Server

To act as an MCP server, the RTFS server adapter would need to:

*   **Implement MCP Handshake & Capability Advertisement:**
    *   Respond to the `initialize` request, advertising its server capabilities.
    *   Implement `tools/list` to expose selected RTFS tasks as MCP `Tool`s. This includes providing a `toolID`, `description`, and `parameters` (as a JSON Schema) for each exposed RTFS task.
    *   If applicable, implement `resources/list` and `prompts/list` if RTFS manages entities that can be exposed as MCP resources or prompts.
*   **Implement MCP JSON-RPC Methods:**
    *   `tools/call`: For invoking RTFS tasks exposed as tools. The adapter would parse `CallToolRequest`, validate arguments against the tool's JSON Schema, invoke the RTFS task, and return a `CallToolResult`. Error handling must align with MCP's convention (tool execution errors often reported within the `CallToolResult`).
    *   `resources/read`, `prompts/get` (if applicable): For serving content from RTFS-managed resources or prompts.
    *   `resources/subscribe` (if applicable): If RTFS can provide updates for exposed resources.

### 5.4. Effort and Feasibility

The effort to enable A2A/MCP calls *into* RTFS is **non-trivial** and constitutes a development project to build the necessary server-side adapter.

*   **Feasible if:**
    *   The RTFS runtime has a clear API for programmatic task invocation and result retrieval.
    *   RTFS data types can be reasonably mapped to/from JSON and A2A `Part` structures.
    *   The execution model of RTFS tasks is compatible with a server request/response or streaming paradigm.
*   **"Crazy effort" if:**
    *   The RTFS runtime is a "black box" with no clean integration points.
    *   Significant mismatches exist between RTFS's internal workings (type system, execution model) and the requirements of A2A/MCP.
    *   Defining clear boundaries for exposable RTFS "capabilities" is difficult.

In summary, while possible, making RTFS callable by A2A or MCP clients requires dedicated development to build the server-side protocol adapters and integrate them with the RTFS core.
