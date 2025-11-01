// RTFS 2.0 Object Builders
// Provides fluent interfaces for creating RTFS 2.0 objects programmatically

pub mod action_builder;
pub mod capability_builder;
pub mod intent_builder;
pub mod module_builder;
pub mod plan_builder;
pub mod resource_builder;

pub use action_builder::ActionBuilder;
pub use capability_builder::CapabilityBuilder;
pub use intent_builder::IntentBuilder;
pub use module_builder::ModuleBuilder;
pub use plan_builder::PlanBuilder;
pub use resource_builder::ResourceBuilder;

use std::collections::HashMap;

/// Common trait for all RTFS 2.0 object builders
pub trait ObjectBuilder<T> {
    /// Build the final object, consuming the builder
    fn build(self) -> Result<T, BuilderError>;

    /// Validate the current state without building
    fn validate(&self) -> Result<(), Vec<String>>;

    /// Generate RTFS 2.0 syntax for the object
    fn to_rtfs(&self) -> Result<String, BuilderError>;
}

/// Error type for builder operations
#[derive(Debug, thiserror::Error)]
pub enum BuilderError {
    #[error("Validation failed: {0}")]
    Validation(String),

    #[error("Missing required field: {0}")]
    MissingField(String),

    #[error("Invalid value for field '{0}': {1}")]
    InvalidValue(String, String),

    #[error("RTFS generation failed: {0}")]
    RtfsGeneration(String),

    #[error("Natural language parsing failed: {0}")]
    NaturalLanguageParsing(String),
}

/// Priority levels for intents and plans
#[derive(Debug, Clone, PartialEq)]
pub enum Priority {
    Low,
    Medium,
    High,
    Critical,
}

impl std::fmt::Display for Priority {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Priority::Low => write!(f, "low"),
            Priority::Medium => write!(f, "medium"),
            Priority::High => write!(f, "high"),
            Priority::Critical => write!(f, "critical"),
        }
    }
}

/// Constraint types for intents and plans
#[derive(Debug, Clone)]
pub enum Constraint {
    MaxCost(f64),
    Deadline(String),
    DataLocality(Vec<String>),
    SecurityClearance(String),
    PreferredStyle(String),
    MaxDuration(u64), // seconds
    MinAccuracy(f64),
    MaxRetries(u32),
}

impl Constraint {
    pub fn name(&self) -> &'static str {
        match self {
            Constraint::MaxCost(_) => "max-cost",
            Constraint::Deadline(_) => "deadline",
            Constraint::DataLocality(_) => "data-locality",
            Constraint::SecurityClearance(_) => "security-clearance",
            Constraint::PreferredStyle(_) => "preferred-style",
            Constraint::MaxDuration(_) => "max-duration",
            Constraint::MinAccuracy(_) => "min-accuracy",
            Constraint::MaxRetries(_) => "max-retries",
        }
    }

    pub fn value(&self) -> String {
        match self {
            Constraint::MaxCost(cost) => cost.to_string(),
            Constraint::Deadline(deadline) => format!("\"{}\"", deadline),
            Constraint::DataLocality(locations) => {
                let locations_str = locations
                    .iter()
                    .map(|loc| format!(":{}", loc))
                    .collect::<Vec<_>>()
                    .join(" ");
                format!("[{}]", locations_str)
            }
            Constraint::SecurityClearance(level) => format!(":{}", level),
            Constraint::PreferredStyle(style) => format!(":{}", style),
            Constraint::MaxDuration(duration) => duration.to_string(),
            Constraint::MinAccuracy(accuracy) => accuracy.to_string(),
            Constraint::MaxRetries(retries) => retries.to_string(),
        }
    }
}

/// Success criteria function type
pub type SuccessCriteria = Box<dyn Fn(&HashMap<String, serde_json::Value>) -> bool + Send + Sync>;

/// Helper function to convert constraints to RTFS map syntax
pub fn constraints_to_rtfs(constraints: &[Constraint]) -> String {
    if constraints.is_empty() {
        return "{}".to_string();
    }

    let constraint_pairs: Vec<String> = constraints
        .iter()
        .map(|c| format!("  {}: {}", c.name(), c.value()))
        .collect();

    format!("{{\n{}\n}}", constraint_pairs.join("\n"))
}

/// Helper function to validate constraint values
pub fn validate_constraint(constraint: &Constraint) -> Result<(), String> {
    match constraint {
        Constraint::MaxCost(cost) => {
            if *cost < 0.0 {
                return Err("Cost cannot be negative".to_string());
            }
        }
        Constraint::Deadline(deadline) => {
            // Basic ISO 8601 validation
            if !deadline.contains('T') || !deadline.contains('Z') {
                return Err(
                    "Deadline must be in ISO 8601 format (YYYY-MM-DDTHH:MM:SSZ)".to_string()
                );
            }
        }
        Constraint::DataLocality(locations) => {
            if locations.is_empty() {
                return Err("Data locality must specify at least one location".to_string());
            }
        }
        Constraint::MinAccuracy(accuracy) => {
            if *accuracy < 0.0 || *accuracy > 1.0 {
                return Err("Accuracy must be between 0.0 and 1.0".to_string());
            }
        }
        Constraint::MaxRetries(retries) => {
            if *retries > 100 {
                return Err("Max retries cannot exceed 100".to_string());
            }
        }
        _ => {} // Other constraints don't need validation
    }
    Ok(())
}
