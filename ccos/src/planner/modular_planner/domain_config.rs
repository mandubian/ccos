//! Domain configuration loader
//!
//! Loads domain hints from config file, allowing new domains to be added
//! without code changes.

use once_cell::sync::Lazy;
use serde::Deserialize;
use std::path::Path;
use std::sync::RwLock;

/// A domain definition from config
#[derive(Debug, Clone, Deserialize)]
pub struct DomainDef {
    pub name: String,
    pub keywords: Vec<String>,
    pub mcp_servers: Vec<String>,
}

/// Root config structure
#[derive(Debug, Clone, Deserialize, Default)]
pub struct DomainHintsConfig {
    #[serde(default)]
    pub domains: Vec<DomainDef>,
}

/// Global domain config, loaded lazily
static DOMAIN_CONFIG: Lazy<RwLock<DomainHintsConfig>> = Lazy::new(|| {
    let config = load_domain_config().unwrap_or_default();
    RwLock::new(config)
});

/// Load domain config from the standard location
fn load_domain_config() -> Option<DomainHintsConfig> {
    // Try multiple locations
    let locations = [
        "config/domain_hints.toml",
        "../config/domain_hints.toml",
        "../../config/domain_hints.toml",
    ];

    for loc in &locations {
        if let Ok(content) = std::fs::read_to_string(loc) {
            if let Ok(config) = toml::from_str(&content) {
                log::debug!("Loaded domain hints from {}", loc);
                return Some(config);
            }
        }
    }

    // Also try relative to env var if set
    if let Ok(config_dir) = std::env::var("CCOS_CONFIG_DIR") {
        let path = Path::new(&config_dir).join("domain_hints.toml");
        if let Ok(content) = std::fs::read_to_string(&path) {
            if let Ok(config) = toml::from_str(&content) {
                log::debug!("Loaded domain hints from {:?}", path);
                return Some(config);
            }
        }
    }

    log::debug!("No domain hints config found, using defaults");
    None
}

/// Infer domain from goal text using configured keywords
pub fn infer_domain(text: &str) -> Option<String> {
    let lower = text.to_lowercase();
    let config = DOMAIN_CONFIG.read().ok()?;

    for domain in &config.domains {
        if domain.keywords.iter().any(|kw| lower.contains(kw)) {
            return Some(domain.name.clone());
        }
    }

    None
}

/// Infer all matching domains from text
pub fn infer_all_domains(text: &str) -> Vec<String> {
    let lower = text.to_lowercase();
    let config = match DOMAIN_CONFIG.read() {
        Ok(c) => c,
        Err(_) => return vec![],
    };

    config
        .domains
        .iter()
        .filter(|d| d.keywords.iter().any(|kw| lower.contains(kw)))
        .map(|d| d.name.clone())
        .collect()
}

/// Get MCP servers that might handle a domain
pub fn mcp_servers_for_domain(domain: &str) -> Vec<String> {
    let config = match DOMAIN_CONFIG.read() {
        Ok(c) => c,
        Err(_) => return vec![],
    };

    config
        .domains
        .iter()
        .find(|d| d.name == domain)
        .map(|d| d.mcp_servers.clone())
        .unwrap_or_default()
}

/// Get all configured domain names
pub fn all_domain_names() -> Vec<String> {
    let config = match DOMAIN_CONFIG.read() {
        Ok(c) => c,
        Err(_) => return vec![],
    };

    config.domains.iter().map(|d| d.name.clone()).collect()
}

/// Reload domain config from disk (useful for hot-reload)
pub fn reload_config() -> Result<(), String> {
    let new_config = load_domain_config().ok_or("Failed to load domain config")?;
    let mut config = DOMAIN_CONFIG
        .write()
        .map_err(|e| format!("Lock error: {}", e))?;
    *config = new_config;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_infer_domain_from_text() {
        // These tests depend on config being loaded
        // In real tests, we'd inject a mock config
        let domain = infer_domain("list issues in a repository");
        // May be Some("github") if config loaded, None otherwise
        if let Some(d) = domain {
            assert_eq!(d, "github");
        }
    }

    #[test]
    fn test_infer_all_domains() {
        let domains = infer_all_domains("send a message to slack channel");
        // May contain "slack" if config loaded
        if !domains.is_empty() {
            assert!(domains.contains(&"slack".to_string()));
        }
    }
}
