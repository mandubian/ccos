\
<!-- 
**ARCHIVED DOCUMENT**

**Date Archived:** June 8, 2025

**Reason:** The core concepts and proposals outlined in this document have been superseded by and integrated into the main RTFS specification documents, primarily:
- `specs/agent_discovery.md`: Details the agent card structure, discovery mechanisms, and the `discover-agents` special form.
- `specs/syntax_spec.md`: Incorporates syntax for agent profiles (`agent-profile`), declaring required capabilities in tasks, and the `discover-agents` special form.
- `specs/language_semantics.md`: Defines the runtime behavior of `discover-agents` and other agent interaction forms.

These documents should be considered the current source of truth for RTFS agent discovery. This proposal is retained for historical context.
-->

# RTFS Agent Discovery Protocol - Implementation Proposal

## Overview

This proposal defines how RTFS agents can discover each other's capabilities and establish communication channels, addressing the most critical gap identified in the agent communication analysis.

## Problem Statement

Currently, RTFS lacks standardized mechanisms for:
- Agents to advertise their capabilities
- Discovery of available agents in a network/environment
- Capability negotiation before task delegation
- Dynamic agent registration and deregistration

## Proposed Solution

### 1. Agent Card Specification

Extend RTFS specification with an `agent_card` section that defines agent capabilities:

```yaml
# agent_card.rtfs
agent_card:
  name: "DataProcessor"
  version: "1.2.0"
  description: "Specialized agent for data transformation and analysis"
  
  capabilities:
    - name: "csv_processing"
      description: "Process CSV files with various transformations"
      input_schema:
        type: "object"
        properties:
          file_path: { type: "string" }
          operations: { type: "array", items: { type: "string" } }
      output_schema:
        type: "object"
        properties:
          processed_file: { type: "string" }
          summary: { type: "object" }
    
    - name: "data_validation"
      description: "Validate data against predefined schemas"
      input_schema:
        type: "object"
        properties:
          data: { type: "object" }
          schema: { type: "object" }
      output_schema:
        type: "object"
        properties:
          valid: { type: "boolean" }
          errors: { type: "array" }

  communication:
    protocols: ["http", "websocket"]
    endpoints:
      - protocol: "http"
        url: "http://localhost:8080/api/rtfs"
        methods: ["POST"]
      - protocol: "websocket"
        url: "ws://localhost:8080/ws/rtfs"
    
  metadata:
    author: "DataTeam"
    tags: ["data", "csv", "validation"]
    requirements:
      - "python >= 3.8"
      - "pandas >= 1.0"
    resource_limits:
      max_memory_mb: 1024
      max_cpu_percent: 50
```

### 2. Discovery Registry Protocol

Define a registry system for agent discovery:

#### Registry API Specification

```json
{
  "jsonrpc": "2.0",
  "method": "rtfs.registry.register",
  "params": {
    "agent_card": { /* AgentCard object */ },
    "endpoint_url": "http://agent.example.com/rtfs",
    "ttl_seconds": 3600
  },
  "id": "reg-001"
}
```

#### Discovery Query API

```json
{
  "jsonrpc": "2.0",
  "method": "rtfs.registry.discover",
  "params": {
    "capabilities": ["csv_processing"],
    "tags": ["data"],
    "version_range": ">=1.0.0",
    "max_results": 10
  },
  "id": "disc-001"
}
```

### 3. RTFS Language Extensions

#### New Task Declaration Syntax

```yaml
task: analyze_customer_data
description: "Analyze customer data using specialized data processing agent"

agent_requirements:
  capabilities: ["csv_processing", "data_validation"]
  tags: ["data"]
  version: ">=1.0.0"
  resource_limits:
    max_memory_mb: 2048

steps:
  - name: "discover_agent"
    type: "agent_discovery"
    query:
      capabilities: ["csv_processing"]
      tags: ["data"]
    output: "discovered_agents"
  
  - name: "delegate_processing"
    type: "agent_delegation"
    agent: "{{ discovered_agents[0] }}"
    task_spec:
      name: "process_csv"
      inputs:
        file_path: "{{ inputs.data_file }}"
        operations: ["clean", "validate", "transform"]
    output: "processing_result"
  
  - name: "verify_results"
    type: "local_execution"
    command: "python verify_output.py {{ processing_result.processed_file }}"
```

### 4. Implementation Architecture

#### Registry Service Component

```rust
// rtfs_compiler/src/registry/mod.rs
pub struct AgentRegistry {
    agents: HashMap<String, AgentCard>,
    endpoints: HashMap<String, AgentEndpoint>,
    discovery_index: CapabilityIndex,
}

impl AgentRegistry {
    pub async fn register_agent(&mut self, card: AgentCard, endpoint: AgentEndpoint) -> Result<(), RegistryError> {
        // Implementation for agent registration
    }
    
    pub async fn discover_agents(&self, query: DiscoveryQuery) -> Result<Vec<AgentCard>, RegistryError> {
        // Implementation for capability-based discovery
    }
    
    pub async fn health_check(&self) -> Result<Vec<AgentStatus>, RegistryError> {
        // Implementation for agent health monitoring
    }
}
```

#### Agent Communication Client

```rust
// rtfs_compiler/src/communication/client.rs
pub struct RTFSAgentClient {
    http_client: reqwest::Client,
    websocket_client: Option<WebSocketClient>,
    registry: Arc<AgentRegistry>,
}

impl RTFSAgentClient {
    pub async fn delegate_task(&self, agent_id: &str, task_spec: TaskSpec) -> Result<TaskResult, CommunicationError> {
        // Implementation for task delegation
    }
    
    pub async fn stream_task_updates(&self, task_id: &str) -> Result<impl Stream<Item = TaskUpdate>, CommunicationError> {
        // Implementation for streaming task updates
    }
}
```

### 5. Configuration Integration

#### RTFS Runtime Configuration

```yaml
# rtfs_config.yaml
runtime:
  agent_discovery:
    enabled: true
    registry_url: "http://registry.example.com"
    local_registry: true
    discovery_timeout_ms: 5000
    health_check_interval_ms: 30000
  
  communication:
    protocols: ["http", "websocket"]
    timeouts:
      connection_ms: 10000
      request_ms: 30000
      task_delegation_ms: 300000
    
  agent_card:
    auto_register: true
    ttl_seconds: 3600
    capabilities_file: "./agent_capabilities.rtfs"
```

### 6. Backward Compatibility

- Agent discovery features are opt-in via configuration
- Existing RTFS specifications continue to work without modification
- New agent-related syntax is ignored by older RTFS compilers
- Discovery protocols use separate namespace (`rtfs.registry.*`)

### 7. Security Considerations

- Agent authentication via JWT tokens or API keys
- TLS encryption for all agent-to-agent communication
- Capability-based access control for task delegation
- Registry access controls and rate limiting

## Implementation Timeline

### Phase 1 (Weeks 1-4): Core Specification
- Define AgentCard JSON schema
- Create discovery query specification
- Extend RTFS syntax for agent requirements

### Phase 2 (Weeks 5-8): Registry Implementation
- Build registry service in Rust
- Implement HTTP API endpoints
- Create agent registration/deregistration logic

### Phase 3 (Weeks 9-12): Client Integration
- Integrate discovery client into RTFS compiler
- Add agent delegation step types
- Implement communication protocols

### Phase 4 (Weeks 13-16): Testing & Documentation
- Create comprehensive test suite
- Write integration examples
- Update RTFS specification documentation

## Success Metrics

1. **Functional Requirements**
   - Agents can register and discover each other
   - Task delegation works across agent boundaries
   - Registry maintains accurate agent status

2. **Performance Requirements**
   - Discovery queries complete within 5 seconds
   - Registry supports 1000+ concurrent agents
   - Agent registration has 99.9% success rate

3. **Usability Requirements**
   - Simple RTFS syntax for agent interaction
   - Clear error messages for discovery failures
   - Comprehensive documentation and examples

## Future Extensions

- Federated registry support for multi-organization deployments
- Advanced capability matching with semantic similarity
- Load balancing and failover for agent selection
- Integration with container orchestration platforms

This proposal establishes the foundation for RTFS agent communication while maintaining the language's core simplicity and declarative nature.
