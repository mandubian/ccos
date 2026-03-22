//! Gateway configuration loading.

use autonoetic_types::config::GatewayConfig;
use std::path::Path;

/// Load a `GatewayConfig` from a YAML file on disk.
///
/// Falls back to `GatewayConfig::default()` if the path does not exist.
pub fn load_config(path: &Path) -> anyhow::Result<GatewayConfig> {
    if path.exists() {
        let contents = std::fs::read_to_string(path)?;
        let mut config: GatewayConfig = serde_yaml::from_str(&contents)?;
        // Canonicalize agents_dir to absolute path so all components resolve to the same location
        config.agents_dir = config
            .agents_dir
            .canonicalize()
            .unwrap_or_else(|_| config.agents_dir.clone());
        Ok(config)
    } else {
        tracing::warn!("Config not found at {}, using defaults", path.display());
        Ok(GatewayConfig::default())
    }
}
