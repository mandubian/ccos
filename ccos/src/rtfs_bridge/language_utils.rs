//! Language Tagging Utilities
//! 
//! This module provides utilities for validating and normalizing language tags
//! for Plans and Capabilities.

use super::errors::RtfsBridgeError;
use crate::types::PlanLanguage;
use rtfs::ast::{MapKey, Symbol};
use rtfs::runtime::values::Value;

/// Canonical language identifier strings
pub mod canonical_languages {
    pub const RTFS20: &str = "rtfs20";
    pub const WASM: &str = "wasm";
    pub const PYTHON: &str = "python";
    pub const GRAPHJSON: &str = "graphjson";
}

/// Convert PlanLanguage to canonical language string
pub fn plan_language_to_string(lang: &PlanLanguage) -> String {
    match lang {
        PlanLanguage::Rtfs20 => canonical_languages::RTFS20.to_string(),
        PlanLanguage::Wasm => canonical_languages::WASM.to_string(),
        PlanLanguage::Python => canonical_languages::PYTHON.to_string(),
        PlanLanguage::GraphJson => canonical_languages::GRAPHJSON.to_string(),
        PlanLanguage::Other(ref s) => s.clone(),
    }
}

/// Parse language string to PlanLanguage
pub fn parse_language_string(lang: &str) -> Result<PlanLanguage, RtfsBridgeError> {
    match lang.to_lowercase().trim() {
        "rtfs" | "rtfs20" | "rtfs2.0" | "rtfs 2.0" => Ok(PlanLanguage::Rtfs20),
        "wasm" => Ok(PlanLanguage::Wasm),
        "python" | "py" => Ok(PlanLanguage::Python),
        "graph" | "graphjson" | "graph-json" => Ok(PlanLanguage::GraphJson),
        other => Ok(PlanLanguage::Other(other.to_string())),
    }
}

/// Extract language from a capability map
/// 
/// Returns the language string if present, or None if not found.
pub fn extract_language_from_capability_map(
    cap_map: &std::collections::HashMap<MapKey, Value>,
) -> Option<String> {
    let lang_key = MapKey::String(":language".to_string());
    
    if let Some(lang_val) = cap_map.get(&lang_key) {
        match lang_val {
            Value::String(s) => Some(s.clone()),
            Value::Keyword(k) => Some(format!(":{}", k.0)),
            Value::Symbol(Symbol(s)) => Some(s.clone()),
            _ => None,
        }
    } else {
        None
    }
}

/// Extract provider from a capability map
pub fn extract_provider_from_capability_map(
    cap_map: &std::collections::HashMap<MapKey, Value>,
) -> Option<String> {
    let provider_key = MapKey::String(":provider".to_string());
    
    if let Some(provider_val) = cap_map.get(&provider_key) {
        match provider_val {
            Value::String(s) => Some(s.clone()),
            Value::Keyword(k) => Some(format!(":{}", k.0)),
            Value::Symbol(Symbol(s)) => Some(s.clone()),
            _ => None,
        }
    } else {
        None
    }
}

/// Validate that a language string is valid
pub fn validate_language_string(lang: &str) -> Result<(), RtfsBridgeError> {
    // Language should not be empty
    if lang.trim().is_empty() {
        return Err(RtfsBridgeError::ValidationFailed {
            message: "Language string cannot be empty".to_string(),
        });
    }

    // Language should be a valid identifier (alphanumeric, dots, hyphens, underscores)
    if !lang.chars().all(|c| c.is_alphanumeric() || c == '.' || c == '-' || c == '_') {
        return Err(RtfsBridgeError::ValidationFailed {
            message: format!("Invalid language string format: '{}' (must be alphanumeric with dots, hyphens, or underscores)", lang),
        });
    }

    Ok(())
}

/// Validate that a local capability has a language field
/// 
/// Local capabilities MUST have a `:language` field to indicate how to execute
/// the implementation.
pub fn validate_local_capability_has_language(
    cap_map: &std::collections::HashMap<MapKey, Value>,
) -> Result<(), RtfsBridgeError> {
    let provider = extract_provider_from_capability_map(cap_map)
        .unwrap_or_else(|| "Local".to_string());
    
    // Only validate for local capabilities
    if provider.to_lowercase() == "local" {
        if extract_language_from_capability_map(cap_map).is_none() {
            return Err(RtfsBridgeError::ValidationFailed {
                message: "Local capabilities must have a :language field to specify implementation language".to_string(),
            });
        }
    }

    Ok(())
}

/// Ensure language is set for local capabilities
/// 
/// If a local capability doesn't have a language, this function will attempt
/// to infer it from the implementation or set a default.
pub fn ensure_language_for_local_capability(
    cap_map: &mut std::collections::HashMap<MapKey, Value>,
    default_language: Option<&str>,
) -> Result<(), RtfsBridgeError> {
    let provider = extract_provider_from_capability_map(cap_map)
        .unwrap_or_else(|| "Local".to_string());
    
    // Only enforce for local capabilities
    if provider.to_lowercase() == "local" {
        if extract_language_from_capability_map(cap_map).is_none() {
            // Set default language
            let lang = default_language
                .unwrap_or(canonical_languages::RTFS20)
                .to_string();
            
            validate_language_string(&lang)?;
            
            cap_map.insert(
                MapKey::String(":language".to_string()),
                Value::String(lang),
            );
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use rtfs::ast::MapKey;
    use rtfs::runtime::values::Value;
    use std::collections::HashMap;

    #[test]
    fn test_plan_language_to_string() {
        assert_eq!(plan_language_to_string(&PlanLanguage::Rtfs20), "rtfs20");
        assert_eq!(plan_language_to_string(&PlanLanguage::Wasm), "wasm");
        assert_eq!(plan_language_to_string(&PlanLanguage::Python), "python");
        assert_eq!(plan_language_to_string(&PlanLanguage::GraphJson), "graphjson");
    }

    #[test]
    fn test_parse_language_string() {
        assert!(matches!(
            parse_language_string("rtfs20").unwrap(),
            PlanLanguage::Rtfs20
        ));
        assert!(matches!(
            parse_language_string("wasm").unwrap(),
            PlanLanguage::Wasm
        ));
        assert!(matches!(
            parse_language_string("python").unwrap(),
            PlanLanguage::Python
        ));
    }

    #[test]
    fn test_validate_language_string() {
        assert!(validate_language_string("rtfs20").is_ok());
        assert!(validate_language_string("wasm").is_ok());
        assert!(validate_language_string("python-3.11").is_ok());
        assert!(validate_language_string("custom.lang").is_ok());
        assert!(validate_language_string("").is_err());
        assert!(validate_language_string("invalid lang").is_err());
    }

    #[test]
    fn test_validate_local_capability_has_language() {
        let mut cap_map = HashMap::new();
        cap_map.insert(
            MapKey::String(":provider".to_string()),
            Value::String("Local".to_string()),
        );
        cap_map.insert(
            MapKey::String(":language".to_string()),
            Value::String("rtfs20".to_string()),
        );

        assert!(validate_local_capability_has_language(&cap_map).is_ok());

        // Remove language - should fail
        cap_map.remove(&MapKey::String(":language".to_string()));
        assert!(validate_local_capability_has_language(&cap_map).is_err());
    }

    #[test]
    fn test_ensure_language_for_local_capability() {
        let mut cap_map = HashMap::new();
        cap_map.insert(
            MapKey::String(":provider".to_string()),
            Value::String("Local".to_string()),
        );

        // Should add default language
        ensure_language_for_local_capability(&mut cap_map, None).unwrap();
        assert!(extract_language_from_capability_map(&cap_map).is_some());
        assert_eq!(
            extract_language_from_capability_map(&cap_map).unwrap(),
            "rtfs20"
        );
    }
}

