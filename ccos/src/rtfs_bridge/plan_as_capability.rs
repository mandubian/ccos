//! Plan-as-Capability Adapter
//! 
//! This module provides functionality to expose a Plan as a callable Capability.
//! This enables plans to be reused as capabilities in the marketplace, with
//! proper schema inheritance and effect propagation.
//! 
//! Effects and permissions are automatically propagated from the plan's used capabilities
//! by analyzing the plan body and looking up capabilities in the marketplace.

use super::effects_propagation::{
    propagate_effects_from_plan, EffectsPropagationConfig,
};
use super::errors::RtfsBridgeError;
use super::language_utils::{
    ensure_language_for_local_capability, plan_language_to_string,
    validate_local_capability_has_language,
};
use crate::capability_marketplace::CapabilityManifest;
use crate::types::Plan;
use rtfs::ast::{Expression, Literal, MapKey};
use rtfs::runtime::values::Value;
use std::collections::HashMap;

/// Adapter configuration for plan-as-capability conversion
#[derive(Debug, Clone)]
pub struct PlanAsCapabilityConfig {
    /// Capability ID to use (defaults to plan name with prefix)
    pub capability_id: Option<String>,
    /// Capability name (defaults to plan name)
    pub capability_name: Option<String>,
    /// Capability version (defaults to "1.0.0")
    pub version: Option<String>,
    /// Provider type (defaults to "Local")
    pub provider: Option<String>,
    /// Additional metadata for the capability
    pub metadata: HashMap<String, String>,
    /// Permissions to add (defaults to empty)
    pub permissions: Vec<String>,
    /// Effects to add (computed from plan's capabilities-required if not provided)
    pub effects: Option<Vec<String>>,
    /// Language for the implementation (defaults to plan's language)
    pub language: Option<String>,
}

impl Default for PlanAsCapabilityConfig {
    fn default() -> Self {
        Self {
            capability_id: None,
            capability_name: None,
            version: Some("1.0.0".to_string()),
            provider: Some("Local".to_string()),
            metadata: HashMap::new(),
            permissions: Vec::new(),
            effects: None,
            language: None,
        }
    }
}

impl PlanAsCapabilityConfig {
    /// Create a new config with default values
    pub fn new() -> Self {
        Self::default()
    }

    /// Set the capability ID
    pub fn with_id(mut self, id: String) -> Self {
        self.capability_id = Some(id);
        self
    }

    /// Set the capability name
    pub fn with_name(mut self, name: String) -> Self {
        self.capability_name = Some(name);
        self
    }

    /// Set the version
    pub fn with_version(mut self, version: String) -> Self {
        self.version = Some(version);
        self
    }

    /// Add metadata
    pub fn with_metadata(mut self, key: String, value: String) -> Self {
        self.metadata.insert(key, value);
        self
    }

    /// Add permissions
    pub fn with_permissions(mut self, permissions: Vec<String>) -> Self {
        self.permissions = permissions;
        self
    }

    /// Set effects explicitly
    pub fn with_effects(mut self, effects: Vec<String>) -> Self {
        self.effects = Some(effects);
        self
    }
}

/// Convert a Plan to a Capability RTFS map expression
/// 
/// This creates a canonical capability map that wraps the plan's :body as the
/// :implementation. The capability inherits the plan's input/output schemas
/// and capabilities-required list.
/// 
/// Example:
/// ```rtfs
/// {:type "capability"
///  :id "plan.my-plan"
///  :name "My Plan"
///  :version "1.0.0"
///  :description "Plan exposed as capability"
///  :input-schema {...}  ; from plan
///  :output-schema {...}  ; from plan
///  :implementation (fn [input] ...)  ; wraps plan body
///  :language "rtfs20"
///  :provider "Local"
///  :metadata {:plan-id "..." :source "plan"}}
/// ```
pub fn plan_to_capability_map(
    plan: &Plan,
    config: PlanAsCapabilityConfig,
) -> Result<Expression, RtfsBridgeError> {
    use super::converters::plan_to_rtfs_map;
    
    // Get plan map first
    let plan_map_expr = plan_to_rtfs_map(plan)?;
    let plan_map = match plan_map_expr {
        Expression::Map(m) => m,
        _ => {
            return Err(RtfsBridgeError::InvalidObjectFormat {
                message: "plan_to_rtfs_map should return a map".to_string(),
            });
        }
    };

    // Determine capability ID
    let capability_id = config.capability_id.unwrap_or_else(|| {
        plan.name
            .as_ref()
            .map(|n| format!("plan.{}", n))
            .unwrap_or_else(|| "plan.unnamed".to_string())
    });

    // Determine capability name
    let capability_name = config.capability_name
        .or_else(|| plan.name.clone())
        .unwrap_or_else(|| "Unnamed Plan".to_string());

    // Build capability map
    let mut cap_map: HashMap<MapKey, Expression> = HashMap::new();

    // Required fields
    cap_map.insert(
        MapKey::String(":type".to_string()),
        Expression::Literal(Literal::String("capability".to_string())),
    );
    cap_map.insert(
        MapKey::String(":id".to_string()),
        Expression::Literal(Literal::String(capability_id.clone())),
    );
    cap_map.insert(
        MapKey::String(":name".to_string()),
        Expression::Literal(Literal::String(capability_name.clone())),
    );
    cap_map.insert(
        MapKey::String(":version".to_string()),
        Expression::Literal(Literal::String(
            config.version.unwrap_or_else(|| "1.0.0".to_string()),
        )),
    );
    cap_map.insert(
        MapKey::String(":description".to_string()),
        Expression::Literal(Literal::String(
            format!("Plan '{}' exposed as capability", capability_name),
        )),
    );

    // Inherit input/output schemas from plan
    if let Some(input_schema_expr) = plan_map.get(&MapKey::String(":input-schema".to_string())) {
        cap_map.insert(
            MapKey::String(":input-schema".to_string()),
            input_schema_expr.clone(),
        );
    }

    if let Some(output_schema_expr) =
        plan_map.get(&MapKey::String(":output-schema".to_string()))
    {
        cap_map.insert(
            MapKey::String(":output-schema".to_string()),
            output_schema_expr.clone(),
        );
    }

    // Create implementation that wraps the plan body
    // The implementation is a function that takes input and executes the plan body
    let body_expr = plan_map
        .get(&MapKey::String(":body".to_string()))
        .ok_or_else(|| RtfsBridgeError::MissingRequiredField {
            field: "body".to_string(),
        })?;

    // Wrap plan body in a function that accepts input
    // For RTFS plans, we create: (fn [input] (let [input input] <plan-body>))
    // The body is a vector of expressions, with the plan body wrapped in a let binding
    let implementation = Expression::Fn(rtfs::ast::FnExpr {
        params: vec![rtfs::ast::ParamDef {
            pattern: rtfs::ast::Pattern::Symbol(rtfs::ast::Symbol("input".to_string())),
            type_annotation: None,
        }],
        variadic_param: None,
        return_type: None,
        body: vec![Expression::Let(rtfs::ast::LetExpr {
            bindings: vec![rtfs::ast::LetBinding {
                pattern: rtfs::ast::Pattern::Symbol(rtfs::ast::Symbol("input".to_string())),
                type_annotation: None,
                value: Box::new(Expression::Symbol(rtfs::ast::Symbol("input".to_string()))),
            }],
            body: vec![body_expr.clone()],
        })],
        delegation_hint: None,
    });

    cap_map.insert(
        MapKey::String(":implementation".to_string()),
        implementation,
    );

    // Set language (from plan or config) - REQUIRED for local capabilities
    let language = config.language.unwrap_or_else(|| {
        plan_language_to_string(&plan.language)
    });
    cap_map.insert(
        MapKey::String(":language".to_string()),
        Expression::Literal(Literal::String(language)),
    );

    // Set provider
    cap_map.insert(
        MapKey::String(":provider".to_string()),
        Expression::Literal(Literal::String(
            config.provider.unwrap_or_else(|| "Local".to_string()),
        )),
    );

    // Add permissions
    if !config.permissions.is_empty() {
        let perms_vec: Vec<Expression> = config
            .permissions
            .iter()
            .map(|p| Expression::Literal(Literal::String(p.clone())))
            .collect();
        cap_map.insert(
            MapKey::String(":permissions".to_string()),
            Expression::Vector(perms_vec),
        );
    }

    // Add effects (from plan's capabilities-required if not explicitly set)
    // If effects are not explicitly provided, use the plan's capabilities-required
    // (static analysis propagation can be done via propagate_effects_from_plan_with_marketplace)
    let effects = config.effects.unwrap_or_else(|| {
        // Conservative: assume plan's required capabilities contribute to effects
        plan.capabilities_required.clone()
    });
    if !effects.is_empty() {
        let effects_vec: Vec<Expression> = effects
            .iter()
            .map(|e| Expression::Literal(Literal::String(e.clone())))
            .collect();
        cap_map.insert(
            MapKey::String(":effects".to_string()),
            Expression::Vector(effects_vec),
        );
    }

    // Add metadata
    let mut metadata_map = HashMap::new();
    metadata_map.insert(
        MapKey::String(":plan-id".to_string()),
        Expression::Literal(Literal::String(plan.plan_id.clone())),
    );
    metadata_map.insert(
        MapKey::String(":source".to_string()),
        Expression::Literal(Literal::String("plan".to_string())),
    );
    if let Some(name) = &plan.name {
        metadata_map.insert(
            MapKey::String(":plan-name".to_string()),
            Expression::Literal(Literal::String(name.clone())),
        );
    }
    // Add custom metadata from config
    for (key, value) in &config.metadata {
        metadata_map.insert(
            MapKey::String(format!(":{}", key)),
            Expression::Literal(Literal::String(value.clone())),
        );
    }
    cap_map.insert(
        MapKey::String(":metadata".to_string()),
        Expression::Map(metadata_map),
    );

    // Add provider-meta if needed (for non-local providers, this would contain
    // provider-specific metadata like endpoint URLs, auth config, etc.)
    let mut provider_meta = HashMap::new();
    provider_meta.insert(
        MapKey::String(":plan-id".to_string()),
        Expression::Literal(Literal::String(plan.plan_id.clone())),
    );
    provider_meta.insert(
        MapKey::String(":source".to_string()),
        Expression::Literal(Literal::String("plan".to_string())),
    );
    cap_map.insert(
        MapKey::String(":provider-meta".to_string()),
        Expression::Map(provider_meta),
    );

    // Validate that local capabilities have language (should already be set above)
    // Convert to Value::Map for validation
    let mut cap_map_value = HashMap::new();
    for (k, v) in &cap_map {
        // For validation, we need to convert Expression to Value
        // This is a simplified check - full validation happens elsewhere
        if let MapKey::String(key_str) = k {
            if key_str == ":language" {
                if let Expression::Literal(Literal::String(lang)) = v {
                    cap_map_value.insert(
                        rtfs::ast::MapKey::String(key_str.clone()),
                        Value::String(lang.clone()),
                    );
                }
            }
            if key_str == ":provider" {
                if let Expression::Literal(Literal::String(prov)) = v {
                    cap_map_value.insert(
                        rtfs::ast::MapKey::String(key_str.clone()),
                        Value::String(prov.clone()),
                    );
                }
            }
        }
    }

    // Validate local capability has language
    if let Err(e) = validate_local_capability_has_language(&cap_map_value) {
        // If validation fails, ensure language is set with default
        ensure_language_for_local_capability(&mut cap_map_value, Some("rtfs20"))
            .map_err(|_| e)?;
        
        // Update the Expression map with the ensured language
        if let Some(lang_val) = cap_map_value.get(&rtfs::ast::MapKey::String(":language".to_string())) {
            if let Value::String(lang) = lang_val {
                cap_map.insert(
                    MapKey::String(":language".to_string()),
                    Expression::Literal(Literal::String(lang.clone())),
                );
            }
        }
    }

    Ok(Expression::Map(cap_map))
}

/// Prepare a plan for registration as a capability with automatic effects propagation
/// 
/// This function:
/// 1. Analyzes the plan's body to extract capability calls
/// 2. Looks up capabilities using the provided lookup function to get their effects/permissions
/// 3. Conservatively propagates all effects and permissions to the wrapper capability
/// 4. Converts the plan to a capability map with propagated effects
/// 5. Returns the capability ID and map for registration
/// 
/// The `capability_lookup` function should return `Some(CapabilityManifest)` if the
/// capability is found, or `None` if not found. If None is always returned,
/// falls back to using plan's capabilities-required as effects.
pub fn prepare_plan_as_capability_with_propagation<F>(
    plan: &Plan,
    config: PlanAsCapabilityConfig,
    capability_lookup: Option<F>,
) -> Result<(String, Expression), RtfsBridgeError>
where
    F: Fn(&str) -> Option<CapabilityManifest>,
{
    // Extract capability ID (before moving config)
    let capability_id = config.capability_id.clone().unwrap_or_else(|| {
        plan.name
            .as_ref()
            .map(|n| format!("plan.{}", n))
            .unwrap_or_else(|| "plan.unnamed".to_string())
    });

    // If capability lookup is available, propagate effects and permissions
    let mut config_with_propagation = config;
    if let Some(lookup) = capability_lookup {
        let propagation_config = EffectsPropagationConfig::default();
        let propagated = propagate_effects_from_plan(plan, lookup, propagation_config)?;

        // Merge propagated effects with config effects (config takes precedence)
        if !propagated.effects.is_empty() && config_with_propagation.effects.is_none() {
            config_with_propagation.effects = Some(propagated.effects);
        }

        // Merge propagated permissions with config permissions
        if !propagated.permissions.is_empty() && config_with_propagation.permissions.is_empty() {
            config_with_propagation.permissions = propagated.permissions;
        }
    }

    // Convert plan to capability map
    let cap_map_expr = plan_to_capability_map(plan, config_with_propagation)?;

    // Return capability ID and map for registration
    Ok((capability_id, cap_map_expr))
}

/// Prepare a plan for registration as a capability
/// 
/// This is a convenience function that:
/// 1. Converts the plan to a capability map
/// 2. Validates the conversion
/// 3. Returns the capability ID for registration
/// 
/// Note: Full implementation would need access to the marketplace and orchestrator
/// to actually register and execute the plan. This is a placeholder that validates
/// the conversion and returns the capability ID.
/// 
/// To register, use the returned capability map with the marketplace's registration
/// methods after converting it to a CapabilityManifest.
/// 
/// For automatic effects propagation, use `prepare_plan_as_capability_with_propagation`.
pub fn prepare_plan_as_capability(
    plan: &Plan,
    config: PlanAsCapabilityConfig,
) -> Result<(String, Expression), RtfsBridgeError> {
    // Use a dummy lookup function that always returns None
    prepare_plan_as_capability_with_propagation(plan, config, Option::<fn(&str) -> Option<CapabilityManifest>>::None)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{Plan, PlanBody, PlanLanguage, PlanStatus};

    fn create_test_plan() -> Plan {
        Plan::new_named(
            "test-plan".to_string(),
            PlanLanguage::Rtfs20,
            PlanBody::Rtfs("(do (step \"test\" {}))".to_string()),
            vec![],
        )
    }

    #[test]
    fn test_plan_to_capability_map() {
        let plan = create_test_plan();
        let config = PlanAsCapabilityConfig::new()
            .with_id("plan.test".to_string())
            .with_name("Test Plan Capability".to_string());

        let result = plan_to_capability_map(&plan, config);
        assert!(result.is_ok());

        if let Ok(Expression::Map(cap_map)) = result {
            // Check required fields
            assert!(cap_map.contains_key(&MapKey::String(":type".to_string())));
            assert!(cap_map.contains_key(&MapKey::String(":id".to_string())));
            assert!(cap_map.contains_key(&MapKey::String(":name".to_string())));
            assert!(cap_map.contains_key(&MapKey::String(":version".to_string())));
            assert!(cap_map.contains_key(&MapKey::String(":description".to_string())));
            assert!(cap_map.contains_key(&MapKey::String(":implementation".to_string())));
            assert!(cap_map.contains_key(&MapKey::String(":language".to_string())));
            assert!(cap_map.contains_key(&MapKey::String(":provider".to_string())));
        } else {
            panic!("Expected Map expression");
        }
    }

    #[test]
    fn test_plan_to_capability_map_defaults() {
        let plan = create_test_plan();
        let config = PlanAsCapabilityConfig::default();

        let result = plan_to_capability_map(&plan, config);
        assert!(result.is_ok());

        if let Ok(Expression::Map(cap_map)) = result {
            // Check that ID defaults to "plan.{name}"
            if let Some(Expression::Literal(Literal::String(id))) =
                cap_map.get(&MapKey::String(":id".to_string()))
            {
                assert!(id.starts_with("plan."));
            }
        }
    }
}
