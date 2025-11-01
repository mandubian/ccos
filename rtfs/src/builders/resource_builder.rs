use super::{BuilderError, ObjectBuilder};
use crate::ast::{Expression, Keyword, Literal, MapKey, Property, ResourceDefinition, Symbol};
use std::collections::HashMap;

/// Fluent interface builder for RTFS 2.0 Resource objects
#[derive(Debug, Clone)]
pub struct ResourceBuilder {
    name: String,
    resource_type: Option<String>,
    access_control: Option<AccessControl>,
    lifecycle: Option<ResourceLifecycle>,
    properties: HashMap<String, String>,
    metadata: HashMap<String, String>,
}

/// Access control configuration
#[derive(Debug, Clone)]
pub struct AccessControl {
    pub owner: String,
    pub permissions: Vec<String>,
    pub public: bool,
}

/// Resource lifecycle management
#[derive(Debug, Clone)]
pub struct ResourceLifecycle {
    pub created_at: String,
    pub expires_at: Option<String>,
    pub deleted_at: Option<String>,
    pub status: String,
}

impl ResourceBuilder {
    /// Create a new ResourceBuilder with the given name
    pub fn new(name: &str) -> Self {
        Self {
            name: name.to_string(),
            resource_type: None,
            access_control: None,
            lifecycle: None,
            properties: HashMap::new(),
            metadata: HashMap::new(),
        }
    }

    /// Set the resource type
    pub fn with_type(mut self, resource_type: &str) -> Self {
        self.resource_type = Some(resource_type.to_string());
        self
    }

    /// Set access control
    pub fn with_access_control(mut self, ac: AccessControl) -> Self {
        self.access_control = Some(ac);
        self
    }

    /// Set lifecycle
    pub fn with_lifecycle(mut self, lifecycle: ResourceLifecycle) -> Self {
        self.lifecycle = Some(lifecycle);
        self
    }

    /// Add a property
    pub fn with_property(mut self, key: &str, value: &str) -> Self {
        self.properties.insert(key.to_string(), value.to_string());
        self
    }

    /// Add metadata key-value pair
    pub fn with_metadata(mut self, key: &str, value: &str) -> Self {
        self.metadata.insert(key.to_string(), value.to_string());
        self
    }

    /// Get suggestions for completing the resource
    pub fn suggest_completion(&self) -> Vec<String> {
        let mut suggestions = Vec::new();
        if self.resource_type.is_none() {
            suggestions.push("Set resource type with .with_type(\"type\")".to_string());
        }
        if self.access_control.is_none() {
            suggestions.push("Set access control with .with_access_control(...)".to_string());
        }
        if self.lifecycle.is_none() {
            suggestions.push("Set lifecycle with .with_lifecycle(...)".to_string());
        }
        suggestions
    }
}

impl AccessControl {
    pub fn new(owner: &str, permissions: Vec<String>, public: bool) -> Self {
        Self {
            owner: owner.to_string(),
            permissions,
            public,
        }
    }
}

impl ResourceLifecycle {
    pub fn new(created_at: &str, status: &str) -> Self {
        Self {
            created_at: created_at.to_string(),
            expires_at: None,
            deleted_at: None,
            status: status.to_string(),
        }
    }
    pub fn with_expires_at(mut self, expires_at: &str) -> Self {
        self.expires_at = Some(expires_at.to_string());
        self
    }
    pub fn with_deleted_at(mut self, deleted_at: &str) -> Self {
        self.deleted_at = Some(deleted_at.to_string());
        self
    }
}

// Helper to convert Vec<Property> to HashMap<MapKey, Expression>
fn properties_to_map(props: Vec<Property>) -> HashMap<MapKey, Expression> {
    props
        .into_iter()
        .map(|p| (MapKey::Keyword(p.key), p.value))
        .collect()
}

impl ObjectBuilder<ResourceDefinition> for ResourceBuilder {
    fn build(self) -> Result<ResourceDefinition, BuilderError> {
        if self.name.is_empty() {
            return Err(BuilderError::MissingField("name".to_string()));
        }
        if self.resource_type.is_none() {
            return Err(BuilderError::MissingField("resource_type".to_string()));
        }
        let mut properties = vec![
            Property {
                key: Keyword::new("name"),
                value: Expression::Literal(Literal::String(self.name.clone())),
            },
            Property {
                key: Keyword::new("type"),
                value: Expression::Literal(Literal::String(self.resource_type.unwrap())),
            },
        ];
        if let Some(ac) = self.access_control {
            let ac_props = vec![
                Property {
                    key: Keyword::new("owner"),
                    value: Expression::Literal(Literal::String(ac.owner)),
                },
                Property {
                    key: Keyword::new("public"),
                    value: Expression::Literal(Literal::Boolean(ac.public)),
                },
                Property {
                    key: Keyword::new("permissions"),
                    value: Expression::Vector(
                        ac.permissions
                            .iter()
                            .map(|p| Expression::Literal(Literal::String(p.clone())))
                            .collect(),
                    ),
                },
            ];
            properties.push(Property {
                key: Keyword::new("access-control"),
                value: Expression::Map(properties_to_map(ac_props)),
            });
        }
        if let Some(lc) = self.lifecycle {
            let mut lc_props = vec![
                Property {
                    key: Keyword::new("created-at"),
                    value: Expression::Literal(Literal::String(lc.created_at)),
                },
                Property {
                    key: Keyword::new("status"),
                    value: Expression::Literal(Literal::String(lc.status)),
                },
            ];
            if let Some(expires_at) = lc.expires_at {
                lc_props.push(Property {
                    key: Keyword::new("expires-at"),
                    value: Expression::Literal(Literal::String(expires_at)),
                });
            }
            if let Some(deleted_at) = lc.deleted_at {
                lc_props.push(Property {
                    key: Keyword::new("deleted-at"),
                    value: Expression::Literal(Literal::String(deleted_at)),
                });
            }
            properties.push(Property {
                key: Keyword::new("lifecycle"),
                value: Expression::Map(properties_to_map(lc_props)),
            });
        }
        if !self.properties.is_empty() {
            let prop_props: Vec<Property> = self
                .properties
                .iter()
                .map(|(k, v)| Property {
                    key: Keyword::new(k),
                    value: Expression::Literal(Literal::String(v.clone())),
                })
                .collect();
            properties.push(Property {
                key: Keyword::new("properties"),
                value: Expression::Map(properties_to_map(prop_props)),
            });
        }
        if !self.metadata.is_empty() {
            let metadata_props: Vec<Property> = self
                .metadata
                .iter()
                .map(|(k, v)| Property {
                    key: Keyword::new(k),
                    value: Expression::Literal(Literal::String(v.clone())),
                })
                .collect();
            properties.push(Property {
                key: Keyword::new("metadata"),
                value: Expression::Map(properties_to_map(metadata_props)),
            });
        }
        Ok(ResourceDefinition {
            name: Symbol::new(&self.name),
            properties,
        })
    }
    fn validate(&self) -> Result<(), Vec<String>> {
        let mut errors = Vec::new();
        if self.name.is_empty() {
            errors.push("Resource name cannot be empty".to_string());
        }
        if self.resource_type.is_none() {
            errors.push("Resource type is required".to_string());
        }
        if errors.is_empty() {
            Ok(())
        } else {
            Err(errors)
        }
    }
    fn to_rtfs(&self) -> Result<String, BuilderError> {
        let mut rtfs = format!("resource {} {{\n", self.name);
        rtfs.push_str(&format!("  name: \"{}\"\n", self.name));
        if let Some(resource_type) = &self.resource_type {
            rtfs.push_str(&format!("  type: \"{}\"\n", resource_type));
        }
        if let Some(ac) = &self.access_control {
            rtfs.push_str("  access-control: {\n");
            rtfs.push_str(&format!("    owner: \"{}\"\n", ac.owner));
            rtfs.push_str(&format!("    public: {}\n", ac.public));
            let perms = ac
                .permissions
                .iter()
                .map(|p| format!("\"{}\"", p))
                .collect::<Vec<_>>()
                .join(" ");
            rtfs.push_str(&format!("    permissions: [{}]\n", perms));
            rtfs.push_str("  }\n");
        }
        if let Some(lc) = &self.lifecycle {
            rtfs.push_str("  lifecycle: {\n");
            rtfs.push_str(&format!("    created-at: \"{}\"\n", lc.created_at));
            rtfs.push_str(&format!("    status: \"{}\"\n", lc.status));
            if let Some(expires_at) = &lc.expires_at {
                rtfs.push_str(&format!("    expires-at: \"{}\"\n", expires_at));
            }
            if let Some(deleted_at) = &lc.deleted_at {
                rtfs.push_str(&format!("    deleted-at: \"{}\"\n", deleted_at));
            }
            rtfs.push_str("  }\n");
        }
        if !self.properties.is_empty() {
            rtfs.push_str("  properties: {\n");
            for (k, v) in &self.properties {
                rtfs.push_str(&format!("    {}: \"{}\"\n", k, v));
            }
            rtfs.push_str("  }\n");
        }
        if !self.metadata.is_empty() {
            rtfs.push_str("  metadata: {\n");
            for (k, v) in &self.metadata {
                rtfs.push_str(&format!("    {}: \"{}\"\n", k, v));
            }
            rtfs.push_str("  }\n");
        }
        rtfs.push_str("}");
        Ok(rtfs)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_basic_resource_builder() {
        let ac = AccessControl::new(
            "user1",
            vec!["read".to_string(), "write".to_string()],
            false,
        );
        let lc = ResourceLifecycle::new("2025-01-01T00:00:00Z", "active");
        let resource = ResourceBuilder::new("test-resource")
            .with_type("file")
            .with_access_control(ac)
            .with_lifecycle(lc)
            .with_property("path", "/tmp/test.txt")
            .build()
            .unwrap();
        assert_eq!(resource.name.0, "test-resource");
        assert_eq!(resource.properties.len(), 5); // name, type, access-control, lifecycle, properties
    }

    #[test]
    fn test_rtfs_generation() {
        let ac = AccessControl::new("user1", vec!["read".to_string()], true);
        let rtfs = ResourceBuilder::new("test-resource")
            .with_type("file")
            .with_access_control(ac)
            .to_rtfs()
            .unwrap();
        assert!(rtfs.contains("resource test-resource"));
        assert!(rtfs.contains("type: \"file\""));
        assert!(rtfs.contains("owner: \"user1\""));
    }

    #[test]
    fn test_validation() {
        let result = ResourceBuilder::new("").with_type("file").validate();
        assert!(result.is_err());
        assert!(result
            .unwrap_err()
            .contains(&"Resource name cannot be empty".to_string()));
    }
}
