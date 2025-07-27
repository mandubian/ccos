# RTFS 2.0 Network Discovery Protocol Specification

**Version**: 2.0.0  
**Status**: Stable  
**Date**: July 2025  
**Based on**: Issue #43 Implementation

## 1. Overview

The RTFS 2.0 Network Discovery Protocol enables distributed capability discovery across RTFS deployments. It provides a standardized way for RTFS instances to discover and register capabilities from remote registries, supporting both centralized and federated discovery architectures.

## 2. Protocol Architecture

### 2.1 Core Components

- **Discovery Registry**: Centralized or federated service for capability registration and discovery
- **Discovery Agents**: Pluggable agents that implement discovery mechanisms
- **Network Registry**: Remote registry configuration and communication
- **Capability Marketplace**: Local capability management with network discovery integration

### 2.2 Communication Protocol

The protocol uses **HTTP/HTTPS** with **JSON-RPC 2.0** for structured communication:

- **Transport**: HTTP/HTTPS
- **Content-Type**: `application/json`
- **Protocol**: JSON-RPC 2.0
- **Authentication**: Bearer tokens or API keys
- **Timeout**: Configurable (default: 30 seconds)

## 3. Discovery Registry API

### 3.1 Registry Endpoints

#### 3.1.1 Capability Registration

**Endpoint**: `POST /register`

**Request**:
```json
{
  "jsonrpc": "2.0",
  "method": "rtfs.registry.register",
  "params": {
    "capability": {
      "id": "unique-capability-id",
      "name": "Capability Name",
      "description": "Capability description",
      "provider_type": "http|mcp|a2a|plugin|remote_rtfs|stream",
      "endpoint": "https://api.example.com/capability",
      "version": "1.0.0",
      "input_schema": {
        "type": "map",
        "entries": [
          {
            "key": "name",
            "value_type": {"type": "primitive", "primitive": "string"},
            "optional": false
          },
          {
            "key": "age",
            "value_type": {"type": "primitive", "primitive": "number"},
            "optional": true
          }
        ]
      },
      "output_schema": {
        "type": "map",
        "entries": [
          {
            "key": "result",
            "value_type": {"type": "primitive", "primitive": "string"},
            "optional": false
          }
        ]
      },
      "attestation": {
        "signature": "sha256:...",
        "authority": "trusted-authority",
        "created_at": "2025-07-24T10:30:00Z",
        "expires_at": "2025-08-24T10:30:00Z"
      },
      "provenance": {
        "source": "network_registry",
        "content_hash": "sha256:...",
        "custody_chain": ["registry1", "registry2"],
        "registered_at": "2025-07-24T10:30:00Z"
      },
      "permissions": ["read", "execute"],
      "metadata": {
        "owner": "team-name",
        "tags": ["data-processing", "ml"]
      }
    },
    "ttl_seconds": 3600
  },
  "id": "req-001"
}
```

**Schema Correspondence:**

**Rust TypeExpr:**
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
            optional: true,
        }
    ],
    wildcard: None,
};
```

**RTFS Syntax:**
```rtfs
[:map [:name string] [:age float ?]]
```

**JSON Schema Representation:**
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
      "key": "age", 
      "value_type": {"type": "primitive", "primitive": "number"},
      "optional": true
    }
  ]
}
```

**Response**:
```json
{
  "jsonrpc": "2.0",
  "result": {
    "status": "registered",
    "capability_id": "unique-capability-id",
    "expires_at": "2025-07-24T11:30:00Z"
  },
  "id": "req-001"
}
```

#### 3.1.2 Capability Discovery

**Endpoint**: `POST /discover`

**Request**:
```json
{
  "jsonrpc": "2.0",
  "method": "rtfs.registry.discover",
  "params": {
    "query": "data_processing",
    "capability_id": "specific-capability-id",
    "provider_type": "http",
    "version_constraint": ">=1.0.0 <2.0.0",
    "tags": ["ml", "production"],
    "limit": 10,
    "offset": 0
  },
  "id": "req-002"
}
```

**Response**:
```json
{
  "jsonrpc": "2.0",
  "result": {
    "capabilities": [
      {
        "id": "data-processor-v1",
        "name": "Data Processing Capability",
        "description": "Processes and analyzes datasets",
        "provider_type": "http",
        "endpoint": "https://api.example.com/data-processor",
        "version": "1.2.0",
        "input_schema": { /* RTFS TypeExpr */ },
        "output_schema": { /* RTFS TypeExpr */ },
        "attestation": { /* attestation data */ },
        "provenance": { /* provenance data */ },
        "permissions": ["read", "execute"],
        "metadata": {
          "owner": "DataTeam",
          "tags": ["data-processing", "ml", "production"]
        }
      }
    ],
    "total_count": 1,
    "has_more": false
  },
  "id": "req-002"
}
```

#### 3.1.3 Registry Health Check

**Endpoint**: `GET /health`

**Response**:
```json
{
  "status": "healthy",
  "version": "2.0.0",
  "capabilities_count": 1250,
  "uptime_seconds": 86400,
  "last_cleanup": "2025-07-24T09:00:00Z"
}
```

### 3.2 Error Responses

**Standard Error Format**:
```json
{
  "jsonrpc": "2.0",
  "error": {
    "code": -32001,
    "message": "Capability registration failed: Invalid schema",
    "data": {
      "field": "input_schema",
      "reason": "Invalid TypeExpr format"
    }
  },
  "id": "req-001"
}
```

**Error Codes**:
- `-32000`: Server error
- `-32001`: Invalid request
- `-32002`: Method not found
- `-32003`: Invalid params
- `-32004`: Internal error
- `-32005`: Parse error
- `-32006`: Invalid capability
- `-32007`: Duplicate capability
- `-32008`: Authentication required
- `-32009`: Rate limit exceeded

## 4. Discovery Agent Framework

### 4.1 Discovery Agent Interface

```rust
pub trait CapabilityDiscovery: Send + Sync {
    async fn discover(&self) -> Result<Vec<CapabilityManifest>, RuntimeError>;
}
```

### 4.2 Built-in Discovery Agents

#### 4.2.1 Network Discovery Agent

```rust
pub struct NetworkDiscoveryAgent {
    registry_endpoint: String,
    auth_token: Option<String>,
    refresh_interval: std::time::Duration,
    last_discovery: std::time::Instant,
}
```

**Configuration**:
```rust
let agent = NetworkDiscoveryAgent::new(
    "https://registry.example.com".to_string(),
    Some("auth_token".to_string()),
    3600 // refresh interval in seconds
);
```

#### 4.2.2 Local File Discovery Agent

```rust
pub struct LocalFileDiscoveryAgent {
    discovery_path: std::path::PathBuf,
    file_pattern: String,
}
```

**Configuration**:
```rust
let agent = LocalFileDiscoveryAgent::new(
    std::path::PathBuf::from("/capabilities"),
    "*.capability.json".to_string()
);
```

### 4.3 Custom Discovery Agents

Implement the `CapabilityDiscovery` trait for custom discovery mechanisms:

```rust
pub struct CustomDiscoveryAgent {
    // Custom fields
}

impl CapabilityDiscovery for CustomDiscoveryAgent {
    async fn discover(&self) -> Result<Vec<CapabilityManifest>, RuntimeError> {
        // Custom discovery logic
        Ok(vec![])
    }
}
```

## 5. Network Registry Configuration

### 5.1 Registry Configuration

```rust
pub struct NetworkRegistryConfig {
    pub endpoint: String,
    pub auth_token: Option<String>,
    pub refresh_interval: u64,
    pub verify_attestations: bool,
}
```

### 5.2 Configuration Examples

**Basic Configuration**:
```rust
let config = NetworkRegistryConfig {
    endpoint: "https://registry.example.com".to_string(),
    auth_token: None,
    refresh_interval: 3600,
    verify_attestations: false,
};
```

**Secure Configuration**:
```rust
let config = NetworkRegistryConfig {
    endpoint: "https://secure-registry.example.com".to_string(),
    auth_token: Some("bearer-token".to_string()),
    refresh_interval: 1800, // 30 minutes
    verify_attestations: true,
};
```

## 6. Discovery Integration

### 6.1 Marketplace Integration

```rust
impl CapabilityMarketplace {
    pub async fn discover_capabilities(
        &self,
        query: &str,
        limit: Option<usize>,
    ) -> Result<Vec<CapabilityManifest>, RuntimeError> {
        if let Some(registry_config) = &self.network_registry {
            self.discover_from_network(registry_config, query, limit).await
        } else {
            self.discover_local_capabilities(query, limit).await
        }
    }
}
```

### 6.2 Discovery Workflow

1. **Query Preparation**: Format discovery query with filters
2. **Network Request**: Send HTTP request to registry
3. **Response Processing**: Parse and validate capability manifests
4. **Attestation Verification**: Verify capability attestations if enabled
5. **Local Registration**: Register discovered capabilities locally
6. **Error Handling**: Handle network errors and fallbacks

### 6.3 Discovery Query Format

**Simple Query**:
```json
{
  "query": "data_processing",
  "limit": 10
}
```

**Advanced Query**:
```json
{
  "query": "ml_inference",
  "provider_type": "http",
  "version_constraint": ">=1.0.0",
  "tags": ["production", "gpu"],
  "limit": 5,
  "offset": 0
}
```

## 7. Security Features

### 7.1 Authentication

**Bearer Token Authentication**:
```http
Authorization: Bearer <token>
```

**API Key Authentication**:
```http
X-API-Key: <api-key>
```

### 7.2 Attestation Verification

```rust
async fn verify_capability_attestation(
    &self,
    attestation: &CapabilityAttestation,
    manifest: &CapabilityManifest,
) -> Result<bool, RuntimeError> {
    // Verify digital signature
    // Check expiration
    // Validate authority
    // Return verification result
}
```

### 7.3 Content Integrity

**Content Hash Verification**:
```rust
fn verify_content_hash(&self, content: &str, expected_hash: &str) -> bool {
    let computed_hash = self.compute_content_hash(content);
    computed_hash == expected_hash
}
```

## 8. Error Handling and Resilience

### 8.1 Network Error Handling

```rust
async fn discover_from_network(
    &self,
    config: &NetworkRegistryConfig,
    query: &str,
    limit: Option<usize>,
) -> Result<Vec<CapabilityManifest>, RuntimeError> {
    let client = reqwest::Client::new();
    let response = client
        .post(&config.endpoint)
        .json(&discovery_payload)
        .timeout(std::time::Duration::from_secs(30))
        .send()
        .await
        .map_err(|e| RuntimeError::NetworkError(format!("Discovery failed: {}", e)))?;
    
    // Process response...
}
```

### 8.2 Fallback Mechanisms

1. **Local Fallback**: Use local capabilities if network discovery fails
2. **Cached Results**: Use cached discovery results if available
3. **Retry Logic**: Retry failed requests with exponential backoff
4. **Multiple Registries**: Try multiple registry endpoints

### 8.3 Rate Limiting

**Client-Side Rate Limiting**:
```rust
pub struct RateLimitedDiscoveryAgent {
    agent: Box<dyn CapabilityDiscovery>,
    rate_limiter: Arc<RateLimiter>,
}
```

## 9. Performance Optimization

### 9.1 Caching

**Discovery Result Caching**:
```rust
pub struct CachedDiscoveryAgent {
    agent: Box<dyn CapabilityDiscovery>,
    cache: Arc<RwLock<HashMap<String, (Vec<CapabilityManifest>, Instant)>>>,
    cache_ttl: Duration,
}
```

### 9.2 Connection Pooling

**HTTP Client Configuration**:
```rust
let client = reqwest::Client::builder()
    .pool_max_idle_per_host(10)
    .timeout(Duration::from_secs(30))
    .build()?;
```

### 9.3 Parallel Discovery

**Concurrent Discovery**:
```rust
async fn discover_from_multiple_sources(
    &self,
    agents: &[Box<dyn CapabilityDiscovery>],
) -> Result<Vec<CapabilityManifest>, RuntimeError> {
    let futures: Vec<_> = agents.iter().map(|agent| agent.discover()).collect();
    let results = futures::future::join_all(futures).await;
    
    // Merge and deduplicate results
    let mut all_capabilities = Vec::new();
    for result in results {
        if let Ok(capabilities) = result {
            all_capabilities.extend(capabilities);
        }
    }
    
    Ok(all_capabilities)
}
```

## 10. Monitoring and Observability

### 10.1 Discovery Metrics

```rust
pub struct DiscoveryMetrics {
    pub total_discoveries: u64,
    pub successful_discoveries: u64,
    pub failed_discoveries: u64,
    pub average_response_time: Duration,
    pub last_discovery: Option<Instant>,
}
```

### 10.2 Health Monitoring

**Registry Health Check**:
```rust
async fn check_registry_health(&self, endpoint: &str) -> Result<bool, RuntimeError> {
    let response = reqwest::get(format!("{}/health", endpoint)).await?;
    let health: serde_json::Value = response.json().await?;
    
    Ok(health["status"].as_str() == Some("healthy"))
}
```

## 11. Testing

### 11.1 Unit Tests

```rust
#[tokio::test]
async fn test_network_discovery() {
    let marketplace = create_test_marketplace();
    
    // Test discovery functionality
    let capabilities = marketplace.discover_capabilities("test", Some(5)).await.unwrap();
    assert!(!capabilities.is_empty());
}
```

### 11.2 Integration Tests

```rust
#[tokio::test]
async fn test_registry_integration() {
    // Start mock registry server
    let mock_server = MockRegistryServer::new();
    let endpoint = mock_server.start().await;
    
    // Test discovery against mock registry
    let agent = NetworkDiscoveryAgent::new(endpoint, None, 60);
    let capabilities = agent.discover().await.unwrap();
    
    assert_eq!(capabilities.len(), 2);
}
```

## 12. Deployment Considerations

### 12.1 Registry Deployment

**Single Registry**:
- Simple deployment for small to medium deployments
- Single point of failure
- Easy to manage and monitor

**Federated Registries**:
- Multiple registry instances
- Load balancing and redundancy
- Geographic distribution

### 12.2 Security Deployment

**TLS Configuration**:
```rust
let client = reqwest::Client::builder()
    .use_rustls_tls()
    .build()?;
```

**Certificate Pinning**:
```rust
let client = reqwest::Client::builder()
    .add_root_certificate(cert)
    .build()?;
```

## 13. Migration from RTFS 1.0

### 13.1 Breaking Changes

- New JSON-RPC 2.0 protocol format
- Enhanced security with attestation verification
- Improved error handling and resilience
- Support for multiple discovery agents

### 13.2 Migration Steps

1. **Update Client Code**: Use new discovery API
2. **Configure Security**: Set up attestation verification
3. **Update Registry**: Deploy new registry version
4. **Test Integration**: Verify discovery functionality

## 14. Future Extensions

### 14.1 Planned Features

- **Real-time Discovery**: WebSocket-based real-time capability updates
- **Advanced Filtering**: Complex query language for capability discovery
- **Federation Protocol**: Inter-registry communication protocol
- **Discovery Analytics**: Usage analytics and insights

### 14.2 Extension Points

- **Custom Protocols**: Support for non-HTTP discovery protocols
- **Advanced Caching**: Intelligent caching strategies
- **Load Balancing**: Dynamic load balancing for registry access
- **Geographic Routing**: Geographic-aware discovery routing

---

**Note**: This specification defines the complete RTFS 2.0 network discovery protocol based on the implementation completed in Issue #43. 