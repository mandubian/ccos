use super::errors::RtfsBridgeError;
use crate::rtfs_bridge::pretty_printer::expression_to_inline_string;
pub use crate::rtfs_bridge::pretty_printer::expression_to_rtfs_string;
use crate::types::{Intent, IntentStatus, Plan};
use rtfs::ast::{Expression, Keyword, Literal, MapKey};
use rtfs::runtime::values::Value;
use std::collections::HashMap;

/// Extracts a CCOS Intent from an RTFS expression
///
/// Supports both function call format: `(ccos/intent "name" :goal "..." :constraints {...})`
/// and map format: `{:type "intent" :name "..." :goal "..."}`
pub fn extract_intent_from_rtfs(expr: &Expression) -> Result<Intent, RtfsBridgeError> {
    match expr {
        Expression::FunctionCall { callee, arguments } => {
            extract_intent_from_function_call(callee, arguments)
        }
        Expression::Map(map) => extract_intent_from_map(map),
        _ => Err(RtfsBridgeError::InvalidObjectFormat {
            message: format!(
                "Expected FunctionCall or Map for Intent, got {}",
                expression_to_rtfs_string(expr)
            ),
        }),
    }
}

/// Extracts a CCOS Plan from an RTFS expression
///
/// Supports both function call format: `(ccos/plan "name" :body (...))`
/// and map format: `{:type "plan" :name "..." :body (...)}`
///
/// This function normalizes function-call syntax to canonical maps before extraction.
pub fn extract_plan_from_rtfs(expr: &Expression) -> Result<Plan, RtfsBridgeError> {
    use super::normalizer::{normalize_plan_to_map, NormalizationConfig};

    // Normalize to canonical map format first (handles function-call syntax)
    let normalized = normalize_plan_to_map(
        expr,
        NormalizationConfig {
            warn_on_function_call: true,
            validate_after_normalization: true,
        },
    )?;

    // Extract from normalized map (guaranteed to be a map after normalization)
    let plan = match normalized {
        Expression::Map(map) => extract_plan_from_map(&map)?,
        _ => {
            return Err(RtfsBridgeError::InvalidObjectFormat {
                message: "Normalization should produce a map".to_string(),
            });
        }
    };

    // Validate the extracted plan (including TypeExpr-aware schema validation)
    use super::validators::validate_plan;
    validate_plan(&plan)?;

    Ok(plan)
}

fn extract_intent_from_function_call(
    callee: &Expression,
    arguments: &[Expression],
) -> Result<Intent, RtfsBridgeError> {
    // Check if this is a CCOS intent function call
    let callee_name = if let Expression::Symbol(symbol) = callee {
        &symbol.0
    } else {
        return Err(RtfsBridgeError::InvalidCcosFunctionCall {
            message: "Expected symbol as function name".to_string(),
        });
    };

    // Accept both "ccos/intent" and "intent" for flexibility with LLM-generated code
    if callee_name != "ccos/intent" && callee_name != "intent" {
        return Err(RtfsBridgeError::InvalidCcosFunctionCall {
            message: format!(
                "Expected ccos/intent or intent function call, got {}",
                callee_name
            ),
        });
    }

    // Extract name from first argument
    let name = if let Some(first_arg) = arguments.first() {
        match first_arg {
            Expression::Literal(Literal::String(s)) => s.clone(),
            _ => {
                return Err(RtfsBridgeError::InvalidFieldType {
                    field: "name".to_string(),
                    expected: "string literal".to_string(),
                    actual: expression_to_inline_string(first_arg),
                })
            }
        }
    } else {
        return Err(RtfsBridgeError::MissingRequiredField {
            field: "name".to_string(),
        });
    };

    // Extract other fields from keyword arguments or a single Map arg
    let mut goal = None;
    let mut constraints = HashMap::new();
    let mut preferences = HashMap::new();
    let mut success_criteria = None;

    // Helper to handle a map expression that may be provided as a single map arg
    let mut handle_map = |map: &HashMap<MapKey, Expression>| {
        for (key, value) in map {
            let key_name = map_key_to_string(key);
            match key_name.as_str() {
                ":goal" | "goal" => {
                    goal = Some(expression_to_inline_string(value));
                }
                ":constraints" | "constraints" => {
                    if let Expression::Map(constraints_map) = value {
                        for (c_key, c_value) in constraints_map {
                            constraints
                                .insert(map_key_to_string(c_key), expression_to_value(c_value));
                        }
                    }
                }
                ":preferences" | "preferences" => {
                    if let Expression::Map(prefs_map) = value {
                        for (p_key, p_value) in prefs_map {
                            preferences
                                .insert(map_key_to_string(p_key), expression_to_value(p_value));
                        }
                    }
                }
                ":success-criteria" | "success-criteria" => {
                    success_criteria = Some(Value::from(value.clone()));
                }
                _ => {
                    // Ignore unknown fields
                }
            }
        }
    };

    // Two supported styles from LLMs:
    // 1) A single map argument: (intent "name" {:goal "..." :constraints {...}})
    // 2) Keyword-style args: (intent "name" :goal "..." :constraints {...})
    // Prefer the single map if present, otherwise parse keyword pairs directly.
    if arguments.len() >= 2 {
        // If a single map was provided as the second arg, handle it and ignore other args
        if let Expression::Map(map) = &arguments[1] {
            handle_map(map);
        } else {
            // Parse keyword-style pairs: iterate with index
            let mut i = 1;
            while i < arguments.len() {
                match &arguments[i] {
                    Expression::Literal(Literal::Keyword(kw)) => {
                        // next element should be the value
                        if i + 1 < arguments.len() {
                            let val = &arguments[i + 1];
                            // insert based on kw
                            match kw.0.as_str() {
                                "goal" => {
                                    goal = Some(expression_to_inline_string(val));
                                }
                                "constraints" => {
                                    if let Expression::Map(constraints_map) = val {
                                        for (c_key, c_value) in constraints_map {
                                            constraints.insert(
                                                map_key_to_string(c_key),
                                                expression_to_value(c_value),
                                            );
                                        }
                                    }
                                }
                                "preferences" => {
                                    if let Expression::Map(prefs_map) = val {
                                        for (p_key, p_value) in prefs_map {
                                            preferences.insert(
                                                map_key_to_string(p_key),
                                                expression_to_value(p_value),
                                            );
                                        }
                                    }
                                }
                                "success-criteria" => {
                                    success_criteria = Some(Value::from(val.clone()));
                                }
                                _ => {}
                            }
                            i += 2;
                        } else {
                            break;
                        }
                    }
                    // Accept bare symbol keys too
                    Expression::Symbol(sym) => {
                        if i + 1 < arguments.len() {
                            let val = &arguments[i + 1];
                            match sym.0.as_str() {
                                "goal" => {
                                    goal = Some(expression_to_inline_string(val));
                                }
                                "constraints" => {
                                    if let Expression::Map(constraints_map) = val {
                                        for (c_key, c_value) in constraints_map {
                                            constraints.insert(
                                                map_key_to_string(c_key),
                                                expression_to_value(c_value),
                                            );
                                        }
                                    }
                                }
                                "preferences" => {
                                    if let Expression::Map(prefs_map) = val {
                                        for (p_key, p_value) in prefs_map {
                                            preferences.insert(
                                                map_key_to_string(p_key),
                                                expression_to_value(p_value),
                                            );
                                        }
                                    }
                                }
                                "success-criteria" => {
                                    success_criteria = Some(Value::from(val.clone()));
                                }
                                _ => {}
                            }
                            i += 2;
                        } else {
                            break;
                        }
                    }
                    _ => {
                        // skip unknown shapes
                        i += 1;
                    }
                }
            }
        }
    }

    let goal = goal.ok_or_else(|| RtfsBridgeError::MissingRequiredField {
        field: "goal".to_string(),
    })?;

    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs();

    Ok(Intent {
        intent_id: format!("intent-{}", uuid::Uuid::new_v4()),
        name: Some(name),
        original_request: goal.clone(), // Use goal as original request for now
        goal,
        constraints,
        preferences,
        success_criteria,
        status: IntentStatus::Active,
        created_at: now,
        updated_at: now,
        metadata: HashMap::new(),
    })
}

fn extract_intent_from_map(map: &HashMap<MapKey, Expression>) -> Result<Intent, RtfsBridgeError> {
    // Check if this is a CCOS intent map
    let object_type =
        get_string_from_map(map, ":type").ok_or_else(|| RtfsBridgeError::MissingRequiredField {
            field: "type".to_string(),
        })?;

    if object_type != "intent" {
        return Err(RtfsBridgeError::UnsupportedObjectType {
            object_type: object_type.clone(),
        });
    }

    let name =
        get_string_from_map(map, ":name").ok_or_else(|| RtfsBridgeError::MissingRequiredField {
            field: "name".to_string(),
        })?;

    let goal =
        get_string_from_map(map, ":goal").ok_or_else(|| RtfsBridgeError::MissingRequiredField {
            field: "goal".to_string(),
        })?;

    // Extract other fields...
    let mut constraints = HashMap::new();
    let mut preferences = HashMap::new();
    let mut success_criteria = None;

    if let Some(Expression::Map(constraints_map)) = map_get(map, ":constraints") {
        for (key, value) in constraints_map {
            constraints.insert(map_key_to_string(key), expression_to_value(value));
        }
    }

    if let Some(Expression::Map(prefs_map)) = map_get(map, ":preferences") {
        for (key, value) in prefs_map {
            preferences.insert(map_key_to_string(key), expression_to_value(value));
        }
    }

    if let Some(criteria_expr) = map_get(map, ":success-criteria") {
        success_criteria = Some(Value::from(criteria_expr.clone()));
    }

    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs();

    Ok(Intent {
        intent_id: format!("intent-{}", uuid::Uuid::new_v4()),
        name: Some(name),
        original_request: goal.clone(), // Use goal as original request for now
        goal,
        constraints,
        preferences,
        success_criteria,
        status: IntentStatus::Active,
        created_at: now,
        updated_at: now,
        metadata: HashMap::new(),
    })
}

#[allow(dead_code)]
fn extract_plan_from_function_call(
    callee: &Expression,
    arguments: &[Expression],
) -> Result<Plan, RtfsBridgeError> {
    // Check if this is a CCOS plan function call
    let callee_name = if let Expression::Symbol(symbol) = callee {
        &symbol.0
    } else {
        return Err(RtfsBridgeError::InvalidCcosFunctionCall {
            message: "Expected symbol as function name".to_string(),
        });
    };

    // Accept both "ccos/plan" and "plan" for flexibility with LLM-generated code
    if callee_name != "ccos/plan" && callee_name != "plan" {
        return Err(RtfsBridgeError::InvalidCcosFunctionCall {
            message: format!(
                "Expected ccos/plan or plan function call, got {}",
                callee_name
            ),
        });
    }

    // Extract name from first argument
    let name = if let Some(first_arg) = arguments.first() {
        match first_arg {
            Expression::Literal(Literal::String(s)) => s.clone(),
            _ => {
                return Err(RtfsBridgeError::InvalidFieldType {
                    field: "name".to_string(),
                    expected: "string literal".to_string(),
                    actual: expression_to_inline_string(first_arg),
                })
            }
        }
    } else {
        return Err(RtfsBridgeError::MissingRequiredField {
            field: "name".to_string(),
        });
    };

    // Extract body and other fields from keyword arguments
    let mut body = None;
    let mut intent_ids = Vec::new();
    let mut input_schema = None;
    let mut output_schema = None;
    let mut policies = HashMap::new();
    let mut capabilities_required = Vec::new();
    let mut annotations = HashMap::new();

    for arg in &arguments[1..] {
        if let Expression::Map(map) = arg {
            for (key, value) in map {
                let key_name = map_key_to_string(key);
                match key_name.as_str() {
                    ":body" | "body" => {
                        body = Some(value.clone());
                    }
                    ":intent-ids" | "intent-ids" => {
                        if let Expression::Vector(ids_vec) = value {
                            for id_expr in ids_vec {
                                intent_ids.push(expression_to_inline_string(id_expr));
                            }
                        }
                    }
                    ":input-schema" | "input-schema" => {
                        input_schema = Some(Value::from(value.clone()));
                    }
                    ":output-schema" | "output-schema" => {
                        output_schema = Some(Value::from(value.clone()));
                    }
                    ":policies" | "policies" => {
                        if let Expression::Map(policies_map) = value {
                            for (p_key, p_value) in policies_map {
                                policies
                                    .insert(map_key_to_string(p_key), Value::from(p_value.clone()));
                            }
                        }
                    }
                    ":capabilities-required" | "capabilities-required" => {
                        if let Expression::Vector(caps_vec) = value {
                            for cap_expr in caps_vec {
                                capabilities_required.push(expression_to_inline_string(cap_expr));
                            }
                        }
                    }
                    ":annotations" | "annotations" => {
                        if let Expression::Map(ann_map) = value {
                            for (a_key, a_value) in ann_map {
                                annotations
                                    .insert(map_key_to_string(a_key), Value::from(a_value.clone()));
                            }
                        }
                    }
                    ":goal" | "goal" => {
                        // Store goal in annotations for now, so it can be extracted later
                        annotations.insert("goal".to_string(), expression_to_value(value));
                    }
                    ":metadata" | "metadata" => {
                        if let Expression::Map(_meta_map) = value {
                            // Extract to metadata if we had a metadata field, but we are using metadata from annotations?
                            // Plan has metadata field too.
                            // Let's just merge into metadata if found.
                        }
                    }
                    _ => {
                        // Ignore unknown fields
                    }
                }
            }
        }
    }

    let body = body.ok_or_else(|| RtfsBridgeError::MissingRequiredField {
        field: "body".to_string(),
    })?;

    Ok(Plan {
        plan_id: format!("plan-{}", uuid::Uuid::new_v4()),
        name: Some(name),
        session_id: None,
        intent_ids,
        language: crate::types::PlanLanguage::Rtfs20,
        body: crate::types::PlanBody::Rtfs(expression_to_rtfs_string(&body)),
        status: crate::types::PlanStatus::Draft,
        created_at: std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs(),
        metadata: HashMap::new(),
        input_schema,
        output_schema,
        policies,
        capabilities_required,
        annotations,
    })
}

fn extract_plan_from_map(map: &HashMap<MapKey, Expression>) -> Result<Plan, RtfsBridgeError> {
    // Check if this is a CCOS plan map
    let object_type =
        get_string_from_map(map, ":type").ok_or_else(|| RtfsBridgeError::MissingRequiredField {
            field: "type".to_string(),
        })?;

    if object_type != "plan" {
        return Err(RtfsBridgeError::UnsupportedObjectType {
            object_type: object_type.clone(),
        });
    }

    let name =
        get_string_from_map(map, ":name").ok_or_else(|| RtfsBridgeError::MissingRequiredField {
            field: "name".to_string(),
        })?;

    let body = map_get(map, ":body").ok_or_else(|| RtfsBridgeError::MissingRequiredField {
        field: "body".to_string(),
    })?;

    // Extract other fields...
    let mut intent_ids = Vec::new();
    let mut input_schema = None;
    let mut output_schema = None;
    let mut policies = HashMap::new();
    let mut capabilities_required = Vec::new();
    let mut annotations = HashMap::new();

    if let Some(Expression::Vector(ids_vec)) = map_get(map, ":intent-ids") {
        for id_expr in ids_vec {
            intent_ids.push(expression_to_inline_string(id_expr));
        }
    }

    if let Some(schema) = map_get(map, ":input-schema") {
        input_schema = Some(Value::from(schema.clone()));
    }

    if let Some(schema) = map_get(map, ":output-schema") {
        output_schema = Some(Value::from(schema.clone()));
    }

    if let Some(Expression::Map(policies_map)) = map_get(map, ":policies") {
        for (key, value) in policies_map {
            policies.insert(map_key_to_string(key), Value::from(value.clone()));
        }
    }

    if let Some(Expression::Vector(caps_vec)) = map_get(map, ":capabilities-required") {
        for cap_expr in caps_vec {
            capabilities_required.push(expression_to_inline_string(cap_expr));
        }
    }

    if let Some(Expression::Map(ann_map)) = map_get(map, ":annotations") {
        for (key, value) in ann_map {
            annotations.insert(map_key_to_string(key), Value::from(value.clone()));
        }
    }

    if let Some(goal_expr) = map_get(map, ":goal") {
        annotations.insert("goal".to_string(), expression_to_value(goal_expr));
    }

    let mut metadata = HashMap::new();
    if let Some(Expression::Map(meta_map)) = map_get(map, ":metadata") {
        for (key, value) in meta_map {
            metadata.insert(map_key_to_string(key), expression_to_value(value));
        }
    }

    if let Some(goal_expr) = map_get(map, ":goal") {
        annotations.insert("goal".to_string(), expression_to_value(goal_expr));
    }

    let mut metadata = HashMap::new();
    if let Some(Expression::Map(meta_map)) = map_get(map, ":metadata") {
        for (key, value) in meta_map {
            metadata.insert(map_key_to_string(key), expression_to_value(value));
        }
    }

    Ok(Plan {
        plan_id: format!("plan-{}", uuid::Uuid::new_v4()),
        name: Some(name),
        session_id: None,
        intent_ids,
        language: crate::types::PlanLanguage::Rtfs20,
        body: crate::types::PlanBody::Rtfs(expression_to_rtfs_string(&body)),
        status: crate::types::PlanStatus::Draft,
        created_at: std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs(),
        metadata: HashMap::new(),
        input_schema,
        output_schema,
        policies,
        capabilities_required,
        annotations,
    })
}

// Helper functions
fn map_key_to_string(key: &MapKey) -> String {
    match key {
        MapKey::String(s) => s.clone(),
        MapKey::Keyword(s) => s.0.clone(),
        MapKey::Integer(i) => i.to_string(),
    }
}

fn get_string_from_map(map: &HashMap<MapKey, Expression>, key: &str) -> Option<String> {
    map_get(map, key).and_then(|expr| match expr {
        Expression::Literal(Literal::String(s)) => Some(s.clone()),
        Expression::Symbol(s) => Some(s.0.clone()),
        _ => None,
    })
}

fn map_get<'a>(map: &'a HashMap<MapKey, Expression>, key: &str) -> Option<&'a Expression> {
    let kstr = key.to_string();
    let trimmed = key.trim_start_matches(':');
    map
        // Try exact string (with colon), then plain string (no colon), then keyword form
        .get(&MapKey::String(kstr))
        .or_else(|| map.get(&MapKey::String(trimmed.to_string())))
        .or_else(|| map.get(&MapKey::Keyword(Keyword(trimmed.to_string()))))
}

fn expression_to_value(expr: &Expression) -> Value {
    match expr {
        Expression::Literal(literal) => match literal {
            Literal::String(s) => Value::String(s.clone()),
            Literal::Integer(i) => Value::Integer(*i),
            Literal::Float(f) => Value::Float(*f),
            Literal::Boolean(b) => Value::Boolean(*b),
            Literal::Nil => Value::Nil,
            Literal::Keyword(k) => Value::Keyword(k.clone()),
            // Fall back to a readable string for unknown literal forms
            _ => Value::String(expression_to_inline_string(&Expression::Literal(
                literal.clone(),
            ))),
        },
        Expression::Symbol(s) => Value::Symbol(s.clone()),
        // For other expressions, produce a compact string representation rather than raw Debug
        _ => Value::String(expression_to_inline_string(expr)),
    }
}

/// Serialize a CapabilityDefinition TopLevel into canonical RTFS source string.
/// This builds an s-expression in the form:
/// (capability "name" :property1 <value> :property2 <value> ...)
/// It reuses expression_to_rtfs_string for nested expression values.
pub fn capability_def_to_rtfs_string(cap: &rtfs::ast::CapabilityDefinition) -> String {
    // Start with the capability name
    let mut parts: Vec<String> = Vec::new();
    parts.push("capability".to_string());
    parts.push(format!("\"{}\"", cap.name.0));

    // Serialize properties as keyword-value pairs
    for prop in &cap.properties {
        // property.key is a Keyword (wrapped struct) - we want the raw string without leading ':'
        let key = prop.key.0.clone();
        // Ensure keyword is prefixed with ':' for canonical form
        let key_formatted = if key.starts_with(':') {
            key
        } else {
            format!(":{}", key)
        };
        parts.push(key_formatted);
        // Serialize the value expression
        let val = expression_to_rtfs_string(&prop.value);
        parts.push(val);
    }

    format!("({})", parts.join(" "))
}

/// Convert a TopLevel node into an RTFS string when possible. Currently supports Capability.
pub fn toplevel_to_rtfs_string(tl: &rtfs::ast::TopLevel) -> Option<String> {
    match tl {
        rtfs::ast::TopLevel::Capability(cap) => Some(capability_def_to_rtfs_string(cap)),
        _ => None,
    }
}

// `expression_to_rtfs_string` is public now; use it directly.
