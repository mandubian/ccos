use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Configuration for the Arbiter system
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ArbiterConfig {
    /// The type of arbiter engine to use
    pub engine_type: ArbiterEngineType,
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

/// Types of arbiter engines
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ArbiterEngineType {
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

/// LLM provider configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LlmConfig {
    /// Provider type (openai, anthropic, stub, etc.)
    pub provider: LlmProviderType,
    /// Model name/identifier
    pub model: String,
    /// API key (can be loaded from env)
    pub api_key: Option<String>,
    /// Maximum tokens for context window
    pub max_tokens: usize,
    /// Temperature for generation (0.0 = deterministic, 1.0 = creative)
    pub temperature: f32,
    /// System prompt template
    pub system_prompt: Option<String>,
    /// Intent generation prompt template
    pub intent_prompt_template: Option<String>,
    /// Plan generation prompt template
    pub plan_prompt_template: Option<String>,
}

/// LLM provider types
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum LlmProviderType {
    /// OpenAI API
    OpenAI,
    /// Anthropic API
    Anthropic,
    /// Stub provider for testing
    Stub,
    /// Echo provider for deterministic testing
    Echo,
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
    /// Agent registry configuration
    pub agent_registry: AgentRegistryConfig,
}

/// Agent registry configuration
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

impl Default for ArbiterConfig {
    fn default() -> Self {
        Self {
            engine_type: ArbiterEngineType::Dummy,
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
)"#.to_string(),
                    variables: vec![],
                },
                PlanTemplate {
                    name: "optimization".to_string(),
                    rtfs_template: r#"
(do
    (step "Get Metrics" (call :ccos.echo "metrics collected"))
    (step "Identify Bottlenecks" (call :ccos.echo "bottlenecks identified"))
    (step "Apply Optimizations" (call :ccos.echo "optimizations applied"))
)"#.to_string(),
                    variables: vec![],
                },
            ],
            fallback: FallbackBehavior::Default,
        }
    }
}

impl ArbiterConfig {
    /// Create a configuration from a TOML file
    pub fn from_file(path: &str) -> Result<Self, Box<dyn std::error::Error>> {
        let content = std::fs::read_to_string(path)?;
        let config: ArbiterConfig = toml::from_str(&content)?;
        Ok(config)
    }

    /// Create a configuration from environment variables
    pub fn from_env() -> Result<Self, Box<dyn std::error::Error>> {
        let mut config = ArbiterConfig::default();
        
        // Engine type
        if let Ok(engine_type) = std::env::var("CCOS_ARBITER_ENGINE_TYPE") {
            config.engine_type = match engine_type.as_str() {
                "template" => ArbiterEngineType::Template,
                "llm" => ArbiterEngineType::Llm,
                "delegating" => ArbiterEngineType::Delegating,
                "hybrid" => ArbiterEngineType::Hybrid,
                "dummy" => ArbiterEngineType::Dummy,
                _ => return Err("Invalid engine type".into()),
            };
        }

        // LLM configuration
        if let Ok(provider) = std::env::var("CCOS_LLM_PROVIDER") {
            let llm_config = LlmConfig {
                provider: match provider.as_str() {
                    "openai" => LlmProviderType::OpenAI,
                    "anthropic" => LlmProviderType::Anthropic,
                    "stub" => LlmProviderType::Stub,
                    "echo" => LlmProviderType::Echo,
                    _ => return Err("Invalid LLM provider".into()),
                },
                model: std::env::var("CCOS_LLM_MODEL").unwrap_or_else(|_| "gpt-4".to_string()),
                api_key: std::env::var("CCOS_LLM_API_KEY").ok(),
                max_tokens: std::env::var("CCOS_LLM_MAX_TOKENS")
                    .unwrap_or_else(|_| "4096".to_string())
                    .parse()
                    .unwrap_or(4096),
                temperature: std::env::var("CCOS_LLM_TEMPERATURE")
                    .unwrap_or_else(|_| "0.7".to_string())
                    .parse()
                    .unwrap_or(0.7),
                system_prompt: std::env::var("CCOS_LLM_SYSTEM_PROMPT").ok(),
                intent_prompt_template: std::env::var("CCOS_LLM_INTENT_PROMPT").ok(),
                plan_prompt_template: std::env::var("CCOS_LLM_PLAN_PROMPT").ok(),
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
                    agent_registry: AgentRegistryConfig::default(),
                    print_extracted_intent: None,
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
            ArbiterEngineType::Llm => {
                if self.llm_config.is_none() {
                    errors.push("LLM engine requires llm_config".to_string());
                }
            }
            ArbiterEngineType::Delegating => {
                if self.llm_config.is_none() {
                    errors.push("Delegating engine requires llm_config".to_string());
                }
                if self.delegation_config.is_none() {
                    errors.push("Delegating engine requires delegation_config".to_string());
                }
            }
            ArbiterEngineType::Hybrid => {
                if self.llm_config.is_none() {
                    errors.push("Hybrid engine requires llm_config".to_string());
                }
                if self.template_config.is_none() {
                    errors.push("Hybrid engine requires template_config".to_string());
                }
            }
            ArbiterEngineType::Template => {
                if self.template_config.is_none() {
                    errors.push("Template engine requires template_config".to_string());
                }
            }
            ArbiterEngineType::Dummy => {
                // No additional config required
            }
        }

        // Validate LLM config if present
        if let Some(llm_config) = &self.llm_config {
            if llm_config.temperature < 0.0 || llm_config.temperature > 1.0 {
                errors.push("LLM temperature must be between 0.0 and 1.0".to_string());
            }
            if llm_config.max_tokens == 0 {
                errors.push("LLM max_tokens must be greater than 0".to_string());
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
        let config = ArbiterConfig::default();
        assert!(config.validate().is_ok());
    }

    #[test]
    fn test_llm_config_validation() {
        let mut config = ArbiterConfig::default();
        config.engine_type = ArbiterEngineType::Llm;
        
        // Should fail without llm_config
        assert!(config.validate().is_err());
        
        // Should pass with valid llm_config
        config.llm_config = Some(LlmConfig {
            provider: LlmProviderType::Stub,
            model: "test-model".to_string(),
            api_key: None,
            max_tokens: 1000,
            temperature: 0.5,
            system_prompt: None,
            intent_prompt_template: None,
            plan_prompt_template: None,
        });
        
        assert!(config.validate().is_ok());
    }

    #[test]
    fn test_invalid_temperature() {
        let mut config = ArbiterConfig::default();
        config.llm_config = Some(LlmConfig {
            provider: LlmProviderType::Stub,
            model: "test-model".to_string(),
            api_key: None,
            max_tokens: 1000,
            temperature: 1.5, // Invalid
            system_prompt: None,
            intent_prompt_template: None,
            plan_prompt_template: None,
        });
        
        let errors = config.validate().unwrap_err();
        assert!(errors.iter().any(|e| e.contains("temperature")));
    }
}
