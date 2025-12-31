//! Dependency Extraction for Synthesized Capabilities
//!
//! This module implements Phase 1 of the missing capability resolution plan:
//! - Extract dependencies from synthesized RTFS code
//! - Compare against CapabilityMarketplace to identify missing capabilities
//! - Attach metadata and emit audit events

use crate::capability_marketplace::types::{CapabilityManifest, EffectType};
use regex::Regex;
use rtfs::runtime::error::RuntimeResult;
use std::collections::{HashMap, HashSet};

/// Represents a capability dependency found in RTFS code
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct CapabilityDependency {
    /// The capability ID (e.g., "travel.flights")
    pub capability_id: String,
    /// The full call expression (e.g., "(call :travel.flights {...})")
    pub call_expression: String,
    /// Line number where the call was found
    pub line_number: usize,
}

/// Result of dependency extraction from RTFS code
#[derive(Debug, Clone)]
pub struct DependencyExtractionResult {
    /// All capability dependencies found
    pub dependencies: Vec<CapabilityDependency>,
    /// Dependencies that exist in the marketplace
    pub resolved_dependencies: HashSet<String>,
    /// Dependencies that are missing from the marketplace
    pub missing_dependencies: HashSet<String>,
    /// Metadata to attach to the capability
    pub metadata: HashMap<String, String>,
}

/// Extract capability dependencies from RTFS code
pub fn extract_dependencies(rtfs_code: &str) -> RuntimeResult<DependencyExtractionResult> {
    let dependencies = extract_capability_calls(rtfs_code)?;

    let dependency_ids: HashSet<String> = dependencies
        .iter()
        .map(|d| d.capability_id.trim().trim_matches('"').to_string())
        .collect();

    let resolved_dependencies = HashSet::new();
    let mut missing_dependencies = HashSet::new();

    // For now, we'll mark all dependencies as missing since we don't have access to marketplace
    // In the full implementation, this would check against the marketplace snapshot
    missing_dependencies = dependency_ids;

    let mut metadata = HashMap::new();
    metadata.insert(
        "dependencies.total".to_string(),
        dependencies.len().to_string(),
    );
    metadata.insert(
        "dependencies.missing".to_string(),
        missing_dependencies.len().to_string(),
    );
    metadata.insert(
        "dependencies.resolved".to_string(),
        resolved_dependencies.len().to_string(),
    );

    // Create comma-separated list of missing dependencies for easy access
    if !missing_dependencies.is_empty() {
        let missing_list: Vec<String> = missing_dependencies.iter().cloned().collect();
        metadata.insert("needs_capabilities".to_string(), missing_list.join(","));
    }

    Ok(DependencyExtractionResult {
        dependencies,
        resolved_dependencies,
        missing_dependencies,
        metadata,
    })
}

/// Extract capability calls from RTFS code using regex
fn extract_capability_calls(rtfs_code: &str) -> RuntimeResult<Vec<CapabilityDependency>> {
    let mut dependencies = Vec::new();

    // Regex to match (call :capability.id ...) patterns
    let call_regex = Regex::new(r#"(?m)\(call\s+:([a-zA-Z0-9._-]+)\s+"#).map_err(|e| {
        rtfs::runtime::error::RuntimeError::Generic(format!("Failed to compile regex: {}", e))
    })?;

    for (line_num, line) in rtfs_code.lines().enumerate() {
        if let Some(captures) = call_regex.captures(line) {
            if let Some(capability_id) = captures.get(1) {
                let capability_id = capability_id.as_str().trim().trim_matches('"').to_string();
                let call_expression = line.trim().to_string();

                dependencies.push(CapabilityDependency {
                    capability_id,
                    call_expression,
                    line_number: line_num + 1,
                });
            }
        }
    }

    Ok(dependencies)
}

/// Check dependencies against marketplace snapshot
pub fn check_dependencies_against_marketplace(
    dependencies: &[CapabilityDependency],
    marketplace_snapshot: &[CapabilityManifest],
) -> (HashSet<String>, HashSet<String>) {
    let marketplace_ids: HashSet<String> = marketplace_snapshot
        .iter()
        .map(|manifest| manifest.id.trim().trim_matches('"').to_string())
        .collect();

    let dependency_ids: HashSet<String> = dependencies
        .iter()
        .map(|d| d.capability_id.trim().trim_matches('"').to_string())
        .collect();

    let resolved: HashSet<String> = dependency_ids
        .intersection(&marketplace_ids)
        .cloned()
        .collect();

    let missing: HashSet<String> = dependency_ids
        .difference(&marketplace_ids)
        .cloned()
        .collect();

    (resolved, missing)
}

/// Create audit event data for missing dependencies
pub fn create_audit_event_data(
    capability_id: &str,
    missing_dependencies: &HashSet<String>,
) -> HashMap<String, String> {
    let mut event_data = HashMap::new();
    event_data.insert(
        "event_type".to_string(),
        "capability_deps_missing".to_string(),
    );
    event_data.insert("capability_id".to_string(), capability_id.to_string());
    event_data.insert(
        "missing_count".to_string(),
        missing_dependencies.len().to_string(),
    );

    if !missing_dependencies.is_empty() {
        let missing_list: Vec<String> = missing_dependencies.iter().cloned().collect();
        event_data.insert("missing_capabilities".to_string(), missing_list.join(","));
    }

    event_data
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extract_dependencies_basic() {
        let rtfs_code = r#"
(do
  (let flights (call :travel.flights {:destination "Paris" :dates "2024-01-01"}))
  (let hotels (call :travel.hotels {:city "Paris" :budget 1000}))
  (let transport (call :travel.transport {:destination "Paris"}))
  {:status "completed" :flights flights :hotels hotels :transport transport})
"#;

        let result = extract_dependencies(rtfs_code).unwrap();

        assert_eq!(result.dependencies.len(), 3);
        assert!(result
            .dependencies
            .iter()
            .any(|d| d.capability_id == "travel.flights"));
        assert!(result
            .dependencies
            .iter()
            .any(|d| d.capability_id == "travel.hotels"));
        assert!(result
            .dependencies
            .iter()
            .any(|d| d.capability_id == "travel.transport"));

        assert_eq!(result.missing_dependencies.len(), 3);
        assert!(result.missing_dependencies.contains("travel.flights"));
        assert!(result.missing_dependencies.contains("travel.hotels"));
        assert!(result.missing_dependencies.contains("travel.transport"));

        assert_eq!(result.metadata.get("dependencies.total").unwrap(), "3");
        assert_eq!(result.metadata.get("dependencies.missing").unwrap(), "3");
        assert_eq!(result.metadata.get("dependencies.resolved").unwrap(), "0");
    }

    #[test]
    fn test_extract_dependencies_no_calls() {
        let rtfs_code = r#"
(do
  (let result {:status "completed" :message "No external calls"})
  result)
"#;

        let result = extract_dependencies(rtfs_code).unwrap();

        assert_eq!(result.dependencies.len(), 0);
        assert_eq!(result.missing_dependencies.len(), 0);
        assert_eq!(result.resolved_dependencies.len(), 0);
        assert_eq!(result.metadata.get("dependencies.total").unwrap(), "0");
    }

    #[test]
    fn test_extract_dependencies_with_nested_calls() {
        let rtfs_code = r#"
(do
  (let result (call :travel.itinerary {:days 5
                                      :attractions (call :travel.attractions {:city "Paris"})
                                      :hotels (call :travel.hotels {:city "Paris"})}))
  result)
"#;

        let result = extract_dependencies(rtfs_code).unwrap();

        // Should find all three calls even though they're nested
        assert_eq!(result.dependencies.len(), 3);
        assert!(result
            .dependencies
            .iter()
            .any(|d| d.capability_id == "travel.itinerary"));
        assert!(result
            .dependencies
            .iter()
            .any(|d| d.capability_id == "travel.attractions"));
        assert!(result
            .dependencies
            .iter()
            .any(|d| d.capability_id == "travel.hotels"));
    }

    #[test]
    fn test_check_dependencies_against_marketplace() {
        let dependencies = vec![
            CapabilityDependency {
                capability_id: "travel.flights".to_string(),
                call_expression: "(call :travel.flights {...})".to_string(),
                line_number: 1,
            },
            CapabilityDependency {
                capability_id: "travel.hotels".to_string(),
                call_expression: "(call :travel.hotels {...})".to_string(),
                line_number: 2,
            },
            CapabilityDependency {
                capability_id: "travel.transport".to_string(),
                call_expression: "(call :travel.transport {...})".to_string(),
                line_number: 3,
            },
        ];

        let marketplace_snapshot = vec![
            CapabilityManifest {
                id: "travel.flights".to_string(),
                name: "Flight Search".to_string(),
                description: "Search for flights".to_string(),
                version: "1.0.0".to_string(),
                provider: crate::capability_marketplace::types::ProviderType::Local(
                    crate::capability_marketplace::types::LocalCapability {
                        handler: std::sync::Arc::new(|_args| {
                            Ok(rtfs::runtime::values::Value::String(
                                "pending_resolution".to_string(),
                            ))
                        }),
                    },
                ),
                input_schema: None,
                output_schema: None,
                attestation: None,
                provenance: None,
                permissions: vec![],
                effects: vec![],
                metadata: HashMap::new(),
                agent_metadata: None,
                domains: Vec::new(),
                categories: Vec::new(),
                effect_type: EffectType::default(),
            },
            CapabilityManifest {
                id: "travel.hotels".to_string(),
                name: "Hotel Search".to_string(),
                description: "Search for hotels".to_string(),
                version: "1.0.0".to_string(),
                provider: crate::capability_marketplace::types::ProviderType::Local(
                    crate::capability_marketplace::types::LocalCapability {
                        handler: std::sync::Arc::new(|_args| {
                            Ok(rtfs::runtime::values::Value::String(
                                "pending_resolution".to_string(),
                            ))
                        }),
                    },
                ),
                input_schema: None,
                output_schema: None,
                attestation: None,
                provenance: None,
                permissions: vec![],
                effects: vec![],
                metadata: HashMap::new(),
                agent_metadata: None,
                domains: Vec::new(),
                categories: Vec::new(),
                effect_type: EffectType::default(),
            },
        ];

        let (resolved, missing) =
            check_dependencies_against_marketplace(&dependencies, &marketplace_snapshot);

        assert_eq!(resolved.len(), 2);
        assert!(resolved.contains("travel.flights"));
        assert!(resolved.contains("travel.hotels"));

        assert_eq!(missing.len(), 1);
        assert!(missing.contains("travel.transport"));
    }

    #[test]
    fn test_create_audit_event_data() {
        let missing_deps: HashSet<String> =
            ["travel.flights".to_string(), "travel.hotels".to_string()]
                .iter()
                .cloned()
                .collect();
        let event_data = create_audit_event_data("travel.trip-planner.paris.v1", &missing_deps);

        assert_eq!(
            event_data.get("event_type").unwrap(),
            "capability_deps_missing"
        );
        assert_eq!(
            event_data.get("capability_id").unwrap(),
            "travel.trip-planner.paris.v1"
        );
        assert_eq!(event_data.get("missing_count").unwrap(), "2");

        // Check that both capabilities are present (order may vary due to HashSet)
        let missing_list = event_data.get("missing_capabilities").unwrap();
        assert!(missing_list.contains("travel.flights"));
        assert!(missing_list.contains("travel.hotels"));
    }
}
