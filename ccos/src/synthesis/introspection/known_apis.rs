//! Known APIs loader
//!
//! This module loads pre-built API definitions from the `capabilities/known_apis/` directory.
//! These definitions are used as a fallback when OpenAPI discovery fails.

use crate::synthesis::introspection::{
    APIIntrospectionResult, AuthRequirements, DiscoveredEndpoint, EndpointParameter, RateLimitInfo,
};
use rtfs::ast::{Keyword, MapTypeEntry, PrimitiveType, TypeExpr};
use rtfs::runtime::error::{RuntimeError, RuntimeResult};
use serde::Deserialize;
use std::collections::HashMap;
use std::path::{Path, PathBuf};

/// Known API definition loaded from TOML
#[derive(Debug, Clone, Deserialize)]
pub struct KnownApiDefinition {
    pub api: ApiMetadata,
    pub auth: Option<AuthConfig>,
    pub rate_limits: Option<RateLimitsConfig>,
    #[serde(default)]
    pub endpoints: Vec<EndpointDefinition>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ApiMetadata {
    pub name: String,
    pub title: String,
    pub base_url: String,
    pub version: String,
    pub description: String,
    #[serde(default)]
    pub domains: Vec<String>,
    pub documentation_url: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct AuthConfig {
    #[serde(rename = "type")]
    pub auth_type: String,
    #[serde(default)]
    pub location: String,
    #[serde(default)]
    pub param_name: String,
    pub env_var: Option<String>,
    #[serde(default)]
    pub required: bool,
}

#[derive(Debug, Clone, Deserialize)]
pub struct RateLimitsConfig {
    pub requests_per_minute: Option<u32>,
    pub requests_per_day: Option<u32>,
    pub requests_per_second: Option<u32>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct EndpointDefinition {
    pub id: String,
    pub name: String,
    pub description: String,
    pub method: String,
    pub path: String,
    #[serde(default)]
    pub params: Vec<ParamDefinition>,
    #[serde(default)]
    pub subscription_required: bool,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ParamDefinition {
    pub name: String,
    #[serde(rename = "type")]
    pub param_type: String,
    pub location: String,
    #[serde(default)]
    pub required: bool,
    pub description: Option<String>,
    pub default: Option<String>,
}

/// Known APIs registry
pub struct KnownApisRegistry {
    /// Loaded API definitions by name
    apis: HashMap<String, KnownApiDefinition>,
    /// Domain to API name mapping
    domain_index: HashMap<String, Vec<String>>,
}

impl KnownApisRegistry {
    /// Create a new registry and load APIs from the default directory
    pub fn new() -> RuntimeResult<Self> {
        let mut registry = Self {
            apis: HashMap::new(),
            domain_index: HashMap::new(),
        };

        // Try to load from capabilities/known_apis/
        let paths = [
            PathBuf::from("capabilities/known_apis"),
            PathBuf::from("../capabilities/known_apis"),
            PathBuf::from("../../capabilities/known_apis"),
        ];

        for path in &paths {
            if path.exists() {
                registry.load_from_directory(path)?;
                break;
            }
        }

        Ok(registry)
    }

    /// Load all API definitions from a directory
    pub fn load_from_directory(&mut self, dir: &Path) -> RuntimeResult<()> {
        if !dir.exists() {
            return Ok(());
        }

        let entries = std::fs::read_dir(dir).map_err(|e| {
            RuntimeError::Generic(format!("Failed to read known_apis directory: {}", e))
        })?;

        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().map_or(false, |ext| ext == "toml") {
                if let Err(e) = self.load_api_file(&path) {
                    log::warn!("Failed to load API definition {:?}: {}", path, e);
                }
            }
        }

        Ok(())
    }

    /// Load a single API definition file
    fn load_api_file(&mut self, path: &Path) -> RuntimeResult<()> {
        let content = std::fs::read_to_string(path).map_err(|e| {
            RuntimeError::Generic(format!("Failed to read {:?}: {}", path, e))
        })?;

        let api: KnownApiDefinition = toml::from_str(&content).map_err(|e| {
            RuntimeError::Generic(format!("Failed to parse {:?}: {}", path, e))
        })?;

        let name = api.api.name.clone();

        // Index by domains
        for domain in &api.api.domains {
            self.domain_index
                .entry(domain.to_lowercase())
                .or_default()
                .push(name.clone());
        }

        // Also index by name
        self.domain_index
            .entry(name.to_lowercase())
            .or_default()
            .push(name.clone());

        self.apis.insert(name, api);

        Ok(())
    }

    /// Find an API by domain hint (e.g., "openweathermap", "weather")
    pub fn find_by_domain(&self, domain: &str) -> Option<&KnownApiDefinition> {
        let domain_lower = domain.to_lowercase();

        // Direct name match
        if let Some(api) = self.apis.get(&domain_lower) {
            return Some(api);
        }

        // Check domain index
        if let Some(names) = self.domain_index.get(&domain_lower) {
            if let Some(name) = names.first() {
                return self.apis.get(name);
            }
        }

        // Partial match in API names
        for (name, api) in &self.apis {
            if domain_lower.contains(name) || name.contains(&domain_lower) {
                return Some(api);
            }
        }

        None
    }

    /// Search APIs by keyword
    pub fn search(&self, query: &str) -> Vec<&KnownApiDefinition> {
        let query_lower = query.to_lowercase();
        let mut results = Vec::new();

        for api in self.apis.values() {
            let matches = api.api.name.to_lowercase().contains(&query_lower)
                || api.api.title.to_lowercase().contains(&query_lower)
                || api.api.description.to_lowercase().contains(&query_lower)
                || api.api.domains.iter().any(|d| d.to_lowercase().contains(&query_lower));

            if matches {
                results.push(api);
            }
        }

        results
    }

    /// Convert a known API definition to introspection result
    pub fn to_introspection_result(
        &self,
        api: &KnownApiDefinition,
    ) -> RuntimeResult<APIIntrospectionResult> {
        let endpoints = api
            .endpoints
            .iter()
            .map(|ep| self.convert_endpoint(ep))
            .collect::<RuntimeResult<Vec<_>>>()?;

        let auth_requirements = match &api.auth {
            Some(auth) => AuthRequirements {
                auth_type: auth.auth_type.clone(),
                auth_location: auth.location.clone(),
                auth_param_name: auth.param_name.clone(),
                required: auth.required,
                env_var_name: auth.env_var.clone(),
            },
            None => AuthRequirements {
                auth_type: "none".to_string(),
                auth_location: String::new(),
                auth_param_name: String::new(),
                required: false,
                env_var_name: None,
            },
        };

        let rate_limits = api.rate_limits.as_ref().map(|rl| RateLimitInfo {
            requests_per_minute: rl.requests_per_minute,
            requests_per_day: rl.requests_per_day,
            requests_per_second: rl.requests_per_second,
        });

        Ok(APIIntrospectionResult {
            base_url: api.api.base_url.clone(),
            api_title: api.api.title.clone(),
            api_version: api.api.version.clone(),
            endpoints,
            auth_requirements,
            rate_limits,
        })
    }

    /// Convert an endpoint definition to discovered endpoint
    fn convert_endpoint(&self, ep: &EndpointDefinition) -> RuntimeResult<DiscoveredEndpoint> {
        let parameters: Vec<EndpointParameter> = ep
            .params
            .iter()
            .map(|p| EndpointParameter {
                name: p.name.clone(),
                param_type: self.parse_type(&p.param_type),
                required: p.required,
                location: p.location.clone(),
                description: p.description.clone(),
            })
            .collect();

        // Build input schema from parameters
        let input_entries: Vec<MapTypeEntry> = ep
            .params
            .iter()
            .map(|p| MapTypeEntry {
                key: Keyword(p.name.clone()),
                value_type: Box::new(self.parse_type(&p.param_type)),
                optional: !p.required,
            })
            .collect();

        let input_schema = if input_entries.is_empty() {
            None
        } else {
            Some(TypeExpr::Map {
                entries: input_entries,
                wildcard: None,
            })
        };

        Ok(DiscoveredEndpoint {
            endpoint_id: ep.id.clone(),
            name: ep.name.clone(),
            description: ep.description.clone(),
            method: ep.method.clone(),
            path: ep.path.clone(),
            input_schema,
            output_schema: None, // Would need response schema in TOML
            requires_auth: true, // Assume auth required if API has auth config
            parameters,
        })
    }

    /// Parse a type string to TypeExpr
    fn parse_type(&self, type_str: &str) -> TypeExpr {
        match type_str.to_lowercase().as_str() {
            "string" => TypeExpr::Primitive(PrimitiveType::String),
            "int" | "integer" => TypeExpr::Primitive(PrimitiveType::Int),
            "float" | "number" | "double" => TypeExpr::Primitive(PrimitiveType::Float),
            "bool" | "boolean" => TypeExpr::Primitive(PrimitiveType::Bool),
            _ => TypeExpr::Primitive(PrimitiveType::String), // Default to string
        }
    }

    /// List all loaded APIs
    pub fn list_apis(&self) -> Vec<&KnownApiDefinition> {
        self.apis.values().collect()
    }
}

impl Default for KnownApisRegistry {
    fn default() -> Self {
        Self::new().unwrap_or_else(|_| Self {
            apis: HashMap::new(),
            domain_index: HashMap::new(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_load_openweathermap() {
        let registry = KnownApisRegistry::new().unwrap();
        
        if let Some(api) = registry.find_by_domain("openweathermap") {
            assert_eq!(api.api.name, "openweathermap");
            assert!(!api.endpoints.is_empty());
        }
    }

    #[test]
    fn test_search_by_domain() {
        let registry = KnownApisRegistry::new().unwrap();
        
        let results = registry.search("weather");
        // Should find openweathermap
        assert!(results.iter().any(|api| api.api.name == "openweathermap"));
    }
}
