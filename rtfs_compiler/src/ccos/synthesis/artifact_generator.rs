//! Artifact Generator
//!
//! Generates RTFS capability s-expressions from parameter schemas.
//! Uses (call ...) primitive for host delegation. Implements spec section 21.3-21.5.

use super::schema_builder::{ParamSchema, ParamTypeInfo};
use super::status::{STATUS_READY_FOR_EXECUTION, STATUS_REQUIRES_AGENT, STATUS_PROCESSING};
use crate::ccos::rtfs_bridge::extractors::capability_def_to_rtfs_string;
use crate::ast::{Expression, Literal, Symbol, CapabilityDefinition, Property, Keyword, MapKey, LetBinding, LetExpr, DoExpr};
use std::collections::HashMap;

/// Generate a collector capability that asks sequential questions.
pub fn generate_collector(schema: &ParamSchema, domain: &str) -> String {
    // (parameters -> built below using AST types)

    // Build Let bindings as AST LetBinding entries
    let mut bindings: Vec<LetBinding> = Vec::new();
    for (i, (_k, _meta)) in schema.params.iter().enumerate() {
        let var = format!("p{}", i + 1);
        let prompt = if _meta.prompt.is_empty() { _meta.key.clone() } else { _meta.prompt.clone() };
        let prompt_sane = prompt.replace('"', "'");
        // (call ccos.user.ask "prompt")
        let call = Expression::FunctionCall {
            callee: Box::new(Expression::Symbol(Symbol("call".into()))),
            arguments: vec![Expression::Symbol(Symbol("ccos.user.ask".into())), Expression::Literal(Literal::String(prompt_sane))],
        };
        bindings.push(LetBinding { pattern: crate::ast::Pattern::Symbol(Symbol(var.clone())), type_annotation: None, value: Box::new(call) });
    }

    // Build context map: {:key p1 :key2 p2}
    let mut context_map: HashMap<MapKey, Expression> = HashMap::new();
    for (i, (_k, meta)) in schema.params.iter().enumerate() {
        let var = format!("p{}", i + 1);
        context_map.insert(MapKey::Keyword(Keyword(meta.key.clone())), Expression::Symbol(Symbol(var)));
    }

    // inner let: (let [context { ... }] {:status "..." :context context})
    let inner_let = LetExpr { bindings: vec![LetBinding { pattern: crate::ast::Pattern::Symbol(Symbol("context".into())), type_annotation: None, value: Box::new(Expression::Map(context_map)) }], body: vec![Expression::Map({
        let mut m = HashMap::new();
        m.insert(MapKey::Keyword(Keyword("status".into())), Expression::Literal(Literal::String(STATUS_READY_FOR_EXECUTION.into())));
        m.insert(MapKey::Keyword(Keyword("context".into())), Expression::Symbol(Symbol("context".into())));
        m
    })] };

    let impl_do = Expression::Let(LetExpr { bindings, body: vec![Expression::Let(inner_let)] });

    // Build CapabilityDefinition using AST types
    let cap_def = CapabilityDefinition {
        name: Symbol(format!("{}.collector.v1", domain)),
        properties: vec![
            Property { key: Keyword("description".into()), value: Expression::Literal(Literal::String("AUTO-GENERATED COLLECTOR".into())) },
            Property { key: Keyword("parameters".into()), value: Expression::List(vec![]) },
            Property { key: Keyword("implementation".into()), value: impl_do },
        ],
    };

    capability_def_to_rtfs_string(&cap_def)
}

/// Generate a planner capability that processes context and produces output.
pub fn generate_planner(schema: &ParamSchema, domain: &str) -> String {
    // Build expects expression as AST
    let mut keys_expr: Vec<Expression> = Vec::new();
    for (_k, meta) in schema.params.iter() {
        keys_expr.push(Expression::Symbol(Symbol(format!(":{}", meta.key))));
    }

    let expects_expr = if keys_expr.is_empty() {
        Expression::List(vec![Expression::Symbol(Symbol(":expects".into())), Expression::List(vec![])])
    } else {
        Expression::List(vec![Expression::Symbol(Symbol(":expects".into())), Expression::List(vec![Expression::Symbol(Symbol(":context/keys".into())), Expression::List(keys_expr)])])
    };

    // (let [events (call ccos.<domain>.fetch-events {}) result events] {:status "..." :result result :context context})

    // Build AST for: (do (let [events (call ccos.<domain>.fetch-events {}) result events] {:status "processing" :result result :context context}))
    let events_call = Expression::FunctionCall { callee: Box::new(Expression::Symbol(Symbol(format!("ccos.{}.fetch-events", domain)))), arguments: vec![Expression::List(vec![])] };
    let let_bindings = vec![
        LetBinding { pattern: crate::ast::Pattern::Symbol(Symbol("events".into())), type_annotation: None, value: Box::new(events_call) },
        LetBinding { pattern: crate::ast::Pattern::Symbol(Symbol("result".into())), type_annotation: None, value: Box::new(Expression::Symbol(Symbol("events".into()))) },
    ];

    let mut result_map = HashMap::new();
    result_map.insert(MapKey::Keyword(Keyword("status".into())), Expression::Literal(Literal::String(STATUS_PROCESSING.into())));
    result_map.insert(MapKey::Keyword(Keyword("result".into())), Expression::Symbol(Symbol("result".into())));
    result_map.insert(MapKey::Keyword(Keyword("context".into())), Expression::Symbol(Symbol("context".into())));

    let impl_do = Expression::Do(DoExpr { expressions: vec![Expression::Let(LetExpr { bindings: let_bindings, body: vec![Expression::Map(result_map)] })] });

    let mut properties = vec![Property { key: Keyword("description".into()), value: Expression::Literal(Literal::String("AUTO-GENERATED PLANNER".into())) }, Property { key: Keyword("parameters".into()), value: Expression::Map({ let mut m = HashMap::new(); m.insert(MapKey::Keyword(Keyword("context".into())), Expression::Literal(Literal::String("map".into()))); m }) }];

    properties.push(Property { key: Keyword("expects".into()), value: expects_expr });
    properties.push(Property { key: Keyword("implementation".into()), value: impl_do });

    let cap_def = CapabilityDefinition { name: Symbol(format!("{}.planner.v1", domain)), properties };

    capability_def_to_rtfs_string(&cap_def)
}

/// Generate a stub capability when required agent is missing.
pub fn generate_stub(agent_id: &str, context_keys: &[String]) -> String {
    // Build expects for stub using AST
    let mut keys_expr: Vec<Expression> = Vec::new();
    for k in context_keys {
        keys_expr.push(Expression::Symbol(Symbol(format!(":{}", k))));
    }
    let expects_expr = if keys_expr.is_empty() {
        Expression::List(vec![Expression::Symbol(Symbol(":expects".into())), Expression::List(vec![])])
    } else {
        Expression::List(vec![Expression::Symbol(Symbol(":expects".into())), Expression::List(vec![Expression::Symbol(Symbol(":context/keys".into())), Expression::List(keys_expr)])])
    };

    // impl_do as AST Do with map result
    let mut impl_map = HashMap::new();
    impl_map.insert(MapKey::Keyword(Keyword("status".into())), Expression::Literal(Literal::String(STATUS_REQUIRES_AGENT.into())));
    impl_map.insert(MapKey::Keyword(Keyword("explanation".into())), Expression::Literal(Literal::String("Stub execution placeholder".into())));
    impl_map.insert(MapKey::Keyword(Keyword("context".into())), Expression::Symbol(Symbol("context".into())));

    let impl_do = Expression::Do(DoExpr { expressions: vec![Expression::Map(impl_map)] });

    let mut properties = vec![Property { key: Keyword("description".into()), value: Expression::Literal(Literal::String("[STUB] Auto-generated placeholder for missing agent".into())) }, Property { key: Keyword("parameters".into()), value: Expression::Map({ let mut m = HashMap::new(); m.insert(MapKey::Keyword(Keyword("context".into())), Expression::Literal(Literal::String("map".into()))); m }) }, Property { key: Keyword("expects".into()), value: expects_expr }];
    properties.push(Property { key: Keyword("implementation".into()), value: impl_do });

    let cap_def = CapabilityDefinition { name: Symbol(agent_id.into()), properties };
    format!("; AUTO-GENERATED STUB - REQUIRES IMPLEMENTATION\n{}", capability_def_to_rtfs_string(&cap_def))
}

/// Convert our local `Elem` builder into the repository `ast::Expression`.
// All generators now construct AST types directly; no builder placeholder needed.

/// Extract capability requirements from synthesized RTFS code.
pub fn extract_capability_requirements(rtfs_code: &str) -> Vec<String> {
    let mut reqs = Vec::new();
    for line in rtfs_code.lines() {
        if let Some(pos) = line.find("(call ccos.") {
            let start = pos + "(call ".len();
            // find end of token
            if let Some(end_rel) = line[start..].find(|c: char| c.is_whitespace() || c == ')' ) {
                let cap = line[start..start + end_rel].trim().to_string();
                reqs.push(cap);
            }
        }
    }
    reqs
}

/// Helper: format a parameter type for RTFS :parameters map
fn _format_param_type(param_type: &ParamTypeInfo) -> String {
    match param_type {
        ParamTypeInfo::String => "\"string\"".to_string(),
        ParamTypeInfo::Enum { values } => {
            format!("[:enum {}]", values.iter().map(|v| format!("\"{}\"", v)).collect::<Vec<_>>().join(" "))
        }
    }
}

// ---- RTFS serializer helpers (small and pragmatic) ----
fn rtfs_escape(s: &str) -> String {
    s.replace('\\', "\\\\").replace('"', "\\\"")
}

fn rtfs_str(s: &str) -> String {
    format!("\"{}\"", rtfs_escape(s))
}


