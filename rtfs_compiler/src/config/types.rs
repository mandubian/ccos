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
    /// Delegation configuration (optional tuning for agent delegation heuristics)
    pub delegation: DelegationConfig,
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

/// Delegation configuration
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct DelegationConfig {
    /// Whether delegation is enabled (fallback: enabled if agent registry present)
    pub enabled: Option<bool>,
    /// Score threshold to approve delegation (default 0.65)
    pub threshold: Option<f64>,
    /// Minimum number of matched skills required
    pub min_skill_hits: Option<u32>,
    /// Maximum number of candidate agents to shortlist
    pub max_candidates: Option<u32>,
    /// Weight applied to recent successful executions when updating scores
    pub feedback_success_weight: Option<f64>,
    /// Decay factor applied to historical success metrics
    pub feedback_decay: Option<f64>,
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
            delegation: DelegationConfig::default(),
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

impl Default for DelegationConfig {
    fn default() -> Self {
        Self {
            enabled: None,
            threshold: None,
            min_skill_hits: None,
            max_candidates: None,
            feedback_success_weight: None,
            feedback_decay: None,
        }
    }
}