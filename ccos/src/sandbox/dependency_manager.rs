//! Dependency management for sandbox code execution
//!
//! Handles package allowlist checking, dynamic pip installation,
//! and package caching for Python code execution.

use crate::config::types::{PackageAllowlistConfig, SandboxConfig};
use std::collections::HashSet;
use std::path::PathBuf;
use std::process::Stdio;
use tokio::process::Command;
use tracing::{debug, info, warn};

/// Dependency resolution result
#[derive(Debug, Clone)]
pub enum DependencyResolution {
    /// Package is already available in the base image
    AlreadyAvailable,
    /// Package needs to be installed (auto-approved)
    AutoInstall { package: String },
    /// Package requires manual approval
    RequiresApproval { package: String },
    /// Package is blocked
    Blocked { package: String, reason: String },
}

/// Dependency manager for Python packages
pub struct DependencyManager {
    config: SandboxConfig,
    allowlist: PackageAllowlistConfig,
    base_packages: HashSet<String>,
}

impl DependencyManager {
    /// Create a new dependency manager
    pub fn new(config: SandboxConfig) -> Self {
        // Extract base packages from the default image
        let base_packages: HashSet<String> = config
            .images
            .iter()
            .find(|img| img.name == "python-data-science")
            .map(|img| {
                img.packages
                    .iter()
                    .map(|p| extract_package_name(p))
                    .collect()
            })
            .unwrap_or_default();

        Self {
            allowlist: config.package_allowlist.clone(),
            base_packages,
            config,
        }
    }

    /// Check if a package is in the base image
    pub fn is_in_base_image(&self, package: &str) -> bool {
        let pkg_name = extract_package_name(package);
        self.base_packages.contains(&pkg_name)
    }

    /// Resolve a dependency against the allowlist
    pub fn resolve_dependency(&self, package: &str) -> DependencyResolution {
        let pkg_name = extract_package_name(package);

        // Check if already in base image
        if self.is_in_base_image(&pkg_name) {
            return DependencyResolution::AlreadyAvailable;
        }

        // Check blocked list
        if self
            .allowlist
            .blocked
            .iter()
            .any(|p| extract_package_name(p) == pkg_name)
        {
            return DependencyResolution::Blocked {
                package: pkg_name,
                reason: "Package is in blocked list".to_string(),
            };
        }

        // Check auto-approved list
        if self
            .allowlist
            .auto_approved
            .iter()
            .any(|p| extract_package_name(p) == pkg_name)
        {
            return DependencyResolution::AutoInstall { package: pkg_name };
        }

        // Check requires-approval list
        if self
            .allowlist
            .requires_approval
            .iter()
            .any(|p| extract_package_name(p) == pkg_name)
        {
            return DependencyResolution::RequiresApproval { package: pkg_name };
        }

        // Default: requires approval for unknown packages
        DependencyResolution::RequiresApproval { package: pkg_name }
    }

    /// Resolve multiple dependencies
    pub fn resolve_dependencies(&self, packages: &[String]) -> Vec<DependencyResolution> {
        packages
            .iter()
            .map(|p| self.resolve_dependency(p))
            .collect()
    }

    /// Install packages in the sandbox
    pub async fn install_packages(
        &self,
        packages: &[String],
        work_dir: &PathBuf,
    ) -> Result<(), String> {
        if packages.is_empty() {
            return Ok(());
        }

        info!("Installing packages: {:?}", packages);

        // Build pip install command
        let mut cmd = Command::new("python3");
        cmd.arg("-m");
        cmd.arg("pip");
        cmd.arg("install");
        cmd.arg("--target");
        cmd.arg("."); // Install into the current work_dir
        cmd.arg("--no-cache-dir"); // Don't use pip cache
        cmd.arg("--disable-pip-version-check");

        // Use package cache if enabled
        if self.config.enable_package_cache {
            let cache_dir = PathBuf::from(&self.config.package_cache_dir);
            if cache_dir.exists() {
                cmd.arg("--find-links").arg(&cache_dir);
            }
        }

        // Add packages
        for pkg in packages {
            cmd.arg(pkg);
        }

        // Set working directory and environment
        cmd.current_dir(work_dir);
        cmd.env("PIP_NO_WARN_SCRIPT_LOCATION", "0");
        cmd.env("PYTHONDONTWRITEBYTECODE", "1");

        // Execute
        let output = cmd
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .output()
            .await
            .map_err(|e| format!("Failed to execute pip install: {}", e))?;

        if output.status.success() {
            debug!("Successfully installed packages: {:?}", packages);
            Ok(())
        } else {
            let stderr = String::from_utf8_lossy(&output.stderr);
            warn!("Failed to install packages: {}", stderr);
            Err(format!("pip install failed: {}", stderr))
        }
    }

    /// Cache packages for future use
    pub async fn cache_packages(&self, packages: &[String]) -> Result<(), String> {
        if !self.config.enable_package_cache {
            return Ok(());
        }

        let cache_dir = PathBuf::from(&self.config.package_cache_dir);

        // Create cache directory if it doesn't exist
        tokio::fs::create_dir_all(&cache_dir)
            .await
            .map_err(|e| format!("Failed to create cache directory: {}", e))?;

        // Download packages to cache
        let mut cmd = Command::new("pip");
        cmd.arg("download");
        cmd.arg("--no-deps"); // Only download specified packages, not dependencies
        cmd.arg("-d").arg(&cache_dir);

        for pkg in packages {
            cmd.arg(pkg);
        }

        let output = cmd
            .output()
            .await
            .map_err(|e| format!("Failed to download packages: {}", e))?;

        if output.status.success() {
            info!("Cached packages to {:?}", cache_dir);
            Ok(())
        } else {
            let stderr = String::from_utf8_lossy(&output.stderr);
            warn!("Failed to cache packages: {}", stderr);
            Err(format!("pip download failed: {}", stderr))
        }
    }

    /// Install npm packages in the sandbox
    pub async fn install_npm_packages(
        &self,
        packages: &[String],
        work_dir: &std::path::PathBuf,
    ) -> Result<(), String> {
        if packages.is_empty() {
            return Ok(());
        }

        info!("Installing npm packages: {:?}", packages);

        // Build npm install command
        let mut cmd = Command::new("npm");
        cmd.arg("install");
        cmd.arg("--no-save"); // Don't update package.json
        cmd.arg("--no-package-lock");
        cmd.arg("--omit=dev"); // Only production dependencies

        // Add packages
        for pkg in packages {
            cmd.arg(pkg);
        }

        // Set working directory
        cmd.current_dir(work_dir);

        // Execute
        let output = cmd
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .output()
            .await
            .map_err(|e| format!("Failed to execute npm install: {}", e))?;

        if output.status.success() {
            debug!("Successfully installed npm packages: {:?}", packages);
            Ok(())
        } else {
            let stderr = String::from_utf8_lossy(&output.stderr);
            warn!("Failed to install npm packages: {}", stderr);
            Err(format!("npm install failed: {}", stderr))
        }
    }
}

/// Extract package name from a package spec
/// Supports both Python (e.g., "pandas>=2.0") and npm (e.g., "lodash@4.17.21")
fn extract_package_name(package_spec: &str) -> String {
    // For npm scoped packages like @types/node@14.0.0
    if package_spec.starts_with('@') {
        let parts: Vec<&str> = package_spec.split('@').collect();
        if parts.len() >= 2 {
            // @scope/name@version -> @scope/name
            return format!("@{}", parts[1]);
        }
    }

    // Split by version specifiers
    package_spec
        .split(|c| c == '=' || c == '<' || c == '>' || c == '!' || c == '@' || c == '^' || c == '~')
        .next()
        .unwrap_or(package_spec)
        .trim()
        .to_lowercase()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extract_package_name() {
        assert_eq!(extract_package_name("pandas"), "pandas");
        assert_eq!(extract_package_name("pandas>=2.0"), "pandas");
        assert_eq!(extract_package_name("numpy==1.24.0"), "numpy");
        assert_eq!(extract_package_name("requests<3.0"), "requests");
    }

    #[test]
    fn test_is_in_base_image() {
        let config = SandboxConfig::default();
        let manager = DependencyManager::new(config);

        assert!(manager.is_in_base_image("pandas"));
        assert!(manager.is_in_base_image("pandas>=2.0"));
        assert!(!manager.is_in_base_image("nonexistent-package"));
    }

    #[test]
    fn test_resolve_dependency() {
        let config = SandboxConfig::default();
        let manager = DependencyManager::new(config);

        // Should be already available
        match manager.resolve_dependency("pandas") {
            DependencyResolution::AlreadyAvailable => (),
            _ => panic!("Expected AlreadyAvailable for pandas"),
        }

        // Should be blocked
        match manager.resolve_dependency("pyautogui") {
            DependencyResolution::Blocked { .. } => (),
            _ => panic!("Expected Blocked for pyautogui"),
        }

        // Should require approval (not in any list)
        match manager.resolve_dependency("unknown-package") {
            DependencyResolution::RequiresApproval { .. } => (),
            _ => panic!("Expected RequiresApproval for unknown package"),
        }
    }
}
