use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Configuration for the Cognitive Engine system
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CognitiveEngineConfig {
    /// The type of cognitive engine to use
    pub engine_type: CognitiveEngineType,
    /// LLM-specific configuration (if using LLM engine)
    pub llm_config: Option<LlmConfig>,
    /// Delegation configuration (if using delegating engine)
    pub delegation_config: Option<DelegationConfig>,
    /// Capability configuration
    pub capability_config: CapabilityConfig,
    /// Security configuration
    pub security_config: SecurityConfig,
    /// Template patterns for template-based engine
    pub template_config: Option<TemplateConfig>,
}

/// Types of cognitive engines
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum CognitiveEngineType {
    /// Simple pattern matching with templates
    Template,
    /// LLM-driven reasoning
    Llm,
    /// LLM + agent delegation
    Delegating,
    /// Template fallback to LLM
    Hybrid,
    /// Deterministic dummy for testing
    Dummy,
}

/// Plan generation retry configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RetryConfig {
    /// Maximum number of retry attempts for plan generation
    pub max_retries: u32,
    /// Whether to send error feedback to LLM on retry
    pub send_error_feedback: bool,
    /// Whether to simplify the request on final retry attempt
    pub simplify_on_final_attempt: bool,
    /// Whether to use stub fallback when all retries fail
    pub use_stub_fallback: bool,
}

impl Default for RetryConfig {
    fn default() -> Self {
        Self {
            max_retries: 2,
            send_error_feedback: true,
            simplify_on_final_attempt: true,
            use_stub_fallback: false,
        }
    }
}

/// LLM provider configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LlmConfig {
    /// Provider type (openai, anthropic, stub, etc.)
    pub provider_type: LlmProviderType,
    /// Model name/identifier
    pub model: String,
    /// API key (can be loaded from env)
    pub api_key: Option<String>,
    /// Base URL for API (optional, for custom endpoints)
    pub base_url: Option<String>,
    /// Maximum tokens for context window
    pub max_tokens: Option<u32>,
    /// Temperature for generation (0.0 = deterministic, 1.0 = creative)
    pub temperature: Option<f64>,
    /// Timeout in seconds
    pub timeout_seconds: Option<u64>,
    /// Prompt selection and versioning
    #[serde(default)]
    pub prompts: Option<crate::arbiter::prompt::PromptConfig>,
    /// Plan generation retry configuration
    #[serde(default)]
    pub retry_config: RetryConfig,
}

// Re-export LlmProviderType from llm_provider module
pub use super::llm_provider::LlmProviderType;

impl LlmConfig {
    /// Convert LlmConfig to LlmProviderConfig
    pub fn to_provider_config(&self) -> super::llm_provider::LlmProviderConfig {
        super::llm_provider::LlmProviderConfig {
            provider_type: self.provider_type.clone(),
            model: self.model.clone(),
            api_key: self.api_key.clone(),
            base_url: self.base_url.clone(),
            max_tokens: self.max_tokens,
            temperature: self.temperature,
            timeout_seconds: self.timeout_seconds,
            retry_config: self.retry_config.clone(),
        }
    }
}

/// Delegation configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DelegationConfig {
    /// Whether delegation is enabled
    pub enabled: bool,
    /// Threshold score for delegation (0.0-1.0)
    pub threshold: f64,
    /// Maximum number of candidates to consider
    pub max_candidates: usize,
    /// Minimum skill hits required
    pub min_skill_hits: Option<usize>,
    /// [DEPRECATED] Agent registry configuration - use CapabilityMarketplace instead
    #[serde(default)]
    pub agent_registry: Option<serde_json::Value>,
    /// Adaptive threshold configuration
    pub adaptive_threshold: Option<crate::config::types::AdaptiveThresholdConfig>,
    /// Optional convenience flag to print the extracted RTFS intent s-expression
    /// when generating intents. When Some(true), the arbiter will print the
    /// extracted `(intent ...)` s-expression. This is intended for diagnostics.
    #[serde(default)]
    pub print_extracted_intent: Option<bool>,
    /// Optional convenience flag to print the extracted RTFS plan s-expression
    /// when generating plans. When Some(true), the arbiter will print the
    /// extracted RTFS plan (the `(do ...)` or plan body). Intended for diagnostics.
    #[serde(default)]
    pub print_extracted_plan: Option<bool>,
    // NOTE: These runtime debug flags are mirrored in the user-facing
    // `AgentConfig.delegation` shape (`src/config/types.rs::DelegationConfig`).
    // The external config uses Option<bool> so users can omit fields; they are
    // converted into concrete runtime values by `to_arbiter_config()`.
}

/// Agent registry configuration
///
/// # Deprecation Notice
/// This struct is deprecated. Use CapabilityMarketplace configuration instead.
/// See AgentRegistryShim for backward compatibility.
#[deprecated(
    since = "0.1.0",
    note = "Use CapabilityMarketplace configuration instead"
)]
#[allow(deprecated)]
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentRegistryConfig {
    /// Registry type (in_memory, database, etc.)
    pub registry_type: RegistryType,
    /// Database connection string (if applicable)
    pub database_url: Option<String>,
    /// Agent definitions
    pub agents: Vec<AgentDefinition>,
}

/// Registry types
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum RegistryType {
    /// In-memory registry
    InMemory,
    /// Database-backed registry
    Database,
    /// File-based registry
    File,
}

/// Agent definition
///
/// # Deprecation Notice
/// This struct is deprecated. Use CapabilityMarketplace with :kind :agent capabilities instead.
/// See AgentRegistryShim for backward compatibility.
#[deprecated(
    since = "0.1.0",
    note = "Use CapabilityMarketplace with :kind :agent capabilities instead"
)]
#[derive(Debug, Clone, Serialize, Deserialize)]
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

/// Capability configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CapabilityConfig {
    /// Whether to validate capabilities before plan execution
    pub validate_capabilities: bool,
    /// Whether to suggest alternatives for missing capabilities
    pub suggest_alternatives: bool,
    /// Default capabilities to include
    pub default_capabilities: Vec<String>,
    /// Capability marketplace configuration
    pub marketplace: CapabilityMarketplaceConfig,
}

/// Capability marketplace configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CapabilityMarketplaceConfig {
    /// Marketplace type
    pub marketplace_type: MarketplaceType,
    /// Discovery endpoints
    pub discovery_endpoints: Vec<String>,
    /// Cache configuration
    pub cache_config: CacheConfig,
}

/// Marketplace types
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum MarketplaceType {
    /// Local marketplace
    Local,
    /// Remote marketplace
    Remote,
    /// Hybrid (local + remote)
    Hybrid,
}

/// Cache configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CacheConfig {
    /// Whether caching is enabled
    pub enabled: bool,
    /// Cache TTL in seconds
    pub ttl_seconds: u64,
    /// Maximum cache size
    pub max_size: usize,
}

/// Security configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SecurityConfig {
    /// Whether to validate intents
    pub validate_intents: bool,
    /// Whether to validate plans
    pub validate_plans: bool,
    /// Maximum plan complexity
    pub max_plan_complexity: usize,
    /// Allowed capability prefixes
    pub allowed_capability_prefixes: Vec<String>,
    /// Blocked capability prefixes
    pub blocked_capability_prefixes: Vec<String>,
}

/// Template configuration for template-based engine
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TemplateConfig {
    /// Intent patterns
    pub intent_patterns: Vec<IntentPattern>,
    /// Plan templates
    pub plan_templates: Vec<PlanTemplate>,
    /// Fallback behavior
    pub fallback: FallbackBehavior,
}

/// Intent pattern for template matching
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IntentPattern {
    /// Pattern name
    pub name: String,
    /// Regex pattern to match
    pub pattern: String,
    /// Intent name to generate
    pub intent_name: String,
    /// Intent goal template
    pub goal_template: String,
    /// Intent constraints
    pub constraints: Vec<String>,
    /// Intent preferences
    pub preferences: Vec<String>,
}

/// Plan template
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlanTemplate {
    /// Template name
    pub name: String,
    /// RTFS plan template
    pub rtfs_template: String,
    /// Template variables
    pub variables: Vec<String>,
}

/// Fallback behavior
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum FallbackBehavior {
    /// Use default template
    Default,
    /// Use LLM fallback
    Llm,
    /// Return error
    Error,
}

impl Default for CognitiveEngineConfig {
    fn default() -> Self {
        Self {
            engine_type: CognitiveEngineType::Dummy,
            llm_config: None,
            delegation_config: None,
            capability_config: CapabilityConfig::default(),
            security_config: SecurityConfig::default(),
            template_config: Some(TemplateConfig::default()),
        }
    }
}

impl Default for CapabilityConfig {
    fn default() -> Self {
        Self {
            validate_capabilities: true,
            suggest_alternatives: true,
            default_capabilities: vec![":ccos.echo".to_string()],
            marketplace: CapabilityMarketplaceConfig::default(),
        }
    }
}

impl Default for CapabilityMarketplaceConfig {
    fn default() -> Self {
        Self {
            marketplace_type: MarketplaceType::Local,
            discovery_endpoints: vec![],
            cache_config: CacheConfig::default(),
        }
    }
}

impl Default for CacheConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            ttl_seconds: 3600, // 1 hour
            max_size: 1000,
        }
    }
}

impl Default for SecurityConfig {
    fn default() -> Self {
        Self {
            validate_intents: true,
            validate_plans: true,
            max_plan_complexity: 100,
            allowed_capability_prefixes: vec!["ccos.".to_string()],
            blocked_capability_prefixes: vec![],
        }
    }
}

impl Default for TemplateConfig {
    fn default() -> Self {
        Self {
            intent_patterns: vec![
                IntentPattern {
                    name: "sentiment_analysis".to_string(),
                    pattern: r"(?i)sentiment|analyze.*feeling|emotion".to_string(),
                    intent_name: "analyze_user_sentiment".to_string(),
                    goal_template: "Analyze sentiment from {input}".to_string(),
                    constraints: vec!["privacy".to_string()],
                    preferences: vec!["accuracy".to_string()],
                },
                IntentPattern {
                    name: "optimization".to_string(),
                    pattern: r"(?i)optimize|improve.*performance|speed.*up".to_string(),
                    intent_name: "optimize_response_time".to_string(),
                    goal_template: "Optimize performance for {input}".to_string(),
                    constraints: vec!["budget".to_string()],
                    preferences: vec!["speed".to_string()],
                },
            ],
            plan_templates: vec![
                PlanTemplate {
                    name: "sentiment_analysis".to_string(),
                    rtfs_template: r#"
(do
    (step "Fetch Data" (call :ccos.echo "fetched user interactions"))
    (step "Analyze Sentiment" (call :ccos.echo "sentiment: positive"))
    (step "Generate Report" (call :ccos.echo "report generated"))
)"#
                    .to_string(),
                    variables: vec![],
                },
                PlanTemplate {
                    name: "optimization".to_string(),
                    rtfs_template: r#"
(do
    (step "Get Metrics" (call :ccos.echo "metrics collected"))
    (step "Identify Bottlenecks" (call :ccos.echo "bottlenecks identified"))
    (step "Apply Optimizations" (call :ccos.echo "optimizations applied"))
)"#
                    .to_string(),
                    variables: vec![],
                },
            ],
            fallback: FallbackBehavior::Default,
        }
    }
}

impl CognitiveEngineConfig {
    /// Create a configuration from a TOML file
    pub fn from_file(path: &str) -> Result<Self, Box<dyn std::error::Error>> {
        let content = std::fs::read_to_string(path)?;
        let config: CognitiveEngineConfig = toml::from_str(&content)?;
        Ok(config)
    }

    /// Create a configuration from environment variables
    pub fn from_env() -> Result<Self, Box<dyn std::error::Error>> {
        let mut config = CognitiveEngineConfig::default();

        // Engine type
        if let Ok(engine_type) = std::env::var("CCOS_ARBITER_ENGINE_TYPE") {
            config.engine_type = match engine_type.as_str() {
                "template" => CognitiveEngineType::Template,
                "llm" => CognitiveEngineType::Llm,
                "delegating" => CognitiveEngineType::Delegating,
                "hybrid" => CognitiveEngineType::Hybrid,
                "dummy" => CognitiveEngineType::Dummy,
                _ => return Err("Invalid engine type".into()),
            };
        }

        // LLM configuration
        if let Ok(provider) = std::env::var("CCOS_LLM_PROVIDER") {
            let retry_config = RetryConfig {
                max_retries: std::env::var("CCOS_MAX_PLAN_RETRIES")
                    .ok()
                    .and_then(|s| s.parse().ok())
                    .unwrap_or(2),
                send_error_feedback: std::env::var("CCOS_PLAN_RETRY_FEEDBACK")
                    .map(|s| s == "true")
                    .unwrap_or(true),
                simplify_on_final_attempt: std::env::var("CCOS_PLAN_SIMPLIFY_FINAL")
                    .map(|s| s == "true")
                    .unwrap_or(true),
                use_stub_fallback: std::env::var("CCOS_PLAN_STUB_FALLBACK")
                    .map(|s| s == "true")
                    .unwrap_or(false),
            };

            let llm_config = LlmConfig {
                provider_type: match provider.as_str() {
                    "openai" | "openrouter" => {
                        // OpenRouter uses OpenAI-compatible API with different base URL
                        // Set base URL if not already set and provider is openrouter
                        if provider == "openrouter" && std::env::var("CCOS_LLM_BASE_URL").is_err() {
                            std::env::set_var("CCOS_LLM_BASE_URL", "https://openrouter.ai/api/v1");
                        }
                        LlmProviderType::OpenAI
                    },
                    "anthropic" => LlmProviderType::Anthropic,
                    "stub" => {
                        // Warn about stub usage
                        eprintln!("⚠️  WARNING: Using stub LLM provider (testing only)");
                        eprintln!("   For production, use: CCOS_LLM_PROVIDER=openai or CCOS_LLM_PROVIDER=anthropic");
                        LlmProviderType::Stub
                    },
                    "local" => LlmProviderType::Local,
                    _ => return Err("Invalid LLM provider. Use: openai, openrouter, anthropic, or stub (testing only)".into()),
                },
                model: std::env::var("CCOS_LLM_MODEL").unwrap_or_else(|_| "stub-model".to_string()),
                api_key: std::env::var("CCOS_LLM_API_KEY").ok(),
                base_url: std::env::var("CCOS_LLM_BASE_URL").ok(),
                max_tokens: std::env::var("CCOS_LLM_MAX_TOKENS")
                    .ok()
                    .and_then(|s| s.parse().ok()),
                temperature: std::env::var("CCOS_LLM_TEMPERATURE")
                    .ok()
                    .and_then(|s| s.parse().ok()),
                timeout_seconds: std::env::var("CCOS_LLM_TIMEOUT")
                    .ok()
                    .and_then(|s| s.parse().ok()),
                prompts: None,
                retry_config,
            };
            config.llm_config = Some(llm_config);
        }

        // Delegation configuration
        if let Ok(enabled) = std::env::var("CCOS_DELEGATION_ENABLED") {
            if enabled == "true" {
                let delegation_config = DelegationConfig {
                    enabled: true,
                    threshold: std::env::var("CCOS_DELEGATION_THRESHOLD")
                        .unwrap_or_else(|_| "0.65".to_string())
                        .parse()
                        .unwrap_or(0.65),
                    max_candidates: std::env::var("CCOS_DELEGATION_MAX_CANDIDATES")
                        .unwrap_or_else(|_| "3".to_string())
                        .parse()
                        .unwrap_or(3),
                    min_skill_hits: std::env::var("CCOS_DELEGATION_MIN_SKILL_HITS")
                        .ok()
                        .and_then(|s| s.parse().ok()),
                    agent_registry: None,
                    adaptive_threshold: None,
                    print_extracted_intent: None,
                    print_extracted_plan: None,
                };
                config.delegation_config = Some(delegation_config);
            }
        }

        Ok(config)
    }

    /// Validate the configuration
    pub fn validate(&self) -> Result<(), Vec<String>> {
        let mut errors = Vec::new();

        // Validate engine type and required configs
        match self.engine_type {
            CognitiveEngineType::Llm => {
                if self.llm_config.is_none() {
                    errors.push("LLM engine requires llm_config".to_string());
                }
            }
            CognitiveEngineType::Delegating => {
                if self.llm_config.is_none() {
                    errors.push("Delegating engine requires llm_config".to_string());
                }
                if self.delegation_config.is_none() {
                    errors.push("Delegating engine requires delegation_config".to_string());
                }
            }
            CognitiveEngineType::Hybrid => {
                if self.llm_config.is_none() {
                    errors.push("Hybrid engine requires llm_config".to_string());
                }
                if self.template_config.is_none() {
                    errors.push("Hybrid engine requires template_config".to_string());
                }
            }
            CognitiveEngineType::Template => {
                if self.template_config.is_none() {
                    errors.push("Template engine requires template_config".to_string());
                }
            }
            CognitiveEngineType::Dummy => {
                // No additional config required
            }
        }

        // Validate LLM config if present
        if let Some(llm_config) = &self.llm_config {
            if let Some(temp) = llm_config.temperature {
                if temp < 0.0 || temp > 1.0 {
                    errors.push("LLM temperature must be between 0.0 and 1.0".to_string());
                }
            }
            if let Some(tokens) = llm_config.max_tokens {
                if tokens == 0 {
                    errors.push("LLM max_tokens must be greater than 0".to_string());
                }
            }
        }

        // Validate delegation config if present
        if let Some(delegation_config) = &self.delegation_config {
            if delegation_config.threshold < 0.0 || delegation_config.threshold > 1.0 {
                errors.push("Delegation threshold must be between 0.0 and 1.0".to_string());
            }
            if delegation_config.max_candidates == 0 {
                errors.push("Delegation max_candidates must be greater than 0".to_string());
            }
        }

        if errors.is_empty() {
            Ok(())
        } else {
            Err(errors)
        }
    }
}

#[allow(deprecated)]
impl Default for AgentRegistryConfig {
    fn default() -> Self {
        Self {
            registry_type: RegistryType::InMemory,
            database_url: None,
            agents: vec![],
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_config() {
        let config = CognitiveEngineConfig::default();
        assert!(config.validate().is_ok());
    }

    #[test]
    fn test_llm_config_validation() {
        // Valid config
        let valid_config = LlmConfig {
            provider_type: LlmProviderType::Stub,
            model: "test-model".to_string(),
            api_key: None,
            base_url: None,
            max_tokens: Some(1000),
            temperature: Some(0.5),
            timeout_seconds: Some(30),
            prompts: None,
            retry_config: RetryConfig::default(),
        };
        assert_eq!(valid_config.provider_type, LlmProviderType::Stub);
        assert_eq!(valid_config.model, "test-model");

        // Invalid config - empty model
        let invalid_config = LlmConfig {
            provider_type: LlmProviderType::OpenAI,
            model: "".to_string(), // Empty model
            api_key: None,
            base_url: None,
            max_tokens: Some(1000),
            temperature: Some(1.5), // Invalid temperature
            timeout_seconds: Some(30),
            prompts: None,
            retry_config: RetryConfig::default(),
        };
        assert_eq!(invalid_config.model, "");
    }

    #[test]
    fn test_llm_config_creation() {
        let config = LlmConfig {
            provider_type: LlmProviderType::Stub,
            model: "test-model".to_string(),
            api_key: None,
            base_url: None,
            max_tokens: Some(1000),
            temperature: Some(0.7),
            timeout_seconds: Some(60),
            prompts: None,
            retry_config: RetryConfig::default(),
        };

        assert_eq!(config.provider_type, LlmProviderType::Stub);
        assert_eq!(config.model, "test-model");
        assert_eq!(config.max_tokens, Some(1000));
        assert_eq!(config.temperature, Some(0.7));
        assert_eq!(config.timeout_seconds, Some(60));
    }
}
