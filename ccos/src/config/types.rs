//! Agent configuration types for CCOS
//!
//! This module defines the data structures for CCOS agent runtime configurations,
//! including orchestrator settings, governance policies, delegation settings,
//! marketplace config, causal chain storage, LLM profiles, discovery settings,
//! and missing capability resolution.
//!
//! These types were migrated from `rtfs::config::types` as part of issue #166
//! to properly separate language concerns (RTFS) from runtime concerns (CCOS).

use crate::budget::{BudgetLimits, BudgetPolicies, ExhaustionPolicy};
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
    /// Optional display name for the agent
    #[serde(default)]
    pub agent_name: Option<String>,
    /// Optional allowlist of agent context fields to share with the LLM
    #[serde(default)]
    pub llm_context_allowlist: Option<Vec<String>>,
    /// Agent profile type
    pub profile: String,
    /// Orchestrator configuration
    #[serde(default)]
    pub orchestrator: OrchestratorConfig,
    /// Network configuration
    #[serde(default)]
    pub network: NetworkConfig,
    /// MicroVM-specific configuration (optional)
    #[serde(default)]
    pub microvm: Option<MicroVMConfig>,
    /// Capability configuration
    #[serde(default)]
    pub capabilities: CapabilitiesConfig,
    /// Governance configuration
    #[serde(default)]
    pub governance: GovernanceConfig,
    /// Causal Chain configuration
    #[serde(default)]
    pub causal_chain: CausalChainConfig,
    /// Marketplace configuration
    #[serde(default)]
    pub marketplace: MarketplaceConfig,
    /// Delegation configuration (optional tuning for agent delegation heuristics)
    #[serde(default)]
    pub delegation: DelegationConfig,
    /// Feature flags
    #[serde(default)]
    pub features: Vec<String>,
    /// Optional catalog of LLM model profiles for interactive selection
    #[serde(default)]
    pub llm_profiles: Option<LlmProfilesConfig>,
    /// Discovery engine configuration
    #[serde(default)]
    pub discovery: DiscoveryConfig,
    /// Server discovery pipeline configuration
    #[serde(default)]
    pub server_discovery_pipeline: ServerDiscoveryPipelineConfig,
    /// Catalog configuration (plan/capability reuse thresholds)
    #[serde(default)]
    pub catalog: CatalogConfig,
    /// Missing capability resolution configuration
    #[serde(default)]
    pub missing_capabilities: MissingCapabilityRuntimeConfig,
    /// Storage paths configuration
    #[serde(default)]
    pub storage: StoragePathsConfig,
    /// AI Self-Programming configuration (trust levels, approval gates)
    #[serde(default)]
    pub self_programming: SelfProgrammingConfig,
    /// LLM Validation configuration (schema, plan validation, auto-repair)
    #[serde(default)]
    pub validation: ValidationConfig,
    /// Real-time tracking and observability configuration for gateway/agent monitoring
    #[serde(default)]
    pub realtime_tracking: RealtimeTrackingConfig,
    /// Sandbox configuration for code execution
    #[serde(default)]
    pub sandbox: SandboxConfig,
    /// Coding agents configuration for specialized code generation
    #[serde(default)]
    pub coding_agents: CodingAgentsConfig,
    /// Autonomous agent configuration for iterative LLM consultation
    #[serde(default)]
    pub autonomous_agent: AutonomousAgentConfig,
}

impl Default for AgentConfig {
    fn default() -> Self {
        Self {
            version: "0.1".to_string(),
            agent_id: "agent.default".to_string(),
            agent_name: None,
            llm_context_allowlist: None,
            profile: "local".to_string(),
            orchestrator: OrchestratorConfig::default(),
            network: NetworkConfig::default(),
            microvm: None,
            capabilities: CapabilitiesConfig::default(),
            governance: GovernanceConfig::default(),
            causal_chain: CausalChainConfig::default(),
            marketplace: MarketplaceConfig::default(),
            delegation: DelegationConfig::default(),
            features: vec![],
            llm_profiles: None,
            discovery: DiscoveryConfig::default(),
            server_discovery_pipeline: ServerDiscoveryPipelineConfig::default(),
            catalog: CatalogConfig::default(),
            missing_capabilities: MissingCapabilityRuntimeConfig::default(),
            storage: StoragePathsConfig::default(),
            self_programming: SelfProgrammingConfig::default(),
            validation: ValidationConfig::default(),
            realtime_tracking: RealtimeTrackingConfig::default(),
            sandbox: SandboxConfig::default(),
            coding_agents: CodingAgentsConfig::default(),
            autonomous_agent: AutonomousAgentConfig::default(),
        }
    }
}

/// Autonomous agent configuration for iterative LLM consultation
/// Controls how the agent consults with LLM after each action
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct AutonomousAgentConfig {
    /// Enable iterative mode (agent consults LLM after each action)
    #[serde(default = "default_autonomous_enabled")]
    pub enabled: bool,
    /// Maximum iterations per user request (safety limit)
    #[serde(default = "default_max_iterations")]
    pub max_iterations: u32,
    /// Maximum context entries to keep in conversation history
    #[serde(default = "default_max_context_entries")]
    pub max_context_entries: usize,
    /// Context management strategy: "truncate" or "summarize"
    #[serde(default = "default_context_strategy")]
    pub context_strategy: String,
    /// Enable intermediate progress responses to user
    #[serde(default = "default_send_intermediate")]
    pub send_intermediate_responses: bool,
    /// On action failure: "ask_user" or "abort"
    #[serde(default = "default_failure_handling")]
    pub failure_handling: String,
    /// Max consecutive failures before asking user
    #[serde(default = "default_max_consecutive_failures")]
    pub max_consecutive_failures: u32,
}

impl Default for AutonomousAgentConfig {
    fn default() -> Self {
        Self {
            enabled: default_autonomous_enabled(),
            max_iterations: default_max_iterations(),
            max_context_entries: default_max_context_entries(),
            context_strategy: default_context_strategy(),
            send_intermediate_responses: default_send_intermediate(),
            failure_handling: default_failure_handling(),
            max_consecutive_failures: default_max_consecutive_failures(),
        }
    }
}

fn default_autonomous_enabled() -> bool {
    true
}

fn default_max_iterations() -> u32 {
    10
}

fn default_max_context_entries() -> usize {
    20
}

fn default_context_strategy() -> String {
    "truncate".to_string()
}

fn default_send_intermediate() -> bool {
    true
}

fn default_failure_handling() -> String {
    "ask_user".to_string()
}

fn default_max_consecutive_failures() -> u32 {
    2
}

/// Sandbox configuration for code execution
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct SandboxConfig {
    /// Whether sandbox execution is enabled
    #[serde(default = "default_sandbox_enabled")]
    pub enabled: bool,
    /// Runtime type for sandbox (bubblewrap, docker, etc.)
    #[serde(default = "default_sandbox_runtime")]
    pub runtime: String,
    /// Package allowlist configuration
    #[serde(default)]
    pub package_allowlist: PackageAllowlistConfig,
    /// Pre-baked image configurations
    #[serde(default)]
    pub images: Vec<SandboxImageConfig>,
    /// Whether network access is enabled in the sandbox
    #[serde(default = "default_true")]
    pub network_enabled: bool,
    /// Package cache directory
    #[serde(default = "default_package_cache_dir")]
    pub package_cache_dir: String,
    /// Enable package caching
    #[serde(default = "default_true")]
    pub enable_package_cache: bool,
}

impl Default for SandboxConfig {
    fn default() -> Self {
        Self {
            enabled: default_sandbox_enabled(),
            runtime: default_sandbox_runtime(),
            package_allowlist: PackageAllowlistConfig::default(),
            images: vec![SandboxImageConfig::default()],
            network_enabled: true,
            package_cache_dir: default_package_cache_dir(),
            enable_package_cache: default_true(),
        }
    }
}

fn default_sandbox_enabled() -> bool {
    true
}

fn default_sandbox_runtime() -> String {
    "bubblewrap".to_string()
}

fn default_package_cache_dir() -> String {
    "/tmp/ccos-sandbox-cache".to_string()
}

fn default_true() -> bool {
    true
}

/// Package allowlist configuration
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct PackageAllowlistConfig {
    /// Auto-approved packages (no approval needed)
    #[serde(default = "default_auto_approved_packages")]
    pub auto_approved: Vec<String>,
    /// Packages requiring approval
    #[serde(default)]
    pub requires_approval: Vec<String>,
    /// Blocked packages (cannot be installed)
    #[serde(default = "default_blocked_packages")]
    pub blocked: Vec<String>,
}

impl Default for PackageAllowlistConfig {
    fn default() -> Self {
        Self {
            auto_approved: default_auto_approved_packages(),
            requires_approval: vec![],
            blocked: default_blocked_packages(),
        }
    }
}

fn default_auto_approved_packages() -> Vec<String> {
    vec![
        "pandas".to_string(),
        "numpy".to_string(),
        "matplotlib".to_string(),
        "requests".to_string(),
        "beautifulsoup4".to_string(),
        "pillow".to_string(),
        "openpyxl".to_string(),
        "xlrd".to_string(),
        "pyyaml".to_string(),
        "jinja2".to_string(),
        "seaborn".to_string(),
        "scipy".to_string(),
        "scikit-learn".to_string(),
        // JavaScript packages
        "lodash".to_string(),
        "axios".to_string(),
        "moment".to_string(),
        "async".to_string(),
        "chalk".to_string(),
    ]
}

fn default_blocked_packages() -> Vec<String> {
    vec![
        "subprocess32".to_string(),
        "pyautogui".to_string(),
        "keyboard".to_string(),
        "mouse".to_string(),
    ]
}

/// Pre-baked sandbox image configuration
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct SandboxImageConfig {
    /// Image name
    pub name: String,
    /// Base image (e.g., python:3.12-slim)
    pub base: String,
    /// Pre-installed packages
    #[serde(default)]
    pub packages: Vec<String>,
    /// Warm pool size (number of pre-warmed instances)
    #[serde(default)]
    pub warm_pool_size: u32,
}

impl Default for SandboxImageConfig {
    fn default() -> Self {
        Self {
            name: "python-data-science".to_string(),
            base: "python:3.12-slim".to_string(),
            packages: vec![
                "pandas>=2.0".to_string(),
                "numpy>=1.24".to_string(),
                "matplotlib>=3.7".to_string(),
                "seaborn>=0.12".to_string(),
                "scipy>=1.10".to_string(),
                "scikit-learn>=1.3".to_string(),
                "requests>=2.28".to_string(),
            ],
            warm_pool_size: 0,
        }
    }
}

/// Coding agents configuration for specialized code generation
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct CodingAgentsConfig {
    /// Default coding agent profile to use
    #[serde(default = "default_coding_agent")]
    pub default: String,
    /// Available coding agent profiles
    #[serde(default = "default_coding_profiles")]
    pub profiles: Vec<CodingAgentProfile>,
    /// Enable test generation
    #[serde(default = "default_true")]
    pub generate_tests: bool,
    /// Maximum turns for iterative code refinement
    #[serde(default = "default_max_coding_turns")]
    pub max_coding_turns: u32,
}

impl Default for CodingAgentsConfig {
    fn default() -> Self {
        Self {
            default: default_coding_agent(),
            profiles: default_coding_profiles(),
            generate_tests: true,
            max_coding_turns: 5,
        }
    }
}

fn default_coding_agent() -> String {
    "deepseek-coder".to_string()
}

fn default_max_coding_turns() -> u32 {
    5
}

fn default_coding_profiles() -> Vec<CodingAgentProfile> {
    vec![
        CodingAgentProfile {
            name: "deepseek-coder".to_string(),
            provider: "openrouter".to_string(),
            model: "deepseek/deepseek-v3.2".to_string(),
            api_key_env: "OPENROUTER_API_KEY".to_string(),
            system_prompt: "You are an expert code generation assistant. Generate clean, well-documented code. Always include: 1. Import statements, 2. Type hints (Python) or TypeScript types (JS), 3. Error handling, 4. Brief comments explaining logic, 5. Unit tests.".to_string(),
            max_tokens: 4096,
            temperature: 0.2,
        },
        CodingAgentProfile {
            name: "claude-sonnet".to_string(),
            provider: "anthropic".to_string(),
            model: "claude-3-sonnet-20240229".to_string(),
            api_key_env: "ANTHROPIC_API_KEY".to_string(),
            system_prompt: "You are an expert programmer. Write production-quality code. Focus on correctness, efficiency, and maintainability. Always validate inputs and handle edge cases. Include comprehensive unit tests.".to_string(),
            max_tokens: 8192,
            temperature: 0.3,
        },
        CodingAgentProfile {
            name: "gpt-4".to_string(),
            provider: "openai".to_string(),
            model: "gpt-4-turbo-preview".to_string(),
            api_key_env: "OPENAI_API_KEY".to_string(),
            system_prompt: "You are a senior software engineer. Generate high-quality, maintainable code with excellent documentation and comprehensive test coverage.".to_string(),
            max_tokens: 4096,
            temperature: 0.2,
        },
    ]
}

/// Individual coding agent profile configuration
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct CodingAgentProfile {
    /// Profile name (used for selection)
    pub name: String,
    /// LLM provider (openrouter, anthropic, openai)
    pub provider: String,
    /// Model identifier
    pub model: String,
    /// Environment variable for API key
    pub api_key_env: String,
    /// System prompt for code generation
    pub system_prompt: String,
    /// Maximum tokens for response
    #[serde(default = "default_max_tokens")]
    pub max_tokens: u32,
    /// Temperature for generation (0.0-1.0)
    #[serde(default = "default_temperature")]
    pub temperature: f32,
}

fn default_max_tokens() -> u32 {
    4096
}

fn default_temperature() -> f32 {
    0.2
}

impl AgentConfig {
    /// Load agent configuration from environment (AGENT_CONFIG_PATH) or use default
    ///
    /// Note: For full parsing support, use `ccos::config::parser::AgentConfigParser`
    /// or the CCOS CLI/config loading utilities.
    pub fn from_env() -> Self {
        let config_path = std::env::var("AGENT_CONFIG_PATH");
        if let Ok(path) = config_path {
            if let Ok(_content) = std::fs::read_to_string(&path) {
                // TODO: Integrate with CCOS config parser when available
                // For now, return default if file exists but parsing is deferred
                return Self::default();
            }
        }
        Self::default()
    }
}

/// Storage paths configuration for capability directories
/// All paths are relative to the config file's directory unless absolute.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct StoragePathsConfig {
    /// Base directory for all capabilities (relative to config file or absolute)
    /// Default: "../capabilities" (one level up from config/ directory)
    #[serde(default = "default_capabilities_dir")]
    pub capabilities_dir: String,
    /// Subdirectory for generated/synthesized capabilities within capabilities_dir
    #[serde(default = "default_generated_subdir")]
    pub generated_subdir: String,
    /// Subdirectory for approved server capabilities within capabilities_dir
    #[serde(default = "default_servers_approved_subdir")]
    pub servers_approved_subdir: String,
    /// Subdirectory for pending synthesis queue (relative to storage root)
    #[serde(default = "default_pending_synth_subdir")]
    pub pending_synth_subdir: String,
    /// Directory for unified approval requests
    #[serde(default = "default_approvals_dir")]
    pub approvals_dir: String,
    /// Subdirectory for sessions within storage root or capabilities_dir
    #[serde(default = "default_sessions_subdir")]
    pub sessions_subdir: String,
}

fn default_capabilities_dir() -> String {
    "../capabilities".to_string()
}

fn default_generated_subdir() -> String {
    "generated".to_string()
}

fn default_servers_approved_subdir() -> String {
    "servers/approved".to_string()
}

fn default_pending_synth_subdir() -> String {
    "storage/pending_synth".to_string()
}

fn default_approvals_dir() -> String {
    "storage/approvals".to_string()
}

fn default_sessions_subdir() -> String {
    "sessions".to_string()
}

impl Default for StoragePathsConfig {
    fn default() -> Self {
        Self {
            capabilities_dir: default_capabilities_dir(),
            generated_subdir: default_generated_subdir(),
            servers_approved_subdir: default_servers_approved_subdir(),
            pending_synth_subdir: default_pending_synth_subdir(),
            approvals_dir: default_approvals_dir(),
            sessions_subdir: default_sessions_subdir(),
        }
    }
}

/// AI Self-Programming configuration for autonomous capability evolution
/// Controls trust levels, approval gates, and safety boundaries
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct SelfProgrammingConfig {
    /// Enable self-programming features (decompose, resolve, synthesize)
    #[serde(default = "default_true")]
    pub enabled: bool,
    /// Trust level (0-4):
    /// 0 = ALL self-modification requires approval (recommended for start)
    /// 1 = Read-only introspection auto-approved
    /// 2 = Pure data transforms auto-approved
    /// 3 = Capability synthesis auto-approved (with rollback)
    /// 4 = Full autonomy (use with extreme caution)
    #[serde(default)]
    pub trust_level: u8,
    /// Maximum synthesis attempts per session before requiring approval
    #[serde(default = "default_max_synthesis")]
    pub max_synthesis_per_session: u32,
    /// Maximum recursion depth for goal decomposition
    #[serde(default = "default_max_decomposition_depth")]
    pub max_decomposition_depth: u32,
    /// Require explicit approval for capability registration (enforced at trust_level < 3)
    #[serde(default = "default_true")]
    pub require_approval_for_registration: bool,
    /// Enable capability versioning for rollback support
    #[serde(default = "default_true")]
    pub enable_versioning: bool,
    /// Auto-rollback on execution failure
    #[serde(default = "default_true")]
    pub auto_rollback_on_failure: bool,
}

fn default_max_synthesis() -> u32 {
    10
}

fn default_max_decomposition_depth() -> u32 {
    5
}

impl Default for SelfProgrammingConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            trust_level: 0, // Start with full approval required
            max_synthesis_per_session: 10,
            max_decomposition_depth: 5,
            require_approval_for_registration: true,
            enable_versioning: true,
            auto_rollback_on_failure: true,
        }
    }
}

impl SelfProgrammingConfig {
    /// Check if the given action type is auto-approved at the current trust level
    pub fn is_auto_approved(&self, action: SelfProgrammingAction) -> bool {
        match action {
            SelfProgrammingAction::Introspect => self.trust_level >= 1,
            SelfProgrammingAction::Transform => self.trust_level >= 2,
            SelfProgrammingAction::Synthesize => self.trust_level >= 3,
            SelfProgrammingAction::Register => {
                !self.require_approval_for_registration || self.trust_level >= 3
            }
            SelfProgrammingAction::Execute => self.trust_level >= 4,
        }
    }
}

/// Types of self-programming actions for approval checking
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum SelfProgrammingAction {
    /// Read-only introspection (plan traces, capability listing)
    Introspect,
    /// Pure data transformation (no side effects)
    Transform,
    /// Capability synthesis (creating new capabilities)
    Synthesize,
    /// Capability registration (adding to marketplace)
    Register,
    /// Capability execution (running synthesized code)
    Execute,
}

/// A named LLM model profile that can be selected at runtime
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct LlmProfile {
    /// Human readable / command name (unique)
    pub name: String,
    /// Provider identifier (openai, openrouter, claude, stub, local)
    pub provider: String,
    /// Concrete model slug for the provider
    pub model: String,
    /// Optional base URL override (for proxies / OpenRouter / self-hosted gateways)
    pub base_url: Option<String>,
    /// Name of environment variable containing API key (preferred over inline key)
    pub api_key_env: Option<String>,
    /// Inline API key (discouraged for committed configs, but allowed for throwaway local files)
    pub api_key: Option<String>,
    /// Temperature override
    pub temperature: Option<f64>,
    /// Max tokens override
    pub max_tokens: Option<u32>,
}

/// Collection of LLM profiles with an optional default
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
pub struct LlmProfilesConfig {
    /// Optional default profile name selected on startup
    pub default: Option<String>,
    /// Available profiles
    pub profiles: Vec<LlmProfile>,
    /// Optional grouped model sets per provider (expanded into synthetic profiles at runtime)
    pub model_sets: Option<Vec<LlmModelSet>>,
}

/// A named set of related model specs for a single provider (e.g. tiers: fast, balanced, high_quality)
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct LlmModelSet {
    /// Set name (used as namespace prefix when expanding to synthetic profiles)
    pub name: String,
    /// Provider identifier (openai, openrouter, claude, gemini, stub, local)
    pub provider: String,
    /// Optional base URL override per set
    pub base_url: Option<String>,
    /// API key environment variable name
    pub api_key_env: Option<String>,
    /// Inline API key (discouraged for committed configs)
    pub api_key: Option<String>,
    /// Optional default spec name within this set
    pub default: Option<String>,
    /// Model specifications contained in the set
    pub models: Vec<LlmModelSpec>,
}

/// Specification metadata for a model option inside a set
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct LlmModelSpec {
    /// Short human readable spec name (e.g. fast, quality, reasoning)
    pub name: String,
    /// Concrete provider model slug
    pub model: String,
    /// Maximum acceptable prompt cost USD per 1K tokens (if known)
    pub max_prompt_cost_per_1k: Option<f64>,
    /// Maximum acceptable completion cost USD per 1K tokens (if known)
    pub max_completion_cost_per_1k: Option<f64>,
    /// Soft quality tier label (e.g. basic, standard, premium, reasoning)
    pub quality: Option<String>,
    /// Upper bound on output tokens for planning flows (override)
    pub max_output_tokens: Option<u32>,
    /// Free-form notes (latency expectations, etc.)
    pub notes: Option<String>,
}

/// Discovery engine configuration persisted in agent config
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct DiscoveryConfig {
    /// Minimum semantic match score threshold (0.0 to 1.0)
    #[serde(default = "default_discovery_match_threshold")]
    pub match_threshold: f64,
    /// Enable embedding-based matching (more accurate but requires API)
    #[serde(default = "default_discovery_use_embeddings")]
    pub use_embeddings: bool,
    /// Preferred remote embedding model (e.g., OpenRouter)
    #[serde(default)]
    pub embedding_model: Option<String>,
    /// Preferred local embedding model (e.g., Ollama)
    #[serde(default)]
    pub local_embedding_model: Option<String>,
    /// Minimum score required for action verb match (0.0 to 1.0)
    #[serde(default = "default_discovery_action_verb_threshold")]
    pub action_verb_threshold: f64,
    /// Weight for action verbs in matching (higher = more important)
    #[serde(default = "default_discovery_action_verb_weight")]
    pub action_verb_weight: f64,
    /// Weight for capability class matching (0.0 to 1.0)
    #[serde(default = "default_discovery_capability_class_weight")]
    pub capability_class_weight: f64,
}

impl Default for DiscoveryConfig {
    fn default() -> Self {
        Self {
            match_threshold: default_discovery_match_threshold(),
            use_embeddings: default_discovery_use_embeddings(),
            embedding_model: None,
            local_embedding_model: None,
            action_verb_threshold: default_discovery_action_verb_threshold(),
            action_verb_weight: default_discovery_action_verb_weight(),
            capability_class_weight: default_discovery_capability_class_weight(),
        }
    }
}

/// Configuration for LLM validation (schema, plan, auto-repair)
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ValidationConfig {
    /// Enable LLM schema validation for inferred schemas
    #[serde(default)]
    pub enable_schema_validation: bool,

    /// Enable LLM plan validation (schema compatibility, dependencies)
    #[serde(default)]
    pub enable_plan_validation: bool,

    /// Enable auto-repair on validation failures
    #[serde(default = "default_true")]
    pub enable_auto_repair: bool,

    /// Enable LLM repair for runtime execution errors (dialog-based)
    #[serde(default = "default_true")]
    pub enable_runtime_repair: bool,

    /// Max auto-repair attempts before queuing for external review
    #[serde(default = "default_max_repair_attempts")]
    pub max_repair_attempts: usize,

    /// Max runtime repair attempts (dialog loop bound)
    #[serde(default = "default_max_runtime_repair_attempts")]
    pub max_runtime_repair_attempts: usize,

    /// LLM profile to use for validation (from llm_profiles section)
    /// Format: "set:model" or "profile_name"
    #[serde(default)]
    pub validation_profile: Option<String>,
}

impl Default for ValidationConfig {
    fn default() -> Self {
        Self {
            enable_schema_validation: false,
            enable_plan_validation: false,
            enable_auto_repair: true,
            enable_runtime_repair: true,
            max_repair_attempts: 2,
            max_runtime_repair_attempts: 5,
            validation_profile: None,
        }
    }
}

/// Server discovery pipeline configuration (configurable stages + ordering)
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ServerDiscoveryPipelineConfig {
    /// Enable the pipeline globally
    #[serde(default = "default_true")]
    pub enabled: bool,
    /// Default execution mode: "preview" or "stage_and_queue"
    #[serde(default = "default_server_discovery_mode")]
    pub mode: String,
    /// Ordered stages for query discovery (e.g. registry_search → llm_suggest → rank → dedupe → limit)
    #[serde(default = "default_query_pipeline_order")]
    pub query_pipeline_order: Vec<String>,
    /// Ordered introspection stages (e.g. mcp → openapi → browser)
    #[serde(default = "default_introspection_order")]
    pub introspection_order: Vec<String>,
    /// Max number of candidates returned from discovery stages before ranking
    #[serde(default = "default_server_discovery_max_candidates")]
    pub max_candidates: usize,
    /// Max number of ranked candidates to keep
    #[serde(default = "default_server_discovery_max_ranked")]
    pub max_ranked: usize,
    /// Default relevance threshold
    #[serde(default = "default_discovery_match_threshold")]
    pub threshold: f64,
    /// Source enable/disable configuration
    #[serde(default)]
    pub sources: ServerDiscoverySourcesConfig,
    /// Introspection enable/disable configuration
    #[serde(default)]
    pub introspection: ServerDiscoveryIntrospectionConfig,
    /// Staging configuration
    #[serde(default)]
    pub staging: ServerDiscoveryStagingConfig,
    /// Approvals configuration
    #[serde(default)]
    pub approvals: ServerDiscoveryApprovalsConfig,
}

impl Default for ServerDiscoveryPipelineConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            mode: default_server_discovery_mode(),
            query_pipeline_order: default_query_pipeline_order(),
            introspection_order: default_introspection_order(),
            max_candidates: default_server_discovery_max_candidates(),
            max_ranked: default_server_discovery_max_ranked(),
            threshold: default_discovery_match_threshold(),
            sources: ServerDiscoverySourcesConfig::default(),
            introspection: ServerDiscoveryIntrospectionConfig::default(),
            staging: ServerDiscoveryStagingConfig::default(),
            approvals: ServerDiscoveryApprovalsConfig::default(),
        }
    }
}

/// Source enablement and tuning for discovery candidates
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ServerDiscoverySourcesConfig {
    /// Use MCP registry as a discovery source
    #[serde(default = "default_true")]
    pub mcp_registry: bool,
    /// Use NPM registry for MCP-related packages
    #[serde(default = "default_true")]
    pub npm: bool,
    /// Use local overrides.json entries
    #[serde(default = "default_true")]
    pub overrides: bool,
    /// Use APIs.guru OpenAPI catalog
    #[serde(default = "default_true")]
    pub apis_guru: bool,
    /// Use web search discovery (fallback, potentially slow)
    #[serde(default)]
    pub web_search: Option<bool>,
    /// Use LLM to suggest APIs from a query
    #[serde(default = "default_true")]
    pub llm_suggest: bool,
    /// Use known APIs registry (static curated set)
    #[serde(default = "default_true")]
    pub known_apis: bool,
}

impl Default for ServerDiscoverySourcesConfig {
    fn default() -> Self {
        Self {
            mcp_registry: true,
            npm: true,
            overrides: true,
            apis_guru: true,
            web_search: None,
            llm_suggest: true,
            known_apis: true,
        }
    }
}

/// Introspection enablement per protocol
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ServerDiscoveryIntrospectionConfig {
    /// Enable MCP HTTP introspection
    #[serde(default = "default_true")]
    pub mcp_http: bool,
    /// Enable MCP stdio introspection
    #[serde(default = "default_true")]
    pub mcp_stdio: bool,
    /// Enable OpenAPI introspection
    #[serde(default = "default_true")]
    pub openapi: bool,
    /// Enable browser-based docs extraction
    #[serde(default = "default_true")]
    pub browser: bool,
}

impl Default for ServerDiscoveryIntrospectionConfig {
    fn default() -> Self {
        Self {
            mcp_http: true,
            mcp_stdio: true,
            openapi: true,
            browser: true,
        }
    }
}

/// Staging configuration for pipeline outputs
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ServerDiscoveryStagingConfig {
    /// Pending subdirectory for staged capability files
    #[serde(default = "default_server_discovery_pending_subdir")]
    pub pending_subdir: String,
    /// Server ID strategy (e.g. sanitize_filename)
    #[serde(default = "default_server_discovery_server_id_strategy")]
    pub server_id_strategy: String,
    /// Layout name for staged files
    #[serde(default = "default_server_discovery_layout")]
    pub layout: String,
}

impl Default for ServerDiscoveryStagingConfig {
    fn default() -> Self {
        Self {
            pending_subdir: default_server_discovery_pending_subdir(),
            server_id_strategy: default_server_discovery_server_id_strategy(),
            layout: default_server_discovery_layout(),
        }
    }
}

/// Approval settings for staged server discoveries
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ServerDiscoveryApprovalsConfig {
    /// Whether to create approval requests for discoveries
    #[serde(default = "default_true")]
    pub enabled: bool,
    /// Expiry (hours) for approval requests
    #[serde(default = "default_server_discovery_expiry_hours")]
    pub expiry_hours: i64,
    /// Default risk level label (low/medium/high)
    #[serde(default = "default_server_discovery_risk_default")]
    pub risk_default: String,
}

impl Default for ServerDiscoveryApprovalsConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            expiry_hours: default_server_discovery_expiry_hours(),
            risk_default: default_server_discovery_risk_default(),
        }
    }
}

fn default_max_repair_attempts() -> usize {
    2
}

fn default_max_runtime_repair_attempts() -> usize {
    5
}

fn default_discovery_match_threshold() -> f64 {
    0.65
}

fn default_discovery_use_embeddings() -> bool {
    false
}

fn default_discovery_action_verb_threshold() -> f64 {
    0.7
}

fn default_discovery_action_verb_weight() -> f64 {
    0.4
}

fn default_discovery_capability_class_weight() -> f64 {
    0.3
}

fn default_server_discovery_mode() -> String {
    "stage_and_queue".to_string()
}

fn default_query_pipeline_order() -> Vec<String> {
    vec![
        "registry_search".to_string(),
        "llm_suggest".to_string(),
        "rank".to_string(),
        "dedupe".to_string(),
        "limit".to_string(),
    ]
}

fn default_introspection_order() -> Vec<String> {
    vec![
        "mcp".to_string(),
        "openapi".to_string(),
        "browser".to_string(),
    ]
}

fn default_server_discovery_max_candidates() -> usize {
    30
}

fn default_server_discovery_max_ranked() -> usize {
    15
}

fn default_server_discovery_pending_subdir() -> String {
    "capabilities/servers/pending".to_string()
}

fn default_server_discovery_server_id_strategy() -> String {
    "sanitize_filename".to_string()
}

fn default_server_discovery_layout() -> String {
    "rtfs_layout_v1".to_string()
}

fn default_server_discovery_expiry_hours() -> i64 {
    168
}

fn default_server_discovery_risk_default() -> String {
    "medium".to_string()
}

/// Catalog configuration stored in agent config
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct CatalogConfig {
    #[serde(default = "default_catalog_plan_min_score")]
    pub plan_min_score: f32,
    #[serde(default = "default_catalog_keyword_min_score")]
    pub keyword_min_score: f32,
}

impl Default for CatalogConfig {
    fn default() -> Self {
        Self {
            plan_min_score: default_catalog_plan_min_score(),
            keyword_min_score: default_catalog_keyword_min_score(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
pub struct MissingCapabilityToolSelectorRuntimeConfig {
    /// Enable or disable the LLM-based tool selector
    pub enabled: Option<bool>,
    /// Override prompt ID used for selection
    pub prompt_id: Option<String>,
    /// Override prompt version used for selection
    pub prompt_version: Option<String>,
    /// Limit number of tools provided to the selector
    pub max_tools: Option<u32>,
    /// Limit candidate description length
    pub max_description_chars: Option<u32>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
pub struct MissingCapabilityRuntimeConfig {
    /// Enable or disable missing capability runtime detection
    pub enabled: Option<bool>,
    /// Allow runtime detection of missing capabilities
    #[serde(rename = "runtime_detection")]
    pub runtime_detection: Option<bool>,
    /// Automatically attempt resolution when a capability is missing
    pub auto_resolution: Option<bool>,
    /// Permit the resolver to call the LLM synthesis pipeline
    pub llm_synthesis: Option<bool>,
    /// Allow discovery fallback to web search
    pub web_search: Option<bool>,
    /// Require human approval before registering synthesized capabilities
    pub human_approval_required: Option<bool>,
    /// Emit verbose resolver logs
    pub verbose_logging: Option<bool>,
    /// Maximum resolution attempts before giving up
    pub max_attempts: Option<u32>,
    /// Enable output schema introspection (requires auth)
    pub output_schema_introspection: Option<bool>,
    /// Tool selector configuration (LLM-assisted matching)
    #[serde(default)]
    pub tool_selector: MissingCapabilityToolSelectorRuntimeConfig,
}

fn default_catalog_plan_min_score() -> f32 {
    0.65
}

fn default_catalog_keyword_min_score() -> f32 {
    1.0
}

/// Orchestrator configuration
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
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
    /// Filesystem configuration (defaults if omitted in config)
    #[serde(default)]
    pub fs: FSConfig,
}

impl Default for IsolationConfig {
    fn default() -> Self {
        Self {
            mode: "wasm".to_string(),
            fs: FSConfig::default(),
        }
    }
}

/// Filesystem configuration
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
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

impl Default for MountConfig {
    fn default() -> Self {
        Self {
            mode: "ro".to_string(),
        }
    }
}

/// DLP (Data Loss Prevention) configuration
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
pub struct DLPConfig {
    /// Whether DLP is enabled
    pub enabled: bool,
    /// Active DLP policy (lenient|strict)
    #[serde(default = "default_dlp_policy")]
    pub policy: String,
}

fn default_dlp_policy() -> String {
    "lenient".to_string()
}

/// Network configuration
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
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
    #[serde(default)]
    pub allow_domains: Vec<String>,
    /// Whether mTLS is enabled
    pub mtls: bool,
    /// TLS certificate pins
    #[serde(default)]
    pub tls_pins: Vec<String>,
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
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
pub struct CapabilitiesConfig {
    /// HTTP capability configuration
    pub http: HTTPCapabilityConfig,
    /// Filesystem capability configuration
    pub fs: FSCapabilityConfig,
    /// LLM capability configuration
    pub llm: LLMCapabilityConfig,
}

/// HTTP capability configuration
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
pub struct HTTPCapabilityConfig {
    /// Whether HTTP capability is enabled
    pub enabled: bool,
    /// HTTP egress configuration
    pub egress: HTTPEgressConfig,
}

/// HTTP egress configuration
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
pub struct HTTPEgressConfig {
    /// Allowed domains for HTTP
    pub allow_domains: Vec<String>,
    /// Whether mTLS is enabled for HTTP
    pub mtls: bool,
}

/// Filesystem capability configuration
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
pub struct FSCapabilityConfig {
    /// Whether filesystem capability is enabled
    pub enabled: bool,
}

/// LLM capability configuration
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
pub struct LLMCapabilityConfig {
    /// Whether LLM capability is enabled
    pub enabled: bool,
}

/// Governance configuration
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
pub struct GovernanceConfig {
    /// Policy configurations
    pub policies: HashMap<String, PolicyConfig>,
    /// Key configuration
    pub keys: KeyConfig,
}

/// Policy configuration
use serde::de::{Deserializer, Error as DeError};
#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct PolicyConfig {
    /// Risk tier (low, medium, high)
    pub risk_tier: String,
    /// Number of required approvals
    pub requires_approvals: u32,
    /// Budget configuration
    pub budgets: BudgetConfig,
}

/// Budget configuration
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
pub struct BudgetConfig {
    /// Budget limits
    pub limits: BudgetLimits,
    /// Budget policies
    pub policies: BudgetPolicies,
}

/// Key configuration
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
pub struct KeyConfig {
    /// Verification public key
    pub verify: String,
}

/// Causal Chain configuration
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
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

impl Default for StorageConfig {
    fn default() -> Self {
        Self {
            mode: "in_memory".to_string(),
            buffer_local: None,
        }
    }
}

/// Anchor configuration
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
pub struct AnchorConfig {
    /// Whether anchoring is enabled
    pub enabled: bool,
}

/// Marketplace configuration
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
pub struct MarketplaceConfig {
    /// Registry paths
    pub registry_paths: Vec<String>,
    /// Whether marketplace is read-only
    pub readonly: Option<bool>,
}

/// Delegation configuration
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
pub struct DelegationConfig {
    /// NOTE: This is the external/serializable shape used for user-provided
    /// agent configuration (AgentConfig). It intentionally differs from the
    /// runtime `DelegationConfig` found in `ccos::arbiter::arbiter_config`.
    /// The runtime shape is more opinionated (concrete types) and is produced
    /// by `DelegationConfig::to_arbiter_config()` below. Keep both in sync by
    /// adding fields here and forwarding them in `to_arbiter_config()`.

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
    /// Whether to print the extracted RTFS intent s-expression when debugging
    /// (config-level override; runtime may still read env vars).
    pub print_extracted_intent: Option<bool>,
    /// Whether to print the extracted RTFS plan s-expression when debugging
    /// (config-level override; runtime may still read env vars).
    pub print_extracted_plan: Option<bool>,
    /// Adaptive threshold configuration
    pub adaptive_threshold: Option<AdaptiveThresholdConfig>,
}

/// Agent registry configuration
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
pub struct AgentRegistryConfig {
    /// Registry type (in_memory, database, etc.)
    pub registry_type: RegistryType,
    /// Database connection string (if applicable)
    pub database_url: Option<String>,
    /// Agent definitions
    pub agents: Vec<AgentDefinition>,
}

/// Registry types
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
pub enum RegistryType {
    /// In-memory registry
    #[default]
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

impl Default for PolicyConfig {
    fn default() -> Self {
        Self {
            risk_tier: "low".to_string(),
            requires_approvals: 0,
            budgets: BudgetConfig::default(),
        }
    }
}

impl<'de> Deserialize<'de> for PolicyConfig {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        #[derive(Deserialize)]
        #[serde(untagged)]
        enum Repr {
            Full {
                risk_tier: String,
                requires_approvals: u32,
                budgets: BudgetConfig,
            },
            Simple(String),
        }
        match Repr::deserialize(deserializer)? {
            Repr::Full {
                risk_tier,
                requires_approvals,
                budgets,
            } => Ok(PolicyConfig {
                risk_tier,
                requires_approvals,
                budgets,
            }),
            Repr::Simple(s) => {
                let (risk_tier, requires_approvals, budgets) = match s.as_str() {
                    "allow" | "lenient" => (
                        "low".to_string(),
                        0,
                        BudgetConfig {
                            limits: BudgetLimits {
                                llm_tokens: 10_000,
                                cost_usd: 1.0,
                                ..Default::default()
                            },
                            policies: BudgetPolicies {
                                llm_tokens: ExhaustionPolicy::SoftWarn,
                                cost_usd: ExhaustionPolicy::SoftWarn,
                                ..Default::default()
                            },
                        },
                    ),
                    "moderate" => (
                        "medium".to_string(),
                        1,
                        BudgetConfig {
                            limits: BudgetLimits {
                                llm_tokens: 50_000,
                                cost_usd: 5.0,
                                ..Default::default()
                            },
                            policies: BudgetPolicies {
                                llm_tokens: ExhaustionPolicy::ApprovalRequired,
                                cost_usd: ExhaustionPolicy::ApprovalRequired,
                                ..Default::default()
                            },
                        },
                    ),
                    "strict" | "deny" => (
                        "high".to_string(),
                        2,
                        BudgetConfig {
                            limits: BudgetLimits {
                                llm_tokens: 0,
                                cost_usd: 0.0,
                                ..Default::default()
                            },
                            policies: BudgetPolicies {
                                llm_tokens: ExhaustionPolicy::HardStop,
                                cost_usd: ExhaustionPolicy::HardStop,
                                ..Default::default()
                            },
                        },
                    ),
                    other => {
                        return Err(D::Error::custom(format!(
                            "unknown policy shorthand '{}' (expected allow|moderate|strict)",
                            other
                        )))
                    }
                };
                Ok(PolicyConfig {
                    risk_tier,
                    requires_approvals,
                    budgets,
                })
            }
        }
    }
}

impl DelegationConfig {
    /// Convert AgentConfig DelegationConfig to arbiter DelegationConfig
    /// Note: Full implementation available when RTFS is used with CCOS
    #[cfg(feature = "ccos-integration")]
    pub fn to_arbiter_config(&self) {
        // Implementation requires CCOS integration
        todo!("CCOS integration required for to_arbiter_config")
    }

    #[cfg(not(feature = "ccos-integration"))]
    #[cfg(test)]
    fn to_arbiter_config(&self) -> DelegationConfigStub {
        // Stub implementation for RTFS-only tests
        DelegationConfigStub {
            enabled: self.enabled.unwrap_or(true),
            threshold: self.threshold.unwrap_or(0.65),
            max_candidates: self.max_candidates.unwrap_or(3) as usize,
            min_skill_hits: self.min_skill_hits,
            agent_registry: self
                .agent_registry
                .clone()
                .unwrap_or_else(|| AgentRegistryConfig {
                    registry_type: RegistryType::InMemory,
                    database_url: None,
                    agents: vec![],
                }),
            adaptive_threshold: self.adaptive_threshold.clone(),
        }
    }

    #[cfg(not(feature = "ccos-integration"))]
    #[cfg(not(test))]
    pub fn to_arbiter_config(&self) -> () {
        // Stub implementation for RTFS-only mode (non-test)
        ()
    }
}

#[cfg(test)]
#[derive(Debug, Clone, PartialEq)]
struct DelegationConfigStub {
    enabled: bool,
    threshold: f64,
    max_candidates: usize,
    min_skill_hits: Option<u32>,
    agent_registry: AgentRegistryConfig,
    adaptive_threshold: Option<AdaptiveThresholdConfig>,
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

    // --- SelfProgrammingConfig Tests ---

    #[test]
    fn test_self_programming_config_default() {
        let config = SelfProgrammingConfig::default();
        assert!(config.enabled);
        assert_eq!(config.trust_level, 0); // Default is most restrictive
        assert_eq!(config.max_synthesis_per_session, 10);
        assert_eq!(config.max_decomposition_depth, 5);
        assert!(config.require_approval_for_registration);
        assert!(config.enable_versioning);
        assert!(config.auto_rollback_on_failure);
    }

    #[test]
    fn test_self_programming_trust_level_0() {
        let config = SelfProgrammingConfig {
            trust_level: 0,
            ..Default::default()
        };

        // At level 0, NOTHING is auto-approved
        assert!(!config.is_auto_approved(SelfProgrammingAction::Introspect));
        assert!(!config.is_auto_approved(SelfProgrammingAction::Transform));
        assert!(!config.is_auto_approved(SelfProgrammingAction::Synthesize));
        assert!(!config.is_auto_approved(SelfProgrammingAction::Register));
        assert!(!config.is_auto_approved(SelfProgrammingAction::Execute));
    }

    #[test]
    fn test_self_programming_trust_level_1() {
        let config = SelfProgrammingConfig {
            trust_level: 1,
            ..Default::default()
        };

        // Level 1: Only introspection auto-approved
        assert!(config.is_auto_approved(SelfProgrammingAction::Introspect));
        assert!(!config.is_auto_approved(SelfProgrammingAction::Transform));
        assert!(!config.is_auto_approved(SelfProgrammingAction::Synthesize));
        assert!(!config.is_auto_approved(SelfProgrammingAction::Register));
        assert!(!config.is_auto_approved(SelfProgrammingAction::Execute));
    }

    #[test]
    fn test_self_programming_trust_level_2() {
        let config = SelfProgrammingConfig {
            trust_level: 2,
            ..Default::default()
        };

        // Level 2: Introspection + Transform
        assert!(config.is_auto_approved(SelfProgrammingAction::Introspect));
        assert!(config.is_auto_approved(SelfProgrammingAction::Transform));
        assert!(!config.is_auto_approved(SelfProgrammingAction::Synthesize));
        assert!(!config.is_auto_approved(SelfProgrammingAction::Register)); // require_approval_for_registration=true
        assert!(!config.is_auto_approved(SelfProgrammingAction::Execute));
    }
}

/// Real-time tracking and observability configuration for gateway/agent monitoring
/// Controls WebSocket streaming, heartbeat intervals, and event replay
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct RealtimeTrackingConfig {
    /// WebSocket ping interval in seconds (keepalive)
    #[serde(default = "default_ws_ping_interval_secs")]
    pub ws_ping_interval_secs: u64,

    /// Agent heartbeat timeout for "unhealthy" status (seconds)
    #[serde(default = "default_heartbeat_timeout_secs")]
    pub heartbeat_timeout_secs: u64,

    /// Agent heartbeat send interval (seconds)
    #[serde(default = "default_heartbeat_send_interval_secs")]
    pub heartbeat_send_interval_secs: u64,

    /// Gateway health check interval (seconds)
    #[serde(default = "default_health_check_interval_secs")]
    pub health_check_interval_secs: u64,

    /// Number of historical events to replay on WebSocket connect
    #[serde(default = "default_event_replay_limit")]
    pub event_replay_limit: usize,
}

fn default_ws_ping_interval_secs() -> u64 {
    10
}
fn default_heartbeat_timeout_secs() -> u64 {
    3
}
fn default_heartbeat_send_interval_secs() -> u64 {
    1
}
fn default_health_check_interval_secs() -> u64 {
    5
}
fn default_event_replay_limit() -> usize {
    100
}

impl Default for RealtimeTrackingConfig {
    fn default() -> Self {
        Self {
            ws_ping_interval_secs: default_ws_ping_interval_secs(),
            heartbeat_timeout_secs: default_heartbeat_timeout_secs(),
            heartbeat_send_interval_secs: default_heartbeat_send_interval_secs(),
            health_check_interval_secs: default_health_check_interval_secs(),
            event_replay_limit: default_event_replay_limit(),
        }
    }
}

#[cfg(test)]
mod tests_realtime {
    use super::*;

    #[test]
    fn test_self_programming_trust_level_3() {
        let config = SelfProgrammingConfig {
            trust_level: 3,
            ..Default::default()
        };

        // Level 3: Introspection + Transform + Synthesize + Register
        assert!(config.is_auto_approved(SelfProgrammingAction::Introspect));
        assert!(config.is_auto_approved(SelfProgrammingAction::Transform));
        assert!(config.is_auto_approved(SelfProgrammingAction::Synthesize));
        assert!(config.is_auto_approved(SelfProgrammingAction::Register));
        assert!(!config.is_auto_approved(SelfProgrammingAction::Execute));
    }

    #[test]
    fn test_self_programming_trust_level_4_full_autonomy() {
        let config = SelfProgrammingConfig {
            trust_level: 4,
            ..Default::default()
        };

        // Level 4: Full autonomy - everything auto-approved
        assert!(config.is_auto_approved(SelfProgrammingAction::Introspect));
        assert!(config.is_auto_approved(SelfProgrammingAction::Transform));
        assert!(config.is_auto_approved(SelfProgrammingAction::Synthesize));
        assert!(config.is_auto_approved(SelfProgrammingAction::Register));
        assert!(config.is_auto_approved(SelfProgrammingAction::Execute));
    }

    #[test]
    fn test_self_programming_registration_requires_approval_flag() {
        // Test that require_approval_for_registration works independently
        let config_with_approval = SelfProgrammingConfig {
            trust_level: 2,
            require_approval_for_registration: true,
            ..Default::default()
        };
        assert!(!config_with_approval.is_auto_approved(SelfProgrammingAction::Register));

        let config_without_approval = SelfProgrammingConfig {
            trust_level: 2,
            require_approval_for_registration: false,
            ..Default::default()
        };
        assert!(config_without_approval.is_auto_approved(SelfProgrammingAction::Register));
    }

    #[test]
    fn test_self_programming_config_serialization() {
        let config = SelfProgrammingConfig {
            enabled: true,
            trust_level: 2,
            max_synthesis_per_session: 20,
            max_decomposition_depth: 3,
            require_approval_for_registration: false,
            enable_versioning: true,
            auto_rollback_on_failure: false,
        };

        let serialized = serde_json::to_string(&config).unwrap();
        let deserialized: SelfProgrammingConfig = serde_json::from_str(&serialized).unwrap();

        assert_eq!(config, deserialized);
    }
}
