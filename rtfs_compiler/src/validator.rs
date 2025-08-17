use crate::ast::{
    ActionDefinition, CapabilityDefinition, IntentDefinition, PlanDefinition, Property,
    ResourceDefinition, TopLevel,
};
use crate::error_reporting::ValidationError;
use std::collections::HashMap;
use validator::Validate;

// Schema validation for RTFS 2.0 objects
pub struct SchemaValidator;

impl SchemaValidator {
    /// Validate an RTFS 2.0 object against its schema
    pub fn validate_object(toplevel: &TopLevel) -> Result<(), ValidationError> {
        // First run basic validator::Validate
        match toplevel.validate() {
            Ok(_) => {}
            Err(e) => {
                let type_name = match toplevel {
                    TopLevel::Intent(_) => "Intent",
                    TopLevel::Plan(_) => "Plan",
                    TopLevel::Action(_) => "Action",
                    TopLevel::Capability(_) => "Capability",
                    TopLevel::Resource(_) => "Resource",
                    TopLevel::Module(_) => "Module",
                    TopLevel::Expression(_) => "Expression",
                };
                return Err(ValidationError::SchemaError {
                    type_name: type_name.to_string(),
                    errors: e,
                });
            }
        }

        // Then run schema-specific validation
        match toplevel {
            TopLevel::Intent(def) => Self::validate_intent(def),
            TopLevel::Plan(def) => Self::validate_plan(def),
            TopLevel::Action(def) => Self::validate_action(def),
            TopLevel::Capability(def) => Self::validate_capability(def),
            TopLevel::Resource(def) => Self::validate_resource(def),
            TopLevel::Module(_) => Ok(()), // Module validation is simpler
            TopLevel::Expression(_) => Ok(()), // Expression validation handled by validator::Validate
        }
    }

    /// Validate Intent object against schema
    fn validate_intent(def: &IntentDefinition) -> Result<(), ValidationError> {
        let mut errors = Vec::new();

        // Validate name (should be a versioned type identifier)
        if !Self::is_valid_versioned_type(&def.name.0) {
            errors.push(format!(
                "Intent name '{}' is not a valid versioned type identifier",
                def.name.0
            ));
        }

        // Validate required properties
        let properties = Self::properties_to_map(&def.properties);

        // Check for required fields
        let required_fields = [
            "type",
            "intent-id",
            "goal",
            "created-at",
            "created-by",
            "status",
        ];
        for field in &required_fields {
            if !properties.contains_key(&field.to_string()) {
                errors.push(format!("Intent missing required field: {}", field));
            }
        }

        // Validate type field
        if let Some(type_prop) = properties.get("type") {
            if !Self::is_valid_type_field(type_prop, ":rtfs.core:v2.0:intent") {
                errors.push("Intent type field must be ':rtfs.core:v2.0:intent'".to_string());
            }
        }

        // Validate status enum
        if let Some(status_prop) = properties.get("status") {
            let valid_statuses = [
                "draft",
                "active",
                "paused",
                "completed",
                "failed",
                "archived",
            ];
            if !Self::is_valid_enum_value(status_prop, &valid_statuses) {
                errors.push(format!(
                    "Invalid intent status: must be one of {:?}",
                    valid_statuses
                ));
            }
        }

        // Validate priority enum if present
        if let Some(priority_prop) = properties.get("priority") {
            let valid_priorities = ["low", "normal", "high", "urgent", "critical"];
            if !Self::is_valid_enum_value(priority_prop, &valid_priorities) {
                errors.push(format!(
                    "Invalid intent priority: must be one of {:?}",
                    valid_priorities
                ));
            }
        }

        if !errors.is_empty() {
            return Err(ValidationError::SchemaError {
                type_name: "Intent".to_string(),
                errors: validator::ValidationErrors::new(),
            });
        }

        Ok(())
    }

    /// Validate Plan object against schema
    fn validate_plan(def: &PlanDefinition) -> Result<(), ValidationError> {
        let mut errors = Vec::new();

        // Validate name
        if !Self::is_valid_versioned_type(&def.name.0) {
            errors.push(format!(
                "Plan name '{}' is not a valid versioned type identifier",
                def.name.0
            ));
        }

        let properties = Self::properties_to_map(&def.properties);

        // Check required fields
        let required_fields = [
            "type",
            "plan-id",
            "created-at",
            "created-by",
            "intent-ids",
            "program",
            "status",
        ];
        for field in &required_fields {
            if !properties.contains_key(&field.to_string()) {
                errors.push(format!("Plan missing required field: {}", field));
            }
        }

        // Validate type field
        if let Some(type_prop) = properties.get("type") {
            if !Self::is_valid_type_field(type_prop, ":rtfs.core:v2.0:plan") {
                errors.push("Plan type field must be ':rtfs.core:v2.0:plan'".to_string());
            }
        }

        // Validate status enum
        if let Some(status_prop) = properties.get("status") {
            let valid_statuses = [
                "draft",
                "ready",
                "executing",
                "completed",
                "failed",
                "cancelled",
            ];
            if !Self::is_valid_enum_value(status_prop, &valid_statuses) {
                errors.push(format!(
                    "Invalid plan status: must be one of {:?}",
                    valid_statuses
                ));
            }
        }

        // Validate strategy enum if present
        if let Some(strategy_prop) = properties.get("strategy") {
            let valid_strategies = [
                "sequential",
                "parallel",
                "hybrid",
                "cost-optimized",
                "speed-optimized",
                "reliability-optimized",
            ];
            if !Self::is_valid_enum_value(strategy_prop, &valid_strategies) {
                errors.push(format!(
                    "Invalid plan strategy: must be one of {:?}",
                    valid_strategies
                ));
            }
        }

        if !errors.is_empty() {
            return Err(ValidationError::SchemaError {
                type_name: "Plan".to_string(),
                errors: validator::ValidationErrors::new(),
            });
        }

        Ok(())
    }

    /// Validate Action object against schema
    fn validate_action(def: &ActionDefinition) -> Result<(), ValidationError> {
        let mut errors = Vec::new();

        // Validate name
        if !Self::is_valid_versioned_type(&def.name.0) {
            errors.push(format!(
                "Action name '{}' is not a valid versioned type identifier",
                def.name.0
            ));
        }

        let properties = Self::properties_to_map(&def.properties);

        // Check required fields
        let required_fields = [
            "type",
            "action-id",
            "timestamp",
            "plan-id",
            "step-id",
            "intent-id",
            "capability-used",
            "executor",
            "input",
            "output",
            "execution",
            "signature",
        ];
        for field in &required_fields {
            if !properties.contains_key(&field.to_string()) {
                errors.push(format!("Action missing required field: {}", field));
            }
        }

        // Validate type field
        if let Some(type_prop) = properties.get("type") {
            if !Self::is_valid_type_field(type_prop, ":rtfs.core:v2.0:action") {
                errors.push("Action type field must be ':rtfs.core:v2.0:action'".to_string());
            }
        }

        if !errors.is_empty() {
            return Err(ValidationError::SchemaError {
                type_name: "Action".to_string(),
                errors: validator::ValidationErrors::new(),
            });
        }

        Ok(())
    }

    /// Validate Capability object against schema
    fn validate_capability(def: &CapabilityDefinition) -> Result<(), ValidationError> {
        let mut errors = Vec::new();

        // Validate name
        if !Self::is_valid_versioned_type(&def.name.0) {
            errors.push(format!(
                "Capability name '{}' is not a valid versioned type identifier",
                def.name.0
            ));
        }

        let properties = Self::properties_to_map(&def.properties);

        // Check required fields
        let required_fields = [
            "type",
            "capability-id",
            "name",
            "version",
            "created-at",
            "created-by",
            "interface",
            "implementation",
        ];
        for field in &required_fields {
            if !properties.contains_key(&field.to_string()) {
                errors.push(format!("Capability missing required field: {}", field));
            }
        }

        // Validate type field
        if let Some(type_prop) = properties.get("type") {
            if !Self::is_valid_type_field(type_prop, ":rtfs.core:v2.0:capability") {
                errors
                    .push("Capability type field must be ':rtfs.core:v2.0:capability'".to_string());
            }
        }

        if !errors.is_empty() {
            return Err(ValidationError::SchemaError {
                type_name: "Capability".to_string(),
                errors: validator::ValidationErrors::new(),
            });
        }

        Ok(())
    }

    /// Validate Resource object against schema
    fn validate_resource(def: &ResourceDefinition) -> Result<(), ValidationError> {
        let mut errors = Vec::new();

        // Validate name
        if !Self::is_valid_versioned_type(&def.name.0) {
            errors.push(format!(
                "Resource name '{}' is not a valid versioned type identifier",
                def.name.0
            ));
        }

        let properties = Self::properties_to_map(&def.properties);

        // Check required fields
        let required_fields = [
            "type",
            "resource-id",
            "uri",
            "created-at",
            "created-by",
            "access-control",
        ];
        for field in &required_fields {
            if !properties.contains_key(&field.to_string()) {
                errors.push(format!("Resource missing required field: {}", field));
            }
        }

        // Validate type field
        if let Some(type_prop) = properties.get("type") {
            if !Self::is_valid_type_field(type_prop, ":rtfs.core:v2.0:resource") {
                errors.push("Resource type field must be ':rtfs.core:v2.0:resource'".to_string());
            }
        }

        if !errors.is_empty() {
            return Err(ValidationError::SchemaError {
                type_name: "Resource".to_string(),
                errors: validator::ValidationErrors::new(),
            });
        }

        Ok(())
    }

    // Helper methods for validation

    /// Convert properties vector to HashMap for easier lookup
    pub fn properties_to_map(properties: &[Property]) -> HashMap<String, &crate::ast::Expression> {
        properties
            .iter()
            .map(|prop| (prop.key.0.clone(), &prop.value))
            .collect()
    }

    /// Validate versioned type identifier pattern
    pub fn is_valid_versioned_type(s: &str) -> bool {
        // Pattern: :namespace:version:type
        let parts: Vec<&str> = s.split(':').collect();
        if parts.len() != 4 || parts[0] != "" {
            return false;
        }

        let namespace = parts[1];
        let version = parts[2];
        let type_name = parts[3];

        // Validate namespace: alphanumeric, dots, underscores, hyphens
        if !namespace
            .chars()
            .all(|c| c.is_alphanumeric() || c == '.' || c == '_' || c == '-')
        {
            return false;
        }

        // Validate version: v followed by numbers and dots
        if !version.starts_with('v') || !version[1..].chars().all(|c| c.is_numeric() || c == '.') {
            return false;
        }

        // Validate type name: alphanumeric, underscores, hyphens
        if !type_name
            .chars()
            .all(|c| c.is_alphanumeric() || c == '_' || c == '-')
        {
            return false;
        }

        true
    }

    /// Validate type field matches expected value
    pub fn is_valid_type_field(expr: &crate::ast::Expression, expected: &str) -> bool {
        match expr {
            crate::ast::Expression::Literal(crate::ast::Literal::Keyword(kw)) => kw.0 == expected,
            _ => false,
        }
    }

    /// Validate enum value
    pub fn is_valid_enum_value(expr: &crate::ast::Expression, valid_values: &[&str]) -> bool {
        match expr {
            crate::ast::Expression::Literal(crate::ast::Literal::String(s)) => {
                valid_values.contains(&s.as_str())
            }
            _ => false,
        }
    }
}

// Legacy function for backward compatibility
pub fn validate_toplevel(toplevel: &TopLevel) -> Result<(), ValidationError> {
    SchemaValidator::validate_object(toplevel)
}
