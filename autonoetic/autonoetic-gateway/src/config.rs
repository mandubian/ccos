//! Gateway configuration loading.

use autonoetic_types::config::GatewayConfig;
use std::path::Path;

/// Load a `GatewayConfig` from a YAML file on disk.
///
/// Falls back to `GatewayConfig::default()` if the path does not exist.
pub fn load_config(path: &Path) -> anyhow::Result<GatewayConfig> {
    if path.exists() {
        let contents = std::fs::read_to_string(path)?;
        let config: GatewayConfig = serde_yaml::from_str(&contents)?;
        Ok(config)
    } else {
        tracing::warn!("Config not found at {}, using defaults", path.display());
        Ok(GatewayConfig::default())
    }
}
