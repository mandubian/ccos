//! CLI context - shared state and services for all commands

use crate::capability_marketplace::CapabilityMarketplace;
use crate::mcp::core::MCPDiscoveryService;
use rtfs::config::types::AgentConfig;
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
            if profiles.profiles.is_empty() && profiles.model_sets.as_ref().map_or(true, |s| s.is_empty()) {
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
}
