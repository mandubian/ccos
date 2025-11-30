use std::collections::HashMap;
use std::sync::Arc;

use serde::{de::DeserializeOwned, Deserialize, Serialize};
use serde_json::Value as JsonValue;

use rtfs::runtime::error::{RuntimeError, RuntimeResult};
use rtfs::runtime::values::Value;

use crate::capability_marketplace::types::ProviderType;
use crate::capability_marketplace::CapabilityMarketplace;
use crate::catalog::{CatalogEntryKind, CatalogFilter, CatalogService};
use crate::mcp::discovery_session::{MCPServerInfo, MCPSessionManager};
use crate::CCOS;

/// Register granular planner capabilities (v2) for the autonomous agent loop.
pub async fn register_planner_capabilities_v2(
    marketplace: Arc<CapabilityMarketplace>,
    catalog: Arc<CatalogService>,
    ccos: Arc<CCOS>,
) -> RuntimeResult<()> {
    // 1. planner.build_menu
    // Scans the catalog/marketplace for capabilities relevant to the goal.
    let catalog_for_menu = Arc::clone(&catalog);
    let build_menu_handler = Arc::new(move |input: &Value| {
        let payload: BuildMenuInput = parse_payload("planner.build_menu", input)?;
        let catalog = Arc::clone(&catalog_for_menu);

        // Use semantic search to find relevant capabilities
        let filter = CatalogFilter::for_kind(CatalogEntryKind::Capability);
        let hits = catalog.search_semantic(&payload.goal, Some(&filter), 10);

        let mut menu = Vec::new();
        for hit in hits {
            // Filter out internal planner capabilities to avoid infinite recursion
            if !hit.entry.id.starts_with("planner.") {
                menu.push(hit.entry.id);
            }
        }

        // Always add basic utilities
        menu.push("ccos.user.ask".to_string());
        menu.push("tool/log".to_string());

        // Deduplicate
        menu.sort();
        menu.dedup();

        produce_value("planner.build_menu", BuildMenuOutput { menu })
    });

    marketplace
        .register_local_capability(
            "planner.build_menu".to_string(),
            "Planner / Build Menu".to_string(),
            "Selects a list of relevant capabilities for a given goal.".to_string(),
            build_menu_handler,
        )
        .await?;

    // 2. planner.synthesize
    // Creates a plan (list of steps) based on the goal and menu.
    let delegating_opt_for_synth = ccos.get_delegating_arbiter();
    let marketplace_for_synth = Arc::clone(&marketplace);

    let synthesize_handler = Arc::new(move |input: &Value| {
        let payload: SynthesizeInput = parse_payload("planner.synthesize", input)?;

        // Get delegating arbiter
        let delegating = delegating_opt_for_synth
            .clone()
            .ok_or_else(|| RuntimeError::Generic("Delegating arbiter not available".to_string()))?;

        let marketplace = marketplace_for_synth.clone();

        // We need to call the LLM.
        // We spawn a thread to handle the async execution and blocking since we are in a sync closure.
        let goal = payload.goal.clone();
        let menu = payload.menu.clone();

        // Capture the current Tokio runtime handle to execute async code that needs the reactor (e.g. reqwest)
        let rt_handle = tokio::runtime::Handle::current();

        let plan_dto = std::thread::spawn(move || {
            rt_handle.block_on(async {
                // delegating is already Arc<DelegatingArbiter> which is Send
                
                // Enhance menu with capability details and schemas
                let mut detailed_menu = Vec::new();
                for cap_id in &menu {
                    if let Some(cap) = marketplace.get_capability(cap_id).await {
                        let schema_str = if let Some(schema) = &cap.input_schema {
                             match schema.to_json() {
                                 Ok(json) => serde_json::to_string_pretty(&json).unwrap_or_else(|_| "Invalid JSON".to_string()),
                                 Err(_) => "Schema unavailable".to_string()
                             }
                        } else {
                             "No schema".to_string()
                        };
                        
                        detailed_menu.push(format!("- {}\n  Description: {}\n  Input Schema: {}", 
                            cap_id, 
                            cap.description,
                            schema_str
                        ));
                    } else {
                        detailed_menu.push(format!("- {}", cap_id));
                    }
                }
                
                let menu_list = detailed_menu.join("\n\n");
                
                let prompt = format!(
                    r#"You are an autonomous planner.
Goal: {}

Available Capabilities:
{}

Instructions:
1. Select capabilities from the list above to achieve the goal.
2. Create a sequence of steps.
3. For each step, provide the 'id' (e.g., step_1), 'capability_id', and 'inputs' (as a JSON object).
4. CRITICAL: Use ONLY the parameters defined in the Input Schema. Do NOT hallucinate parameters like 'first', 'sort', etc. if they are not in the schema.
5. CRITICAL: Respect Enum values EXACTLY (case-sensitive). If schema says "DESC", do not use "desc".
6. If you need to search, use search tools. If you need to ask the user, use 'ccos.user.ask'.
7. Output ONLY valid JSON matching this structure:
{{
  "id": "plan_id",
  "steps": [
    {{ "id": "step_1", "capability_id": "...", "inputs": {{ ... }} }},
    ...
  ]
}}
"#, 
                    goal, menu_list
                );
                
                // DEBUG: Print prompt to verify schema injection
                // println!("DEBUG: Prompt sent to LLM:\n{}", prompt);
                
                let response = delegating.generate_raw_text(&prompt).await?;
                
                // Extract JSON from response
                let json_str = extract_json(&response).ok_or_else(|| 
                    RuntimeError::Generic("No JSON found in LLM response".to_string())
                )?;
                
                let plan: PlanDto = serde_json::from_str(json_str).map_err(|e| 
                    RuntimeError::Generic(format!("Failed to parse plan JSON: {}", e))
                )?;
                
                Ok::<PlanDto, RuntimeError>(plan)
            })
        }).join().map_err(|_| RuntimeError::Generic("Thread join error in planner.synthesize".to_string()))??;

        produce_value("planner.synthesize", SynthesizeOutput { plan: plan_dto })
    });

    marketplace
        .register_local_capability(
            "planner.synthesize".to_string(),
            "Planner / Synthesize".to_string(),
            "Generates a plan (steps) from a goal and a menu.".to_string(),
            synthesize_handler,
        )
        .await?;

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
            if !payload.menu.contains(&step.capability_id) && step.capability_id != "ccos.user.ask"
            {
                // Allow ccos.user.ask as a fallback even if not in menu explicitly
                // valid = false;
                // errors.push(format!("Capability {} not in menu", step.capability_id));
            }
        }

        produce_value("planner.validate", ValidateOutput { valid, errors })
    });

    marketplace
        .register_local_capability(
            "planner.validate".to_string(),
            "Planner / Validate".to_string(),
            "Validates a generated plan.".to_string(),
            validate_handler,
        )
        .await?;

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

        // Convert JSON args to RTFS Value (clone args_json as we might need the original json for MCP)
        let args_value = json_to_rtfs_value(args_json.clone())?;

        // Execute
        // We spawn a new thread to avoid nested LocalPool execution issues
        // (host.rs uses block_on, and if we use block_on here on the same thread, it panics)
        let marketplace_clone = marketplace_for_exec.clone();

        // Capture the current Tokio runtime handle to execute async code that needs the reactor
        let rt_handle = tokio::runtime::Handle::current();

        let result = std::thread::spawn(move || {
            rt_handle.block_on(async {
                let cap = marketplace_clone
                    .get_capability(&cap_id)
                    .await
                    .ok_or_else(|| {
                        RuntimeError::Generic(format!("Capability {} not found", cap_id))
                    })?;

                // We need to execute the handler.
                match &cap.provider {
                    ProviderType::Local(local_cap) => (local_cap.handler)(&args_value),
                    ProviderType::MCP(mcp_cap) => {
                        // MCP Execution Logic

                        // Determine Auth Token from environment
                        // In a production system, we would look this up based on the capability's configuration/metadata
                        // For this demo, we stick to the standard MCP_AUTH_TOKEN
                        let auth_token = std::env::var("MCP_AUTH_TOKEN").ok();

                        let auth_headers = auth_token.map(|token| {
                            let mut headers = HashMap::new();
                            headers
                                .insert("Authorization".to_string(), format!("Bearer {}", token));
                            headers
                        });

                        let session_manager = MCPSessionManager::new(auth_headers);
                        let client_info = MCPServerInfo {
                            name: "ccos-planner".to_string(),
                            version: "1.0.0".to_string(),
                        };

                        // Initialize session
                        let session = session_manager
                            .initialize_session(&mcp_cap.server_url, &client_info)
                            .await?;

                        // Call tool
                        // args_json is already serde_json::Value from the payload
                        let result_json = session_manager
                            .make_request(
                                &session,
                                "tools/call",
                                serde_json::json!({
                                    "name": mcp_cap.tool_name,
                                    "arguments": args_json
                                }),
                            )
                            .await;

                        // Terminate session
                        let _ = session_manager.terminate_session(&session).await;

                        let response = result_json?;

                        // Convert response to RTFS Value
                        json_to_rtfs_value(response)
                    }
                    _ => Err(RuntimeError::Generic(format!(
                        "Capability {} is not a supported capability type in this demo context",
                        cap_id
                    ))),
                }
            })
        })
        .join()
        .map_err(|_| RuntimeError::Generic("Thread join error in execute_step".to_string()))??;

        // Return the result as is (it's already a Value)
        Ok(result)
    });

    marketplace
        .register_local_capability(
            "planner.execute_step".to_string(),
            "Planner / Execute Step".to_string(),
            "Dynamically executes a capability step.".to_string(),
            execute_step_handler,
        )
        .await?;

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
        _ => Err(RuntimeError::Generic(format!(
            "Cannot convert RTFS value to JSON: {:?}",
            value
        ))),
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

fn extract_json(response: &str) -> Option<&str> {
    if let Some(start) = response.find('{') {
        if let Some(end) = response.rfind('}') {
            if end >= start {
                return Some(&response[start..=end]);
            }
        }
    }
    None
}
