//! Capability versioning and update utilities
//!
//! This module provides semantic version parsing, comparison, and breaking change detection
//! for capability manifests.

use rtfs::runtime::error::{RuntimeError, RuntimeResult};
use std::cmp::Ordering;

/// Represents a semantic version (major.minor.patch)
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SemanticVersion {
    pub major: u64,
    pub minor: u64,
    pub patch: u64,
    pub pre_release: Option<String>,
    pub build_metadata: Option<String>,
}

impl SemanticVersion {
    /// Parse a version string into a SemanticVersion
    ///
    /// Supports formats like:
    /// - "1.0.0"
    /// - "1.2.3-alpha"
    /// - "2.0.0-beta.1"
    /// - "1.0.0+build.123"
    pub fn parse(version_str: &str) -> RuntimeResult<Self> {
        let version_str = version_str.trim();
        
        // Split on '+' for build metadata
        let (version_part, build_metadata) = if let Some(pos) = version_str.find('+') {
            (
                &version_str[..pos],
                Some(version_str[pos + 1..].to_string()),
            )
        } else {
            (version_str, None)
        };

        // Split on '-' for pre-release
        let (version_part, pre_release) = if let Some(pos) = version_part.find('-') {
            (
                &version_part[..pos],
                Some(version_part[pos + 1..].to_string()),
            )
        } else {
            (version_part, None)
        };

        // Parse major.minor.patch
        let parts: Vec<&str> = version_part.split('.').collect();
        if parts.len() < 2 || parts.len() > 3 {
            return Err(RuntimeError::Generic(format!(
                "Invalid version format: {} (expected major.minor.patch)",
                version_str
            )));
        }

        let major = parts[0]
            .parse::<u64>()
            .map_err(|_| RuntimeError::Generic(format!("Invalid major version: {}", parts[0])))?;

        let minor = parts[1]
            .parse::<u64>()
            .map_err(|_| RuntimeError::Generic(format!("Invalid minor version: {}", parts[1])))?;

        let patch = if parts.len() == 3 {
            parts[2]
                .parse::<u64>()
                .map_err(|_| RuntimeError::Generic(format!("Invalid patch version: {}", parts[2])))?
        } else {
            0
        };

        Ok(Self {
            major,
            minor,
            patch,
            pre_release,
            build_metadata,
        })
    }

    /// Compare two semantic versions
    ///
    /// Returns:
    /// - Ordering::Less if self < other
    /// - Ordering::Equal if self == other
    /// - Ordering::Greater if self > other
    pub fn compare(&self, other: &Self) -> Ordering {
        match self.major.cmp(&other.major) {
            Ordering::Equal => match self.minor.cmp(&other.minor) {
                Ordering::Equal => match self.patch.cmp(&other.patch) {
                    Ordering::Equal => {
                        // Pre-release versions are considered less than release versions
                        match (&self.pre_release, &other.pre_release) {
                            (None, None) => Ordering::Equal,
                            (Some(_), None) => Ordering::Less,
                            (None, Some(_)) => Ordering::Greater,
                            (Some(a), Some(b)) => a.cmp(b),
                        }
                    }
                    other => other,
                },
                other => other,
            },
            other => other,
        }
    }

    /// Check if this version is a major version bump from another
    pub fn is_major_bump(&self, other: &Self) -> bool {
        self.major > other.major
    }

    /// Check if this version is a minor version bump from another
    pub fn is_minor_bump(&self, other: &Self) -> bool {
        self.major == other.major && self.minor > other.minor
    }

    /// Check if this version is a patch version bump from another
    pub fn is_patch_bump(&self, other: &Self) -> bool {
        self.major == other.major
            && self.minor == other.minor
            && self.patch > other.patch
    }

    /// Check if this version is newer than another
    pub fn is_newer_than(&self, other: &Self) -> bool {
        self.compare(other) == Ordering::Greater
    }

    /// Check if this version is older than another
    pub fn is_older_than(&self, other: &Self) -> bool {
        self.compare(other) == Ordering::Less
    }

    /// Convert back to string representation
    pub fn to_string(&self) -> String {
        let mut result = format!("{}.{}.{}", self.major, self.minor, self.patch);
        if let Some(ref pre) = self.pre_release {
            result.push_str("-");
            result.push_str(pre);
        }
        if let Some(ref build) = self.build_metadata {
            result.push_str("+");
            result.push_str(build);
        }
        result
    }
}

impl std::fmt::Display for SemanticVersion {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.to_string())
    }
}

/// Result of comparing two capability versions
#[derive(Debug, Clone, PartialEq)]
pub enum VersionComparison {
    /// Same version
    Equal,
    /// New version is a patch update (backward compatible)
    PatchUpdate,
    /// New version is a minor update (backward compatible additions)
    MinorUpdate,
    /// New version is a major update (potentially breaking)
    MajorUpdate,
    /// New version is older (downgrade)
    Downgrade,
}

/// Compare two version strings and determine the type of update
pub fn compare_versions(old_version: &str, new_version: &str) -> RuntimeResult<VersionComparison> {
    let old = SemanticVersion::parse(old_version)?;
    let new = SemanticVersion::parse(new_version)?;

    match new.compare(&old) {
        Ordering::Equal => Ok(VersionComparison::Equal),
        Ordering::Less => Ok(VersionComparison::Downgrade),
        Ordering::Greater => {
            if new.is_major_bump(&old) {
                Ok(VersionComparison::MajorUpdate)
            } else if new.is_minor_bump(&old) {
                Ok(VersionComparison::MinorUpdate)
            } else if new.is_patch_bump(&old) {
                Ok(VersionComparison::PatchUpdate)
            } else {
                // Should not happen, but handle gracefully
                Ok(VersionComparison::PatchUpdate)
            }
        }
    }
}

/// Detect if a capability update contains breaking changes
///
/// Breaking changes are detected by:
/// - Major version bump
/// - Input schema changes (removed required fields, type changes)
/// - Output schema structure changes
/// - Effects/permissions broadening (security concern)
pub fn detect_breaking_changes(
    old_manifest: &crate::capability_marketplace::types::CapabilityManifest,
    new_manifest: &crate::capability_marketplace::types::CapabilityManifest,
) -> RuntimeResult<Vec<String>> {
    let mut breaking_changes = Vec::new();

    // Check version
    match compare_versions(&old_manifest.version, &new_manifest.version)? {
        VersionComparison::MajorUpdate => {
            breaking_changes.push(format!(
                "Major version bump: {} -> {}",
                old_manifest.version, new_manifest.version
            ));
        }
        VersionComparison::Downgrade => {
            breaking_changes.push(format!(
                "Version downgrade: {} -> {}",
                old_manifest.version, new_manifest.version
            ));
        }
        _ => {}
    }

    // Check input schema changes (simplified - would need deeper schema comparison)
    if old_manifest.input_schema != new_manifest.input_schema {
        breaking_changes.push("Input schema changed".to_string());
    }

    // Check output schema changes
    if old_manifest.output_schema != new_manifest.output_schema {
        breaking_changes.push("Output schema changed".to_string());
    }

    // Check effects/permissions broadening (security)
    let old_effects: std::collections::HashSet<_> = old_manifest.effects.iter().collect();
    let new_effects: std::collections::HashSet<_> = new_manifest.effects.iter().collect();
    let added_effects: Vec<_> = new_effects.difference(&old_effects).collect();
    if !added_effects.is_empty() {
        breaking_changes.push(format!(
            "Effects broadened: added {:?}",
            added_effects
        ));
    }

    let old_permissions: std::collections::HashSet<_> = old_manifest.permissions.iter().collect();
    let new_permissions: std::collections::HashSet<_> = new_manifest.permissions.iter().collect();
    let added_permissions: Vec<_> = new_permissions.difference(&old_permissions).collect();
    if !added_permissions.is_empty() {
        breaking_changes.push(format!(
            "Permissions broadened: added {:?}",
            added_permissions
        ));
    }

    Ok(breaking_changes)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_semantic_version() {
        let v1 = SemanticVersion::parse("1.0.0").unwrap();
        assert_eq!(v1.major, 1);
        assert_eq!(v1.minor, 0);
        assert_eq!(v1.patch, 0);

        let v2 = SemanticVersion::parse("2.1.3").unwrap();
        assert_eq!(v2.major, 2);
        assert_eq!(v2.minor, 1);
        assert_eq!(v2.patch, 3);

        let v3 = SemanticVersion::parse("1.2").unwrap();
        assert_eq!(v3.major, 1);
        assert_eq!(v3.minor, 2);
        assert_eq!(v3.patch, 0);
    }

    #[test]
    fn test_version_comparison() {
        let v1 = SemanticVersion::parse("1.0.0").unwrap();
        let v2 = SemanticVersion::parse("1.0.1").unwrap();
        let v3 = SemanticVersion::parse("1.1.0").unwrap();
        let v4 = SemanticVersion::parse("2.0.0").unwrap();

        assert!(v2.is_newer_than(&v1));
        assert!(v3.is_newer_than(&v2));
        assert!(v4.is_newer_than(&v3));
        assert!(v1.is_older_than(&v2));
    }

    #[test]
    fn test_compare_versions() {
        assert_eq!(
            compare_versions("1.0.0", "1.0.1").unwrap(),
            VersionComparison::PatchUpdate
        );
        assert_eq!(
            compare_versions("1.0.0", "1.1.0").unwrap(),
            VersionComparison::MinorUpdate
        );
        assert_eq!(
            compare_versions("1.0.0", "2.0.0").unwrap(),
            VersionComparison::MajorUpdate
        );
        assert_eq!(
            compare_versions("1.0.0", "1.0.0").unwrap(),
            VersionComparison::Equal
        );
    }
}

