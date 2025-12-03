//! File system utilities
//!
//! Provides shared functions for file system operations, including filename sanitization.

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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_sanitize_filename() {
        assert_eq!(sanitize_filename("valid_name"), "valid_name");
        assert_eq!(sanitize_filename("invalid/name"), "invalid_name");
        assert_eq!(sanitize_filename("name with spaces"), "name_with_spaces");
        assert_eq!(sanitize_filename("multiple__underscores"), "multiple_underscores");
        assert_eq!(sanitize_filename("github.com/user/repo"), "github.com_user_repo");
        assert_eq!(sanitize_filename("!@#$%^&*()"), "");
        assert_eq!(sanitize_filename("foo/bar/baz"), "foo_bar_baz");
    }
}

