# WS9: Polyglot Sandboxed Capabilities - Implementation Plan

**Status**: In Progress  
**Priority**: High  
**Related**: [Spec](./spec-polyglot-sandboxed-capabilities.md), [Roadmap](./ccos-secure-chat-gateway-roadmap.md)

## Executive Summary

WS9 enables CCOS to execute capabilities written in any programming language (Python, JS, Go, etc.) while maintaining security guarantees. The foundation already exists in the codebase; this plan focuses on completing the integration and governance layers.

---

## Current Implementation Status

### ✅ Already Implemented

| Component | Location | Status |
|-----------|----------|--------|
| `MicroVMFactory` | `rtfs/src/runtime/microvm/mod.rs` | ✅ Complete |
| `ProcessMicroVMProvider` | `rtfs/src/runtime/microvm/providers/process.rs` | ✅ Complete |
| `FirecrackerMicroVMProvider` | `rtfs/src/runtime/microvm/providers/firecracker.rs` | ✅ Skeleton |
| `GvisorMicroVMProvider` | `rtfs/src/runtime/microvm/providers/gvisor.rs` | ✅ Skeleton |
| `WasmMicroVMProvider` | `rtfs/src/runtime/microvm/providers/wasm.rs` | ✅ Skeleton |
| `SandboxedExecutor` | `ccos/src/capability_marketplace/executors.rs` | ✅ Complete |
| `SandboxedCapability` type | `ccos/src/capability_marketplace/types.rs` | ✅ Complete |
| Integration tests | `ccos/tests/test_sandboxed_capability.rs` | ✅ Complete |
| `SandboxRuntime` trait | `ccos/src/sandbox/mod.rs` | ✅ Complete |
| `SandboxManager` | `ccos/src/sandbox/manager.rs` | ✅ Complete |
| `SandboxConfig` | `ccos/src/sandbox/config.rs` | ✅ Complete |
| `NetworkProxy` | `ccos/src/sandbox/network_proxy.rs` | ✅ Complete |
| `SecretInjector` | `ccos/src/sandbox/secret_injection.rs` | ✅ Complete |
| `VirtualFilesystem` | `ccos/src/sandbox/filesystem.rs` | ✅ Complete |
| `ResourceLimits` / `ResourceMetrics` | `ccos/src/sandbox/resources.rs` | ✅ Complete |
| Budget integration | `ccos/src/budget/context.rs` | ✅ `record_sandbox_consumption()` |
| Host metering | `ccos/src/host.rs` | ✅ CPU/memory/wall-clock extraction |

### 🔴 Missing / Incomplete

| Component | Gap |
|-----------|-----|
| Capability Manifest Schema | `:runtime` field not parsed from RTFS |
| Manifest `:filesystem` parsing | Uses temporary metadata keys |
| Manifest `:resources` parsing | Uses temporary metadata keys |
| Skills Layer | Not started |

---

## Phased Implementation

### Phase 0: Foundation Hardening ✅ COMPLETE

**Goal**: Ensure existing sandbox infrastructure is robust and integrated with GK.

**Status**: ✅ Complete — `SandboxRuntime` trait, `SandboxManager`, `SandboxConfig` implemented. GK routing via metadata keys.

#### 0.1 Sandbox Manager Interface

Create a unified interface that abstracts over all runtime types:

```rust
// ccos/src/sandbox/mod.rs

pub trait SandboxRuntime: Send + Sync {
    fn name(&self) -> &str;
    fn isolation_level(&self) -> IsolationLevel;
    
    async fn spawn(
        &self,
        config: &SandboxConfig,
        program: &Program,
    ) -> Result<SandboxHandle, SandboxError>;
    
    async fn execute(
        &self,
        handle: &SandboxHandle,
        input: &Value,
    ) -> Result<Value, SandboxError>;
    
    async fn destroy(&self, handle: &SandboxHandle) -> Result<(), SandboxError>;
}

pub enum IsolationLevel {
    None,       // :native
    MemorySafe, // :wasm
    Namespace,  // :container
    Hardware,   // :microvm
}
```

**Files to create/modify:**
- `ccos/src/sandbox/mod.rs` — New module
- `ccos/src/sandbox/manager.rs` — `SandboxManager` with runtime selection
- `ccos/src/sandbox/config.rs` — `SandboxConfig` with FS/network/resource policies

#### 0.2 GK-Sandbox Integration

Route sandboxed execution through GovernanceKernel:

```rust
// In ccos/src/governance_kernel.rs

impl GovernanceKernel {
    pub async fn execute_sandboxed_capability(
        &self,
        capability_id: &str,
        inputs: &Value,
        context: &RuntimeContext,
    ) -> RuntimeResult<Value> {
        // 1. Pre-flight checks (budget, effects, secrets)
        self.validate_capability_call(capability_id, context)?;
        
        // 2. Resolve sandbox config from manifest
        let manifest = self.resolve_manifest(capability_id)?;
        let sandbox_config = self.build_sandbox_config(&manifest)?;
        
        // 3. Execute in sandbox via SandboxManager
        let result = self.sandbox_manager
            .execute(capability_id, inputs, sandbox_config)
            .await?;
        
        // 4. Post-call metering
        self.record_sandbox_consumption(&result.metadata)?;
        
        Ok(result.value)
    }
}
```

**Files to modify:**
- `ccos/src/governance_kernel.rs` — Add `execute_sandboxed_capability()`
- `ccos/src/orchestrator.rs` — Route sandboxed calls to GK

---

### Phase 1: Network Proxy & Secret Injection ✅ COMPLETE

**Goal**: Implement the security-critical layers for sandboxed network and secrets.

**Status**: ✅ Complete — `NetworkProxy` with host/port allowlists, `SecretInjector` for header injection.

**Current metadata keys** (temporary):
- `sandbox_required_secrets`: comma-separated secret names
- `sandbox_allowed_hosts`: comma-separated host allowlist
- `sandbox_allowed_ports`: comma-separated ports

**Example runtime context** (cross-plan params):

```rust
let mut ctx = RuntimeContext::controlled(vec!["ccos.network.http-fetch".to_string()]);
ctx.cross_plan_params.insert(
    "sandbox_allowed_hosts".to_string(),
    Value::String("api.example.com".to_string()),
);
ctx.cross_plan_params.insert(
    "sandbox_allowed_ports".to_string(),
    Value::String("443".to_string()),
);
ctx.cross_plan_params.insert(
    "sandbox_required_secrets".to_string(),
    Value::String("EXAMPLE_API_KEY".to_string()),
);
```

**Runtime behavior** (current):
- Required secrets are written to a temp directory and exposed via `CCOS_SECRET_DIR`.
- MicroVM FS policy is set to read-only for that secrets directory.
- Host allowlists are enforced by preflight URL parsing in `SandboxManager`.
- Port allowlists are enforced by preflight URL parsing in `SandboxManager`.
- `NetworkProxy` forwards requests with allowlist checks and secret header injection.
- `ccos.network.http-fetch` routes through `NetworkProxy` when `sandbox_*` metadata is present.

#### 1.1 GK Network Proxy

All outbound network requests from sandboxes must route through a GK-controlled proxy:

```rust
// ccos/src/sandbox/network_proxy.rs

pub struct NetworkProxy {
    allowed_hosts: HashSet<String>,
    allowed_ports: HashSet<u16>,
    egress_rate_limiter: RateLimiter,
    secret_injector: SecretInjector,
}

impl NetworkProxy {
    pub async fn forward_request(
        &self,
        request: NetworkRequest,
        capability_id: &str,
    ) -> Result<NetworkResponse, NetworkProxyError> {
        // 1. Check host allowlist
        if !self.allowed_hosts.contains(&request.host) {
            self.log_denial(capability_id, &request);
            return Err(NetworkProxyError::HostNotAllowed(request.host));
        }
        
        // 2. Check rate limit
        self.egress_rate_limiter.check()?;
        
        // 3. Inject auth headers from secrets
        let enriched = self.secret_injector.inject_auth(request, capability_id)?;
        
        // 4. Forward and log
        let response = self.http_client.execute(enriched).await?;
        self.log_egress(capability_id, &request, &response);
        
        Ok(response)
    }
}
```

**Implementation approach:**
- For `:container` / `:microvm`: Run a local proxy server, configure sandbox networking to route through it
- For `:process`: Intercept HTTP via env variables (`HTTP_PROXY`, `HTTPS_PROXY`)
- For `:wasm`: Native WASI socket interception

**Files to create:**
- `ccos/src/sandbox/network_proxy.rs`
- `ccos/src/sandbox/egress_allowlist.rs`

#### 1.2 Secret Injection

Secrets are never exposed as environment variables. Instead:

```rust
// ccos/src/sandbox/secret_injection.rs

pub struct SecretInjector {
    secret_store: Arc<SecretStore>,
}

impl SecretInjector {
    pub fn inject_for_sandbox(
        &self,
        capability_id: &str,
        required_secrets: &[String],
    ) -> Result<SecretMount, SecretError> {
        // 1. Verify capability is allowed these secrets
        for secret_name in required_secrets {
            self.verify_access(capability_id, secret_name)?;
        }
        
        // 2. Create tmpfs mount with secret files
        let mount_point = format!("/run/secrets/{}", capability_id);
        let mut files = HashMap::new();
        for secret_name in required_secrets {
            let value = self.secret_store.get(secret_name)?;
            files.insert(secret_name.clone(), value);
        }
        
        Ok(SecretMount { mount_point, files })
    }
}
```

**Files to create:**
- `ccos/src/sandbox/secret_injection.rs`

**Files to modify:**
- `ccos/src/secrets/mod.rs` — Add capability-scoped access control

---

### Phase 2: Filesystem & Resource Limits ✅ COMPLETE

**Goal**: Implement virtual filesystem mounting and resource budget enforcement.

**Status**: ✅ Complete — `VirtualFilesystem`, `ResourceLimits`, `ResourceMetrics` with MicroVM metadata extraction. Budget integration via `record_sandbox_consumption()`.

#### 2.1 Virtual Filesystem

```rust
// ccos/src/sandbox/filesystem.rs

pub struct VirtualFilesystem {
    mounts: Vec<Mount>,
    quota_mb: u64,
    mode: FilesystemMode,
}

pub struct Mount {
    host_path: PathBuf,
    guest_path: String,
    mode: MountMode, // ReadOnly, ReadWrite
}

pub enum FilesystemMode {
    Ephemeral,  // Destroyed after each call
    Session,    // Persists within session
    Persistent, // Persists across sessions (with quota)
}
```

**Files to create:**
- `ccos/src/sandbox/filesystem.rs`

#### 2.2 Resource Limits & Metering

```rust
// ccos/src/sandbox/resources.rs

pub struct ResourceLimits {
    cpu_shares: u32,
    memory_mb: u64,
    timeout_ms: u64,
    network_egress_bytes: u64,
}

pub struct ResourceMetrics {
    cpu_time_ms: u64,
    memory_peak_mb: u64,
    wall_clock_ms: u64,
    network_egress_bytes: u64,
}

impl SandboxManager {
    pub fn meter_consumption(&self, handle: &SandboxHandle) -> ResourceMetrics {
        // For containers: read cgroups stats
        // For microVMs: read Firecracker metrics API
        // For process: use /proc/{pid}/stat
    }
}
```

**Temporary metadata keys** (until manifest fields land in Phase 3):
- `sandbox_filesystem`: JSON-encoded `VirtualFilesystem`
- `sandbox_resources`: JSON-encoded `ResourceLimits`

**Integration with Budget System:**
Sandboxed capability consumption is reported to `BudgetContext`:

```rust
// In governance_kernel.rs after sandbox execution
self.runtime_host.budget_context.record_sandbox_consumption(
    capability_id,
    metrics.cpu_time_ms,
    metrics.network_egress_bytes,
);
```

**Files to create:**
- `ccos/src/sandbox/resources.rs`

**Files to modify:**
- `ccos/src/budget/context.rs` — Add `record_sandbox_consumption()`

---

### Phase 3: Capability Manifest & RTFS Integration (1-2 weeks)

**Goal**: Parse `:runtime` field from capability manifests and wire to sandbox execution.

#### 3.1 Manifest Schema Extension

```rust
// ccos/src/capability_marketplace/types.rs

pub struct RuntimeSpec {
    pub runtime_type: RuntimeType,
    pub image: Option<String>,
    pub entrypoint: Vec<String>,
    pub port: Option<u16>,
    pub startup_timeout_ms: u64,
    pub health_check: Option<String>,
}

pub enum RuntimeType {
    Rtfs,      // Pure RTFS, no sandbox
    Wasm,      // WASM sandbox
    Container, // nsjail/bubblewrap
    MicroVM,   // Firecracker
    Native,    // Host process (trusted only)
}

pub struct CapabilityManifest {
    // ... existing fields ...
    pub runtime: Option<RuntimeSpec>,
    pub filesystem: Option<FilesystemPolicy>,
    pub network: Option<NetworkPolicy>,
    pub secrets: Vec<String>,
    pub resources: Option<ResourceLimits>,
}
```

#### 3.2 RTFS Parser for Manifest

Parse the `:runtime` block from RTFS capability definitions:

```rust
// In rtfs/src/parser or ccos/src/capability_marketplace/manifest_parser.rs

pub fn parse_runtime_spec(expr: &Expression) -> Result<RuntimeSpec, ParseError> {
    // Parse from:
    // :runtime {
    //   :type :microvm
    //   :image "python:3.12-slim"
    //   :entrypoint ["uvicorn" "app:main" "--port" "8000"]
    //   :port 8000
    // }
}
```

**Files to modify:**
- `ccos/src/capability_marketplace/types.rs` — Extend `CapabilityManifest`
- `ccos/src/capability_marketplace/mcp_discovery.rs` — Parse `:runtime` from RTFS

---

### Phase 4: Skills Layer (Optional, 2-3 weeks)

**Goal**: Implement natural-language skill definitions that map to governed capabilities.

#### 4.1 Skill Schema

```rust
// ccos/src/skills/types.rs

pub struct Skill {
    pub name: String,
    pub description: String,
    pub version: String,
    pub capabilities: Vec<String>,  // Required capability IDs
    pub effects: Vec<String>,
    pub secrets: Vec<String>,
    pub data_classes: DataClassification,
    pub approval: ApprovalConfig,
    pub display: DisplayMetadata,
    pub instructions: String,       // Natural language teaching doc
}
```

#### 4.2 Skill → Capability Mapping

```rust
// ccos/src/skills/mapper.rs

pub struct SkillMapper {
    marketplace: Arc<CapabilityMarketplace>,
}

impl SkillMapper {
    pub async fn resolve_capabilities(
        &self,
        skill: &Skill,
    ) -> Result<Vec<CapabilityManifest>, SkillError> {
        // Verify all required capabilities exist and are approved
    }
    
    pub async fn execute_skill_intent(
        &self,
        skill: &Skill,
        intent: &Intent,
    ) -> Result<Value, SkillError> {
        // LLM interprets skill instructions
        // Selects appropriate capability
        // Routes through GK for execution
    }
}
```

**Files to create:**
- `ccos/src/skills/mod.rs`
- `ccos/src/skills/types.rs`
- `ccos/src/skills/parser.rs` — Parse YAML skill definitions
- `ccos/src/skills/mapper.rs`

---

## File Change Summary

### New Files

| File | Purpose |
|------|---------|
| `ccos/src/sandbox/mod.rs` | Sandbox module root |
| `ccos/src/sandbox/manager.rs` | `SandboxManager` with runtime selection |
| `ccos/src/sandbox/config.rs` | `SandboxConfig` struct |
| `ccos/src/sandbox/network_proxy.rs` | GK network proxy with allowlist |
| `ccos/src/sandbox/secret_injection.rs` | Secret mounting for sandboxes |
| `ccos/src/sandbox/filesystem.rs` | Virtual FS with quotas |
| `ccos/src/sandbox/resources.rs` | Resource limits and metering |
| `ccos/src/skills/mod.rs` | Skills layer root (Phase 4) |
| `ccos/src/skills/types.rs` | Skill schema types |
| `ccos/src/skills/parser.rs` | YAML skill parser |
| `ccos/src/skills/mapper.rs` | Skill → capability mapping |

### Modified Files

| File | Changes |
|------|---------|
| `ccos/src/lib.rs` | Add `pub mod sandbox;` and `pub mod skills;` |
| `ccos/src/governance_kernel.rs` | Add `execute_sandboxed_capability()` |
| `ccos/src/orchestrator.rs` | Route sandboxed calls to GK |
| `ccos/src/capability_marketplace/types.rs` | Add `RuntimeSpec`, extend `CapabilityManifest` |
| `ccos/src/capability_marketplace/mcp_discovery.rs` | Parse `:runtime` from RTFS |
| `ccos/src/budget/context.rs` | Add `record_sandbox_consumption()` |
| `ccos/src/secrets/mod.rs` | Add capability-scoped access control |

---

## Verification Plan

### Unit Tests
- `sandbox/manager.rs` — Runtime selection logic
- `sandbox/network_proxy.rs` — Allowlist enforcement
- `sandbox/filesystem.rs` — Mount configuration

### Integration Tests
- `test_sandboxed_python_with_network.rs` — Python capability with proxied network
- `test_sandboxed_secret_injection.rs` — Secret files visible in sandbox
- `test_sandboxed_resource_limits.rs` — Timeout and memory enforcement
- `test_sandboxed_gk_integration.rs` — Full flow through GovernanceKernel

### Manual Verification
- Run `places.search` capability in Firecracker microVM
- Verify network requests route through proxy
- Verify secrets not visible in environment

---

## Dependencies

- **External**: Firecracker binary + kernel + rootfs for microVM testing
- **Rust crates**: `wasmtime` for WASM, `nix` for namespace isolation
- **CCOS modules**: Budget enforcement (WS11 ✅), Secret store, GovernanceKernel

---

## Open Questions

1. **Warm pool sizing**: How many sandboxes to keep warm per runtime type?
2. **GPU passthrough**: Required for ML capabilities — defer to Phase 5?
3. **Multi-runtime capabilities**: One capability using Python + Rust?
4. **Cost attribution**: How to attribute cloud costs per capability/session?

---

## References

- [Polyglot Sandboxed Capabilities Spec](./spec-polyglot-sandboxed-capabilities.md)
- [Resource Budget Enforcement Spec](./spec-resource-budget-enforcement.md)
- [Secure Chat Gateway Roadmap](./ccos-secure-chat-gateway-roadmap.md)
- [Firecracker Documentation](https://firecracker-microvm.github.io/)
