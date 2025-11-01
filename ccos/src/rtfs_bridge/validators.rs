use super::errors::RtfsBridgeError;
use crate::types::{Intent, Plan};

/// Validates a CCOS Intent extracted from RTFS
pub fn validate_intent(intent: &Intent) -> Result<(), RtfsBridgeError> {
    // Check required fields
    if intent.goal.is_empty() {
        return Err(RtfsBridgeError::ValidationFailed {
            message: "Intent goal cannot be empty".to_string(),
        });
    }

    if intent.intent_id.is_empty() {
        return Err(RtfsBridgeError::ValidationFailed {
            message: "Intent ID cannot be empty".to_string(),
        });
    }

    // Validate constraints format
    for (key, _value) in &intent.constraints {
        if key.trim().is_empty() {
            return Err(RtfsBridgeError::ValidationFailed {
                message: "Constraint key cannot be empty".to_string(),
            });
        }
    }

    // Validate preferences format
    for (key, _value) in &intent.preferences {
        if key.trim().is_empty() {
            return Err(RtfsBridgeError::ValidationFailed {
                message: "Preference key cannot be empty".to_string(),
            });
        }
    }

    Ok(())
}

/// Validates a CCOS Plan extracted from RTFS
pub fn validate_plan(plan: &Plan) -> Result<(), RtfsBridgeError> {
    // Check required fields
    if plan.plan_id.is_empty() {
        return Err(RtfsBridgeError::ValidationFailed {
            message: "Plan ID cannot be empty".to_string(),
        });
    }

    // Validate plan body
    match &plan.body {
        crate::types::PlanBody::Rtfs(rtfs_code) => {
            if rtfs_code.trim().is_empty() {
                return Err(RtfsBridgeError::ValidationFailed {
                    message: "Plan RTFS body cannot be empty".to_string(),
                });
            }
        }
        crate::types::PlanBody::Wasm(_) => {
            // WASM bodies are binary, so we can't check for emptiness in the same way
            // This is handled by the WASM runtime
        }
    }

    // Validate intent IDs format
    for (i, intent_id) in plan.intent_ids.iter().enumerate() {
        if intent_id.trim().is_empty() {
            return Err(RtfsBridgeError::ValidationFailed {
                message: format!("Intent ID {} cannot be empty", i),
            });
        }
    }

    // Validate capabilities required format
    for (i, capability) in plan.capabilities_required.iter().enumerate() {
        if capability.trim().is_empty() {
            return Err(RtfsBridgeError::ValidationFailed {
                message: format!("Required capability {} cannot be empty", i),
            });
        }
    }

    // Validate policies format
    for (key, _value) in &plan.policies {
        if key.trim().is_empty() {
            return Err(RtfsBridgeError::ValidationFailed {
                message: "Policy key cannot be empty".to_string(),
            });
        }
    }

    // Validate annotations format
    for (key, _value) in &plan.annotations {
        if key.trim().is_empty() {
            return Err(RtfsBridgeError::ValidationFailed {
                message: "Annotation key cannot be empty".to_string(),
            });
        }
    }

    Ok(())
}

/// Validates that a Plan's input schema is compatible with its Intent's constraints
pub fn validate_plan_intent_compatibility(
    plan: &Plan,
    intent: &Intent,
) -> Result<(), RtfsBridgeError> {
    // Check that the plan references the intent
    if !plan.intent_ids.contains(&intent.intent_id) {
        return Err(RtfsBridgeError::ValidationFailed {
            message: format!("Plan does not reference intent '{}'", intent.intent_id),
        });
    }

    // TODO: Add more sophisticated validation of input schema vs intent constraints
    // This would involve analyzing the RTFS body and checking that it can handle
    // the constraints defined in the intent

    Ok(())
}
