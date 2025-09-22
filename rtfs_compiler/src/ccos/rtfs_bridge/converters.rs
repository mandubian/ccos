use crate::ast::{Expression, MapKey, Symbol, Literal};
use crate::ccos::types::{Intent, Plan};
use crate::runtime::values::Value;
use super::errors::RtfsBridgeError;
use std::collections::HashMap;

/// Converts a CCOS Intent to an RTFS function call expression
/// 
/// Format: `(ccos/intent "name" :goal "..." :constraints {...} :preferences {...} :success-criteria ...)`
pub fn intent_to_rtfs_function_call(intent: &Intent) -> Result<Expression, RtfsBridgeError> {
    let mut args = vec![];
    
    // Add name as first argument
    if let Some(name) = &intent.name {
        args.push(Expression::Literal(Literal::String(name.clone())));
    } else {
        return Err(RtfsBridgeError::MissingRequiredField {
            field: "name".to_string()
        });
    }
    
    // Build keyword arguments map
    let mut keyword_args = HashMap::new();
    
    // Add goal
    keyword_args.insert(
        MapKey::String(":goal".to_string()),
        Expression::Literal(Literal::String(intent.goal.clone()))
    );
    
    // Add constraints if any
    if !intent.constraints.is_empty() {
        let mut constraints_map = HashMap::new();
        for (key, value) in &intent.constraints {
            constraints_map.insert(
                MapKey::String(key.clone()),
                value_to_expression(value)
            );
        }
        keyword_args.insert(
            MapKey::String(":constraints".to_string()),
            Expression::Map(constraints_map)
        );
    }
    
    // Add preferences if any
    if !intent.preferences.is_empty() {
        let mut preferences_map = HashMap::new();
        for (key, value) in &intent.preferences {
            preferences_map.insert(
                MapKey::String(key.clone()),
                value_to_expression(value)
            );
        }
        keyword_args.insert(
            MapKey::String(":preferences".to_string()),
            Expression::Map(preferences_map)
        );
    }
    
    // Add success criteria if any
    if let Some(criteria) = &intent.success_criteria {
        keyword_args.insert(
            MapKey::String(":success-criteria".to_string()),
            value_to_expression(criteria)
        );
    }
    
    args.push(Expression::Map(keyword_args));
    
    Ok(    Expression::FunctionCall {
        callee: Box::new(Expression::Symbol(Symbol("intent".to_string()))), // Use simpler name for LLM compatibility
        arguments: args,
    })
}

/// Converts a CCOS Intent to an RTFS map expression
/// 
/// Format: `{:type "intent" :name "..." :goal "..." :constraints {...} :preferences {...} :success-criteria ...}`
pub fn intent_to_rtfs_map(intent: &Intent) -> Result<Expression, RtfsBridgeError> {
    let mut map = HashMap::new();
    
    // Add type
    map.insert(
        MapKey::String(":type".to_string()),
        Expression::Literal(Literal::String("intent".to_string()))
    );
    
    // Add name
    if let Some(name) = &intent.name {
        map.insert(
            MapKey::String(":name".to_string()),
            Expression::Literal(Literal::String(name.clone()))
        );
    }
    
    // Add goal
    map.insert(
        MapKey::String(":goal".to_string()),
        Expression::Literal(Literal::String(intent.goal.clone()))
    );
    
    // Add constraints if any
    if !intent.constraints.is_empty() {
        let mut constraints_map = HashMap::new();
        for (key, value) in &intent.constraints {
            constraints_map.insert(
                MapKey::String(key.clone()),
                value_to_expression(value)
            );
        }
        map.insert(
            MapKey::String(":constraints".to_string()),
            Expression::Map(constraints_map)
        );
    }
    
    // Add preferences if any
    if !intent.preferences.is_empty() {
        let mut preferences_map = HashMap::new();
        for (key, value) in &intent.preferences {
            preferences_map.insert(
                MapKey::String(key.clone()),
                value_to_expression(value)
            );
        }
        map.insert(
            MapKey::String(":preferences".to_string()),
            Expression::Map(preferences_map)
        );
    }
    
    // Add success criteria if any
    if let Some(criteria) = &intent.success_criteria {
        map.insert(
            MapKey::String(":success-criteria".to_string()),
            value_to_expression(criteria)
        );
    }
    
    Ok(Expression::Map(map))
}

/// Converts a CCOS Plan to an RTFS function call expression
/// 
/// Format: `(ccos/plan "name" :body (...) :intent-ids [...] :input-schema {...} :output-schema {...} :policies {...} :capabilities-required [...] :annotations {...})`
pub fn plan_to_rtfs_function_call(plan: &Plan) -> Result<Expression, RtfsBridgeError> {
    let mut args = vec![];
    
    // Add name as first argument
    if let Some(name) = &plan.name {
        args.push(Expression::Literal(Literal::String(name.clone())));
    } else {
        return Err(RtfsBridgeError::MissingRequiredField {
            field: "name".to_string()
        });
    }
    
    // Build keyword arguments map
    let mut keyword_args = HashMap::new();
    
    // Add body
    match &plan.body {
        crate::ccos::types::PlanBody::Rtfs(rtfs_code) => {
            // Parse the RTFS code to get an Expression
            match crate::parser::parse_expression(rtfs_code) {
                Ok(body_expr) => {
                    keyword_args.insert(
                        MapKey::String(":body".to_string()),
                        body_expr
                    );
                }
                Err(_) => {
                    // If parsing fails, use as string literal
                    keyword_args.insert(
                        MapKey::String(":body".to_string()),
                        Expression::Literal(Literal::String(rtfs_code.clone()))
                    );
                }
            }
        }
        crate::ccos::types::PlanBody::Wasm(wasm_bytes) => {
            keyword_args.insert(
                MapKey::String(":body".to_string()),
                Expression::Literal(Literal::String(format!("<WASM_BYTES:{}_bytes>", wasm_bytes.len())))
            );
        }
    }
    
    // Add intent IDs if any
    if !plan.intent_ids.is_empty() {
        let ids_vec = plan.intent_ids.iter()
            .map(|id| Expression::Literal(Literal::String(id.clone())))
            .collect();
        keyword_args.insert(
            MapKey::String(":intent-ids".to_string()),
            Expression::Vector(ids_vec)
        );
    }
    
    // Add input schema if present
    if let Some(schema) = &plan.input_schema {
        keyword_args.insert(
            MapKey::String(":input-schema".to_string()),
            value_to_expression(schema)
        );
    }
    
    // Add output schema if present
    if let Some(schema) = &plan.output_schema {
        keyword_args.insert(
            MapKey::String(":output-schema".to_string()),
            value_to_expression(schema)
        );
    }
    
    // Add policies if any
    if !plan.policies.is_empty() {
        let mut policies_map = HashMap::new();
        for (key, value) in &plan.policies {
            policies_map.insert(
                MapKey::String(key.clone()),
                value_to_expression(value)
            );
        }
        keyword_args.insert(
            MapKey::String(":policies".to_string()),
            Expression::Map(policies_map)
        );
    }
    
    // Add capabilities required if any
    if !plan.capabilities_required.is_empty() {
        let caps_vec = plan.capabilities_required.iter()
            .map(|cap| Expression::Literal(Literal::String(cap.clone())))
            .collect();
        keyword_args.insert(
            MapKey::String(":capabilities-required".to_string()),
            Expression::Vector(caps_vec)
        );
    }
    
    // Add annotations if any
    if !plan.annotations.is_empty() {
        let mut annotations_map = HashMap::new();
        for (key, value) in &plan.annotations {
            annotations_map.insert(
                MapKey::String(key.clone()),
                value_to_expression(value)
            );
        }
        keyword_args.insert(
            MapKey::String(":annotations".to_string()),
            Expression::Map(annotations_map)
        );
    }
    
    args.push(Expression::Map(keyword_args));
    
    Ok(    Expression::FunctionCall {
        callee: Box::new(Expression::Symbol(Symbol("plan".to_string()))), // Use simpler name for LLM compatibility
        arguments: args,
    })
}

/// Converts a CCOS Plan to an RTFS map expression
/// 
/// Format: `{:type "plan" :name "..." :body (...) :intent-ids [...] :input-schema {...} :output-schema {...} :policies {...} :capabilities-required [...] :annotations {...}}`
pub fn plan_to_rtfs_map(plan: &Plan) -> Result<Expression, RtfsBridgeError> {
    let mut map = HashMap::new();
    
    // Add type
    map.insert(
        MapKey::String(":type".to_string()),
        Expression::Literal(Literal::String("plan".to_string()))
    );
    
    // Add name
    if let Some(name) = &plan.name {
        map.insert(
            MapKey::String(":name".to_string()),
            Expression::Literal(Literal::String(name.clone()))
        );
    }
    
    // Add body
    match &plan.body {
        crate::ccos::types::PlanBody::Rtfs(rtfs_code) => {
            // Parse the RTFS code to get an Expression
            match crate::parser::parse_expression(rtfs_code) {
                Ok(body_expr) => {
                    map.insert(
                        MapKey::String(":body".to_string()),
                        body_expr
                    );
                }
                Err(_) => {
                    // If parsing fails, use as string literal
                    map.insert(
                        MapKey::String(":body".to_string()),
                        Expression::Literal(Literal::String(rtfs_code.clone()))
                    );
                }
            }
        }
        crate::ccos::types::PlanBody::Wasm(wasm_bytes) => {
            map.insert(
                MapKey::String(":body".to_string()),
                Expression::Literal(Literal::String(format!("<WASM_BYTES:{}_bytes>", wasm_bytes.len())))
            );
        }
    }
    
    // Add intent IDs if any
    if !plan.intent_ids.is_empty() {
        let ids_vec = plan.intent_ids.iter()
            .map(|id| Expression::Literal(Literal::String(id.clone())))
            .collect();
        map.insert(
            MapKey::String(":intent-ids".to_string()),
            Expression::Vector(ids_vec)
        );
    }
    
    // Add input schema if present
    if let Some(schema) = &plan.input_schema {
        map.insert(
            MapKey::String(":input-schema".to_string()),
            value_to_expression(schema)
        );
    }
    
    // Add output schema if present
    if let Some(schema) = &plan.output_schema {
        map.insert(
            MapKey::String(":output-schema".to_string()),
            value_to_expression(schema)
        );
    }
    
    // Add policies if any
    if !plan.policies.is_empty() {
        let mut policies_map = HashMap::new();
        for (key, value) in &plan.policies {
            policies_map.insert(
                MapKey::String(key.clone()),
                value_to_expression(value)
            );
        }
        map.insert(
            MapKey::String(":policies".to_string()),
            Expression::Map(policies_map)
        );
    }
    
    // Add capabilities required if any
    if !plan.capabilities_required.is_empty() {
        let caps_vec = plan.capabilities_required.iter()
            .map(|cap| Expression::Literal(Literal::String(cap.clone())))
            .collect();
        map.insert(
            MapKey::String(":capabilities-required".to_string()),
            Expression::Vector(caps_vec)
        );
    }
    
    // Add annotations if any
    if !plan.annotations.is_empty() {
        let mut annotations_map = HashMap::new();
        for (key, value) in &plan.annotations {
            annotations_map.insert(
                MapKey::String(key.clone()),
                value_to_expression(value)
            );
        }
        map.insert(
            MapKey::String(":annotations".to_string()),
            Expression::Map(annotations_map)
        );
    }
    
    Ok(Expression::Map(map))
}

// Helper function to convert Value to Expression
fn value_to_expression(value: &Value) -> Expression {
    match value {
        Value::String(s) => Expression::Literal(Literal::String(s.clone())),
        Value::Integer(n) => Expression::Literal(Literal::Integer(*n)),
        Value::Float(f) => Expression::Literal(Literal::Float(*f)),
        Value::Boolean(b) => Expression::Literal(Literal::Boolean(*b)),
        Value::Nil => Expression::Literal(Literal::Nil),
        Value::Keyword(k) => Expression::Literal(Literal::Keyword(k.clone())),
        Value::Symbol(s) => Expression::Symbol(s.clone()),
        Value::Vector(v) => {
            let exprs = v.iter().map(value_to_expression).collect();
            Expression::Vector(exprs)
        }
        Value::Map(m) => {
            let map = m.iter().map(|(k, v)| (k.clone(), value_to_expression(v))).collect();
            Expression::Map(map)
        }
        Value::Timestamp(ts) => Expression::Literal(Literal::Timestamp(ts.clone())),
        Value::Uuid(uuid) => Expression::Literal(Literal::Uuid(uuid.clone())),
        Value::ResourceHandle(handle) => Expression::Literal(Literal::ResourceHandle(handle.clone())),
    // Value::Atom variant removed - no longer exists
        Value::List(_) => Expression::Literal(Literal::String("<list>".to_string())),
        Value::Function(_) => Expression::Literal(Literal::String("<function>".to_string())),
        Value::FunctionPlaceholder(_) => Expression::Literal(Literal::String("<function_placeholder>".to_string())),
        Value::Error(_) => Expression::Literal(Literal::String("<error>".to_string())),
    }
}
