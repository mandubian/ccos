# RTFS 2.0 Capability System Specification

**Version**: 2.0.0  
**Status**: Stable  
**Date**: July 2025  
**Based on**: Issue #43 Implementation

## 1. Overview

The RTFS 2.0 Capability System provides a secure, extensible framework for dynamic capability discovery, registration, and execution. It supports multiple provider types, schema validation, attestation, and network-based discovery.

## 2. Core Architecture

### 2.1 Capability Marketplace

The `CapabilityMarketplace` serves as the central hub for capability management:

```rust
pub struct CapabilityMarketplace {
    capabilities: Arc<RwLock<HashMap<String, CapabilityManifest>>>,
    discovery_agents: Vec<Box<dyn CapabilityDiscovery>>,
    capability_registry: Arc<RwLock<CapabilityRegistry>>,
    executors: HashMap<TypeId, Arc<dyn CapabilityExecutor>>,
    network_registry: Option<NetworkRegistryConfig>,
    type_validator: Arc<TypeValidator>,
}
```

### 2.2 Capability Manifest

Each capability is described by a `CapabilityManifest`:

```rust
pub struct CapabilityManifest {
    pub id: String,
    pub name: String,
    pub description: String,
    pub provider: ProviderType,
    pub version: String,
    pub input_schema: Option<TypeExpr>,
    pub output_schema: Option<TypeExpr>,
    pub attestation: Option<CapabilityAttestation>,
    pub provenance: Option<CapabilityProvenance>,
    pub permissions: Vec<String>,
    pub metadata: HashMap<String, String>,
}
```

## 3. Provider Types

### 3.1 Local Provider

Executes capabilities within the local RTFS runtime:

```rust
pub struct LocalCapability {
    pub handler: Arc<dyn Fn(&Value) -> RuntimeResult<Value> + Send + Sync>,
}
```

**Registration:**
```rust
marketplace.register_local_capability(
    "capability_id".to_string(),
    "Capability Name".to_string(),
    "Description".to_string(),
    Arc::new(|inputs| { /* implementation */ })
).await?;
```

### 3.2 HTTP Provider

Executes capabilities via HTTP APIs:

```rust
pub struct HttpCapability {
    pub base_url: String,
    pub auth_token: Option<String>,
    pub timeout_ms: u64,
}
```

**Registration:**
```rust
marketplace.register_http_capability(
    "http_capability".to_string(),
    "HTTP Capability".to_string(),
    "Description".to_string(),
    "https://api.example.com".to_string(),
    Some("auth_token".to_string())
).await?;
```

### 3.3 MCP Provider

Executes capabilities via Model Context Protocol:

```rust
pub struct MCPCapability {
    pub server_url: String,
    pub tool_name: String,
    pub timeout_ms: u64,
}
```

**Registration:**
```rust
marketplace.register_mcp_capability(
    "mcp_capability".to_string(),
    "MCP Capability".to_string(),
    "Description".to_string(),
    "http://localhost:3000".to_string(),
    "tool_name".to_string(),
    30000
).await?;
```

### 3.4 A2A Provider

Executes capabilities via Agent-to-Agent communication:

```rust
pub struct A2ACapability {
    pub agent_id: String,
    pub endpoint: String,
    pub protocol: String, // "http", "websocket", "grpc"
    pub timeout_ms: u64,
}
```

**Registration:**
```rust
marketplace.register_a2a_capability(
    "a2a_capability".to_string(),
    "A2A Capability".to_string(),
    "Description".to_string(),
    "agent_123".to_string(),
    "http://agent.example.com".to_string(),
    "http".to_string(),
    30000
).await?;
```

### 3.5 Plugin Provider

Executes capabilities via dynamic plugins:

```rust
pub struct PluginCapability {
    pub plugin_path: String,
    pub function_name: String,
}
```

**Registration:**
```rust
marketplace.register_plugin_capability(
    "plugin_capability".to_string(),
    "Plugin Capability".to_string(),
    "Description".to_string(),
    "/path/to/plugin.so".to_string(),
    "function_name".to_string()
).await?;
```

### 3.6 RemoteRTFS Provider

Executes capabilities on remote RTFS instances:

```rust
pub struct RemoteRTFSCapability {
    pub endpoint: String,
    pub timeout_ms: u64,
    pub auth_token: Option<String>,
}
```

**Registration:**
```rust
marketplace.register_remote_rtfs_capability(
    "remote_capability".to_string(),
    "Remote Capability".to_string(),
    "Description".to_string(),
    "http://remote-rtfs.example.com".to_string(),
    Some("auth_token".to_string()),
    30000
).await?;
```

### 3.7 Streaming Provider

Executes streaming capabilities:

```rust
pub struct StreamCapabilityImpl {
    pub provider: StreamingProvider,
    pub stream_type: StreamType,
    pub input_schema: Option<TypeExpr>,
    pub output_schema: Option<TypeExpr>,
    pub supports_progress: bool,
    pub supports_cancellation: bool,
    pub bidirectional_config: Option<BidirectionalConfig>,
    pub duplex_config: Option<DuplexChannels>,
    pub stream_config: Option<StreamConfig>,
}
```

**Stream Types:**
- `Source`: Produces data to consumers
- `Sink`: Consumes data from producers
- `Transform`: Transforms data
- `Bidirectional`: Bidirectional stream
- `Duplex`: Separate input and output channels

## 4. Schema Validation

### 4.1 RTFS Native Type System

All capabilities support RTFS native type validation using `TypeExpr`. Here are examples showing the correspondence between Rust syntax and RTFS syntax:

#### 4.1.1 Simple String Schema

**Rust Syntax:**
```rust
let input_schema = TypeExpr::Primitive(PrimitiveType::String);
```

**RTFS Syntax:**
```rtfs
string
```

#### 4.1.2 Map Schema with Required Fields

**Rust Syntax:**
```rust
let input_schema = TypeExpr::Map {
    entries: vec![
        MapTypeEntry {
            key: Keyword("name".to_string()),
            value_type: Box::new(TypeExpr::Primitive(PrimitiveType::String)),
            optional: false,
        },
        MapTypeEntry {
            key: Keyword("age".to_string()),
            value_type: Box::new(TypeExpr::Primitive(PrimitiveType::Number)),
            optional: false,
        }
    ],
    wildcard: None,
};
```

**RTFS Syntax:**
```rtfs
[:map [:name string] [:age float]]
```

#### 4.1.3 Map Schema with Optional Fields

**Rust Syntax:**
```rust
let input_schema = TypeExpr::Map {
    entries: vec![
        MapTypeEntry {
            key: Keyword("name".to_string()),
            value_type: Box::new(TypeExpr::Primitive(PrimitiveType::String)),
            optional: false,
        },
        MapTypeEntry {
            key: Keyword("email".to_string()),
            value_type: Box::new(TypeExpr::Primitive(PrimitiveType::String)),
            optional: true,
        }
    ],
    wildcard: None,
};
```

**RTFS Syntax:**
```rtfs
[:map [:name string] [:email string ?]]
```

#### 4.1.4 List Schema

**Rust Syntax:**
```rust
let input_schema = TypeExpr::List {
    element_type: Box::new(TypeExpr::Primitive(PrimitiveType::String)),
};
```

**RTFS Syntax:**
```rtfs
[:vector string]
```

#### 4.1.5 Union Schema

**Rust Syntax:**
```rust
let input_schema = TypeExpr::Union {
    variants: vec![
        TypeExpr::Primitive(PrimitiveType::String),
        TypeExpr::Primitive(PrimitiveType::Number),
    ],
};
```

**RTFS Syntax:**
```rtfs
[:union string float]
```

#### 4.1.6 Complex Nested Schema

**Rust Syntax:**
```rust
let input_schema = TypeExpr::Map {
    entries: vec![
        MapTypeEntry {
            key: Keyword("user".to_string()),
            value_type: Box::new(TypeExpr::Map {
                entries: vec![
                    MapTypeEntry {
                        key: Keyword("name".to_string()),
                        value_type: Box::new(TypeExpr::Primitive(PrimitiveType::String)),
                        optional: false,
                    },
                    MapTypeEntry {
                        key: Keyword("preferences".to_string()),
                        value_type: Box::new(TypeExpr::List {
                            element_type: Box::new(TypeExpr::Primitive(PrimitiveType::String)),
                        }),
                        optional: true,
                    }
                ],
                wildcard: None,
            }),
            optional: false,
        },
        MapTypeEntry {
            key: Keyword("settings".to_string()),
            value_type: Box::new(TypeExpr::Map {
                entries: vec![],
                wildcard: Some(Box::new(TypeExpr::Primitive(PrimitiveType::String))),
            }),
            optional: true,
        }
    ],
    wildcard: None,
};
```

**RTFS Syntax:**
```rtfs
[:map [:user [:map [:name string] [:preferences [:vector string] ?]]] [:settings [:map [:* string] ?]]]
```

#### 4.1.7 Output Schema Example

**Rust Syntax:**
```rust
let output_schema = TypeExpr::Map {
    entries: vec![
        MapTypeEntry {
            key: Keyword("result".to_string()),
            value_type: Box::new(TypeExpr::Primitive(PrimitiveType::String)),
            optional: false,
        },
        MapTypeEntry {
            key: Keyword("status".to_string()),
            value_type: Box::new(TypeExpr::Primitive(PrimitiveType::String)),
            optional: false,
        }
    ],
    wildcard: None,
};
```

**RTFS Syntax:**
```rtfs
[:map [:result string] [:status string]]
```

### 4.2 Schema Correspondence Reference

The RTFS type system provides three equivalent representations for schemas:

#### 4.2.1 Primitive Types

| Rust TypeExpr | RTFS Syntax | JSON Schema |
|---------------|-------------|-------------|
| `TypeExpr::Primitive(PrimitiveType::String)` | `string` | `{"type": "primitive", "primitive": "string"}` |
| `TypeExpr::Primitive(PrimitiveType::Float)` | `float` | `{"type": "primitive", "primitive": "number"}` |
| `TypeExpr::Primitive(PrimitiveType::Int)` | `int` | `{"type": "primitive", "primitive": "integer"}` |
| `TypeExpr::Primitive(PrimitiveType::Bool)` | `bool` | `{"type": "primitive", "primitive": "boolean"}` |
| `TypeExpr::Primitive(PrimitiveType::Nil)` | `nil` | `{"type": "primitive", "primitive": "null"}` |

#### 4.2.2 Complex Types

| Rust TypeExpr | RTFS Syntax | JSON Schema |
|---------------|-------------|-------------|
| `TypeExpr::Vector(Box::new(TypeExpr::Primitive(PrimitiveType::String)))` | `[:vector string]` | `{"type": "array", "items": {"type": "primitive", "primitive": "string"}}` |
| `TypeExpr::Union(vec![TypeExpr::Primitive(PrimitiveType::String), TypeExpr::Primitive(PrimitiveType::Float)])` | `[:union string float]` | `{"type": "union", "variants": [{"type": "primitive", "primitive": "string"}, {"type": "primitive", "primitive": "number"}]}` |

#### 4.2.3 Map Types

**Required Fields Only:**
```rust
TypeExpr::Map {
    entries: vec![
        MapTypeEntry {
            key: Keyword("name".to_string()),
            value_type: Box::new(TypeExpr::Primitive(PrimitiveType::String)),
            optional: false,
        }
    ],
    wildcard: None,
}
```
```rtfs
[:map [:name string]]
```
```json
{
  "type": "map",
  "entries": [
    {
      "key": "name",
      "value_type": {"type": "primitive", "primitive": "string"},
      "optional": false
    }
  ]
}
```

**With Optional Fields:**
```rust
TypeExpr::Map {
    entries: vec![
        MapTypeEntry {
            key: Keyword("name".to_string()),
            value_type: Box::new(TypeExpr::Primitive(PrimitiveType::String)),
            optional: false,
        },
        MapTypeEntry {
            key: Keyword("email".to_string()),
            value_type: Box::new(TypeExpr::Primitive(PrimitiveType::String)),
            optional: true,
        }
    ],
    wildcard: None,
}
```
```rtfs
[:map [:name string] [:email string ?]]
```
```json
{
  "type": "map",
  "entries": [
    {
      "key": "name",
      "value_type": {"type": "primitive", "primitive": "string"},
      "optional": false
    },
    {
      "key": "email",
      "value_type": {"type": "primitive", "primitive": "string"},
      "optional": true
    }
  ]
}
```

**With Wildcard:**
```rust
TypeExpr::Map {
    entries: vec![],
    wildcard: Some(Box::new(TypeExpr::Primitive(PrimitiveType::String))),
}
```
```rtfs
[:map [:* string]]
```
```json
{
  "type": "map",
  "entries": [],
  "wildcard": {"type": "primitive", "primitive": "string"}
}
```

### 4.3 Validation Methods

**With Schema Validation:**
```rust
marketplace.register_local_capability_with_schema(
    "validated_capability".to_string(),
    "Validated Capability".to_string(),
    "Description".to_string(),
    handler,
    Some(input_schema),
    Some(output_schema)
).await?;
```

**Execution with Validation:**
```rust
let result = marketplace.execute_with_validation(
    "capability_id",
    &params
).await?;
```

## 5. Security Features

### 5.1 Capability Attestation

```rust
pub struct CapabilityAttestation {
    pub signature: String,
    pub authority: String,
    pub created_at: DateTime<Utc>,
    pub expires_at: Option<DateTime<Utc>>,
    pub metadata: HashMap<String, String>,
}
```

### 5.2 Capability Provenance

```rust
pub struct CapabilityProvenance {
    pub source: String,
    pub version: Option<String>,
    pub content_hash: String,
    pub custody_chain: Vec<String>,
    pub registered_at: DateTime<Utc>,
}
```

### 5.3 Content Hashing

All capabilities are assigned a content hash for integrity verification:

```rust
fn compute_content_hash(&self, content: &str) -> String {
    use sha2::{Sha256, Digest};
    let mut hasher = Sha256::new();
    hasher.update(content.as_bytes());
    format!("{:x}", hasher.finalize())
}
```

## 6. Network Discovery

### 6.1 Discovery Agents

The system supports pluggable discovery agents:

```rust
pub trait CapabilityDiscovery: Send + Sync {
    async fn discover(&self) -> Result<Vec<CapabilityManifest>, RuntimeError>;
}
```

### 6.2 Network Registry Configuration

```rust
pub struct NetworkRegistryConfig {
    pub endpoint: String,
    pub auth_token: Option<String>,
    pub refresh_interval: u64,
    pub verify_attestations: bool,
}
```

### 6.3 Discovery Protocol

**Discovery Query:**
```json
{
  "query": "capability_name",
  "limit": 10,
  "timestamp": "2025-07-24T10:30:00Z"
}
```

**Discovery Response:**
```json
{
  "capabilities": [
    {
      "id": "capability_id",
      "name": "Capability Name",
      "description": "Description",
      "endpoint": "https://api.example.com",
      "attestation": { /* attestation data */ },
      "provenance": { /* provenance data */ }
    }
  ]
}
```

## 7. Execution Framework

### 7.1 Capability Executors

Extensible executor system for different provider types:

```rust
pub trait CapabilityExecutor: Send + Sync {
    fn provider_type_id(&self) -> TypeId;
    async fn execute(&self, provider: &ProviderType, inputs: &Value) -> RuntimeResult<Value>;
}
```

### 7.2 Execution Flow

1. **Lookup**: Find capability by ID
2. **Validation**: Validate input schema if present
3. **Execution**: Execute via appropriate provider
4. **Validation**: Validate output schema if present
5. **Return**: Return result with provenance

### 7.3 Error Handling

Comprehensive error handling with specific error types:

```rust
pub enum RuntimeError {
    CapabilityNotFound { id: String },
    SchemaValidationError { message: String },
    ExecutionError { message: String },
    NetworkError { message: String },
    AttestationError { message: String },
    // ... other error types
}
```

## 8. Usage Examples

### 8.1 Basic Capability Registration

```rust
// Create marketplace
let registry = Arc::new(RwLock::new(CapabilityRegistry::new()));
let marketplace = CapabilityMarketplace::new(registry);

// Register local capability
marketplace.register_local_capability(
    "greet".to_string(),
    "Greeting Capability".to_string(),
    "Returns a greeting message".to_string(),
    Arc::new(|inputs| {
        if let Value::Map(map) = inputs {
            if let Some(Value::String(name)) = map.get(&MapKey::Keyword("name".to_string())) {
                Ok(Value::String(format!("Hello, {}!", name)))
            } else {
                Ok(Value::String("Hello, World!".to_string()))
            }
        } else {
            Ok(Value::String("Invalid input".to_string()))
        }
    })
).await?;
```

### 8.2 Capability Registration with Schema Validation

```rust
// Define input schema: {:name String :age? Number}
let input_schema = TypeExpr::Map {
    entries: vec![
        MapTypeEntry {
            key: Keyword("name".to_string()),
            value_type: Box::new(TypeExpr::Primitive(PrimitiveType::String)),
            optional: false,
        },
        MapTypeEntry {
            key: Keyword("age".to_string()),
            value_type: Box::new(TypeExpr::Primitive(PrimitiveType::Number)),
            optional: true,
        }
    ],
    wildcard: None,
};

// Define output schema: {:result String :status String}
let output_schema = TypeExpr::Map {
    entries: vec![
        MapTypeEntry {
            key: Keyword("result".to_string()),
            value_type: Box::new(TypeExpr::Primitive(PrimitiveType::String)),
            optional: false,
        },
        MapTypeEntry {
            key: Keyword("status".to_string()),
            value_type: Box::new(TypeExpr::Primitive(PrimitiveType::String)),
            optional: false,
        }
    ],
    wildcard: None,
};

// Register capability with schema validation
marketplace.register_local_capability_with_schema(
    "validated_greet".to_string(),
    "Validated Greeting Capability".to_string(),
    "Returns a greeting message with schema validation".to_string(),
    Arc::new(|inputs| {
        if let Value::Map(map) = inputs {
            let name = map.get(&MapKey::Keyword("name".to_string()))
                .and_then(|v| v.as_string())
                .unwrap_or("World");
            
            let age = map.get(&MapKey::Keyword("age".to_string()))
                .and_then(|v| v.as_number());
            
            let greeting = if let Some(age) = age {
                format!("Hello, {}! You are {} years old.", name, age)
            } else {
                format!("Hello, {}!", name)
            };
            
            let mut result = HashMap::new();
            result.insert(MapKey::Keyword("result".to_string()), Value::String(greeting));
            result.insert(MapKey::Keyword("status".to_string()), Value::String("success".to_string()));
            
            Ok(Value::Map(result))
        } else {
            let mut result = HashMap::new();
            result.insert(MapKey::Keyword("result".to_string()), Value::String("Invalid input".to_string()));
            result.insert(MapKey::Keyword("status".to_string()), Value::String("error".to_string()));
            Ok(Value::Map(result))
        }
    }),
    Some(input_schema),
    Some(output_schema)
).await?;
```

**Equivalent RTFS Schema Definitions:**

**Input Schema:**
```rtfs
[:map [:name string] [:age float ?]]
```

**Output Schema:**
```rtfs
[:map [:result string] [:status string]]
```

### 8.2 Capability Execution

```rust
// Execute capability
let mut params = HashMap::new();
params.insert("name".to_string(), Value::String("Alice".to_string()));

let result = marketplace.execute_capability("greet", &Value::Map(params)).await?;
println!("Result: {:?}", result);
```

### 8.3 Network Discovery

```rust
// Configure network registry
let network_config = NetworkRegistryConfig {
    endpoint: "https://registry.example.com/discover".to_string(),
    auth_token: Some("token".to_string()),
    refresh_interval: 3600,
    verify_attestations: true,
};

// Discover capabilities
let capabilities = marketplace.discover_capabilities("data_processing", Some(10)).await?;
for capability in capabilities {
    println!("Discovered: {}", capability.name);
}
```

## 9. Configuration

### 9.1 Marketplace Configuration

```rust
let marketplace = CapabilityMarketplace::new(registry);

// Add discovery agents
marketplace.add_discovery_agent(Box::new(NetworkDiscoveryAgent::new(
    "https://registry.example.com".to_string(),
    Some("auth_token".to_string()),
    3600
)));

// Register executors
marketplace.register_executor(Arc::new(MCPExecutor));
marketplace.register_executor(Arc::new(A2AExecutor));
marketplace.register_executor(Arc::new(PluginExecutor));
```

### 9.2 Security Configuration

```rust
// Enable attestation verification
let config = TypeCheckingConfig {
    verify_attestations: true,
    verify_provenance: true,
    strict_mode: true,
    ..Default::default()
};
```

## 10. Testing

### 10.1 Unit Testing

```rust
#[tokio::test]
async fn test_capability_registration() {
    let registry = Arc::new(RwLock::new(CapabilityRegistry::new()));
    let marketplace = CapabilityMarketplace::new(registry);
    
    // Register capability
    marketplace.register_local_capability(/* ... */).await.unwrap();
    
    // Verify registration
    let capability = marketplace.get_capability("test_capability").await;
    assert!(capability.is_some());
}
```

### 10.2 Integration Testing

```rust
#[tokio::test]
async fn test_network_discovery() {
    let marketplace = create_test_marketplace();
    
    // Test discovery
    let capabilities = marketplace.discover_capabilities("test", Some(5)).await.unwrap();
    assert!(!capabilities.is_empty());
}
```

## 11. Migration from RTFS 1.0

### 11.1 Breaking Changes

- Schema validation now uses RTFS native types instead of JSON Schema
- All provider types require explicit registration
- Network discovery requires configuration

### 11.2 Migration Guide

1. **Update Schema Definitions**: Convert JSON Schema to RTFS TypeExpr
2. **Register Providers**: Add explicit provider registration
3. **Configure Discovery**: Set up network discovery if needed
4. **Update Error Handling**: Use new error types

## 12. Future Extensions

### 12.1 Planned Features

- **MicroVM Integration**: Secure execution environments
- **Advanced Caching**: Intelligent capability result caching
- **Load Balancing**: Distributed capability execution
- **Metrics Collection**: Performance and usage metrics

### 12.2 Extension Points

- **Custom Executors**: Implement custom provider types
- **Discovery Agents**: Add custom discovery mechanisms
- **Validation Rules**: Extend schema validation
- **Security Policies**: Custom security enforcement

---

**Note**: This specification is based on the implementation completed in Issue #43 and represents the stable RTFS 2.0 capability system architecture. 