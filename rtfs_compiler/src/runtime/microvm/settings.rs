//! MicroVM Configuration Management
//!
//! This module provides configuration management for MicroVM providers and security policies.

use super::config::{MicroVMConfig, NetworkPolicy, FileSystemPolicy};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::time::Duration;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MicroVMSettings {
    /// Default MicroVM provider to use
    pub default_provider: String,
    /// Provider-specific configurations
    pub provider_configs: HashMap<String, serde_json::Value>,
    /// Default configuration for all MicroVM executions
    pub default_config: MicroVMConfig,
    /// Capability-specific configurations
    pub capability_configs: HashMap<String, MicroVMConfig>,
}

impl Default for MicroVMSettings {
    fn default() -> Self {
        let mut capability_configs = HashMap::new();
        
        // Network capabilities get more restrictive settings
        capability_configs.insert(
            "ccos.network.http-fetch".to_string(),
            MicroVMConfig {
                timeout: Duration::from_secs(30),
                memory_limit_mb: 64,
                cpu_limit: 0.3,
                network_policy: NetworkPolicy::AllowList(vec![
                    "api.github.com".to_string(),
                    "localhost".to_string(),
                ]),
                fs_policy: FileSystemPolicy::None,
                env_vars: HashMap::new(),
            }
        );
        
        // File I/O capabilities get sandboxed filesystem access
        for capability in ["ccos.io.open-file", "ccos.io.read-line", "ccos.io.write-line", "ccos.io.close-file"] {
            capability_configs.insert(
                capability.to_string(),
                MicroVMConfig {
                    timeout: Duration::from_secs(10),
                    memory_limit_mb: 32,
                    cpu_limit: 0.2,
                    network_policy: NetworkPolicy::Denied,
                    fs_policy: FileSystemPolicy::ReadWrite(vec![
                        "/tmp/rtfs_sandbox".to_string(),
                        "/workspace".to_string(),
                    ]),
                    env_vars: HashMap::new(),
                }
            );
        }
        
        Self {
            default_provider: "mock".to_string(),
            provider_configs: HashMap::new(),
            default_config: MicroVMConfig::default(),
            capability_configs,
        }
    }
}

impl MicroVMSettings {
    pub fn load_from_file(path: &str) -> Result<Self, Box<dyn std::error::Error>> {
        let content = std::fs::read_to_string(path)?;
        let settings: MicroVMSettings = toml::from_str(&content)?;
        Ok(settings)
    }
    
    pub fn save_to_file(&self, path: &str) -> Result<(), Box<dyn std::error::Error>> {
        let content = toml::to_string_pretty(self)?;
        std::fs::write(path, content)?;
        Ok(())
    }
    
    pub fn get_config_for_capability(&self, capability_id: &str) -> MicroVMConfig {
        self.capability_configs
            .get(capability_id)
            .cloned()
            .unwrap_or_else(|| self.default_config.clone())
    }
    
    pub fn set_config_for_capability(&mut self, capability_id: &str, config: MicroVMConfig) {
        self.capability_configs.insert(capability_id.to_string(), config);
    }
}

/// Environment-specific MicroVM configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EnvironmentMicroVMConfig {
    /// Development environment settings
    pub development: MicroVMSettings,
    /// Production environment settings
    pub production: MicroVMSettings,
    /// Testing environment settings
    pub testing: MicroVMSettings,
}

impl Default for EnvironmentMicroVMConfig {
    fn default() -> Self {
        let mut development = MicroVMSettings::default();
        development.default_provider = "mock".to_string();
        
        let mut production = MicroVMSettings::default();
        production.default_provider = "firecracker".to_string();
        
        let mut testing = MicroVMSettings::default();
        testing.default_provider = "mock".to_string();
        // More restrictive settings for testing
        testing.default_config.timeout = Duration::from_secs(5);
        testing.default_config.memory_limit_mb = 32;
        
        Self {
            development,
            production,
            testing,
        }
    }
}

impl EnvironmentMicroVMConfig {
    pub fn get_settings_for_env(&self, env: &str) -> &MicroVMSettings {
        match env {
            "development" | "dev" => &self.development,
            "production" | "prod" => &self.production,
            "testing" | "test" => &self.testing,
            _ => &self.development, // Default to development
        }
    }
}
