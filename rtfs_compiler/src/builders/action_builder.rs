use super::{BuilderError, ObjectBuilder};
use crate::ast::{ActionDefinition, Expression, Keyword, Literal, MapKey, Property, Symbol};
use chrono::{DateTime, Utc};
use std::collections::HashMap;

/// Fluent interface builder for RTFS 2.0 Action objects
#[derive(Debug, Clone)]
pub struct ActionBuilder {
    name: String,
    capability_id: Option<String>,
    parameters: HashMap<String, String>,
    input_schema: Option<String>,
    output_schema: Option<String>,
    cost: Option<f64>,
    duration: Option<u64>, // seconds
    signature: Option<String>,
    provenance: Option<Provenance>,
    performance_metrics: Option<PerformanceMetrics>,
    metadata: HashMap<String, String>,
}

/// Provenance information for actions
#[derive(Debug, Clone)]
pub struct Provenance {
    pub created_at: DateTime<Utc>,
    pub created_by: String,
    pub version: String,
    pub source: String,
}

/// Performance metrics for actions
#[derive(Debug, Clone)]
pub struct PerformanceMetrics {
    pub avg_execution_time: f64,
    pub success_rate: f64,
    pub error_rate: f64,
    pub last_executed: Option<DateTime<Utc>>,
}

impl ActionBuilder {
    /// Create a new ActionBuilder with the given name
    pub fn new(name: &str) -> Self {
        Self {
            name: name.to_string(),
            capability_id: None,
            parameters: HashMap::new(),
            input_schema: None,
            output_schema: None,
            cost: None,
            duration: None,
            signature: None,
            provenance: None,
            performance_metrics: None,
            metadata: HashMap::new(),
        }
    }

    /// Set the capability ID this action implements
    pub fn for_capability(mut self, capability_id: &str) -> Self {
        self.capability_id = Some(capability_id.to_string());
        self
    }

    /// Add a parameter to the action
    pub fn with_parameter(mut self, key: &str, value: &str) -> Self {
        self.parameters.insert(key.to_string(), value.to_string());
        self
    }

    /// Add multiple parameters at once
    pub fn with_parameters(mut self, params: HashMap<String, String>) -> Self {
        self.parameters.extend(params);
        self
    }

    /// Set the input schema
    pub fn with_input_schema(mut self, schema: &str) -> Self {
        self.input_schema = Some(schema.to_string());
        self
    }

    /// Set the output schema
    pub fn with_output_schema(mut self, schema: &str) -> Self {
        self.output_schema = Some(schema.to_string());
        self
    }

    /// Set the cost
    pub fn with_cost(mut self, cost: f64) -> Result<Self, BuilderError> {
        if cost < 0.0 {
            return Err(BuilderError::InvalidValue(
                "cost".to_string(),
                "Cost cannot be negative".to_string(),
            ));
        }
        self.cost = Some(cost);
        Ok(self)
    }

    /// Set the duration in seconds
    pub fn with_duration(mut self, duration: u64) -> Self {
        self.duration = Some(duration);
        self
    }

    /// Set the cryptographic signature
    pub fn with_signature(mut self, signature: &str) -> Self {
        self.signature = Some(signature.to_string());
        self
    }

    /// Set provenance information
    pub fn with_provenance(mut self, provenance: Provenance) -> Self {
        self.provenance = Some(provenance);
        self
    }

    /// Set performance metrics
    pub fn with_performance_metrics(mut self, metrics: PerformanceMetrics) -> Self {
        self.performance_metrics = Some(metrics);
        self
    }

    /// Add metadata key-value pair
    pub fn with_metadata(mut self, key: &str, value: &str) -> Self {
        self.metadata.insert(key.to_string(), value.to_string());
        self
    }

    /// Get suggestions for completing the action
    pub fn suggest_completion(&self) -> Vec<String> {
        let mut suggestions = Vec::new();

        if self.capability_id.is_none() {
            suggestions
                .push("Link to a capability with .for_capability(\"capability-id\")".to_string());
        }

        if self.parameters.is_empty() {
            suggestions.push("Add parameters with .with_parameter(\"key\", \"value\")".to_string());
        }

        if self.cost.is_none() {
            suggestions.push("Set cost with .with_cost(10.0)".to_string());
        }

        if self.duration.is_none() {
            suggestions.push("Set duration with .with_duration(60)".to_string());
        }

        suggestions
    }
}

impl Provenance {
    /// Create new provenance information
    pub fn new(created_by: &str, version: &str, source: &str) -> Self {
        Self {
            created_at: Utc::now(),
            created_by: created_by.to_string(),
            version: version.to_string(),
            source: source.to_string(),
        }
    }

    /// Create with custom timestamp
    pub fn with_timestamp(mut self, timestamp: DateTime<Utc>) -> Self {
        self.created_at = timestamp;
        self
    }
}

impl PerformanceMetrics {
    /// Create new performance metrics
    pub fn new(
        avg_execution_time: f64,
        success_rate: f64,
        error_rate: f64,
    ) -> Result<Self, BuilderError> {
        if success_rate < 0.0 || success_rate > 1.0 {
            return Err(BuilderError::InvalidValue(
                "success_rate".to_string(),
                "Success rate must be between 0.0 and 1.0".to_string(),
            ));
        }
        if error_rate < 0.0 || error_rate > 1.0 {
            return Err(BuilderError::InvalidValue(
                "error_rate".to_string(),
                "Error rate must be between 0.0 and 1.0".to_string(),
            ));
        }

        Ok(Self {
            avg_execution_time,
            success_rate,
            error_rate,
            last_executed: None,
        })
    }

    /// Set last executed timestamp
    pub fn with_last_executed(mut self, timestamp: DateTime<Utc>) -> Self {
        self.last_executed = Some(timestamp);
        self
    }
}

impl ObjectBuilder<ActionDefinition> for ActionBuilder {
    fn build(self) -> Result<ActionDefinition, BuilderError> {
        // Validate required fields
        if self.name.is_empty() {
            return Err(BuilderError::MissingField("name".to_string()));
        }

        if self.capability_id.is_none() {
            return Err(BuilderError::MissingField("capability_id".to_string()));
        }

        // Convert to properties
        let mut properties = vec![
            Property {
                key: Keyword::new("name"),
                value: Expression::Literal(Literal::String(self.name.clone())),
            },
            Property {
                key: Keyword::new("capability-id"),
                value: Expression::Literal(Literal::String(self.capability_id.unwrap())),
            },
        ];

        // Add parameters
        if !self.parameters.is_empty() {
            let param_props: Vec<Property> = self
                .parameters
                .iter()
                .map(|(k, v)| Property {
                    key: Keyword::new(k),
                    value: Expression::Literal(Literal::String(v.clone())),
                })
                .collect();

            properties.push(Property {
                key: Keyword::new("parameters"),
                value: Expression::Map(properties_to_map(param_props)),
            });
        }

        // Add optional properties
        if let Some(input_schema) = self.input_schema {
            properties.push(Property {
                key: Keyword::new("input-schema"),
                value: Expression::Literal(Literal::String(input_schema)),
            });
        }

        if let Some(output_schema) = self.output_schema {
            properties.push(Property {
                key: Keyword::new("output-schema"),
                value: Expression::Literal(Literal::String(output_schema)),
            });
        }

        if let Some(cost) = self.cost {
            properties.push(Property {
                key: Keyword::new("cost"),
                value: Expression::Literal(Literal::Float(cost)),
            });
        }

        if let Some(duration) = self.duration {
            properties.push(Property {
                key: Keyword::new("duration"),
                value: Expression::Literal(Literal::Integer(duration as i64)),
            });
        }

        if let Some(signature) = self.signature {
            properties.push(Property {
                key: Keyword::new("signature"),
                value: Expression::Literal(Literal::String(signature)),
            });
        }

        if let Some(provenance) = self.provenance {
            let provenance_props = vec![
                Property {
                    key: Keyword::new("created-at"),
                    value: Expression::Literal(Literal::String(provenance.created_at.to_rfc3339())),
                },
                Property {
                    key: Keyword::new("created-by"),
                    value: Expression::Literal(Literal::String(provenance.created_by)),
                },
                Property {
                    key: Keyword::new("version"),
                    value: Expression::Literal(Literal::String(provenance.version)),
                },
                Property {
                    key: Keyword::new("source"),
                    value: Expression::Literal(Literal::String(provenance.source)),
                },
            ];

            properties.push(Property {
                key: Keyword::new("provenance"),
                value: Expression::Map(properties_to_map(provenance_props)),
            });
        }

        if let Some(metrics) = self.performance_metrics {
            let mut metrics_props = vec![
                Property {
                    key: Keyword::new("avg-execution-time"),
                    value: Expression::Literal(Literal::Float(metrics.avg_execution_time)),
                },
                Property {
                    key: Keyword::new("success-rate"),
                    value: Expression::Literal(Literal::Float(metrics.success_rate)),
                },
                Property {
                    key: Keyword::new("error-rate"),
                    value: Expression::Literal(Literal::Float(metrics.error_rate)),
                },
            ];

            if let Some(last_executed) = metrics.last_executed {
                metrics_props.push(Property {
                    key: Keyword::new("last-executed"),
                    value: Expression::Literal(Literal::String(last_executed.to_rfc3339())),
                });
            }

            properties.push(Property {
                key: Keyword::new("performance-metrics"),
                value: Expression::Map(properties_to_map(metrics_props)),
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

        Ok(ActionDefinition {
            name: Symbol::new(&self.name),
            properties,
        })
    }

    fn validate(&self) -> Result<(), Vec<String>> {
        let mut errors = Vec::new();

        if self.name.is_empty() {
            errors.push("Action name cannot be empty".to_string());
        }

        if self.capability_id.is_none() {
            errors.push("Capability ID is required".to_string());
        }

        if let Some(cost) = self.cost {
            if cost < 0.0 {
                errors.push("Cost cannot be negative".to_string());
            }
        }

        if let Some(metrics) = &self.performance_metrics {
            if metrics.success_rate < 0.0 || metrics.success_rate > 1.0 {
                errors.push("Success rate must be between 0.0 and 1.0".to_string());
            }
            if metrics.error_rate < 0.0 || metrics.error_rate > 1.0 {
                errors.push("Error rate must be between 0.0 and 1.0".to_string());
            }
        }

        if errors.is_empty() {
            Ok(())
        } else {
            Err(errors)
        }
    }

    fn to_rtfs(&self) -> Result<String, BuilderError> {
        let mut rtfs = format!("action {} {{\n", self.name);

        // Required fields
        rtfs.push_str(&format!("  name: \"{}\"\n", self.name));

        if let Some(capability_id) = &self.capability_id {
            rtfs.push_str(&format!("  capability-id: \"{}\"\n", capability_id));
        }

        // Parameters
        if !self.parameters.is_empty() {
            rtfs.push_str("  parameters: {\n");
            for (key, value) in &self.parameters {
                rtfs.push_str(&format!("    {}: \"{}\"\n", key, value));
            }
            rtfs.push_str("  }\n");
        }

        // Optional fields
        if let Some(input_schema) = &self.input_schema {
            rtfs.push_str(&format!("  input-schema: \"{}\"\n", input_schema));
        }

        if let Some(output_schema) = &self.output_schema {
            rtfs.push_str(&format!("  output-schema: \"{}\"\n", output_schema));
        }

        if let Some(cost) = self.cost {
            rtfs.push_str(&format!("  cost: {}\n", cost));
        }

        if let Some(duration) = self.duration {
            rtfs.push_str(&format!("  duration: {}\n", duration));
        }

        if let Some(signature) = &self.signature {
            rtfs.push_str(&format!("  signature: \"{}\"\n", signature));
        }

        if let Some(provenance) = &self.provenance {
            rtfs.push_str("  provenance: {\n");
            rtfs.push_str(&format!(
                "    created-at: \"{}\"\n",
                provenance.created_at.to_rfc3339()
            ));
            rtfs.push_str(&format!("    created-by: \"{}\"\n", provenance.created_by));
            rtfs.push_str(&format!("    version: \"{}\"\n", provenance.version));
            rtfs.push_str(&format!("    source: \"{}\"\n", provenance.source));
            rtfs.push_str("  }\n");
        }

        if let Some(metrics) = &self.performance_metrics {
            rtfs.push_str("  performance-metrics: {\n");
            rtfs.push_str(&format!(
                "    avg-execution-time: {}\n",
                metrics.avg_execution_time
            ));
            rtfs.push_str(&format!("    success-rate: {}\n", metrics.success_rate));
            rtfs.push_str(&format!("    error-rate: {}\n", metrics.error_rate));
            if let Some(last_executed) = &metrics.last_executed {
                rtfs.push_str(&format!(
                    "    last-executed: \"{}\"\n",
                    last_executed.to_rfc3339()
                ));
            }
            rtfs.push_str("  }\n");
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
    fn test_basic_action_builder() {
        let action = ActionBuilder::new("test-action")
            .for_capability("test-capability")
            .with_parameter("input", "data.csv")
            .with_cost(10.0)
            .unwrap()
            .build()
            .unwrap();

        assert_eq!(action.name.0, "test-action");
        assert_eq!(action.properties.len(), 4); // name, capability-id, parameters, cost
    }

    #[test]
    fn test_action_with_provenance() {
        let provenance = Provenance::new("user1", "1.0.0", "github.com/example/action");
        let action = ActionBuilder::new("test-action")
            .for_capability("test-capability")
            .with_provenance(provenance)
            .build()
            .unwrap();

        assert_eq!(action.properties.len(), 3); // name, capability-id, provenance
    }

    #[test]
    fn test_rtfs_generation() {
        let rtfs = ActionBuilder::new("test-action")
            .for_capability("test-capability")
            .with_parameter("input", "data.csv")
            .to_rtfs()
            .unwrap();

        assert!(rtfs.contains("action test-action"));
        assert!(rtfs.contains("capability-id: \"test-capability\""));
        assert!(rtfs.contains("input: \"data.csv\""));
    }

    #[test]
    fn test_validation() {
        let result = ActionBuilder::new("")
            .for_capability("test-capability")
            .validate();

        assert!(result.is_err());
        assert!(result
            .unwrap_err()
            .contains(&"Action name cannot be empty".to_string()));
    }
}
