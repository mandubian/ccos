use std::collections::{HashMap, HashSet};

use rtfs::ast::{Expression, Keyword, Literal, MapKey};
use rtfs::parser::{parse, parse_expression};
use rtfs::runtime::error::RuntimeError;

use crate::intent_graph::IntentGraph;
use crate::rtfs_bridge::extract_intent_from_rtfs;
use crate::types::{
    EdgeType, GenerationContext, Intent, IntentId, IntentStatus, StorableIntent, TriggerSource,
};

/// Build an IntentGraph from a small RTFS graph DSL and return the root intent id.
///
/// Accepted shapes (minimal, LLM-friendly):
/// - Top-level (do ...) containing (intent ...) and (edge ...)
/// - Or multiple top-level items: intents and edges
///
/// Intent forms supported (both):
///   (intent "name" {:goal "..." ...})
///   {:type "intent" :name "name" :goal "..."}
///
/// Edge forms supported:
///   (edge {:from "child" :to "parent" :type :IsSubgoalOf})
///   (edge :DependsOn "from" "to")
pub fn build_graph_from_rtfs(
    rtfs: &str,
    graph: &mut IntentGraph,
) -> Result<IntentId, RuntimeError> {
    // Parse either a full program with multiple top-level items or a single expression
    let items = match parse(rtfs) {
        Ok(tops) => tops
            .into_iter()
            .filter_map(|t| match t {
                rtfs::ast::TopLevel::Expression(expr) => Some(expr),
                _ => None,
            })
            .collect::<Vec<_>>(),
        Err(_) => vec![parse_expression(rtfs)
            .map_err(|e| RuntimeError::Generic(format!("Failed to parse RTFS graph: {:?}", e)))?],
    };

    let mut name_to_id: HashMap<String, IntentId> = HashMap::new();
    // Fuzzy map to tolerate model variations in casing
    let mut name_to_id_lower: HashMap<String, IntentId> = HashMap::new();
    let mut inserted_names_in_order: Vec<String> = Vec::new();
    // Track IsSubgoalOf edges by names to infer root later if not explicit
    let mut subgoal_from_names: HashSet<String> = HashSet::new();
    let mut pending_edges: Vec<(String, String, EdgeType)> = Vec::new();

    // Helper: insert an intent Expression into the graph
    let mut insert_intent_expr = |expr: &Expression| -> Result<(), RuntimeError> {
        let intent: Intent = extract_intent_from_rtfs(expr)
            .map_err(|e| RuntimeError::Generic(format!("Invalid intent form: {}", e)))?;
        let name = intent
            .name
            .clone()
            .unwrap_or_else(|| intent.intent_id.clone());
        let now = intent.created_at;
        let storable = StorableIntent {
            intent_id: intent.intent_id.clone(),
            name: intent.name.clone(),
            original_request: intent.original_request.clone(),
            rtfs_intent_source: format!("(intent \"{}\" {{:goal \"{}\"}})", name, intent.goal),
            goal: intent.goal.clone(),
            constraints: intent
                .constraints
                .iter()
                .map(|(k, v)| (k.clone(), format!("{}", v)))
                .collect(),
            preferences: intent
                .preferences
                .iter()
                .map(|(k, v)| (k.clone(), format!("{}", v)))
                .collect(),
            success_criteria: intent.success_criteria.as_ref().map(|v| format!("{}", v)),
            parent_intent: None,
            child_intents: vec![],
            session_id: None,
            triggered_by: TriggerSource::HumanRequest,
            generation_context: GenerationContext {
                arbiter_version: "rtfs-graph-interpreter-1.0".to_string(),
                generation_timestamp: now,
                input_context: HashMap::new(),
                reasoning_trace: Some("Interpreted from RTFS graph".to_string()),
            },
            status: IntentStatus::Active,
            priority: 1,
            created_at: now,
            updated_at: now,
            metadata: HashMap::new(),
        };
        let id = storable.intent_id.clone();
        graph.store_intent(storable)?;
        name_to_id.insert(name.clone(), id.clone());
        name_to_id_lower.insert(name.to_lowercase(), id);
        inserted_names_in_order.push(name);
        Ok(())
    };

    // Helper: parse an edge Expression and buffer until all intents are inserted
    let mut parse_edge_expr = |expr: &Expression| -> Result<(), RuntimeError> {
        match expr {
            Expression::FunctionCall { callee, arguments } => {
                // Expect callee "edge" and either map form or (:Type from to)
                let cname = if let Expression::Symbol(sym) = &**callee {
                    sym.0.as_str()
                } else {
                    return Err(RuntimeError::Generic(
                        "edge form: callee must be symbol".to_string(),
                    ));
                };
                if cname != "edge" {
                    return Err(RuntimeError::Generic(format!(
                        "Unsupported form '{}', expected 'edge'",
                        cname
                    )));
                }

                if let Some(Expression::Map(m)) = arguments.first() {
                    // Map form
                    // Accept both string and keyword keys
                    let from = get_string(m, ":from")
                        .or_else(|| get_string(m, "from"))
                        .ok_or_else(|| {
                            RuntimeError::Generic("edge {:from ...} missing".to_string())
                        })?;
                    let to = get_string(m, ":to")
                        .or_else(|| get_string(m, "to"))
                        .ok_or_else(|| {
                            RuntimeError::Generic("edge {:to ...} missing".to_string())
                        })?;
                    let et = get_edge_type(map_get(m, ":type").or_else(|| map_get(m, "type")));
                    let et = et.ok_or_else(|| {
                        RuntimeError::Generic("edge {:type ...} invalid".to_string())
                    })?;
                    if matches!(et, EdgeType::IsSubgoalOf) {
                        subgoal_from_names.insert(from.clone());
                    }
                    pending_edges.push((from, to, et));
                    Ok(())
                } else if arguments.len() >= 3 {
                    // Positional form: (edge :DependsOn "from" "to")
                    let et = get_edge_type(arguments.get(0));
                    let from = expr_to_string(arguments.get(1).unwrap());
                    let to = expr_to_string(arguments.get(2).unwrap());
                    let et =
                        et.ok_or_else(|| RuntimeError::Generic("edge type invalid".to_string()))?;
                    if matches!(et, EdgeType::IsSubgoalOf) {
                        subgoal_from_names.insert(from.clone());
                    }
                    pending_edges.push((from, to, et));
                    Ok(())
                } else {
                    Err(RuntimeError::Generic("edge form invalid".to_string()))
                }
            }
            _ => Err(RuntimeError::Generic(
                "edge form must be a function call".to_string(),
            )),
        }
    };

    // Flatten items: handle (do ...) containing many expressions
    let mut seq: Vec<Expression> = Vec::new();
    for item in items {
        match item {
            Expression::Do(d) => {
                for e in d.expressions {
                    seq.push(e);
                }
            }
            other => seq.push(other),
        }
    }

    // First pass: insert intents
    for e in &seq {
        if is_intent_form(e) {
            insert_intent_expr(e)?;
        }
    }
    // Second pass: parse edges
    for e in &seq {
        if is_edge_form(e) {
            parse_edge_expr(e)?;
        }
    }

    // Create edges now that ids are known
    for (from_name, to_name, et) in pending_edges {
        // Resolve by exact name, then case-insensitive as a fallback
        let from_id = name_to_id
            .get(&from_name)
            .cloned()
            .or_else(|| name_to_id_lower.get(&from_name.to_lowercase()).cloned())
            .ok_or_else(|| {
                RuntimeError::Generic(format!("Unknown intent name for edge : {}", from_name))
            })?;
        let to_id = name_to_id
            .get(&to_name)
            .cloned()
            .or_else(|| name_to_id_lower.get(&to_name.to_lowercase()).cloned())
            .ok_or_else(|| {
                RuntimeError::Generic(format!("Unknown intent name for edge : {}", to_name))
            })?;
        graph.create_edge(from_id, to_id, et)?;
    }

    // Infer root: first intent that is not a subgoal child in any IsSubgoalOf edge
    let root_name = inserted_names_in_order
        .iter()
        .find(|n| !subgoal_from_names.contains(*n))
        .cloned()
        .unwrap_or_else(|| {
            inserted_names_in_order
                .first()
                .cloned()
                .unwrap_or_else(|| "".to_string())
        });
    let root_id = name_to_id
        .get(&root_name)
        .cloned()
        .ok_or_else(|| RuntimeError::Generic("No intents found in RTFS graph".to_string()))?;

    Ok(root_id)
}

fn is_intent_form(expr: &Expression) -> bool {
    match expr {
        Expression::FunctionCall { callee, .. } => {
            matches!(&**callee, Expression::Symbol(s) if s.0 == "intent" || s.0 == "ccos/intent")
        }
        Expression::Map(map) => {
            // Support both string and keyword keys for :type
            match map
                .get(&MapKey::String(":type".to_string()))
                .or_else(|| map.get(&MapKey::Keyword(Keyword("type".to_string()))))
            {
                Some(Expression::Literal(Literal::String(t))) if t == "intent" => true,
                _ => false,
            }
        }
        _ => false,
    }
}

fn is_edge_form(expr: &Expression) -> bool {
    matches!(expr, Expression::FunctionCall { callee, .. } if matches!(&**callee, Expression::Symbol(s) if s.0 == "edge"))
}

fn get_string(map: &HashMap<MapKey, Expression>, key: &str) -> Option<String> {
    map_get(map, key).map(|v| expr_to_string(v))
}

fn map_get<'a>(map: &'a HashMap<MapKey, Expression>, key: &str) -> Option<&'a Expression> {
    // Accept both ":key" as String and keyword :key
    let kstr = key.to_string();
    let trimmed = key.trim_start_matches(':');
    map.get(&MapKey::String(kstr))
        .or_else(|| map.get(&MapKey::Keyword(Keyword(trimmed.to_string()))))
}

fn expr_to_string(e: &Expression) -> String {
    match e {
        Expression::Literal(Literal::String(s)) => s.clone(),
        Expression::Symbol(s) => s.0.clone(),
        Expression::Literal(Literal::Keyword(Keyword(k))) => k.clone(),
        _ => format!("{}", format_expr_for_debug(e)),
    }
}

fn get_edge_type(e: Option<&Expression>) -> Option<EdgeType> {
    match e? {
        Expression::Literal(Literal::Keyword(Keyword(k)))
        | Expression::Symbol(rtfs::ast::Symbol(k)) => match k.as_str() {
            ":DependsOn" | "DependsOn" => Some(EdgeType::DependsOn),
            ":IsSubgoalOf" | "IsSubgoalOf" => Some(EdgeType::IsSubgoalOf),
            ":ConflictsWith" | "ConflictsWith" => Some(EdgeType::ConflictsWith),
            ":Enables" | "Enables" => Some(EdgeType::Enables),
            ":RelatedTo" | "RelatedTo" => Some(EdgeType::RelatedTo),
            ":TriggeredBy" | "TriggeredBy" => Some(EdgeType::TriggeredBy),
            ":Blocks" | "Blocks" => Some(EdgeType::Blocks),
            _ => None,
        },
        _ => None,
    }
}

fn format_expr_for_debug(e: &Expression) -> String {
    match e {
        Expression::Literal(Literal::String(s)) => format!("\"{}\"", s),
        Expression::Literal(Literal::Integer(i)) => i.to_string(),
        Expression::Literal(Literal::Float(f)) => f.to_string(),
        Expression::Literal(Literal::Boolean(b)) => b.to_string(),
        Expression::Literal(Literal::Keyword(Keyword(k))) => format!(":{}", k),
        Expression::Symbol(s) => s.0.clone(),
        _ => "<expr>".to_string(),
    }
}
