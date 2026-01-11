//! Minimal demo runner for the introspection helpers.
//! It builds an in-memory causal chain and plan archive, then prints:
//! - Observed capability graph (parent → child)
//! - Plan trace for the sample plan
//! - Type analysis for the sample plan (shows missing capabilities)

use ccos::capability_marketplace::CapabilityMarketplace;
use ccos::introspect::{
    build_observed_capability_graph, build_plan_trace, run_type_analysis, CapabilityGraphInput,
    CausalChainInput, PlanTraceInput, TypeAnalysisInput,
};
use ccos::plan_archive::PlanArchive;
use ccos::types::{Action, ActionType, ExecutionResult, Plan, PlanBody, PlanLanguage, PlanStatus};
use ccos::utils::value_conversion::rtfs_value_to_json;
use rtfs::ast::{Keyword, MapKey, TypeExpr};
use rtfs::runtime::error::{RuntimeError, RuntimeResult};
use rtfs::runtime::values::Value;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use tokio::sync::RwLock;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // In-memory causal chain with:
    // - A successful plan (demo.echo_ok → demo.add) with real executions
    // - A failing plan (demo.missing) that is not registered
    let chain = Arc::new(Mutex::new(
        ccos::causal_chain::CausalChain::new().expect("chain"),
    ));
    let plan_id = "demo-plan".to_string();
    let intent_id = "demo-intent".to_string();
    let missing_plan_id = "missing-plan".to_string();
    let missing_intent_id = "missing-intent".to_string();

    // Plan archive with RTFS containing registered + missing capability
    let plan_archive = Arc::new(PlanArchive::new());
    let rtfs_src = r#"
    (do
      (call "demo.echo_ok" {:message "hello"})
      (call "demo.add" {:args [1 2]})
      (call "demo.missing" {:z 3})
      {:ok true})
    "#;
    let plan = Plan {
        plan_id: plan_id.clone(),
        name: Some("demo plan".to_string()),
        intent_ids: vec![intent_id.clone()],
        language: PlanLanguage::Rtfs20,
        body: PlanBody::Rtfs(rtfs_src.to_string()),
        status: PlanStatus::Draft,
        created_at: std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)?
            .as_secs(),
        metadata: HashMap::new(),
        input_schema: None,
        output_schema: None,
        policies: HashMap::new(),
        capabilities_required: Vec::new(),
        annotations: HashMap::new(),
    };
    plan_archive.archive_plan(&plan).expect("archive plan");

    // Marketplace with real capabilities registered
    let marketplace = Arc::new(CapabilityMarketplace::new(Arc::new(RwLock::new(
        ccos::capabilities::registry::CapabilityRegistry::new(),
    ))));
    register_real_capabilities(&marketplace).await?;

    // Execute registered capabilities and log to causal chain
    let echo_args = {
        let mut m = HashMap::new();
        m.insert(
            MapKey::Keyword(Keyword("message".to_string())),
            Value::String("hello".to_string()),
        );
        Value::Map(m)
    };
    let echo_call_id = execute_and_log(
        &chain,
        &marketplace,
        &plan_id,
        &intent_id,
        "demo.echo_ok",
        echo_args,
        None,
    )
    .await?;
    let add_args = {
        let mut m = HashMap::new();
        m.insert(
            MapKey::Keyword(Keyword("args".to_string())),
            Value::Vector(vec![Value::Integer(1), Value::Integer(2), Value::Integer(3)]),
        );
        Value::Map(m)
    };
    let _add_call_id = execute_and_log(
        &chain,
        &marketplace,
        &plan_id,
        &intent_id,
        "demo.add",
        add_args,
        Some(echo_call_id.clone()),
    )
    .await?;

    // Execute missing capability (will be recorded as failure)
    let _missing_call_id = execute_missing_and_log(
        &chain,
        &missing_plan_id,
        &missing_intent_id,
        "demo.missing",
        "MissingCapability",
        "unknown capability",
    )?;

    // 1) Observed capability graph (includes both success and missing plans)
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
    println!("capability_graph (observed): {}", rtfs_value_to_json(&graph)?);

    // 2) Plan trace (success)
    let trace = build_plan_trace(
        &chain,
        &PlanTraceInput {
            plan_id: plan_id.clone(),
            include_args: Some(false),
            include_result: Some(false),
            limit: None,
        },
    )
    .await?;
    println!("plan_trace: {}", rtfs_value_to_json(&trace)?);

    // 3) Plan trace (missing capability path)
    let trace_missing = build_plan_trace(
        &chain,
        &PlanTraceInput {
            plan_id: missing_plan_id.clone(),
            include_args: Some(false),
            include_result: Some(true),
            limit: None,
        },
    )
    .await?;
    println!(
        "plan_trace_missing: {}",
        rtfs_value_to_json(&trace_missing)?
    );

    // 3) Type analysis
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

    // 4) Causal chain query for completeness (CapabilityCall only)
    let chain_dump = ccos::introspect::query_causal_chain(
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

    // 5) Causal chain for the missing plan
    let chain_dump_missing = ccos::introspect::query_causal_chain(
        &chain,
        &CausalChainInput {
            intent_id: Some(missing_intent_id.clone()),
            plan_id: Some(missing_plan_id.clone()),
            action_type: Some("CapabilityCall".to_string()),
            parent_action_id: None,
            limit: Some(50),
        },
    )
    .await?;
    println!(
        "causal_chain_missing: {}",
        rtfs_value_to_json(&chain_dump_missing)?
    );

    Ok(())
}

async fn register_real_capabilities(
    marketplace: &CapabilityMarketplace,
) -> RuntimeResult<()> {
    let map_any = TypeExpr::Map {
        entries: vec![],
        wildcard: Some(Box::new(TypeExpr::Any)),
    };

    // Echo capability
    marketplace
        .register_local_capability_with_schema(
            "demo.echo_ok".to_string(),
            "Demo Echo".to_string(),
            "Echo the input map".to_string(),
            Arc::new(|input: &Value| Ok(input.clone())),
            Some(map_any.clone()),
            Some(map_any.clone()),
        )
        .await?;

    // Add capability: sum integers in :args vector/list
    marketplace
        .register_local_capability_with_schema(
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
            Some(map_any),
            Some(TypeExpr::Primitive(rtfs::ast::PrimitiveType::Int)),
        )
        .await?;

    Ok(())
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
    // Create and append call action
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

    // Execute capability
    let value = marketplace
        .execute_capability_enhanced(capability_id, &args, None)
        .await?;

    // Record result
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

fn execute_missing_and_log(
    chain: &Arc<Mutex<ccos::causal_chain::CausalChain>>,
    plan_id: &str,
    intent_id: &str,
    capability_id: &str,
    category: &str,
    message: &str,
) -> RuntimeResult<()> {
    let action = Action::new(
        ActionType::CapabilityCall,
        plan_id.to_string(),
        intent_id.to_string(),
    )
    .with_name(capability_id);
    chain
        .lock()
        .unwrap()
        .append(&action)
        .map_err(|e| RuntimeError::Generic(format!("append error: {}", e)))?;

    let mut meta = HashMap::new();
    meta.insert(
        "error".to_string(),
        Value::String(message.to_string()),
    );
    meta.insert(
        "error_category".to_string(),
        Value::String(category.to_string()),
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

    Ok(())
}

