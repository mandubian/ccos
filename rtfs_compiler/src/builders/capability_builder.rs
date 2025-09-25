use super::{BuilderError, ObjectBuilder};
use crate::ast::{
    CapabilityDefinition, Expression, Keyword, Literal, MapKey, Property, Symbol, TypeExpr,
};
use std::collections::HashMap;

/// Fluent interface builder for RTFS 2.0 Capability objects
#[derive(Debug, Clone)]
pub struct CapabilityBuilder {
    name: String,
    provider: Option<String>,
    function_signature: Option<FunctionSignature>,
    input_schema: Option<TypeExpr>,
    output_schema: Option<TypeExpr>,
    sla: Option<SLA>,
    pricing: Option<Pricing>,
    examples: Vec<Example>,
    metadata: HashMap<String, String>,
}

/// Function signature for capabilities
#[derive(Debug, Clone)]
pub struct FunctionSignature {
    pub name: String,
    pub parameters: Vec<Parameter>,
    pub return_type: String,
}

/// Parameter definition
#[derive(Debug, Clone)]
pub struct Parameter {
    pub name: String,
    pub param_type: String,
    pub required: bool,
    pub default_value: Option<String>,
    pub description: Option<String>,
}

/// Service Level Agreement
#[derive(Debug, Clone)]
pub struct SLA {
    pub availability: f64,  // percentage
    pub response_time: u64, // milliseconds
    pub throughput: u64,    // requests per second
    pub error_rate: f64,    // percentage
}

/// Pricing information
#[derive(Debug, Clone)]
pub struct Pricing {
    pub base_cost: f64,
    pub per_request_cost: f64,
    pub per_unit_cost: f64,
    pub unit_type: String,
    pub currency: String,
}

/// Example usage
#[derive(Debug, Clone)]
pub struct Example {
    pub name: String,
    pub description: String,
    pub input: String,
    pub output: String,
}

impl CapabilityBuilder {
    /// Create a new CapabilityBuilder with the given name
    pub fn new(name: &str) -> Self {
        Self {
            name: name.to_string(),
            provider: None,
            function_signature: None,
            input_schema: None,
            output_schema: None,
            sla: None,
            pricing: None,
            examples: Vec::new(),
            metadata: HashMap::new(),
        }
    }

    /// Set the provider
    pub fn with_provider(mut self, provider: &str) -> Self {
        self.provider = Some(provider.to_string());
        self
    }

    /// Set the function signature
    pub fn with_function_signature(mut self, signature: FunctionSignature) -> Self {
        self.function_signature = Some(signature);
        self
    }

    /// Set the input schema
    pub fn with_input_schema(mut self, schema: TypeExpr) -> Self {
        self.input_schema = Some(schema);
        self
    }

    /// Set the output schema
    pub fn with_output_schema(mut self, schema: TypeExpr) -> Self {
        self.output_schema = Some(schema);
        self
    }

    /// Set the SLA
    pub fn with_sla(mut self, sla: SLA) -> Result<Self, BuilderError> {
        if sla.availability < 0.0 || sla.availability > 100.0 {
            return Err(BuilderError::InvalidValue(
                "availability".to_string(),
                "Availability must be between 0.0 and 100.0".to_string(),
            ));
        }
        if sla.error_rate < 0.0 || sla.error_rate > 100.0 {
            return Err(BuilderError::InvalidValue(
                "error_rate".to_string(),
                "Error rate must be between 0.0 and 100.0".to_string(),
            ));
        }
        self.sla = Some(sla);
        Ok(self)
    }

    /// Set the pricing
    pub fn with_pricing(mut self, pricing: Pricing) -> Result<Self, BuilderError> {
        if pricing.base_cost < 0.0 {
            return Err(BuilderError::InvalidValue(
                "base_cost".to_string(),
                "Base cost cannot be negative".to_string(),
            ));
        }
        if pricing.per_request_cost < 0.0 {
            return Err(BuilderError::InvalidValue(
                "per_request_cost".to_string(),
                "Per request cost cannot be negative".to_string(),
            ));
        }
        if pricing.per_unit_cost < 0.0 {
            return Err(BuilderError::InvalidValue(
                "per_unit_cost".to_string(),
                "Per unit cost cannot be negative".to_string(),
            ));
        }
        self.pricing = Some(pricing);
        Ok(self)
    }

    /// Add an example
    pub fn with_example(mut self, example: Example) -> Self {
        self.examples.push(example);
        self
    }

    /// Add multiple examples
    pub fn with_examples(mut self, examples: Vec<Example>) -> Self {
        self.examples.extend(examples);
        self
    }

    /// Add metadata key-value pair
    pub fn with_metadata(mut self, key: &str, value: &str) -> Self {
        self.metadata.insert(key.to_string(), value.to_string());
        self
    }

    /// Get suggestions for completing the capability
    pub fn suggest_completion(&self) -> Vec<String> {
        let mut suggestions = Vec::new();

        if self.provider.is_none() {
            suggestions.push("Set provider with .with_provider(\"provider-name\")".to_string());
        }

        if self.function_signature.is_none() {
            suggestions
                .push("Add function signature with .with_function_signature(...)".to_string());
        }

        if self.input_schema.is_none() {
            suggestions.push("Set input schema with .with_input_schema(TypeExpr::from_str(\":string\").unwrap())".to_string());
        }

        if self.output_schema.is_none() {
            suggestions.push(
                "Set output schema with .with_output_schema(TypeExpr::from_str(\":any\").unwrap())"
                    .to_string(),
            );
        }

        if self.sla.is_none() {
            suggestions.push("Add SLA with .with_sla(SLA::new(...))".to_string());
        }

        if self.pricing.is_none() {
            suggestions.push("Add pricing with .with_pricing(Pricing::new(...))".to_string());
        }

        suggestions
    }
}

impl FunctionSignature {
    /// Create a new function signature
    pub fn new(name: &str, return_type: &str) -> Self {
        Self {
            name: name.to_string(),
            parameters: Vec::new(),
            return_type: return_type.to_string(),
        }
    }

    /// Add a parameter
    pub fn with_parameter(mut self, parameter: Parameter) -> Self {
        self.parameters.push(parameter);
        self
    }

    /// Add multiple parameters
    pub fn with_parameters(mut self, parameters: Vec<Parameter>) -> Self {
        self.parameters.extend(parameters);
        self
    }
}

impl Parameter {
    /// Create a new parameter
    pub fn new(name: &str, param_type: &str, required: bool) -> Self {
        Self {
            name: name.to_string(),
            param_type: param_type.to_string(),
            required,
            default_value: None,
            description: None,
        }
    }

    /// Set default value
    pub fn with_default_value(mut self, value: &str) -> Self {
        self.default_value = Some(value.to_string());
        self
    }

    /// Set description
    pub fn with_description(mut self, description: &str) -> Self {
        self.description = Some(description.to_string());
        self
    }
}

impl SLA {
    /// Create a new SLA
    pub fn new(availability: f64, response_time: u64, throughput: u64, error_rate: f64) -> Self {
        Self {
            availability,
            response_time,
            throughput,
            error_rate,
        }
    }
}

impl Pricing {
    /// Create new pricing information
    pub fn new(
        base_cost: f64,
        per_request_cost: f64,
        per_unit_cost: f64,
        unit_type: &str,
        currency: &str,
    ) -> Self {
        Self {
            base_cost,
            per_request_cost,
            per_unit_cost,
            unit_type: unit_type.to_string(),
            currency: currency.to_string(),
        }
    }
}

impl Example {
    /// Create a new example
    pub fn new(name: &str, description: &str, input: &str, output: &str) -> Self {
        Self {
            name: name.to_string(),
            description: description.to_string(),
            input: input.to_string(),
            output: output.to_string(),
        }
    }
}

impl ObjectBuilder<CapabilityDefinition> for CapabilityBuilder {
    fn build(self) -> Result<CapabilityDefinition, BuilderError> {
        // Validate required fields
        if self.name.is_empty() {
            return Err(BuilderError::MissingField("name".to_string()));
        }

        if self.provider.is_none() {
            return Err(BuilderError::MissingField("provider".to_string()));
        }

        // Convert to properties
        let mut properties = vec![
            Property {
                key: Keyword::new("name"),
                value: Expression::Literal(Literal::String(self.name.clone())),
            },
            Property {
                key: Keyword::new("provider"),
                value: Expression::Literal(Literal::String(self.provider.unwrap())),
            },
        ];

        // Add function signature
        if let Some(signature) = self.function_signature {
            let mut sig_props = vec![
                Property {
                    key: Keyword::new("name"),
                    value: Expression::Literal(Literal::String(signature.name)),
                },
                Property {
                    key: Keyword::new("return-type"),
                    value: Expression::Literal(Literal::String(signature.return_type)),
                },
            ];

            if !signature.parameters.is_empty() {
                let param_expressions: Vec<Expression> = signature
                    .parameters
                    .iter()
                    .map(|param| {
                        let mut param_props = vec![
                            Property {
                                key: Keyword::new("name"),
                                value: Expression::Literal(Literal::String(param.name.clone())),
                            },
                            Property {
                                key: Keyword::new("type"),
                                value: Expression::Literal(Literal::String(
                                    param.param_type.clone(),
                                )),
                            },
                            Property {
                                key: Keyword::new("required"),
                                value: Expression::Literal(Literal::Boolean(param.required)),
                            },
                        ];

                        if let Some(default_value) = &param.default_value {
                            param_props.push(Property {
                                key: Keyword::new("default-value"),
                                value: Expression::Literal(Literal::String(default_value.clone())),
                            });
                        }

                        if let Some(description) = &param.description {
                            param_props.push(Property {
                                key: Keyword::new("description"),
                                value: Expression::Literal(Literal::String(description.clone())),
                            });
                        }

                        Expression::Map(properties_to_map(param_props))
                    })
                    .collect();

                sig_props.push(Property {
                    key: Keyword::new("parameters"),
                    value: Expression::Vector(param_expressions),
                });
            }

            properties.push(Property {
                key: Keyword::new("function-signature"),
                value: Expression::Map(properties_to_map(sig_props)),
            });
        }

        // Add optional properties
        if let Some(input_schema) = &self.input_schema {
            properties.push(Property {
                key: Keyword::new("input-schema"),
                value: Expression::Symbol(Symbol::new("type-expr")), // Placeholder for TypeExpr
            });
        }

        if let Some(output_schema) = &self.output_schema {
            properties.push(Property {
                key: Keyword::new("output-schema"),
                value: Expression::Symbol(Symbol::new("type-expr")), // Placeholder for TypeExpr
            });
        }

        if let Some(sla) = self.sla {
            let sla_props = vec![
                Property {
                    key: Keyword::new("availability"),
                    value: Expression::Literal(Literal::Float(sla.availability)),
                },
                Property {
                    key: Keyword::new("response-time"),
                    value: Expression::Literal(Literal::Integer(sla.response_time as i64)),
                },
                Property {
                    key: Keyword::new("throughput"),
                    value: Expression::Literal(Literal::Integer(sla.throughput as i64)),
                },
                Property {
                    key: Keyword::new("error-rate"),
                    value: Expression::Literal(Literal::Float(sla.error_rate)),
                },
            ];

            properties.push(Property {
                key: Keyword::new("sla"),
                value: Expression::Map(properties_to_map(sla_props)),
            });
        }

        if let Some(pricing) = self.pricing {
            let pricing_props = vec![
                Property {
                    key: Keyword::new("base-cost"),
                    value: Expression::Literal(Literal::Float(pricing.base_cost)),
                },
                Property {
                    key: Keyword::new("per-request-cost"),
                    value: Expression::Literal(Literal::Float(pricing.per_request_cost)),
                },
                Property {
                    key: Keyword::new("per-unit-cost"),
                    value: Expression::Literal(Literal::Float(pricing.per_unit_cost)),
                },
                Property {
                    key: Keyword::new("unit-type"),
                    value: Expression::Literal(Literal::String(pricing.unit_type)),
                },
                Property {
                    key: Keyword::new("currency"),
                    value: Expression::Literal(Literal::String(pricing.currency)),
                },
            ];

            properties.push(Property {
                key: Keyword::new("pricing"),
                value: Expression::Map(properties_to_map(pricing_props)),
            });
        }

        if !self.examples.is_empty() {
            let example_expressions: Vec<Expression> = self
                .examples
                .iter()
                .map(|example| {
                    let example_props = vec![
                        Property {
                            key: Keyword::new("name"),
                            value: Expression::Literal(Literal::String(example.name.clone())),
                        },
                        Property {
                            key: Keyword::new("description"),
                            value: Expression::Literal(Literal::String(
                                example.description.clone(),
                            )),
                        },
                        Property {
                            key: Keyword::new("input"),
                            value: Expression::Literal(Literal::String(example.input.clone())),
                        },
                        Property {
                            key: Keyword::new("output"),
                            value: Expression::Literal(Literal::String(example.output.clone())),
                        },
                    ];

                    Expression::Map(properties_to_map(example_props))
                })
                .collect();

            properties.push(Property {
                key: Keyword::new("examples"),
                value: Expression::Vector(example_expressions),
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

        Ok(CapabilityDefinition {
            name: Symbol::new(&self.name),
            properties,
        })
    }

    fn validate(&self) -> Result<(), Vec<String>> {
        let mut errors = Vec::new();

        if self.name.is_empty() {
            errors.push("Capability name cannot be empty".to_string());
        }

        if self.provider.is_none() {
            errors.push("Provider is required".to_string());
        }

        if let Some(sla) = &self.sla {
            if sla.availability < 0.0 || sla.availability > 100.0 {
                errors.push("Availability must be between 0.0 and 100.0".to_string());
            }
            if sla.error_rate < 0.0 || sla.error_rate > 100.0 {
                errors.push("Error rate must be between 0.0 and 100.0".to_string());
            }
        }

        if let Some(pricing) = &self.pricing {
            if pricing.base_cost < 0.0 {
                errors.push("Base cost cannot be negative".to_string());
            }
            if pricing.per_request_cost < 0.0 {
                errors.push("Per request cost cannot be negative".to_string());
            }
            if pricing.per_unit_cost < 0.0 {
                errors.push("Per unit cost cannot be negative".to_string());
            }
        }

        if errors.is_empty() {
            Ok(())
        } else {
            Err(errors)
        }
    }

    fn to_rtfs(&self) -> Result<String, BuilderError> {
        let mut rtfs = format!("capability {} {{\n", self.name);

        // Required fields
        rtfs.push_str(&format!("  name: \"{}\"\n", self.name));

        if let Some(provider) = &self.provider {
            rtfs.push_str(&format!("  provider: \"{}\"\n", provider));
        }

        // Function signature
        if let Some(signature) = &self.function_signature {
            rtfs.push_str("  function-signature: {\n");
            rtfs.push_str(&format!("    name: \"{}\"\n", signature.name));
            rtfs.push_str(&format!("    return-type: \"{}\"\n", signature.return_type));

            if !signature.parameters.is_empty() {
                rtfs.push_str("    parameters: [\n");
                for param in &signature.parameters {
                    rtfs.push_str("      {\n");
                    rtfs.push_str(&format!("        name: \"{}\"\n", param.name));
                    rtfs.push_str(&format!("        type: \"{}\"\n", param.param_type));
                    rtfs.push_str(&format!("        required: {}\n", param.required));

                    if let Some(default_value) = &param.default_value {
                        rtfs.push_str(&format!("        default-value: \"{}\"\n", default_value));
                    }

                    if let Some(description) = &param.description {
                        rtfs.push_str(&format!("        description: \"{}\"\n", description));
                    }

                    rtfs.push_str("      }\n");
                }
                rtfs.push_str("    ]\n");
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

        if let Some(sla) = &self.sla {
            rtfs.push_str("  sla: {\n");
            rtfs.push_str(&format!("    availability: {}\n", sla.availability));
            rtfs.push_str(&format!("    response-time: {}\n", sla.response_time));
            rtfs.push_str(&format!("    throughput: {}\n", sla.throughput));
            rtfs.push_str(&format!("    error-rate: {}\n", sla.error_rate));
            rtfs.push_str("  }\n");
        }

        if let Some(pricing) = &self.pricing {
            rtfs.push_str("  pricing: {\n");
            rtfs.push_str(&format!("    base-cost: {}\n", pricing.base_cost));
            rtfs.push_str(&format!(
                "    per-request-cost: {}\n",
                pricing.per_request_cost
            ));
            rtfs.push_str(&format!("    per-unit-cost: {}\n", pricing.per_unit_cost));
            rtfs.push_str(&format!("    unit-type: \"{}\"\n", pricing.unit_type));
            rtfs.push_str(&format!("    currency: \"{}\"\n", pricing.currency));
            rtfs.push_str("  }\n");
        }

        if !self.examples.is_empty() {
            rtfs.push_str("  examples: [\n");
            for example in &self.examples {
                rtfs.push_str("    {\n");
                rtfs.push_str(&format!("      name: \"{}\"\n", example.name));
                rtfs.push_str(&format!("      description: \"{}\"\n", example.description));
                rtfs.push_str(&format!("      input: \"{}\"\n", example.input));
                rtfs.push_str(&format!("      output: \"{}\"\n", example.output));
                rtfs.push_str("    }\n");
            }
            rtfs.push_str("  ]\n");
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
    fn test_basic_capability_builder() {
        let signature = FunctionSignature::new("analyze_data", "json")
            .with_parameter(Parameter::new("input", "string", true));

        let capability = CapabilityBuilder::new("test-capability")
            .with_provider("test-provider")
            .with_function_signature(signature)
            .with_input_schema(TypeExpr::from_str(":string").unwrap())
            .with_output_schema(TypeExpr::from_str(":any").unwrap())
            .build()
            .unwrap();

        assert_eq!(capability.name.0, "test-capability");
        assert_eq!(capability.properties.len(), 5); // name, provider, function-signature, input-schema, output-schema
    }

    #[test]
    fn test_capability_with_sla_and_pricing() {
        let sla = SLA::new(99.9, 100, 1000, 0.1);
        let pricing = Pricing::new(10.0, 0.01, 0.001, "request", "USD");

        let capability = CapabilityBuilder::new("test-capability")
            .with_provider("test-provider")
            .with_sla(sla)
            .unwrap()
            .with_pricing(pricing)
            .unwrap()
            .build()
            .unwrap();

        assert_eq!(capability.properties.len(), 4); // name, provider, sla, pricing
    }

    #[test]
    fn test_rtfs_generation() {
        let signature = FunctionSignature::new("process", "string");
        let rtfs = CapabilityBuilder::new("test-capability")
            .with_provider("test-provider")
            .with_function_signature(signature)
            .to_rtfs()
            .unwrap();

        assert!(rtfs.contains("capability test-capability"));
        assert!(rtfs.contains("provider: \"test-provider\""));
        assert!(rtfs.contains("name: \"process\""));
    }

    #[test]
    fn test_validation() {
        let result = CapabilityBuilder::new("").validate();

        assert!(result.is_err());
        assert!(result
            .unwrap_err()
            .contains(&"Capability name cannot be empty".to_string()));
    }
}
