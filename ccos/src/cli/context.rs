//! CLI context - shared state and services for all commands

use crate::arbiter::llm_provider::{
    LlmProvider, LlmProviderConfig, LlmProviderFactory, LlmProviderType,
};
use crate::capability_marketplace::CapabilityMarketplace;
use crate::mcp::core::MCPDiscoveryService;
use crate::config::types::AgentConfig;
use rtfs::runtime::error::{RuntimeError, RuntimeResult};
use std::path::PathBuf;
use std::sync::Arc;

/// Shared context for CLI commands
pub struct CliContext {
    /// Path to the configuration file
    pub config_path: PathBuf,
    /// Loaded agent configuration
    pub config: AgentConfig,
    /// Output format preference
    pub output_format: super::OutputFormat,
    /// Quiet mode (suppress status messages)
    pub quiet: bool,
    /// Verbose mode (extra debug output)
    pub verbose: bool,
    /// Capability marketplace (lazy initialized)
    marketplace: Option<Arc<CapabilityMarketplace>>,
    /// MCP discovery service (lazy initialized)
    discovery_service: Option<Arc<MCPDiscoveryService>>,
}

impl CliContext {
    /// Create a new CLI context from configuration path
    pub fn new(config_path: PathBuf) -> RuntimeResult<Self> {
        let config = Self::load_config(&config_path)?;
        Ok(Self {
            config_path,
            config,
            output_format: super::OutputFormat::Table,
            quiet: false,
            verbose: false,
            marketplace: None,
            discovery_service: None,
        })
    }

    /// Create context with default configuration
    pub fn with_defaults() -> RuntimeResult<Self> {
        let default_paths = [
            PathBuf::from("../config/agent_config.toml"),
            PathBuf::from("config/agent_config.toml"),
            PathBuf::from("agent_config.toml"),
        ];

        for path in &default_paths {
            if path.exists() {
                return Self::new(path.clone());
            }
        }

        // No config found, use empty defaults
        Ok(Self {
            config_path: PathBuf::from("agent_config.toml"),
            config: AgentConfig::default(),
            output_format: super::OutputFormat::Table,
            quiet: false,
            verbose: false,
            marketplace: None,
            discovery_service: None,
        })
    }

    /// Load configuration from file
    fn load_config(path: &PathBuf) -> RuntimeResult<AgentConfig> {
        if !path.exists() {
            return Ok(AgentConfig::default());
        }

        let content = std::fs::read_to_string(path).map_err(|e| {
            RuntimeError::Generic(format!("Failed to read config file {:?}: {}", path, e))
        })?;

        toml::from_str(&content).map_err(|e| {
            RuntimeError::Generic(format!("Failed to parse config file {:?}: {}", path, e))
        })
    }

    /// Validate the current configuration
    pub fn validate_config(&self) -> RuntimeResult<Vec<String>> {
        let mut warnings = Vec::new();

        // Check LLM profiles if needed
        if let Some(ref profiles) = self.config.llm_profiles {
            if profiles.profiles.is_empty()
                && profiles.model_sets.as_ref().map_or(true, |s| s.is_empty())
            {
                warnings.push("No LLM profiles or model sets configured".to_string());
            }
        } else {
            warnings.push("No LLM profiles section configured".to_string());
        }

        // Check governance settings
        if self.config.governance.policies.is_empty() {
            warnings.push("No governance policies configured".to_string());
        }

        Ok(warnings)
    }

    /// Get or initialize the capability marketplace
    /// Note: Requires CapabilityRegistry - for now returns error until fully integrated
    pub async fn marketplace(&mut self) -> RuntimeResult<Arc<CapabilityMarketplace>> {
        if let Some(ref mp) = self.marketplace {
            return Ok(Arc::clone(mp));
        }

        // TODO: Need to properly initialize with CapabilityRegistry
        // For now, return error - this will be implemented when we add capability commands
        Err(RuntimeError::Generic(
            "CapabilityMarketplace initialization not yet implemented in CLI context".to_string(),
        ))
    }

    /// Get or initialize the MCP discovery service
    pub async fn discovery_service(&mut self) -> RuntimeResult<Arc<MCPDiscoveryService>> {
        if let Some(ref ds) = self.discovery_service {
            return Ok(Arc::clone(ds));
        }

        let ds = Arc::new(MCPDiscoveryService::new());
        self.discovery_service = Some(Arc::clone(&ds));
        Ok(ds)
    }

    /// Print status message (respects quiet mode)
    pub fn status(&self, message: &str) {
        if !self.quiet {
            eprintln!("{}", message);
        }
    }

    /// Print verbose message (only in verbose mode)
    pub fn debug(&self, message: &str) {
        if self.verbose {
            eprintln!("[DEBUG] {}", message);
        }
    }

    /// Resolve a path relative to the workspace root
    /// Uses CCOS_WORKSPACE_ROOT env var if set, otherwise uses config file parent directory
    pub fn resolve_workspace_path(&self, relative_path: &str) -> PathBuf {
        let workspace_root = std::env::var("CCOS_WORKSPACE_ROOT")
            .map(PathBuf::from)
            .unwrap_or_else(|_| {
                self.config_path
                    .parent()
                    .map(|p| p.to_path_buf())
                    .unwrap_or_else(|| PathBuf::from("."))
            });
        workspace_root.join(relative_path)
    }

    /// Create an LLM provider from the agent configuration
    /// Uses the default profile from llm_profiles (supports both explicit profiles and model_sets)
    pub async fn create_llm_provider(
        &self,
        model_override: Option<String>,
    ) -> RuntimeResult<Arc<dyn LlmProvider>> {
        // Try to get from config's llm_profiles
        if let Some(ref profiles) = self.config.llm_profiles {
            // Determine which profile name to use
            let profile_name = if let Some(ref default_name) = profiles.default {
                default_name.clone()
            } else if let Some(first_profile) = profiles.profiles.first() {
                first_profile.name.clone()
            } else {
                // No explicit profiles, try first model set
                if let Some(ref model_sets) = profiles.model_sets {
                    if let Some(first_set) = model_sets.first() {
                        if let Some(ref default_spec) = first_set.default {
                            format!("{}:{}", first_set.name, default_spec)
                        } else if let Some(first_spec) = first_set.models.first() {
                            format!("{}:{}", first_set.name, first_spec.name)
                        } else {
                            return self.create_llm_provider_from_env(model_override).await;
                        }
                    } else {
                        return self.create_llm_provider_from_env(model_override).await;
                    }
                } else {
                    return self.create_llm_provider_from_env(model_override).await;
                }
            };

            // Try to find the profile (explicit or from model set)
            let profile = self.find_llm_profile(profiles, &profile_name)?;

            let provider_type = match profile.provider.to_lowercase().as_str() {
                "openai" | "openrouter" => LlmProviderType::OpenAI,
                "anthropic" | "claude" => LlmProviderType::Anthropic,
                "local" | "ollama" => LlmProviderType::Local,
                "stub" | "test" => LlmProviderType::Stub,
                _ => LlmProviderType::OpenAI, // Default to OpenAI-compatible
            };

            // Get API key from env var or inline
            let api_key = profile
                .api_key_env
                .as_ref()
                .and_then(|env_var| std::env::var(env_var).ok())
                .or_else(|| profile.api_key.clone());

            let config = LlmProviderConfig {
                provider_type,
                model: model_override.unwrap_or_else(|| profile.model.clone()),
                api_key,
                base_url: profile.base_url.clone(),
                max_tokens: profile.max_tokens,
                temperature: profile.temperature,
                timeout_seconds: Some(120), // Increased to 120s for documentation parsing and long responses
                retry_config: Default::default(),
            };

            let provider = LlmProviderFactory::create_provider(config).await?;
            return Ok(Arc::from(provider));
        }

        // Fallback: try environment variables directly
        self.create_llm_provider_from_env(model_override).await
    }

    /// Find an LLM profile by name (supports both explicit profiles and model_sets)
    fn find_llm_profile(
        &self,
        profiles_config: &crate::config::types::LlmProfilesConfig,
        profile_name: &str,
    ) -> RuntimeResult<crate::config::types::LlmProfile> {
        // 1. Check explicit profiles first
        if let Some(profile) = profiles_config
            .profiles
            .iter()
            .find(|p| p.name == profile_name)
        {
            return Ok(profile.clone());
        }

        // 2. Check model sets (format: "set_name:spec_name")
        if let Some(model_sets) = &profiles_config.model_sets {
            if let Some((set_name, spec_name)) = profile_name.split_once(':') {
                if let Some(set) = model_sets.iter().find(|s| s.name == set_name) {
                    if let Some(spec) = set.models.iter().find(|m| m.name == spec_name) {
                        // Construct synthetic profile from model set
                        return Ok(crate::config::types::LlmProfile {
                            name: profile_name.to_string(),
                            provider: set.provider.clone(),
                            model: spec.model.clone(),
                            base_url: set.base_url.clone(),
                            api_key_env: set.api_key_env.clone(),
                            api_key: set.api_key.clone(),
                            temperature: None,
                            max_tokens: spec.max_output_tokens,
                        });
                    }
                }
            }
        }

        Err(RuntimeError::Generic(format!(
            "LLM profile '{}' not found in llm_profiles. Check agent_config.toml for available profiles.",
            profile_name
        )))
    }

    /// Create LLM provider from environment variables (fallback)
    /// Note: This should only be called if llm_profiles are not configured.
    /// It will return an error to ensure users configure llm_profiles in agent_config.toml
    async fn create_llm_provider_from_env(
        &self,
        _model: Option<String>,
    ) -> RuntimeResult<Arc<dyn LlmProvider>> {
        Err(RuntimeError::Generic(
            "No LLM configuration found. Please configure llm_profiles in agent_config.toml with your LLM provider settings.".to_string()
        ))
    }
}
