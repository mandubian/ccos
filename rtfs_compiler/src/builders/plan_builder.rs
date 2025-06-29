use super::{
    constraints_to_rtfs, validate_constraint, BuilderError, Constraint, ObjectBuilder, Priority,
};
use crate::ast::{Expression, Keyword, Literal, MapKey, PlanDefinition, Property, Symbol};
use std::collections::HashMap;

/// Fluent interface builder for RTFS 2.0 Plan objects
#[derive(Debug, Clone)]
pub struct PlanBuilder {
    name: String,
    intent_id: Option<String>,
    steps: Vec<PlanStep>,
    priority: Option<Priority>,
    constraints: Vec<Constraint>,
    estimated_cost: Option<f64>,
    estimated_duration: Option<u64>, // seconds
    dependencies: Vec<String>,
    metadata: HashMap<String, String>,
}

/// Builder for individual plan steps
#[derive(Debug, Clone)]
pub struct PlanStep {
    pub id: String,
    pub action_id: String,
    pub parameters: HashMap<String, String>,
    pub dependencies: Vec<String>,
    pub estimated_cost: Option<f64>,
    pub estimated_duration: Option<u64>,
    pub retry_policy: Option<RetryPolicy>,
}

/// Retry policy for plan steps
#[derive(Debug, Clone)]
pub struct RetryPolicy {
    pub max_retries: u32,
    pub backoff_factor: f64,
    pub max_delay: u64, // seconds
}

impl PlanBuilder {
    /// Create a new PlanBuilder with the given name
    pub fn new(name: &str) -> Self {
        Self {
            name: name.to_string(),
            intent_id: None,
            steps: Vec::new(),
            priority: None,
            constraints: Vec::new(),
            estimated_cost: None,
            estimated_duration: None,
            dependencies: Vec::new(),
            metadata: HashMap::new(),
        }
    }

    /// Set the intent ID this plan implements
    pub fn for_intent(mut self, intent_id: &str) -> Self {
        self.intent_id = Some(intent_id.to_string());
        self
    }

    /// Add a step to the plan
    pub fn with_step(mut self, step: PlanStep) -> Self {
        self.steps.push(step);
        self
    }

    /// Add multiple steps at once
    pub fn with_steps(mut self, steps: Vec<PlanStep>) -> Self {
        self.steps.extend(steps);
        self
    }

    /// Set the priority level
    pub fn with_priority(mut self, priority: Priority) -> Self {
        self.priority = Some(priority);
        self
    }

    /// Add a constraint to the plan
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

    /// Set estimated cost
    pub fn with_estimated_cost(mut self, cost: f64) -> Result<Self, BuilderError> {
        if cost < 0.0 {
            return Err(BuilderError::InvalidValue(
                "estimated_cost".to_string(),
                "Cost cannot be negative".to_string(),
            ));
        }
        self.estimated_cost = Some(cost);
        Ok(self)
    }

    /// Set estimated duration in seconds
    pub fn with_estimated_duration(mut self, duration: u64) -> Self {
        self.estimated_duration = Some(duration);
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

    /// Add metadata key-value pair
    pub fn with_metadata(mut self, key: &str, value: &str) -> Self {
        self.metadata.insert(key.to_string(), value.to_string());
        self
    }

    /// Validate step dependencies
    pub fn validate_step_dependencies(&self) -> Result<(), Vec<String>> {
        let mut errors = Vec::new();
        let step_ids: std::collections::HashSet<_> = self.steps.iter().map(|s| &s.id).collect();

        for step in &self.steps {
            for dep in &step.dependencies {
                if !step_ids.contains(dep) {
                    errors.push(format!(
                        "Step '{}' depends on unknown step '{}'",
                        step.id, dep
                    ));
                }
            }
        }

        if errors.is_empty() {
            Ok(())
        } else {
            Err(errors)
        }
    }

    /// Get suggestions for completing the plan
    pub fn suggest_completion(&self) -> Vec<String> {
        let mut suggestions = Vec::new();

        if self.intent_id.is_none() {
            suggestions.push("Link to an intent with .for_intent(\"intent-id\")".to_string());
        }

        if self.steps.is_empty() {
            suggestions.push("Add steps with .with_step(PlanStep::new(...))".to_string());
        }

        if self.priority.is_none() {
            suggestions
                .push("Set priority with .with_priority(Priority::High/Medium/Low)".to_string());
        }

        if self.estimated_cost.is_none() {
            suggestions.push("Set estimated cost with .with_estimated_cost(25.0)".to_string());
        }

        suggestions
    }
}

impl PlanStep {
    /// Create a new plan step
    pub fn new(id: &str, action_id: &str) -> Self {
        Self {
            id: id.to_string(),
            action_id: action_id.to_string(),
            parameters: HashMap::new(),
            dependencies: Vec::new(),
            estimated_cost: None,
            estimated_duration: None,
            retry_policy: None,
        }
    }

    /// Add a parameter to the step
    pub fn with_parameter(mut self, key: &str, value: &str) -> Self {
        self.parameters.insert(key.to_string(), value.to_string());
        self
    }

    /// Add multiple parameters
    pub fn with_parameters(mut self, params: HashMap<String, String>) -> Self {
        self.parameters.extend(params);
        self
    }

    /// Add a dependency
    pub fn with_dependency(mut self, step_id: &str) -> Self {
        self.dependencies.push(step_id.to_string());
        self
    }

    /// Add multiple dependencies
    pub fn with_dependencies(mut self, step_ids: Vec<String>) -> Self {
        self.dependencies.extend(step_ids);
        self
    }

    /// Set estimated cost
    pub fn with_estimated_cost(mut self, cost: f64) -> Self {
        self.estimated_cost = Some(cost);
        self
    }

    /// Set estimated duration in seconds
    pub fn with_estimated_duration(mut self, duration: u64) -> Self {
        self.estimated_duration = Some(duration);
        self
    }

    /// Set retry policy
    pub fn with_retry_policy(mut self, policy: RetryPolicy) -> Self {
        self.retry_policy = Some(policy);
        self
    }
}

impl RetryPolicy {
    /// Create a new retry policy
    pub fn new(max_retries: u32, backoff_factor: f64, max_delay: u64) -> Self {
        Self {
            max_retries,
            backoff_factor,
            max_delay,
        }
    }
}

impl ObjectBuilder<PlanDefinition> for PlanBuilder {
    fn build(self) -> Result<PlanDefinition, BuilderError> {
        // Validate required fields
        if self.name.is_empty() {
            return Err(BuilderError::MissingField("name".to_string()));
        }

        if self.intent_id.is_none() {
            return Err(BuilderError::MissingField("intent_id".to_string()));
        }

        if self.steps.is_empty() {
            return Err(BuilderError::MissingField("steps".to_string()));
        }

        // Validate step dependencies
        self.validate_step_dependencies()
            .map_err(|e| BuilderError::Validation(e.join(", ")))?;

        // Convert to properties
        let mut properties = vec![
            Property {
                key: Keyword::new("name"),
                value: Expression::Literal(Literal::String(self.name.clone())),
            },
            Property {
                key: Keyword::new("intent-id"),
                value: Expression::Literal(Literal::String(self.intent_id.unwrap())),
            },
        ];

        // Add steps
        let step_expressions: Vec<Expression> = self
            .steps
            .iter()
            .map(|step| {
                let mut step_props = vec![
                    Property {
                        key: Keyword::new("id"),
                        value: Expression::Literal(Literal::String(step.id.clone())),
                    },
                    Property {
                        key: Keyword::new("action-id"),
                        value: Expression::Literal(Literal::String(step.action_id.clone())),
                    },
                ];

                if !step.parameters.is_empty() {
                    let param_props: Vec<Property> = step
                        .parameters
                        .iter()
                        .map(|(k, v)| Property {
                            key: Keyword::new(k),
                            value: Expression::Literal(Literal::String(v.clone())),
                        })
                        .collect();
                    step_props.push(Property {
                        key: Keyword::new("parameters"),
                        value: Expression::Map(properties_to_map(param_props)),
                    });
                }

                if !step.dependencies.is_empty() {
                    let dep_values: Vec<Expression> = step
                        .dependencies
                        .iter()
                        .map(|dep| Expression::Literal(Literal::String(dep.clone())))
                        .collect();
                    step_props.push(Property {
                        key: Keyword::new("dependencies"),
                        value: Expression::Vector(dep_values.into_iter().collect()),
                    });
                }

                if let Some(cost) = step.estimated_cost {
                    step_props.push(Property {
                        key: Keyword::new("estimated-cost"),
                        value: Expression::Literal(Literal::Float(cost)),
                    });
                }

                if let Some(duration) = step.estimated_duration {
                    step_props.push(Property {
                        key: Keyword::new("estimated-duration"),
                        value: Expression::Literal(Literal::Integer(duration as i64)),
                    });
                }

                if let Some(policy) = &step.retry_policy {
                    let policy_props = vec![
                        Property {
                            key: Keyword::new("max-retries"),
                            value: Expression::Literal(Literal::Integer(policy.max_retries as i64)),
                        },
                        Property {
                            key: Keyword::new("backoff-factor"),
                            value: Expression::Literal(Literal::Float(policy.backoff_factor)),
                        },
                        Property {
                            key: Keyword::new("max-delay"),
                            value: Expression::Literal(Literal::Integer(policy.max_delay as i64)),
                        },
                    ];
                    step_props.push(Property {
                        key: Keyword::new("retry-policy"),
                        value: Expression::Map(properties_to_map(policy_props)),
                    });
                }

                Expression::Map(properties_to_map(step_props))
            })
            .collect();

        properties.push(Property {
            key: Keyword::new("steps"),
            value: Expression::Vector(step_expressions.into_iter().collect()),
        });

        // Add optional properties
        if let Some(priority) = self.priority {
            properties.push(Property {
                key: Keyword::new("priority"),
                value: Expression::Literal(Literal::Keyword(Keyword::new(&priority.to_string()))),
            });
        }

        if !self.constraints.is_empty() {
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

        if let Some(cost) = self.estimated_cost {
            properties.push(Property {
                key: Keyword::new("estimated-cost"),
                value: Expression::Literal(Literal::Float(cost)),
            });
        }

        if let Some(duration) = self.estimated_duration {
            properties.push(Property {
                key: Keyword::new("estimated-duration"),
                value: Expression::Literal(Literal::Integer(duration as i64)),
            });
        }

        if !self.dependencies.is_empty() {
            let dep_values: Vec<Expression> = self
                .dependencies
                .iter()
                .map(|dep| Expression::Literal(Literal::String(dep.clone())))
                .collect();

            properties.push(Property {
                key: Keyword::new("dependencies"),
                value: Expression::Vector(dep_values.into_iter().collect()),
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

        Ok(PlanDefinition {
            name: Symbol::new(&self.name),
            properties,
        })
    }

    fn validate(&self) -> Result<(), Vec<String>> {
        let mut errors = Vec::new();

        if self.name.is_empty() {
            errors.push("Plan name cannot be empty".to_string());
        }

        if self.intent_id.is_none() {
            errors.push("Intent ID is required".to_string());
        }

        if self.steps.is_empty() {
            errors.push("At least one step is required".to_string());
        }

        // Validate constraints
        for constraint in &self.constraints {
            if let Err(e) = validate_constraint(constraint) {
                errors.push(format!("Invalid constraint: {}", e));
            }
        }

        // Validate step dependencies
        if let Err(step_errors) = self.validate_step_dependencies() {
            errors.extend(step_errors);
        }

        if errors.is_empty() {
            Ok(())
        } else {
            Err(errors)
        }
    }

    fn to_rtfs(&self) -> Result<String, BuilderError> {
        let mut rtfs = format!("plan {} {{\n", self.name);

        // Required fields
        rtfs.push_str(&format!("  name: \"{}\"\n", self.name));

        if let Some(intent_id) = &self.intent_id {
            rtfs.push_str(&format!("  intent-id: \"{}\"\n", intent_id));
        }

        // Steps
        rtfs.push_str("  steps: [\n");
        for step in &self.steps {
            rtfs.push_str(&format!("    {{\n"));
            rtfs.push_str(&format!("      id: \"{}\"\n", step.id));
            rtfs.push_str(&format!("      action-id: \"{}\"\n", step.action_id));

            if !step.parameters.is_empty() {
                rtfs.push_str("      parameters: {\n");
                for (key, value) in &step.parameters {
                    rtfs.push_str(&format!("        {}: \"{}\"\n", key, value));
                }
                rtfs.push_str("      }\n");
            }

            if !step.dependencies.is_empty() {
                let deps_str = step
                    .dependencies
                    .iter()
                    .map(|dep| format!("\"{}\"", dep))
                    .collect::<Vec<_>>()
                    .join(" ");
                rtfs.push_str(&format!("      dependencies: [{}]\n", deps_str));
            }

            if let Some(cost) = step.estimated_cost {
                rtfs.push_str(&format!("      estimated-cost: {}\n", cost));
            }

            if let Some(duration) = step.estimated_duration {
                rtfs.push_str(&format!("      estimated-duration: {}\n", duration));
            }

            if let Some(policy) = &step.retry_policy {
                rtfs.push_str("      retry-policy: {\n");
                rtfs.push_str(&format!("        max-retries: {}\n", policy.max_retries));
                rtfs.push_str(&format!(
                    "        backoff-factor: {}\n",
                    policy.backoff_factor
                ));
                rtfs.push_str(&format!("        max-delay: {}\n", policy.max_delay));
                rtfs.push_str("      }\n");
            }

            rtfs.push_str("    }\n");
        }
        rtfs.push_str("  ]\n");

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

        if let Some(cost) = self.estimated_cost {
            rtfs.push_str(&format!("  estimated-cost: {}\n", cost));
        }

        if let Some(duration) = self.estimated_duration {
            rtfs.push_str(&format!("  estimated-duration: {}\n", duration));
        }

        if !self.dependencies.is_empty() {
            let deps_str = self
                .dependencies
                .iter()
                .map(|dep| format!("\"{}\"", dep))
                .collect::<Vec<_>>()
                .join(" ");
            rtfs.push_str(&format!("  dependencies: [{}]\n", deps_str));
        }

        if !self.metadata.is_empty() {
            rtfs.push_str("  metadata: {\n");
            for (key, value) in &self.metadata {
                rtfs.push_str(&format!("    {}: \"{}\"\n", key, value));
            }
            rtfs.push_str("  }\n");
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
    fn test_basic_plan_builder() {
        let step = PlanStep::new("step1", "action1")
            .with_parameter("input", "data.csv")
            .with_estimated_cost(10.0);

        let plan = PlanBuilder::new("test-plan")
            .for_intent("test-intent")
            .with_step(step)
            .with_priority(Priority::High)
            .with_estimated_cost(25.0)
            .unwrap()
            .build()
            .unwrap();

        assert_eq!(plan.name.0, "test-plan");
        assert_eq!(plan.properties.len(), 5); // name, intent-id, steps, priority, estimated-cost
    }

    #[test]
    fn test_plan_with_dependencies() {
        let step1 = PlanStep::new("step1", "action1");
        let step2 = PlanStep::new("step2", "action2").with_dependency("step1");

        let plan = PlanBuilder::new("test-plan")
            .for_intent("test-intent")
            .with_steps(vec![step1, step2])
            .build()
            .unwrap();

        assert_eq!(plan.properties.len(), 3); // name, intent-id, steps
    }

    #[test]
    fn test_rtfs_generation() {
        let step = PlanStep::new("step1", "action1").with_parameter("input", "data.csv");

        let rtfs = PlanBuilder::new("test-plan")
            .for_intent("test-intent")
            .with_step(step)
            .to_rtfs()
            .unwrap();

        assert!(rtfs.contains("plan test-plan"));
        assert!(rtfs.contains("intent-id: \"test-intent\""));
        assert!(rtfs.contains("action-id: \"action1\""));
    }

    #[test]
    fn test_validation() {
        let result = PlanBuilder::new("").for_intent("test-intent").validate();

        assert!(result.is_err());
        assert!(result
            .unwrap_err()
            .contains(&"Plan name cannot be empty".to_string()));
    }
}
