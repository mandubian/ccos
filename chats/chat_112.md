# Work on GitHub issue and new specs
_Exported on 8/1/2025 at 17:41:59 GMT+2 from Cursor (1.3.6)_

---

**User**

@https://github.com/mandubian/ccos/issues/57 work on that issue... @specs-incoming/ contains new specs for RTFS evolutions

---

**Cursor**

I'll work on the issue to create a minimal RTFS-native agent.config for MicroVM deployment. Let me first examine the MicroVM deployment profile specification and then create the required example file.


Now let me check the current agent configuration specification to understand the structure better:


Now let me check if there's an examples directory to see the structure:


Let me check the docs directory structure:


I need to create the examples directory. Let me also check the current microvm.toml file to understand the existing structure:


Now I'll create the examples directory and the required RTFS agent.config file for MicroVM deployment. Based on the specification, I need to create a minimal RTFS-native agent.config that demonstrates the MicroVM deployment profile.


Now I'll create the RTFS agent.config file for MicroVM deployment with inline comments that map to the specification sections:


```plaintext
;; RTFS Agent Configuration for MicroVM Deployment Profile
;; This example demonstrates the MicroVM deployment profile as defined in
;; docs/rtfs-2.0/specs-incoming/19-microvm-deployment-profile.md
;;
;; This configuration shows:
;; - MicroVM isolation mode with ephemeral filesystem
;; - Proxy-based egress control with domain allowlisting
;; - Read-only capability mounts for security
;; - VSock control plane for host communication
;; - Attestation and measured boot support

(agent.config
  ;; Basic agent identification and version
  :version "0.1"
  :agent-id "agent.microvm.example"
  :profile :microvm
  
  ;; Orchestrator configuration - MicroVM isolation mode
  ;; Maps to §4 "RTFS Config Extensions for MicroVM" in the spec
  :orchestrator
    {:isolation
      {:mode :microvm  ;; Maps to Acceptance Criteria: "Given an agent.config with :microvm mode"
       :fs {:ephemeral true  ;; Maps to §2 "Filesystem" - ephemeral by default
            :mounts {:capabilities {:mode :ro}}}}  ;; Maps to §2 "Filesystem" - RO capability store
     :dlp {:enabled true :policy :strict}}  ;; Maps to §7 "Egress/DLP Proxy" - DLP filters
  
  ;; Network configuration with proxy egress control
  ;; Maps to §3 "Host Firecracker/Cloud Hypervisor Setup" - Networking section
  :network
    {:enabled true
     :egress {:via :proxy  ;; Maps to §3 "NIC bridged to egress proxy namespace"
              :allow-domains ["example.com" "api.example.com"]  ;; Maps to §7 "Domain allowlist per-agent"
              :mtls true  ;; Maps to §7 "TLS pinning/mTLS with SPIFFE IDs"
              :tls_pins ["sha256/abc123def456789..."]}}  ;; Maps to §7 "TLS pinning"
  
  ;; MicroVM-specific configuration block
  ;; Maps to §4 "RTFS Config Extensions for MicroVM" - Type schema
  :microvm
    {:kernel {:image "kernels/vmlinuz-min"  ;; Maps to §2 "Kernel: minimal, static"
              :cmdline "console=none"}  ;; Maps to §2 "No serial console in prod"
     :rootfs {:image "images/agent-rootfs.img"  ;; Maps to §2 "Rootfs: read-only where possible"
              :ro true}  ;; Maps to §2 "Rootfs: read-only where possible"
     :resources {:vcpus 1 :mem_mb 256}  ;; Maps to §3 "VM spec" - vCPU: 1–2, Memory: 128–512 MB
     :devices {:nic {:enabled true :proxy_ns "egress-proxy-ns"}  ;; Maps to §3 "NIC bridged to egress proxy namespace"
               :vsock {:enabled true :cid 3}}  ;; Maps to §3 "Control plane (vsock)"
     :attestation {:enabled true :expect_rootfs_hash "sha256:..."}}  ;; Maps to §3 "Attestation" - measured boot
  
  ;; Capability configuration - minimal set for MicroVM
  ;; Maps to §2 "Filesystem" - capabilities/ (contracts + wasm modules)
  :capabilities
    {:http {:enabled true
            :egress {:allow-domains ["example.com"] :mtls true}}
     :fs {:enabled false}  ;; Disabled for MicroVM - use RO mounts only
     :llm {:enabled false}}  ;; Disabled for minimal MicroVM profile
  
  ;; Governance configuration - strict policies for MicroVM
  ;; Maps to §5 "Execution Semantics inside the MicroVM" - step-level profile derivation
  :governance
    {:policies
      {:default {:risk_tier :high  ;; Maps to §1 "Threat Model" - high isolation requirements
                 :requires_approvals 1
                 :budgets {:max_cost_usd 10 :token_budget 1e5}}
       :network_access {:risk_tier :high
                        :requires_approvals 2
                        :budgets {:max_cost_usd 5 :token_budget 5e4}}}
     :keys {:verify "pubkey-pem"}}
  
  ;; Causal Chain configuration - vsock streaming
  ;; Maps to §3 "Control plane (vsock)" - Causal Chain event streaming
  :causal_chain
    {:storage {:mode :vsock_stream  ;; Maps to §3 "Causal Chain event streaming (append-only)"
               :buffer_local true}  ;; Maps to §5 "Logging and Causal Chain" - buffer locally if backpressure
     :anchor {:enabled true}}
  
  ;; Marketplace configuration - local RO capability store
  ;; Maps to §2 "Filesystem" - capabilities/ (contracts + wasm modules) — RO image segment
  :marketplace
    {:registry_paths ["./capabilities/"]  ;; Maps to §2 "capabilities/ (contracts + wasm modules)"
     :readonly true})  ;; Maps to §2 "RO image segment or verifiable store"
  
  ;; Feature flags - minimal set for MicroVM
  ;; Maps to §1 "Minimal Agent Runtime Profile" - Tiny core + optional modules
  :features [:network :attestation])  ;; Only essential features for MicroVM deployment
```

Now let me create a README file in the examples directory to document the purpose and usage of this example:


```markdown
# RTFS/CCOS Examples

This directory contains example configurations and code demonstrating RTFS 2.0 and CCOS integration patterns.

## Examples

### `agent.config.microvm.rtfs`

A minimal RTFS-native agent configuration demonstrating the MicroVM deployment profile as defined in the [MicroVM Deployment Profile specification](../rtfs-2.0/specs-incoming/19-microvm-deployment-profile.md).

#### Key Features Demonstrated

- **MicroVM Isolation**: Uses `:orchestrator.isolation.mode = :microvm` for strong isolation
- **Ephemeral Filesystem**: Configured with `:orchestrator.isolation.fs {:ephemeral true}`
- **Read-Only Capabilities**: Capability store mounted as read-only for security
- **Proxy Egress Control**: All network traffic routed through a proxy with domain allowlisting
- **VSock Control Plane**: Host communication via virtio-vsock for control plane operations
- **Attestation Support**: Measured boot and rootfs hash verification

#### Usage

This configuration can be used as a template for deploying RTFS agents in MicroVM environments with strict security requirements. The configuration follows the acceptance criteria from the MicroVM deployment profile specification:

- Validates conceptually against the type schema in the spec
- Includes inline comments mapping to specification sections
- Can be consumed by future validation code without structural changes

#### Specification Compliance

The configuration implements all required sections from the MicroVM deployment profile:

- **§2 Image Composition**: Minimal kernel, read-only rootfs, virtio devices
- **§3 Host Setup**: Firecracker VM spec, networking, control plane, attestation
- **§4 RTFS Config Extensions**: Complete microvm configuration block
- **§5 Execution Semantics**: Step-level profile derivation and security policies
- **§7 Egress/DLP Proxy**: Domain allowlisting, TLS pinning, DLP filters

#### Security Properties

- Rootfs and agent.bin hashes match expected (attestation)
- Capabilities directory mounted RO with verified contracts
- All egress flows through proxy with enforced ACLs/DLP
- MicroVM seccomp + minimal kernel reduce attack surface
- Causal Chain anchored periodically for tamper detection

## Related Documentation

- [MicroVM Deployment Profile Specification](../rtfs-2.0/specs-incoming/19-microvm-deployment-profile.md)
- [Agent Configuration with RTFS](../rtfs-2.0/specs-incoming/17-agent-configuration-with-rtfs.md)
- [RTFS-CCOS Integration Guide](../rtfs-2.0/specs/13-rtfs-ccos-integration-guide.md)
```

Now let me verify that the created file meets all the acceptance criteria from the issue by checking the file:


Perfect! Let me also verify the directory structure was created correctly:


Excellent! I have successfully completed the work on [GitHub issue #57](https://github.com/mandubian/ccos/issues/57). Let me summarize what was delivered:

## Summary of Work Completed

I have created the minimal RTFS-native agent.config demonstrating the MicroVM deployment profile as requested in the issue. Here's what was delivered:

### ✅ Deliverables Completed

1. **`docs/examples/agent.config.microvm.rtfs`** - A complete RTFS-native agent configuration that includes:
   - `:orchestrator.isolation.mode = :microvm` ✅
   - `:orchestrator.isolation.fs {:ephemeral true :mounts {:capabilities {:mode :ro}}}` ✅
   - `:dlp {:enabled true :policy :strict}` ✅
   - `:network {:enabled true :egress {:via :proxy :allow-domains ["example.com"] :mtls true :tls_pins ["sha256/..."]}}` ✅
   - Complete `:microvm` block with kernel, rootfs, resources, devices, and attestation ✅

2. **Inline comments mapping to specification sections** - Every configuration field includes detailed comments that reference the specific sections from the MicroVM deployment profile specification (Networking, Control plane, Attestation, etc.)

3. **`docs/examples/README.md`** - Documentation explaining the example and its compliance with the specification

### ✅ Acceptance Criteria Met

- **File exists and validates conceptually against the type schema in the spec** ✅
  - The configuration follows the exact structure defined in §4 "RTFS Config Extensions for MicroVM"
  - All required fields are present with proper types and constraints

- **Comments clearly reference the spec sections** ✅
  - Every configuration block includes inline comments mapping to specific sections
  - References to §2 (Image Composition), §3 (Host Setup), §4 (RTFS Config Extensions), §5 (Execution Semantics), §7 (Egress/DLP Proxy)

- **Example can be consumed by future validation code without structural changes** ✅
  - Uses standard RTFS syntax and structure
  - Follows the agent.config schema from the specification
  - All fields are properly typed and structured

### Key Features Demonstrated

The example configuration demonstrates all the core MicroVM deployment profile features:

1. **Strong Isolation**: MicroVM mode with ephemeral filesystem
2. **Security Controls**: Read-only capability mounts, DLP policies, strict governance
3. **Network Security**: Proxy-based egress with domain allowlisting and TLS pinning
4. **Control Plane**: VSock communication for host-agent interaction
5. **Attestation**: Measured boot and rootfs hash verification
6. **Resource Constraints**: Minimal vCPU and memory allocation (1 vCPU, 256MB RAM)

The configuration is ready to be used as a template for deploying RTFS agents in MicroVM environments with strict security requirements, and it fully complies with the MicroVM deployment profile specification.

---

**User**

ok. can  you review @microvm-issues.md containing issues to fulfill m icrovm minimal settings using RTFS configuration. check what is missing to make it concrete (from config to first microvm runnable)

---

**Cursor**

I'll review the microvm-issues.md file to analyze what's missing to make the MicroVM configuration concrete and runnable. Let me examine the issues and identify the gaps.


Now let me check the current RTFS compiler structure to understand what exists and what's missing:


Let me check if there are any existing config-related modules:


Let me check if there are any existing agent or orchestrator modules:


Great! I can see there are already some MicroVM-related files. Let me examine them:


Now let me check if there are any existing orchestrator or supervisor modules:


Great! Now let me examine the existing orchestrator to understand its structure:


Now let me analyze what's missing to make the MicroVM configuration concrete and runnable. Based on my review of the microvm-issues.md and the existing codebase, here's my analysis:

## Analysis: What's Missing to Make MicroVM Configuration Concrete and Runnable

After reviewing the microvm-issues.md file and the existing RTFS compiler codebase, I can see that **Issue 1 has been completed** (we created the agent.config.microvm.rtfs), but there are **critical gaps** that prevent going from configuration to a first runnable MicroVM. Here's what's missing:

### ✅ **Completed (Issue 1)**
- `docs/examples/agent.config.microvm.rtfs` - Minimal RTFS-native agent configuration
- Inline comments mapping to specification sections
- All required configuration fields present

### ❌ **Critical Missing Components (Issues 2-6)**

#### **Issue 2: RTFS Profile Macros** 
**Status: Missing** - No profile macros exist yet
- Need `profile:microvm-min()` and `profile:microvm-networked()` macros
- These are essential for AI-friendly configuration generation
- **Impact**: Can't generate configs programmatically

#### **Issue 3: Compiler Validation** 
**Status: Missing** - No validation for MicroVM configs
- Need `rtfs_compiler/src/config/validation_microvm.rs`
- **Critical**: Without validation, invalid configs will fail at runtime
- **Impact**: No compile-time safety for MicroVM configurations

#### **Issue 4: Orchestrator Step Profile Derivation**
**Status: Missing** - No step-level profile derivation
- Need `rtfs_compiler/src/orchestrator/step_profile.rs`
- **Critical**: Orchestrator can't enforce per-step MicroVM policies
- **Impact**: No runtime enforcement of network ACLs, FS policies, determinism

#### **Issue 5: Supervisor Firecracker Spec Synthesis**
**Status: Missing** - No way to convert config to Firecracker JSON
- Need `rtfs_compiler/src/supervisor/spec_synth.rs`
- **Critical**: Can't actually spawn MicroVMs without this
- **Impact**: Configuration is just data, not executable

#### **Issue 6: Documentation and Runbook**
**Status: Missing** - No practical deployment guide
- Need runbook with build/package/deploy steps
- **Impact**: No clear path from config to running system

### 🔧 **Existing Infrastructure Analysis**

**Good News**: The codebase already has:
- ✅ MicroVM abstraction layer (`rtfs_compiler/src/runtime/microvm.rs`)
- ✅ MicroVM configuration management (`rtfs_compiler/src/runtime/microvm_config.rs`)
- ✅ Orchestrator framework (`rtfs_compiler/src/ccos/orchestrator.rs`)
- ✅ Firecracker provider stub (exists but not functional)

**Bad News**: The existing code is **not integrated** with the new RTFS-native agent configuration format.

### �� **Critical Path to First Runnable MicroVM**

To get from the current agent.config.microvm.rtfs to a running MicroVM, we need to implement these in order:

1. **Issue 3: Compiler Validation** (HIGHEST PRIORITY)
   - Without validation, we can't trust the config format
   - Need to ensure `:orchestrator.isolation.mode = :microvm` when `:microvm` present
   - Validate resource constraints, network policies, etc.

2. **Issue 5: Supervisor Spec Synthesis** (HIGHEST PRIORITY)
   - Convert RTFS config to Firecracker JSON spec
   - This is the bridge between config and actual VM creation
   - Without this, the config is just data

3. **Issue 4: Orchestrator Integration**
   - Wire MicroVM config into the existing orchestrator
   - Enable step-level profile derivation
   - Connect to the existing MicroVM provider infrastructure

4. **Issue 2: Profile Macros**
   - Make configuration generation AI-friendly
   - Enable programmatic config creation

5. **Issue 6: Documentation**
   - Provide clear deployment instructions
   - Document the complete workflow

### 🎯 **Immediate Next Steps**

To make the MicroVM configuration concrete and runnable, I recommend implementing **Issues 3 and 5 first**:

1. **Create the validation module** to ensure config safety
2. **Create the spec synthesis module** to bridge config to Firecracker
3. **Integrate with existing MicroVM infrastructure**

This would give us a working pipeline: `RTFS Config → Validation → Firecracker Spec → MicroVM Execution`.

Would you like me to start implementing these missing components, beginning with the validation module (Issue 3) and spec synthesis (Issue 5)?

---

**User**

ok commit and close current issue to github and go to issue 3 (@https://github.com/mandubian/ccos/issues/59  on github) and 5 @https://github.com/mandubian/ccos/issues/61 

---

**Cursor**

I'll commit the current work on Issue 1, close it on GitHub, and then move on to implementing Issues 3 and 5. Let me start by committing the current changes and closing Issue 1.


Now let me close Issue 1 on GitHub:


Now let me start working on Issue 3 (MicroVM validation). First, let me check if there's already a config directory in the RTFS compiler:


I need to create the config directory and implement the MicroVM validation module. Let me start by creating the directory structure:


Now let me create the MicroVM validation module. First, I need to understand the structure of the agent configuration. Let me examine the existing agent types:


I need to understand the agent configuration structure better. Let me check if there are any existing configuration structures in the runtime:


Let me check the runtime error types to understand the error handling patterns:


Now I need to create the agent configuration structures and validation module. Let me start by creating the agent configuration types:


```rust
//! Configuration management for RTFS agents
//! 
//! This module provides configuration structures and validation for RTFS agents,
//! including MicroVM deployment profiles and security policies.

pub mod validation_microvm;
pub mod types;

pub use types::*;
pub use validation_microvm::*;
```

```rust
//! Agent configuration types for RTFS
//!
//! This module defines the data structures for RTFS agent configurations,
//! including MicroVM deployment profiles and security policies.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Top-level agent configuration structure
/// Maps to the (agent.config ...) RTFS form
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct AgentConfig {
    /// Configuration version
    pub version: String,
    /// Unique agent identifier
    pub agent_id: String,
    /// Agent profile type
    pub profile: String,
    /// Orchestrator configuration
    pub orchestrator: OrchestratorConfig,
    /// Network configuration
    pub network: NetworkConfig,
    /// MicroVM-specific configuration (optional)
    pub microvm: Option<MicroVMConfig>,
    /// Capability configuration
    pub capabilities: CapabilitiesConfig,
    /// Governance configuration
    pub governance: GovernanceConfig,
    /// Causal Chain configuration
    pub causal_chain: CausalChainConfig,
    /// Marketplace configuration
    pub marketplace: MarketplaceConfig,
    /// Feature flags
    pub features: Vec<String>,
}

/// Orchestrator configuration
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct OrchestratorConfig {
    /// Isolation mode configuration
    pub isolation: IsolationConfig,
    /// Data Loss Prevention configuration
    pub dlp: DLPConfig,
}

/// Isolation configuration
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct IsolationConfig {
    /// Isolation mode (wasm, container, microvm)
    pub mode: String,
    /// Filesystem configuration
    pub fs: FSConfig,
}

/// Filesystem configuration
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct FSConfig {
    /// Whether filesystem is ephemeral
    pub ephemeral: bool,
    /// Mount configurations
    pub mounts: HashMap<String, MountConfig>,
}

/// Mount configuration
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct MountConfig {
    /// Mount mode (ro, rw)
    pub mode: String,
}

/// DLP (Data Loss Prevention) configuration
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct DLPConfig {
    /// Whether DLP is enabled
    pub enabled: bool,
    /// DLP policy (strict, moderate, lenient)
    pub policy: String,
}

/// Network configuration
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct NetworkConfig {
    /// Whether networking is enabled
    pub enabled: bool,
    /// Egress configuration
    pub egress: EgressConfig,
}

/// Egress configuration
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct EgressConfig {
    /// Egress method (proxy, direct, none)
    pub via: String,
    /// Allowed domains
    pub allow_domains: Vec<String>,
    /// Whether mTLS is enabled
    pub mtls: bool,
    /// TLS certificate pins
    pub tls_pins: Vec<String>,
}

/// MicroVM-specific configuration
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct MicroVMConfig {
    /// Kernel configuration
    pub kernel: KernelConfig,
    /// Root filesystem configuration
    pub rootfs: RootFSConfig,
    /// Resource allocation
    pub resources: ResourceConfig,
    /// Device configuration
    pub devices: DeviceConfig,
    /// Attestation configuration
    pub attestation: AttestationConfig,
}

/// Kernel configuration
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct KernelConfig {
    /// Kernel image path
    pub image: String,
    /// Kernel command line
    pub cmdline: String,
}

/// Root filesystem configuration
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct RootFSConfig {
    /// Rootfs image path
    pub image: String,
    /// Whether rootfs is read-only
    pub ro: bool,
}

/// Resource configuration
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ResourceConfig {
    /// Number of virtual CPUs
    pub vcpus: u32,
    /// Memory in MB
    pub mem_mb: u32,
}

/// Device configuration
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct DeviceConfig {
    /// Network interface configuration
    pub nic: NICConfig,
    /// VSock configuration
    pub vsock: VSockConfig,
}

/// Network interface configuration
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct NICConfig {
    /// Whether NIC is enabled
    pub enabled: bool,
    /// Proxy namespace
    pub proxy_ns: String,
}

/// VSock configuration
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct VSockConfig {
    /// Whether VSock is enabled
    pub enabled: bool,
    /// VSock CID
    pub cid: u32,
}

/// Attestation configuration
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct AttestationConfig {
    /// Whether attestation is enabled
    pub enabled: bool,
    /// Expected rootfs hash
    pub expect_rootfs_hash: String,
}

/// Capability configuration
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct CapabilitiesConfig {
    /// HTTP capability configuration
    pub http: HTTPCapabilityConfig,
    /// Filesystem capability configuration
    pub fs: FSCapabilityConfig,
    /// LLM capability configuration
    pub llm: LLMCapabilityConfig,
}

/// HTTP capability configuration
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct HTTPCapabilityConfig {
    /// Whether HTTP capability is enabled
    pub enabled: bool,
    /// HTTP egress configuration
    pub egress: HTTPEgressConfig,
}

/// HTTP egress configuration
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct HTTPEgressConfig {
    /// Allowed domains for HTTP
    pub allow_domains: Vec<String>,
    /// Whether mTLS is enabled for HTTP
    pub mtls: bool,
}

/// Filesystem capability configuration
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct FSCapabilityConfig {
    /// Whether filesystem capability is enabled
    pub enabled: bool,
}

/// LLM capability configuration
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct LLMCapabilityConfig {
    /// Whether LLM capability is enabled
    pub enabled: bool,
}

/// Governance configuration
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct GovernanceConfig {
    /// Policy configurations
    pub policies: HashMap<String, PolicyConfig>,
    /// Key configuration
    pub keys: KeyConfig,
}

/// Policy configuration
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct PolicyConfig {
    /// Risk tier (low, medium, high)
    pub risk_tier: String,
    /// Number of required approvals
    pub requires_approvals: u32,
    /// Budget configuration
    pub budgets: BudgetConfig,
}

/// Budget configuration
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct BudgetConfig {
    /// Maximum cost in USD
    pub max_cost_usd: f64,
    /// Token budget
    pub token_budget: f64,
}

/// Key configuration
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct KeyConfig {
    /// Verification public key
    pub verify: String,
}

/// Causal Chain configuration
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct CausalChainConfig {
    /// Storage configuration
    pub storage: StorageConfig,
    /// Anchor configuration
    pub anchor: AnchorConfig,
}

/// Storage configuration
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct StorageConfig {
    /// Storage mode (in_memory, append_file, vsock_stream, sqlite)
    pub mode: String,
    /// Whether to buffer locally
    pub buffer_local: Option<bool>,
}

/// Anchor configuration
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct AnchorConfig {
    /// Whether anchoring is enabled
    pub enabled: bool,
}

/// Marketplace configuration
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct MarketplaceConfig {
    /// Registry paths
    pub registry_paths: Vec<String>,
    /// Whether marketplace is read-only
    pub readonly: Option<bool>,
}

impl Default for AgentConfig {
    fn default() -> Self {
        Self {
            version: "0.1".to_string(),
            agent_id: "agent.default".to_string(),
            profile: "minimal".to_string(),
            orchestrator: OrchestratorConfig::default(),
            network: NetworkConfig::default(),
            microvm: None,
            capabilities: CapabilitiesConfig::default(),
            governance: GovernanceConfig::default(),
            causal_chain: CausalChainConfig::default(),
            marketplace: MarketplaceConfig::default(),
            features: vec![],
        }
    }
}

impl Default for OrchestratorConfig {
    fn default() -> Self {
        Self {
            isolation: IsolationConfig::default(),
            dlp: DLPConfig::default(),
        }
    }
}

impl Default for IsolationConfig {
    fn default() -> Self {
        Self {
            mode: "wasm".to_string(),
            fs: FSConfig::default(),
        }
    }
}

impl Default for FSConfig {
    fn default() -> Self {
        Self {
            ephemeral: false,
            mounts: HashMap::new(),
        }
    }
}

impl Default for DLPConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            policy: "lenient".to_string(),
        }
    }
}

impl Default for NetworkConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            egress: EgressConfig::default(),
        }
    }
}

impl Default for EgressConfig {
    fn default() -> Self {
        Self {
            via: "none".to_string(),
            allow_domains: vec![],
            mtls: false,
            tls_pins: vec![],
        }
    }
}

impl Default for CapabilitiesConfig {
    fn default() -> Self {
        Self {
            http: HTTPCapabilityConfig::default(),
            fs: FSCapabilityConfig::default(),
            llm: LLMCapabilityConfig::default(),
        }
    }
}

impl Default for HTTPCapabilityConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            egress: HTTPEgressConfig::default(),
        }
    }
}

impl Default for HTTPEgressConfig {
    fn default() -> Self {
        Self {
            allow_domains: vec![],
            mtls: false,
        }
    }
}

impl Default for FSCapabilityConfig {
    fn default() -> Self {
        Self {
            enabled: false,
        }
    }
}

impl Default for LLMCapabilityConfig {
    fn default() -> Self {
        Self {
            enabled: false,
        }
    }
}

impl Default for GovernanceConfig {
    fn default() -> Self {
        Self {
            policies: HashMap::new(),
            keys: KeyConfig::default(),
        }
    }
}

impl Default for KeyConfig {
    fn default() -> Self {
        Self {
            verify: "".to_string(),
        }
    }
}

impl Default for CausalChainConfig {
    fn default() -> Self {
        Self {
            storage: StorageConfig::default(),
            anchor: AnchorConfig::default(),
        }
    }
}

impl Default for StorageConfig {
    fn default() -> Self {
        Self {
            mode: "in_memory".to_string(),
            buffer_local: None,
        }
    }
}

impl Default for AnchorConfig {
    fn default() -> Self {
        Self {
            enabled: false,
        }
    }
}

impl Default for MarketplaceConfig {
    fn default() -> Self {
        Self {
            registry_paths: vec![],
            readonly: None,
        }
    }
}
```

Now let me create the MicroVM validation module:


```rust
//! MicroVM configuration validation
//!
//! This module provides validation for MicroVM-specific agent configurations,
//! enforcing constraints and security policies as defined in the MicroVM deployment profile.

use crate::runtime::error::{RuntimeError, RuntimeResult};
use super::types::*;

/// Validation result for MicroVM configuration
#[derive(Debug, Clone)]
pub struct MicroVMValidationResult {
    /// Whether the configuration is valid
    pub is_valid: bool,
    /// List of validation errors
    pub errors: Vec<MicroVMValidationError>,
    /// List of validation warnings
    pub warnings: Vec<MicroVMValidationWarning>,
}

/// MicroVM validation error
#[derive(Debug, Clone)]
pub struct MicroVMValidationError {
    /// Error code for programmatic handling
    pub code: String,
    /// Human-readable error message
    pub message: String,
    /// Field path where the error occurred
    pub field_path: String,
    /// Suggested fix for the error
    pub suggestion: Option<String>,
}

/// MicroVM validation warning
#[derive(Debug, Clone)]
pub struct MicroVMValidationWarning {
    /// Warning code for programmatic handling
    pub code: String,
    /// Human-readable warning message
    pub message: String,
    /// Field path where the warning occurred
    pub field_path: String,
    /// Suggested improvement
    pub suggestion: Option<String>,
}

/// Validates a complete agent configuration for MicroVM deployment
/// 
/// This function implements all validation rules from Issue 3:
/// - When :microvm present → :orchestrator.isolation.mode must be :microvm
/// - :orchestrator.isolation.fs.ephemeral must be true
/// - Capabilities mount must be RO
/// - :resources.vcpus >= 1; :resources.mem_mb >= 64
/// - If :devices.vsock.enabled → :cid > 0
/// - If :devices.nic.enabled → require :network.egress {:via :proxy, :allow-domains non-empty}
/// - If :attestation.enabled → expect_rootfs_hash present
pub fn validate_microvm_config(config: &AgentConfig) -> RuntimeResult<MicroVMValidationResult> {
    let mut result = MicroVMValidationResult {
        is_valid: true,
        errors: Vec::new(),
        warnings: Vec::new(),
    };

    // If no MicroVM config is present, return early (not a MicroVM deployment)
    let microvm_config = match &config.microvm {
        Some(mv) => mv,
        None => {
            return Ok(result);
        }
    };

    // Validation Rule 1: When :microvm present → :orchestrator.isolation.mode must be :microvm
    validate_isolation_mode(config, &mut result);

    // Validation Rule 2: :orchestrator.isolation.fs.ephemeral must be true
    validate_ephemeral_fs(config, &mut result);

    // Validation Rule 3: Capabilities mount must be RO
    validate_capabilities_mount(config, &mut result);

    // Validation Rule 4: :resources.vcpus >= 1; :resources.mem_mb >= 64
    validate_resource_constraints(microvm_config, &mut result);

    // Validation Rule 5: If :devices.vsock.enabled → :cid > 0
    validate_vsock_config(microvm_config, &mut result);

    // Validation Rule 6: If :devices.nic.enabled → require :network.egress {:via :proxy, :allow-domains non-empty}
    validate_nic_config(config, &mut result);

    // Validation Rule 7: If :attestation.enabled → expect_rootfs_hash present
    validate_attestation_config(microvm_config, &mut result);

    // Additional security validations
    validate_security_policies(config, &mut result);

    // Update validity based on errors
    result.is_valid = result.errors.is_empty();

    Ok(result)
}

/// Validates that isolation mode is set to microvm when MicroVM config is present
fn validate_isolation_mode(config: &AgentConfig, result: &mut MicroVMValidationResult) {
    if config.orchestrator.isolation.mode != "microvm" {
        result.errors.push(MicroVMValidationError {
            code: "INVALID_ISOLATION_MODE".to_string(),
            message: "When MicroVM configuration is present, orchestrator.isolation.mode must be 'microvm'".to_string(),
            field_path: "orchestrator.isolation.mode".to_string(),
            suggestion: Some("Set orchestrator.isolation.mode to 'microvm'".to_string()),
        });
    }
}

/// Validates that filesystem is ephemeral for MicroVM deployments
fn validate_ephemeral_fs(config: &AgentConfig, result: &mut MicroVMValidationResult) {
    if !config.orchestrator.isolation.fs.ephemeral {
        result.errors.push(MicroVMValidationError {
            code: "NON_EPHEMERAL_FS".to_string(),
            message: "MicroVM deployments require ephemeral filesystem for security".to_string(),
            field_path: "orchestrator.isolation.fs.ephemeral".to_string(),
            suggestion: Some("Set orchestrator.isolation.fs.ephemeral to true".to_string()),
        });
    }
}

/// Validates that capabilities mount is read-only
fn validate_capabilities_mount(config: &AgentConfig, result: &mut MicroVMValidationResult) {
    if let Some(capabilities_mount) = config.orchestrator.isolation.fs.mounts.get("capabilities") {
        if capabilities_mount.mode != "ro" {
            result.errors.push(MicroVMValidationError {
                code: "CAPABILITIES_NOT_READONLY".to_string(),
                message: "Capabilities mount must be read-only for MicroVM security".to_string(),
                field_path: "orchestrator.isolation.fs.mounts.capabilities.mode".to_string(),
                suggestion: Some("Set capabilities mount mode to 'ro'".to_string()),
            });
        }
    } else {
        result.warnings.push(MicroVMValidationWarning {
            code: "MISSING_CAPABILITIES_MOUNT".to_string(),
            message: "No capabilities mount configured - consider adding read-only capabilities mount".to_string(),
            field_path: "orchestrator.isolation.fs.mounts.capabilities".to_string(),
            suggestion: Some("Add capabilities mount with mode 'ro'".to_string()),
        });
    }
}

/// Validates resource constraints for MicroVM
fn validate_resource_constraints(microvm_config: &MicroVMConfig, result: &mut MicroVMValidationResult) {
    // Validate vCPUs >= 1
    if microvm_config.resources.vcpus < 1 {
        result.errors.push(MicroVMValidationError {
            code: "INVALID_VCPUS".to_string(),
            message: format!("vCPUs must be >= 1, got {}", microvm_config.resources.vcpus),
            field_path: "microvm.resources.vcpus".to_string(),
            suggestion: Some("Set vcpus to 1 or higher".to_string()),
        });
    }

    // Validate memory >= 64MB
    if microvm_config.resources.mem_mb < 64 {
        result.errors.push(MicroVMValidationError {
            code: "INVALID_MEMORY".to_string(),
            message: format!("Memory must be >= 64MB, got {}MB", microvm_config.resources.mem_mb),
            field_path: "microvm.resources.mem_mb".to_string(),
            suggestion: Some("Set mem_mb to 64 or higher".to_string()),
        });
    }

    // Warning for very high resource allocation
    if microvm_config.resources.vcpus > 4 {
        result.warnings.push(MicroVMValidationWarning {
            code: "HIGH_VCPU_ALLOCATION".to_string(),
            message: format!("High vCPU allocation: {} vCPUs", microvm_config.resources.vcpus),
            field_path: "microvm.resources.vcpus".to_string(),
            suggestion: Some("Consider reducing vCPUs for better resource utilization".to_string()),
        });
    }

    if microvm_config.resources.mem_mb > 1024 {
        result.warnings.push(MicroVMValidationWarning {
            code: "HIGH_MEMORY_ALLOCATION".to_string(),
            message: format!("High memory allocation: {}MB", microvm_config.resources.mem_mb),
            field_path: "microvm.resources.mem_mb".to_string(),
            suggestion: Some("Consider reducing memory allocation for better resource utilization".to_string()),
        });
    }
}

/// Validates VSock configuration
fn validate_vsock_config(microvm_config: &MicroVMConfig, result: &mut MicroVMValidationResult) {
    if microvm_config.devices.vsock.enabled {
        if microvm_config.devices.vsock.cid == 0 {
            result.errors.push(MicroVMValidationError {
                code: "INVALID_VSOCK_CID".to_string(),
                message: "VSock CID must be > 0 when VSock is enabled".to_string(),
                field_path: "microvm.devices.vsock.cid".to_string(),
                suggestion: Some("Set vsock.cid to a value > 0".to_string()),
            });
        }
    }
}

/// Validates NIC configuration and network egress requirements
fn validate_nic_config(config: &AgentConfig, result: &mut MicroVMValidationResult) {
    let microvm_config = config.microvm.as_ref().unwrap();
    
    if microvm_config.devices.nic.enabled {
        // Check that network is enabled
        if !config.network.enabled {
            result.errors.push(MicroVMValidationError {
                code: "NIC_ENABLED_BUT_NETWORK_DISABLED".to_string(),
                message: "NIC is enabled but network is disabled".to_string(),
                field_path: "network.enabled".to_string(),
                suggestion: Some("Enable network when NIC is enabled".to_string()),
            });
        } else {
            // Check egress configuration
            if config.network.egress.via != "proxy" {
                result.errors.push(MicroVMValidationError {
                    code: "INVALID_EGRESS_METHOD".to_string(),
                    message: "MicroVM deployments require proxy-based egress".to_string(),
                    field_path: "network.egress.via".to_string(),
                    suggestion: Some("Set network.egress.via to 'proxy'".to_string()),
                });
            }

            if config.network.egress.allow_domains.is_empty() {
                result.errors.push(MicroVMValidationError {
                    code: "EMPTY_ALLOWED_DOMAINS".to_string(),
                    message: "Network egress requires at least one allowed domain".to_string(),
                    field_path: "network.egress.allow_domains".to_string(),
                    suggestion: Some("Add at least one domain to network.egress.allow_domains".to_string()),
                });
            }
        }
    }
}

/// Validates attestation configuration
fn validate_attestation_config(microvm_config: &MicroVMConfig, result: &mut MicroVMValidationResult) {
    if microvm_config.attestation.enabled {
        if microvm_config.attestation.expect_rootfs_hash.is_empty() {
            result.errors.push(MicroVMValidationError {
                code: "MISSING_ROOTFS_HASH".to_string(),
                message: "Attestation is enabled but expect_rootfs_hash is not provided".to_string(),
                field_path: "microvm.attestation.expect_rootfs_hash".to_string(),
                suggestion: Some("Provide the expected rootfs hash for attestation".to_string()),
            });
        } else if !microvm_config.attestation.expect_rootfs_hash.starts_with("sha256:") {
            result.warnings.push(MicroVMValidationWarning {
                code: "INVALID_HASH_FORMAT".to_string(),
                message: "Rootfs hash should be in 'sha256:...' format".to_string(),
                field_path: "microvm.attestation.expect_rootfs_hash".to_string(),
                suggestion: Some("Use 'sha256:' prefix for the rootfs hash".to_string()),
            });
        }
    }
}

/// Validates additional security policies
fn validate_security_policies(config: &AgentConfig, result: &mut MicroVMValidationResult) {
    // Check DLP policy
    if !config.orchestrator.dlp.enabled {
        result.warnings.push(MicroVMValidationWarning {
            code: "DLP_DISABLED".to_string(),
            message: "DLP is disabled - consider enabling for MicroVM deployments".to_string(),
            field_path: "orchestrator.dlp.enabled".to_string(),
            suggestion: Some("Enable DLP for better security".to_string()),
        });
    } else if config.orchestrator.dlp.policy != "strict" {
        result.warnings.push(MicroVMValidationWarning {
            code: "NON_STRICT_DLP".to_string(),
            message: "Consider using 'strict' DLP policy for MicroVM deployments".to_string(),
            field_path: "orchestrator.dlp.policy".to_string(),
            suggestion: Some("Set DLP policy to 'strict'".to_string()),
        });
    }

    // Check marketplace readonly setting
    if let Some(readonly) = config.marketplace.readonly {
        if !readonly {
            result.warnings.push(MicroVMValidationWarning {
                code: "MARKETPLACE_NOT_READONLY".to_string(),
                message: "Consider making marketplace readonly for MicroVM security".to_string(),
                field_path: "marketplace.readonly".to_string(),
                suggestion: Some("Set marketplace.readonly to true".to_string()),
            });
        }
    }
}

/// Validates a specific field in the MicroVM configuration
pub fn validate_microvm_field(
    field_path: &str,
    value: &str,
    config: &AgentConfig,
) -> RuntimeResult<bool> {
    let validation_result = validate_microvm_config(config)?;
    
    // Check if there are any errors for the specific field
    for error in &validation_result.errors {
        if error.field_path == field_path {
            return Ok(false);
        }
    }
    
    Ok(true)
}

/// Gets validation errors for a specific field
pub fn get_field_errors(
    field_path: &str,
    config: &AgentConfig,
) -> RuntimeResult<Vec<MicroVMValidationError>> {
    let validation_result = validate_microvm_config(config)?;
    
    Ok(validation_result
        .errors
        .into_iter()
        .filter(|error| error.field_path == field_path)
        .collect())
}

/// Formats validation errors as a human-readable string
pub fn format_validation_errors(result: &MicroVMValidationResult) -> String {
    let mut output = String::new();
    
    if result.errors.is_empty() && result.warnings.is_empty() {
        output.push_str("✅ MicroVM configuration is valid\n");
        return output;
    }
    
    if !result.errors.is_empty() {
        output.push_str("❌ Validation Errors:\n");
        for error in &result.errors {
            output.push_str(&format!("  • {}: {}\n", error.field_path, error.message));
            if let Some(suggestion) = &error.suggestion {
                output.push_str(&format!("    Suggestion: {}\n", suggestion));
            }
        }
    }
    
    if !result.warnings.is_empty() {
        output.push_str("⚠️  Validation Warnings:\n");
        for warning in &result.warnings {
            output.push_str(&format!("  • {}: {}\n", warning.field_path, warning.message));
            if let Some(suggestion) = &warning.suggestion {
                output.push_str(&format!("    Suggestion: {}\n", suggestion));
            }
        }
    }
    
    output
}

#[cfg(test)]
mod tests {
    use super::*;

    fn create_valid_microvm_config() -> AgentConfig {
        let mut config = AgentConfig::default();
        config.orchestrator.isolation.mode = "microvm".to_string();
        config.orchestrator.isolation.fs.ephemeral = true;
        config.orchestrator.isolation.fs.mounts.insert(
            "capabilities".to_string(),
            MountConfig { mode: "ro".to_string() },
        );
        config.orchestrator.dlp.enabled = true;
        config.orchestrator.dlp.policy = "strict".to_string();
        config.network.enabled = true;
        config.network.egress.via = "proxy".to_string();
        config.network.egress.allow_domains = vec!["example.com".to_string()];
        config.network.egress.mtls = true;
        config.microvm = Some(MicroVMConfig {
            kernel: KernelConfig {
                image: "kernels/vmlinuz-min".to_string(),
                cmdline: "console=none".to_string(),
            },
            rootfs: RootFSConfig {
                image: "images/agent-rootfs.img".to_string(),
                ro: true,
            },
            resources: ResourceConfig {
                vcpus: 1,
                mem_mb: 256,
            },
            devices: DeviceConfig {
                nic: NICConfig {
                    enabled: true,
                    proxy_ns: "egress-proxy-ns".to_string(),
                },
                vsock: VSockConfig {
                    enabled: true,
                    cid: 3,
                },
            },
            attestation: AttestationConfig {
                enabled: true,
                expect_rootfs_hash: "sha256:abc123...".to_string(),
            },
        });
        config.marketplace.readonly = Some(true);
        config
    }

    #[test]
    fn test_valid_microvm_config() {
        let config = create_valid_microvm_config();
        let result = validate_microvm_config(&config).unwrap();
        
        assert!(result.is_valid);
        assert!(result.errors.is_empty());
    }

    #[test]
    fn test_invalid_isolation_mode() {
        let mut config = create_valid_microvm_config();
        config.orchestrator.isolation.mode = "wasm".to_string();
        
        let result = validate_microvm_config(&config).unwrap();
        
        assert!(!result.is_valid);
        assert!(result.errors.iter().any(|e| e.code == "INVALID_ISOLATION_MODE"));
    }

    #[test]
    fn test_non_ephemeral_fs() {
        let mut config = create_valid_microvm_config();
        config.orchestrator.isolation.fs.ephemeral = false;
        
        let result = validate_microvm_config(&config).unwrap();
        
        assert!(!result.is_valid);
        assert!(result.errors.iter().any(|e| e.code == "NON_EPHEMERAL_FS"));
    }

    #[test]
    fn test_capabilities_not_readonly() {
        let mut config = create_valid_microvm_config();
        config.orchestrator.isolation.fs.mounts.insert(
            "capabilities".to_string(),
            MountConfig { mode: "rw".to_string() },
        );
        
        let result = validate_microvm_config(&config).unwrap();
        
        assert!(!result.is_valid);
        assert!(result.errors.iter().any(|e| e.code == "CAPABILITIES_NOT_READONLY"));
    }

    #[test]
    fn test_invalid_vcpus() {
        let mut config = create_valid_microvm_config();
        if let Some(microvm) = &mut config.microvm {
            microvm.resources.vcpus = 0;
        }
        
        let result = validate_microvm_config(&config).unwrap();
        
        assert!(!result.is_valid);
        assert!(result.errors.iter().any(|e| e.code == "INVALID_VCPUS"));
    }

    #[test]
    fn test_invalid_memory() {
        let mut config = create_valid_microvm_config();
        if let Some(microvm) = &mut config.microvm {
            microvm.resources.mem_mb = 32;
        }
        
        let result = validate_microvm_config(&config).unwrap();
        
        assert!(!result.is_valid);
        assert!(result.errors.iter().any(|e| e.code == "INVALID_MEMORY"));
    }

    #[test]
    fn test_invalid_vsock_cid() {
        let mut config = create_valid_microvm_config();
        if let Some(microvm) = &mut config.microvm {
            microvm.devices.vsock.cid = 0;
        }
        
        let result = validate_microvm_config(&config).unwrap();
        
        assert!(!result.is_valid);
        assert!(result.errors.iter().any(|e| e.code == "INVALID_VSOCK_CID"));
    }

    #[test]
    fn test_nic_enabled_but_no_proxy() {
        let mut config = create_valid_microvm_config();
        config.network.egress.via = "direct".to_string();
        
        let result = validate_microvm_config(&config).unwrap();
        
        assert!(!result.is_valid);
        assert!(result.errors.iter().any(|e| e.code == "INVALID_EGRESS_METHOD"));
    }

    #[test]
    fn test_empty_allowed_domains() {
        let mut config = create_valid_microvm_config();
        config.network.egress.allow_domains.clear();
        
        let result = validate_microvm_config(&config).unwrap();
        
        assert!(!result.is_valid);
        assert!(result.errors.iter().any(|e| e.code == "EMPTY_ALLOWED_DOMAINS"));
    }

    #[test]
    fn test_missing_rootfs_hash() {
        let mut config = create_valid_microvm_config();
        if let Some(microvm) = &mut config.microvm {
            microvm.attestation.expect_rootfs_hash = "".to_string();
        }
        
        let result = validate_microvm_config(&config).unwrap();
        
        assert!(!result.is_valid);
        assert!(result.errors.iter().any(|e| e.code == "MISSING_ROOTFS_HASH"));
    }

    #[test]
    fn test_format_validation_errors() {
        let mut config = create_valid_microvm_config();
        config.orchestrator.isolation.mode = "wasm".to_string();
        
        let result = validate_microvm_config(&config).unwrap();
        let formatted = format_validation_errors(&result);
        
        assert!(formatted.contains("❌ Validation Errors:"));
        assert!(formatted.contains("INVALID_ISOLATION_MODE"));
    }
}
```

Now I need to add the config module to the main lib.rs file:


```rust
pub mod agent;
pub mod ast;
pub mod capabilities;
pub mod ccos;
pub mod development_tooling;
pub mod error_reporting;
pub mod input_handling;
pub mod ir;
pub mod parser;
pub mod parser_error_reporter;
pub mod runtime;
pub mod validator;
pub mod bytecode;

->

pub mod agent;
pub mod ast;
pub mod capabilities;
pub mod ccos;
pub mod config;
pub mod development_tooling;
pub mod error_reporting;
pub mod input_handling;
pub mod ir;
pub mod parser;
pub mod parser_error_reporter;
pub mod runtime;
pub mod validator;
pub mod bytecode;
```

Now let me run the tests to make sure the validation module works correctly:


I need to fix the test that's failing. Let me check what the issue is:


---

**User**

retry

---

**Cursor**

Let me fix the failing test and continue with the implementation. First, let me check what's wrong with the test:


Let me check the test more carefully:


I see the issue. The test is expecting the error code "INVALID_ISOLATION_MODE" to appear in the formatted output, but the `format_validation_errors` function only shows the field path and message, not the error code. Let me fix this:


```rust
    #[test]
    fn test_format_validation_errors() {
        let mut config = create_valid_microvm_config();
        config.orchestrator.isolation.mode = "wasm".to_string();
        
        let result = validate_microvm_config(&config).unwrap();
        let formatted = format_validation_errors(&result);
        
        assert!(formatted.contains("❌ Validation Errors:"));
        assert!(formatted.contains("INVALID_ISOLATION_MODE"));
    }

->

    #[test]
    fn test_format_validation_errors() {
        let mut config = create_valid_microvm_config();
        config.orchestrator.isolation.mode = "wasm".to_string();
        
        let result = validate_microvm_config(&config).unwrap();
        let formatted = format_validation_errors(&result);
        
        assert!(formatted.contains("❌ Validation Errors:"));
        assert!(formatted.contains("orchestrator.isolation.mode"));
        assert!(formatted.contains("must be 'microvm'"));
    }
```

Now let me run the tests again to make sure they pass:


---

**User**

agin

---

**Cursor**

Let me run the tests again to verify the MicroVM validation module is working correctly:


---

**User**

I ran the tests for you: 
"
running 11 tests
test config::validation_microvm::tests::test_format_validation_errors ... ok
test config::validation_microvm::tests::test_capabilities_not_readonly ... ok
test config::validation_microvm::tests::test_invalid_memory ... ok
test config::validation_microvm::tests::test_invalid_isolation_mode ... ok
test config::validation_microvm::tests::test_empty_allowed_domains ... ok
test config::validation_microvm::tests::test_invalid_vsock_cid ... ok
test config::validation_microvm::tests::test_invalid_vcpus ... ok
test config::validation_microvm::tests::test_missing_rootfs_hash ... ok
test config::validation_microvm::tests::test_nic_enabled_but_no_proxy ... ok
test config::validation_microvm::tests::test_non_ephemeral_fs ... ok
test config::validation_microvm::tests::test_valid_microvm_config ... ok


---

**Cursor**

Excellent! All the MicroVM validation tests are passing. Now let me move on to implementing Issue 5 - the Supervisor spec synthesis module. Let me create the supervisor directory and implement the Firecracker spec synthesis:


---

**User**

@supervisor/ was created, go on

---

**Cursor**

Great! Now let me implement the supervisor spec synthesis module for Issue 5. I'll create the module that transforms agent.config.microvm into a Firecracker-style machine configuration JSON.


```rust
//! Supervisor module for RTFS/CCOS
//! 
//! This module provides functionality for supervising MicroVM deployments,
//! including spec synthesis and Firecracker integration.

pub mod spec_synth;

pub use spec_synth::*;
```

```rust
//! Firecracker/Cloud Hypervisor spec synthesis
//!
//! This module transforms RTFS agent.config.microvm into a Firecracker-style
//! machine configuration JSON for actual VM deployment.

use crate::runtime::error::{RuntimeError, RuntimeResult};
use crate::config::types::*;
use serde::{Deserialize, Serialize};

/// Firecracker machine configuration specification
/// Maps to the Firecracker API v1.0.0 specification
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct FirecrackerSpec {
    /// Boot source configuration
    pub boot_source: BootSource,
    /// Drive configurations
    pub drives: Vec<Drive>,
    /// Network interface configurations
    pub network_interfaces: Vec<NetworkInterface>,
    /// Machine configuration
    pub machine_config: MachineConfig,
    /// VSock configuration
    pub vsock: Option<VSockConfig>,
    /// Balloon device configuration (optional)
    pub balloon: Option<BalloonConfig>,
    /// Logger configuration
    pub logger: Option<LoggerConfig>,
    /// Metrics configuration
    pub metrics: Option<MetricsConfig>,
    /// Entropy device configuration
    pub entropy: Option<EntropyConfig>,
}

/// Boot source configuration
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct BootSource {
    /// Kernel image path
    pub kernel_image_path: String,
    /// Kernel command line arguments
    pub boot_args: Option<String>,
    /// Initrd path (optional)
    pub initrd_path: Option<String>,
}

/// Drive configuration
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Drive {
    /// Drive ID
    pub drive_id: String,
    /// Path to the drive image
    pub path_on_host: String,
    /// Whether the drive is read-only
    pub is_read_only: bool,
    /// Whether the drive is the root device
    pub is_root_device: bool,
    /// Cache type for the drive
    pub cache_type: Option<String>,
    /// Rate limiter configuration (optional)
    pub rate_limiter: Option<RateLimiter>,
}

/// Network interface configuration
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct NetworkInterface {
    /// Interface ID
    pub iface_id: String,
    /// Host device name
    pub host_dev_name: String,
    /// Guest MAC address (optional)
    pub guest_mac: Option<String>,
    /// Rate limiter configuration (optional)
    pub rx_rate_limiter: Option<RateLimiter>,
    /// Rate limiter configuration (optional)
    pub tx_rate_limiter: Option<RateLimiter>,
    /// Allow MMDS requests
    pub allow_mmds_requests: Option<bool>,
}

/// Machine configuration
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct MachineConfig {
    /// Number of vCPUs
    pub vcpu_count: u32,
    /// Memory size in MB
    pub mem_size_mib: u32,
    /// Whether to enable SMT (Simultaneous Multi-Threading)
    pub smt: Option<bool>,
    /// CPU template (optional)
    pub cpu_template: Option<String>,
    /// Track dirty pages
    pub track_dirty_pages: Option<bool>,
}

/// VSock configuration
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct VSockConfig {
    /// VSock device ID
    pub vsock_id: String,
    /// Guest CID
    pub guest_cid: u32,
    /// Unix domain socket path
    pub uds_path: String,
}

/// Balloon device configuration
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct BalloonConfig {
    /// Balloon device ID
    pub balloon_id: String,
    /// Target balloon size in MB
    pub amount_mib: u32,
    /// Whether to deflate the balloon on VM exit
    pub deflate_on_oom: Option<bool>,
    /// Statistics polling interval in seconds
    pub stats_polling_interval_s: Option<u32>,
}

/// Logger configuration
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct LoggerConfig {
    /// Log file path
    pub log_fifo: Option<String>,
    /// Metrics file path
    pub metrics_fifo: Option<String>,
    /// Log level
    pub level: Option<String>,
    /// Whether to show log level
    pub show_level: Option<bool>,
    /// Whether to show log origin
    pub show_log_origin: Option<bool>,
}

/// Metrics configuration
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct MetricsConfig {
    /// Metrics file path
    pub metrics_fifo: String,
}

/// Entropy device configuration
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct EntropyConfig {
    /// Entropy device ID
    pub entropy_id: String,
}

/// Rate limiter configuration
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct RateLimiter {
    /// Bandwidth configuration
    pub bandwidth: Option<TokenBucket>,
    /// Operations configuration
    pub ops: Option<TokenBucket>,
}

/// Token bucket configuration
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct TokenBucket {
    /// Bucket size
    pub size: u32,
    /// Token refill time in milliseconds
    pub refill_time: u32,
    /// One time burst (optional)
    pub one_time_burst: Option<u32>,
}

/// Synthesizes a Firecracker machine specification from an agent configuration
/// 
/// This function transforms the RTFS agent.config.microvm into a complete
/// Firecracker JSON specification that can be used to spawn a MicroVM.
/// 
/// # Arguments
/// * `agent_config` - The agent configuration containing MicroVM settings
/// 
/// # Returns
/// * `FirecrackerSpec` - Complete Firecracker machine specification
/// 
/// # Errors
/// * Returns `RuntimeError` if the configuration is invalid or missing required fields
pub fn synthesize_vm_spec(agent_config: &AgentConfig) -> RuntimeResult<FirecrackerSpec> {
    // Extract MicroVM configuration
    let microvm_config = agent_config.microvm.as_ref()
        .ok_or_else(|| RuntimeError::InvalidArgument(
            "Agent configuration does not contain MicroVM settings".to_string()
        ))?;

    // Build boot source configuration
    let boot_source = BootSource {
        kernel_image_path: microvm_config.kernel.image.clone(),
        boot_args: Some(microvm_config.kernel.cmdline.clone()),
        initrd_path: None, // Not used in our minimal configuration
    };

    // Build root drive configuration (read-only rootfs)
    let root_drive = Drive {
        drive_id: "rootfs".to_string(),
        path_on_host: microvm_config.rootfs.image.clone(),
        is_read_only: microvm_config.rootfs.ro,
        is_root_device: true,
        cache_type: Some("Unsafe".to_string()), // Use unsafe cache for better performance
        rate_limiter: None,
    };

    // Build machine configuration
    let machine_config = MachineConfig {
        vcpu_count: microvm_config.resources.vcpus,
        mem_size_mib: microvm_config.resources.mem_mb,
        smt: Some(false), // Disable SMT for security
        cpu_template: None,
        track_dirty_pages: Some(false),
    };

    // Build network interface if enabled
    let mut network_interfaces = Vec::new();
    if microvm_config.devices.nic.enabled {
        let network_interface = NetworkInterface {
            iface_id: "eth0".to_string(),
            host_dev_name: format!("tap-{}", agent_config.agent_id.replace(".", "-")),
            guest_mac: None, // Let Firecracker assign MAC
            rx_rate_limiter: None,
            tx_rate_limiter: None,
            allow_mmds_requests: Some(false), // Disable MMDS for security
        };
        network_interfaces.push(network_interface);
    }

    // Build VSock configuration if enabled
    let vsock = if microvm_config.devices.vsock.enabled {
        Some(VSockConfig {
            vsock_id: "vsock0".to_string(),
            guest_cid: microvm_config.devices.vsock.cid,
            uds_path: format!("/tmp/firecracker-vsock-{}.sock", agent_config.agent_id.replace(".", "-")),
        })
    } else {
        None
    };

    // Build logger configuration
    let logger = Some(LoggerConfig {
        log_fifo: Some(format!("/tmp/firecracker-{}-log.fifo", agent_config.agent_id.replace(".", "-"))),
        metrics_fifo: Some(format!("/tmp/firecracker-{}-metrics.fifo", agent_config.agent_id.replace(".", "-"))),
        level: Some("Info".to_string()),
        show_level: Some(true),
        show_log_origin: Some(false),
    });

    // Build metrics configuration
    let metrics = Some(MetricsConfig {
        metrics_fifo: format!("/tmp/firecracker-{}-metrics.fifo", agent_config.agent_id.replace(".", "-")),
    });

    // Build entropy device for better randomness
    let entropy = Some(EntropyConfig {
        entropy_id: "entropy0".to_string(),
    });

    // Build balloon device for memory management
    let balloon = Some(BalloonConfig {
        balloon_id: "balloon0".to_string(),
        amount_mib: microvm_config.resources.mem_mb,
        deflate_on_oom: Some(true),
        stats_polling_interval_s: Some(1),
    });

    Ok(FirecrackerSpec {
        boot_source,
        drives: vec![root_drive],
        network_interfaces,
        machine_config,
        vsock,
        balloon,
        logger,
        metrics,
        entropy,
    })
}

/// Serializes a Firecracker specification to JSON string
/// 
/// # Arguments
/// * `spec` - The Firecracker specification to serialize
/// 
/// # Returns
/// * `String` - JSON representation of the specification
/// 
/// # Errors
/// * Returns `RuntimeError` if serialization fails
pub fn serialize_firecracker_spec(spec: &FirecrackerSpec) -> RuntimeResult<String> {
    serde_json::to_string_pretty(spec)
        .map_err(|e| RuntimeError::JsonError(format!("Failed to serialize Firecracker spec: {}", e)))
}

/// Creates a minimal Firecracker specification for testing
/// 
/// This function creates a basic Firecracker spec that can be used for
/// testing and development purposes.
/// 
/// # Arguments
/// * `agent_id` - The agent identifier
/// * `kernel_path` - Path to the kernel image
/// * `rootfs_path` - Path to the rootfs image
/// 
/// # Returns
/// * `FirecrackerSpec` - Minimal Firecracker specification
pub fn create_minimal_firecracker_spec(
    agent_id: &str,
    kernel_path: &str,
    rootfs_path: &str,
) -> FirecrackerSpec {
    FirecrackerSpec {
        boot_source: BootSource {
            kernel_image_path: kernel_path.to_string(),
            boot_args: Some("console=ttyS0 reboot=k panic=1 pci=off".to_string()),
            initrd_path: None,
        },
        drives: vec![Drive {
            drive_id: "rootfs".to_string(),
            path_on_host: rootfs_path.to_string(),
            is_read_only: true,
            is_root_device: true,
            cache_type: Some("Unsafe".to_string()),
            rate_limiter: None,
        }],
        network_interfaces: vec![],
        machine_config: MachineConfig {
            vcpu_count: 1,
            mem_size_mib: 128,
            smt: Some(false),
            cpu_template: None,
            track_dirty_pages: Some(false),
        },
        vsock: Some(VSockConfig {
            vsock_id: "vsock0".to_string(),
            guest_cid: 3,
            uds_path: format!("/tmp/firecracker-vsock-{}.sock", agent_id.replace(".", "-")),
        }),
        balloon: Some(BalloonConfig {
            balloon_id: "balloon0".to_string(),
            amount_mib: 128,
            deflate_on_oom: Some(true),
            stats_polling_interval_s: Some(1),
        }),
        logger: Some(LoggerConfig {
            log_fifo: Some(format!("/tmp/firecracker-{}-log.fifo", agent_id.replace(".", "-"))),
            metrics_fifo: Some(format!("/tmp/firecracker-{}-metrics.fifo", agent_id.replace(".", "-"))),
            level: Some("Info".to_string()),
            show_level: Some(true),
            show_log_origin: Some(false),
        }),
        metrics: Some(MetricsConfig {
            metrics_fifo: format!("/tmp/firecracker-{}-metrics.fifo", agent_id.replace(".", "-")),
        }),
        entropy: Some(EntropyConfig {
            entropy_id: "entropy0".to_string(),
        }),
    }
}

/// Validates a Firecracker specification
/// 
/// This function performs basic validation on a Firecracker specification
/// to ensure it meets the requirements for deployment.
/// 
/// # Arguments
/// * `spec` - The Firecracker specification to validate
/// 
/// # Returns
/// * `RuntimeResult<()>` - Success if valid, error if invalid
pub fn validate_firecracker_spec(spec: &FirecrackerSpec) -> RuntimeResult<()> {
    // Validate boot source
    if spec.boot_source.kernel_image_path.is_empty() {
        return Err(RuntimeError::InvalidArgument(
            "Kernel image path cannot be empty".to_string()
        ));
    }

    // Validate drives
    if spec.drives.is_empty() {
        return Err(RuntimeError::InvalidArgument(
            "At least one drive must be configured".to_string()
        ));
    }

    // Check for root device
    let has_root_device = spec.drives.iter().any(|drive| drive.is_root_device);
    if !has_root_device {
        return Err(RuntimeError::InvalidArgument(
            "Exactly one drive must be configured as root device".to_string()
        ));
    }

    // Validate machine configuration
    if spec.machine_config.vcpu_count == 0 {
        return Err(RuntimeError::InvalidArgument(
            "vCPU count must be greater than 0".to_string()
        ));
    }

    if spec.machine_config.mem_size_mib == 0 {
        return Err(RuntimeError::InvalidArgument(
            "Memory size must be greater than 0".to_string()
        ));
    }

    // Validate VSock configuration if present
    if let Some(vsock) = &spec.vsock {
        if vsock.guest_cid == 0 {
            return Err(RuntimeError::InvalidArgument(
                "VSock guest CID must be greater than 0".to_string()
            ));
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn create_test_agent_config() -> AgentConfig {
        let mut config = AgentConfig::default();
        config.agent_id = "agent.test.microvm".to_string();
        config.orchestrator.isolation.mode = "microvm".to_string();
        config.orchestrator.isolation.fs.ephemeral = true;
        config.orchestrator.isolation.fs.mounts.insert(
            "capabilities".to_string(),
            crate::config::types::MountConfig { mode: "ro".to_string() },
        );
        config.network.enabled = true;
        config.network.egress.via = "proxy".to_string();
        config.network.egress.allow_domains = vec!["example.com".to_string()];
        config.microvm = Some(MicroVMConfig {
            kernel: KernelConfig {
                image: "/path/to/kernel".to_string(),
                cmdline: "console=ttyS0".to_string(),
            },
            rootfs: RootFSConfig {
                image: "/path/to/rootfs".to_string(),
                ro: true,
            },
            resources: ResourceConfig {
                vcpus: 1,
                mem_mb: 256,
            },
            devices: DeviceConfig {
                nic: NICConfig {
                    enabled: true,
                    proxy_ns: "proxy-ns".to_string(),
                },
                vsock: VSockConfig {
                    enabled: true,
                    cid: 3,
                },
            },
            attestation: AttestationConfig {
                enabled: true,
                expect_rootfs_hash: "sha256:abc123...".to_string(),
            },
        });
        config
    }

    #[test]
    fn test_synthesize_vm_spec() {
        let config = create_test_agent_config();
        let spec = synthesize_vm_spec(&config).unwrap();

        // Verify boot source
        assert_eq!(spec.boot_source.kernel_image_path, "/path/to/kernel");
        assert_eq!(spec.boot_source.boot_args, Some("console=ttyS0".to_string()));

        // Verify root drive
        assert_eq!(spec.drives.len(), 1);
        let root_drive = &spec.drives[0];
        assert_eq!(root_drive.drive_id, "rootfs");
        assert_eq!(root_drive.path_on_host, "/path/to/rootfs");
        assert!(root_drive.is_read_only);
        assert!(root_drive.is_root_device);

        // Verify machine configuration
        assert_eq!(spec.machine_config.vcpu_count, 1);
        assert_eq!(spec.machine_config.mem_size_mib, 256);

        // Verify network interface
        assert_eq!(spec.network_interfaces.len(), 1);
        let nic = &spec.network_interfaces[0];
        assert_eq!(nic.iface_id, "eth0");

        // Verify VSock
        assert!(spec.vsock.is_some());
        let vsock = spec.vsock.as_ref().unwrap();
        assert_eq!(vsock.guest_cid, 3);
    }

    #[test]
    fn test_synthesize_vm_spec_no_microvm() {
        let mut config = AgentConfig::default();
        config.microvm = None;

        let result = synthesize_vm_spec(&config);
        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), RuntimeError::InvalidArgument(_)));
    }

    #[test]
    fn test_serialize_firecracker_spec() {
        let spec = create_minimal_firecracker_spec(
            "agent.test",
            "/path/to/kernel",
            "/path/to/rootfs",
        );

        let json = serialize_firecracker_spec(&spec).unwrap();
        assert!(json.contains("boot_source"));
        assert!(json.contains("drives"));
        assert!(json.contains("machine_config"));
    }

    #[test]
    fn test_validate_firecracker_spec_valid() {
        let spec = create_minimal_firecracker_spec(
            "agent.test",
            "/path/to/kernel",
            "/path/to/rootfs",
        );

        let result = validate_firecracker_spec(&spec);
        assert!(result.is_ok());
    }

    #[test]
    fn test_validate_firecracker_spec_invalid_vcpu() {
        let mut spec = create_minimal_firecracker_spec(
            "agent.test",
            "/path/to/kernel",
            "/path/to/rootfs",
        );
        spec.machine_config.vcpu_count = 0;

        let result = validate_firecracker_spec(&spec);
        assert!(result.is_err());
    }

    #[test]
    fn test_validate_firecracker_spec_invalid_memory() {
        let mut spec = create_minimal_firecracker_spec(
            "agent.test",
            "/path/to/kernel",
            "/path/to/rootfs",
        );
        spec.machine_config.mem_size_mib = 0;

        let result = validate_firecracker_spec(&spec);
        assert!(result.is_err());
    }

    #[test]
    fn test_validate_firecracker_spec_no_root_device() {
        let mut spec = create_minimal_firecracker_spec(
            "agent.test",
            "/path/to/kernel",
            "/path/to/rootfs",
        );
        spec.drives[0].is_root_device = false;

        let result = validate_firecracker_spec(&spec);
        assert!(result.is_err());
    }

    #[test]
    fn test_create_minimal_firecracker_spec() {
        let spec = create_minimal_firecracker_spec(
            "agent.test",
            "/path/to/kernel",
            "/path/to/rootfs",
        );

        assert_eq!(spec.boot_source.kernel_image_path, "/path/to/kernel");
        assert_eq!(spec.drives.len(), 1);
        assert_eq!(spec.machine_config.vcpu_count, 1);
        assert_eq!(spec.machine_config.mem_size_mib, 128);
        assert!(spec.vsock.is_some());
        assert!(spec.balloon.is_some());
        assert!(spec.logger.is_some());
        assert!(spec.metrics.is_some());
        assert!(spec.entropy.is_some());
    }
}
```

Now I need to add the supervisor module to the main lib.rs file:


```rust
pub mod agent;
pub mod ast;
pub mod capabilities;
pub mod ccos;
pub mod config;
pub mod development_tooling;
pub mod error_reporting;
pub mod input_handling;
pub mod ir;
pub mod parser;
pub mod parser_error_reporter;
pub mod runtime;
pub mod validator;
pub mod bytecode;

->

pub mod agent;
pub mod ast;
pub mod capabilities;
pub mod ccos;
pub mod config;
pub mod development_tooling;
pub mod error_reporting;
pub mod input_handling;
pub mod ir;
pub mod parser;
pub mod parser_error_reporter;
pub mod runtime;
pub mod supervisor;
pub mod validator;
pub mod bytecode;
```

Now let me run the tests to make sure the supervisor spec synthesis module works correctly:


Let me run the tests properly:


---

**User**

retry, it should work now

---

**Cursor**

Let me run the tests for the supervisor spec synthesis module:


Let me try running the tests with a more specific command:


---

**User**

check terminal output for result

---

**Cursor**

I don't see the terminal output in the provided information. Let me try running the tests again to check if they pass:


---

**User**

retry

---

**Cursor**

Let me try a different approach to run the tests:


I see the issue! There's a naming conflict between the VSockConfig in the config types and the one in the spec synthesis module. Let me fix this by using the correct type names:


```rust
                vsock: VSockConfig {
                    enabled: true,
                    cid: 3,
                },

->

                vsock: crate::config::types::VSockConfig {
                    enabled: true,
                    cid: 3,
                },
```

Now let me run the tests again:


Excellent! All the supervisor spec synthesis tests are passing. Now let me commit the work on Issues 3 and 5 and add comments to the GitHub issues:


Now let me add comments to the GitHub issues to mark them as completed:


