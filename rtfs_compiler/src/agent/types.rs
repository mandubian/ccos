// RTFS Agent System - Data Types and Structures
// Implements AgentCard, AgentProfile, and DiscoveryQuery as per agent_discovery.md specification

use std::collections::HashMap;
use serde::{Deserialize, Serialize};

/// Agent Card - Data structure for agent discovery registry communication
/// Derived from agent-profile and optimized for registration/querying
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct AgentCard {
    /// Unique identifier for the agent
    pub agent_id: String,
    
    /// Optional URI to the full agent-profile document
    pub agent_profile_uri: Option<String>,
    
    /// Human-readable name of the agent
    pub name: String,
    
    /// Version of the agent (semantic versioning)
    pub version: String,
    
    /// Description of what the agent does
    pub description: String,
    
    /// List of capabilities the agent provides
    pub capabilities: Vec<AgentCapability>,
    
    /// Communication protocols and endpoints
    pub communication: AgentCommunication,
    
    /// Tags for discovery filtering
    pub discovery_tags: Vec<String>,
    
    /// Additional metadata
    pub metadata: HashMap<String, serde_json::Value>,
}

/// Capability offered by an agent
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct AgentCapability {
    /// Unique identifier for this capability
    pub capability_id: String,
    
    /// Human-readable description
    pub description: String,
    
    /// Reference to input schema (URI or inline)
    pub input_schema_ref: Option<String>,
    
    /// Reference to output schema (URI or inline)
    pub output_schema_ref: Option<String>,
    
    /// Version of this capability
    pub version: Option<String>,
    
    /// Additional capability metadata
    pub metadata: Option<HashMap<String, serde_json::Value>>,
}

/// Communication configuration for an agent
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct AgentCommunication {
    /// Supported protocols (e.g., ["http", "grpc"])
    pub protocols: Vec<String>,
    
    /// Communication endpoints
    pub endpoints: Vec<AgentEndpoint>,
}

/// Communication endpoint for an agent
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct AgentEndpoint {
    /// Protocol type (http, grpc, etc.)
    pub protocol: String,
    
    /// URI for the endpoint
    pub uri: String,
    
    /// Protocol-specific details
    pub details: Option<HashMap<String, serde_json::Value>>,
}

/// Discovery query parameters for (discover-agents ...) special form
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct DiscoveryQuery {
    /// Specific capability ID to search for
    pub capability_id: Option<String>,
    
    /// Version constraint for the capability
    pub version_constraint: Option<String>,
    
    /// Specific agent ID to find
    pub agent_id: Option<String>,
    
    /// Tags to filter by
    pub discovery_tags: Option<Vec<String>>,
    
    /// Custom query parameters
    pub discovery_query: Option<HashMap<String, serde_json::Value>>,
    
    /// Maximum number of results
    pub limit: Option<u32>,
}

/// Options for discovery operations
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct DiscoveryOptions {
    /// Specific registry URI to query
    pub registry_uri: Option<String>,
    
    /// Timeout in milliseconds
    pub timeout_ms: Option<u64>,
    
    /// Cache policy
    pub cache_policy: Option<CachePolicy>,
}

/// Cache policy for discovery operations
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum CachePolicy {
    UseCache,
    NoCache,
    RefreshCache,
}

/// Agent Profile - Canonical comprehensive description of an agent
/// This is the full RTFS-based agent definition (typically in agent-profile.rtfs)
#[derive(Debug, Clone, PartialEq)]
pub struct AgentProfile {
    /// Agent metadata
    pub metadata: AgentMetadata,
    
    /// Agent capabilities definitions
    pub capabilities: Vec<ProfileCapability>,
    
    /// Communication configuration
    pub communication: AgentCommunication,
    
    /// Requirements and dependencies
    pub requirements: Option<AgentRequirements>,
    
    /// Discovery configuration
    pub discovery: Option<AgentDiscoveryConfig>,
      /// Additional profile data
    pub extensions: Option<HashMap<String, serde_json::Value>>,
}

/// Agent metadata from profile
#[derive(Debug, Clone, PartialEq)]
pub struct AgentMetadata {
    pub id: String,
    pub name: String,
    pub version: String,
    pub description: String,
    pub author: Option<String>,
    pub license: Option<String>,
    pub tags: Vec<String>,
    pub created: Option<String>,
    pub updated: Option<String>,
}

/// Capability definition in agent profile
#[derive(Debug, Clone, PartialEq)]
pub struct ProfileCapability {
    pub id: String,
    pub name: String,
    pub description: String,
    pub version: String,    pub input_schema: Option<serde_json::Value>,
    pub output_schema: Option<serde_json::Value>,
    pub examples: Option<Vec<serde_json::Value>>,
    pub documentation: Option<String>,
}

/// Agent requirements and dependencies
#[derive(Debug, Clone, PartialEq)]
pub struct AgentRequirements {
    pub runtime_version: Option<String>,
    pub dependencies: Option<Vec<String>>,
    pub resources: Option<HashMap<String, serde_json::Value>>,
}

/// Discovery configuration for agent
#[derive(Debug, Clone, PartialEq)]
pub struct AgentDiscoveryConfig {
    pub registry_uris: Option<Vec<String>>,
    pub ttl_seconds: Option<u64>,
    pub auto_register: Option<bool>,
    pub health_check_endpoint: Option<String>,
}

/// JSON-RPC request for agent registry
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JsonRpcRequest {
    pub jsonrpc: String,
    pub method: String,
    pub params: serde_json::Value,
    pub id: String,
}

/// JSON-RPC response from agent registry
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JsonRpcResponse {
    pub jsonrpc: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub result: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<JsonRpcError>,
    pub id: String,
}

/// JSON-RPC error structure
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JsonRpcError {
    pub code: i32,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data: Option<serde_json::Value>,
}

/// Agent registration request
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RegistrationRequest {
    pub agent_card: AgentCard,
    pub endpoint_url: String,
    pub ttl_seconds: Option<u64>,
}

/// Agent registration response
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RegistrationResponse {
    pub status: String,
    pub agent_id: String,
    pub expires_at: String,
}

/// Discovery response from registry
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DiscoveryResponse {
    pub agents: Vec<AgentCard>,
    pub total_count: Option<u32>,
    pub query_time_ms: Option<u64>,
}

impl AgentCard {
    /// Create a new AgentCard with basic information
    pub fn new(agent_id: String, name: String, version: String, description: String) -> Self {
        Self {
            agent_id,
            agent_profile_uri: None,
            name,
            version,
            description,
            capabilities: Vec::new(),
            communication: AgentCommunication {
                protocols: Vec::new(),
                endpoints: Vec::new(),
            },
            discovery_tags: Vec::new(),
            metadata: HashMap::new(),
        }
    }
    
    /// Add a capability to this agent card
    pub fn add_capability(&mut self, capability: AgentCapability) {
        self.capabilities.push(capability);
    }
      /// Add a communication endpoint
    pub fn add_endpoint(&mut self, endpoint: AgentEndpoint) {
        if !self.communication.protocols.contains(&endpoint.protocol) {
            self.communication.protocols.push(endpoint.protocol.clone());
        }
        self.communication.endpoints.push(endpoint);
    }
    
    /// Check if this agent has a specific capability
    pub fn has_capability(&self, capability_id: &str) -> bool {
        self.capabilities.iter().any(|cap| cap.capability_id == capability_id)
    }
    
    /// Get a specific capability by ID
    pub fn get_capability(&self, capability_id: &str) -> Option<&AgentCapability> {
        self.capabilities.iter().find(|cap| cap.capability_id == capability_id)
    }
}

impl AgentCapability {
    /// Create a new capability
    pub fn new(capability_id: String, description: String) -> Self {
        Self {
            capability_id,
            description,
            input_schema_ref: None,
            output_schema_ref: None,
            version: None,
            metadata: None,
        }
    }
    
    /// Set version for this capability
    pub fn with_version(mut self, version: String) -> Self {
        self.version = Some(version);
        self
    }
    
    /// Set input schema reference
    pub fn with_input_schema(mut self, schema_ref: String) -> Self {
        self.input_schema_ref = Some(schema_ref);
        self
    }
    
    /// Set output schema reference
    pub fn with_output_schema(mut self, schema_ref: String) -> Self {
        self.output_schema_ref = Some(schema_ref);
        self
    }
}

impl AgentEndpoint {
    /// Create a new endpoint
    pub fn new(protocol: String, uri: String) -> Self {
        Self {
            protocol,
            uri,
            details: None,
        }
    }
    
    /// Add protocol-specific details
    pub fn with_details(mut self, details: HashMap<String, serde_json::Value>) -> Self {
        self.details = Some(details);
        self
    }
}

impl DiscoveryQuery {
    /// Create a new empty discovery query
    pub fn new() -> Self {
        Self {
            capability_id: None,
            version_constraint: None,
            agent_id: None,
            discovery_tags: None,
            discovery_query: None,
            limit: None,
        }
    }
    
    /// Set capability ID filter
    pub fn with_capability_id(mut self, capability_id: String) -> Self {
        self.capability_id = Some(capability_id);
        self
    }
    
    /// Set agent ID filter
    pub fn with_agent_id(mut self, agent_id: String) -> Self {
        self.agent_id = Some(agent_id);
        self
    }
    
    /// Set discovery tags filter
    pub fn with_tags(mut self, tags: Vec<String>) -> Self {
        self.discovery_tags = Some(tags);
        self
    }
    
    /// Set result limit
    pub fn with_limit(mut self, limit: u32) -> Self {
        self.limit = Some(limit);
        self
    }
}

impl Default for DiscoveryQuery {
    fn default() -> Self {
        Self::new()
    }
}

impl Default for DiscoveryOptions {
    fn default() -> Self {
        Self {
            registry_uri: None,
            timeout_ms: Some(10000), // 10 second default timeout
            cache_policy: Some(CachePolicy::UseCache),
        }
    }
}
