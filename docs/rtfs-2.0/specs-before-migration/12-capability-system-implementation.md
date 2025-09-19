# RTFS 2.0 Capability System Implementation

**Status:** Stable  
**Version:** 2.0.0  
**Date:** July 2025  
**Integration:** CCOS Capability Marketplace

## 1. Overview

This document specifies the implementation architecture of the RTFS 2.0 Capability System, which provides a secure, extensible, and robust mechanism for executing a wide range of operations within the CCOS cognitive architecture.

The system is built on a **three-component architecture** that separates high-level orchestration, extensible execution, and low-level secure execution:

1. **CapabilityMarketplace**: High-level orchestration and discovery
2. **CapabilityExecutor Pattern**: Extensible execution framework  
3. **CapabilityRegistry**: Low-level, secure execution engine for built-in capabilities

## 2. Core Components

### 2.1 CapabilityMarketplace

The `CapabilityMarketplace` is the primary, high-level interface for the RTFS runtime to interact with capabilities.

**Responsibilities:**
- **Discovery**: Provides mechanisms to discover and list available capabilities
- **Orchestration**: Acts as the main entry point for all `(call ...)` operations
- **Executor Management**: Manages registered `CapabilityExecutor` instances
- **High-Level Capability Execution**: Directly handles asynchronous or I/O-based capabilities
- **Delegation**: Forwards requests for local, built-in capabilities to the `CapabilityRegistry`

**Supported Capability Types:**
- HTTP-based remote capabilities
- MCP (Model Context Protocol) capabilities using the official Rust SDK
- A2A (Agent-to-Agent) capabilities
- Plugin-based capabilities
- Streaming capabilities
- Local, built-in capabilities (delegated to CapabilityRegistry)

### 2.2 CapabilityExecutor Pattern

The `CapabilityExecutor` pattern provides an extensible framework for executing different types of capabilities.

**Key Components:**
```rust
pub trait CapabilityExecutor: Send + Sync {
    fn provider_type_id(&self) -> TypeId;
    async fn execute(&self, provider: &ProviderType, inputs: &Value) -> RuntimeResult<Value>;
}
```

**Built-in Executors:**
- **MCPExecutor**: Uses the official MCP Rust SDK for Model Context Protocol communication
- **A2AExecutor**: Handles Agent-to-Agent communication across multiple protocols
- **LocalExecutor**: Executes local, in-process capabilities
- **HttpExecutor**: Handles HTTP-based remote capabilities
- **PluginExecutor**: Manages plugin-based capabilities
- **RemoteRTFSExecutor**: Handles remote RTFS system communication
- **StreamExecutor**: Manages streaming capabilities

### 2.3 CapabilityRegistry

The `CapabilityRegistry` is the low-level, secure execution engine for a curated set of built-in, sandboxed capabilities.

**Responsibilities:**
- **Secure Execution**: Executes trusted, built-in functions in a controlled environment
- **Performance**: Optimized for fast, synchronous execution of core functionalities
- **Isolation**: Completely decoupled from high-level components to ensure security

## 3. Core Data Structures

### 3.1 CapabilityManifest

The `CapabilityManifest` is a public-facing data structure that describes a capability to the system.

```rust
/// Describes a capability and how to execute it.
#[derive(Debug, Clone)]
pub struct CapabilityManifest {
    pub id: String,
    pub name: String,
    pub description: String,
    /// The specific provider type that implements the capability.
    pub provider: ProviderType,
    pub version: String,
    pub input_schema: Option<TypeExpr>,  // RTFS type expression for input validation
    pub output_schema: Option<TypeExpr>, // RTFS type expression for output validation
    pub attestation: Option<CapabilityAttestation>,
    pub provenance: Option<CapabilityProvenance>,
    pub permissions: Vec<String>,
    pub metadata: std::collections::HashMap<String, String>,
}
```

### 3.2 ProviderType Enum

```rust
#[derive(Debug, Clone)]
pub enum ProviderType {
    Local(LocalProvider),
    Http(HttpProvider),
    Mcp(McpProvider),
    A2A(A2AProvider),
    Plugin(PluginProvider),
    RemoteRTFS(RemoteRTFSProvider),
    Streaming(StreamingProvider),
}
```

### 3.3 CapabilityAttestation

```rust
#[derive(Debug, Clone)]
pub struct CapabilityAttestation {
    pub signature: String,
    pub algorithm: String,
    pub authority: String,
    pub key_id: String,
    pub timestamp: DateTime<Utc>,
    pub expires: Option<DateTime<Utc>>,
    pub chain_of_trust: Vec<String>,
    pub verification_status: VerificationStatus,
}
```

## 4. RTFS 2.0 Integration

### 4.1 Call Expression Integration

The `(call ...)` expression in RTFS 2.0 integrates directly with the CapabilityMarketplace:

```clojure
;; RTFS 2.0 call expression
(call :com.acme.db:v1.0:sales-query 
      {:query "SELECT * FROM sales WHERE quarter = 'Q2-2025'",
       :format :csv})
```

**Execution Flow:**
1. RTFS runtime evaluates the `(call ...)` expression
2. CapabilityMarketplace receives the capability request
3. Marketplace determines the appropriate executor based on provider type
4. Executor handles the capability execution
5. Result is returned to RTFS runtime
6. Action is recorded in the Causal Chain

### 4.2 Security Integration

```rust
impl CapabilityMarketplace {
    pub async fn execute_capability(
        &self,
        capability_id: &str,
        inputs: &Value,
        context: &ExecutionContext,
    ) -> RuntimeResult<Value> {
        // 1. Security validation
        if !self.security_context.is_capability_allowed(capability_id) {
            return Err(RuntimeError::SecurityViolation {
                operation: "call".to_string(),
                capability: capability_id.to_string(),
                context: format!("{:?}", self.security_context),
            });
        }
        
        // 2. Capability discovery
        let manifest = self.discover_capability(capability_id).await?;
        
        // 3. Executor selection
        let executor = self.get_executor_for_provider(&manifest.provider)?;
        
        // 4. Execution
        let result = executor.execute(&manifest.provider, inputs).await?;
        
        // 5. Action recording
        self.record_action(capability_id, inputs, &result, context).await?;
        
        Ok(result)
    }
}
```

## 5. Implementation Examples

### 5.1 HTTP Capability Example

```rust
// HTTP Provider Configuration
let http_provider = HttpProvider {
    endpoint: "https://api.example.com/data".to_string(),
    method: "GET".to_string(),
    headers: HashMap::new(),
    timeout_ms: Some(5000),
    retry_config: None,
    auth: None,
};

// Capability Manifest
let manifest = CapabilityManifest {
    id: "com.example.api:v1.0:get-data".to_string(),
    name: "get-data".to_string(),
    description: "Retrieve data from external API".to_string(),
    provider: ProviderType::Http(http_provider),
    version: "1.0.0".to_string(),
    input_schema: Some(TypeExpr::Struct(HashMap::new())),
    output_schema: Some(TypeExpr::Map),
    attestation: None,
    provenance: None,
    permissions: vec!["network.read".to_string()],
    metadata: HashMap::new(),
};
```

### 5.2 MCP Capability Example

```rust
// MCP Provider Configuration
let mcp_provider = McpProvider {
    server_url: "mcp://localhost:3000".to_string(),
    tools: vec!["file.read".to_string(), "file.write".to_string()],
    protocol_version: "2024-11-05".to_string(),
    authentication: None,
};

// Capability Manifest
let manifest = CapabilityManifest {
    id: "org.mcp.file:v1.0:read".to_string(),
    name: "file.read".to_string(),
    description: "Read file contents via MCP".to_string(),
    provider: ProviderType::Mcp(mcp_provider),
    version: "1.0.0".to_string(),
    input_schema: Some(TypeExpr::Struct({
        let mut map = HashMap::new();
        map.insert("path".to_string(), TypeExpr::String);
        map
    })),
    output_schema: Some(TypeExpr::String),
    attestation: None,
    provenance: None,
    permissions: vec!["file.read".to_string()],
    metadata: HashMap::new(),
};
```

## 6. Error Handling

### 6.1 Capability Execution Errors

```rust
#[derive(Debug, thiserror::Error)]
pub enum CapabilityError {
    #[error("Capability not found: {0}")]
    NotFound(String),
    
    #[error("Capability execution failed: {0}")]
    ExecutionFailed(String),
    
    #[error("Security violation: {operation} on {capability}")]
    SecurityViolation { operation: String, capability: String },
    
    #[error("Provider error: {0}")]
    ProviderError(String),
    
    #[error("Schema validation failed: {0}")]
    SchemaValidationFailed(String),
}
```

### 6.2 Error Recovery Strategies

```rust
impl CapabilityMarketplace {
    async fn execute_with_fallback(
        &self,
        capability_id: &str,
        inputs: &Value,
        context: &ExecutionContext,
    ) -> RuntimeResult<Value> {
        // Try primary capability
        match self.execute_capability(capability_id, inputs, context).await {
            Ok(result) => Ok(result),
            Err(CapabilityError::NotFound(_)) => {
                // Try fallback capability
                let fallback_id = self.get_fallback_capability(capability_id)?;
                self.execute_capability(&fallback_id, inputs, context).await
            }
            Err(e) => Err(e),
        }
    }
}
```

## 7. Performance Considerations

### 7.1 Caching Strategy

```rust
impl CapabilityMarketplace {
    async fn execute_capability_cached(
        &self,
        capability_id: &str,
        inputs: &Value,
        context: &ExecutionContext,
    ) -> RuntimeResult<Value> {
        // Check cache first
        if let Some(cached_result) = self.cache.get(&self.cache_key(capability_id, inputs)) {
            return Ok(cached_result.clone());
        }
        
        // Execute and cache
        let result = self.execute_capability(capability_id, inputs, context).await?;
        self.cache.set(self.cache_key(capability_id, inputs), result.clone());
        
        Ok(result)
    }
}
```

### 7.2 Async Execution

```rust
impl CapabilityMarketplace {
    pub async fn execute_capabilities_parallel(
        &self,
        capabilities: Vec<(String, Value)>,
        context: &ExecutionContext,
    ) -> RuntimeResult<Vec<Value>> {
        let futures: Vec<_> = capabilities
            .into_iter()
            .map(|(id, inputs)| self.execute_capability(&id, &inputs, context))
            .collect();
        
        let results = futures::future::join_all(futures).await;
        
        // Collect results, preserving order
        results.into_iter().collect()
    }
}
```

## 8. Testing and Validation

### 8.1 Unit Testing

```rust
#[cfg(test)]
mod tests {
    use super::*;
    
    #[tokio::test]
    async fn test_http_capability_execution() {
        let marketplace = CapabilityMarketplace::new();
        let inputs = Value::Map(HashMap::new());
        let context = ExecutionContext::new("test-plan", vec!["test-intent"]);
        
        let result = marketplace
            .execute_capability("com.example.api:v1.0:get-data", &inputs, &context)
            .await;
        
        assert!(result.is_ok());
    }
}
```

### 8.2 Integration Testing

```rust
#[tokio::test]
async fn test_capability_lifecycle() {
    // 1. Register capability
    let manifest = create_test_manifest();
    marketplace.register_capability(manifest).await?;
    
    // 2. Discover capability
    let discovered = marketplace.discover_capability("test:capability").await?;
    assert_eq!(discovered.id, "test:capability");
    
    // 3. Execute capability
    let result = marketplace
        .execute_capability("test:capability", &Value::Null, &context)
        .await?;
    
    // 4. Verify action recording
    let actions = causal_chain.get_actions_for_plan("test-plan").await?;
    assert_eq!(actions.len(), 1);
}
```

## 9. Conclusion

The RTFS 2.0 Capability System provides a robust, secure, and extensible foundation for executing capabilities within the CCOS cognitive architecture. The three-component design ensures proper separation of concerns while maintaining the flexibility needed for diverse capability types.

Key benefits:
- **Security**: Proper isolation and validation at multiple levels
- **Extensibility**: New capability types can be added without core changes
- **Performance**: Optimized execution with caching and parallel processing
- **Integration**: Seamless integration with RTFS 2.0 language constructs
- **Auditability**: Complete action tracking for the Causal Chain

---

**Note**: This implementation specification complements the formal RTFS 2.0 language specification and provides the technical foundation for capability execution within the CCOS architecture. 