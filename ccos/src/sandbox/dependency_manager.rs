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
        // For bubblewrap, we don't use pre-baked images, so treat base packages as empty
        // to force auto-installation of allowed packages.
        let base_packages: HashSet<String> = if config.runtime == "bubblewrap" {
            HashSet::new()
        } else {
            config
                .images
                .iter()
                .find(|img| img.name == "python-data-science")
                .map(|img| {
                    img.packages
                        .iter()
                        .map(|p| extract_package_name(p))
                        .collect()
                })
                .unwrap_or_default()
        };

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

    /// Install packages into an isolated venv using `uv`.
    ///
    /// Creates `<target_dir>/.venv` via `uv venv`, then installs all packages
    /// with `uv pip install --python <venv>`. The venv is self-contained and
    /// strictly isolated: no packages from the host Python environment leak in.
    ///
    /// Returns the path to the venv's `python` binary so callers can activate it.
    ///
    /// # Errors
    /// Returns `Err` if `uv` is not on `PATH`, or if the install fails.
    /// There is **no fallback** to `pip --target`; that would write to the host.
    pub async fn install_packages(
        &self,
        packages: &[String],
        work_dir: &PathBuf,
        target_dir: &PathBuf,
    ) -> Result<PathBuf, String> {
        if packages.is_empty() {
            // Return the system python if there is nothing to install
            return Ok(PathBuf::from("/usr/bin/python3"));
        }

        // Locate uv — fail hard if absent; we do not fall back to host pip.
        let uv = find_uv_binary().ok_or_else(|| {
            "uv not found. Install with: curl -LsSf https://astral.sh/uv/install.sh | sh\n\
                 NOTE: pip --target is intentionally NOT used as it would install to the host."
                .to_string()
        })?;

        let venv_dir = target_dir.join(".venv");
        info!(
            "Creating venv at {:?} for packages {:?}",
            venv_dir, packages
        );

        // Step 1: create the venv
        let venv_out = Command::new(&uv)
            .arg("venv")
            .arg(&venv_dir)
            .current_dir(work_dir)
            .env(
                "HOME",
                std::env::var("HOME").unwrap_or_else(|_| "/tmp".to_string()),
            )
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .output()
            .await
            .map_err(|e| format!("Failed to run `uv venv`: {}", e))?;

        if !venv_out.status.success() {
            let stderr = String::from_utf8_lossy(&venv_out.stderr);
            return Err(format!("uv venv failed: {}", stderr));
        }

        let venv_python = venv_dir.join("bin").join("python");

        // Step 2: install packages into the venv
        let mut cmd = Command::new(&uv);
        cmd.arg("pip")
            .arg("install")
            .arg("--python")
            .arg(&venv_python)
            .arg("--no-cache-dir");

        for pkg in packages {
            cmd.arg(pkg);
        }

        cmd.current_dir(work_dir);
        cmd.env(
            "HOME",
            std::env::var("HOME").unwrap_or_else(|_| "/tmp".to_string()),
        );
        cmd.env("PYTHONDONTWRITEBYTECODE", "1");
        cmd.stdout(Stdio::piped());
        cmd.stderr(Stdio::piped());

        let install_out = cmd
            .output()
            .await
            .map_err(|e| format!("Failed to run `uv pip install`: {}", e))?;

        if install_out.status.success() {
            debug!("Successfully installed {:?} into {:?}", packages, venv_dir);
            Ok(venv_python)
        } else {
            let stderr = String::from_utf8_lossy(&install_out.stderr);
            warn!("uv pip install failed: {}", stderr);
            Err(format!("uv pip install failed: {}", stderr))
        }
    }

    /// Cache packages for future use using `uv pip download`.
    ///
    /// Uses `uv` exclusively — no bare `pip` calls that could touch the host environment.
    pub async fn cache_packages(&self, packages: &[String]) -> Result<(), String> {
        if !self.config.enable_package_cache {
            return Ok(());
        }

        let uv = find_uv_binary().ok_or_else(|| {
            "uv not found; cannot cache packages. \
             Install with: curl -LsSf https://astral.sh/uv/install.sh | sh"
                .to_string()
        })?;

        let cache_dir = PathBuf::from(&self.config.package_cache_dir);
        tokio::fs::create_dir_all(&cache_dir)
            .await
            .map_err(|e| format!("Failed to create cache directory: {}", e))?;

        let mut cmd = Command::new(&uv);
        cmd.arg("pip").arg("download");
        cmd.arg("--no-deps");
        cmd.arg("-d").arg(&cache_dir);
        cmd.env(
            "HOME",
            std::env::var("HOME").unwrap_or_else(|_| "/tmp".to_string()),
        );

        for pkg in packages {
            cmd.arg(pkg);
        }

        let output = cmd
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .output()
            .await
            .map_err(|e| format!("Failed to run `uv pip download`: {}", e))?;

        if output.status.success() {
            info!("Cached packages to {:?}", cache_dir);
            Ok(())
        } else {
            let stderr = String::from_utf8_lossy(&output.stderr);
            warn!("uv pip download failed: {}", stderr);
            Err(format!("uv pip download failed: {}", stderr))
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

/// Locate the `uv` binary by checking well-known paths and then `PATH`.
fn find_uv_binary() -> Option<PathBuf> {
    use std::path::Path;
    // Check $HOME/.local/bin/uv (common for user installs) and well-known system paths.
    let home_local = std::env::var("HOME")
        .ok()
        .map(|h| PathBuf::from(h).join(".local").join("bin").join("uv"));
    if let Some(ref p) = home_local {
        if p.exists() {
            return Some(p.clone());
        }
    }
    for p in &["/usr/local/bin/uv", "/usr/bin/uv"] {
        if Path::new(p).exists() {
            return Some(PathBuf::from(p));
        }
    }
    // Fall through to PATH lookup
    std::process::Command::new("which")
        .arg("uv")
        .output()
        .ok()
        .filter(|o| o.status.success())
        .and_then(|o| String::from_utf8(o.stdout).ok())
        .map(|s| PathBuf::from(s.trim()))
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
        let mut config = SandboxConfig::default();
        config.runtime = "microvm".to_string();
        let manager = DependencyManager::new(config);

        assert!(manager.is_in_base_image("pandas"));
        assert!(manager.is_in_base_image("pandas>=2.0"));
        assert!(!manager.is_in_base_image("nonexistent-package"));
    }

    #[test]
    fn test_resolve_dependency() {
        let mut config = SandboxConfig::default();
        config.runtime = "microvm".to_string();
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
