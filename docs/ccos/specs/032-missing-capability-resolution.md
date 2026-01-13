# Missing Capability Resolution System

**Status**: Authoritative  
**Version**: 1.2  
**Last Updated**: 2025-12-25  
**Scope**: Complete missing capability detection, resolution, and registration system

---

## Implementation Status

| Feature | Status | Location |
|---------|--------|----------|
| `MissingCapabilityResolver` | ✅ Implemented | `synthesis/core/missing_capability_resolver.rs` |
| `MissingCapabilityStrategy` trait | ✅ Implemented | `synthesis/core/missing_capability_strategies.rs` |
| `PureRtfsGenerationStrategy` | ✅ Implemented | `synthesis/core/missing_capability_strategies.rs` |
| `ExternalLlmHintStrategy` | ✅ Implemented | `synthesis/core/missing_capability_strategies.rs` |
| Planner integration via `step_discover_new_tools` | ✅ Implemented | `planner/modular_planner/steps.rs` |
| Planner retry loop `step_resolve_with_discovery` | ✅ Implemented | `planner/modular_planner/steps.rs` |
| `planner.discover_tools` capability | ✅ Implemented | `planner/capabilities_v2.rs` |
| Meta-planner discovery integration | ✅ Implemented | `capabilities/core/meta-planner.rtfs` |
| Continuous resolution loop | 🔄 Partial | `synthesis/runtime/continuous_resolution.rs` |
| Deferred execution checkpoints | 📋 Planned | - |

---

## 1. Overview

The Missing Capability Resolution System enables CCOS to automatically detect, discover, source, and register capabilities that are referenced but not yet available. This creates a self-healing and self-extending capability ecosystem.

### Core Philosophy

1. **No Stubs**: Never create placeholder capabilities; defer execution instead
2. **Discovery-First**: Search existing registries before synthesizing
3. **Security-Aware**: All discovery respects `RuntimeContext` permissions
4. **Auditable**: Full trace of resolution decisions for debugging

---

## 2. Architecture

```
┌────────────────────────────────────────────────────────────────────┐
│                    Missing Capability Resolution                    │
├────────────────────────────────────────────────────────────────────┤
│                                                                      │
│  ┌──────────────┐   ┌──────────────┐   ┌──────────────┐            │
│  │  Detection   │──▶│  Resolution  │──▶│ Registration │            │
│  │    Phase     │   │    Phase     │   │    Phase     │            │
│  └──────────────┘   └──────────────┘   └──────────────┘            │
│         │                  │                   │                    │
│         ▼                  ▼                   ▼                    │
│  ┌──────────────┐   ┌──────────────┐   ┌──────────────┐            │
│  │ Dependency   │   │   Fan-out    │   │  Validation  │            │
│  │ Extractor    │   │  Discovery   │   │  Governance  │            │
│  │ Runtime Trap │   │  Importers   │   │  Versioning  │            │
│  └──────────────┘   └──────────────┘   └──────────────┘            │
│                                                                      │
└────────────────────────────────────────────────────────────────────┘
```

---

## 3. Detection Phase

### 3.1 Dependency Extraction

The `DependencyExtractor` scans RTFS code for capability references:

```rust
pub struct DependencyExtractor;

impl DependencyExtractor {
    /// Extract all capability IDs from RTFS code
    pub fn extract_dependencies(code: &str) -> Vec<String> {
        // Parses (call :capability.id ...) patterns
    }
    
    /// Find capabilities that don't exist in marketplace
    pub fn find_missing_capabilities(
        dependencies: &[String],
        marketplace: &CapabilityMarketplace,
    ) -> Vec<String> { ... }
}
```

### 3.2 Runtime Trap

The `CapabilityRegistry` intercepts calls to non-existent capabilities:

```rust
impl CapabilityRegistry {
    pub fn invoke_capability(
        &self,
        capability_id: &str,
        args: &Value,
        context: &RuntimeContext,
    ) -> RuntimeResult<Value> {
        // Check if capability exists
        if !self.has_capability(capability_id) {
            // Enqueue for resolution
            self.enqueue_missing_capability(capability_id, args, context)?;
            
            // Return deferred execution
            return Err(RuntimeError::MissingCapability {
                id: capability_id.to_string(),
                deferred: true,
            });
        }
        
        // Normal execution
        self.execute_capability(capability_id, args, context)
    }
}
```

### 3.3 Resolution Queue

```rust
pub struct ResolutionQueue {
    pending: VecDeque<PendingResolution>,
    in_progress: HashMap<String, ResolutionStatus>,
    completed: HashMap<String, ResolutionResult>,
}

pub struct PendingResolution {
    pub capability_id: String,
    pub requested_at: DateTime<Utc>,
    pub context: Option<ResolutionContext>,
    pub priority: ResolutionPriority,
    pub backoff_state: Option<BackoffState>,
}
```

---

## 4. Resolution Phase

### 4.1 Discovery Pipeline (Fan-out)

The system searches multiple sources in priority order:

```rust
pub struct MissingCapabilityResolver {
    // Discovery sources
    marketplace: Arc<CapabilityMarketplace>,
    mcp_discovery: Arc<MCPDiscoveryService>,
    registry_client: MCPRegistryClient,
    
    // Importers
    openapi_importer: OpenAPIImporter,
    graphql_importer: GraphQLImporter,
    http_wrapper: HTTPWrapper,
    
    // Strategies
    strategies: Vec<Box<dyn MissingCapabilityStrategy>>,
    
    // State
    resolution_queue: Arc<Mutex<ResolutionQueue>>,
    trust_registry: Arc<ServerTrustRegistry>,
}
```

### 4.2 Search Order

1. **Exact Match**: Check marketplace for exact capability ID
2. **Partial Match**: Find similar capabilities by name/domain
3. **MCP Registry**: Query official MCP Registry for matching servers
4. **Local Aliases**: Check `capabilities/mcp/aliases.json` for known mappings
5. **Curated Overrides**: Check `capabilities/mcp/overrides.json` for curated servers
6. **OpenAPI Discovery**: Search for OpenAPI specs that might provide the capability
7. **LLM Synthesis**: Generate capability using LLM with guardrails

### 4.3 Discovery Strategies

```rust
pub trait MissingCapabilityStrategy: Send + Sync {
    /// Strategy name for logging
    fn name(&self) -> &str;
    
    /// Check if this strategy can handle the capability
    fn can_resolve(&self, capability_id: &str, context: &ResolutionContext) -> bool;
    
    /// Attempt to resolve the capability
    async fn resolve(
        &self,
        capability_id: &str,
        context: &ResolutionContext,
    ) -> RuntimeResult<Option<CapabilityManifest>>;
}
```

**Built-in Strategies:**

| Strategy | Description | Priority |
|----------|-------------|----------|
| `MarketplaceSearchStrategy` | Search existing marketplace | 1 |
| `MCPRegistryStrategy` | Query MCP registry | 2 |
| `AliasLookupStrategy` | Check alias mappings | 3 |
| `OpenAPIImportStrategy` | Import from OpenAPI specs | 4 |
| `UserInteractionStrategy` | Ask user for guidance | 5 |
| `PureRtfsGenerationStrategy` | Generate RTFS implementation | 6 |
| `ExternalLlmHintStrategy` | LLM-assisted resolution | 7 |

---

## 5. MCP Registry Integration

### 5.1 Registry Search

```rust
impl MissingCapabilityResolver {
    pub async fn search_mcp_registry(
        &self,
        capability_id: &str,
    ) -> RuntimeResult<Vec<McpServer>> {
        // Extract search terms from capability ID
        let terms = self.extract_search_terms(capability_id);
        
        // Query registry
        let servers = self.registry_client.search_servers(&terms).await?;
        
        // Rank by relevance
        let ranked = self.rank_server_candidates(&servers, capability_id);
        
        Ok(ranked)
    }
}
```

### 5.2 Server Selection

When multiple servers match, the system applies ranking:

```rust
pub struct ServerCandidate {
    pub server: McpServer,
    pub relevance_score: f64,
    pub trust_tier: TrustTier,
    pub tool_match_score: f64,
}

pub fn rank_candidates(candidates: &mut [ServerCandidate]) {
    candidates.sort_by(|a, b| {
        // Composite score: relevance * trust_weight + tool_match
        let score_a = a.relevance_score * a.trust_tier.weight() + a.tool_match_score;
        let score_b = b.relevance_score * b.trust_tier.weight() + b.tool_match_score;
        score_b.partial_cmp(&score_a).unwrap()
    });
}
```

### 5.3 Trust Tiers

```rust
pub enum TrustTier {
    Official,     // modelcontextprotocol/* servers
    Verified,     // Verified by registry
    Community,    // Community contributions
    Unknown,      // New/unverified servers
}

impl TrustTier {
    pub fn weight(&self) -> f64 {
        match self {
            TrustTier::Official => 1.0,
            TrustTier::Verified => 0.8,
            TrustTier::Community => 0.5,
            TrustTier::Unknown => 0.2,
        }
    }
}
```

---

## 6. Importers

### 6.1 OpenAPI Importer

Converts OpenAPI specifications to CCOS capabilities:

```rust
pub struct OpenAPIImporter {
    http_client: reqwest::Client,
    auth_config: Option<AuthConfig>,
}

impl OpenAPIImporter {
    /// Import a single operation
    pub async fn operation_to_capability(
        &self,
        operation: &OpenAPIOperation,
        spec_url: &str,
    ) -> RuntimeResult<CapabilityManifest> {
        let manifest = CapabilityManifest {
            id: self.generate_capability_id(operation),
            name: operation.operation_id.clone().unwrap_or_default(),
            description: operation.summary.clone().unwrap_or_default(),
            provider: ProviderType::OpenApi(OpenApiCapability {
                base_url: self.extract_base_url(spec_url),
                spec_url: Some(spec_url.to_string()),
                operations: vec![operation.clone()],
                auth: self.auth_config.clone(),
                timeout_ms: 30000,
            }),
            input_schema: self.parameters_to_schema(&operation.parameters),
            output_schema: self.response_to_schema(&operation.responses),
            ..Default::default()
        };
        
        Ok(manifest)
    }
}
```

### 6.2 GraphQL Importer

```rust
pub struct GraphQLImporter;

impl GraphQLImporter {
    pub async fn import_schema(
        &self,
        endpoint: &str,
        introspection_query: Option<&str>,
    ) -> RuntimeResult<Vec<CapabilityManifest>> {
        // Introspect schema
        let schema = self.introspect_schema(endpoint).await?;
        
        // Generate capabilities for queries and mutations
        let mut capabilities = Vec::new();
        
        for query in &schema.query_type.fields {
            capabilities.push(self.field_to_capability(query, "query")?);
        }
        
        for mutation in &schema.mutation_type.fields {
            capabilities.push(self.field_to_capability(mutation, "mutation")?);
        }
        
        Ok(capabilities)
    }
}
```

### 6.3 HTTP Wrapper

Generic HTTP API wrapper for non-standard APIs:

```rust
pub struct HTTPWrapper {
    templates: HashMap<String, HTTPTemplate>,
}

pub struct HTTPTemplate {
    pub method: String,
    pub url_pattern: String,
    pub headers: HashMap<String, String>,
    pub body_template: Option<String>,
    pub response_path: Option<String>,
}

impl HTTPWrapper {
    pub fn wrap_as_capability(
        &self,
        name: &str,
        template: HTTPTemplate,
    ) -> CapabilityManifest {
        CapabilityManifest {
            id: format!("http.{}", name),
            provider: ProviderType::Http(HttpCapability {
                base_url: template.url_pattern.clone(),
                auth_token: None,
                timeout_ms: 30000,
            }),
            ..Default::default()
        }
    }
}
```

---

## 7. Deferred Execution Model

### 7.1 Checkpoint Creation

Instead of creating stubs, execution is paused:

```rust
pub struct CheckpointRecord {
    pub checkpoint_id: String,
    pub plan_id: String,
    pub intent_id: String,
    pub evaluator_state: EvaluatorSnapshot,
    pub missing_capabilities: Vec<String>,
    pub created_at: DateTime<Utc>,
    pub auto_resume_enabled: bool,
}

impl Orchestrator {
    pub fn checkpoint_for_missing_capability(
        &self,
        plan_id: &str,
        missing_id: &str,
        evaluator: &Evaluator,
    ) -> RuntimeResult<CheckpointRecord> {
        let checkpoint = CheckpointRecord {
            checkpoint_id: generate_checkpoint_id(),
            plan_id: plan_id.to_string(),
            intent_id: self.current_intent_id.clone(),
            evaluator_state: evaluator.snapshot(),
            missing_capabilities: vec![missing_id.to_string()],
            created_at: Utc::now(),
            auto_resume_enabled: true,
        };
        
        self.checkpoint_archive.store(&checkpoint)?;
        
        Ok(checkpoint)
    }
}
```

### 7.2 Auto-Resume

When capabilities are resolved, waiting checkpoints resume:

```rust
impl MissingCapabilityResolver {
    pub async fn on_capability_registered(
        &self,
        capability_id: &str,
    ) -> RuntimeResult<()> {
        // Find checkpoints waiting for this capability
        let checkpoints = self.checkpoint_archive
            .find_waiting_for(capability_id);
        
        for checkpoint in checkpoints {
            if self.can_resume(&checkpoint) {
                // Emit audit event
                self.emit_auto_resume_ready(&checkpoint).await?;
                
                // Resume if fully resolved
                if checkpoint.all_capabilities_resolved() {
                    self.resume_checkpoint(&checkpoint).await?;
                }
            }
        }
        
        Ok(())
    }
}
```

---

## 8. Validation and Governance

### 8.1 Validation Pipeline

```rust
pub struct ValidationHarness {
    static_analyzers: Vec<Box<dyn StaticAnalyzer>>,
    governance_policies: Vec<Box<dyn GovernancePolicy>>,
}

impl ValidationHarness {
    pub async fn validate_capability(
        &self,
        manifest: &CapabilityManifest,
        rtfs_code: Option<&str>,
    ) -> RuntimeResult<ValidationResult> {
        let mut issues = Vec::new();
        
        // Static analysis
        for analyzer in &self.static_analyzers {
            issues.extend(analyzer.analyze(manifest, rtfs_code)?);
        }
        
        // Governance policies
        for policy in &self.governance_policies {
            issues.extend(policy.check(manifest)?);
        }
        
        Ok(ValidationResult {
            passed: issues.iter().all(|i| !i.is_blocking()),
            issues,
        })
    }
}
```

### 8.2 Governance Policies

```rust
pub trait GovernancePolicy: Send + Sync {
    fn name(&self) -> &str;
    fn check(&self, manifest: &CapabilityManifest) -> RuntimeResult<Vec<PolicyIssue>>;
}

// Built-in policies
pub struct TrustTierPolicy;        // Enforce minimum trust
pub struct EffectsPolicy;          // Validate declared effects
pub struct PermissionsPolicy;      // Check required permissions
pub struct DomainAllowlistPolicy;  // Enforce domain restrictions
```

### 8.3 Static Analyzers

```rust
pub trait StaticAnalyzer: Send + Sync {
    fn name(&self) -> &str;
    fn analyze(
        &self,
        manifest: &CapabilityManifest,
        code: Option<&str>,
    ) -> RuntimeResult<Vec<AnalysisIssue>>;
}

// Built-in analyzers
pub struct SchemaConsistencyAnalyzer;  // Input/output schema validation
pub struct SecurityPatternAnalyzer;    // Dangerous patterns in code
pub struct DependencyAnalyzer;         // Check dependency availability
```

---

## 9. Registration Phase

### 9.1 Registration Flow

```rust
pub struct RegistrationFlow {
    marketplace: Arc<CapabilityMarketplace>,
    catalog: Arc<CatalogService>,
    validation_harness: ValidationHarness,
}

impl RegistrationFlow {
    pub async fn register_capability(
        &self,
        manifest: CapabilityManifest,
    ) -> RuntimeResult<RegistrationResult> {
        // 1. Validate
        let validation = self.validation_harness.validate(&manifest, None).await?;
        if !validation.passed {
            return Err(RuntimeError::ValidationFailed(validation.issues));
        }
        
        // 2. Version check
        let update_result = self.marketplace
            .update_capability(manifest.clone(), false)
            .await?;
        
        // 3. Catalog indexing
        self.catalog.register_capability(
            &manifest,
            CatalogSource::Discovered,
        );
        
        // 4. Emit audit event
        self.emit_registration_event(&manifest, &update_result).await?;
        
        Ok(RegistrationResult {
            capability_id: manifest.id,
            version: manifest.version,
            update_info: update_result,
        })
    }
}
```

### 9.2 Version Handling

```rust
// When registering a discovered capability
match update_result.version_comparison {
    VersionComparison::Equal => {
        // Same version, skip registration
    }
    VersionComparison::PatchUpdate | VersionComparison::MinorUpdate => {
        // Safe update, auto-register
    }
    VersionComparison::MajorUpdate => {
        if update_result.breaking_changes.is_empty() {
            // Major but no detected breaks, register with warning
        } else {
            // Breaking changes, require approval
            return Err(RuntimeError::BreakingChangesDetected(
                update_result.breaking_changes
            ));
        }
    }
    VersionComparison::Downgrade => {
        // Never auto-downgrade
        return Err(RuntimeError::VersionDowngrade);
    }
}
```

---

## 10. Backoff and Retry

### 10.1 Backoff Strategy

```rust
pub struct BackoffState {
    pub attempt_count: u32,
    pub last_attempt: DateTime<Utc>,
    pub next_attempt: DateTime<Utc>,
    pub backoff_seconds: u64,
}

impl BackoffState {
    pub fn new() -> Self {
        Self {
            attempt_count: 0,
            last_attempt: Utc::now(),
            next_attempt: Utc::now(),
            backoff_seconds: 1,
        }
    }
    
    pub fn increment(&mut self) {
        self.attempt_count += 1;
        self.last_attempt = Utc::now();
        
        // Exponential backoff with cap
        self.backoff_seconds = (self.backoff_seconds * 2).min(3600);
        self.next_attempt = Utc::now() + Duration::seconds(self.backoff_seconds as i64);
    }
    
    pub fn should_retry(&self) -> bool {
        Utc::now() >= self.next_attempt && self.attempt_count < 10
    }
}
```

### 10.2 Risk Assessment

The resolver uses risk assessment to prioritize and gate resolution:

```rust
pub struct RiskAssessment {
    pub risk_level: RiskLevel,
    pub factors: Vec<RiskFactor>,
    pub recommendation: RiskRecommendation,
}

pub enum RiskLevel {
    Low,      // Auto-resolve
    Medium,   // Resolve with logging
    High,     // Require human approval
    Critical, // Block resolution
}

impl MissingCapabilityResolver {
    pub fn assess_risk(&self, candidate: &ServerCandidate) -> RiskAssessment {
        let mut factors = Vec::new();
        
        // Trust tier
        if matches!(candidate.trust_tier, TrustTier::Unknown) {
            factors.push(RiskFactor::UnknownServer);
        }
        
        // Effects
        if candidate.declares_effects(&["network", "filesystem"]) {
            factors.push(RiskFactor::SideEffects);
        }
        
        // Permissions
        if candidate.requires_permissions(&["admin", "delete"]) {
            factors.push(RiskFactor::ElevatedPermissions);
        }
        
        RiskAssessment::from_factors(factors)
    }
}
```

---

## 11. Continuous Resolution Loop

### 11.1 Background Processing

```rust
pub struct ContinuousResolutionLoop {
    resolver: Arc<MissingCapabilityResolver>,
    poll_interval: Duration,
    running: AtomicBool,
}

impl ContinuousResolutionLoop {
    pub async fn run(&self) {
        while self.running.load(Ordering::Relaxed) {
            // Get pending resolutions
            let pending = self.resolver.get_pending_resolutions();
            
            for resolution in pending {
                // Check backoff
                if !resolution.backoff_state.should_retry() {
                    continue;
                }
                
                // Assess risk
                let risk = self.resolver.assess_risk(&resolution);
                
                // Handle based on risk
                match risk.risk_level {
                    RiskLevel::Low | RiskLevel::Medium => {
                        self.auto_resolve(&resolution).await;
                    }
                    RiskLevel::High => {
                        self.request_human_approval(&resolution).await;
                    }
                    RiskLevel::Critical => {
                        self.block_resolution(&resolution).await;
                    }
                }
            }
            
            tokio::time::sleep(self.poll_interval).await;
        }
    }
}
```

---

## 12. LLM Synthesis Governance Gate

### 12.1 Overview

Before invoking LLM synthesis, the `MissingCapabilityResolver` runs a governance gate to assess risk and block or approve synthesis based on capability characteristics.

### 12.2 Governance Flow

```rust
// In resolve_capability() → attempt_llm_capability_synthesis branch
if self.feature_checker.is_llm_synthesis_enabled() {
    let risk = RiskAssessment::assess(&capability_id, &self.config);
    
    match risk.priority {
        ResolutionPriority::Critical => {
            // Block synthesis entirely
            self.emit_event(&capability_id, "governance_blocked", ...);
        }
        ResolutionPriority::High if risk.requires_human_approval => {
            // Skip synthesis, fall back to user interaction
            self.emit_event(&capability_id, "governance_approval_required", ...);
        }
        _ => {
            // Proceed with LLM synthesis
            self.attempt_llm_capability_synthesis(request, &capability_id).await?;
        }
    }
}
```

### 12.3 Risk Detection Keywords

Capabilities are assessed based on ID patterns:

| Pattern | Risk Factor | Example |
|---------|-------------|---------|
| `admin`, `root`, `sudo` | Security concerns | `admin.delete_user` |
| `payment`, `financial`, `billing` | PCI-DSS compliance | `payment.process` |
| `auth`, `credential` | High privilege access | `auth.reset_password` |
| `database`, `delete` | Data protection | `database.drop_table` |
| `pii`, `personal`, `gdpr` | GDPR compliance | `user.export_pii` |

### 12.4 RTFS Synthesis Prompts

The synthesis prompts include explicit warnings about Clojure syntax that is NOT supported in RTFS:

| Unsupported | Clojure Example | RTFS Alternative |
|-------------|-----------------|------------------|
| Quote syntax | `'()`, `'expr` | `[]` (empty vector) |
| Atoms/mutation | `atom`, `@var` | Pure state |
| Set literals | `#{1 2}` | Filter for uniqueness |
| Regex literals | `#"pattern"` | String functions |
| `cons` | `(cons x list)` | `(conj coll x)` |

These warnings are documented in `assets/prompts/cognitive_engine/capability_synthesis/v1/anti_patterns.md`.

---

## 13. CLI Tools

### 13.1 resolve-deps Command

```bash
# Resolve a specific capability
cargo run --bin resolve-deps -- resolve --capability-id github

# List pending resolutions
cargo run --bin resolve-deps -- list-pending

# Resume a checkpoint
cargo run --bin resolve-deps -- resume \
    --checkpoint-id checkpoint_123 \
    --capability-id github

# Show resolution status
cargo run --bin resolve-deps -- status --capability-id github
```

### 13.2 Example Output

```
🚀 Bootstrapped marketplace with 5 test capabilities
🔍 Resolving dependencies for capability: github
📋 Adding missing capability to resolution queue...
⚙️ Processing resolution queue...

🔍 DISCOVERY: Querying MCP Registry for 'github'
   Found 3 matching servers:
   1. modelcontextprotocol/github (official) - Score: 0.95
   2. community/github-extended - Score: 0.72
   3. my-github-server - Score: 0.45

✅ DISCOVERY: Selected 'modelcontextprotocol/github'
   Discovering tools...
   Found 47 tools

📝 VALIDATION: Checking governance policies...
   ✓ Trust tier: Official
   ✓ No dangerous effects
   ✓ Schema validated

✅ REGISTRATION: Registered capability 'mcp.github.list_issues'
   Version: 1.0.0
   Domain: github.issues
   Category: crud.read

✅ Dependency resolution completed!
```

---

## 14. Configuration

### 14.1 Feature Flags

```rust
pub struct MissingCapabilityConfig {
    pub enabled: bool,                    // Master switch
    pub auto_resolution_enabled: bool,    // Auto-resolve at runtime
    pub mcp_registry_enabled: bool,       // Query MCP registry
    pub human_approval_required: bool,    // Require approval for high-risk
    pub max_resolution_attempts: u32,     // Max retries per capability
    pub resolution_timeout_seconds: u64,  // Timeout per resolution
}
```

### 14.2 Environment Variables

| Variable | Default | Description |
|----------|---------|-------------|
| `CCOS_MISSING_CAPABILITY_ENABLED` | `false` | Master switch |
| `CCOS_AUTO_RESOLUTION_ENABLED` | `false` | Auto-resolve at runtime |
| `CCOS_MCP_REGISTRY_ENABLED` | `true` | Query MCP registry |
| `CCOS_HUMAN_APPROVAL_REQUIRED` | `true` | Require human approval |
| `CCOS_QUIET_RESOLVER` | `false` | Suppress console output |

---

## 15. File Locations

| Component | Location |
|-----------|----------|
| Resolver | `ccos/src/synthesis/core/missing_capability_resolver.rs` |
| Strategies | `ccos/src/synthesis/core/missing_capability_strategies.rs` |
| Feature Flags | `ccos/src/synthesis/core/feature_flags.rs` |
| Dependency Extractor | `ccos/src/synthesis/core/dependency_extractor.rs` |
| OpenAPI Importer | `ccos/src/synthesis/importers/openapi_importer.rs` |
| GraphQL Importer | `ccos/src/synthesis/importers/graphql_importer.rs` |
| HTTP Wrapper | `ccos/src/synthesis/importers/http_wrapper.rs` |
| Validation | `ccos/src/synthesis/registration/validation_harness.rs` |
| Governance | `ccos/src/synthesis/registration/governance_policies.rs` |
| Registration | `ccos/src/synthesis/registration/registration_flow.rs` |
| Continuous Loop | `ccos/src/synthesis/runtime/continuous_resolution.rs` |
| CLI | `ccos/src/bin/resolve_deps.rs` |

---

## 16. See Also

- [030-capability-system-architecture.md](./030-capability-system-architecture.md) - Overall capability system
- [031-mcp-discovery-unified-service.md](./031-mcp-discovery-unified-service.md) - MCP discovery details
- [033-capability-importers-and-synthesis.md](./033-capability-importers-and-synthesis.md) - Importers
