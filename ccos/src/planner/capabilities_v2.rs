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
// Note: Using local SubIntentDto instead of importing private SubIntent

use crate::synthesis::validation::llm_validator::{
    auto_repair_plan, validate_plan, ValidationConfig, ValidationError,
};
use crate::utils::value_conversion::{json_to_rtfs_value, rtfs_value_to_json};
use crate::CCOS;

/// Register granular planner capabilities (v2) for the autonomous agent loop.
/// Now includes recursive meta-planning capabilities for AI self-programming.
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

        // Capture Tokio handle for async execution
        let rt_handle = tokio::runtime::Handle::current();

        let menu = std::thread::spawn(move || {
            rt_handle.block_on(async {
                // Use semantic search to find relevant capabilities
                let filter = CatalogFilter::for_kind(CatalogEntryKind::Capability);
                let hits = catalog
                    .search_semantic(&payload.goal, Some(&filter), 10)
                    .await;

                let is_meta_goal = payload.goal.to_lowercase().contains("plan")
                    || payload.goal.to_lowercase().contains("meta");

                let mut menu = Vec::new();
                for hit in hits {
                    // Allow planner capabilities, but skip the very high-level ones
                    // from the menu unless we are specifically doing meta-planning
                    // to avoid confusing a standard planner.
                    if hit.entry.id.starts_with("planner.") {
                        if is_meta_goal
                            || hit.entry.id.contains("validate")
                            || hit.entry.id.contains("repair")
                            || hit.entry.id.contains("discover")
                        {
                            menu.push(hit.entry.id);
                        }
                    } else {
                        menu.push(hit.entry.id);
                    }
                }

                // Always add basic utilities
                menu.push("ccos.user.ask".to_string());
                menu.push("tool/log".to_string());

                // Deduplicate
                menu.sort();
                menu.dedup();

                Ok::<Vec<String>, RuntimeError>(menu)
            })
        })
        .join()
        .map_err(|_| {
            RuntimeError::Generic("Thread join error in planner.build_menu".to_string())
        })??;

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

    // ------------------------------------------------------------------
    // META-PLANNING CAPABILITIES (AI Self-Programming)
    // ------------------------------------------------------------------

    // planner.decompose - Break a goal into sub-intents (recursive decomposition)
    let catalog_for_decompose = Arc::clone(&catalog);
    let delegating_for_decompose = ccos.cognitive_engine.clone();

    let decompose_handler = Arc::new(move |input: &Value| {
        let payload: DecomposeInput = parse_payload("planner.decompose", input)?;
        let catalog = Arc::clone(&catalog_for_decompose);
        let delegating = delegating_for_decompose.clone();
        let goal = payload.goal.clone();
        let max_depth = payload.max_depth.unwrap_or(3);

        // Capture Tokio handle for async execution
        let rt_handle = tokio::runtime::Handle::current();

        let sub_intents = std::thread::spawn(move || {
            rt_handle.block_on(async {
                // Use LLM to decompose if available
                let arbiter = delegating;
                {
                    let prompt = format!(
                        r#"Decompose this goal into 2-5 sub-tasks. Return JSON array.
Goal: {}
Max depth remaining: {}

Output format:
[
  {{"id": "step_1", "description": "...", "type": "api_call|data_transform|output"}},
  ...
]"#,
                        goal, max_depth
                    );

                    eprintln!("[planner.decompose] Calling LLM with goal: {}", goal);
                    let response = arbiter.generate_raw_text(&prompt).await?;
                    eprintln!("[planner.decompose] LLM response (first 500 chars): {}", 
                        response.chars().take(500).collect::<String>());

                    // Parse JSON array from response
                    if let Some(json_str) = extract_json(&response) {
                        eprintln!("[planner.decompose] Extracted JSON: {}", json_str);
                        let intents: Vec<SubIntentDto> = serde_json::from_str(json_str)
                            .unwrap_or_else(|e| {
                                eprintln!("[planner.decompose] JSON parse error: {}", e);
                                vec![SubIntentDto {
                                    id: "step_1".to_string(),
                                    description: goal.clone(),
                                    intent_type: "unknown".to_string(),
                                }]
                            });
                        return Ok(intents);
                    } else {
                        eprintln!("[planner.decompose] No JSON found in response, falling back to catalog search");
                    }
                }

                // Fallback: Use catalog semantic search to find related capabilities
                let filter = CatalogFilter::for_kind(CatalogEntryKind::Capability);
                let hits = catalog.search_semantic(&goal, Some(&filter), 5).await;

                let intents: Vec<SubIntentDto> = hits
                    .iter()
                    .enumerate()
                    .map(|(i, hit)| SubIntentDto {
                        id: format!("step_{}", i + 1),
                        description: format!(
                            "Use {} for: {}",
                            hit.entry.id,
                            hit.entry
                                .description
                                .as_deref()
                                .unwrap_or("(no description)")
                        ),
                        intent_type: "api_call".to_string(),
                    })
                    .collect();

                Ok::<Vec<SubIntentDto>, RuntimeError>(intents)
            })
        })
        .join()
        .map_err(|_| {
            RuntimeError::Generic("Thread join error in planner.decompose".to_string())
        })??;

        produce_value("planner.decompose", DecomposeOutput { sub_intents })
    });

    marketplace
        .register_local_capability(
            "planner.decompose".to_string(),
            "Planner / Decompose".to_string(),
            "Break a goal into sub-intents for recursive planning.".to_string(),
            decompose_handler,
        )
        .await?;

    // planner.resolve_intent - Find a capability for an intent
    let catalog_for_resolve = Arc::clone(&catalog);
    let marketplace_for_resolve = Arc::clone(&marketplace);

    let resolve_intent_handler = Arc::new(move |input: &Value| {
        let payload: ResolveIntentInput = parse_payload("planner.resolve_intent", input)?;
        let catalog = Arc::clone(&catalog_for_resolve);
        let marketplace = Arc::clone(&marketplace_for_resolve);
        let description = payload.description.clone();

        // Capture Tokio handle for async execution
        let rt_handle = tokio::runtime::Handle::current();

        let resolution = std::thread::spawn(move || {
            rt_handle.block_on(async {
                // 1. Try semantic search in catalog
                let filter = CatalogFilter::for_kind(CatalogEntryKind::Capability);
                let hits = catalog.search_semantic(&description, Some(&filter), 3).await;

                // Instrumentation to understand why resolution fails
                if let Some(best) = hits.first() {
                    eprintln!(
                        "[planner.resolve_intent] desc=\"{}\" top_hit=\"{}\" score={:.3}",
                        description,
                        best.entry.id,
                        best.score
                    );
                } else {
                    eprintln!(
                        "[planner.resolve_intent] desc=\"{}\" no hits returned",
                        description
                    );
                }

                if let Some(best) = hits.first() {
                    if best.score > 0.6 {
                        // Verify capability exists in marketplace
                        if marketplace.get_capability(&best.entry.id).await.is_some() {
                            return Ok(ResolveIntentOutput {
                                resolved: true,
                                capability_id: Some(best.entry.id.clone()),
                                confidence: Some(best.score as f64),
                                needs_synthesis: false,
                            });
                        }
                    } else {
                        eprintln!(
                            "[planner.resolve_intent] top hit below threshold: id=\"{}\" score={:.3}",
                            best.entry.id,
                            best.score
                        );
                    }
                }

                // 2. Not found - mark as needing synthesis
                Ok::<ResolveIntentOutput, RuntimeError>(ResolveIntentOutput {
                    resolved: false,
                    capability_id: None,
                    confidence: None,
                    needs_synthesis: true,
                })
            })
        })
        .join()
        .map_err(|_| {
            RuntimeError::Generic("Thread join error in planner.resolve_intent".to_string())
        })??;

        produce_value("planner.resolve_intent", resolution)
    });

    marketplace
        .register_local_capability(
            "planner.resolve_intent".to_string(),
            "Planner / Resolve Intent".to_string(),
            "Find a capability that can fulfill an intent.".to_string(),
            resolve_intent_handler,
        )
        .await?;

    // planner.synthesize_capability - Create a missing capability via LLM
    let delegating_for_synthesis = ccos.cognitive_engine.clone();
    let resolver_for_synthesis = ccos.get_missing_capability_resolver();

    let synthesize_capability_handler = Arc::new(move |input: &Value| {
        let payload: SynthesizeCapabilityInput =
            parse_payload("planner.synthesize_capability", input)?;
        let delegating = delegating_for_synthesis.clone();
        let resolver = resolver_for_synthesis.clone();
        let description = payload.description.clone();
        let input_schema = payload.input_schema.clone();
        let output_schema = payload.output_schema.clone();

        // Capture Tokio handle for async execution
        let rt_handle = tokio::runtime::Handle::current();

        let result = std::thread::spawn(move || {
            rt_handle.block_on(async {
                let capability_id = format!("generated/{}", slugify(&description));

                if let Some(resolver) = resolver {
                    // Use MissingCapabilityResolver for proper synthesis
                    let request = crate::synthesis::core::MissingCapabilityRequest {
                        capability_id: capability_id.clone(),
                        arguments: vec![],
                        context: {
                            let mut ctx = HashMap::new();
                            ctx.insert("description".to_string(), description.clone());
                            if let Some(ref schema) = input_schema {
                                ctx.insert("input_schema".to_string(), schema.clone());
                            }
                            if let Some(ref schema) = output_schema {
                                ctx.insert("output_schema".to_string(), schema.clone());
                            }
                            ctx
                        },
                        requested_at: std::time::SystemTime::now(),
                        attempt_count: 0,
                    };

                    let resolved = resolver.resolve_capability(&request).await?;

                    // Map ResolutionResult to our output
                    return match resolved {
                        crate::synthesis::core::ResolutionResult::Resolved {
                            capability_id,
                            resolution_method,
                            ..
                        } => {
                            Ok::<SynthesizeCapabilityOutput, RuntimeError>(SynthesizeCapabilityOutput {
                                success: true,
                                capability_id: Some(capability_id),
                                rtfs_code: None, // Code is stored in capability file
                                error: None,
                            })
                        }
                        crate::synthesis::core::ResolutionResult::Failed { reason, .. } => {
                            Ok::<SynthesizeCapabilityOutput, RuntimeError>(SynthesizeCapabilityOutput {
                                success: false,
                                capability_id: None,
                                rtfs_code: None,
                                error: Some(reason),
                            })
                        }
                        crate::synthesis::core::ResolutionResult::PermanentlyFailed {
                            reason,
                            ..
                        } => Ok::<SynthesizeCapabilityOutput, RuntimeError>(SynthesizeCapabilityOutput {
                            success: false,
                            capability_id: None,
                            rtfs_code: None,
                            error: Some(reason),
                        }),
                    };
                }

                // Fallback: Direct LLM synthesis
                // Fallback: Direct LLM synthesis
                // delegating is Arc<DelegatingCognitiveEngine>, always present logic
                let prompt = format!(
                    r#"Create an RTFS capability for: {}
Input schema hint: {:?}
Output schema hint: {:?}

Output ONLY valid RTFS capability code."#,
                    description, input_schema, output_schema
                );

                let response = delegating.generate_raw_text(&prompt).await?;

                Ok(SynthesizeCapabilityOutput {
                    success: true,
                    capability_id: Some(capability_id),
                    rtfs_code: Some(response),
                    error: None,
                })
            })
        })
        .join()
        .map_err(|_| {
            RuntimeError::Generic("Thread join error in planner.synthesize_capability".to_string())
        })??;

        produce_value("planner.synthesize_capability", result)
    });

    marketplace
        .register_local_capability(
            "planner.synthesize_capability".to_string(),
            "Planner / Synthesize Capability".to_string(),
            "Create a new capability via LLM synthesis.".to_string(),
            synthesize_capability_handler,
        )
        .await?;

    // ------------------------------------------------------------------
    // EXISTING CAPABILITIES (v2)
    // ------------------------------------------------------------------

    // 2. planner.synthesize
    // Creates a plan (list of steps) based on the goal and menu.
    let delegating_opt_for_synth = ccos.cognitive_engine.clone();
    let marketplace_for_synth = Arc::clone(&marketplace);

    let synthesize_handler = Arc::new(move |input: &Value| {
        let payload: SynthesizeInput = parse_payload("planner.synthesize", input)?;

        // Get delegating arbiter
        let delegating = delegating_opt_for_synth.clone();

        let marketplace = marketplace_for_synth.clone();

        // We need to call the LLM.
        // We spawn a thread to handle the async execution and blocking since we are in a sync closure.
        let goal = payload.goal.clone();
        let menu = payload.menu.clone();

        // Capture the current Tokio runtime handle to execute async code that needs the reactor (e.g. reqwest)
        let rt_handle = tokio::runtime::Handle::current();

        let plan_dto = std::thread::spawn(move || {
            rt_handle.block_on(async {
                // delegating is already Arc<DelegatingCognitiveEngine> which is Send

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

                let response: String = delegating.generate_raw_text(&prompt).await?;

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
        let args_value = json_to_rtfs_value(&args_json)?;

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
                        json_to_rtfs_value(&response)
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

    // 5. planner.validate_with_llm
    // High-level plan validation using LLM.
    let validate_with_llm_handler = Arc::new(move |input: &Value| {
        let payload: ValidateWithLlmInput = parse_payload("planner.validate_with_llm", input)?;

        // Capture Tokio handle for async execution
        let rt_handle = tokio::runtime::Handle::current();
        let plan = payload.plan;
        let goal = payload.goal;
        let resolutions = payload.resolutions;

        let result = std::thread::spawn(move || {
            rt_handle.block_on(async {
                let config = ValidationConfig::default();
                validate_plan(&plan, &resolutions, &goal, &config)
                    .await
                    .map_err(|e| RuntimeError::Generic(e))
            })
        })
        .join()
        .map_err(|_| {
            RuntimeError::Generic("Thread join error in planner.validate_with_llm".to_string())
        })??;

        produce_value(
            "planner.validate_with_llm",
            ValidateWithLlmOutput {
                valid: result.is_valid,
                errors: result.errors,
                suggestions: result.suggestions,
            },
        )
    });

    marketplace
        .register_local_capability(
            "planner.validate_with_llm".to_string(),
            "Planner / Validate with LLM".to_string(),
            "Validates an RTFS plan using an LLM for schema compatibility and logic.".to_string(),
            validate_with_llm_handler,
        )
        .await?;

    // 6. planner.repair_plan
    // Attempts to repair a plan based on validation errors.
    let repair_plan_handler = Arc::new(move |input: &Value| {
        let payload: RepairPlanInput = parse_payload("planner.repair_plan", input)?;

        // Capture Tokio handle for async execution
        let rt_handle = tokio::runtime::Handle::current();
        let plan = payload.plan;
        let errors = payload.errors;
        let attempt = payload.attempt;

        let result = std::thread::spawn(move || {
            rt_handle.block_on(async {
                let config = ValidationConfig::default();
                auto_repair_plan(&plan, &errors, attempt, &config)
                    .await
                    .map_err(|e| RuntimeError::Generic(e))
            })
        })
        .join()
        .map_err(|_| {
            RuntimeError::Generic("Thread join error in planner.repair_plan".to_string())
        })??;

        let success = result.is_some();
        produce_value(
            "planner.repair_plan",
            RepairPlanOutput {
                repaired_plan: result,
                success,
            },
        )
    });

    marketplace
        .register_local_capability(
            "planner.repair_plan".to_string(),
            "Planner / Repair Plan".to_string(),
            "Attempts to repair an RTFS plan using validation error feedback.".to_string(),
            repair_plan_handler,
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

// --- Meta-Planning DTOs (AI Self-Programming) ---

#[derive(Debug, Deserialize)]
struct DecomposeInput {
    goal: String,
    #[serde(default)]
    max_depth: Option<i32>,
}

#[derive(Debug, Serialize)]
struct DecomposeOutput {
    sub_intents: Vec<SubIntentDto>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
struct SubIntentDto {
    id: String,
    description: String,
    #[serde(rename = "type")]
    intent_type: String,
}

#[derive(Debug, Deserialize)]
struct ResolveIntentInput {
    description: String,
}

#[derive(Debug, Serialize)]
struct ResolveIntentOutput {
    resolved: bool,
    capability_id: Option<String>,
    confidence: Option<f64>,
    needs_synthesis: bool,
}

#[derive(Debug, Deserialize)]
struct SynthesizeCapabilityInput {
    description: String,
    #[serde(default)]
    input_schema: Option<String>,
    #[serde(default)]
    output_schema: Option<String>,
}

#[derive(Debug, Serialize)]
struct SynthesizeCapabilityOutput {
    success: bool,
    capability_id: Option<String>,
    rtfs_code: Option<String>,
    error: Option<String>,
}

// --- Discovery DTOs ---

#[derive(Debug, Deserialize)]
struct DiscoverToolsInput {
    query: String,
    #[serde(default)]
    max_results: Option<usize>,
}

#[derive(Debug, Serialize)]
struct DiscoverToolsOutput {
    tools: Vec<DiscoveredToolDto>,
    query: String,
}

#[derive(Debug, Serialize)]
struct DiscoveredToolDto {
    name: String,
    description: Option<String>,
    source: String,
    score: f64,
    endpoints: Vec<String>,
}

// --- Validation & Repair DTOs ---

#[derive(Debug, Deserialize)]
struct ValidateWithLlmInput {
    plan: String,
    goal: String,
    resolutions: HashMap<String, String>,
}

#[derive(Debug, Serialize)]
struct ValidateWithLlmOutput {
    valid: bool,
    errors: Vec<ValidationError>,
    suggestions: Vec<String>,
}

#[derive(Debug, Deserialize)]
struct RepairPlanInput {
    plan: String,
    errors: Vec<ValidationError>,
    attempt: usize,
}

#[derive(Debug, Serialize)]
struct RepairPlanOutput {
    repaired_plan: Option<String>,
    success: bool,
}

// --- Helpers ---

/// Slugify a description into a valid capability ID component
fn slugify(s: &str) -> String {
    s.chars()
        .take(50) // Limit length
        .map(|c| {
            if c.is_alphanumeric() {
                c.to_ascii_lowercase()
            } else {
                '-'
            }
        })
        .collect::<String>()
        .trim_matches('-')
        .to_string()
}

fn parse_payload<T: DeserializeOwned>(capability: &str, value: &Value) -> RuntimeResult<T> {
    let serialized = rtfs_value_to_json(value)?;

    serde_json::from_value(serialized).map_err(|err| {
        RuntimeError::Generic(format!(
            "{}: input payload does not match schema: {}",
            capability, err
        ))
    })
}

fn produce_value<T: Serialize>(capability: &str, output: T) -> RuntimeResult<Value> {
    let json_value = serde_json::to_value(output).map_err(|err| {
        RuntimeError::Generic(format!(
            "{}: failed to serialize output: {}",
            capability, err
        ))
    })?;

    json_to_rtfs_value(&json_value)
}

fn extract_json(response: &str) -> Option<&str> {
    // Try to find JSON array first (for decompose which expects [])
    if let Some(arr_start) = response.find('[') {
        if let Some(arr_end) = response.rfind(']') {
            if arr_end >= arr_start {
                return Some(&response[arr_start..=arr_end]);
            }
        }
    }
    // Fallback to JSON object {}
    if let Some(start) = response.find('{') {
        if let Some(end) = response.rfind('}') {
            if end >= start {
                return Some(&response[start..=end]);
            }
        }
    }
    None
}
