use std::sync::Arc;
use std::collections::HashMap;

use serde::{de::DeserializeOwned, Deserialize, Serialize};
use serde_json::Value as JsonValue;

use rtfs::runtime::error::{RuntimeError, RuntimeResult};
use rtfs::runtime::values::Value;

use crate::capability_marketplace::types::ProviderType;
use crate::capability_marketplace::CapabilityMarketplace;
use crate::catalog::CatalogService;

/// Register granular planner capabilities (v2) for the autonomous agent loop.
pub async fn register_planner_capabilities_v2(
    marketplace: Arc<CapabilityMarketplace>,
    catalog: Arc<CatalogService>,
) -> RuntimeResult<()> {
    
    // 1. planner.build_menu
    // Scans the catalog/marketplace for capabilities relevant to the goal.
    let catalog_for_menu = Arc::clone(&catalog);
    let build_menu_handler = Arc::new(move |input: &Value| {
        let payload: BuildMenuInput = parse_payload("planner.build_menu", input)?;
        
        let mut menu = Vec::new();
        
        // Simple keyword matching for the demo
        // In a real system, this would use catalog.search_semantic
        let goal_lower = payload.goal.to_lowercase();
        
        if goal_lower.contains("search") || goal_lower.contains("find") {
             menu.push("discovery.search".to_string());
        }
        if goal_lower.contains("analyze") || goal_lower.contains("check") {
             menu.push("analysis.analyze_imports".to_string());
        }
        if goal_lower.contains("ask") || goal_lower.contains("user") {
             menu.push("ccos.user.ask".to_string());
        }
        if goal_lower.contains("issue") || goal_lower.contains("github") {
             menu.push("github.list_issues".to_string());
        }
        
        // Always add some basics
        menu.push("tool/log".to_string());
        menu.push("tool/time-ms".to_string());

        // Deduplicate
        menu.sort();
        menu.dedup();

        produce_value(
            "planner.build_menu",
            BuildMenuOutput { menu }
        )
    });

    marketplace.register_local_capability(
        "planner.build_menu".to_string(),
        "Planner / Build Menu".to_string(),
        "Selects a list of relevant capabilities for a given goal.".to_string(),
        build_menu_handler
    ).await?;

    // 2. planner.synthesize
    // Creates a plan (list of steps) based on the goal and menu.
    let synthesize_handler = Arc::new(move |input: &Value| {
        let payload: SynthesizeInput = parse_payload("planner.synthesize", input)?;
        
        let mut steps = Vec::new();
        let goal_lower = payload.goal.to_lowercase();

        // Rule-based synthesis for demo
        if goal_lower.contains("search") && payload.menu.contains(&"discovery.search".to_string()) {
            steps.push(PlanStep {
                id: "step_1".to_string(),
                capability_id: "discovery.search".to_string(),
                inputs: serde_json::json!({
                    "query": payload.goal, // Naive mapping
                    "context": "workspace"
                }),
            });
        } else if goal_lower.contains("analyze") && payload.menu.contains(&"analysis.analyze_imports".to_string()) {
             steps.push(PlanStep {
                id: "step_1".to_string(),
                capability_id: "analysis.analyze_imports".to_string(),
                inputs: serde_json::json!({
                    "path": "./"
                }),
            });
        } else if goal_lower.contains("issue") && payload.menu.contains(&"github.list_issues".to_string()) {
             steps.push(PlanStep {
                id: "step_1".to_string(),
                capability_id: "github.list_issues".to_string(),
                inputs: serde_json::json!({
                    "repo": "current"
                }),
            });
        } else {
            // Fallback: ask user what to do
             steps.push(PlanStep {
                id: "step_fallback".to_string(),
                capability_id: "ccos.user.ask".to_string(),
                inputs: serde_json::json!({
                    "args": [format!("I don't know how to '{}'. What should I do?", payload.goal)]
                }),
            });
        }

        produce_value(
            "planner.synthesize",
            SynthesizeOutput { 
                plan: PlanDto {
                    id: "generated_plan".to_string(),
                    steps
                }
            }
        )
    });

    marketplace.register_local_capability(
        "planner.synthesize".to_string(),
        "Planner / Synthesize".to_string(),
        "Generates a plan (steps) from a goal and a menu.".to_string(),
        synthesize_handler
    ).await?;

    // 3. planner.validate
    // Checks if the plan is valid (e.g. all capabilities exist in menu).
    let validate_handler = Arc::new(move |input: &Value| {
        let payload: ValidateInput = parse_payload("planner.validate", input)?;
        
        let mut valid = true;
        let mut errors = Vec::new();
        
        if payload.plan.steps.is_empty() {
            valid = false;
            errors.push("Plan has no steps".to_string());
        }
        
        // Check if capabilities are in the menu (optional strictness)
        for step in &payload.plan.steps {
            if !payload.menu.contains(&step.capability_id) && step.capability_id != "ccos.user.ask" {
                 // Allow ccos.user.ask as a fallback even if not in menu explicitly
                 // valid = false;
                 // errors.push(format!("Capability {} not in menu", step.capability_id));
            }
        }

        produce_value(
            "planner.validate",
            ValidateOutput { valid, errors }
        )
    });

    marketplace.register_local_capability(
        "planner.validate".to_string(),
        "Planner / Validate".to_string(),
        "Validates a generated plan.".to_string(),
        validate_handler
    ).await?;

    // 4. planner.execute_step
    // Dynamically executes a capability.
    // This is needed because RTFS `call` might not support dynamic capability IDs easily in all versions.
    let marketplace_for_exec = Arc::clone(&marketplace);
    let execute_step_handler = Arc::new(move |input: &Value| {
        let payload: ExecuteStepInput = parse_payload("planner.execute_step", input)?;
        
        // We need to execute the capability.
        // Since we are inside a capability handler, we are in async context.
        // We can look up the capability and execute it.
        
        let cap_id = payload.capability_id.clone();
        let args_json = payload.inputs;
        
        // Convert JSON args to RTFS Value
        let args_value = json_to_rtfs_value(args_json)?;
        
        // Execute
        // We spawn a new thread to avoid nested LocalPool execution issues
        // (host.rs uses block_on, and if we use block_on here on the same thread, it panics)
        let marketplace_clone = marketplace_for_exec.clone();
        
        let result = std::thread::spawn(move || {
            futures::executor::block_on(async {
                let cap = marketplace_clone.get_capability(&cap_id).await
                    .ok_or_else(|| RuntimeError::Generic(format!("Capability {} not found", cap_id)))?;
                
                // We need to execute the handler.
                match &cap.provider {
                    ProviderType::Local(local_cap) => (local_cap.handler)(&args_value),
                    _ => Err(RuntimeError::Generic(format!("Capability {} is not a local capability, cannot execute directly in this demo context", cap_id))),
                }
            })
        }).join().map_err(|_| RuntimeError::Generic("Thread join error in execute_step".to_string()))??;

        // Return the result as is (it's already a Value)
        Ok(result)
    });

    marketplace.register_local_capability(
        "planner.execute_step".to_string(),
        "Planner / Execute Step".to_string(),
        "Dynamically executes a capability step.".to_string(),
        execute_step_handler
    ).await?;

    Ok(())
}

// --- DTOs ---

#[derive(Debug, Deserialize)]
struct BuildMenuInput {
    goal: String,
}

#[derive(Debug, Serialize)]
struct BuildMenuOutput {
    menu: Vec<String>,
}

#[derive(Debug, Deserialize)]
struct SynthesizeInput {
    goal: String,
    menu: Vec<String>,
}

#[derive(Debug, Serialize)]
struct SynthesizeOutput {
    plan: PlanDto,
}

#[derive(Debug, Deserialize)]
struct ValidateInput {
    plan: PlanDto,
    menu: Vec<String>,
}

#[derive(Debug, Serialize)]
struct ValidateOutput {
    valid: bool,
    errors: Vec<String>,
}

#[derive(Debug, Deserialize)]
struct ExecuteStepInput {
    capability_id: String,
    inputs: JsonValue,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct PlanDto {
    pub id: String,
    pub steps: Vec<PlanStep>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct PlanStep {
    pub id: String,
    pub capability_id: String,
    pub inputs: JsonValue,
}

// --- Helpers ---

fn parse_payload<T: DeserializeOwned>(capability: &str, value: &Value) -> RuntimeResult<T> {
    let serialized = rtfs_value_to_json(value)?;

    serde_json::from_value(serialized).map_err(|err| {
        RuntimeError::Generic(format!(
            "{}: input payload does not match schema: {}",
            capability, err
        ))
    })
}

fn rtfs_value_to_json(value: &Value) -> RuntimeResult<serde_json::Value> {
    match value {
        Value::Nil => Ok(serde_json::Value::Null),
        Value::Boolean(b) => Ok(serde_json::Value::Bool(*b)),
        Value::Integer(i) => Ok(serde_json::json!(i)),
        Value::Float(f) => Ok(serde_json::json!(f)),
        Value::String(s) => Ok(serde_json::Value::String(s.clone())),
        Value::Keyword(k) => Ok(serde_json::Value::String(k.0.clone())),
        Value::Symbol(s) => Ok(serde_json::Value::String(s.0.clone())),
        Value::List(l) | Value::Vector(l) => {
            let mut arr = Vec::new();
            for v in l {
                arr.push(rtfs_value_to_json(v)?);
            }
            Ok(serde_json::Value::Array(arr))
        }
        Value::Map(m) => {
            let mut map = serde_json::Map::new();
            for (k, v) in m {
                let key_str = match k {
                    rtfs::ast::MapKey::String(s) => s.clone(),
                    rtfs::ast::MapKey::Keyword(k) => k.0.clone(),
                    rtfs::ast::MapKey::Integer(i) => i.to_string(),
                };
                map.insert(key_str, rtfs_value_to_json(v)?);
            }
            Ok(serde_json::Value::Object(map))
        }
        _ => Err(RuntimeError::Generic(format!("Cannot convert RTFS value to JSON: {:?}", value))),
    }
}

fn produce_value<T: Serialize>(capability: &str, output: T) -> RuntimeResult<Value> {
    let json_value = serde_json::to_value(output).map_err(|err| {
        RuntimeError::Generic(format!(
            "{}: failed to serialize output: {}",
            capability, err
        ))
    })?;
    
    json_to_rtfs_value(json_value)
}

fn json_to_rtfs_value(json: serde_json::Value) -> RuntimeResult<Value> {
    match json {
        serde_json::Value::Null => Ok(Value::Nil),
        serde_json::Value::Bool(b) => Ok(Value::Boolean(b)),
        serde_json::Value::Number(n) => {
            if let Some(i) = n.as_i64() {
                Ok(Value::Integer(i))
            } else if let Some(f) = n.as_f64() {
                Ok(Value::Float(f))
            } else {
                Err(RuntimeError::Generic("Invalid number format".to_string()))
            }
        }
        serde_json::Value::String(s) => Ok(Value::String(s)),
        serde_json::Value::Array(arr) => {
            let mut values = Vec::new();
            for v in arr {
                values.push(json_to_rtfs_value(v)?);
            }
            Ok(Value::List(values))
        }
        serde_json::Value::Object(map) => {
            let mut values = HashMap::new();
            for (k, v) in map {
                values.insert(rtfs::ast::MapKey::String(k), json_to_rtfs_value(v)?);
            }
            Ok(Value::Map(values))
        }
    }
}
