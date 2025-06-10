# RTFS - Language Semantics (Draft)

This document describes the intended runtime behavior and evaluation rules for the standalone RTFS language.

## 1. Evaluation Model

*   **Applicative Order Evaluation:** Arguments to functions and tool calls are typically evaluated *before* the function/tool is invoked. The order of argument evaluation (e.g., left-to-right) should be specified.
*   **Special Forms:** Certain keywords (`if`, `let`, `do`, `fn`, `def`, `with-resource`, `try`, `match`, `parallel`) have specific evaluation rules that deviate from standard function application (e.g., `if` only evaluates one branch).
*   **Immutability:** Core data structures (numbers, strings, bools, lists, vectors, maps) are immutable by default. Mutation is handled through explicit resource operations or potentially dedicated state management constructs (TBD).
*   **Environment Model:** Evaluation occurs within nested lexical environments. `let` and `fn` create new scopes. `def` adds bindings to the current scope.
*   **Handling Effects:** While RTFS does not expose generic monadic operators (like `bind`), it provides specific constructs (`do`, `let`, `try/catch`, `match`, `with-resource`, tool calls) to explicitly manage the sequencing and context of computations involving effects such as errors, resource lifecycles, and I/O. This achieves similar goals of structured effect management found in monadic programming patterns.

## 2. Scoping Rules

*   **Lexical Scoping:** Variables are resolved based on the structure of the code (where they are defined), not the dynamic call stack.
*   **Module Scope:** Each module defines a distinct namespace. Top-level `def` and `defn` forms within a `(module ...)` block create bindings within that module's namespace.
*   **`let`:** Creates local bindings visible only within the `let` block's body. Shadowing is allowed (including shadowing module-level or imported bindings).
*   **`fn`:** Creates a closure, capturing the lexical environment where it was defined (including any imported or module-level bindings visible at that point). Parameters form a new scope within the function body.
*   **`def`:** Inside a module, defines a variable within the module's namespace. Inside a task plan or function body, defines a variable within the current lexical scope. Re-definition is allowed and shadows.
*   **`import`:** Makes symbols from another module's namespace accessible in the current scope (either directly or via an alias). See Section 7 for details.
*   **Resolution Order:** When resolving a symbol, the runtime typically checks: 1) Local bindings (innermost `let`/`fn` scope first), 2) Imported symbols (potentially qualified), 3) Module-level definitions (if within a module), 4) Built-in/standard library symbols. (Exact order needs careful definition, especially regarding qualified vs unqualified imports).
*   **Task Context (`@`):** A special mechanism (e.g., `@intent`, `@id`) provides read-only access to the fields of the containing `task` artifact from within the `:plan`.

## 3. Concurrency (`parallel`)

*   The `(parallel [id1 expr1] [id2 expr2] ...)` form initiates the concurrent evaluation of `expr1`, `expr2`, etc.
*   The runtime environment is responsible for managing the underlying concurrency (e.g., threads, async tasks).
*   The `parallel` form blocks until *all* constituent expressions have completed (either successfully or with an error).
*   **Result:** Upon successful completion of all branches, `parallel` returns a map where keys are the binding identifiers (as keywords, e.g., `:id1`) and values are the results of the corresponding expressions.
*   **Error Handling:** If any expression within `parallel` evaluates to an error (e.g., returns `[:error error-map]` or throws an exception), the evaluation of other branches may continue or be cancelled (runtime dependent), but the overall `parallel` form immediately terminates and propagates the *first* error encountered. It does not wait for all branches to finish if an error occurs.

## 4. Error Handling Semantics

*   **Structured Errors:** Errors are represented as structured data (e.g., maps with `:type`, `:message`, `:details`), not just strings. The specific structure should be defined by the type system.
*   **Propagation:** By default, errors (represented by `[:error error-map]` return values or runtime exceptions) propagate up the call stack, unwinding execution unless caught.
*   **`try/catch`:** Allows catching specific error types (based on structure/type field in the error map) or any error originating from runtime issues or propagated from tool calls. Enables recovery or alternative logic. It does *not* catch standard `[:error ...]` return values unless those are explicitly re-thrown as exceptions (which is not the standard flow).
*   **`Result` Types (Convention):** Functions and tool calls are strongly encouraged (and often required by their contracts/signatures) to signal expected failure modes by returning a tagged result, e.g., `[:ok value]` or `[:error error-map]`. The `match` expression provides the primary, structured way to handle these expected alternative outcomes.
*   **User Errors:** User-defined functions should signal errors by returning `[:error error-map]`, not by using an internal `throw`-like mechanism.

### 4.1 Standard Error Map Structure

When an error occurs (either via `[:error error-map]` return or a runtime exception), the `error-map` should conform to a standard structure to facilitate consistent handling:

```acl
;; Proposed Standard Error Map Structure
{
  :type :keyword ;; Required. A namespaced keyword identifying the error category (e.g., :error/network, :error/validation, :error/tool-failed, :error/resource-unavailable).
  :message :string ;; Required. A human-readable description of the error.
  :details :map?   ;; Optional. A map containing additional context-specific information about the error (e.g., invalid value, failed URL, tool name).
  ;; :stacktrace :vector? ;; Optional. A vector of strings or structured entries representing the call stack at the point of error (if available/configured).
}
```
*   The `:type` keyword is crucial for `catch` blocks and `match` expressions to dispatch on specific error kinds.
*   Tools should return errors conforming to this structure when they fail.

## 5. Resource Management Semantics

*   **Handles:** Resources (files, sockets, etc.) are represented by opaque handles with associated types.
*   **`with-resource`:** Guarantees that a resource's cleanup function (associated with its type) is called when execution leaves the block's scope, whether normally or due to an error. It simulates RAII (Resource Acquisition Is Initialization) or Python's `with` statement.
*   **Ownership/Consumption (Linearity Simulation):**
    *   The handle bound by `with-resource` is considered "owned" within the block.
    *   Functions operating on resources may "consume" the handle (invalidating the original binding) and potentially return a new handle representing the next state, preventing use-after-consumption. This requires runtime checks or potentially more advanced static analysis based on type system rules (see `type_system.md`, `resource_management.md`).

## 6. Tool Call Semantics

*   Tool calls (`tool:namespace/action ...`) invoke external functionality provided by the runtime environment.
*   The runtime is responsible for resolving the tool name, checking required capabilities (see `security_model.md`), marshalling arguments, invoking the tool, and marshalling the result (or error) back into an RTFS value.
*   Tools abstract interactions with both local resources (files, processes) and non-local resources (web APIs, databases, remote services). For resources requiring lifecycle management (like database connections), tools typically return handles intended to be managed using `with-resource`. For stateless interactions (like simple API calls), tools might operate directly on data arguments.
*   Tool signatures (expected arguments, return types, potential errors) should be declaratively available to the RTFS type checker/validator, likely via the `:contracts` mechanism.
*   **Tool Signature Discovery:** The mechanism by which the runtime/type-checker discovers tool signatures (required for type checking plans and validating `:capabilities-required`) is crucial but considered **external** to the core language specification. It depends on the specific runtime environment and tooling implementation (e.g., using a central registry, manifest files, or an introspection protocol).

## 7. Module System Semantics

*   **Definition (`module`):**
    *   A `(module full.namespace.name ...)` form defines a compilation unit and establishes a unique namespace based on the provided `full.namespace.name` (a namespaced identifier).
    *   Top-level `def` and `defn` forms within the `module` block create bindings (vars) associated with this namespace.
    *   Module definitions are typically expected to reside in files whose paths correspond to the namespace (e.g., `my/company/utils.rtfs` for `my.company.utils`), although the exact loading mechanism is runtime-dependent.
*   **Namespaces:**
    *   Namespaces prevent naming collisions between different modules.
    *   Symbols defined within a module can be referred to externally using their fully qualified name: `namespace.name/symbol-name` (e.g., `my.math.utils/add`).
*   **Exports (`:exports`):**
    *   The `(:exports [symbol1 symbol2 ...])` option within a `module` definition declares which symbols defined within that module are intended for public use.
    *   By default, if `:exports` is present, only the listed symbols are considered public. If `:exports` is omitted, the runtime might default to exporting all top-level `def`/`defn` symbols or none (needs a defined default behavior - let's default to **exporting all** if `:exports` is omitted for simplicity).
    *   Accessing non-exported symbols from outside the module (even using the fully qualified name) should ideally result in an error during resolution or type checking.
*   **Imports (`import`):**
    *   The `(import full.namespace.name ...)` form makes symbols from another module available.
    *   `import` forms are typically allowed at the top level of a module or task plan, or potentially at the beginning of `let` or `do` blocks.
    *   **Default Import:** `(import my.math.utils)` - Makes the public symbols of `my.math.utils` available using their fully qualified names (e.g., `my.math.utils/add`).
    *   **Alias (`:as`):** `(import my.math.utils (:as m))` - Makes public symbols available using the alias prefix: `m/add`. This is the preferred way to avoid namespace collisions.
    *   **Selective Import (`:only`):** `(import my.math.utils (:only [add]))` - Makes *only* the specified public symbols (`add`) available directly in the current scope *without* qualification. Use with caution to avoid polluting the local namespace.
    *   **Combined:** `(import my.math.utils (:as m :only [PI]))` - Makes `PI` available directly, and all other public symbols via `m/symbol`.
*   **Loading:**
    *   The runtime environment is responsible for locating, loading, compiling (if necessary), and caching module definitions when an `import` statement is encountered or a fully qualified symbol is used.
    *   The exact mechanism (filesystem search paths, pre-compiled caches, network loading) is runtime-dependent.
    *   Circular dependencies between modules should ideally be detected and result in an error during loading.
*   **Scope of Imports:** Imported symbols (whether qualified by alias/namespace or imported directly via `:only`) are added to the lexical scope where the `import` form appears. They are subject to shadowing by local `let` or parameter bindings.

## 8. Agent Interaction Semantics

This section details the runtime behavior of special forms designed for tasks to interact with capabilities exposed by external agents, as defined in their `agent-profile` artifacts. These interactions rely on the declarations made in a task\'s `:contracts :requires` section.

### 8.1. `invoke` Semantics

The `(invoke capability-target args-map [options-map?])` special form is used to execute a standard request-response capability of an agent.

*   **Capability Resolution:**
    *   The `capability-target` must be an alias defined in the task\'s `:contracts :requires` list.
    *   The runtime resolves this alias to a specific capability on a specific agent. This involves:
        1.  Looking up the alias in the `:requires` entry to get the `capability-id`, optional `agent-profile-uri`, version constraints, etc.
        2.  If an `agent-profile-uri` is provided, the runtime may fetch and parse this profile.
        3.  The runtime uses the `capability-id` and potentially other information (like version constraints or a pre-configured agent registry) to discover the appropriate agent and its communication endpoint that provides the specified capability. This discovery process is environment-specific but relies on the information in agent profiles.
        4.  The runtime selects a suitable `:communication-endpoints` from the agent\'s profile that matches the capability.

*   **Argument Passing:**
    *   The `args-map` is a map of arguments passed to the capability.
    *   The keys and values in `args-map` **must** conform to the `:input-schema` defined for the target capability in the agent\'s profile. The runtime should validate this before attempting the invocation.

*   **Invocation Options:**
    *   The optional `options-map` can provide invocation-specific parameters, such as `:timeout-ms`.
    *   These options override any default options specified in the corresponding `:requires` entry in the task\'s contract. If not provided in the `invoke` call or the `:requires` entry, runtime defaults apply.

*   **Execution and Result:**
    *   The runtime serializes the arguments and sends the request to the resolved agent endpoint using the protocol specified in the agent profile (e.g., JSON-RPC over HTTP).
    *   The `invoke` form blocks execution until a response is received from the agent or a timeout occurs.
    *   Upon successful execution, the agent returns a result. This result is deserialized by the runtime and **must** conform to the `:output-schema` of the capability.
    *   The `invoke` form evaluates to this resulting value.

*   **Error Handling:**
    *   If the agent encounters an error during capability execution, it should return a structured error.
    *   Errors can also occur at the runtime level (e.g., network issues, agent unreachable, timeout, failure to resolve capability, request/response schema validation failure, failure to resolve due to ambiguity if multiple agents match an alias without further disambiguation).
    *   All such errors are typically propagated back to the task as an `[:error error-map]` structure, conforming to the standard error map format (see Section 4.1). The `invoke` form would then evaluate to this error structure.

### 8.1.1. `invoke` with Dynamic Agent Selection (Refinement)

While `invoke` primarily uses a pre-defined `capability-target` alias from the `:requires` section, its resolution can be influenced by dynamic information, especially when working with results from `discover-agents` or when an alias is intentionally general.

*   **Disambiguation via Options:** If an alias in `:requires` could resolve to multiple agent instances (e.g., it lacks a specific `:agent-id`), the `invoke` call can provide disambiguation hints in its `options-map`:
    *   `":agent-id-override" :string`: Specifies the exact `:agent-id` to use for this particular invocation, overriding any broader match from the alias definition. This is useful when `discover-agents` has identified a specific agent instance.
    *   `":endpoint-override" :string`: Specifies a direct URI for the agent's endpoint, bypassing parts of the discovery and selection logic. This should be used with caution as it tightly couples the task to a specific endpoint.
*   **Resolution Process with Overrides:**
    1.  The `capability-target` alias is looked up in `:requires`.
    2.  If `":agent-id-override"` is present in `invoke`'s options, the runtime attempts to find an agent matching this ID that provides the required capability (as per the alias's `:capability-id` and `:version-constraint`).
    3.  If `":endpoint-override"` is present, the runtime attempts to use this endpoint directly, validating that it can serve the required capability.
    4.  If no overrides are given, the runtime proceeds with its standard discovery/resolution based on the alias's properties (including any `:agent-id`, `:agent-profile-uri`, or discovery parameters like `:discovery-tags` defined in the `:requires` entry itself).
    5.  If, after all resolution attempts, a unique, suitable agent endpoint cannot be determined, `invoke` results in an `[:error/agent.resolution-failed]` or `[:error/agent.ambiguous-target]` error.

### 8.2. `consume-stream` Semantics

The `(consume-stream capability-target params-map {item-binding => body} [options-map?])` special form is used to connect to, and process items from, a streaming capability (a `:stream-source`) offered by an agent.

*   **Capability Resolution:**
    *   Similar to `invoke`, the `capability-target` (an alias from `:requires`) is resolved to a specific streaming capability on an agent, including selecting an appropriate streaming endpoint (e.g., WebSocket).

*   **Stream Initiation:**
    *   The `params-map` contains parameters required to initiate or subscribe to the stream. These parameters **must** conform to the `:input-schema` defined for the streaming capability in the agent\'s profile (which often describes subscription parameters).
    *   The runtime sends these parameters to the agent to establish the stream.

*   **Item Processing:**
    *   Once the stream is established, the agent begins sending items.
    *   For each item received from the stream:
        1.  The item **must** conform to the `:output-schema` of the streaming capability (which defines the structure of individual stream items).
        2.  The `item-binding` symbol is bound to the received item.
        3.  The `body` expressions are evaluated in a new lexical scope that includes this `item-binding`.
    *   The processing of items typically occurs sequentially as they arrive.

*   **Lifecycle and Blocking:**
    *   The `consume-stream` form generally blocks the current flow of execution while the stream is active.
    *   The stream continues until:
        *   It is explicitly closed by the agent (source).
        *   An error occurs (either in the stream itself or during the evaluation of the `body`).
        *   A timeout specified in the `options-map` (e.g., an overall stream `:timeout-ms`) is reached.
        *   The task itself is terminated.

*   **Stream Options:**
    *   The optional `options-map` can include:
        *   `:on-error (fn [err] ...)`: A callback function that is invoked if an error occurs related to the stream (e.g., connection drop, deserialization error of an item). The `err` argument will be an error map.
        *   `:on-complete (fn [] ...)`: A callback function invoked when the stream is closed normally by the source.
        *   `:timeout-ms`: An overall timeout for the stream duration.
        *   Other protocol-specific options.

*   **Return Value:**
    *   The `consume-stream` form itself typically evaluates to `nil` or a status indicator (e.g., `[:ok {:items-processed count}]`) upon normal completion. If an unhandled error occurs that terminates the stream consumption, it will propagate an `[:error error-map]`.

*   **Error Handling:**
    *   Errors can occur during stream setup (e.g., invalid parameters, agent refuses connection), during item transmission (e.g., malformed item, connection lost), or during the evaluation of the `body` for an item.
    *   Errors in the `body` will typically terminate the stream consumption unless handled within the `body` itself.
    *   The `:on-error` callback provides a mechanism to react to stream-level errors without immediately propagating them, though the stream might still terminate.

### 8.3. `produce-to-stream` Semantics

The `(produce-to-stream capability-target item-expression [options-map?])` special form is used to send individual items to a streaming capability that acts as a data sink (a `:stream-sink`) on an agent. This implies an already established or connectable stream.

*   **Capability Resolution:**
    *   The `capability-target` (an alias from `:requires`) is resolved to a specific stream sink capability on an agent. This might involve establishing a connection if one isn\'t already active for this sink (behavior depends on runtime and protocol).

*   **Item Sending:**
    *   The `item-expression` is evaluated. Its result is the item to be sent to the stream.
    *   This item **must** conform to the `:input-schema` of the target stream sink capability (which defines the structure of items it expects to receive).
    *   The runtime serializes the item and sends it to the agent\'s streaming endpoint.

*   **Invocation Options:**
    *   The optional `options-map` can include:
        *   `:ack-timeout-ms`: If the protocol supports acknowledgments for sent items, this specifies how long to wait for an acknowledgment.
        *   Other protocol-specific options.

*   **Execution and Return Value:**
    *   The `produce-to-stream` form typically sends the item and may block briefly depending on the underlying buffering, network, or acknowledgment mechanism.
    *   It commonly evaluates to `nil` on successful sending (or successful acknowledgment if applicable).
    *   If sending fails or an acknowledgment is not received within the timeout, it evaluates to an `[:error error-map]`.

*   **Error Handling:**
    *   Errors can include: failure to connect to the stream sink, schema validation failure for the `item-expression`, network errors during sending, or explicit negative acknowledgment from the agent.
    *   These errors are returned as an `[:error error-map]`.

### 8.4. `discover-agents` Semantics (New Section)

The `(discover-agents criteria-map [options-map?])` special form allows a task to dynamically query an agent discovery service (e.g., an agent registry) for agents matching specified criteria.

*   **Criteria Evaluation:**
    *   The `criteria-map` is evaluated. It contains keys like `:capability-id`, `:version-constraint`, `:agent-id`, `:discovery-tags`, `:discovery-query`, and `:limit` as defined in `syntax_spec.md`.
    *   These criteria are used to construct a query to the agent discovery service.

*   **Discovery Service Interaction:**
    *   The runtime determines the discovery service to use:
        *   If `:registry-uri` is provided in the `options-map`, that registry is queried.
        *   Otherwise, a default discovery service/registry configured in the runtime environment is used.
    *   The runtime sends the query to the discovery service and awaits a response.
    *   The optional `:timeout-ms` from `options-map` applies to this interaction.
    *   The optional `:cache-policy` from `options-map` can influence whether cached results are used or a fresh query is made.

*   **Return Value:**
    *   **Success:** If the discovery is successful, `discover-agents` returns a **vector of `agent_card` structures**. Each `agent_card` is a map conforming to the structure defined in [`agent_discovery.md`](./agent_discovery.md), providing comprehensive information about a discovered agent. This typically includes:
        *   `:agent_id` (`string`): The unique ID of the discovered agent.
        *   `:agent_profile_uri` (`string`?): A URI where the full agent profile can be retrieved.
        *   `:name` (`string`?): The agent\'s name.
        *   `:version` (`string`?): The agent\'s version.
        *   `:description` (`string`?): A description of the agent.
        *   `:capabilities` (`[:vector :map]`?): A list of capability summaries offered by this agent.
        *   `:communication` (`:map`?): Details about communication protocols and endpoints.
        *   `:discovery_tags` (`[:vector :string]`?): Tags associated with the agent for discovery.
        *   `:metadata` (`:map`?): Other relevant metadata.
        *   Other fields as defined in the `agent_card` structure in [`agent_discovery.md`](./agent_discovery.md) (e.g., registry-specific ranking or scoring information might be included by some registry implementations, though not part of the core `agent_card` definition).
    *   If no agents match the criteria, an empty vector `[]` is returned.
    *   **Failure:** If the discovery process itself fails (e.g., registry unreachable, timeout, malformed query, registry error), `discover-agents` evaluates to an `[:error error-map]` structure. Standard error types like `:error/discovery.unavailable`, `:error/discovery.timeout`, or `:error/discovery.query-failed` should be used.

*   **Usage with `invoke`:**
    *   The `agent_card`s returned by `discover-agents` (specifically fields like `:agent_id`, and capability details from `:capabilities`) can be used to dynamically target an agent with the `invoke` special form.
    *   The primary mechanism for this is using the `":agent-id-override"` option in the `invoke` call, providing the `:agent_id` from a selected `agent_card`. The `capability-target` alias in `invoke` would still refer to an entry in the task's `:requires` section, which defines the `:capability-id` and other general contract aspects, while the `":agent-id-override"` pinpoints the specific agent instance.
    *   For example, a task might have a general requirement: `{:alias translate-service :capability-id "vendor/translate"}`.
    *   After `(def discovered (discover-agents {:capability-id "vendor/translate" :discovery-tags [:french]}))`, the plan could pick an agent from `discovered` (e.g., `(first discovered)`) and then call `(invoke translate-service {:text "Hello"} [{:agent-id-override (:agent_id (first discovered))}])`.

*   **Idempotency and Caching:**
    *   Discovery queries may or may not be idempotent depending on the registry and network conditions.
    *   The `:cache-policy` option provides a hint to the runtime on how to handle caching of discovery results to balance freshness with performance.

### 8.5. General Considerations for Agent Interactions

*   **Connection Management:** The RTFS runtime is responsible for managing the underlying network connections to agents based on the details in their profiles (e.g., HTTP connections, WebSocket connections). This may involve connection pooling or keep-alive mechanisms.
    *   For `produce-to-stream`, if multiple items are intended for the same stream sink in sequence, the runtime should endeavor to use a persistent connection if the protocol supports it and the agent endpoint configuration allows. Explicit stream opening/closing by the task for `produce-to-stream` is not currently defined; connection lifecycle is managed by the runtime based on usage patterns and timeouts.
*   **Serialization/Deserialization:** The runtime handles the serialization of outgoing requests/items and deserialization of incoming responses/items according to the protocol and format specified in the agent\\\'s `:communication-endpoints` (e.g., JSON for JSON-RPC).
*   **Schema Validation:** Robust schema validation against the agent capability\\\'s declared `:input-schema` and `:output-schema` is critical at the boundaries (before sending and after receiving) to ensure data integrity and catch errors early.
*   **Discovery and Binding:** The process of discovering agent endpoints and binding them to capability aliases is a key runtime service.
    *   If an `:agent-profile-uri` is provided in a task's `:requires` entry, the runtime should prioritize fetching and using this profile.
    *   If no direct URI is provided, the runtime must rely on the `capability-id` (which might be fully qualified as `agent-id/capability-name-version` or just `capability-name-version`) and consult its configured discovery mechanisms (e.g., a central agent registry, local cache, DNS-based discovery as hinted in `AGENT_DISCOVERY_PROTOCOL_PROPOSAL.MD` and detailed in [`agent_discovery.md`](./agent_discovery.md)). The exact resolution order and mechanisms are environment-specific but must aim to uniquely identify a compatible agent and capability.
    *   The runtime should consider version constraints (`:version-constraint` in `:requires`) during discovery and selection, ensuring the chosen agent capability meets the specified criteria (e.g., semantic versioning compatibility).
*   **Security and Authentication Context:**
    *   The RTFS runtime is responsible for securely managing and applying credentials required for agent communication, as specified in the chosen agent endpoint's `:authentication` details.
    *   The mechanism for provisioning these credentials to the runtime (e.g., environment variables, secure vault integration, task execution context) is outside the scope of RTFS language semantics but is a critical operational concern for the runtime environment. Tasks do not directly handle or specify credentials in the `invoke`/`consume-stream`/`produce-to-stream` calls. The runtime implicitly uses the appropriate credentials based on the resolved agent endpoint. (This should be further detailed in `security_model.md`).
*   **Standardized Agent-Related Error Types:**
    *   In addition to general error types, the runtime and agents should endeavor to use standardized error `:type` keywords for common agent interaction failures. Examples include:
        *   `:error/agent.unavailable`: The agent could not be reached.
        *   `:error/agent.capability-not-found`: The agent was reached, but the specified capability ID is not offered or does not match version constraints.
        *   `:error/agent.authentication-failed`: Authentication with the agent failed.
        *   `:error/agent.authorization-failed`: Authentication succeeded, but the caller is not authorized for the capability.
        *   `:error/agent.protocol-error`: An error occurred in the communication protocol (e.g., malformed message, unexpected response).
        *   `:error/agent.request-schema-validation`: The request/parameters sent to the agent failed its input schema validation.
        *   `:error/agent.response-schema-validation`: The response/item received from the agent failed its output schema validation.
        *   `:error/agent.timeout`: The operation timed out waiting for the agent.
        *   `:error/agent.internal-error`: The agent reported an internal error while executing the capability.
    *   These allow tasks to implement more granular error handling for agent interactions.

*(Further details on specific expression semantics will be added as the language definition matures.)*
