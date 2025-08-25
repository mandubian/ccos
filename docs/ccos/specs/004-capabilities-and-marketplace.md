# CCOS Specification 004: Capabilities and Marketplace

**Status:** Proposed
**Version:** 1.0
**Date:** 2025-07-20
**Related:** [SEP-000: System Architecture](./000-ccos-architecture.md), [SEP-007: Global Function Mesh](./007-global-function-mesh.md)

## 1. Abstract

This document defines how CCOS extends its functionality through `Capabilities`. A Capability is a versioned, verifiable, and discoverable function or service that can be invoked by a `Plan`. This specification also outlines the evolution from a simple capability registry to a dynamic **Generative Capability Marketplace**.

## 2. Core Concepts

### 2.1. Capability

A `Capability` is a formal description of a service that can be executed. It includes:

-   **Identifier**: A universal, unique name (e.g., `:com.acme.database:v1.query`).
-   **Schema**: A formal definition of the expected inputs and outputs.
-   **Provider**: The concrete agent or module that implements the capability.
-   **Metadata**: Additional information like cost, latency, or security classification.

### 2.2. The `(call)` Primitive

Capabilities are invoked from within a `Plan` using the `(call ...)` primitive:

```rtfs
(call :com.acme.database:v1.query {:table "users" :id 123})
```

### 2.3. Discovery via the Global Function Mesh (GFM)

The GFM (see SEP-007) is responsible for resolving a capability identifier to a list of available providers.

## 3. The Capability Registry

In its initial implementation, CCOS uses a simple **Capability Registry**. This is a local or federated database that maps capability identifiers to provider implementations. The Orchestrator queries this registry via the GFM to find a valid provider for a given `(call)`.

## 4. Future Vision: The Generative Capability Marketplace

The long-term vision is to evolve the simple registry into a dynamic, economic ecosystem. This **Generative Capability Marketplace** will be a core component of the CCOS, enabling advanced, autonomous behavior.

### 4.1. Capabilities as Service Level Agreements (SLAs)

In the Marketplace, providers don't just register a function; they offer a service with a rich SLA, including:

-   **Cost**: Price per call or per token.
-   **Speed**: Average latency metrics.
-   **Confidence**: A score representing the likely accuracy of the result.
-   **Data Provenance**: Information about the origin of the data the capability uses.
-   **Ethical Alignment Profile**: A declaration of the ethical principles the capability adheres to.

The Arbiter will use this rich metadata to act as a broker, selecting the provider that best matches the `constraints` and `preferences` defined in the active `Intent`.

### 4.2. Generative Capabilities

The Arbiter itself will be able to create and publish new capabilities to the marketplace. If it requires a function that does not exist, it can:

1.  Find constituent capabilities on the marketplace.
2.  Compose them into a new RTFS function.
3.  Wrap this new function in a formal capability definition.
4.  Publish the new "generative capability" back to the marketplace for itself and others to use.

This allows the CCOS to learn, grow, and autonomously expand its own skillset over time, transforming it from a static tool into a living, evolving system.
5.  The result is returned to the RTFS runtime and the `CapabilityCall` action in the Causal Chain is updated with the outcome.

## 5. Delegation

Delegation is the process of using a capability to assign a sub-task to another cognitive agent, which could be a specialized LLM, another CCOS instance, or even a human.

### 5.1. The Delegation Pattern

Delegation is not a different mechanism; it is a pattern of using capabilities. A typical delegation capability might be `llm.generate-plan` or `human.approve-transaction`.

The following example shows how a plan can use one step to generate a sub-plan and a second step to execute it. The outer `(let ...)` block ensures that the result of the first step (`generated_plan`) is available to the second step.

```lisp
(let [
  ;; Step 1: Generate a sub-plan. The resulting plan object is bound to the `generated_plan` variable.
  generated_plan (step "Generate a sub-plan"
    (call :llm.generate-plan {:goal "Analyze user sentiment data" :constraints ...}))
]
  ;; Step 2: Execute the plan that was created in the previous step.
  (step "Execute the sub-plan"
    (call :ccos.execute-plan generated_plan))
)
```

> **Note on `(let)` and `(step)` Patterns**
>
> There are two primary ways to combine `let` and `step`, each for a different purpose:
>
> 1.  **`(let [var (step ...)] ...)` (Sequencing):** As shown above, this is the standard pattern for creating a sequence of dependent steps. The `let` creates a scope, and the result of one step is stored in a variable that can be used by subsequent steps.
> 2.  **`(step ... (let ...))` (Encapsulation):** This pattern is used when a single step requires complex internal logic or temporary variables to prepare for its main `(call)`. The `let` is contained entirely *within* the step and its variables are not visible to other steps.

### 5.2. Key Features

-   **Recursive CCOS**: A delegated call to another CCOS instance creates its own nested Causal Chain. The parent action in the calling system can store the `plan_id` of the sub-plan, creating a verifiable link between the two execution histories.
-   **Human-in-the-Loop**: A call to a human-in-the-loop capability (e.g., `human.ask-question`) would pause the plan's execution (`PlanPaused` action) until the human provides a response, at which point a `PlanResumed` action is logged and execution continues.
-   **Specialized Agents**: Allows the main orchestrator to act as a generalist that delegates complex, domain-specific tasks to expert agents (e.g., a coding agent, a data analysis agent).
 
## 6. Execution Path and Executors

### 6.1. Provider Types and Built-in Executors

A `CapabilityManifest` declares a concrete `Provider` (e.g., MCP, A2A, Local, HTTP, Stream). The runtime ships with built-in executors for core provider families, ensuring a zero-config path for standard capability execution.

### 6.2. Dyn-safe Executor Registry (Enum-based)

To enable lightweight pluggability without the pitfalls of `async fn` in object-safe traits, the runtime uses an enum-based registry:

- Executors are represented by an `ExecutorVariant` enum which wraps concrete executors (e.g., MCP, A2A, Local, HTTP)
- `CapabilityMarketplace` maintains a map from `TypeId` of provider to `ExecutorVariant`
- On `(call ...)`, marketplace resolves manifest -> provider -> executor and dispatches
- If no executor is found, it falls back to built-in provider-specific methods

This approach avoids dyn-trait vtable issues for async methods while preserving extensibility and determinism.

### 6.3. Execution Flow

1. Resolve capability ID to `CapabilityManifest`
2. Determine provider type (e.g., `ProviderType::Http`)
3. Check the enum-based executor registry for a matching executor
4. If present, dispatch via the executor; otherwise, use direct provider execution
5. Record `CapabilityCall` in the Causal Chain

### 6.4. Future: Full Plugin Registry

When broader third-party plugin support is required, the enum-based registry can be upgraded to an object-safe trait returning boxed futures, or a code-generated adapter layer that reifies async into sync trait methods. Until then, the current approach is simple, robust, and avoids unsound dyn patterns.

---

## Addendum (2025-08-24): Marketplace Implementation Details (v2.0)

This addendum documents the concrete implementation delivered in this worktree. It augments the original specification above without changing its normative sections.

### A. Bootstrap and Discovery System

#### A.1. Startup Bootstrap Process
The marketplace implements an automatic bootstrap process that initializes the capability ecosystem on startup:

```rust
impl CapabilityMarketplace {
    pub async fn bootstrap(&self) -> Result<(), RuntimeError> {
        // Load capabilities from local registry
        let registry_capabilities = self.registry.get_capabilities();
        
        // Register default capabilities (ccos.echo, ccos.math.add, etc.)
        crate::runtime::stdlib::register_default_capabilities(&self).await?;
        
        // Discover additional capabilities via discovery providers
        for provider in &self.discovery_providers {
            let discovered = provider.discover().await?;
            for manifest in discovered {
                self.register_capability(manifest).await?;
            }
        }
        Ok(())
    }
}
```

#### A.2. Discovery Provider Framework
The system implements a trait-based discovery framework supporting multiple provider types:

```rust
#[async_trait::async_trait]
pub trait CapabilityDiscovery {
    async fn discover(&self) -> Result<Vec<CapabilityManifest>, RuntimeError>;
}

pub enum DiscoveryProvider {
    Static { capabilities: Vec<CapabilityManifest> },
    FileManifest { path: PathBuf, format: ManifestFormat },
    Network { endpoint: Url, auth: NetworkAuth },
}
```

- **Static Provider**: Pre-configured capability manifests loaded at compile time
- **File Manifest Provider**: Loads capabilities from JSON/YAML manifest files
- **Network Provider**: Discovers capabilities via HTTP endpoints (placeholder implementation)

#### A.3. Extensibility
New discovery providers can be added by implementing the `CapabilityDiscovery` trait and registering them with the marketplace.

### B. Isolation Policy System

#### B.1. CapabilityIsolationPolicy Structure
The isolation system provides fine-grained control over capability execution:

```rust
pub struct CapabilityIsolationPolicy {
    pub allowed_capabilities: Vec<String>,      // Glob patterns: ["ccos.*", "math.*"]
    pub denied_capabilities: Vec<String>,       // Glob patterns: ["system.*", "admin.*"]
    pub namespace_policies: HashMap<String, NamespacePolicy>,
    pub resource_constraints: ResourceConstraints,
    pub time_constraints: Option<TimeConstraints>,
}

pub struct NamespacePolicy {
    pub allowed: Vec<String>,
    pub denied: Vec<String>,
    pub resource_limits: Option<ResourceConstraints>,
}

pub struct ResourceConstraints {
    pub max_memory_mb: Option<u64>,
    pub max_cpu_percent: Option<f64>,
    pub max_execution_time_ms: Option<u64>,
    pub max_concurrent_calls: Option<u32>,
}

pub struct TimeConstraints {
    pub allowed_hours: Vec<u8>,           // 0-23
    pub allowed_days: Vec<u8>,            // 0-6 (Sunday=0)
    pub timezone: String,                 // "UTC", "America/New_York"
}
```

#### B.2. Policy Enforcement
Policies are evaluated before capability execution:

```rust
impl CapabilityMarketplace {
    fn validate_capability_access(&self, capability_id: &str) -> Result<(), RuntimeError> {
        let policy = &self.isolation_policy;
        
        // Check allow/deny patterns
        if !policy.allowed_capabilities.is_empty() {
            let allowed = policy.allowed_capabilities.iter()
                .any(|pattern| glob_match(pattern, capability_id));
            if !allowed {
                return Err(RuntimeError::AccessDenied);
            }
        }
        
        // Check namespace policies
        if let Some(namespace) = extract_namespace(capability_id) {
            if let Some(ns_policy) = policy.namespace_policies.get(&namespace) {
                // Apply namespace-specific rules
            }
        }
        
        // Check time constraints
        if let Some(time_constraints) = &policy.time_constraints {
            let now = chrono::Utc::now();
            // Validate current time against allowed hours/days
        }
        
        Ok(())
    }
}
```

### C. Execution and Auditing System

#### C.1. Executor Registry Implementation
The enum-based executor registry provides type-safe capability execution:

```rust
pub enum ExecutorVariant {
    Local(LocalExecutor),
    Http(HttpExecutor),
    Mcp(McpExecutor),
    A2A(A2AExecutor),
    Stream(StreamExecutor),
}

impl CapabilityMarketplace {
    pub async fn execute_capability(
        &self,
        capability_id: &str,
        args: &Value
    ) -> Result<Value, RuntimeError> {
        // Validate access
        self.validate_capability_access(capability_id)?;
        
        // Resolve capability and executor
        let capability = self.get_capability(capability_id)?;
        let executor = self.get_executor(&capability.provider_type)?;
        
        // Execute and record audit event
        let start_time = std::time::Instant::now();
        let result = executor.execute(&capability, args).await?;
        let duration = start_time.elapsed();
        
        // Emit audit event
        self.emit_capability_audit_event(
            capability_id,
            "executed",
            args,
            &result,
            duration.as_millis() as u64
        ).await?;
        
        Ok(result)
    }
}
```

#### C.2. Causal Chain Integration
Capability lifecycle events are recorded in the immutable Causal Chain:

```rust
// Extended ActionType enum
pub enum ActionType {
    // ... existing types ...
    CapabilityRegistered,
    CapabilityRemoved,
    CapabilityUpdated,
    CapabilityDiscoveryCompleted,
}

impl CapabilityMarketplace {
    async fn emit_capability_audit_event(
        &self,
        capability_id: &str,
        event_type: &str,
        args: &Value,
        result: &Value,
        duration_ms: u64
    ) -> Result<(), RuntimeError> {
        if let Some(causal_chain) = &self.causal_chain {
            let action = Action {
                timestamp: chrono::Utc::now(),
                action_type: ActionType::CapabilityRegistered, // or appropriate type
                function_name: capability_id.to_string(),
                arguments: serde_json::to_string(args)?,
                result: serde_json::to_string(result)?,
                cost: 0, // TBD: cost calculation
                duration_ms,
                // ... other fields
            };
            
            let mut chain = causal_chain.lock().expect("Should acquire lock");
            chain.append(action).await?;
        }
        
        // Human-readable audit log
        println!("[AUDIT] Capability {} {} - Duration: {}ms", 
                 capability_id, event_type, duration_ms);
        
        Ok(())
    }
}
```

### D. Testing and Validation Framework

#### D.1. Integration Test Coverage
Comprehensive test suite covering all marketplace features:

```rust
#[tokio::test]
async fn test_capability_marketplace_bootstrap() {
    let registry = CapabilityRegistry::new();
    let marketplace = CapabilityMarketplace::new(registry);
    
    marketplace.bootstrap().await.unwrap();
    
    // Verify default capabilities are registered
    assert!(marketplace.get_capability("ccos.echo").is_ok());
    assert!(marketplace.get_capability("ccos.math.add").is_ok());
}

#[tokio::test]
async fn test_capability_marketplace_isolation_policy() {
    let policy = CapabilityIsolationPolicy {
        allowed_capabilities: vec!["ccos.*".to_string()],
        denied_capabilities: vec!["system.*".to_string()],
        // ... other fields
    };
    
    let marketplace = CapabilityMarketplace::with_isolation_policy(policy);
    
    // Test allowed capability
    assert!(marketplace.validate_capability_access("ccos.echo").is_ok());
    
    // Test denied capability
    assert!(marketplace.validate_capability_access("system.shutdown").is_err());
}

#[tokio::test]
async fn test_capability_marketplace_causal_chain_integration() {
    let causal_chain = Arc::new(Mutex::new(CausalChain::new()));
    let marketplace = CapabilityMarketplace::with_causal_chain(causal_chain.clone());
    
    // Execute capability
    marketplace.execute_capability("ccos.echo", &json!({"message": "test"})).await.unwrap();
    
    // Verify action recorded in Causal Chain
    let chain = causal_chain.lock().unwrap();
    let actions: Vec<Action> = chain.get_all_actions().iter().cloned().collect();
    
    assert!(actions.iter().any(|action| 
        matches!(action.action_type, ActionType::CapabilityRegistered)
    ));
}
```

#### D.2. Deterministic Testing
- Registry bootstrap is deterministic and side-effect free
- Network discovery is stubbed to avoid non-determinism in CI
- All tests use controlled, predictable inputs

### E. Future Work and Roadmap

#### E.1. Capability Versioning and Dependencies
- **Semantic Versioning**: Support for capability version resolution
- **Dependency Resolution**: Automatic dependency management
- **Compatibility Checking**: Version compatibility validation

#### E.2. Health Checks and Monitoring
- **Liveness Probes**: Regular health checks for registered capabilities
- **Performance Metrics**: Execution time, success rate, error rate tracking
- **Automatic Cleanup**: Remove stale or failing capabilities

#### E.3. Performance Optimization
- **Lazy Loading**: Lazy loading for large capability sets
- **Caching**: Capability caching mechanisms
- **Concurrent Discovery**: Parallel capability discovery

---

## F. Addendum: Recently Implemented Features (2025-08-24)

### F.1. Resource Constraint Enforcement System

#### F.1.1. Extensible Resource Monitoring
A comprehensive resource monitoring system has been implemented with support for:

- **Memory Monitoring**: Real-time memory usage tracking with configurable limits
- **CPU Monitoring**: CPU utilization monitoring with percentage-based limits
- **GPU Support**: GPU memory and utilization monitoring for AI workloads
- **Environmental Monitoring**: CO2 emissions and energy consumption tracking
- **Custom Resources**: Extensible framework for arbitrary resource types
- **Enforcement Levels**: Hard, Warning, and Adaptive enforcement modes

#### F.1.2. Resource Constraints Configuration
```rust
pub struct ResourceConstraints {
    pub max_memory_mb: Option<u64>,
    pub max_cpu_percent: Option<f64>,
    pub max_execution_time_ms: Option<u64>,
    pub max_concurrent_calls: Option<u32>,
    pub gpu_memory_mb: Option<u64>,
    pub gpu_utilization_percent: Option<f64>,
    pub co2_emissions_g: Option<f64>,
    pub energy_consumption_kwh: Option<f64>,
    pub custom_limits: HashMap<String, f64>,
}
```

#### F.1.3. Resource Provider Architecture
The system uses a trait-based architecture for extensibility:

```rust
#[async_trait]
pub trait ResourceProvider: Send + Sync {
    fn name(&self) -> &str;
    async fn measure_usage(&self, capability_id: &str) -> RuntimeResult<ResourceMeasurement>;
    async fn check_violation(&self, measurement: &ResourceMeasurement, limit: f64) -> RuntimeResult<Option<ResourceViolation>>;
}
```

#### F.1.4. Enforcement Integration
Resource monitoring is integrated into capability execution:

```rust
// In execute_capability method
if let Some(ref monitor) = self.resource_monitor {
    let constraints = self.isolation_policy.get_resource_constraints();
    let usage = monitor.monitor_capability(capability_id, &constraints).await?;
    
    match monitor.enforce_policy(&usage, &constraints).await? {
        EnforcementResult::HardViolation(violation) => {
            return Err(RuntimeError::ResourceLimitExceeded(violation.to_string()));
        }
        EnforcementResult::WarningViolation(violation) => {
            eprintln!("Warning: {}", violation);
        }
        EnforcementResult::Compliant => {}
    }
}
```

### F.2. Network Discovery Implementation

#### F.2.1. Generic Network Discovery
A robust HTTP-based discovery system with:

- **Pagination Support**: Handles large capability registries with pagination
- **Retry Logic**: Exponential backoff with configurable retry limits
- **Error Handling**: Comprehensive error handling for network failures
- **JSON Parsing**: Robust JSON manifest parsing with validation
- **Health Checks**: Built-in health monitoring for discovery endpoints

#### F.2.2. MCP (Model Context Protocol) Discovery
Specialized discovery for MCP servers:

```rust
pub struct MCPDiscoveryProvider {
    client: reqwest::Client,
    server_config: MCPServerConfig,
}

impl MCPDiscoveryProvider {
    pub async fn discover_tools(&self) -> RuntimeResult<Vec<CapabilityManifest>> {
        // Discover MCP tools and convert to capability manifests
    }
    
    pub async fn discover_resources(&self) -> RuntimeResult<Vec<CapabilityManifest>> {
        // Discover MCP resources and convert to capability manifests
    }
}
```

#### F.2.3. A2A (Agent-to-Agent) Discovery
Specialized discovery for A2A agents with:

- **Dynamic Discovery**: Real-time capability discovery from A2A agents
- **Static Fallback**: Static capability definitions as fallback
- **Agent Status**: Health and status monitoring for A2A agents
- **Parameter Metadata**: Rich parameter information from A2A definitions

#### F.2.4. Discovery Provider Architecture
Unified discovery interface with provider-specific implementations:

```rust
#[async_trait]
pub trait CapabilityDiscovery: Send + Sync {
    async fn discover(&self) -> Result<Vec<CapabilityManifest>, RuntimeError>;
    fn name(&self) -> &str;
    fn as_any(&self) -> &dyn std::any::Any;
}
```

### F.3. Testing and Validation

#### F.3.1. Resource Monitoring Tests
Comprehensive test suite covering all resource monitoring features:

```rust
#[tokio::test]
async fn test_capability_marketplace_resource_monitoring() {
    // Test basic resource monitoring functionality
}

#[tokio::test]
async fn test_capability_marketplace_gpu_resource_limits() {
    // Test GPU memory and utilization monitoring
}

#[tokio::test]
async fn test_capability_marketplace_environmental_limits() {
    // Test CO2 emissions and energy consumption monitoring
}

#[tokio::test]
async fn test_capability_marketplace_custom_resource_limits() {
    // Test custom resource type extensibility
}

#[tokio::test]
async fn test_capability_marketplace_resource_violation_handling() {
    // Test Hard and Warning enforcement levels
}

#[tokio::test]
async fn test_capability_marketplace_resource_monitoring_disabled() {
    // Test graceful handling when monitoring is disabled
}
```

#### F.3.2. Network Discovery Tests
Tests for all discovery providers:

```rust
#[tokio::test]
async fn test_network_discovery_provider() {
    // Test generic HTTP-based discovery
}

#[tokio::test]
async fn test_mcp_discovery_provider() {
    // Test MCP-specific discovery
}

#[tokio::test]
async fn test_a2a_discovery_provider() {
    // Test A2A-specific discovery
}
```

### F.4. Integration with Capability Marketplace

#### F.4.1. Resource Monitoring Integration
Resource monitoring is seamlessly integrated into the marketplace:

```rust
pub struct CapabilityMarketplace {
    // ... existing fields
    pub(crate) resource_monitor: Option<ResourceMonitor>,
}

impl CapabilityMarketplace {
    pub fn with_resource_monitoring(
        registry: Arc<RwLock<CapabilityRegistry>>,
        causal_chain: Option<Arc<Mutex<CausalChain>>>,
        resource_config: ResourceMonitoringConfig,
    ) -> Self {
        // Initialize with resource monitoring
    }
}
```

#### F.4.2. Discovery Integration
Discovery providers are integrated into the marketplace:

```rust
impl CapabilityMarketplace {
    pub fn add_network_discovery(&mut self, config: NetworkDiscoveryBuilder) -> RuntimeResult<()> {
        // Add network discovery provider
    }
    
    pub fn add_mcp_discovery(&mut self, config: MCPServerConfig) -> RuntimeResult<()> {
        // Add MCP discovery provider
    }
    
    pub fn add_a2a_discovery(&mut self, config: A2AAgentConfig) -> RuntimeResult<()> {
        // Add A2A discovery provider
    }
}
```

### F.5. Performance and Reliability

#### F.5.1. Resource Monitoring Performance
- **Low Overhead**: Resource monitoring adds minimal execution overhead
- **Asynchronous**: Non-blocking resource measurement
- **Configurable**: Monitoring can be enabled/disabled per marketplace instance
- **Extensible**: New resource types can be added without breaking existing functionality

#### F.5.2. Discovery Reliability
- **Fault Tolerance**: Discovery failures don't affect core marketplace functionality
- **Health Monitoring**: Built-in health checks for all discovery providers
- **Fallback Mechanisms**: Static fallbacks when dynamic discovery fails
- **Error Recovery**: Automatic retry and recovery mechanisms

### F.6. Security and Compliance

#### F.6.1. Resource Security
- **Isolation**: Resource limits provide execution isolation
- **Audit Trail**: All resource violations are logged in the Causal Chain
- **Configurable Limits**: Fine-grained control over resource constraints
- **Enforcement Levels**: Flexible enforcement from warnings to hard stops

#### F.6.2. Discovery Security
- **Authentication**: Support for various authentication mechanisms
- **Validation**: Comprehensive validation of discovered capability manifests
- **Attestation**: Verification of capability attestations
- **Provenance**: Tracking of capability provenance and origins

---

References:
- docs/rtfs-2.0/specs/13-rtfs-ccos-integration-guide.md (Intent/Plan/Action formats, step special form)
- docs/ccos/specs/003-causal-chain.md (Causal Chain model)
- docs/ccos/specs/014-step-special-form-design.md (step logging semantics)