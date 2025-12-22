//! Modular Planner Steps
//!
//! This module exposes the individual steps of the planning process as
//! separate, testable functions. Each step corresponds to a `planner.*`
//! meta-capability that can be called from RTFS.
//!
//! ## Steps Overview
//!
//! 1. **Tool Discovery** - Discover available capabilities for a goal
//! 2. **Decomposition** - Break goal into sub-intents
//! 3. **Intent Storage** - Store intents in the IntentGraph
//! 4. **Resolution** - Map intents to capabilities
//! 5. **Refinement** - Further decompose unresolved intents
//! 6. **Synthesis** - Create missing capabilities
//! 7. **Safe Execution** - Execute read-only capabilities for grounding
//! 8. **RTFS Generation** - Generate executable plan code
//! 9. **Validation** - Validate the generated plan
//! 10. **Archiving** - Save plan to persistent storage

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};

use uuid::Uuid;

use crate::discovery::registry_search::{RegistrySearchResult, RegistrySearcher};
use crate::intent_graph::{Edge, IntentGraph};
use crate::plan_archive::PlanArchive;
use crate::planner::modular_planner::decomposition::{
    DecompositionContext, DecompositionResult, DecompositionStrategy,
};
use crate::planner::modular_planner::orchestrator::{
    PlannerConfig, PlannerError, PlanningTrace, TraceEvent,
};
use crate::planner::modular_planner::resolution::{
    ResolutionContext, ResolutionStrategy, ResolvedCapability,
};
use crate::planner::modular_planner::types::{
    ApiAction, DomainHint, IntentType, SubIntent, ToolSummary,
};
use crate::synthesis::core::{MissingCapabilityRequest, MissingCapabilityStrategy};
use crate::types::{
    EdgeType, GenerationContext, IntentStatus, Plan, PlanStatus, StorableIntent, TriggerSource,
};

/// Result of the tool discovery step
#[derive(Debug, Clone)]
pub struct ToolDiscoveryResult {
    /// Discovered tools, ranked by relevance
    pub tools: Vec<ToolSummary>,
    /// Domain hints inferred from the goal
    pub domain_hints: Vec<DomainHint>,
}

/// Result of the intent storage step
#[derive(Debug, Clone)]
pub struct IntentStorageResult {
    /// Root intent ID for the goal
    pub root_id: String,
    /// IDs assigned to each sub-intent
    pub intent_ids: Vec<String>,
}

/// Result of the resolution step
#[derive(Debug, Clone)]
pub struct ResolutionResult {
    /// Resolved capabilities keyed by intent ID
    pub resolutions: HashMap<String, ResolvedCapability>,
    /// Intent IDs that could not be resolved
    pub unresolved: Vec<String>,
}

/// Result of the safe execution step
#[derive(Debug, Clone)]
pub struct SafeExecutionResult {
    /// Grounding data keyed by intent ID
    pub grounding_data: HashMap<String, serde_json::Value>,
    /// Formatted grounding params for prompts
    pub grounding_params: HashMap<String, String>,
}

/// Result of RTFS plan generation
#[derive(Debug, Clone)]
pub struct PlanGenerationResult {
    /// Generated RTFS code
    pub rtfs_code: String,
    /// Whether the plan has pending synthesis
    pub has_pending_synthesis: bool,
}

/// Result of plan archiving
#[derive(Debug, Clone)]
pub struct ArchiveResult {
    /// Assigned plan ID
    pub plan_id: String,
    /// Content-addressable hash
    pub archive_hash: String,
    /// Path to the archive
    pub archive_path: PathBuf,
}

/// Result of discovering new tools for unresolved intents
#[derive(Debug, Clone)]
pub struct NewToolsDiscoveryResult {
    /// Servers/APIs discovered that might provide the capability
    pub discovered_servers: Vec<RegistrySearchResult>,
    /// Tools that were converted to ToolSummary for potential use
    pub candidate_tools: Vec<ToolSummary>,
    /// Intent IDs that were searched for
    pub searched_intents: Vec<String>,
}

/// Step 0: Discover tools relevant to the goal
///
/// This step infers domain hints from the goal text and queries
/// the resolution strategy for available tools in those domains.
pub async fn step_discover_tools(
    goal: &str,
    resolution: &dyn ResolutionStrategy,
) -> Result<ToolDiscoveryResult, PlannerError> {
    // Infer domain hints from the goal
    let domain_hints = DomainHint::infer_all_from_text(goal);

    // Query resolution strategy for available tools
    let mut tools = resolution.list_available_tools(Some(&domain_hints)).await;

    // Rank tools by action type (search/list/get first, then CRUD)
    use crate::planner::modular_planner::types::ApiAction;

    fn tool_rank(t: &ToolSummary) -> i32 {
        let name_lc = t.name.to_lowercase();
        let id_lc = t.id.to_lowercase();
        let action_bonus = match t.action {
            ApiAction::Search | ApiAction::List | ApiAction::Get => 3,
            ApiAction::Create | ApiAction::Update | ApiAction::Delete => 1,
            _ => 0,
        };
        let transform_bonus = if name_lc.contains("format")
            || name_lc.contains("filter")
            || name_lc.contains("sort")
            || name_lc.contains("select")
            || id_lc.contains("format")
            || id_lc.contains("filter")
            || id_lc.contains("sort")
            || id_lc.contains("select")
        {
            2
        } else {
            0
        };
        action_bonus + transform_bonus
    }

    tools.sort_by(|a, b| tool_rank(b).cmp(&tool_rank(a)));

    Ok(ToolDiscoveryResult {
        tools,
        domain_hints,
    })
}

/// Step 1: Decompose goal into sub-intents
///
/// Uses the decomposition strategy (pattern matching or LLM) to break
/// the goal into a sequence of sub-intents with dependencies.
pub async fn step_decompose(
    goal: &str,
    tools: Option<&[ToolSummary]>,
    decomposition: &dyn DecompositionStrategy,
    config: &PlannerConfig,
    grounding_params: &HashMap<String, String>,
    trace: &mut PlanningTrace,
) -> Result<DecompositionResult, PlannerError> {
    trace.events.push(TraceEvent::DecompositionStarted {
        strategy: decomposition.name().to_string(),
    });

    let mut decomp_context = DecompositionContext::new()
        .with_max_depth(config.max_depth)
        .with_verbose_llm(config.verbose_llm)
        .with_show_prompt(config.show_prompt)
        .with_confirm_llm(config.confirm_llm);

    // Inject any pre-existing grounding params
    for (k, v) in grounding_params.iter() {
        decomp_context
            .pre_extracted_params
            .insert(k.clone(), v.clone());
    }

    let result = decomposition
        .decompose(goal, tools, &decomp_context)
        .await?;

    trace.events.push(TraceEvent::DecompositionCompleted {
        num_intents: result.sub_intents.len(),
        confidence: result.confidence,
    });

    Ok(result)
}

/// Step 2: Store intents in the IntentGraph
///
/// Creates StorableIntent nodes for the goal and each sub-intent,
/// establishing parent-child and dependency edges.
pub async fn step_store_intents(
    goal: &str,
    sub_intents: &[SubIntent],
    intent_graph: &Arc<Mutex<IntentGraph>>,
    config: &PlannerConfig,
    trace: &mut PlanningTrace,
) -> Result<IntentStorageResult, PlannerError> {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs();

    // Create root intent for the overall goal
    let root_id = format!("{}:{}", config.intent_namespace, Uuid::new_v4());
    let root_intent = StorableIntent {
        intent_id: root_id.clone(),
        name: Some("Root Goal".to_string()),
        original_request: goal.to_string(),
        rtfs_intent_source: format!(r#"(intent "{}" :goal "{}")"#, root_id, goal),
        goal: goal.to_string(),
        constraints: HashMap::new(),
        preferences: HashMap::new(),
        success_criteria: None,
        parent_intent: None,
        child_intents: vec![],
        triggered_by: TriggerSource::HumanRequest,
        generation_context: GenerationContext {
            arbiter_version: "modular-planner-1.0".to_string(),
            generation_timestamp: now,
            input_context: HashMap::new(),
            reasoning_trace: None,
        },
        status: IntentStatus::Active,
        priority: 0,
        created_at: now,
        updated_at: now,
        metadata: HashMap::new(),
    };

    if config.persist_intents {
        let mut graph = intent_graph
            .lock()
            .map_err(|e| PlannerError::IntentGraph(e.to_string()))?;
        graph
            .store_intent(root_intent)
            .map_err(|e| PlannerError::IntentGraph(e.to_string()))?;
    }

    trace.events.push(TraceEvent::IntentCreated {
        intent_id: root_id.clone(),
        description: goal.to_string(),
    });

    // Create sub-intents
    let mut intent_ids = Vec::new();

    for (idx, sub_intent) in sub_intents.iter().enumerate() {
        let intent_id = format!("{}:step-{}", config.intent_namespace, Uuid::new_v4());
        intent_ids.push(intent_id.clone());

        let storable = StorableIntent {
            intent_id: intent_id.clone(),
            name: Some(format!("Step {}", idx + 1)),
            original_request: sub_intent.description.clone(),
            rtfs_intent_source: format!(
                r#"(intent "{}" :goal "{}")"#,
                intent_id, sub_intent.description
            ),
            goal: sub_intent.description.clone(),
            constraints: HashMap::new(),
            preferences: HashMap::new(),
            success_criteria: None,
            parent_intent: Some(root_id.clone()),
            child_intents: vec![],
            triggered_by: TriggerSource::PlanExecution,
            generation_context: GenerationContext {
                arbiter_version: "modular-planner-1.0".to_string(),
                generation_timestamp: now,
                input_context: sub_intent
                    .extracted_params
                    .iter()
                    .map(|(k, v)| (k.clone(), v.clone()))
                    .collect(),
                reasoning_trace: None,
            },
            status: IntentStatus::Active,
            priority: idx as u32,
            created_at: now,
            updated_at: now,
            metadata: {
                let mut meta = HashMap::new();
                meta.insert(
                    "intent_type".to_string(),
                    format!("{:?}", sub_intent.intent_type),
                );
                if let Some(ref domain) = sub_intent.domain_hint {
                    meta.insert("domain_hint".to_string(), format!("{:?}", domain));
                }
                meta
            },
        };

        if config.persist_intents {
            let mut graph = intent_graph
                .lock()
                .map_err(|e| PlannerError::IntentGraph(e.to_string()))?;
            graph
                .store_intent(storable)
                .map_err(|e| PlannerError::IntentGraph(e.to_string()))?;
        }

        trace.events.push(TraceEvent::IntentCreated {
            intent_id: intent_id.clone(),
            description: sub_intent.description.clone(),
        });

        // Create edge to root (IsSubgoalOf)
        if config.create_edges {
            let edge = Edge {
                from: intent_id.clone(),
                to: root_id.clone(),
                edge_type: EdgeType::IsSubgoalOf,
                metadata: None,
                weight: None,
            };

            if let Ok(mut graph) = intent_graph.lock() {
                let _ = futures::executor::block_on(graph.storage.store_edge(edge));
            }

            trace.events.push(TraceEvent::EdgeCreated {
                from: intent_id.clone(),
                to: root_id.clone(),
                edge_type: "IsSubgoalOf".to_string(),
            });
        }

        // Create DependsOn edges for dependencies
        if config.create_edges {
            for &dep_idx in &sub_intent.dependencies {
                if dep_idx < intent_ids.len() {
                    let dep_id = &intent_ids[dep_idx];
                    let edge = Edge {
                        from: intent_id.clone(),
                        to: dep_id.clone(),
                        edge_type: EdgeType::DependsOn,
                        metadata: None,
                        weight: None,
                    };

                    if let Ok(mut graph) = intent_graph.lock() {
                        let _ = futures::executor::block_on(graph.storage.store_edge(edge));
                    }

                    trace.events.push(TraceEvent::EdgeCreated {
                        from: intent_id.clone(),
                        to: dep_id.clone(),
                        edge_type: "DependsOn".to_string(),
                    });
                }
            }
        }
    }

    Ok(IntentStorageResult {
        root_id,
        intent_ids,
    })
}

/// Step 3: Resolve intents to capabilities
///
/// Maps each sub-intent to a capability using the resolution strategy.
/// Returns both resolved and unresolved intents.
pub async fn step_resolve_intents(
    sub_intents: &[SubIntent],
    intent_ids: &[String],
    resolution: &dyn ResolutionStrategy,
    trace: &mut PlanningTrace,
) -> Result<ResolutionResult, PlannerError> {
    let resolution_context = ResolutionContext::new();
    let mut resolutions = HashMap::new();
    let mut unresolved = Vec::new();

    for (idx, sub_intent) in sub_intents.iter().enumerate() {
        let intent_id = &intent_ids[idx];

        trace.events.push(TraceEvent::ResolutionStarted {
            intent_id: intent_id.clone(),
        });

        match resolution.resolve(sub_intent, &resolution_context).await {
            Ok(resolved) => {
                let cap_id = resolved.capability_id().unwrap_or("unknown").to_string();
                trace.events.push(TraceEvent::ResolutionCompleted {
                    intent_id: intent_id.clone(),
                    capability: cap_id.clone(),
                });
                resolutions.insert(intent_id.clone(), resolved);
            }
            Err(e) => {
                trace.events.push(TraceEvent::ResolutionFailed {
                    intent_id: intent_id.clone(),
                    reason: e.to_string(),
                });
                unresolved.push(intent_id.clone());
            }
        }
    }

    Ok(ResolutionResult {
        resolutions,
        unresolved,
    })
}

/// Step 4: Create fallback resolution for unresolved intents
///
/// For intents that couldn't be resolved, creates appropriate fallback
/// resolutions (user prompt, synthesis queue, etc.)
pub fn step_create_fallback_resolutions(
    sub_intents: &[SubIntent],
    intent_ids: &[String],
    unresolved_ids: &[String],
) -> HashMap<String, ResolvedCapability> {
    let mut fallbacks = HashMap::new();

    for unresolved_id in unresolved_ids {
        // Find the sub-intent for this ID
        if let Some(idx) = intent_ids.iter().position(|id| id == unresolved_id) {
            if let Some(sub_intent) = sub_intents.get(idx) {
                let fallback = match &sub_intent.intent_type {
                    IntentType::DataTransform { .. } | IntentType::Output { .. } => {
                        ResolvedCapability::NeedsReferral {
                            reason: "No capability found".to_string(),
                            suggested_action: format!(
                                "Synth-or-enqueue a capability for: {}",
                                sub_intent.description
                            ),
                        }
                    }
                    IntentType::UserInput { .. } => ResolvedCapability::NeedsReferral {
                        reason: "User input resolution failed".to_string(),
                        suggested_action: "Check ccos.user.ask registration".to_string(),
                    },
                    IntentType::ApiCall { .. } => {
                        let mut args = HashMap::new();
                        args.insert(
                            "prompt".to_string(),
                            format!(
                                "I couldn't find a capability for '{}'. How should I proceed?",
                                sub_intent.description
                            ),
                        );
                        ResolvedCapability::BuiltIn {
                            capability_id: "ccos.user.ask".to_string(),
                            arguments: args,
                        }
                    }
                    IntentType::Composite => ResolvedCapability::NeedsReferral {
                        reason: "Composite intent requires further decomposition".to_string(),
                        suggested_action: format!(
                            "Break down '{}' into smaller steps",
                            sub_intent.description
                        ),
                    },
                };
                fallbacks.insert(unresolved_id.clone(), fallback);
            }
        }
    }

    fallbacks
}

/// Result of strategy-based fallback resolution
#[derive(Debug, Clone)]
pub struct StrategyResolutionResult {
    /// Resolved capabilities (strategy-provided or fallback)
    pub resolutions: HashMap<String, ResolvedCapability>,
    /// Which strategy was used for each resolution (if any)
    pub resolution_methods: HashMap<String, String>,
}

/// Step 4b: Create fallback resolutions using strategy pattern
///
/// Enhanced version that tries resolution strategies before
/// falling back to user prompts.
pub async fn step_create_fallback_resolutions_with_strategies(
    sub_intents: &[SubIntent],
    intent_ids: &[String],
    unresolved_ids: &[String],
    strategy: Option<&dyn MissingCapabilityStrategy>,
    resolution_context: &ResolutionContext,
    trace: &mut PlanningTrace,
) -> StrategyResolutionResult {
    let mut resolutions = HashMap::new();
    let mut resolution_methods = HashMap::new();

    for unresolved_id in unresolved_ids {
        // Find the sub-intent for this ID
        if let Some(idx) = intent_ids.iter().position(|id| id == unresolved_id) {
            if let Some(sub_intent) = sub_intents.get(idx) {
                // Try strategy-based resolution if available
                if let Some(strat) = strategy {
                    let request = MissingCapabilityRequest {
                        capability_id: format!(
                            "synthesized.{}",
                            sub_intent
                                .description
                                .to_lowercase()
                                .replace(' ', "_")
                                .chars()
                                .filter(|c| c.is_alphanumeric() || *c == '_')
                                .collect::<String>()
                        ),
                        arguments: vec![],
                        context: sub_intent.extracted_params.clone(),
                        requested_at: SystemTime::now(),
                        attempt_count: 0,
                    };

                    if strat.can_handle(&request) {
                        match strat.resolve(&request, resolution_context).await {
                            Ok(result) => {
                                // Strategy succeeded
                                let method = strat.name().to_string();
                                trace.events.push(TraceEvent::ResolutionCompleted {
                                    intent_id: unresolved_id.clone(),
                                    capability: request.capability_id.clone(),
                                });

                                resolutions.insert(
                                    unresolved_id.clone(),
                                    ResolvedCapability::Local {
                                        capability_id: request.capability_id,
                                        arguments: HashMap::new(),
                                        confidence: 0.9,
                                    },
                                );
                                resolution_methods.insert(unresolved_id.clone(), method);
                                continue;
                            }
                            Err(e) => {
                                // Strategy failed, fall through to default
                                log::debug!(
                                    "Strategy '{}' failed for {}: {:?}",
                                    strat.name(),
                                    unresolved_id,
                                    e
                                );
                            }
                        }
                    }
                }

                // Fallback to simple resolution (same as step_create_fallback_resolutions)
                let fallback = create_simple_fallback(sub_intent);
                resolutions.insert(unresolved_id.clone(), fallback);
                resolution_methods.insert(unresolved_id.clone(), "fallback".to_string());
            }
        }
    }

    StrategyResolutionResult {
        resolutions,
        resolution_methods,
    }
}

/// Create a simple fallback resolution for an intent
fn create_simple_fallback(sub_intent: &SubIntent) -> ResolvedCapability {
    match &sub_intent.intent_type {
        IntentType::DataTransform { .. } | IntentType::Output { .. } => {
            ResolvedCapability::NeedsReferral {
                reason: "No capability found".to_string(),
                suggested_action: format!(
                    "Synth-or-enqueue a capability for: {}",
                    sub_intent.description
                ),
            }
        }
        IntentType::UserInput { .. } => ResolvedCapability::NeedsReferral {
            reason: "User input resolution failed".to_string(),
            suggested_action: "Check ccos.user.ask registration".to_string(),
        },
        IntentType::ApiCall { .. } => {
            let mut args = HashMap::new();
            args.insert(
                "prompt".to_string(),
                format!(
                    "I couldn't find a capability for '{}'. How should I proceed?",
                    sub_intent.description
                ),
            );
            ResolvedCapability::BuiltIn {
                capability_id: "ccos.user.ask".to_string(),
                arguments: args,
            }
        }
        IntentType::Composite => ResolvedCapability::NeedsReferral {
            reason: "Composite intent requires further decomposition".to_string(),
            suggested_action: format!("Break down '{}' into smaller steps", sub_intent.description),
        },
    }
}

/// Step 4.5: Discover new tools for unresolved intents
///
/// When resolution fails, this step searches registries and catalogs
/// for servers/APIs that might provide the missing capabilities.
/// Searches: MCP Registry, local overrides, APIs.guru, and web (if enabled).
pub async fn step_discover_new_tools(
    unresolved_intents: &[SubIntent],
    unresolved_ids: &[String],
    trace: &mut PlanningTrace,
) -> Result<NewToolsDiscoveryResult, PlannerError> {
    if unresolved_intents.is_empty() {
        return Ok(NewToolsDiscoveryResult {
            discovered_servers: vec![],
            candidate_tools: vec![],
            searched_intents: vec![],
        });
    }

    let searcher = RegistrySearcher::new();
    let mut all_results = Vec::new();
    let mut searched_intents = Vec::new();

    for (intent, intent_id) in unresolved_intents.iter().zip(unresolved_ids.iter()) {
        // Extract search terms from intent description
        let search_query = extract_search_query_from_intent(intent);

        if search_query.is_empty() {
            continue;
        }

        searched_intents.push(intent_id.clone());

        // Search all registries
        match searcher.search(&search_query).await {
            Ok(results) => {
                trace.events.push(TraceEvent::DiscoverySearchCompleted {
                    query: search_query.clone(),
                    num_results: results.len(),
                });
                all_results.extend(results);
            }
            Err(e) => {
                log::warn!("Discovery search failed for '{}': {}", search_query, e);
            }
        }
    }

    // Deduplicate by server name
    all_results.sort_by(|a, b| {
        b.match_score
            .partial_cmp(&a.match_score)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    all_results.dedup_by(|a, b| a.server_info.name == b.server_info.name);

    // Convert top results to ToolSummary for potential use in grounded decomposition
    let candidate_tools: Vec<ToolSummary> = all_results
        .iter()
        .take(10) // Limit to top 10 candidates
        .map(|result| ToolSummary {
            id: format!("pending:{}", result.server_info.name),
            name: result.server_info.name.clone(),
            description: result.server_info.description.clone().unwrap_or_default(),
            action: ApiAction::Search, // Default action
            domain: DomainHint::infer_from_text(&result.server_info.name)
                .unwrap_or(DomainHint::Generic),
            input_schema: None, // Will be populated after introspection
        })
        .collect();

    Ok(NewToolsDiscoveryResult {
        discovered_servers: all_results,
        candidate_tools,
        searched_intents,
    })
}

/// Extract a search query from an intent's description
fn extract_search_query_from_intent(intent: &SubIntent) -> String {
    // Extract key domain words from the description
    let description = &intent.description;

    // Remove common action verbs to focus on domain words
    let stop_words = [
        "list", "get", "create", "update", "delete", "show", "find", "search", "fetch", "retrieve",
        "ask", "prompt", "the", "a", "an", "for", "from", "to", "in", "on", "with", "and", "or",
        "but", "user", "me", "my", "their", "all", "any", "some",
    ];

    let lowercased = description.to_lowercase();
    let words: Vec<&str> = lowercased
        .split(|c: char| !c.is_alphanumeric())
        .filter(|w| w.len() > 2 && !stop_words.contains(w))
        .collect();

    // Take up to 3 most relevant words
    words.into_iter().take(3).collect::<Vec<_>>().join(" ")
}

/// Step 5: Archive the generated plan
///
/// Saves the plan to persistent storage with metadata.
pub fn step_archive_plan(
    rtfs_code: &str,
    goal: &str,
    intent_ids: &[String],
    status: PlanStatus,
    config: &PlannerConfig,
) -> Result<Option<ArchiveResult>, PlannerError> {
    if !config.persist_intents {
        return Ok(None);
    }

    let plan_storage_path = crate::utils::fs::default_plan_archive_path();

    if std::fs::create_dir_all(&plan_storage_path).is_err() {
        return Ok(None);
    }

    let archive = match PlanArchive::with_file_storage(plan_storage_path.clone()) {
        Ok(a) => a,
        Err(_) => return Ok(None),
    };

    let mut plan = Plan::new_rtfs(rtfs_code.to_string(), intent_ids.to_vec());
    plan.status = status;
    plan.name = Some(goal.to_string());
    plan.metadata.insert(
        "goal".to_string(),
        rtfs::runtime::values::Value::String(goal.to_string()),
    );

    let plan_id = plan.plan_id.clone();

    match archive.archive_plan(&plan) {
        Ok(hash) => Ok(Some(ArchiveResult {
            plan_id,
            archive_hash: hash,
            archive_path: plan_storage_path,
        })),
        Err(e) => {
            log::warn!("Failed to archive plan: {}", e);
            Ok(None)
        }
    }
}

/// Result of resolution with discovery retry
#[derive(Debug, Clone)]
pub struct ResolutionWithDiscoveryResult {
    /// Final resolutions (from direct resolution or post-discovery)
    pub resolutions: HashMap<String, ResolvedCapability>,
    /// Intents still unresolved after retry
    pub unresolved: Vec<String>,
    /// New tools discovered during the process
    pub discovered_tools: Vec<ToolSummary>,
    /// Whether discovery was triggered
    pub discovery_triggered: bool,
}

/// Step 6: Resolve intents with discovery retry loop
///
/// Orchestrates: resolve → if unresolved → discover_new_tools → re-resolve
/// This allows the planner to dynamically find and use new capabilities.
pub async fn step_resolve_with_discovery(
    sub_intents: &[SubIntent],
    intent_ids: &[String],
    resolution: &dyn ResolutionStrategy,
    max_discovery_rounds: usize,
    trace: &mut PlanningTrace,
) -> Result<ResolutionWithDiscoveryResult, PlannerError> {
    // First attempt: standard resolution
    let mut result = step_resolve_intents(sub_intents, intent_ids, resolution, trace).await?;

    if result.unresolved.is_empty() || max_discovery_rounds == 0 {
        return Ok(ResolutionWithDiscoveryResult {
            resolutions: result.resolutions,
            unresolved: result.unresolved,
            discovered_tools: vec![],
            discovery_triggered: false,
        });
    }

    let mut all_discovered_tools = Vec::new();
    let mut discovery_triggered = false;

    for round in 0..max_discovery_rounds {
        if result.unresolved.is_empty() {
            break;
        }

        // Collect unresolved intents
        let unresolved_intents: Vec<SubIntent> = result
            .unresolved
            .iter()
            .filter_map(|id| {
                intent_ids
                    .iter()
                    .position(|i| i == id)
                    .and_then(|idx| sub_intents.get(idx).cloned())
            })
            .collect();

        if unresolved_intents.is_empty() {
            break;
        }

        log::info!(
            "Discovery round {}: {} unresolved intents",
            round + 1,
            unresolved_intents.len()
        );

        // Discover new tools for unresolved intents
        let discovery =
            step_discover_new_tools(&unresolved_intents, &result.unresolved, trace).await?;

        if discovery.discovered_servers.is_empty() {
            log::debug!("No new servers discovered, stopping retry loop");
            break;
        }

        discovery_triggered = true;
        all_discovered_tools.extend(discovery.candidate_tools.clone());

        // Note: In a full implementation, we would:
        // 1. Register discovered servers with the resolution strategy
        // 2. Re-attempt resolution for unresolved intents
        // For now, we just track what was discovered

        log::info!(
            "Discovered {} potential servers, {} candidate tools",
            discovery.discovered_servers.len(),
            discovery.candidate_tools.len()
        );

        // TODO: Re-resolve after registering discovered capabilities
        // For now, break after first discovery to avoid infinite loop
        break;
    }

    Ok(ResolutionWithDiscoveryResult {
        resolutions: result.resolutions,
        unresolved: result.unresolved,
        discovered_tools: all_discovered_tools,
        discovery_triggered,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_tool_discovery_result_creation() {
        let result = ToolDiscoveryResult {
            tools: vec![],
            domain_hints: vec![],
        };
        assert!(result.tools.is_empty());
    }

    #[test]
    fn test_intent_storage_result_creation() {
        let result = IntentStorageResult {
            root_id: "root-123".to_string(),
            intent_ids: vec!["step-1".to_string(), "step-2".to_string()],
        };
        assert_eq!(result.root_id, "root-123");
        assert_eq!(result.intent_ids.len(), 2);
    }

    #[test]
    fn test_resolution_result_creation() {
        let result = ResolutionResult {
            resolutions: HashMap::new(),
            unresolved: vec!["step-3".to_string()],
        };
        assert!(result.resolutions.is_empty());
        assert_eq!(result.unresolved.len(), 1);
    }
}
