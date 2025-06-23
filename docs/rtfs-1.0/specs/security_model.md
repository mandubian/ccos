# RTFS - Security Model Specification (Draft)

This document outlines the security mechanisms designed for the standalone RTFS language, focusing on authenticity, integrity, and controlled execution.

## 1. Goals

*   **Authenticity:** Verify that task components (intent, plan, log entries) were created by the claimed originating agent.
*   **Integrity:** Ensure that task components have not been tampered with after creation/signing.
*   **Traceability:** Provide a secure, verifiable audit trail of the task lifecycle.
*   **Controlled Execution:** Limit the potential impact of executing RTFS plans by restricting access to external tools and resources (Capability-Based Security).

## 2. Authenticity and Integrity: Signatures in Execution Trace

*   **Mechanism:** Digital signatures are embedded within the `:execution-trace` of the `task` artifact.
*   **Agent Keys:** Each trusted AI agent (or runtime component acting on its behalf) possesses one or more cryptographic key pairs (e.g., Ed25519). Public keys are assumed to be securely distributed or discoverable.
*   **Signing Process:**
    1.  When an agent performs a significant action resulting in a log entry (e.g., `:task-created`, `:plan-generated`, `:step-executed`), it prepares the data for the log entry.
    2.  It constructs the canonical representation of the data to be signed. This **must** include:
        *   The core details of the current log entry (timestamp, agent, event, details).
        *   A cryptographic hash of the *previous* log entry in the trace (or a known initial value for the first entry), creating a tamper-evident chain.
        *   Optionally, hashes or references to larger artifacts created in this step (like the `:plan` or `:contracts`) if they are not stored inline in the `:details`.
    3.  The agent signs this canonical representation using its private key.
    4.  The signature information (key identifier, algorithm, signature value) is added to the log entry structure.
*   **Log Entry Structure:**
    ```acl
    (log-entry
      :timestamp "..."
      :agent "agent-id:version"
      :event :event-type
      :details { ... } ;; Event-specific data
      ;; Optional hash link for chaining
      :previous-entry-hash "sha256-..."
      ;; Signature block
      :signature {
        :key-id "agent-key-identifier" ;; ID of the public key to use for verification
        :algo :ed25519 ;; Signature algorithm used
        :value "base64-encoded-signature" ;; The actual signature
      }
    )
    ```
*   **Verification Process:**
    1.  A verifying agent or runtime iterates through the `:execution-trace`.
    2.  For each entry, it retrieves the claimed agent's public key (using `:key-id`).
    3.  It reconstructs the exact canonical data that should have been signed (current entry details + previous entry hash).
    4.  It verifies the `:signature` using the public key and the reconstructed data.
    5.  It calculates the hash of the current entry to verify the `:previous-entry-hash` link of the *next* entry.
    6.  Any verification failure indicates tampering or forgery.

## 3. Controlled Execution: Capability-Based Security (Conceptual)

*   **Goal:** Limit the authority of an executing RTFS plan to only the external tools and resources explicitly required and granted. Prevent ambient authority.
*   **Capabilities:** Abstract representations of the right to perform a specific action (e.g., call `tool:fetch-url` for a specific domain, write to a specific file path, access a specific database table).
*   **Declaration:** The `:contracts` section of a `task` includes `:capabilities-required`, listing the capabilities the planner determined are needed for the `:plan`.
*   **Granting:** Before execution, the runtime environment (or an orchestrator) validates the requested capabilities against security policies. If allowed, it grants **capability tokens** (secure, unforgeable references, potentially short-lived) corresponding to the requested capabilities. These tokens might be stored in a runtime security context associated with the task execution.
*   **Invocation Check:** When the plan attempts to call a tool or access a resource (e.g., `tool:fetch-url`, `tool:open-file`), the runtime intercepts the call.
    1.  It checks if the executing task possesses a valid capability token granting permission for that specific operation (potentially with constraints, e.g., URL domain matching).
    2.  If a valid token is present, the call proceeds.
    3.  If not, a security error is raised, and the call is denied.
*   **Token Management:** The runtime manages the lifecycle of capability tokens (issuance, revocation, expiry).
*   **Agent Authentication:** When a capability token for an agent interaction is granted and used (e.g., via `invoke`, `consume-stream`, `produce-to-stream`), the RTFS runtime is responsible for handling the actual authentication with the external agent. This is based on the authentication mechanisms specified in the agent's profile (e.g., API keys, OAuth2 tokens). The task itself does not handle these external credentials; the capability token signifies the runtime's authorization to proceed with the authenticated interaction. Refer to `language_semantics.md` (Section 8.4) for more details on runtime credential management.

## 4. Task Input and Intent Security (Placeholder)

*   **Goal:** Ensure the integrity and authenticity of the inputs and intents provided to tasks, preventing injection or tampering.
*   **Mechanism:** To be defined. Potentially involves signing inputs and intents, similar to execution trace signatures.

## 5. Capability Definition Format (Proposal)

To enable validation and granting, the capabilities listed in `:capabilities-required` and managed by the runtime need a defined structure. A map-based format is proposed:

```acl
;; Example Capability Definitions

;; Allow calling a specific tool
{ :type :tool-call
  :tool-name "tool:fetch-url:v1" }

;; Allow calling a tool with constraints on arguments
{ :type :tool-call
  :tool-name "tool:write-file"
  ;; Constraint: Only allow writing to paths starting with "/mnt/data/user_output/"
  :constraints {:args {:path [:string? "starts-with" "/mnt/data/user_output/"]}} }

;; Allow reading from a specific resource type (e.g., any file)
{ :type :resource-access
  :resource-type "FileHandle"
  :permissions [:read] }

;; Allow specific operations on a resource type with path constraints
{ :type :resource-access
  :resource-type "FileHandle"
  :permissions [:read :write]
  :constraints {:path [:string? "regex" "^/tmp/task-\d+/.*$"]} }

;; Allow access to a specific network host/port
{ :type :network-access
  :host "api.example.com"
  :port 443
  :protocols [:https] }

;; ---- NEW CAPABILITY TYPE FOR AGENT INTERACTIONS ----

;; Allow interaction with a specific capability from a specific agent profile
{ :type :agent-capability-access
  :agent-profile-id "polyglot-agent-v1" ;; Matches :id in agent-profile
  :capability-id "translate-text-batch-v1.2" ;; Matches :capability-id in agent's :capabilities list
  ;; Optional: specify which invocation types are allowed for this capability
  :invocation-types [:invoke] ;; Could be [:invoke], [:consume-stream], [:produce-to-stream] or a combination
                              ;; Defaults to all applicable types for the capability if omitted.
}

;; Allow interaction with any capability from a specific agent, with constraints on parameters
{ :type :agent-capability-access
  :agent-profile-id "user-preferences-agent-v2"
  :capability-id "*" ;; Wildcard: any capability from this agent
  ;; Optional: Constraints on the parameters sent to any capability of this agent.
  ;; These predicate schemas apply to the arguments map of 'invoke' or 'consume-stream',
  ;; or the item-expression of 'produce-to-stream'.
  :constraints {:params 
                  {:user-id [:= "current-session-user-id"] ;; Example: parameter must match a runtime-injected value
                   :data-sensitivity [:< 3]}}} ;; Example: parameter must be less than 3

;; Allow consuming a specific stream, constraining initial parameters
{ :type :agent-capability-access
  :agent-profile-id "market-data-feed-agent"
  :capability-id "live-stock-ticker-v1"
  :invocation-types [:consume-stream]
  :constraints {:params ;; Constraints on the parameters map for consume-stream
                  {:symbols [:vector-contains-only [:string-matches-regex "^[A-Z]{1,4}$"]] ;; Only allow valid stock symbols
                   :update-frequency [:in-range 1 60]}}} ;; Update frequency between 1 and 60 seconds
```

*   **:type**: Identifies the category of capability (e.g., `:tool-call`, `:resource-access`, `:network-access`, `:agent-capability-access`).
*   **Identifiers**: Specify the target (e.g., `:tool-name`, `:resource-type`, `:host`, `:agent-profile-id`, `:capability-id`).
*   **:permissions**: List allowed actions (e.g., `:read`, `:write`, `:execute`). For `:agent-capability-access`, permissions are implicitly defined by the agent's capability and optionally refined by `:invocation-types`.
*   **:invocation-types** (for `:agent-capability-access`): An optional list specifying which forms (`:invoke`, `:consume-stream`, `:produce-to-stream`) are permitted for the given agent capability.
*   **:constraints**: An optional map defining restrictions.
    *   For `:tool-call`: `{:args {:arg-name PredicateSchema}}`
    *   For `:resource-access` and `:network-access`: `{:attribute-name PredicateSchema}`
    *   For `:agent-capability-access`: `{:params {:param-name PredicateSchema}}`, applying to the parameters map/item expression of the interaction.
    *   The `PredicateSchema` uses the **exact same syntax** as defined in `type_system.md` for `[:and]` types and type refinements: `[PredicateName Arg1 Arg2 ...]`, where `PredicateName` is resolved to a validation function, and `Arg1...` are literal arguments.

This structured format allows the runtime/orchestrator to parse capability requests, match them against policies, and potentially generate constrained capability tokens.

**Refined Examples with Predicate Schemas:**

```acl
;; Allow calling a specific tool (no constraints)
{ :type :tool-call
  :tool-name "tool:fetch-url:v1" }

;; Allow calling a tool with constraints on arguments
{ :type :tool-call
  :tool-name "tool:write-file"
  ;; Constraint: Only allow writing to paths starting with "/mnt/data/user_output/"
  :constraints {:args {:path [:string-starts-with "/mnt/data/user_output/"]}} }

;; Allow reading from any file resource
{ :type :resource-access
  :resource-type "FileHandle"
  :permissions [:read] }

;; Allow read/write on files matching a specific path regex
{ :type :resource-access
  :resource-type "FileHandle"
  :permissions [:read :write]
  :constraints {:path [:string-matches-regex "^/tmp/task-\d+/.*$"]} }

;; Allow network access only to api.example.com on port 443 via HTTPS
{ :type :network-access
  :host "api.example.com"
  :port 443
  :protocols [:https]
  ;; Example constraint on the host itself (redundant here, but shows possibility)
  :constraints {:host [:= "api.example.com"]} }

;; Allow tool call only if the 'level' argument is less than 5
{ :type :tool-call
  :tool-name "tool:set-log-level"
  :constraints {:args {:level [:< 5]}} }

;; --- Agent Capability Access Examples (Reiteration for clarity in this section) ---

;; Allow invoking the 'translate-text-batch-v1.2' capability from 'polyglot-agent-v1'
{ :type :agent-capability-access
  :agent-profile-id "polyglot-agent-v1"
  :capability-id "translate-text-batch-v1.2"
  :invocation-types [:invoke] 
}

;; Allow consuming the 'live-translation-feed-v1.0' stream from 'polyglot-agent-v1'
;; with a constraint that the 'target-language' parameter must be :fr or :es
{ :type :agent-capability-access
  :agent-profile-id "polyglot-agent-v1"
  :capability-id "live-translation-feed-v1.0"
  :invocation-types [:consume-stream]
  :constraints {:params {:target-language [:enum :fr :es]}}
}
```

## 6. Implementation Considerations

*   **Key Management:** A secure system for managing agent keys and distributing public keys is crucial but external to the RTFS language spec itself.
*   **Canonicalization:** A strict, unambiguous canonical representation format for signed data is essential for reliable verification.
*   **Capability Definition:** A clear language or format for defining capabilities and their associated constraints is needed. This document proposes one such format.
*   **Runtime Integration:** Both signature verification and capability checking require tight integration with the RTFS runtime environment. The runtime must also handle the mapping of `:agent-capability-access` requests to network operations, including managing authentication with external agents as per their profiles.
*   **Agent Profile Trust and Verification:** The process of discovering and fetching agent profiles needs to be secure. Mechanisms to verify the authenticity and integrity of agent profiles (e.g., signed profiles, trusted registries) are important but may be further detailed in agent discovery protocol specifications.

This layered approach (signatures for auditability, capabilities for access control) provides a robust foundation for secure execution of RTFS tasks.
