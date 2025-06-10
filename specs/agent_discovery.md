\
# Agent Discovery in RTFS

## 1. Introduction

Effective multi-agent systems require robust mechanisms for agents to discover each other's capabilities and connection details. In RTFS, this is facilitated by an Agent Discovery Registry and standardized data structures for describing agent characteristics. This document outlines the protocol for interacting with such a registry and clarifies the relationship between an agent's canonical profile and the data exchanged during discovery.

The primary special form used for explicit discovery is `(discover-agents ...)`, as defined in `syntax_spec.md` and `language_semantics.md`. This form interacts with a discovery registry service.

## 2. `agent-profile` vs. `agent_card`

It's crucial to distinguish between two related concepts:

*   **`agent-profile`**: This is the canonical and comprehensive description of an agent's capabilities, identity, communication endpoints, and other metadata. It is defined within an RTFS document (typically in a dedicated file like `agent-profile.rtfs` or embedded within a larger system specification) using RTFS syntax, as outlined in `syntax_spec.md`. The `agent-profile` is the single source of truth for an agent's definition.

*   **`agent_card`**: This is a data structure, typically represented in JSON, that is derived from an agent's `agent-profile`. The `agent_card` is specifically formatted for communication with an Agent Discovery Registry. It contains a subset of the information from the `agent-profile`, optimized for registration and querying.

An agent, upon initialization or deployment, reads its `agent-profile`, constructs an `agent_card`, and uses this card to register with one or more discovery registries.

### Example `agent_card` Structure (derived from `agent-profile`):

```json
{
  "agent_id": "unique-agent-identifier-123", // From agent-profile :id
  "agent_profile_uri": "http://agent.example.com/.well-known/rtfs-agent-profile.json", // Optional: URI to the full agent-profile
  "name": "DataProcessorAgent",             // From agent-profile :metadata :name
  "version": "1.2.1",                        // From agent-profile :metadata :version
  "description": "Processes and analyzes datasets.", // From agent-profile :metadata :description
  "capabilities": [                          // Derived from agent-profile :capabilities
    {
      "capability_id": "csv_processing",
      "description": "Process CSV files",
      "input_schema_ref": "schemas/csv_input.json",  // Reference or inline schema
      "output_schema_ref": "schemas/csv_output.json"
    },
    {
      "capability_id": "image_resizing",
      "description": "Resize JPG and PNG images",
      "input_schema_ref": "schemas/image_input.json",
      "output_schema_ref": "schemas/image_output.json"
    }
  ],
  "communication": {                         // From agent-profile :communication
    "protocols": ["http", "grpc"],
    "endpoints": [
      {
        "protocol": "http",
        "uri": "http://agent-host:8080/rtfs",
        "details": { "methods": ["POST"] }
      },
      {
        "protocol": "grpc",
        "uri": "grpc://agent-host:50051"
      }
    ]
  },
  "discovery_tags": ["data-processing", "images", "version-1.2"], // From agent-profile :metadata :tags
  "metadata": {                              // Other relevant metadata from agent-profile :metadata
    "owner": "DataTeam",
    "last_updated_timestamp": "2023-10-27T10:30:00Z"
  }
}
```
The `agent_card` omits runtime-specific or highly detailed contract information present in the `agent-profile` unless directly relevant for discovery (e.g., high-level capability matching). The `agent_profile_uri` field provides a direct link to the canonical RTFS agent profile document if more detail is needed. The `requirements` field from the original proposal\'s `agent_card` (related to Python versions, etc.) is not directly part of the `agent_card` for discovery, as these are considered implementation details of the agent itself, rather than primary discovery criteria. The focus is on *what* the agent can do and *how* to reach it.

## 3. Agent Discovery Registry Protocol

The Agent Discovery Registry provides a centralized (or federated) service where agents can register their `agent_card`s and other agents can query for available capabilities. Communication with the registry typically uses a JSON-RPC like protocol over HTTP/S.

### 3.1. Agent Registration

Agents register themselves with the discovery registry to make their capabilities known.

**Method**: `rtfs.registry.register`

**Parameters**:

```json
{
  "agent_card": { /* The agent_card object as described above */ },
  "endpoint_url": "http://agent.example.com/rtfs", // The primary endpoint for the agent, can be used by the registry for health checks.
  "ttl_seconds": 3600 // Time-to-live in seconds. The registration will expire after this duration unless refreshed.
}
```

**Returns**:
A success or error response.
```json
// Success
{
  "jsonrpc": "2.0",
  "result": {
    "status": "registered",
    "agent_id": "unique-agent-identifier-123",
    "expires_at": "2023-10-27T11:30:00Z"
  },
  "id": "req-001"
}

// Error
{
  "jsonrpc": "2.0",
  "error": {
    "code": -32001,
    "message": "Registration failed: Invalid agent_card format."
  },
  "id": "req-001"
}
```

### 3.2. Agent Discovery Query

Agents (or RTFS task executors) query the registry to find other agents based on various criteria. This is the mechanism underlying the `(discover-agents ...)` special form.

**Method**: `rtfs.registry.discover`

**Parameters**:
The parameters mirror the criteria map of the `(discover-agents ...)` special form.

```json
{
  "capability_id": "csv_processing",      // Optional: Specific capability ID
  "agent_id": "specific-agent-known-id",  // Optional: Specific agent ID
  "discovery_tags": ["data", "production"], // Optional: List of tags (logical AND or OR, registry dependent)
  "discovery_query": { "custom_field": "value", "text_search": "image manipulation" }, // Optional: JSON object for structured query, aligning with RTFS :discovery-query :map. Can include free-text search under a conventional key.
  "version_constraint": ">=1.2.0 <2.0.0",      // Optional: Semantic version constraint for the agent or capability (aligns with RTFS :version-constraint).
  "limit": 10                       // Optional: Maximum number of results to return (aligns with RTFS :limit).
}
```
*Note: The exact interpretation of combined criteria (e.g., `capability_id` AND `discovery_tags`) is determined by the registry implementation, but typically implies an AND condition.*

**Returns**:
A list of `agent_card`s matching the query. The structure of each entry in the `"agents"` array will conform to the `agent_card` structure defined in Section 2.
```json
// Success
{
  "jsonrpc": "2.0",
  "result": {
    "agents": [
      { 
        "agent_id": "unique-agent-identifier-123",
        "agent_profile_uri": "http://agent.example.com/.well-known/rtfs-agent-profile.json",
        "name": "DataProcessorAgent",
        "version": "1.2.1",
        "description": "Processes and analyzes datasets.",
        "capabilities": [
          {
            "capability_id": "csv_processing",
            "description": "Process CSV files",
            "input_schema_ref": "schemas/csv_input.json",
            "output_schema_ref": "schemas/csv_output.json"
          }
          // ... other capabilities ...
        ],
        "communication": {
          "protocols": ["http"],
          "endpoints": [
            { "protocol": "http", "uri": "http://agent-host:8080/rtfs", "details": { "methods": ["POST"] } }
            // ... other endpoints ...
          ]
        },
        "discovery_tags": ["data-processing", "images", "version-1.2"],
        "metadata": {
          "owner": "DataTeam",
          "last_updated_timestamp": "2023-10-27T10:30:00Z"
          // ... other metadata from agent_card ...
        }
      }
      // ... other matching agent_cards ...
    ]
  },
  "id": "req-002"
}

// Error or No Results
{
  "jsonrpc": "2.0",
  "result": { // Still a result, but with an empty list
    "agents": []
  },
  "id": "req-002"
}
// Or an error object if the query itself was malformed
// {
//   "jsonrpc": "2.0",
//   "error": { ... },
//   "id": "req-002"
// }
```

The `agent_card`s returned by the discovery query should contain enough information for the requesting agent to decide if it wants to interact and how to establish communication (i.e., primarily `agent_id`, `name`, `description`, `capabilities` overview, and `communication` details, as detailed in the `agent_card` structure in Section 2).

## 4. Health Checks and De-registration (Informative)

While not strictly part of the discovery query or registration payload itself, a robust registry implementation would also include:
*   **Health Checks**: The registry might periodically ping the `endpoint_url` provided during registration to ensure the agent is still active. Agents failing health checks might be temporarily or permanently de-listed.
*   **Explicit De-registration**: An agent should ideally de-register itself when shutting down gracefully. This could be a method like `rtfs.registry.deregister` taking the `agent_id`.
*   **TTL Expiry**: As mentioned, registrations expire after their TTL, ensuring stale entries are eventually removed. Agents are responsible for refreshing their registration.

These aspects ensure the discovery service remains accurate and reliable.
