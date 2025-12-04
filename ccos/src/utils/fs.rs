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

/// Find the workspace root directory (where `capabilities/` and `ccos/Cargo.toml` live).
///
/// Strategy:
/// 1) If current dir has both `ccos/Cargo.toml` and `capabilities/`, use it.
/// 2) Walk up to find a dir that has both.
/// 3) Walk up to find a dir that has `capabilities/`.
/// 4) If inside `ccos/`, go up one level if it has `capabilities/` or `ccos/Cargo.toml`.
/// 5) Fallback to current dir.
pub fn find_workspace_root() -> PathBuf {
    let current_dir = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));

    // Strategy 1
    if current_dir.join("ccos/Cargo.toml").exists() && current_dir.join("capabilities").exists() {
        return current_dir;
    }

    // Strategy 2
    let mut path = current_dir.clone();
    loop {
        if path.join("ccos/Cargo.toml").exists() && path.join("capabilities").exists() {
            return path;
        }
        if let Some(parent) = path.parent() {
            path = parent.to_path_buf();
        } else {
            break;
        }
    }

    // Strategy 3
    let mut path = current_dir.clone();
    loop {
        if path.join("capabilities").exists() {
            return path;
        }
        if let Some(parent) = path.parent() {
            path = parent.to_path_buf();
        } else {
            break;
        }
    }

    // Strategy 4
    if current_dir.join("Cargo.toml").exists() {
        if let Some(parent) = current_dir.parent() {
            if parent.join("capabilities").exists() || parent.join("ccos/Cargo.toml").exists() {
                return parent.to_path_buf();
            }
        }
    }

    // Strategy 5
    current_dir
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
}
