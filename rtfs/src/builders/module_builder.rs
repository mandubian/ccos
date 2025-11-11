use super::{BuilderError, ObjectBuilder};
use crate::ast::{ModuleDefinition, Symbol};
use std::collections::HashMap;

/// Fluent interface builder for RTFS 2.0 Module objects
#[derive(Debug, Clone)]
pub struct ModuleBuilder {
    name: String,
    exports: Vec<String>,
    dependencies: Vec<String>,
    version: Option<String>,
    metadata: HashMap<String, String>,
}

impl ModuleBuilder {
    /// Create a new ModuleBuilder with the given name
    pub fn new(name: &str) -> Self {
        Self {
            name: name.to_string(),
            exports: Vec::new(),
            dependencies: Vec::new(),
            version: None,
            metadata: HashMap::new(),
        }
    }

    /// Add an export
    pub fn with_export(mut self, export: &str) -> Self {
        self.exports.push(export.to_string());
        self
    }

    /// Add multiple exports
    pub fn with_exports(mut self, exports: Vec<String>) -> Self {
        self.exports.extend(exports);
        self
    }

    /// Add a dependency
    pub fn with_dependency(mut self, dependency: &str) -> Self {
        self.dependencies.push(dependency.to_string());
        self
    }

    /// Add multiple dependencies
    pub fn with_dependencies(mut self, dependencies: Vec<String>) -> Self {
        self.dependencies.extend(dependencies);
        self
    }

    /// Set the version
    pub fn with_version(mut self, version: &str) -> Self {
        self.version = Some(version.to_string());
        self
    }

    /// Add metadata key-value pair
    pub fn with_metadata(mut self, key: &str, value: &str) -> Self {
        self.metadata.insert(key.to_string(), value.to_string());
        self
    }

    /// Get suggestions for completing the module
    pub fn suggest_completion(&self) -> Vec<String> {
        let mut suggestions = Vec::new();
        if self.exports.is_empty() {
            suggestions.push("Add exports with .with_export(\"symbol\")".to_string());
        }
        if self.version.is_none() {
            suggestions.push("Set version with .with_version(\"1.0.0\")".to_string());
        }
        suggestions
    }
}


impl ObjectBuilder<ModuleDefinition> for ModuleBuilder {
    fn build(self) -> Result<ModuleDefinition, BuilderError> {
        if self.name.is_empty() {
            return Err(BuilderError::MissingField("name".to_string()));
        }

        // Convert exports to Vec<Symbol>
        let exports = if !self.exports.is_empty() {
            Some(self.exports.iter().map(|e| Symbol::new(e)).collect())
        } else {
            None
        };

        // For now, create an empty definitions vector
        // In a real implementation, this would be populated based on the builder's state
        let definitions = Vec::new();

        Ok(ModuleDefinition {
            name: Symbol::new(&self.name),
            docstring: None, // Could be set from metadata if needed
            exports,
            definitions,
        })
    }
    fn validate(&self) -> Result<(), Vec<String>> {
        let mut errors = Vec::new();
        if self.name.is_empty() {
            errors.push("Module name cannot be empty".to_string());
        }
        if errors.is_empty() {
            Ok(())
        } else {
            Err(errors)
        }
    }
    fn to_rtfs(&self) -> Result<String, BuilderError> {
        let mut rtfs = format!("module {} {{\n", self.name);
        rtfs.push_str(&format!("  name: \"{}\"\n", self.name));
        if !self.exports.is_empty() {
            let exports = self
                .exports
                .iter()
                .map(|e| format!("\"{}\"", e))
                .collect::<Vec<_>>()
                .join(" ");
            rtfs.push_str(&format!("  exports: [{}]\n", exports));
        }
        if !self.dependencies.is_empty() {
            let deps = self
                .dependencies
                .iter()
                .map(|d| format!("\"{}\"", d))
                .collect::<Vec<_>>()
                .join(" ");
            rtfs.push_str(&format!("  dependencies: [{}]\n", deps));
        }
        if let Some(version) = &self.version {
            rtfs.push_str(&format!("  version: \"{}\"\n", version));
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
    fn test_basic_module_builder() {
        let module = ModuleBuilder::new("test-module")
            .with_export("foo")
            .with_version("1.0.0")
            .build()
            .unwrap();
        assert_eq!(module.name.0, "test-module");
        assert!(module.exports.is_some());
        assert_eq!(module.exports.as_ref().unwrap().len(), 1);
        assert_eq!(module.exports.as_ref().unwrap()[0].0, "foo");
    }

    #[test]
    fn test_rtfs_generation() {
        let rtfs = ModuleBuilder::new("test-module")
            .with_export("foo")
            .with_dependency("bar")
            .to_rtfs()
            .unwrap();
        assert!(rtfs.contains("module test-module"));
        assert!(rtfs.contains("exports: [\"foo\"]"));
        assert!(rtfs.contains("dependencies: [\"bar\"]"));
    }

    #[test]
    fn test_validation() {
        let result = ModuleBuilder::new("").validate();
        assert!(result.is_err());
        assert!(result
            .unwrap_err()
            .contains(&"Module name cannot be empty".to_string()));
    }
}
