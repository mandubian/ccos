//! Feature flags and configuration for missing capability resolution system
//!
//! This module provides feature flags and configuration options to control
//! the behavior of the missing capability resolution system in production.

use serde::{Deserialize, Serialize};

/// Feature flags for missing capability resolution
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MissingCapabilityFeatureFlags {
    /// Enable/disable the entire missing capability resolution system
    pub enabled: bool,

    /// Enable runtime detection of missing capabilities
    pub runtime_detection: bool,

    /// Enable automatic resolution attempts
    pub auto_resolution: bool,

    /// Enable MCP Registry integration
    pub mcp_registry_enabled: bool,

    /// Enable OpenAPI/GraphQL importers
    pub importers_enabled: bool,

    /// Enable HTTP/JSON generic wrapper
    pub http_wrapper_enabled: bool,

    /// Enable LLM synthesis with guardrails
    pub llm_synthesis_enabled: bool,

    /// Enable web search discovery fallback
    pub web_search_enabled: bool,

    /// Enable continuous resolution loop
    pub continuous_resolution: bool,

    /// Enable auto-resume of paused executions
    pub auto_resume_enabled: bool,

    /// Enable human-in-the-loop approvals for high-risk resolutions
    pub human_approval_required: bool,

    /// Enable audit logging for all resolution activities
    pub audit_logging_enabled: bool,

    /// Enable validation harness and governance policies
    pub validation_enabled: bool,

    /// Enable CLI tooling and observability features
    pub cli_tooling_enabled: bool,
}

impl Default for MissingCapabilityFeatureFlags {
    fn default() -> Self {
        Self {
            // Conservative defaults for production
            enabled: false,                // Disabled by default
            runtime_detection: false,      // Disabled by default
            auto_resolution: false,        // Disabled by default
            mcp_registry_enabled: true,    // Safe to enable
            importers_enabled: false,      // Disabled by default (requires review)
            http_wrapper_enabled: false,   // Disabled by default (security risk)
            llm_synthesis_enabled: false,  // Disabled by default (requires review)
            web_search_enabled: false,     // Disabled by default (external dependency)
            continuous_resolution: false,  // Disabled by default
            auto_resume_enabled: false,    // Disabled by default
            human_approval_required: true, // Always require human approval
            audit_logging_enabled: true,   // Always enable audit logging
            validation_enabled: true,      // Always enable validation
            cli_tooling_enabled: true,     // Safe to enable
        }
    }
}

/// Development feature flags (more permissive)
impl MissingCapabilityFeatureFlags {
    pub fn development() -> Self {
        Self {
            enabled: true,
            runtime_detection: true,
            auto_resolution: true,
            mcp_registry_enabled: true,
            importers_enabled: true,
            http_wrapper_enabled: true,
            llm_synthesis_enabled: true,
            web_search_enabled: true,
            continuous_resolution: true,
            auto_resume_enabled: true,
            human_approval_required: false, // Skip human approval in dev
            audit_logging_enabled: true,
            validation_enabled: true,
            cli_tooling_enabled: true,
        }
    }

    /// Testing feature flags (minimal features)
    pub fn testing() -> Self {
        Self {
            enabled: true,
            runtime_detection: true,
            auto_resolution: false,         // Disable auto-resolution in tests
            mcp_registry_enabled: false,    // Use mock data in tests
            importers_enabled: false,       // Disable external dependencies
            http_wrapper_enabled: false,    // Disable external dependencies
            llm_synthesis_enabled: false,   // Disable LLM calls in tests
            web_search_enabled: false,      // Disable external dependencies
            continuous_resolution: false,   // Disable background processes
            auto_resume_enabled: false,     // Disable auto-resume in tests
            human_approval_required: false, // Skip human approval in tests
            audit_logging_enabled: true,    // Enable audit logging for tests
            validation_enabled: true,       // Enable validation for tests
            cli_tooling_enabled: true,      // Enable CLI for tests
        }
    }
}

/// Configuration options for missing capability resolution
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MissingCapabilityConfig {
    /// Feature flags
    pub feature_flags: MissingCapabilityFeatureFlags,

    /// Maximum number of resolution attempts per capability
    pub max_resolution_attempts: u32,

    /// Timeout for resolution attempts (seconds)
    pub resolution_timeout_seconds: u64,

    /// Maximum number of concurrent resolution attempts
    pub max_concurrent_resolutions: u32,

    /// Backoff configuration for retry attempts
    pub retry_backoff_config: RetryBackoffConfig,

    /// Human approval timeout (seconds)
    pub human_approval_timeout_seconds: u64,

    /// Maximum number of pending capabilities in queue
    pub max_pending_capabilities: u32,

    /// Security and governance settings
    pub security_config: SecurityConfig,

    /// MCP Registry configuration
    pub mcp_registry_config: McpRegistryConfig,

    /// LLM synthesis configuration
    pub llm_synthesis_config: LlmSynthesisConfig,

    /// Web search configuration
    pub web_search_config: WebSearchConfig,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RetryBackoffConfig {
    pub initial_delay_seconds: u64,
    pub max_delay_seconds: u64,
    pub backoff_multiplier: f64,
    pub jitter_enabled: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SecurityConfig {
    /// Require HTTPS for external API calls
    pub require_https: bool,

    /// Allowed domains for external API calls
    pub allowed_domains: Vec<String>,

    /// Blocked domains for external API calls
    pub blocked_domains: Vec<String>,

    /// Maximum request size (bytes)
    pub max_request_size_bytes: u64,

    /// Maximum response size (bytes)
    pub max_response_size_bytes: u64,

    /// Require authentication for external API calls
    pub require_auth: bool,

    /// Maximum execution time for synthesized capabilities (seconds)
    pub max_execution_time_seconds: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct McpRegistryConfig {
    /// MCP Registry API base URL
    pub base_url: String,

    /// API timeout (seconds)
    pub timeout_seconds: u64,

    /// Maximum number of servers to fetch
    pub max_servers: u32,

    /// Cache TTL for registry data (seconds)
    pub cache_ttl_seconds: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LlmSynthesisConfig {
    /// Maximum number of tokens for synthesis prompts
    pub max_tokens: u32,

    /// Temperature for LLM synthesis
    pub temperature: f64,

    /// Maximum number of synthesis attempts
    pub max_attempts: u32,

    /// Require human approval for synthesized capabilities
    pub require_human_approval: bool,

    /// Allowed capability types for synthesis
    pub allowed_capability_types: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WebSearchConfig {
    /// Enable web search discovery
    pub enabled: bool,

    /// Maximum number of search results
    pub max_results: u32,

    /// Search timeout (seconds)
    pub timeout_seconds: u64,

    /// Allowed search engines
    pub allowed_search_engines: Vec<String>,
}

impl Default for MissingCapabilityConfig {
    fn default() -> Self {
        Self {
            feature_flags: MissingCapabilityFeatureFlags::default(),
            max_resolution_attempts: 3,
            resolution_timeout_seconds: 30,
            max_concurrent_resolutions: 5,
            retry_backoff_config: RetryBackoffConfig {
                initial_delay_seconds: 1,
                max_delay_seconds: 60,
                backoff_multiplier: 2.0,
                jitter_enabled: true,
            },
            human_approval_timeout_seconds: 3600, // 1 hour
            max_pending_capabilities: 100,
            security_config: SecurityConfig {
                require_https: true,
                allowed_domains: vec![
                    "registry.modelcontextprotocol.io".to_string(),
                    "api.github.com".to_string(),
                    "api.openai.com".to_string(),
                ],
                blocked_domains: vec![],
                max_request_size_bytes: 10 * 1024 * 1024, // 10MB
                max_response_size_bytes: 50 * 1024 * 1024, // 50MB
                require_auth: true,
                max_execution_time_seconds: 300, // 5 minutes
            },
            mcp_registry_config: McpRegistryConfig {
                base_url: "https://registry.modelcontextprotocol.io".to_string(),
                timeout_seconds: 10,
                max_servers: 100,
                cache_ttl_seconds: 3600, // 1 hour
            },
            llm_synthesis_config: LlmSynthesisConfig {
                max_tokens: 4000,
                temperature: 0.1, // Low temperature for consistent results
                max_attempts: 3,
                require_human_approval: true,
                allowed_capability_types: vec![
                    "utility".to_string(),
                    "data_processing".to_string(),
                    "format_conversion".to_string(),
                ],
            },
            web_search_config: WebSearchConfig {
                enabled: false, // Disabled by default
                max_results: 10,
                timeout_seconds: 15,
                allowed_search_engines: vec!["duckduckgo".to_string(), "bing".to_string()],
            },
        }
    }
}

/// Environment-based configuration loader
impl MissingCapabilityConfig {
    /// Load configuration from environment variables
    pub fn from_env() -> Self {
        let mut config = Self::default();

        // Load feature flags from environment
        if let Ok(enabled) = std::env::var("CCOS_MISSING_CAPABILITY_ENABLED") {
            config.feature_flags.enabled = enabled.parse().unwrap_or(false);
        }

        if let Ok(auto_resolution) = std::env::var("CCOS_AUTO_RESOLUTION_ENABLED") {
            config.feature_flags.auto_resolution = auto_resolution.parse().unwrap_or(false);
        }

        if let Ok(mcp_registry) = std::env::var("CCOS_MCP_REGISTRY_ENABLED") {
            config.feature_flags.mcp_registry_enabled = mcp_registry.parse().unwrap_or(true);
        }

        if let Ok(llm_synthesis) = std::env::var("CCOS_LLM_SYNTHESIS_ENABLED") {
            config.feature_flags.llm_synthesis_enabled = llm_synthesis.parse().unwrap_or(false);
        }

        if let Ok(web_search) = std::env::var("CCOS_WEB_SEARCH_ENABLED") {
            config.feature_flags.web_search_enabled = web_search.parse().unwrap_or(false);
        }

        if let Ok(human_approval) = std::env::var("CCOS_HUMAN_APPROVAL_REQUIRED") {
            config.feature_flags.human_approval_required = human_approval.parse().unwrap_or(true);
        }

        // Load configuration values from environment
        if let Ok(max_attempts) = std::env::var("CCOS_MAX_RESOLUTION_ATTEMPTS") {
            config.max_resolution_attempts = max_attempts.parse().unwrap_or(3);
        }

        if let Ok(timeout) = std::env::var("CCOS_RESOLUTION_TIMEOUT_SECONDS") {
            config.resolution_timeout_seconds = timeout.parse().unwrap_or(30);
        }

        if let Ok(max_concurrent) = std::env::var("CCOS_MAX_CONCURRENT_RESOLUTIONS") {
            config.max_concurrent_resolutions = max_concurrent.parse().unwrap_or(5);
        }

        if let Ok(approval_timeout) = std::env::var("CCOS_HUMAN_APPROVAL_TIMEOUT_SECONDS") {
            config.human_approval_timeout_seconds = approval_timeout.parse().unwrap_or(3600);
        }

        // Load security configuration
        if let Ok(require_https) = std::env::var("CCOS_REQUIRE_HTTPS") {
            config.security_config.require_https = require_https.parse().unwrap_or(true);
        }

        if let Ok(allowed_domains) = std::env::var("CCOS_ALLOWED_DOMAINS") {
            config.security_config.allowed_domains = allowed_domains
                .split(',')
                .map(|s| s.trim().to_string())
                .collect();
        }

        if let Ok(blocked_domains) = std::env::var("CCOS_BLOCKED_DOMAINS") {
            config.security_config.blocked_domains = blocked_domains
                .split(',')
                .map(|s| s.trim().to_string())
                .collect();
        }

        // Load MCP Registry configuration
        if let Ok(base_url) = std::env::var("CCOS_MCP_REGISTRY_BASE_URL") {
            config.mcp_registry_config.base_url = base_url;
        }

        if let Ok(timeout) = std::env::var("CCOS_MCP_REGISTRY_TIMEOUT_SECONDS") {
            config.mcp_registry_config.timeout_seconds = timeout.parse().unwrap_or(10);
        }

        config
    }

    /// Validate configuration for consistency and security
    pub fn validate(&self) -> Result<(), String> {
        // Validate feature flags
        if self.feature_flags.enabled && !self.feature_flags.runtime_detection {
            return Err(
                "Runtime detection must be enabled when missing capability resolution is enabled"
                    .to_string(),
            );
        }

        if self.feature_flags.auto_resolution && !self.feature_flags.enabled {
            return Err(
                "Auto resolution requires missing capability resolution to be enabled".to_string(),
            );
        }

        if self.feature_flags.auto_resume_enabled && !self.feature_flags.auto_resolution {
            return Err("Auto resume requires auto resolution to be enabled".to_string());
        }

        // Validate configuration values
        if self.max_resolution_attempts == 0 {
            return Err("Max resolution attempts must be greater than 0".to_string());
        }

        if self.resolution_timeout_seconds == 0 {
            return Err("Resolution timeout must be greater than 0".to_string());
        }

        if self.max_concurrent_resolutions == 0 {
            return Err("Max concurrent resolutions must be greater than 0".to_string());
        }

        if self.max_concurrent_resolutions > 100 {
            return Err("Max concurrent resolutions should not exceed 100".to_string());
        }

        // Validate security configuration
        if self.security_config.require_https && self.security_config.allowed_domains.is_empty() {
            return Err(
                "At least one allowed domain must be specified when HTTPS is required".to_string(),
            );
        }

        if self.security_config.max_request_size_bytes == 0 {
            return Err("Max request size must be greater than 0".to_string());
        }

        if self.security_config.max_response_size_bytes == 0 {
            return Err("Max response size must be greater than 0".to_string());
        }

        // Validate MCP Registry configuration
        if self.feature_flags.mcp_registry_enabled {
            if self.mcp_registry_config.base_url.is_empty() {
                return Err("MCP Registry base URL cannot be empty".to_string());
            }

            if !self.mcp_registry_config.base_url.starts_with("https://") {
                return Err("MCP Registry base URL must use HTTPS".to_string());
            }
        }

        // Validate LLM synthesis configuration
        if self.feature_flags.llm_synthesis_enabled {
            if self.llm_synthesis_config.max_tokens == 0 {
                return Err("LLM synthesis max tokens must be greater than 0".to_string());
            }

            if self.llm_synthesis_config.temperature < 0.0
                || self.llm_synthesis_config.temperature > 2.0
            {
                return Err("LLM synthesis temperature must be between 0.0 and 2.0".to_string());
            }
        }

        Ok(())
    }

    /// Create a configuration suitable for testing with features enabled
    pub fn testing() -> Self {
        let mut config = Self::default();
        config.feature_flags = MissingCapabilityFeatureFlags::testing();
        config
    }
}

/// Feature flag checker utility
pub struct FeatureFlagChecker {
    config: MissingCapabilityConfig,
}

impl FeatureFlagChecker {
    pub fn new(config: MissingCapabilityConfig) -> Self {
        Self { config }
    }

    pub fn is_enabled(&self) -> bool {
        self.config.feature_flags.enabled
    }

    pub fn is_runtime_detection_enabled(&self) -> bool {
        self.config.feature_flags.enabled && self.config.feature_flags.runtime_detection
    }

    pub fn is_auto_resolution_enabled(&self) -> bool {
        self.config.feature_flags.enabled && self.config.feature_flags.auto_resolution
    }

    pub fn is_mcp_registry_enabled(&self) -> bool {
        self.config.feature_flags.enabled && self.config.feature_flags.mcp_registry_enabled
    }

    pub fn is_importers_enabled(&self) -> bool {
        self.config.feature_flags.enabled && self.config.feature_flags.importers_enabled
    }

    pub fn is_http_wrapper_enabled(&self) -> bool {
        self.config.feature_flags.enabled && self.config.feature_flags.http_wrapper_enabled
    }

    pub fn is_llm_synthesis_enabled(&self) -> bool {
        self.config.feature_flags.enabled && self.config.feature_flags.llm_synthesis_enabled
    }

    pub fn is_web_search_enabled(&self) -> bool {
        self.config.feature_flags.enabled && self.config.feature_flags.web_search_enabled
    }

    pub fn is_continuous_resolution_enabled(&self) -> bool {
        self.config.feature_flags.enabled && self.config.feature_flags.continuous_resolution
    }

    pub fn is_auto_resume_enabled(&self) -> bool {
        self.config.feature_flags.enabled && self.config.feature_flags.auto_resume_enabled
    }

    pub fn is_human_approval_required(&self) -> bool {
        self.config.feature_flags.human_approval_required
    }

    pub fn is_audit_logging_enabled(&self) -> bool {
        self.config.feature_flags.audit_logging_enabled
    }

    pub fn is_validation_enabled(&self) -> bool {
        self.config.feature_flags.validation_enabled
    }

    pub fn is_cli_tooling_enabled(&self) -> bool {
        self.config.feature_flags.cli_tooling_enabled
    }

    pub fn get_config(&self) -> &MissingCapabilityConfig {
        &self.config
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_config() {
        let config = MissingCapabilityConfig::default();
        assert!(!config.feature_flags.enabled);
        assert!(!config.feature_flags.auto_resolution);
        assert!(config.feature_flags.mcp_registry_enabled);
        assert!(!config.feature_flags.llm_synthesis_enabled);
        assert!(config.feature_flags.human_approval_required);
    }

    #[test]
    fn test_development_config() {
        let config = MissingCapabilityConfig {
            feature_flags: MissingCapabilityFeatureFlags::development(),
            ..Default::default()
        };
        assert!(config.feature_flags.enabled);
        assert!(config.feature_flags.auto_resolution);
        assert!(config.feature_flags.llm_synthesis_enabled);
        assert!(!config.feature_flags.human_approval_required);
    }

    #[test]
    fn test_testing_config() {
        let config = MissingCapabilityConfig {
            feature_flags: MissingCapabilityFeatureFlags::testing(),
            ..Default::default()
        };
        assert!(config.feature_flags.enabled);
        assert!(!config.feature_flags.auto_resolution);
        assert!(!config.feature_flags.mcp_registry_enabled);
        assert!(!config.feature_flags.llm_synthesis_enabled);
    }

    #[test]
    fn test_config_validation() {
        let mut config = MissingCapabilityConfig::default();
        assert!(config.validate().is_ok());

        // Test invalid configuration
        config.feature_flags.enabled = true;
        config.feature_flags.runtime_detection = false;
        assert!(config.validate().is_err());

        // Test valid configuration
        config.feature_flags.runtime_detection = true;
        assert!(config.validate().is_ok());
    }

    #[test]
    fn test_feature_flag_checker() {
        let config = MissingCapabilityConfig {
            feature_flags: MissingCapabilityFeatureFlags::development(),
            ..Default::default()
        };
        let checker = FeatureFlagChecker::new(config);

        assert!(checker.is_enabled());
        assert!(checker.is_runtime_detection_enabled());
        assert!(checker.is_auto_resolution_enabled());
        assert!(checker.is_mcp_registry_enabled());
        assert!(checker.is_llm_synthesis_enabled());
        assert!(!checker.is_human_approval_required());
    }
}
