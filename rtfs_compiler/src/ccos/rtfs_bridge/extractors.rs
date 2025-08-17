use crate::ast::{Expression, MapKey, Literal};
use crate::ccos::types::{Intent, Plan, IntentId, PlanId, IntentStatus};
use crate::runtime::values::Value;
use super::errors::RtfsBridgeError;
use std::collections::HashMap;

/// Extracts a CCOS Intent from an RTFS expression
/// 
/// Supports both function call format: `(ccos/intent "name" :goal "..." :constraints {...})`
/// and map format: `{:type "intent" :name "..." :goal "..."}`
pub fn extract_intent_from_rtfs(expr: &Expression) -> Result<Intent, RtfsBridgeError> {
    match expr {
        Expression::FunctionCall { callee, arguments } => extract_intent_from_function_call(callee, arguments),
        Expression::Map(map) => extract_intent_from_map(map),
        _ => Err(RtfsBridgeError::InvalidObjectFormat {
            message: format!("Expected FunctionCall or Map for Intent, got {:?}", expr)
        })
    }
}

/// Extracts a CCOS Plan from an RTFS expression
/// 
/// Supports both function call format: `(ccos/plan "name" :body (...))`
/// and map format: `{:type "plan" :name "..." :body (...)}`
pub fn extract_plan_from_rtfs(expr: &Expression) -> Result<Plan, RtfsBridgeError> {
    match expr {
        Expression::FunctionCall { callee, arguments } => extract_plan_from_function_call(callee, arguments),
        Expression::Map(map) => extract_plan_from_map(map),
        _ => Err(RtfsBridgeError::InvalidObjectFormat {
            message: format!("Expected FunctionCall or Map for Plan, got {:?}", expr)
        })
    }
}

fn extract_intent_from_function_call(callee: &Expression, arguments: &[Expression]) -> Result<Intent, RtfsBridgeError> {
    // Check if this is a CCOS intent function call
    let callee_name = if let Expression::Symbol(symbol) = callee {
        &symbol.0
    } else {
        return Err(RtfsBridgeError::InvalidCcosFunctionCall {
            message: "Expected symbol as function name".to_string()
        });
    };
    
    // Accept both "ccos/intent" and "intent" for flexibility with LLM-generated code
    if callee_name != "ccos/intent" && callee_name != "intent" {
        return Err(RtfsBridgeError::InvalidCcosFunctionCall {
            message: format!("Expected ccos/intent or intent function call, got {}", callee_name)
        });
    }
    
    // Extract name from first argument
    let name = if let Some(first_arg) = arguments.first() {
        match first_arg {
            Expression::Literal(Literal::String(s)) => s.clone(),
            _ => return Err(RtfsBridgeError::InvalidFieldType {
                field: "name".to_string(),
                expected: "string literal".to_string(),
                actual: format!("{:?}", first_arg)
            })
        }
    } else {
        return Err(RtfsBridgeError::MissingRequiredField {
            field: "name".to_string()
        });
    };
    
    // Extract other fields from keyword arguments
    let mut goal = None;
    let mut constraints = HashMap::new();
    let mut preferences = HashMap::new();
    let mut success_criteria = None;
    
    for arg in &arguments[1..] {
        if let Expression::Map(map) = arg {
            for (key, value) in map {
                match key {
                    MapKey::String(key_str) => {
                        match key_str.as_str() {
                            ":goal" => {
                                goal = Some(expression_to_string(value));
                            }
                            ":constraints" => {
                                if let Expression::Map(constraints_map) = value {
                                    for (c_key, c_value) in constraints_map {
                                        constraints.insert(map_key_to_string(c_key), expression_to_value(c_value));
                                    }
                                }
                            }
                            ":preferences" => {
                                if let Expression::Map(prefs_map) = value {
                                    for (p_key, p_value) in prefs_map {
                                        preferences.insert(map_key_to_string(p_key), expression_to_value(p_value));
                                    }
                                }
                            }
                            ":success-criteria" => {
                                success_criteria = Some(Value::from(value.clone()));
                            }
                            _ => {
                                // Ignore unknown fields
                            }
                        }
                    }
                    _ => {
                        // Ignore non-string keys
                    }
                }
            }
        }
    }
    
    let goal = goal.ok_or_else(|| RtfsBridgeError::MissingRequiredField {
        field: "goal".to_string()
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
    let object_type = get_string_from_map(map, ":type")
        .ok_or_else(|| RtfsBridgeError::MissingRequiredField {
            field: "type".to_string()
        })?;
    
    if object_type != "intent" {
        return Err(RtfsBridgeError::UnsupportedObjectType {
            object_type: object_type.clone()
        });
    }
    
    let name = get_string_from_map(map, ":name")
        .ok_or_else(|| RtfsBridgeError::MissingRequiredField {
            field: "name".to_string()
        })?;
    
    let goal = get_string_from_map(map, ":goal")
        .ok_or_else(|| RtfsBridgeError::MissingRequiredField {
            field: "goal".to_string()
        })?;
    
    // Extract other fields...
    let mut constraints = HashMap::new();
    let mut preferences = HashMap::new();
    let mut success_criteria = None;
    
    if let Some(Expression::Map(constraints_map)) = map.get(&MapKey::String(":constraints".to_string())) {
        for (key, value) in constraints_map {
            constraints.insert(map_key_to_string(key), expression_to_value(value));
        }
    }
    
    if let Some(Expression::Map(prefs_map)) = map.get(&MapKey::String(":preferences".to_string())) {
        for (key, value) in prefs_map {
            preferences.insert(map_key_to_string(key), expression_to_value(value));
        }
    }
    
    if let Some(criteria_expr) = map.get(&MapKey::String(":success-criteria".to_string())) {
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

fn extract_plan_from_function_call(callee: &Expression, arguments: &[Expression]) -> Result<Plan, RtfsBridgeError> {
    // Check if this is a CCOS plan function call
    let callee_name = if let Expression::Symbol(symbol) = callee {
        &symbol.0
    } else {
        return Err(RtfsBridgeError::InvalidCcosFunctionCall {
            message: "Expected symbol as function name".to_string()
        });
    };
    
    // Accept both "ccos/plan" and "plan" for flexibility with LLM-generated code
    if callee_name != "ccos/plan" && callee_name != "plan" {
        return Err(RtfsBridgeError::InvalidCcosFunctionCall {
            message: format!("Expected ccos/plan or plan function call, got {}", callee_name)
        });
    }
    
    // Extract name from first argument
    let name = if let Some(first_arg) = arguments.first() {
        match first_arg {
            Expression::Literal(Literal::String(s)) => s.clone(),
            _ => return Err(RtfsBridgeError::InvalidFieldType {
                field: "name".to_string(),
                expected: "string literal".to_string(),
                actual: format!("{:?}", first_arg)
            })
        }
    } else {
        return Err(RtfsBridgeError::MissingRequiredField {
            field: "name".to_string()
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
                match key {
                    MapKey::String(key_str) => {
                        match key_str.as_str() {
                            ":body" => {
                                body = Some(value.clone());
                            }
                            ":intent-ids" => {
                                if let Expression::Vector(ids_vec) = value {
                                    for id_expr in ids_vec {
                                        intent_ids.push(expression_to_string(id_expr));
                                    }
                                }
                            }
                            ":input-schema" => {
                                input_schema = Some(Value::from(value.clone()));
                            }
                            ":output-schema" => {
                                output_schema = Some(Value::from(value.clone()));
                            }
                            ":policies" => {
                                if let Expression::Map(policies_map) = value {
                                    for (p_key, p_value) in policies_map {
                                        policies.insert(map_key_to_string(p_key), Value::from(p_value.clone()));
                                    }
                                }
                            }
                            ":capabilities-required" => {
                                if let Expression::Vector(caps_vec) = value {
                                    for cap_expr in caps_vec {
                                        capabilities_required.push(expression_to_string(cap_expr));
                                    }
                                }
                            }
                            ":annotations" => {
                                if let Expression::Map(ann_map) = value {
                                    for (a_key, a_value) in ann_map {
                                        annotations.insert(map_key_to_string(a_key), Value::from(a_value.clone()));
                                    }
                                }
                            }
                            _ => {
                                // Ignore unknown fields
                            }
                        }
                    }
                    _ => {
                        // Ignore non-string keys
                    }
                }
            }
        }
    }
    
    let body = body.ok_or_else(|| RtfsBridgeError::MissingRequiredField {
        field: "body".to_string()
    })?;
    
    Ok(Plan {
        plan_id: format!("plan-{}", uuid::Uuid::new_v4()),
        name: Some(name),
        intent_ids,
        language: crate::ccos::types::PlanLanguage::Rtfs20,
        body: crate::ccos::types::PlanBody::Rtfs(expression_to_rtfs_string(&body)),
        status: crate::ccos::types::PlanStatus::Draft,
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
    let object_type = get_string_from_map(map, ":type")
        .ok_or_else(|| RtfsBridgeError::MissingRequiredField {
            field: "type".to_string()
        })?;
    
    if object_type != "plan" {
        return Err(RtfsBridgeError::UnsupportedObjectType {
            object_type: object_type.clone()
        });
    }
    
    let name = get_string_from_map(map, ":name")
        .ok_or_else(|| RtfsBridgeError::MissingRequiredField {
            field: "name".to_string()
        })?;
    
    let body = map.get(&MapKey::String(":body".to_string()))
        .ok_or_else(|| RtfsBridgeError::MissingRequiredField {
            field: "body".to_string()
        })?;
    
    // Extract other fields...
    let mut intent_ids = Vec::new();
    let mut input_schema = None;
    let mut output_schema = None;
    let mut policies = HashMap::new();
    let mut capabilities_required = Vec::new();
    let mut annotations = HashMap::new();
    
    if let Some(Expression::Vector(ids_vec)) = map.get(&MapKey::String(":intent-ids".to_string())) {
        for id_expr in ids_vec {
            intent_ids.push(expression_to_string(id_expr));
        }
    }
    
    if let Some(schema) = map.get(&MapKey::String(":input-schema".to_string())) {
        input_schema = Some(Value::from(schema.clone()));
    }
    
    if let Some(schema) = map.get(&MapKey::String(":output-schema".to_string())) {
        output_schema = Some(Value::from(schema.clone()));
    }
    
    if let Some(Expression::Map(policies_map)) = map.get(&MapKey::String(":policies".to_string())) {
        for (key, value) in policies_map {
            policies.insert(map_key_to_string(key), Value::from(value.clone()));
        }
    }
    
    if let Some(Expression::Vector(caps_vec)) = map.get(&MapKey::String(":capabilities-required".to_string())) {
        for cap_expr in caps_vec {
            capabilities_required.push(expression_to_string(cap_expr));
        }
    }
    
    if let Some(Expression::Map(ann_map)) = map.get(&MapKey::String(":annotations".to_string())) {
        for (key, value) in ann_map {
            annotations.insert(map_key_to_string(key), Value::from(value.clone()));
        }
    }
    
    Ok(Plan {
        plan_id: format!("plan-{}", uuid::Uuid::new_v4()),
        name: Some(name),
        intent_ids,
        language: crate::ccos::types::PlanLanguage::Rtfs20,
        body: crate::ccos::types::PlanBody::Rtfs(expression_to_rtfs_string(&body)),
        status: crate::ccos::types::PlanStatus::Draft,
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
fn expression_to_string(expr: &Expression) -> String {
    match expr {
        Expression::Literal(literal) => match literal {
            Literal::String(s) => s.clone(), // Return raw string for compatibility with existing tests
            Literal::Integer(i) => i.to_string(),
            Literal::Float(f) => f.to_string(),
            Literal::Boolean(b) => b.to_string(),
            Literal::Nil => "nil".to_string(),
            _ => format!("{:?}", literal),
        },
        Expression::Symbol(s) => s.0.clone(),
        Expression::FunctionCall { callee, arguments } => {
            let callee_str = expression_to_string(callee);
            let args_str = arguments.iter()
                .map(expression_to_string)
                .collect::<Vec<_>>()
                .join(" ");
            format!("({} {})", callee_str, args_str)
        },
        Expression::Do(do_expr) => {
            let exprs_str = do_expr.expressions.iter()
                .map(expression_to_string)
                .collect::<Vec<_>>()
                .join(" ");
            format!("(do {})", exprs_str)
        },
        _ => format!("{:?}", expr),
    }
}

fn expression_to_rtfs_string(expr: &Expression) -> String {
    match expr {
        Expression::Literal(literal) => match literal {
            Literal::String(s) => format!("\"{}\"", s), // Add quotes for proper RTFS syntax
            Literal::Integer(i) => i.to_string(),
            Literal::Float(f) => f.to_string(),
            Literal::Boolean(b) => b.to_string(),
            Literal::Nil => "nil".to_string(),
            _ => format!("{:?}", literal),
        },
        Expression::Symbol(s) => s.0.clone(),
        Expression::FunctionCall { callee, arguments } => {
            let callee_str = expression_to_rtfs_string(callee);
            let args_str = arguments.iter()
                .map(expression_to_rtfs_string)
                .collect::<Vec<_>>()
                .join(" ");
            format!("({} {})", callee_str, args_str)
        },
        Expression::Do(do_expr) => {
            let exprs_str = do_expr.expressions.iter()
                .map(expression_to_rtfs_string)
                .collect::<Vec<_>>()
                .join(" ");
            format!("(do {})", exprs_str)
        },
        _ => format!("{:?}", expr),
    }
}

fn map_key_to_string(key: &MapKey) -> String {
    match key {
        MapKey::String(s) => s.clone(),
        MapKey::Keyword(s) => s.0.clone(),
        MapKey::Integer(i) => i.to_string(),
    }
}

fn get_string_from_map(map: &HashMap<MapKey, Expression>, key: &str) -> Option<String> {
    map.get(&MapKey::String(key.to_string()))
        .and_then(|expr| match expr {
            Expression::Literal(Literal::String(s)) => Some(s.clone()),
            Expression::Symbol(s) => Some(s.0.clone()),
            _ => None,
        })
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
            _ => Value::String(format!("{:?}", literal)),
        },
        Expression::Symbol(s) => Value::Symbol(s.clone()),
        _ => Value::String(format!("{:?}", expr)),
    }
}
