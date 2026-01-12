//! Runtime plan execution demo:
//! - Registers real capabilities (demo.echo_ok, demo.add)
//! - Executes an RTFS plan by parsing calls and invoking the marketplace
//! - Logs CapabilityCall/CapabilityResult into the causal chain
//! - Introspects graph/trace/causal_chain/type_analysis afterwards

use ccos::capability_marketplace::CapabilityMarketplace;
use ccos::introspect::{
    build_observed_capability_graph, build_plan_trace, query_causal_chain, run_type_analysis,
    CapabilityGraphInput, CausalChainInput, PlanTraceInput, TypeAnalysisInput,
};
use ccos::plan_archive::PlanArchive;
use ccos::types::{Action, ActionType, ExecutionResult, Plan, PlanBody, PlanLanguage, PlanStatus};
use ccos::utils::value_conversion::rtfs_value_to_json;
use rtfs::ast::{Expression, Literal, TopLevel};
use rtfs::runtime::error::{RuntimeError, RuntimeResult};
use rtfs::runtime::values::Value;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use tokio::sync::RwLock;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let chain = Arc::new(Mutex::new(
        ccos::causal_chain::CausalChain::new().expect("chain"),
    ));
    let plan_id = "runtime-plan".to_string();
    let intent_id = "runtime-intent".to_string();

    // Register marketplace capabilities
    let marketplace = Arc::new(CapabilityMarketplace::new(Arc::new(RwLock::new(
        ccos::capabilities::registry::CapabilityRegistry::new(),
    ))));
    register_caps(&marketplace).await?;

    // Archive a plan that includes a missing capability
    let plan_archive = Arc::new(PlanArchive::new());
    let rtfs_src = r#"
    (do
      (call "demo.echo_ok" {:message "hi"})
      (call "demo.add" {:args [1 2 3]})
      (call "demo.missing" {:z 9})
      {:ok true})
    "#;
    archive_plan(&plan_archive, &plan_id, &intent_id, rtfs_src);

    // Execute the plan by parsing RTFS and invoking capabilities
    execute_plan_rtfs(&chain, &marketplace, &plan_id, &intent_id, rtfs_src).await?;

    // Introspect graph/trace/causal chain
    let graph = build_observed_capability_graph(
        &chain,
        CapabilityGraphInput {
            plan_id: Some(plan_id.clone()),
            capability_id: None,
            mode: Some("observed".to_string()),
            limit: None,
        },
    )
    .await?;
    println!(
        "capability_graph (observed): {}",
        rtfs_value_to_json(&graph)?
    );

    let trace = build_plan_trace(
        &chain,
        &PlanTraceInput {
            plan_id: plan_id.clone(),
            include_args: Some(false),
            include_result: Some(true),
            limit: None,
        },
    )
    .await?;
    println!("plan_trace: {}", rtfs_value_to_json(&trace)?);

    let analysis = run_type_analysis(
        &plan_archive,
        &marketplace,
        &TypeAnalysisInput {
            plan_id: Some(plan_id.clone()),
            plan_rtfs: None,
        },
    )
    .await?;
    let analysis_json = rtfs_value_to_json(&analysis)?;
    println!("type_analysis: {}", analysis_json);

    let chain_dump = query_causal_chain(
        &chain,
        &CausalChainInput {
            intent_id: Some(intent_id.clone()),
            plan_id: Some(plan_id.clone()),
            action_type: Some("CapabilityCall".to_string()),
            parent_action_id: None,
            limit: Some(50),
        },
    )
    .await?;
    println!("causal_chain: {}", rtfs_value_to_json(&chain_dump)?);

    Ok(())
}

async fn register_caps(marketplace: &CapabilityMarketplace) -> RuntimeResult<()> {
    marketplace
        .register_local_capability(
            "demo.echo_ok".to_string(),
            "Demo Echo".to_string(),
            "Echo the input map".to_string(),
            Arc::new(|input: &Value| Ok(input.clone())),
        )
        .await?;

    marketplace
        .register_local_capability(
            "demo.add".to_string(),
            "Demo Add".to_string(),
            "Sum integers in :args".to_string(),
            Arc::new(|input: &Value| {
                let mut sum: i64 = 0;
                if let Value::Map(map) = input {
                    for (_k, v) in map {
                        if let Value::Vector(vec) | Value::List(vec) = v {
                            for item in vec {
                                if let Value::Integer(i) = item {
                                    sum += i;
                                }
                            }
                        }
                    }
                }
                Ok(Value::Integer(sum))
            }),
        )
        .await?;

    Ok(())
}

fn archive_plan(archive: &PlanArchive, plan_id: &str, intent_id: &str, rtfs_src: &str) {
    let plan = Plan {
        plan_id: plan_id.to_string(),
        name: Some("runtime demo plan".to_string()),
        intent_ids: vec![intent_id.to_string()],
        language: PlanLanguage::Rtfs20,
        body: PlanBody::Rtfs(rtfs_src.to_string()),
        status: PlanStatus::Draft,
        created_at: std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs(),
        metadata: HashMap::new(),
        input_schema: None,
        output_schema: None,
        policies: HashMap::new(),
        capabilities_required: Vec::new(),
        annotations: HashMap::new(),
    };
    let _ = archive.archive_plan(&plan);
}

async fn execute_plan_rtfs(
    chain: &Arc<Mutex<ccos::causal_chain::CausalChain>>,
    marketplace: &Arc<CapabilityMarketplace>,
    plan_id: &str,
    intent_id: &str,
    rtfs_src: &str,
) -> RuntimeResult<()> {
    let parsed = rtfs::parser::parse(rtfs_src)
        .map_err(|e| RuntimeError::Generic(format!("parse error: {:?}", e)))?;

    let mut calls: Vec<String> = Vec::new();
    for top in parsed {
        if let TopLevel::Expression(expr) = top {
            collect_calls(&expr, &mut calls);
        }
    }

    let mut last_call: Option<String> = None;
    for cap_id in calls {
        let parent = last_call.clone();
        let call_id = execute_and_log(
            chain,
            marketplace,
            plan_id,
            intent_id,
            &cap_id,
            Value::Map(HashMap::new()),
            parent,
        )
        .await?;
        last_call = Some(call_id);
    }

    Ok(())
}

fn collect_calls(expr: &Expression, out: &mut Vec<String>) {
    match expr {
        Expression::Do(do_expr) => {
            for e in &do_expr.expressions {
                collect_calls(e, out);
            }
        }
        Expression::FunctionCall { callee, arguments } if matches!(**callee, Expression::Symbol(ref s) if s.0 == "call") => {
            if let Some(cap_id) = parse_cap_id(arguments) {
                out.push(cap_id);
            }
        }
        _ => {}
    }
}

fn parse_cap_id(args: &[Expression]) -> Option<String> {
    if args.is_empty() {
        return None;
    }
    match &args[0] {
        Expression::Literal(Literal::String(s)) => Some(s.clone()),
        Expression::Literal(Literal::Keyword(k)) => Some(k.0.clone()),
        Expression::Symbol(sym) => Some(sym.0.clone()),
        _ => None,
    }
}

async fn execute_and_log(
    chain: &Arc<Mutex<ccos::causal_chain::CausalChain>>,
    marketplace: &Arc<CapabilityMarketplace>,
    plan_id: &str,
    intent_id: &str,
    capability_id: &str,
    args: Value,
    parent: Option<String>,
) -> RuntimeResult<String> {
    let action = Action::new(
        ActionType::CapabilityCall,
        plan_id.to_string(),
        intent_id.to_string(),
    )
    .with_parent(parent.clone())
    .with_name(capability_id);
    let call_id = action.action_id.clone();

    chain
        .lock()
        .unwrap()
        .append(&action)
        .map_err(|e| RuntimeError::Generic(format!("append error: {}", e)))?;

    let exec = marketplace
        .execute_capability_enhanced(capability_id, &args, None)
        .await;

    match exec {
        Ok(value) => {
            let result = ExecutionResult {
                success: true,
                value,
                metadata: HashMap::new(),
            };
            chain
                .lock()
                .unwrap()
                .record_result(action, result)
                .map_err(|e| RuntimeError::Generic(format!("record_result error: {}", e)))?;
            Ok(call_id)
        }
        Err(e) => {
            let mut meta = HashMap::new();
            meta.insert("error".to_string(), Value::String(format!("{}", e)));
            meta.insert(
                "error_category".to_string(),
                Value::String("ExecutionError".to_string()),
            );
            let err_result = ExecutionResult {
                success: false,
                value: Value::Nil,
                metadata: meta,
            };
            chain
                .lock()
                .unwrap()
                .record_result(action, err_result)
                .map_err(|e| RuntimeError::Generic(format!("record_result error: {}", e)))?;
            Ok(call_id)
        }
    }
}
