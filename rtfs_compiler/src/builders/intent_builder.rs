use super::{
    constraints_to_rtfs, validate_constraint, BuilderError, Constraint, ObjectBuilder, Priority,
    SuccessCriteria,
};
use crate::ast::{Expression, IntentDefinition, Keyword, Literal, MapKey, Property, Symbol};
use chrono::{DateTime, Utc};
use std::collections::HashMap;

/// Fluent interface builder for RTFS 2.0 Intent objects
pub struct IntentBuilder {
    name: String,
    goal: Option<String>,
    priority: Option<Priority>,
    constraints: Vec<Constraint>,
    success_criteria: Option<SuccessCriteria>,
    parent_intent: Option<String>,
    child_intents: Vec<String>,
    metadata: HashMap<String, String>,
    created_at: Option<DateTime<Utc>>,
    created_by: Option<String>,
    status: Option<String>,
}

impl std::fmt::Debug for IntentBuilder {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("IntentBuilder")
            .field("name", &self.name)
            .field("goal", &self.goal)
            .field("priority", &self.priority)
            .field("constraints", &self.constraints)
            .field("success_criteria", &"<function>")
            .field("parent_intent", &self.parent_intent)
            .field("child_intents", &self.child_intents)
            .field("metadata", &self.metadata)
            .field("created_at", &self.created_at)
            .field("created_by", &self.created_by)
            .field("status", &self.status)
            .finish()
    }
}

impl Clone for IntentBuilder {
    fn clone(&self) -> Self {
        Self {
            name: self.name.clone(),
            goal: self.goal.clone(),
            priority: self.priority.clone(),
            constraints: self.constraints.clone(),
            success_criteria: None, // Cannot clone function pointers
            parent_intent: self.parent_intent.clone(),
            child_intents: self.child_intents.clone(),
            metadata: self.metadata.clone(),
            created_at: self.created_at,
            created_by: self.created_by.clone(),
            status: self.status.clone(),
        }
    }
}

impl IntentBuilder {
    /// Create a new IntentBuilder with the given name
    pub fn new(name: &str) -> Self {
        Self {
            name: name.to_string(),
            goal: None,
            priority: None,
            constraints: Vec::new(),
            success_criteria: None,
            parent_intent: None,
            child_intents: Vec::new(),
            metadata: HashMap::new(),
            created_at: None,
            created_by: None,
            status: Some("active".to_string()),
        }
    }

    /// Set the goal of the intent
    pub fn with_goal(mut self, goal: &str) -> Self {
        self.goal = Some(goal.to_string());
        self
    }

    /// Set the priority level
    pub fn with_priority(mut self, priority: Priority) -> Self {
        self.priority = Some(priority);
        self
    }

    /// Add a constraint to the intent
    pub fn with_constraint(mut self, constraint: Constraint) -> Result<Self, BuilderError> {
        validate_constraint(&constraint)
            .map_err(|e| BuilderError::InvalidValue(constraint.name().to_string(), e))?;
        self.constraints.push(constraint);
        Ok(self)
    }

    /// Add multiple constraints at once
    pub fn with_constraints(mut self, constraints: Vec<Constraint>) -> Result<Self, BuilderError> {
        for constraint in &constraints {
            validate_constraint(constraint)
                .map_err(|e| BuilderError::InvalidValue(constraint.name().to_string(), e))?;
        }
        self.constraints.extend(constraints);
        Ok(self)
    }

    /// Set the success criteria function
    pub fn with_success_criteria<F>(mut self, criteria: F) -> Self
    where
        F: Fn(&HashMap<String, serde_json::Value>) -> bool + Send + Sync + 'static,
    {
        self.success_criteria = Some(Box::new(criteria));
        self
    }

    /// Set the parent intent ID
    pub fn with_parent_intent(mut self, parent_id: &str) -> Self {
        self.parent_intent = Some(parent_id.to_string());
        self
    }

    /// Add a child intent ID
    pub fn with_child_intent(mut self, child_id: &str) -> Self {
        self.child_intents.push(child_id.to_string());
        self
    }

    /// Add multiple child intent IDs
    pub fn with_child_intents(mut self, child_ids: Vec<String>) -> Self {
        self.child_intents.extend(child_ids);
        self
    }

    /// Add metadata key-value pair
    pub fn with_metadata(mut self, key: &str, value: &str) -> Self {
        self.metadata.insert(key.to_string(), value.to_string());
        self
    }

    /// Set the creation timestamp
    pub fn with_created_at(mut self, timestamp: DateTime<Utc>) -> Self {
        self.created_at = Some(timestamp);
        self
    }

    /// Set the creator
    pub fn with_created_by(mut self, creator: &str) -> Self {
        self.created_by = Some(creator.to_string());
        self
    }

    /// Set the status
    pub fn with_status(mut self, status: &str) -> Self {
        self.status = Some(status.to_string());
        self
    }

    /// Create an intent from natural language description
    pub fn from_natural_language(prompt: &str) -> Result<Self, BuilderError> {
        // Basic natural language parsing (can be enhanced with LLM integration)
        let prompt_lower = prompt.to_lowercase();

        let mut builder = IntentBuilder::new("auto-generated-intent");

        // Extract goal from prompt
        if let Some(goal_start) = prompt_lower.find("goal") {
            if let Some(goal_end) = prompt[goal_start..].find('.') {
                let goal = prompt[goal_start + 5..goal_start + goal_end].trim();
                builder = builder.with_goal(goal);
            }
        } else {
            // Use the entire prompt as goal if no explicit goal found
            builder = builder.with_goal(prompt);
        }

        // Extract priority
        if prompt_lower.contains("high priority") || prompt_lower.contains("urgent") {
            builder = builder.with_priority(Priority::High);
        } else if prompt_lower.contains("low priority") {
            builder = builder.with_priority(Priority::Low);
        } else if prompt_lower.contains("critical") {
            builder = builder.with_priority(Priority::Critical);
        } else {
            builder = builder.with_priority(Priority::Medium);
        }

        // Extract cost constraints
        if let Some(cost_start) = prompt_lower.find("$") {
            if let Some(cost_end) = prompt_lower[cost_start..].find(' ') {
                if let Ok(cost) = prompt_lower[cost_start + 1..cost_start + cost_end].parse::<f64>()
                {
                    builder = builder.with_constraint(Constraint::MaxCost(cost))?;
                }
            }
        }

        // Extract deadline constraints
        if prompt_lower.contains("by") || prompt_lower.contains("deadline") {
            // Simple deadline extraction (can be enhanced)
            let deadline = "2025-12-31T23:59:59Z".to_string();
            builder = builder.with_constraint(Constraint::Deadline(deadline))?;
        }

        Ok(builder)
    }

    /// Get suggestions for completing the intent
    pub fn suggest_completion(&self) -> Vec<String> {
        let mut suggestions = Vec::new();

        if self.goal.is_none() {
            suggestions.push("Add a goal with .with_goal(\"your goal here\")".to_string());
        }

        if self.priority.is_none() {
            suggestions
                .push("Set priority with .with_priority(Priority::High/Medium/Low)".to_string());
        }

        if self.constraints.is_empty() {
            suggestions.push(
                "Add constraints like .with_constraint(Constraint::MaxCost(25.0))".to_string(),
            );
        }

        if self.success_criteria.is_none() {
            suggestions
                .push("Add success criteria with .with_success_criteria(|result| ...)".to_string());
        }

        suggestions
    }

    /// Validate the current state for LLM generation
    pub fn validate_llm_generated(&self) -> Result<(), Vec<String>> {
        let mut errors = Vec::new();

        if self.name.is_empty() {
            errors.push("Intent name cannot be empty".to_string());
        }

        if self.goal.is_none() {
            errors.push("Goal is required for LLM-generated intents".to_string());
        }

        if self.priority.is_none() {
            errors.push("Priority is required for LLM-generated intents".to_string());
        }

        // Validate constraints
        for constraint in &self.constraints {
            if let Err(e) = validate_constraint(constraint) {
                errors.push(format!("Invalid constraint: {}", e));
            }
        }

        if errors.is_empty() {
            Ok(())
        } else {
            Err(errors)
        }
    }
}

impl ObjectBuilder<IntentDefinition> for IntentBuilder {
    fn build(self) -> Result<IntentDefinition, BuilderError> {
        // Validate required fields
        if self.name.is_empty() {
            return Err(BuilderError::MissingField("name".to_string()));
        }

        if self.goal.is_none() {
            return Err(BuilderError::MissingField("goal".to_string()));
        }

        // Convert constraints to properties
        let mut properties = vec![
            Property {
                key: Keyword::new("name"),
                value: Expression::Literal(Literal::String(self.name.clone())),
            },
            Property {
                key: Keyword::new("goal"),
                value: Expression::Literal(Literal::String(self.goal.unwrap())),
            },
        ];

        // Add optional properties
        if let Some(priority) = self.priority {
            properties.push(Property {
                key: Keyword::new("priority"),
                value: Expression::Literal(Literal::Keyword(Keyword::new(&priority.to_string()))),
            });
        }

        if !self.constraints.is_empty() {
            // Convert constraints to a map expression
            let constraint_props: Vec<Property> = self
                .constraints
                .iter()
                .map(|c| Property {
                    key: Keyword::new(c.name()),
                    value: Expression::Literal(Literal::String(c.value())),
                })
                .collect();

            properties.push(Property {
                key: Keyword::new("constraints"),
                value: Expression::Map(properties_to_map(constraint_props)),
            });
        }

        if let Some(parent) = self.parent_intent {
            properties.push(Property {
                key: Keyword::new("parent-intent"),
                value: Expression::Literal(Literal::String(parent)),
            });
        }

        if !self.child_intents.is_empty() {
            let child_values: Vec<Expression> = self
                .child_intents
                .iter()
                .map(|child| Expression::Literal(Literal::String(child.clone())))
                .collect();

            properties.push(Property {
                key: Keyword::new("child-intents"),
                value: Expression::Vector(child_values),
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

        if let Some(created_at) = self.created_at {
            properties.push(Property {
                key: Keyword::new("created-at"),
                value: Expression::Literal(Literal::String(created_at.to_rfc3339())),
            });
        }

        if let Some(created_by) = self.created_by {
            properties.push(Property {
                key: Keyword::new("created-by"),
                value: Expression::Literal(Literal::String(created_by)),
            });
        }

        if let Some(status) = self.status {
            properties.push(Property {
                key: Keyword::new("status"),
                value: Expression::Literal(Literal::Keyword(Keyword::new(&status))),
            });
        }

        Ok(IntentDefinition {
            name: Symbol::new(&self.name),
            properties,
        })
    }

    fn validate(&self) -> Result<(), Vec<String>> {
        let mut errors = Vec::new();

        if self.name.is_empty() {
            errors.push("Intent name cannot be empty".to_string());
        }

        if self.goal.is_none() {
            errors.push("Goal is required".to_string());
        }

        // Validate constraints
        for constraint in &self.constraints {
            if let Err(e) = validate_constraint(constraint) {
                errors.push(format!("Invalid constraint: {}", e));
            }
        }

        if errors.is_empty() {
            Ok(())
        } else {
            Err(errors)
        }
    }

    fn to_rtfs(&self) -> Result<String, BuilderError> {
        let mut rtfs = format!("intent {} {{\n", self.name);

        // Required fields
        rtfs.push_str(&format!("  name: \"{}\"\n", self.name));

        if let Some(goal) = &self.goal {
            rtfs.push_str(&format!("  goal: \"{}\"\n", goal));
        }

        // Optional fields
        if let Some(priority) = &self.priority {
            rtfs.push_str(&format!("  priority: :{}\n", priority));
        }

        if !self.constraints.is_empty() {
            rtfs.push_str(&format!(
                "  constraints: {}\n",
                constraints_to_rtfs(&self.constraints)
            ));
        }

        if let Some(parent) = &self.parent_intent {
            rtfs.push_str(&format!("  parent-intent: \"{}\"\n", parent));
        }

        if !self.child_intents.is_empty() {
            let children_str = self
                .child_intents
                .iter()
                .map(|child| format!("\"{}\"", child))
                .collect::<Vec<_>>()
                .join(" ");
            rtfs.push_str(&format!("  child-intents: [{}]\n", children_str));
        }

        if !self.metadata.is_empty() {
            rtfs.push_str("  metadata: {\n");
            for (key, value) in &self.metadata {
                rtfs.push_str(&format!("    {}: \"{}\"\n", key, value));
            }
            rtfs.push_str("  }\n");
        }

        if let Some(created_at) = &self.created_at {
            rtfs.push_str(&format!("  created-at: \"{}\"\n", created_at.to_rfc3339()));
        }

        if let Some(created_by) = &self.created_by {
            rtfs.push_str(&format!("  created-by: \"{}\"\n", created_by));
        }

        if let Some(status) = &self.status {
            rtfs.push_str(&format!("  status: :{}\n", status));
        }

        rtfs.push_str("}");

        Ok(rtfs)
    }
}

// Helper to convert Vec<Property> to HashMap<MapKey, Expression>
fn properties_to_map(props: Vec<Property>) -> HashMap<MapKey, Expression> {
    props
        .into_iter()
        .map(|p| (MapKey::Keyword(p.key), p.value))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_basic_intent_builder() {
        let intent = IntentBuilder::new("test-intent")
            .with_goal("Test the builder")
            .with_priority(Priority::High)
            .with_constraint(Constraint::MaxCost(25.0))
            .unwrap()
            .build()
            .unwrap();

        println!("Properties: {:?}", intent.properties);
        println!("Property count: {}", intent.properties.len());

        assert_eq!(intent.name.0, "test-intent");
        assert_eq!(intent.properties.len(), 5); // name, goal, priority, constraints, status
    }

    #[test]
    fn test_natural_language_parsing() {
        let intent = IntentBuilder::from_natural_language(
            "Analyze sales data with high priority and $50 budget",
        )
        .unwrap();

        assert!(intent.goal.is_some());
        assert_eq!(intent.priority, Some(Priority::High));
        assert_eq!(intent.constraints.len(), 1);
    }

    #[test]
    fn test_rtfs_generation() {
        let rtfs = IntentBuilder::new("test-intent")
            .with_goal("Test RTFS generation")
            .with_priority(Priority::Medium)
            .to_rtfs()
            .unwrap();

        assert!(rtfs.contains("intent test-intent"));
        assert!(rtfs.contains("goal: \"Test RTFS generation\""));
        assert!(rtfs.contains("priority: :medium"));
    }

    #[test]
    fn test_validation() {
        let result = IntentBuilder::new("")
            .with_goal("Test validation")
            .validate();

        assert!(result.is_err());
        assert!(result
            .unwrap_err()
            .contains(&"Intent name cannot be empty".to_string()));
    }
}
