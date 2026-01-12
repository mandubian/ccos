use crate::capability_marketplace::CapabilityMarketplace;
use crate::plan_archive::PlanArchive;
use crate::types::{ActionType, ExecutionResult};
use crate::utils::value_conversion::{json_to_rtfs_value, rtfs_value_to_json};
use futures::future::BoxFuture;
use futures::FutureExt;
use rtfs::runtime::error::{RuntimeError, RuntimeResult};
use rtfs::runtime::values::Value;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};

/// Register introspection capabilities (Phase 4).
pub async fn register_introspect_capabilities(
    marketplace: Arc<CapabilityMarketplace>,
    causal_chain: Arc<Mutex<crate::causal_chain::CausalChain>>,
    plan_archive: Arc<PlanArchive>,
) -> Result<(), RuntimeError> {
    let chain_for_graph = Arc::clone(&causal_chain);
    let archive_for_graph = Arc::clone(&plan_archive);
    marketplace
        .register_native_capability(
            "introspect.capability_graph".to_string(),
            "Introspect Capability Graph".to_string(),
            "Observed capability call graph from the causal chain (or static from RTFS)"
                .to_string(),
            Arc::new(
                move |args: &Value| -> BoxFuture<'static, RuntimeResult<Value>> {
                    let chain = Arc::clone(&chain_for_graph);
                    let archive = Arc::clone(&archive_for_graph);
                    let parsed: RuntimeResult<CapabilityGraphInput> = parse_input(args);
                    async move {
                        let chain = Arc::clone(&chain);
                        let archive = Arc::clone(&archive);
                        let input = parsed?;
                        let mode = input.mode.as_deref().unwrap_or("observed");

                        match mode {
                            "observed" => build_observed_capability_graph(&chain, input).await,
                            "static_plan" => build_static_plan_graph(&archive, input).await,
                            _ => Err(RuntimeError::Generic(format!(
                                "Unsupported mode '{}'. Use 'observed' or 'static_plan'.",
                                mode
                            ))),
                        }
                    }
                    .boxed()
                },
            ),
            "low".to_string(),
        )
        .await?;

    let chain_for_trace = Arc::clone(&causal_chain);
    marketplace
        .register_native_capability(
            "introspect.plan_trace".to_string(),
            "Introspect Plan Trace".to_string(),
            "Step-by-step execution trace for a plan from the causal chain".to_string(),
            Arc::new(
                move |args: &Value| -> BoxFuture<'static, RuntimeResult<Value>> {
                    let chain = Arc::clone(&chain_for_trace);
                    let parsed: RuntimeResult<PlanTraceInput> = parse_input(args);
                    async move {
                        let chain = Arc::clone(&chain);
                        let input = parsed?;
                        build_plan_trace(&chain, &input).await
                    }
                    .boxed()
                },
            ),
            "low".to_string(),
        )
        .await?;

    let marketplace_for_type = Arc::clone(&marketplace);
    let archive_for_type = Arc::clone(&plan_archive);
    marketplace
        .register_native_capability(
            "introspect.type_analysis".to_string(),
            "Introspect Type Analysis".to_string(),
            "Best-effort type analysis for a plan (schema checks; static-only)".to_string(),
            Arc::new(
                move |args: &Value| -> BoxFuture<'static, RuntimeResult<Value>> {
                    let archive = Arc::clone(&archive_for_type);
                    let marketplace = Arc::clone(&marketplace_for_type);
                    let parsed: RuntimeResult<TypeAnalysisInput> = parse_input(args);
                    async move {
                        let archive = Arc::clone(&archive);
                        let marketplace = Arc::clone(&marketplace);
                        let input = parsed?;
                        run_type_analysis(&archive, &marketplace, &input).await
                    }
                    .boxed()
                },
            ),
            "low".to_string(),
        )
        .await?;

    let chain_for_query = Arc::clone(&causal_chain);
    marketplace
        .register_native_capability(
            "introspect.causal_chain".to_string(),
            "Introspect Causal Chain".to_string(),
            "Query causal chain actions with filters".to_string(),
            Arc::new(
                move |args: &Value| -> BoxFuture<'static, RuntimeResult<Value>> {
                    let chain = Arc::clone(&chain_for_query);
                    let parsed: RuntimeResult<CausalChainInput> = parse_input(args);
                    async move {
                        let chain = Arc::clone(&chain);
                        let input = parsed?;
                        query_causal_chain(&chain, &input).await
                    }
                    .boxed()
                },
            ),
            "low".to_string(),
        )
        .await?;

    Ok(())
}

// ----------------------------- Inputs/Outputs -----------------------------

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct CapabilityGraphInput {
    #[serde(default)]
    pub plan_id: Option<String>,
    #[serde(default)]
    pub capability_id: Option<String>,
    #[serde(default)]
    pub mode: Option<String>, // observed | static_plan
    #[serde(default)]
    pub limit: Option<usize>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CapabilityGraphNode {
    pub id: String,
    pub call_count: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CapabilityGraphEdge {
    pub from: String,
    pub to: String,
    pub count: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CapabilityGraphOutput {
    pub nodes: Vec<CapabilityGraphNode>,
    pub edges: Vec<CapabilityGraphEdge>,
    pub mode: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlanTraceInput {
    pub plan_id: String,
    #[serde(default)]
    pub include_args: Option<bool>,
    #[serde(default)]
    pub include_result: Option<bool>,
    #[serde(default)]
    pub limit: Option<usize>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TraceResult {
    pub success: bool,
    pub value: serde_json::Value,
    pub metadata: serde_json::Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TraceStep {
    pub action_id: String,
    pub parent_action_id: Option<String>,
    pub action_type: String,
    pub function_name: Option<String>,
    pub timestamp: u64,
    pub duration_ms: Option<u64>,
    pub arguments: Option<Vec<serde_json::Value>>,
    pub result: Option<TraceResult>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlanTraceOutput {
    pub plan_id: String,
    pub steps: Vec<TraceStep>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct TypeAnalysisInput {
    #[serde(default)]
    pub plan_id: Option<String>,
    #[serde(default)]
    pub plan_rtfs: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TypeIssue {
    pub kind: String,
    pub message: String,
    pub capability_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TypeAnalysisOutput {
    pub issues: Vec<TypeIssue>,
    pub discovered_capabilities: Vec<String>,
    #[serde(default)]
    pub suggested_output_schema: Option<rtfs::ast::TypeExpr>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct CausalChainInput {
    #[serde(default)]
    pub intent_id: Option<String>,
    #[serde(default)]
    pub plan_id: Option<String>,
    #[serde(default)]
    pub action_type: Option<String>,
    #[serde(default)]
    pub parent_action_id: Option<String>,
    #[serde(default)]
    pub limit: Option<usize>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct CausalChainAction {
    action_id: String,
    parent_action_id: Option<String>,
    plan_id: String,
    intent_id: String,
    action_type: String,
    function_name: Option<String>,
    timestamp: u64,
    metadata: serde_json::Value,
    result: Option<TraceResult>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CausalChainOutput {
    actions: Vec<CausalChainAction>,
}

// ----------------------------- Capability Graph -----------------------------

pub async fn build_observed_capability_graph(
    chain: &Arc<Mutex<crate::causal_chain::CausalChain>>,
    input: CapabilityGraphInput,
) -> RuntimeResult<Value> {
    let limit = input.limit.unwrap_or(500);
    let query = crate::causal_chain::CausalQuery {
        plan_id: input.plan_id.clone(),
        action_type: Some(ActionType::CapabilityCall),
        ..Default::default()
    };

    let snapshots = {
        let guard = chain
            .lock()
            .map_err(|_| RuntimeError::Generic("Failed to lock causal chain".to_string()))?;
        let actions = guard.query_actions(&query);

        actions
            .iter()
            .filter_map(|a| {
                let fn_name = a.function_name.clone().unwrap_or_default();
                if let Some(filter) = input.capability_id.as_ref() {
                    if !fn_name.contains(filter) {
                        return None;
                    }
                }
                Some(ObservedCall {
                    action_id: a.action_id.clone(),
                    parent_action_id: a.parent_action_id.clone(),
                    function_name: fn_name,
                })
            })
            .collect::<Vec<_>>()
    };

    let mut node_counts: HashMap<String, usize> = HashMap::new();
    let mut edge_counts: HashMap<(String, String), usize> = HashMap::new();

    // Build nodes
    for call in &snapshots {
        *node_counts.entry(call.function_name.clone()).or_insert(0) += 1;
    }

    // Build edges via parent->child when parent is also a capability call
    for call in &snapshots {
        if let Some(parent_id) = call.parent_action_id.as_ref() {
            if let Some(parent_fn) = snapshots
                .iter()
                .find(|p| &p.action_id == parent_id)
                .map(|p| p.function_name.clone())
            {
                let key = (parent_fn, call.function_name.clone());
                *edge_counts.entry(key).or_insert(0) += 1;
            }
        }
    }

    let mut nodes: Vec<CapabilityGraphNode> = node_counts
        .into_iter()
        .map(|(id, call_count)| CapabilityGraphNode { id, call_count })
        .collect();
    nodes.sort_by(|a, b| b.call_count.cmp(&a.call_count));
    nodes.truncate(limit);

    let mut edges: Vec<CapabilityGraphEdge> = edge_counts
        .into_iter()
        .map(|((from, to), count)| CapabilityGraphEdge { from, to, count })
        .collect();
    edges.sort_by(|a, b| b.count.cmp(&a.count));
    edges.truncate(limit);

    to_value(&CapabilityGraphOutput {
        nodes,
        edges,
        mode: "observed".to_string(),
    })
}

#[derive(Debug, Clone)]
struct ObservedCall {
    action_id: String,
    parent_action_id: Option<String>,
    function_name: String,
}

pub async fn build_static_plan_graph(
    plan_archive: &Arc<PlanArchive>,
    input: CapabilityGraphInput,
) -> RuntimeResult<Value> {
    let plan_id = input
        .plan_id
        .as_ref()
        .ok_or_else(|| RuntimeError::Generic("static_plan mode requires plan_id".to_string()))?;

    let Some(plan) = plan_archive.get_plan_by_id(plan_id) else {
        return Err(RuntimeError::Generic(format!(
            "Plan '{}' not found in archive",
            plan_id
        )));
    };

    let rtfs = match plan.body {
        crate::archivable_types::ArchivablePlanBody::String(ref body) => body.clone(),
        crate::archivable_types::ArchivablePlanBody::Legacy { .. } => {
            return Err(RuntimeError::Generic(
                "Legacy plan format not supported for static_plan graph".to_string(),
            ))
        }
    };

    let calls = collect_calls_from_rtfs(&rtfs)?;

    let mut node_counts: HashMap<String, usize> = HashMap::new();
    for cap in &calls {
        *node_counts.entry(cap.clone()).or_insert(0) += 1;
    }

    let nodes: Vec<CapabilityGraphNode> = node_counts
        .into_iter()
        .map(|(id, call_count)| CapabilityGraphNode { id, call_count })
        .collect();

    to_value(&CapabilityGraphOutput {
        nodes,
        edges: Vec::new(),
        mode: "static_plan".to_string(),
    })
}

// ----------------------------- Plan Trace -----------------------------

pub async fn build_plan_trace(
    chain: &Arc<Mutex<crate::causal_chain::CausalChain>>,
    input: &PlanTraceInput,
) -> RuntimeResult<Value> {
    let include_args = input.include_args.unwrap_or(false);
    let include_result = input.include_result.unwrap_or(false);
    let limit = input.limit.unwrap_or(1000);

    let steps = {
        let guard = chain
            .lock()
            .map_err(|_| RuntimeError::Generic("Failed to lock causal chain".to_string()))?;
        let actions = guard.get_actions_for_plan(&input.plan_id);

        let mut cloned: Vec<crate::types::Action> = actions.iter().map(|a| (*a).clone()).collect();
        cloned.sort_by(|a, b| a.timestamp.cmp(&b.timestamp));
        cloned.truncate(limit);
        cloned
    };

    let mut trace_steps = Vec::new();
    for action in steps {
        let arguments = if include_args {
            action.arguments.as_ref().map(|args| {
                args.iter()
                    .filter_map(|v| rtfs_value_to_json(v).ok())
                    .collect::<Vec<_>>()
            })
        } else {
            None
        };

        let result = if include_result {
            action
                .result
                .as_ref()
                .map(|r| exec_result_to_trace_result(r))
                .transpose()?
        } else {
            None
        };

        trace_steps.push(TraceStep {
            action_id: action.action_id.clone(),
            parent_action_id: action.parent_action_id.clone(),
            action_type: format!("{:?}", action.action_type),
            function_name: action.function_name.clone(),
            timestamp: action.timestamp,
            duration_ms: action.duration_ms,
            arguments,
            result,
        });
    }

    to_value(&PlanTraceOutput {
        plan_id: input.plan_id.clone(),
        steps: trace_steps,
    })
}

// ----------------------------- Type Analysis -----------------------------

pub async fn run_type_analysis(
    plan_archive: &Arc<PlanArchive>,
    marketplace: &Arc<CapabilityMarketplace>,
    input: &TypeAnalysisInput,
) -> RuntimeResult<Value> {
    let rtfs = if let Some(plan_id) = input.plan_id.as_ref() {
        let Some(plan) = plan_archive.get_plan_by_id(plan_id) else {
            return Err(RuntimeError::Generic(format!(
                "Plan '{}' not found in archive",
                plan_id
            )));
        };
        match plan.body {
            crate::archivable_types::ArchivablePlanBody::String(ref body) => body.clone(),
            crate::archivable_types::ArchivablePlanBody::Legacy { .. } => {
                return Err(RuntimeError::Generic(
                    "Legacy plan format not supported for type_analysis".to_string(),
                ))
            }
        }
    } else if let Some(rtfs) = input.plan_rtfs.as_ref() {
        rtfs.clone()
    } else {
        return Err(RuntimeError::Generic(
            "type_analysis requires plan_id or plan_rtfs".to_string(),
        ));
    };

    let calls = collect_calls_from_rtfs(&rtfs)?;
    // Best-effort output schema inference from RTFS body
    let suggested_output_schema =
        crate::introspect::schema_inference::infer_output_schema(&rtfs).unwrap_or(None);
    let mut issues = Vec::new();
    let mut discovered = Vec::new();

    for cap_id in &calls {
        discovered.push(cap_id.clone());
        let cap_opt = marketplace.get_capability(cap_id).await;
        if cap_opt.is_none() {
            issues.push(TypeIssue {
                kind: "MissingCapability".to_string(),
                message: format!("Capability '{}' not registered", cap_id),
                capability_id: Some(cap_id.clone()),
            });
            continue;
        }

        if let Some(cap) = cap_opt {
            if schema_is_any(cap.output_schema.as_ref()) {
                issues.push(TypeIssue {
                    kind: "OutputSchemaTooGeneric".to_string(),
                    message: format!("Capability '{}' output_schema is :any or missing", cap_id),
                    capability_id: Some(cap_id.clone()),
                });
            }
            if schema_is_any(cap.input_schema.as_ref()) {
                issues.push(TypeIssue {
                    kind: "InputSchemaTooGeneric".to_string(),
                    message: format!("Capability '{}' input_schema is :any or missing", cap_id),
                    capability_id: Some(cap_id.clone()),
                });
            }
        }
    }

    to_value(&TypeAnalysisOutput {
        issues,
        discovered_capabilities: discovered,
        suggested_output_schema,
    })
}

fn schema_is_any(schema: Option<&rtfs::ast::TypeExpr>) -> bool {
    match schema {
        None => true,
        Some(rtfs::ast::TypeExpr::Any) => true,
        _ => false,
    }
}

// ----------------------------- Causal Chain Query -----------------------------

pub async fn query_causal_chain(
    chain: &Arc<Mutex<crate::causal_chain::CausalChain>>,
    input: &CausalChainInput,
) -> RuntimeResult<Value> {
    let limit = input.limit.unwrap_or(200);
    let action_type = if let Some(at) = input.action_type.as_ref() {
        match parse_action_type(at) {
            Some(t) => Some(t),
            None => {
                return Err(RuntimeError::Generic(format!(
                    "Unknown action_type '{}'",
                    at
                )))
            }
        }
    } else {
        None
    };

    let query = crate::causal_chain::CausalQuery {
        intent_id: input.intent_id.clone(),
        plan_id: input.plan_id.clone(),
        action_type,
        parent_action_id: input.parent_action_id.clone(),
        ..Default::default()
    };

    let actions = {
        let guard = chain
            .lock()
            .map_err(|_| RuntimeError::Generic("Failed to lock causal chain".to_string()))?;
        let mut actions = guard.query_actions(&query);
        actions.sort_by(|a, b| a.timestamp.cmp(&b.timestamp));
        actions.into_iter().take(limit).cloned().collect::<Vec<_>>()
    };

    let mut out = Vec::new();
    for action in actions {
        let metadata = serde_json::to_value(&action.metadata)
            .map_err(|e| RuntimeError::Generic(format!("Failed to serialize metadata: {}", e)))?;
        let result = action
            .result
            .as_ref()
            .map(|r| exec_result_to_trace_result(r))
            .transpose()?;

        out.push(CausalChainAction {
            action_id: action.action_id.clone(),
            parent_action_id: action.parent_action_id.clone(),
            plan_id: action.plan_id.clone(),
            intent_id: action.intent_id.clone(),
            action_type: format!("{:?}", action.action_type),
            function_name: action.function_name.clone(),
            timestamp: action.timestamp,
            metadata,
            result,
        });
    }

    to_value(&CausalChainOutput { actions: out })
}

pub mod schema_inference;

fn parse_action_type(s: &str) -> Option<ActionType> {
    match s {
        "PlanStarted" => Some(ActionType::PlanStarted),
        "PlanCompleted" => Some(ActionType::PlanCompleted),
        "PlanAborted" => Some(ActionType::PlanAborted),
        "PlanPaused" => Some(ActionType::PlanPaused),
        "PlanResumed" => Some(ActionType::PlanResumed),
        "PlanStepStarted" => Some(ActionType::PlanStepStarted),
        "PlanStepCompleted" => Some(ActionType::PlanStepCompleted),
        "PlanStepFailed" => Some(ActionType::PlanStepFailed),
        "PlanStepRetrying" => Some(ActionType::PlanStepRetrying),
        "CapabilityCall" => Some(ActionType::CapabilityCall),
        "CapabilityResult" => Some(ActionType::CapabilityResult),
        "CatalogReuse" => Some(ActionType::CatalogReuse),
        "InternalStep" => Some(ActionType::InternalStep),
        "StepProfileDerived" => Some(ActionType::StepProfileDerived),
        "IntentCreated" => Some(ActionType::IntentCreated),
        "IntentStatusChanged" => Some(ActionType::IntentStatusChanged),
        "IntentRelationshipCreated" => Some(ActionType::IntentRelationshipCreated),
        "IntentRelationshipModified" => Some(ActionType::IntentRelationshipModified),
        "IntentArchived" => Some(ActionType::IntentArchived),
        "IntentReactivated" => Some(ActionType::IntentReactivated),
        "CapabilityRegistered" => Some(ActionType::CapabilityRegistered),
        "CapabilityRemoved" => Some(ActionType::CapabilityRemoved),
        "CapabilityUpdated" => Some(ActionType::CapabilityUpdated),
        "CapabilityDiscoveryCompleted" => Some(ActionType::CapabilityDiscoveryCompleted),
        "CapabilityVersionCreated" => Some(ActionType::CapabilityVersionCreated),
        "CapabilityRollback" => Some(ActionType::CapabilityRollback),
        "CapabilitySynthesisStarted" => Some(ActionType::CapabilitySynthesisStarted),
        "CapabilitySynthesisCompleted" => Some(ActionType::CapabilitySynthesisCompleted),
        "GovernanceApprovalRequested" => Some(ActionType::GovernanceApprovalRequested),
        "GovernanceApprovalGranted" => Some(ActionType::GovernanceApprovalGranted),
        "GovernanceApprovalDenied" => Some(ActionType::GovernanceApprovalDenied),
        "BoundedExplorationLimitReached" => Some(ActionType::BoundedExplorationLimitReached),
        _ => None,
    }
}

// ----------------------------- Helpers -----------------------------

fn parse_input<T: for<'de> Deserialize<'de>>(args: &Value) -> RuntimeResult<T> {
    let json = rtfs_value_to_json(args)?;
    serde_json::from_value(json)
        .map_err(|e| RuntimeError::Generic(format!("Failed to parse input: {}", e)))
}

fn to_value<T: Serialize>(output: &T) -> RuntimeResult<Value> {
    let json = serde_json::to_value(output)
        .map_err(|e| RuntimeError::Generic(format!("Failed to serialize output: {}", e)))?;
    json_to_rtfs_value(&json)
}

fn exec_result_to_trace_result(result: &ExecutionResult) -> RuntimeResult<TraceResult> {
    let value_json = rtfs_value_to_json(&result.value)?;
    let meta_json = serde_json::to_value(&result.metadata)
        .map_err(|e| RuntimeError::Generic(format!("Failed to serialize metadata: {}", e)))?;
    Ok(TraceResult {
        success: result.success,
        value: value_json,
        metadata: meta_json,
    })
}

fn collect_calls_from_rtfs(rtfs_src: &str) -> RuntimeResult<Vec<String>> {
    let tops = rtfs::parser::parse(rtfs_src)
        .map_err(|e| RuntimeError::Generic(format!("Failed to parse RTFS: {:?}", e)))?;
    let mut calls = Vec::new();
    for top in tops {
        if let rtfs::ast::TopLevel::Expression(expr) = top {
            collect_calls_from_expr(&expr, &mut calls);
        }
    }
    Ok(calls)
}

fn collect_calls_from_expr(expr: &rtfs::ast::Expression, out: &mut Vec<String>) {
    use rtfs::ast::{Expression, Literal};
    match expr {
        Expression::FunctionCall { callee, arguments } => {
            if matches!(**callee, Expression::Symbol(ref s) if s.0 == "call") {
                if let Some(first_arg) = arguments.get(0) {
                    match first_arg {
                        Expression::Literal(Literal::String(s)) => out.push(s.clone()),
                        Expression::Literal(Literal::Keyword(k)) => out.push(k.0.clone()),
                        Expression::Symbol(sym) => out.push(sym.0.clone()),
                        _ => {}
                    }
                }
            }
            for arg in arguments {
                collect_calls_from_expr(arg, out);
            }
        }
        Expression::Do(do_block) => {
            for e in &do_block.expressions {
                collect_calls_from_expr(e, out);
            }
        }
        Expression::If(if_expr) => {
            collect_calls_from_expr(&if_expr.condition, out);
            collect_calls_from_expr(&if_expr.then_branch, out);
            if let Some(else_branch) = &if_expr.else_branch {
                collect_calls_from_expr(else_branch, out);
            }
        }
        Expression::Let(let_expr) => {
            for binding in &let_expr.bindings {
                collect_calls_from_expr(&binding.value, out);
            }
            for e in &let_expr.body {
                collect_calls_from_expr(e, out);
            }
        }
        Expression::Fn(lambda) => {
            for e in &lambda.body {
                collect_calls_from_expr(e, out);
            }
        }
        Expression::Match(match_expr) => {
            collect_calls_from_expr(&match_expr.expression, out);
            for clause in &match_expr.clauses {
                collect_calls_from_expr(&clause.body, out);
            }
        }
        _ => {}
    }
}

// ----------------------------- Tests -----------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::capabilities::registry::CapabilityRegistry;
    use crate::types::{Action, ActionType, Plan, PlanBody, PlanLanguage, PlanStatus};
    use futures::FutureExt;
    use rtfs::runtime::error::RuntimeResult;
    use std::time::{SystemTime, UNIX_EPOCH};
    use tokio::sync::RwLock;

    fn make_chain_with_calls() -> Arc<Mutex<crate::causal_chain::CausalChain>> {
        let mut chain = crate::causal_chain::CausalChain::new().expect("chain");
        let plan_id = "plan1".to_string();
        let intent_id = "intent1".to_string();

        let parent = Action::new(
            ActionType::CapabilityCall,
            plan_id.clone(),
            intent_id.clone(),
        )
        .with_name("cap.parent");
        chain.append(&parent).expect("append parent");

        let child = Action::new(ActionType::CapabilityCall, plan_id, intent_id)
            .with_parent(Some(parent.action_id.clone()))
            .with_name("cap.child");
        chain.append(&child).expect("append child");

        Arc::new(Mutex::new(chain))
    }

    fn make_plan_archive_with_rtfs(rtfs: &str, plan_id: &str) -> PlanArchive {
        let archive = PlanArchive::new();
        let plan = Plan {
            plan_id: plan_id.to_string(),
            name: Some("test plan".to_string()),
            intent_ids: vec!["intent1".to_string()],
            language: PlanLanguage::Rtfs20,
            body: PlanBody::Rtfs(rtfs.to_string()),
            status: PlanStatus::Draft,
            created_at: SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_secs(),
            metadata: HashMap::new(),
            input_schema: None,
            output_schema: None,
            policies: HashMap::new(),
            capabilities_required: Vec::new(),
            annotations: HashMap::new(),
        };
        archive.archive_plan(&plan).expect("archive plan");
        archive
    }

    #[tokio::test]
    async fn capability_graph_observed_smoke() {
        let chain = make_chain_with_calls();
        let input = CapabilityGraphInput {
            plan_id: Some("plan1".to_string()),
            capability_id: None,
            mode: Some("observed".to_string()),
            limit: None,
        };

        let value = build_observed_capability_graph(&chain, input)
            .await
            .expect("graph");
        let json = rtfs_value_to_json(&value).expect("json");
        let nodes = json
            .get("nodes")
            .and_then(|v| v.as_array())
            .cloned()
            .unwrap_or_default();
        let edges = json
            .get("edges")
            .and_then(|v| v.as_array())
            .cloned()
            .unwrap_or_default();
        assert_eq!(nodes.len(), 2);
        assert_eq!(edges.len(), 1);
    }

    #[tokio::test]
    async fn plan_trace_smoke() {
        let chain = make_chain_with_calls();
        let input = PlanTraceInput {
            plan_id: "plan1".to_string(),
            include_args: Some(false),
            include_result: Some(false),
            limit: None,
        };
        let value = build_plan_trace(&chain, &input).await.expect("trace");
        let json = rtfs_value_to_json(&value).expect("json");
        let steps = json
            .get("steps")
            .and_then(|v| v.as_array())
            .cloned()
            .unwrap_or_default();
        assert_eq!(steps.len(), 2);
    }

    #[tokio::test]
    async fn type_analysis_reports_missing_capability() {
        let rtfs_src = r#"
        (do
          (call "foo" {:x 1})
          (call :bar {:y 2}))
        "#;
        let archive = make_plan_archive_with_rtfs(rtfs_src, "plan-type");
        let registry = Arc::new(RwLock::new(CapabilityRegistry::new()));
        let marketplace = Arc::new(CapabilityMarketplace::new(registry));

        let input = TypeAnalysisInput {
            plan_id: Some("plan-type".to_string()),
            plan_rtfs: None,
        };
        let value = run_type_analysis(&Arc::new(archive), &marketplace, &input)
            .await
            .expect("analysis");
        let json = rtfs_value_to_json(&value).expect("json");
        let issues = json
            .get("issues")
            .and_then(|v| v.as_array())
            .cloned()
            .unwrap_or_default();
        assert!(!issues.is_empty());
    }

    #[tokio::test]
    async fn capability_graph_static_plan_counts() -> RuntimeResult<()> {
        let rtfs_src = r#"
        (do
          (call "alpha" {:x 1})
          (call "beta" {:y 2})
          (call "alpha" {:z 3}))
        "#;
        let archive = make_plan_archive_with_rtfs(rtfs_src, "plan-static");
        let input = CapabilityGraphInput {
            plan_id: Some("plan-static".to_string()),
            capability_id: None,
            mode: Some("static_plan".to_string()),
            limit: None,
        };

        let value = build_static_plan_graph(&Arc::new(archive), input).await?;
        let json = rtfs_value_to_json(&value)?;
        let nodes = json
            .get("nodes")
            .and_then(|v| v.as_array())
            .cloned()
            .unwrap_or_default();

        assert_eq!(nodes.len(), 2);
        let alpha_calls = nodes
            .iter()
            .find(|n| n.get("id").and_then(|v| v.as_str()) == Some("alpha"))
            .and_then(|n| n.get("call_count"))
            .and_then(|v| v.as_u64())
            .unwrap_or(0);
        assert_eq!(alpha_calls, 2);
        Ok(())
    }

    #[tokio::test]
    async fn plan_trace_includes_args_and_result() -> RuntimeResult<()> {
        let mut chain = crate::causal_chain::CausalChain::new()?;
        let plan_id = "plan-trace".to_string();
        let intent_id = "intent-trace".to_string();

        let action = Action::new(
            ActionType::CapabilityCall,
            plan_id.clone(),
            intent_id.clone(),
        )
        .with_name("cap.trace")
        .with_args(vec![Value::Nil])
        .with_result(ExecutionResult {
            success: true,
            value: Value::Nil,
            metadata: HashMap::new(),
        });
        chain.append(&action)?;
        let chain = Arc::new(Mutex::new(chain));

        let input = PlanTraceInput {
            plan_id,
            include_args: Some(true),
            include_result: Some(true),
            limit: Some(10),
        };

        let value = build_plan_trace(&chain, &input).await?;
        let json = rtfs_value_to_json(&value)?;
        let steps = json
            .get("steps")
            .and_then(|v| v.as_array())
            .cloned()
            .unwrap_or_default();
        assert_eq!(steps.len(), 1);
        let first = steps.first().cloned().unwrap_or_default();
        let args = first
            .get("arguments")
            .and_then(|v| v.as_array())
            .cloned()
            .unwrap_or_default();
        assert_eq!(args.len(), 1);
        let result_obj = first.get("result").cloned().unwrap_or_default();
        assert_eq!(
            result_obj.get("success"),
            Some(&serde_json::Value::Bool(true))
        );
        Ok(())
    }

    #[tokio::test]
    async fn type_analysis_reports_generic_schema() -> RuntimeResult<()> {
        let rtfs_src = r#"(call "cap.generic" {:x 1})"#;
        let archive = make_plan_archive_with_rtfs(rtfs_src, "plan-generic");
        let registry = Arc::new(RwLock::new(CapabilityRegistry::new()));
        let marketplace = Arc::new(CapabilityMarketplace::new(registry));

        marketplace
            .register_native_capability(
                "cap.generic".to_string(),
                "Generic".to_string(),
                "Test generic schema".to_string(),
                Arc::new(|_| async { Ok(Value::Nil) }.boxed()),
                "low".to_string(),
            )
            .await?;

        let input = TypeAnalysisInput {
            plan_id: Some("plan-generic".to_string()),
            plan_rtfs: None,
        };

        let value = run_type_analysis(&Arc::new(archive), &marketplace, &input).await?;
        let json = rtfs_value_to_json(&value)?;
        let issues = json
            .get("issues")
            .and_then(|v| v.as_array())
            .cloned()
            .unwrap_or_default();

        assert!(issues.len() >= 1);
        let kinds: Vec<String> = issues
            .iter()
            .filter_map(|i| i.get("kind").and_then(|v| v.as_str()).map(str::to_string))
            .collect();
        assert!(kinds.iter().any(|k| k.contains("InputSchemaTooGeneric")));
        assert!(kinds.iter().any(|k| k.contains("OutputSchemaTooGeneric")));
        Ok(())
    }

    #[tokio::test]
    async fn causal_chain_query_filters_by_type_and_limit() -> RuntimeResult<()> {
        let mut chain = crate::causal_chain::CausalChain::new()?;
        let plan_id = "plan-cc".to_string();
        let intent_id = "intent-cc".to_string();

        let start = Action::new(ActionType::PlanStarted, plan_id.clone(), intent_id.clone());
        chain.append(&start)?;
        let call = Action::new(
            ActionType::CapabilityCall,
            plan_id.clone(),
            intent_id.clone(),
        )
        .with_name("cap.query");
        chain.append(&call)?;
        let completed = Action::new(
            ActionType::PlanCompleted,
            plan_id.clone(),
            intent_id.clone(),
        );
        chain.append(&completed)?;

        let chain = Arc::new(Mutex::new(chain));
        let input = CausalChainInput {
            intent_id: None,
            plan_id: Some(plan_id),
            action_type: Some("CapabilityCall".to_string()),
            parent_action_id: None,
            limit: Some(1),
        };

        let value = query_causal_chain(&chain, &input).await?;
        let json = rtfs_value_to_json(&value)?;
        let actions = json
            .get("actions")
            .and_then(|v| v.as_array())
            .cloned()
            .unwrap_or_default();
        assert_eq!(actions.len(), 1);
        let action_type = actions
            .first()
            .and_then(|a| a.get("action_type"))
            .and_then(|v| v.as_str())
            .unwrap_or("");
        assert_eq!(action_type, "CapabilityCall");
        Ok(())
    }
}
