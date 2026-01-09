//! File system utilities
//!
//! Provides shared functions for file system operations, including filename sanitization
//! and workspace root detection.

use std::path::PathBuf;

/// Sanitize a string to be safe for use as a filename or directory name
///
/// Replaces characters that are unsafe or problematic in filenames with underscores.
/// Preserves alphanumeric characters, hyphens, and underscores.
/// Collapses multiple consecutive underscores into one.
pub fn sanitize_filename(input: &str) -> String {
    let mut result = String::with_capacity(input.len());
    let mut last_was_underscore = false;

    for c in input.chars() {
        if c.is_alphanumeric() || c == '-' || c == '_' || c == '.' {
            if c == '_' {
                if !last_was_underscore {
                    result.push(c);
                    last_was_underscore = true;
                }
            } else {
                result.push(c);
                last_was_underscore = false;
            }
        } else {
            // Replace unsafe char with underscore
            if !last_was_underscore {
                result.push('_');
                last_was_underscore = true;
            }
        }
    }

    // Trim leading/trailing underscores
    result.trim_matches('_').to_string()
}

use std::sync::OnceLock;

/// Cached workspace root resolved at first use
static WORKSPACE_ROOT: OnceLock<PathBuf> = OnceLock::new();

/// Get the workspace root directory.
///
/// Resolution order:
/// 1. Value set via `set_workspace_root()` (typically from config file location)
/// 2. `CCOS_WORKSPACE_ROOT` environment variable (must be absolute and exist)
/// 3. Current working directory as fallback
///
/// The workspace root is cached after first resolution.
pub fn get_workspace_root() -> PathBuf {
    WORKSPACE_ROOT
        .get_or_init(|| {
            // 1. Environment variable override
            if let Ok(root) = std::env::var("CCOS_WORKSPACE_ROOT") {
                let path = PathBuf::from(&root);
                if path.is_absolute() && path.exists() {
                    return path;
                }
            }

            // 2. Search upwards for a Cargo.toml with [workspace]
            if let Ok(mut current) = std::env::current_dir() {
                loop {
                    let cargo_toml = current.join("Cargo.toml");
                    if cargo_toml.exists() {
                        if let Ok(content) = std::fs::read_to_string(&cargo_toml) {
                            if content.contains("[workspace]") {
                                return current;
                            }
                        }
                    }

                    if !current.pop() {
                        break;
                    }
                }
            }

            // 3. Fallback to current directory
            std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."))
        })
        .clone()
}

/// Set the workspace root explicitly.
///
/// Should be called early during initialization (e.g., when loading config).
/// If the config file is at `<workspace>/config/agent_config.toml`, pass
/// the config file's parent directory (`<workspace>/config/`).
///
/// All relative paths in storage config will be resolved relative to this.
///
/// Returns `true` if the root was set, `false` if already set.
pub fn set_workspace_root(root: PathBuf) -> bool {
    WORKSPACE_ROOT.set(root).is_ok()
}

/// Resolve a path relative to the workspace root.
///
/// If the path is already absolute, returns it as-is.
/// If relative, joins it with the workspace root and canonicalizes to resolve `..`.
pub fn resolve_workspace_path(path: &str) -> PathBuf {
    let p = PathBuf::from(path);
    let resolved = if p.is_absolute() {
        p
    } else {
        get_workspace_root().join(p)
    };

    // Canonicalize to resolve any .. or . in the path
    // Fall back to the resolved path if canonicalize fails (e.g., path doesn't exist yet)
    resolved.canonicalize().unwrap_or(resolved)
}

/// Resolve the default plan archive path with environment overrides.
///
/// Resolution order:
/// 1. `CCOS_PLAN_ARCHIVE_PATH` (absolute or workspace-relative)
/// 2. `CCOS_STORAGE_ROOT` + `/plans`
/// 3. `<workspace>/storage/plans` if present
/// 4. `<workspace>/demo_storage/plans` if present
/// 5. Fallback to `<workspace>/storage/plans`
pub fn default_plan_archive_path() -> PathBuf {
    if let Ok(path) = std::env::var("CCOS_PLAN_ARCHIVE_PATH") {
        let p = PathBuf::from(&path);
        return if p.is_absolute() {
            p
        } else {
            resolve_workspace_path(&path)
        };
    }

    if let Ok(root) = std::env::var("CCOS_STORAGE_ROOT") {
        let base = PathBuf::from(&root);
        let base = if base.is_absolute() {
            base
        } else {
            resolve_workspace_path(&root)
        };
        return base.join("plans");
    }

    let workspace_root = get_workspace_root();
    let storage_path = workspace_root.join("storage/plans");
    if storage_path.exists() {
        return storage_path;
    }

    let demo_path = workspace_root.join("demo_storage/plans");
    if demo_path.exists() {
        return demo_path;
    }

    storage_path
}

/// Helper to load the agent configuration from the default location (config/agent_config.toml).
fn load_agent_config() -> Option<crate::config::types::AgentConfig> {
    let config_path = get_workspace_root().join("config/agent_config.toml");
    if !config_path.exists() {
        return None;
    }

    let content = std::fs::read_to_string(&config_path).ok()?;
    toml::from_str(&content).ok()
}

/// Get the configured capabilities directory path.
///
/// Resolution order:
/// 1. `CCOS_CAPABILITY_STORAGE` environment variable
/// 2. Agent config `storage.capabilities_dir` (loaded from config file)
/// 3. Fallback: `<workspace>/capabilities`
pub fn get_configured_capabilities_path() -> PathBuf {
    // Check environment variable first
    if let Ok(path) = std::env::var("CCOS_CAPABILITY_STORAGE") {
        let p = PathBuf::from(&path);
        return if p.is_absolute() {
            p
        } else {
            resolve_workspace_path(&path)
        };
    }

    // Try to load config
    if let Some(config) = load_agent_config() {
        let caps_path = &config.storage.capabilities_dir;
        return resolve_workspace_path(caps_path);
    }

    // Fallback to workspace/capabilities
    get_workspace_root().join("capabilities")
}

/// Get the configured discovered capabilities path (capabilities_dir/discovered_subdir).
pub fn get_configured_discovered_path() -> PathBuf {
    if let Some(config) = load_agent_config() {
        let base = resolve_workspace_path(&config.storage.capabilities_dir);
        return base.join(&config.storage.discovered_subdir);
    }
    get_configured_capabilities_path().join("discovered")
}

/// Get the configured generated capabilities path (capabilities_dir/generated_subdir).
pub fn get_configured_generated_path() -> PathBuf {
    if let Some(config) = load_agent_config() {
        let base = resolve_workspace_path(&config.storage.capabilities_dir);
        return base.join(&config.storage.generated_subdir);
    }
    get_configured_capabilities_path().join("generated")
}

/// Get the configured sessions path (capabilities_dir/sessions_subdir).
pub fn get_configured_sessions_path() -> PathBuf {
    // Check environment variable first
    if let Ok(path) = std::env::var("CCOS_SESSIONS_STORAGE") {
        let p = PathBuf::from(&path);
        return if p.is_absolute() {
            p
        } else {
            resolve_workspace_path(&path)
        };
    }

    if let Some(config) = load_agent_config() {
        let base = resolve_workspace_path(&config.storage.capabilities_dir);
        return base.join(&config.storage.sessions_subdir);
    }
    get_configured_capabilities_path().join("sessions")
}

/// Legacy alias for backward compatibility. Prefer `get_workspace_root()`.
#[deprecated(since = "0.2.0", note = "Use get_workspace_root() instead")]
pub fn find_workspace_root() -> PathBuf {
    get_workspace_root()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_sanitize_filename() {
        assert_eq!(sanitize_filename("valid_name"), "valid_name");
        assert_eq!(sanitize_filename("invalid/name"), "invalid_name");
        assert_eq!(sanitize_filename("name with spaces"), "name_with_spaces");
        assert_eq!(
            sanitize_filename("multiple__underscores"),
            "multiple_underscores"
        );
        assert_eq!(
            sanitize_filename("github.com/user/repo"),
            "github.com_user_repo"
        );
        assert_eq!(sanitize_filename("!@#$%^&*()"), "");
        assert_eq!(sanitize_filename("foo/bar/baz"), "foo_bar_baz");
    }

    #[test]
    fn test_get_configured_paths() {
        // Test environment variable override
        let test_path = "/tmp/ccos_test_caps";
        std::env::set_var("CCOS_CAPABILITY_STORAGE", test_path);
        assert_eq!(get_configured_capabilities_path(), PathBuf::from(test_path));
        assert!(get_configured_discovered_path().starts_with(test_path));
        assert!(get_configured_generated_path().starts_with(test_path));

        // Clean up env var for next tests
        std::env::remove_var("CCOS_CAPABILITY_STORAGE");

        // Test default (should be workspace/capabilities)
        let default_path = get_workspace_root().join("capabilities");
        assert_eq!(get_configured_capabilities_path(), default_path);
        assert_eq!(
            get_configured_discovered_path(),
            default_path.join("discovered")
        );
        assert_eq!(
            get_configured_generated_path(),
            default_path.join("generated")
        );
    }
}
