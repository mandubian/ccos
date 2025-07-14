# Global Function Mesh

**Status:** Outline – v0.1 (placeholder)

---

## Purpose

Provide a universal, decentralized naming and discovery system for functions and capabilities across the CCOS ecosystem. Think of it as **DNS for functions**.

## Architecture Integration

### **Current CCOS Architecture**

```
Environment → SecurityContext → CapabilityMarketplace → CapabilityProviders
```

### **Future with Global Function Mesh**

```
Environment → SecurityContext → GlobalFunctionMesh → CapabilityMarketplace → CapabilityProviders
                                       ↓
                               DecentralizedRegistry
                               (DNS for Functions)
```

### **Integration Benefits**

- **Universal Naming**: `image-processing/sharpen` resolves globally across all CCOS instances
- **Provider Discovery**: Automatic discovery of multiple providers for the same function
- **Load Balancing**: Intelligent routing based on SLA, cost, and availability
- **Decentralization**: No single point of failure for function resolution
- **Versioning**: Support for multiple versions of the same function

## Architecture Components

### **Global Function Mesh Core**

```rust
/// Global Function Mesh - Universal function resolution system
pub struct GlobalFunctionMesh {
    /// Local capability marketplace
    local_marketplace: CapabilityMarketplace,
    /// Decentralized registry for function discovery
    registry: DecentralizedRegistry,
    /// Cache for resolved capabilities
    resolution_cache: Arc<RwLock<HashMap<String, FunctionRecord>>>,
    /// Provider selection strategy
    selection_strategy: ProviderSelectionStrategy,
}

impl GlobalFunctionMesh {
    /// Resolve a function name to one or more providers
    pub async fn resolve_function(&self, func_name: &str) -> Result<Vec<CapabilityProvider>> {
        // 1. Check local marketplace first (fast path)
        if let Some(local) = self.local_marketplace.get_capability(func_name).await {
            return Ok(vec![local.provider]);
        }
        
        // 2. Check resolution cache
        if let Some(cached) = self.resolution_cache.read().await.get(func_name) {
            return Ok(cached.providers.clone());
        }
        
        // 3. Query decentralized registry
        let record = self.registry.lookup(func_name).await?;
        
        // 4. Convert to capability providers
        let providers = record.providers.iter()
            .map(|p| p.to_capability_provider())
            .collect();
            
        // 5. Cache the result
        self.resolution_cache.write().await.insert(func_name.to_string(), record);
        
        Ok(providers)
    }
    
    /// Select the best provider based on strategy
    pub async fn select_provider(&self, func_name: &str, context: &SecurityContext) -> Result<CapabilityProvider> {
        let providers = self.resolve_function(func_name).await?;
        
        // Apply security filtering
        let allowed_providers: Vec<_> = providers.into_iter()
            .filter(|p| context.is_provider_allowed(&p.id))
            .collect();
            
        if allowed_providers.is_empty() {
            return Err(SecurityViolation(format!("No allowed providers for function '{}'", func_name)));
        }
        
        // Select based on strategy (cost, performance, availability)
        self.selection_strategy.select_best(allowed_providers, context).await
    }
}
```

### **Decentralized Registry**

```rust
/// Decentralized registry for function records
pub trait DecentralizedRegistry: Send + Sync {
    /// Look up a function record by name
    async fn lookup(&self, func_name: &str) -> Result<FunctionRecord>;
    
    /// Register a new function record
    async fn register(&self, record: FunctionRecord) -> Result<()>;
    
    /// Update an existing function record
    async fn update(&self, func_name: &str, record: FunctionRecord) -> Result<()>;
    
    /// List all functions matching a pattern
    async fn list(&self, pattern: &str) -> Result<Vec<FunctionRecord>>;
}

/// Implementation using Git-based storage
pub struct GitRegistry {
    repo_url: String,
    local_cache: PathBuf,
    sync_interval: Duration,
}

/// Implementation using IPFS
pub struct IpfsRegistry {
    ipfs_client: IpfsClient,
    content_cache: Arc<RwLock<HashMap<String, FunctionRecord>>>,
}

/// Implementation using blockchain
pub struct BlockchainRegistry {
    contract_address: String,
    web3_client: Web3Client,
}
```

### **Security Integration**

```rust
/// Extended security context for global functions
impl SecurityContext {
    /// Check if a global function is allowed
    pub fn is_global_function_allowed(&self, func_name: &str) -> bool {
        match self.security_level {
            SecurityLevel::Pure => false,
            SecurityLevel::Controlled => {
                // Check function whitelist
                self.allowed_global_functions.contains(func_name) ||
                self.allowed_function_patterns.iter().any(|pattern| {
                    glob::Pattern::new(pattern).unwrap().matches(func_name)
                })
            },
            SecurityLevel::Full => true,
        }
    }
    
    /// Check if a provider is allowed
    pub fn is_provider_allowed(&self, provider_id: &str) -> bool {
        match self.security_level {
            SecurityLevel::Pure => false,
            SecurityLevel::Controlled => {
                self.allowed_providers.contains(provider_id)
            },
            SecurityLevel::Full => true,
        }
    }
}
```

---

## Key Responsibilities (MVP)

1. **Universal Identifiers** – Map a canonical name like `image-processing/sharpen` to one or more providers.
2. **Decentralized Registry** – Pluggable back-end (Git repo, IPFS, blockchain, etc.)
3. **Versioning & Namespaces** – Allow multiple versions and vendor namespaces to coexist.
4. **Provider Metadata Stub** – Minimal pointer to Capability Marketplace listing (SLA lives there).
5. **Security Integration** – Respect CCOS security contexts and provider permissions.

---

## Data Model (draft)

```rtfs
{:type :ccos.mesh:v0.func-record,
 :func-name "image-processing/sharpen",
 :latest-version "1.2.0",
 :namespace "image-processing",
 :description "Sharpen images using various algorithms",
 :providers [
   {:id "provider-123",
    :capability-ref "marketplace://offer/abc",
    :endpoint "https://api.provider123.com/sharpen",
    :sla-tier "premium",
    :cost-per-call 0.01,
    :avg-response-time-ms 150},
   {:id "provider-456",
    :capability-ref "marketplace://offer/def",
    :endpoint "mcp://image-server.local/sharpen",
    :sla-tier "standard",
    :cost-per-call 0.005,
    :avg-response-time-ms 300}
 ],
 :versions {
   "1.0.0" {:providers [...], :deprecated true},
   "1.1.0" {:providers [...], :deprecated false},
   "1.2.0" {:providers [...], :deprecated false}
 },
 :security-requirements {
   :min-security-level :controlled,
   :required-permissions ["image.read", "image.write"],
   :trusted-providers ["provider-123"]
 }}
```

## Integration Examples

### **Basic Function Resolution**

```rtfs
;; Current: Local capability call
(call :ccos.echo "hello")

;; Future: Global function mesh resolution
(call :image-processing/sharpen {:image data :strength 0.8})
;; Resolves to: provider-123 (best SLA) or provider-456 (lower cost)
```

### **Provider Selection Strategies**

```rust
/// Provider selection strategies
pub enum ProviderSelectionStrategy {
    /// Select cheapest provider
    LowestCost,
    /// Select fastest provider
    FastestResponse,
    /// Select most reliable provider
    HighestAvailability,
    /// Load balance across multiple providers
    LoadBalance,
    /// Custom selection logic
    Custom(Box<dyn Fn(&[CapabilityProvider], &SecurityContext) -> CapabilityProvider>),
}

// Usage
let mesh = GlobalFunctionMesh::new()
    .with_strategy(ProviderSelectionStrategy::LowestCost)
    .with_registry(GitRegistry::new("https://github.com/ccos/function-mesh"))
    .with_security_context(SecurityContext::controlled(...));
```

### **Capability Registration**

```rust
// Register a new capability with the global mesh
let record = FunctionRecord {
    func_name: "data-analysis/sentiment".to_string(),
    latest_version: "2.0.0".to_string(),
    providers: vec![
        ProviderRef {
            id: "nlp-service-pro".to_string(),
            capability_ref: "marketplace://nlp-pro/sentiment-v2".to_string(),
            endpoint: "https://api.nlp-pro.com/sentiment".to_string(),
            sla_tier: "enterprise".to_string(),
            cost_per_call: 0.02,
        }
    ],
    security_requirements: SecurityRequirements {
        min_security_level: SecurityLevel::Controlled,
        required_permissions: vec!["data.read".to_string()],
        trusted_providers: vec!["nlp-service-pro".to_string()],
    },
};

mesh.register_function(record).await?;
```

---

## Open Questions

- **Governance of name collisions?** 
  - Proposed: Namespace-based resolution with vendor prefixes
  - Example: `acme.com/image-processing/sharpen` vs `vendor2.org/image-processing/sharpen`

- **Recommended discovery transport (libp2p? https API?)**
  - Proposed: Multi-transport support with fallback hierarchy
  - Primary: HTTPS API, Secondary: libp2p, Tertiary: Local cache

- **Caching & TTL semantics**
  - Proposed: Configurable TTL with invalidation on provider health changes
  - Local cache: 5min, Regional cache: 1hr, Global cache: 24hr

- **Security & Trust Model**
  - How to verify provider authenticity and capability integrity?
  - Proposed: Cryptographic signatures and provider reputation system

- **Economic Model**
  - How to handle pricing, billing, and cost optimization?
  - Proposed: Integration with capability marketplace billing system

---

## Implementation Roadmap

### **Phase 1: Core Infrastructure** 📋 PLANNED
- [ ] Basic function record data model
- [ ] Git-based registry implementation
- [ ] Local resolution cache
- [ ] Security context integration

### **Phase 2: Provider Integration** 📋 PLANNED
- [ ] CapabilityMarketplace integration
- [ ] Provider selection strategies
- [ ] Health monitoring and failover
- [ ] Cost optimization algorithms

### **Phase 3: Decentralization** 📋 PLANNED
- [ ] IPFS registry implementation
- [ ] Blockchain registry implementation
- [ ] Multi-registry federation
- [ ] Conflict resolution mechanisms

### **Phase 4: Production Features** 📋 PLANNED
- [ ] Provider reputation system
- [ ] Automatic capability discovery
- [ ] Performance monitoring
- [ ] Billing integration

---

## Roadmap Alignment

Phase 9 in `RTFS_MIGRATION_PLAN.md` – **Global Function Mesh V1**.

**Current Status**: Architecture defined, integrated with existing security framework and capability marketplace. Ready for implementation.

---
