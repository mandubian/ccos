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
    /// Agent registry configuration
    pub agent_registry: Option<AgentRegistryConfig>,
    /// Adaptive threshold configuration
    pub adaptive_threshold: Option<AdaptiveThresholdConfig>,
}

/// Agent registry configuration
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct AgentRegistryConfig {
    /// Registry type (in_memory, database, etc.)
    pub registry_type: RegistryType,
    /// Database connection string (if applicable)
    pub database_url: Option<String>,
    /// Agent definitions
    pub agents: Vec<AgentDefinition>,
}

/// Registry types
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum RegistryType {
    /// In-memory registry
    InMemory,
    /// Database-backed registry
    Database,
    /// File-based registry
    File,
}

/// Adaptive threshold configuration for dynamic delegation decisions
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct AdaptiveThresholdConfig {
    /// Whether adaptive threshold is enabled (default: true)
    pub enabled: Option<bool>,
    /// Base threshold value (default: 0.65)
    pub base_threshold: Option<f64>,
    /// Minimum threshold value (default: 0.3)
    pub min_threshold: Option<f64>,
    /// Maximum threshold value (default: 0.9)
    pub max_threshold: Option<f64>,
    /// Weight for recent success rate in threshold calculation (default: 0.7)
    pub success_rate_weight: Option<f64>,
    /// Weight for historical performance in threshold calculation (default: 0.3)
    pub historical_weight: Option<f64>,
    /// Decay factor for historical performance (default: 0.8)
    pub decay_factor: Option<f64>,
    /// Minimum samples required before adaptive threshold applies (default: 5)
    pub min_samples: Option<u32>,
    /// Environment variable override prefix (default: "CCOS_DELEGATION_")
    pub env_prefix: Option<String>,
}

/// Agent definition
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct AgentDefinition {
    /// Unique agent identifier
    pub agent_id: String,
    /// Agent name/description
    pub name: String,
    /// Agent capabilities
    pub capabilities: Vec<String>,
    /// Agent cost (per request)
    pub cost: f64,
    /// Agent trust score (0.0-1.0)
    pub trust_score: f64,
    /// Agent metadata
    pub metadata: HashMap<String, String>,
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
        Self { enabled: false }
    }
}

impl Default for LLMCapabilityConfig {
    fn default() -> Self {
        Self { enabled: false }
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
        Self { enabled: false }
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
            agent_registry: None,
            adaptive_threshold: None,
        }
    }
}

impl Default for AdaptiveThresholdConfig {
    fn default() -> Self {
        Self {
            enabled: Some(true),
            base_threshold: Some(0.65),
            min_threshold: Some(0.3),
            max_threshold: Some(0.9),
            success_rate_weight: Some(0.7),
            historical_weight: Some(0.3),
            decay_factor: Some(0.8),
            min_samples: Some(5),
            env_prefix: Some("CCOS_DELEGATION_".to_string()),
        }
    }
}

impl Default for AgentRegistryConfig {
    fn default() -> Self {
        Self {
            registry_type: RegistryType::InMemory,
            database_url: None,
            agents: vec![],
        }
    }
}

impl DelegationConfig {
    /// Convert AgentConfig DelegationConfig to arbiter DelegationConfig
    pub fn to_arbiter_config(&self) -> crate::ccos::arbiter::arbiter_config::DelegationConfig {
        crate::ccos::arbiter::arbiter_config::DelegationConfig {
            enabled: self.enabled.unwrap_or(true),
            threshold: self.threshold.unwrap_or(0.65),
            max_candidates: self.max_candidates.unwrap_or(3) as usize,
            min_skill_hits: self.min_skill_hits.map(|hits| hits as usize),
            agent_registry: self
                .agent_registry
                .as_ref()
                .map(
                    |registry| crate::ccos::arbiter::arbiter_config::AgentRegistryConfig {
                        registry_type: match registry.registry_type {
                            RegistryType::InMemory => {
                                crate::ccos::arbiter::arbiter_config::RegistryType::InMemory
                            }
                            RegistryType::Database => {
                                crate::ccos::arbiter::arbiter_config::RegistryType::Database
                            }
                            RegistryType::File => {
                                crate::ccos::arbiter::arbiter_config::RegistryType::File
                            }
                        },
                        database_url: registry.database_url.clone(),
                        agents: registry
                            .agents
                            .iter()
                            .map(
                                |agent| crate::ccos::arbiter::arbiter_config::AgentDefinition {
                                    agent_id: agent.agent_id.clone(),
                                    name: agent.name.clone(),
                                    capabilities: agent.capabilities.clone(),
                                    cost: agent.cost,
                                    trust_score: agent.trust_score,
                                    metadata: agent.metadata.clone(),
                                },
                            )
                            .collect(),
                    },
                )
                .unwrap_or_default(),
            adaptive_threshold: self.adaptive_threshold.clone(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_delegation_config_default() {
        let config = DelegationConfig::default();
        assert_eq!(config.enabled, None);
        assert_eq!(config.threshold, None);
        assert_eq!(config.min_skill_hits, None);
        assert_eq!(config.max_candidates, None);
        assert_eq!(config.feedback_success_weight, None);
        assert_eq!(config.feedback_decay, None);
        assert_eq!(config.agent_registry, None);
        assert_eq!(config.adaptive_threshold, None);
    }

    #[test]
    fn test_agent_registry_config_default() {
        let config = AgentRegistryConfig::default();
        assert_eq!(config.registry_type, RegistryType::InMemory);
        assert_eq!(config.database_url, None);
        assert_eq!(config.agents, vec![]);
    }

    #[test]
    fn test_adaptive_threshold_config_default() {
        let config = AdaptiveThresholdConfig::default();
        assert_eq!(config.enabled, Some(true));
        assert_eq!(config.base_threshold, Some(0.65));
        assert_eq!(config.min_threshold, Some(0.3));
        assert_eq!(config.max_threshold, Some(0.9));
        assert_eq!(config.success_rate_weight, Some(0.7));
        assert_eq!(config.historical_weight, Some(0.3));
        assert_eq!(config.decay_factor, Some(0.8));
        assert_eq!(config.min_samples, Some(5));
        assert_eq!(config.env_prefix, Some("CCOS_DELEGATION_".to_string()));
    }

    #[test]
    fn test_delegation_config_to_arbiter_config() {
        let mut agent_config = DelegationConfig::default();
        agent_config.enabled = Some(true);
        agent_config.threshold = Some(0.8);
        agent_config.max_candidates = Some(5);
        agent_config.min_skill_hits = Some(2);

        let agent_registry = AgentRegistryConfig {
            registry_type: RegistryType::InMemory,
            database_url: None,
            agents: vec![AgentDefinition {
                agent_id: "test_agent".to_string(),
                name: "Test Agent".to_string(),
                capabilities: vec!["test".to_string()],
                cost: 0.1,
                trust_score: 0.9,
                metadata: HashMap::new(),
            }],
        };
        agent_config.agent_registry = Some(agent_registry);

        // Add adaptive threshold configuration
        let adaptive_config = AdaptiveThresholdConfig {
            enabled: Some(true),
            base_threshold: Some(0.7),
            min_threshold: Some(0.4),
            max_threshold: Some(0.95),
            success_rate_weight: Some(0.6),
            historical_weight: Some(0.4),
            decay_factor: Some(0.85),
            min_samples: Some(3),
            env_prefix: Some("TEST_DELEGATION_".to_string()),
        };
        agent_config.adaptive_threshold = Some(adaptive_config);

        let arbiter_config = agent_config.to_arbiter_config();

        assert_eq!(arbiter_config.enabled, true);
        assert_eq!(arbiter_config.threshold, 0.8);
        assert_eq!(arbiter_config.max_candidates, 5);
        assert_eq!(arbiter_config.min_skill_hits, Some(2));
        assert_eq!(arbiter_config.agent_registry.agents.len(), 1);
        assert_eq!(
            arbiter_config.agent_registry.agents[0].agent_id,
            "test_agent"
        );

        // Verify adaptive threshold configuration is preserved
        let adaptive_threshold = arbiter_config.adaptive_threshold.unwrap();
        assert_eq!(adaptive_threshold.enabled, Some(true));
        assert_eq!(adaptive_threshold.base_threshold, Some(0.7));
        assert_eq!(adaptive_threshold.min_threshold, Some(0.4));
        assert_eq!(adaptive_threshold.max_threshold, Some(0.95));
        assert_eq!(adaptive_threshold.success_rate_weight, Some(0.6));
        assert_eq!(adaptive_threshold.historical_weight, Some(0.4));
        assert_eq!(adaptive_threshold.decay_factor, Some(0.85));
        assert_eq!(adaptive_threshold.min_samples, Some(3));
        assert_eq!(
            adaptive_threshold.env_prefix,
            Some("TEST_DELEGATION_".to_string())
        );
    }

    #[test]
    fn test_delegation_config_to_arbiter_config_defaults() {
        let agent_config = DelegationConfig::default();
        let arbiter_config = agent_config.to_arbiter_config();

        assert_eq!(arbiter_config.enabled, true); // default when None
        assert_eq!(arbiter_config.threshold, 0.65); // default when None
        assert_eq!(arbiter_config.max_candidates, 3); // default when None
        assert_eq!(arbiter_config.min_skill_hits, None);
        assert_eq!(arbiter_config.agent_registry.agents.len(), 0); // empty default
        assert_eq!(arbiter_config.adaptive_threshold, None); // None when not configured
    }

    #[test]
    fn test_agent_config_with_delegation() {
        let mut agent_config = AgentConfig::default();
        agent_config.delegation.enabled = Some(true);
        agent_config.delegation.threshold = Some(0.75);
        agent_config.delegation.max_candidates = Some(10);

        let agent_registry = AgentRegistryConfig {
            registry_type: RegistryType::InMemory,
            database_url: None,
            agents: vec![AgentDefinition {
                agent_id: "sentiment_agent".to_string(),
                name: "Sentiment Analysis Agent".to_string(),
                capabilities: vec!["sentiment_analysis".to_string()],
                cost: 0.1,
                trust_score: 0.9,
                metadata: HashMap::new(),
            }],
        };
        agent_config.delegation.agent_registry = Some(agent_registry);

        // Add adaptive threshold configuration
        let adaptive_config = AdaptiveThresholdConfig {
            enabled: Some(true),
            base_threshold: Some(0.6),
            min_threshold: Some(0.3),
            max_threshold: Some(0.9),
            success_rate_weight: Some(0.8),
            historical_weight: Some(0.2),
            decay_factor: Some(0.9),
            min_samples: Some(4),
            env_prefix: Some("SENTIMENT_DELEGATION_".to_string()),
        };
        agent_config.delegation.adaptive_threshold = Some(adaptive_config);

        let arbiter_config = agent_config.delegation.to_arbiter_config();

        assert_eq!(arbiter_config.enabled, true);
        assert_eq!(arbiter_config.threshold, 0.75);
        assert_eq!(arbiter_config.max_candidates, 10);
        assert_eq!(arbiter_config.agent_registry.agents.len(), 1);
        assert_eq!(
            arbiter_config.agent_registry.agents[0].agent_id,
            "sentiment_agent"
        );

        // Verify adaptive threshold configuration
        let adaptive_threshold = arbiter_config.adaptive_threshold.unwrap();
        assert_eq!(adaptive_threshold.enabled, Some(true));
        assert_eq!(adaptive_threshold.base_threshold, Some(0.6));
        assert_eq!(adaptive_threshold.success_rate_weight, Some(0.8));
        assert_eq!(adaptive_threshold.historical_weight, Some(0.2));
        assert_eq!(adaptive_threshold.decay_factor, Some(0.9));
        assert_eq!(adaptive_threshold.min_samples, Some(4));
        assert_eq!(
            adaptive_threshold.env_prefix,
            Some("SENTIMENT_DELEGATION_".to_string())
        );
    }

    #[test]
    fn test_adaptive_threshold_config_serialization() {
        let config = AdaptiveThresholdConfig {
            enabled: Some(true),
            base_threshold: Some(0.7),
            min_threshold: Some(0.4),
            max_threshold: Some(0.95),
            success_rate_weight: Some(0.6),
            historical_weight: Some(0.4),
            decay_factor: Some(0.85),
            min_samples: Some(3),
            env_prefix: Some("TEST_DELEGATION_".to_string()),
        };

        // Test serialization/deserialization
        let serialized = serde_json::to_string(&config).unwrap();
        let deserialized: AdaptiveThresholdConfig = serde_json::from_str(&serialized).unwrap();

        assert_eq!(config, deserialized);
    }

    #[test]
    fn test_adaptive_threshold_config_partial() {
        let mut config = AdaptiveThresholdConfig::default();

        // Test with only some fields set
        config.enabled = Some(false);
        config.base_threshold = Some(0.8);
        config.min_threshold = None; // Use default
        config.max_threshold = None; // Use default

        assert_eq!(config.enabled, Some(false));
        assert_eq!(config.base_threshold, Some(0.8));
        assert_eq!(config.min_threshold, None);
        assert_eq!(config.max_threshold, None);

        // Test that defaults are applied correctly
        let default_config = AdaptiveThresholdConfig::default();
        assert_eq!(default_config.min_threshold, Some(0.3));
        assert_eq!(default_config.max_threshold, Some(0.9));
    }
}
