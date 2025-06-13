// RTFS Agent Profile Manager - Handles agent profile parsing and agent_card generation
// Converts between agent-profile RTFS documents and agent_card JSON structures

use std::collections::HashMap;
use std::fs;
use std::path::Path;

use crate::ast::*;
use crate::runtime::{RuntimeResult, RuntimeError, Value};
use crate::parser::parse_expression;
use super::types::*;

/// Manager for agent profiles and agent card generation
pub struct AgentProfileManager {
    /// Cache of parsed profiles
    profile_cache: HashMap<String, AgentProfile>,
}

impl AgentProfileManager {
    /// Create a new profile manager
    pub fn new() -> Self {
        Self {
            profile_cache: HashMap::new(),
        }
    }
    
    /// Load an agent profile from a file
    pub fn load_profile_from_file<P: AsRef<Path>>(&mut self, file_path: P) -> RuntimeResult<AgentProfile> {
        let path = file_path.as_ref();
        let path_str = path.to_string_lossy().to_string();
        
        // Check cache first
        if let Some(cached_profile) = self.profile_cache.get(&path_str) {
            return Ok(cached_profile.clone());
        }
        
        // Read file
        let content = fs::read_to_string(path)
            .map_err(|e| RuntimeError::AgentProfileError {
                message: format!("Failed to read profile file: {}", e),
                profile_uri: Some(path_str.clone()),
            })?;
            
        // Parse profile
        let profile = self.parse_profile_content(&content, Some(path_str.clone()))?;
        
        // Cache it
        self.profile_cache.insert(path_str, profile.clone());
        
        Ok(profile)
    }
    
    /// Load an agent profile from a string
    pub fn load_profile_from_string(&self, content: &str) -> RuntimeResult<AgentProfile> {
        self.parse_profile_content(content, None)
    }
    
    /// Generate an agent_card from an agent profile
    pub fn generate_agent_card(&self, profile: &AgentProfile) -> RuntimeResult<AgentCard> {
        let mut agent_card = AgentCard::new(
            profile.metadata.id.clone(),
            profile.metadata.name.clone(),
            profile.metadata.version.clone(),
            profile.metadata.description.clone(),
        );
        
        // Convert capabilities
        for capability in &profile.capabilities {
            let agent_capability = AgentCapability {
                capability_id: capability.id.clone(),
                description: capability.description.clone(),
                input_schema_ref: None, // Would be derived from input_schema
                output_schema_ref: None, // Would be derived from output_schema
                version: Some(capability.version.clone()),
                metadata: None,
            };
            agent_card.add_capability(agent_capability);
        }
        
        // Set communication
        agent_card.communication = profile.communication.clone();
        
        // Set discovery tags
        agent_card.discovery_tags = profile.metadata.tags.clone();
        
        // Set agent profile URI if available
        // This would typically be set when the profile is hosted somewhere
        
        // Set metadata
        if let Some(author) = &profile.metadata.author {
            agent_card.metadata.insert(
                "author".to_string(), 
                serde_json::Value::String(author.clone())
            );
        }
        
        if let Some(license) = &profile.metadata.license {
            agent_card.metadata.insert(
                "license".to_string(),
                serde_json::Value::String(license.clone())
            );
        }
        
        if let Some(created) = &profile.metadata.created {
            agent_card.metadata.insert(
                "created".to_string(),
                serde_json::Value::String(created.clone())
            );
        }
        
        if let Some(updated) = &profile.metadata.updated {
            agent_card.metadata.insert(
                "updated".to_string(),
                serde_json::Value::String(updated.clone())
            );
        }
        
        Ok(agent_card)
    }
    
    /// Validate an agent profile
    pub fn validate_profile(&self, profile: &AgentProfile) -> RuntimeResult<Vec<String>> {
        let mut warnings = Vec::new();
        
        // Check required fields
        if profile.metadata.id.is_empty() {
            return Err(RuntimeError::AgentProfileError {
                message: "Agent ID is required".to_string(),
                profile_uri: None,
            });
        }
        
        if profile.metadata.name.is_empty() {
            return Err(RuntimeError::AgentProfileError {
                message: "Agent name is required".to_string(),
                profile_uri: None,
            });
        }
        
        if profile.metadata.version.is_empty() {
            return Err(RuntimeError::AgentProfileError {
                message: "Agent version is required".to_string(),
                profile_uri: None,
            });
        }
        
        if profile.capabilities.is_empty() {
            warnings.push("Agent has no capabilities defined".to_string());
        }
        
        if profile.communication.endpoints.is_empty() {
            warnings.push("Agent has no communication endpoints defined".to_string());
        }
        
        // Validate capability IDs are unique
        let mut capability_ids = std::collections::HashSet::new();
        for capability in &profile.capabilities {
            if !capability_ids.insert(&capability.id) {
                return Err(RuntimeError::AgentProfileError {
                    message: format!("Duplicate capability ID: {}", capability.id),
                    profile_uri: None,
                });
            }
        }
        
        // Validate communication endpoints
        for endpoint in &profile.communication.endpoints {
            if endpoint.uri.is_empty() {
                return Err(RuntimeError::AgentProfileError {
                    message: "Communication endpoint URI cannot be empty".to_string(),
                    profile_uri: None,
                });
            }
            
            if endpoint.protocol.is_empty() {
                return Err(RuntimeError::AgentProfileError {
                    message: "Communication endpoint protocol cannot be empty".to_string(),
                    profile_uri: None,
                });
            }
        }
        
        Ok(warnings)
    }
    
    /// Create a minimal agent profile for testing
    pub fn create_minimal_profile(
        id: String,
        name: String,
        version: String,
        description: String
    ) -> AgentProfile {
        AgentProfile {
            metadata: AgentMetadata {
                id,
                name,
                version,
                description,
                author: None,
                license: None,
                tags: Vec::new(),
                created: None,
                updated: None,
            },
            capabilities: Vec::new(),
            communication: AgentCommunication {
                protocols: Vec::new(),
                endpoints: Vec::new(),
            },
            requirements: None,
            discovery: None,
            extensions: None,
        }
    }
    
    // Private helper methods
    
    fn parse_profile_content(&self, content: &str, profile_uri: Option<String>) -> RuntimeResult<AgentProfile> {
        // Parse the RTFS content
        let expr = parse_expression(content)
            .map_err(|e| RuntimeError::AgentProfileError {
                message: format!("Failed to parse profile: {:?}", e),
                profile_uri: profile_uri.clone(),
            })?;
            
        // Extract agent profile from the expression
        self.extract_profile_from_expression(&expr, profile_uri)
    }
    
    fn extract_profile_from_expression(&self, expr: &Expression, profile_uri: Option<String>) -> RuntimeResult<AgentProfile> {
        // For now, implement a simplified parser that expects a specific structure
        // In a full implementation, this would be more sophisticated
        
        match expr {
            Expression::Map(map_expr) => self.parse_profile_map(map_expr, profile_uri),
            Expression::Let(let_expr) => {
                // Look for agent-profile binding
                for binding in &let_expr.bindings {
                    if let Pattern::Symbol(symbol) = &binding.pattern {
                        if symbol.0 == "agent-profile" {
                            return self.extract_profile_from_expression(&binding.value, profile_uri);
                        }
                    }
                }
                Err(RuntimeError::AgentProfileError {
                    message: "No agent-profile binding found in let expression".to_string(),
                    profile_uri,
                })
            },
            _ => Err(RuntimeError::AgentProfileError {
                message: "Expected map or let expression for agent profile".to_string(),
                profile_uri,
            }),
        }
    }
    
    fn parse_profile_map(&self, map_expr: &HashMap<MapKey, Expression>, profile_uri: Option<String>) -> RuntimeResult<AgentProfile> {
        let mut metadata = None;
        let mut capabilities = Vec::new();
        let mut communication = None;
        let mut requirements = None;
        let mut discovery = None;
        let mut extensions = HashMap::new();
          for (key, value) in map_expr {
            match key {
                MapKey::Keyword(k) => {
                    match k.0.as_str() {                        "metadata" => {
                            metadata = Some(self.parse_metadata(value, &profile_uri)?);
                        },
                        "capabilities" => {
                            capabilities = self.parse_capabilities(value, &profile_uri)?;
                        },
                        "communication" => {
                            communication = Some(self.parse_communication(value, &profile_uri)?);
                        },
                        "requirements" => {
                            requirements = Some(self.parse_requirements(value, &profile_uri)?);
                        },
                        "discovery" => {
                            discovery = Some(self.parse_discovery(value, &profile_uri)?);
                        },
                        _ => {
                            // Unknown field - store in extensions
                            extensions.insert(k.0.clone(), Value::String("unknown".to_string()));
                        }
                    }
                },
                _ => {
                    return Err(RuntimeError::AgentProfileError {
                        message: "Profile map keys must be keywords".to_string(),
                        profile_uri,
                    });
                }
            }
        }
        
        let metadata = metadata.ok_or_else(|| RuntimeError::AgentProfileError {
            message: "Profile missing :metadata section".to_string(),
            profile_uri: profile_uri.clone(),
        })?;
        
        let communication = communication.ok_or_else(|| RuntimeError::AgentProfileError {
            message: "Profile missing :communication section".to_string(),
            profile_uri: profile_uri.clone(),
        })?;
        
        Ok(AgentProfile {
            metadata,
            capabilities,
            communication,
            requirements,
            discovery,
            extensions: if extensions.is_empty() { None } else { Some(extensions) },
        })
    }
    
    fn parse_metadata(&self, expr: &Expression, profile_uri: &Option<String>) -> RuntimeResult<AgentMetadata> {
        match expr {
            Expression::Map(map_expr) => {
                let mut id = None;
                let mut name = None;
                let mut version = None;
                let mut description = None;
                let mut author = None;
                let mut license = None;
                let mut tags = Vec::new();
                let mut created = None;
                let mut updated = None;
                
                for (key, value) in map_expr {                    if let MapKey::Keyword(k) = key {
                        match k.0.as_str() {
                            "id" => id = self.extract_string(value)?,
                            "name" => name = self.extract_string(value)?,
                            "version" => version = self.extract_string(value)?,
                            "description" => description = self.extract_string(value)?,
                            "author" => author = self.extract_string(value)?,
                            "license" => license = self.extract_string(value)?,
                            "tags" => tags = self.extract_string_vector(value)?,
                            "created" => created = self.extract_string(value)?,
                            "updated" => updated = self.extract_string(value)?,
                            _ => {} // Ignore unknown fields
                        }
                    }
                }
                
                Ok(AgentMetadata {
                    id: id.ok_or_else(|| RuntimeError::AgentProfileError {
                        message: "Missing :id in metadata".to_string(),
                        profile_uri: profile_uri.clone(),
                    })?,
                    name: name.ok_or_else(|| RuntimeError::AgentProfileError {
                        message: "Missing :name in metadata".to_string(),
                        profile_uri: profile_uri.clone(),
                    })?,
                    version: version.ok_or_else(|| RuntimeError::AgentProfileError {
                        message: "Missing :version in metadata".to_string(),
                        profile_uri: profile_uri.clone(),
                    })?,
                    description: description.ok_or_else(|| RuntimeError::AgentProfileError {
                        message: "Missing :description in metadata".to_string(),
                        profile_uri: profile_uri.clone(),
                    })?,
                    author,
                    license,
                    tags,
                    created,
                    updated,
                })
            },
            _ => Err(RuntimeError::AgentProfileError {
                message: "Metadata must be a map".to_string(),
                profile_uri: profile_uri.clone(),
            }),
        }
    }
    
    fn parse_capabilities(&self, expr: &Expression, _profile_uri: &Option<String>) -> RuntimeResult<Vec<ProfileCapability>> {
        // Simplified implementation - in practice would parse a vector of capability maps
        Ok(Vec::new())
    }
    
    fn parse_communication(&self, expr: &Expression, _profile_uri: &Option<String>) -> RuntimeResult<AgentCommunication> {
        // Simplified implementation - in practice would parse communication configuration
        Ok(AgentCommunication {
            protocols: vec!["http".to_string()],
            endpoints: vec![AgentEndpoint::new("http".to_string(), "http://localhost:8080".to_string())],
        })
    }
    
    fn parse_requirements(&self, _expr: &Expression, _profile_uri: &Option<String>) -> RuntimeResult<AgentRequirements> {
        // Simplified implementation
        Ok(AgentRequirements {
            runtime_version: None,
            dependencies: None,
            resources: None,
        })
    }
    
    fn parse_discovery(&self, _expr: &Expression, _profile_uri: &Option<String>) -> RuntimeResult<AgentDiscoveryConfig> {
        // Simplified implementation
        Ok(AgentDiscoveryConfig {
            registry_uris: None,
            ttl_seconds: None,
            auto_register: None,
            health_check_endpoint: None,
        })
    }
    
    fn extract_string(&self, expr: &Expression) -> RuntimeResult<Option<String>> {
        match expr {
            Expression::Literal(Literal::String(s)) => Ok(Some(s.clone())),
            _ => Ok(None),
        }
    }
    
    fn extract_string_vector(&self, expr: &Expression) -> RuntimeResult<Vec<String>> {
        match expr {
            Expression::Vector(vec_expr) => {
                let mut strings = Vec::new();
                for element in vec_expr {
                    if let Expression::Literal(Literal::String(s)) = element {
                        strings.push(s.clone());
                    }
                }
                Ok(strings)
            },
            _ => Ok(Vec::new()),
        }
    }
}

impl Default for AgentProfileManager {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_create_minimal_profile() {
        let profile = AgentProfileManager::create_minimal_profile(
            "test-agent".to_string(),
            "Test Agent".to_string(),
            "1.0.0".to_string(),
            "A test agent".to_string()
        );
        
        assert_eq!(profile.metadata.id, "test-agent");
        assert_eq!(profile.metadata.name, "Test Agent");
        assert_eq!(profile.metadata.version, "1.0.0");
        assert_eq!(profile.metadata.description, "A test agent");
    }
    
    #[test]
    fn test_generate_agent_card() {
        let manager = AgentProfileManager::new();
        let profile = AgentProfileManager::create_minimal_profile(
            "test-agent".to_string(),
            "Test Agent".to_string(),
            "1.0.0".to_string(),
            "A test agent".to_string()
        );
        
        let agent_card = manager.generate_agent_card(&profile).unwrap();
        
        assert_eq!(agent_card.agent_id, "test-agent");
        assert_eq!(agent_card.name, "Test Agent");
        assert_eq!(agent_card.version, "1.0.0");
        assert_eq!(agent_card.description, "A test agent");
    }
    
    #[test]
    fn test_validate_profile() {
        let manager = AgentProfileManager::new();
        let profile = AgentProfileManager::create_minimal_profile(
            "test-agent".to_string(),
            "Test Agent".to_string(),
            "1.0.0".to_string(),
            "A test agent".to_string()
        );
        
        let warnings = manager.validate_profile(&profile).unwrap();
        assert!(!warnings.is_empty()); // Should have warnings about no capabilities/endpoints
    }
}
