//! Modular Planner Orchestrator
//!
//! The main coordinator that uses decomposition and resolution strategies
//! to convert goals into executable plans, storing all intermediate intents
//! in the IntentGraph.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};
use thiserror::Error;
use uuid::Uuid;

use super::decomposition::{
    DecompositionContext, DecompositionError, DecompositionStrategy, HybridDecomposition,
};
use super::resolution::{
    CompositeResolution, ResolutionContext, ResolutionError, ResolutionStrategy, ResolvedCapability,
};
use super::resolution::semantic::CapabilityCatalog;
use super::types::{IntentType, SubIntent, ToolSummary};
use crate::intent_graph::IntentGraph;
use crate::intent_graph::storage::Edge;
use crate::types::{EdgeType, GenerationContext, IntentStatus, StorableIntent, TriggerSource};

/// Errors that can occur during planning
#[derive(Debug, Error)]
pub enum PlannerError {
    #[error("Decomposition failed: {0}")]
    Decomposition(#[from] DecompositionError),

    #[error("Resolution failed: {0}")]
    Resolution(#[from] ResolutionError),

    #[error("Intent graph error: {0}")]
    IntentGraph(String),

    #[error("Plan generation failed: {0}")]
    PlanGeneration(String),

    #[error("Maximum depth exceeded")]
    MaxDepthExceeded,

    #[error("No strategies available")]
    NoStrategies,
}

/// Configuration for the modular planner
#[derive(Debug, Clone)]
pub struct PlannerConfig {
    /// Maximum recursion depth for composite intents
    pub max_depth: usize,
    /// Whether to store all intents in the graph
    pub persist_intents: bool,
    /// Whether to create edges between intents
    pub create_edges: bool,
    /// Namespace prefix for generated intent IDs
    pub intent_namespace: String,
}

impl Default for PlannerConfig {
    fn default() -> Self {
        Self {
            max_depth: 5,
            persist_intents: true,
            create_edges: true,
            intent_namespace: "plan".to_string(),
        }
    }
}

/// Result of a planning operation
#[derive(Debug)]
pub struct PlanResult {
    /// Root intent ID
    pub root_intent_id: String,
    /// All intent IDs created during planning
    pub intent_ids: Vec<String>,
    /// Resolved capabilities for each intent
    pub resolutions: HashMap<String, ResolvedCapability>,
    /// Generated RTFS plan code
    pub rtfs_plan: String,
    /// Planning trace for debugging
    pub trace: PlanningTrace,
    /// Verification result (if verification was performed)
    pub verification: Option<super::verification::VerificationResult>,
}

/// Trace of planning decisions for debugging/audit
#[derive(Debug, Default)]
pub struct PlanningTrace {
    pub goal: String,
    pub events: Vec<TraceEvent>,
}

#[derive(Debug)]
pub enum TraceEvent {
    DecompositionStarted { strategy: String },
    DecompositionCompleted { num_intents: usize, confidence: f64 },
    IntentCreated { intent_id: String, description: String },
    EdgeCreated { from: String, to: String, edge_type: String },
    ResolutionStarted { intent_id: String },
    ResolutionCompleted { intent_id: String, capability: String },
    ResolutionFailed { intent_id: String, reason: String },
    VerificationStarted,
    VerificationCompleted { verdict: String, issues_count: usize },
}

/// The main modular planner orchestrator.
///
/// Coordinates decomposition strategies and resolution strategies to convert
/// natural language goals into executable RTFS plans, storing all intermediate
/// intents in the IntentGraph.
pub struct ModularPlanner {
    /// Decomposition strategy (how to break goals into sub-intents)
    decomposition: Box<dyn DecompositionStrategy>,
    /// Resolution strategy (how to map intents to capabilities)
    resolution: Box<dyn ResolutionStrategy>,
    /// Capability catalog for listing tools (optional, for grounded decomposition)
    catalog: Option<Arc<dyn CapabilityCatalog>>,
    /// Plan verifier (optional, for consistency checking)
    verifier: Option<Arc<dyn super::verification::PlanVerifier>>,
    /// Intent graph for storing intents and edges
    intent_graph: Arc<Mutex<IntentGraph>>,
    /// Configuration
    config: PlannerConfig,
}

impl ModularPlanner {
    /// Create a new planner with the given strategies
    pub fn new(
        decomposition: Box<dyn DecompositionStrategy>,
        resolution: Box<dyn ResolutionStrategy>,
        intent_graph: Arc<Mutex<IntentGraph>>,
    ) -> Self {
        Self {
            decomposition,
            resolution,
            catalog: None,
            verifier: None,
            intent_graph,
            config: PlannerConfig::default(),
        }
    }

    /// Create with default hybrid decomposition (pattern-only, no LLM)
    pub fn with_patterns(intent_graph: Arc<Mutex<IntentGraph>>) -> Self {
        Self {
            decomposition: Box::new(HybridDecomposition::pattern_only()),
            resolution: Box::new(CompositeResolution::new()),
            catalog: None,
            verifier: None,
            intent_graph,
            config: PlannerConfig::default(),
        }
    }

    pub fn with_catalog(mut self, catalog: Arc<dyn CapabilityCatalog>) -> Self {
        self.catalog = Some(catalog);
        self
    }

    /// Add a plan verifier for consistency checking
    pub fn with_verifier(mut self, verifier: Arc<dyn super::verification::PlanVerifier>) -> Self {
        self.verifier = Some(verifier);
        self
    }

    pub fn with_config(mut self, config: PlannerConfig) -> Self {
        self.config = config;
        self
    }

    /// Plan a goal: decompose → store intents → resolve → generate RTFS
    pub async fn plan(&mut self, goal: &str) -> Result<PlanResult, PlannerError> {
        let mut trace = PlanningTrace {
            goal: goal.to_string(),
            events: vec![],
        };

        // 1. Decompose goal into sub-intents
        trace.events.push(TraceEvent::DecompositionStarted {
            strategy: self.decomposition.name().to_string(),
        });

        let decomp_context = DecompositionContext::new().with_max_depth(self.config.max_depth);

        // Fetch tools if catalog is available (with schemas for grounded decomposition)
        let tools = if let Some(catalog) = &self.catalog {
            let caps = catalog.list_capabilities(None).await;
            Some(caps.into_iter().map(|c| {
                let mut summary = ToolSummary::new(&c.id, &c.description);
                if let Some(schema) = c.input_schema {
                    summary = summary.with_schema(schema);
                }
                summary
            }).collect::<Vec<_>>())
        } else {
            None
        };

        let decomp_result = self.decomposition.decompose(goal, tools.as_deref(), &decomp_context).await?;

        // Post-process: collapse DataTransform intents into preceding ApiCall
        let optimized_intents = self.collapse_transform_intents(decomp_result.sub_intents);

        trace.events.push(TraceEvent::DecompositionCompleted {
            num_intents: optimized_intents.len(),
            confidence: decomp_result.confidence,
        });

        // 2. Store intents in graph and create edges
        let (root_id, intent_ids) = self
            .store_intents_in_graph(goal, &optimized_intents, &mut trace)
            .await?;

        // 3. Resolve each intent to a capability
        let mut resolutions = HashMap::new();
        let resolution_context = ResolutionContext::new();

        for (idx, sub_intent) in optimized_intents.iter().enumerate() {
            let intent_id = &intent_ids[idx];

            trace.events.push(TraceEvent::ResolutionStarted {
                intent_id: intent_id.clone(),
            });

            match self.resolution.resolve(sub_intent, &resolution_context).await {
                Ok(resolved) => {
                    let cap_id = resolved
                        .capability_id()
                        .unwrap_or("unknown")
                        .to_string();
                    trace.events.push(TraceEvent::ResolutionCompleted {
                        intent_id: intent_id.clone(),
                        capability: cap_id,
                    });
                    resolutions.insert(intent_id.clone(), resolved);
                }
                Err(e) => {
                    trace.events.push(TraceEvent::ResolutionFailed {
                        intent_id: intent_id.clone(),
                        reason: e.to_string(),
                    });
                    // Create a NeedsReferral for unresolved intents
                    resolutions.insert(
                        intent_id.clone(),
                        ResolvedCapability::NeedsReferral {
                            reason: e.to_string(),
                            suggested_action: format!(
                                "Could not resolve capability for: {}",
                                sub_intent.description
                            ),
                        },
                    );
                }
            }
        }

        // 4. Generate RTFS plan from resolved intents
        let rtfs_plan = self.generate_rtfs_plan(&optimized_intents, &intent_ids, &resolutions)?;

        // 5. Optional: Verify the plan for consistency
        let verification = if let Some(ref verifier) = self.verifier {
            trace.events.push(TraceEvent::VerificationStarted);
            
            match verifier.verify(goal, &optimized_intents, &resolutions, &rtfs_plan).await {
                Ok(result) => {
                    trace.events.push(TraceEvent::VerificationCompleted {
                        verdict: format!("{:?}", result.verdict),
                        issues_count: result.issues.len(),
                    });
                    Some(result)
                }
                Err(e) => {
                    log::warn!("Plan verification failed: {}", e);
                    None
                }
            }
        } else {
            None
        };

        Ok(PlanResult {
            root_intent_id: root_id,
            intent_ids,
            resolutions,
            rtfs_plan,
            trace,
            verification,
        })
    }

    /// Collapse DataTransform intents into preceding ApiCall intents when possible.
    /// 
    /// This optimization recognizes that filter/sort/paginate operations are typically
    /// API parameters rather than separate client-side processing steps.
    fn collapse_transform_intents(&self, intents: Vec<SubIntent>) -> Vec<SubIntent> {
        use crate::planner::modular_planner::types::TransformType;
        
        if intents.len() <= 1 {
            return intents;
        }
        
        let mut result: Vec<SubIntent> = Vec::new();
        let mut skip_indices: std::collections::HashSet<usize> = std::collections::HashSet::new();
        
        for (i, intent) in intents.iter().enumerate() {
            if skip_indices.contains(&i) {
                continue;
            }
            
            // Check if this is a DataTransform that can be collapsed
            if let IntentType::DataTransform { ref transform } = intent.intent_type {
                // Only collapse filter/sort/paginate - these are typically API params
                let is_collapsible = match transform {
                    TransformType::Filter | TransformType::Sort => true,
                    TransformType::Other(s) => {
                        s.to_lowercase().contains("paginate") || s.to_lowercase().contains("page")
                    }
                    _ => false,
                };
                
                if is_collapsible {
                    // Find the preceding ApiCall this depends on
                    if let Some(&dep_idx) = intent.dependencies.first() {
                        if dep_idx < result.len() {
                            if let IntentType::ApiCall { .. } = result[dep_idx].intent_type {
                                // Merge transform params into the ApiCall
                                let api_intent = &mut result[dep_idx];
                                
                                // Add transform info as params
                                match transform {
                                    TransformType::Filter => {
                                        if let Some(filter_val) = intent.extracted_params.get("filter") {
                                            api_intent.extracted_params.insert("filter".to_string(), filter_val.clone());
                                        }
                                        // Also check for query param
                                        if let Some(query_val) = intent.extracted_params.get("query") {
                                            api_intent.extracted_params.insert("query".to_string(), query_val.clone());
                                        }
                                    }
                                    TransformType::Sort => {
                                        if let Some(sort_val) = intent.extracted_params.get("sort") {
                                            api_intent.extracted_params.insert("sort".to_string(), sort_val.clone());
                                        }
                                    }
                                    TransformType::Other(ref s) if s.to_lowercase().contains("paginate") => {
                                        if let Some(page_val) = intent.extracted_params.get("perPage") {
                                            api_intent.extracted_params.insert("perPage".to_string(), page_val.clone());
                                        }
                                        if let Some(page_val) = intent.extracted_params.get("page") {
                                            api_intent.extracted_params.insert("page".to_string(), page_val.clone());
                                        }
                                    }
                                    _ => {}
                                }
                                
                                // Update description to reflect merged operation
                                api_intent.description = format!(
                                    "{} (with {})",
                                    api_intent.description,
                                    intent.description.to_lowercase()
                                );
                                
                                // Skip this transform intent
                                skip_indices.insert(i);
                                continue;
                            }
                        }
                    }
                }
            }
            
            // Keep the intent as-is
            let mut cloned = intent.clone();
            // Adjust dependencies to account for skipped intents
            cloned.dependencies = intent.dependencies.iter()
                .map(|&d| d - skip_indices.iter().filter(|&&s| s < d).count())
                .collect();
            result.push(cloned);
        }
        
        result
    }

    /// Store sub-intents as StorableIntent nodes in the IntentGraph
    async fn store_intents_in_graph(
        &self,
        goal: &str,
        sub_intents: &[SubIntent],
        trace: &mut PlanningTrace,
    ) -> Result<(String, Vec<String>), PlannerError> {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs();

        // Create root intent for the overall goal
        let root_id = format!("{}:{}", self.config.intent_namespace, Uuid::new_v4());
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

        if self.config.persist_intents {
            let mut graph = self
                .intent_graph
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
            let intent_id = format!("{}:step-{}", self.config.intent_namespace, Uuid::new_v4());
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
                    meta.insert("intent_type".to_string(), format!("{:?}", sub_intent.intent_type));
                    if let Some(ref domain) = sub_intent.domain_hint {
                        meta.insert("domain_hint".to_string(), format!("{:?}", domain));
                    }
                    meta
                },
            };

            if self.config.persist_intents {
                let mut graph = self
                    .intent_graph
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
            if self.config.create_edges {
                let edge = Edge {
                    from: intent_id.clone(),
                    to: root_id.clone(),
                    edge_type: EdgeType::IsSubgoalOf,
                    metadata: None,
                    weight: None,
                };

                let mut graph = self
                    .intent_graph
                    .lock()
                    .map_err(|e| PlannerError::IntentGraph(e.to_string()))?;
                let _ = futures::executor::block_on(graph.storage.store_edge(edge));

                trace.events.push(TraceEvent::EdgeCreated {
                    from: intent_id.clone(),
                    to: root_id.clone(),
                    edge_type: "IsSubgoalOf".to_string(),
                });
            }

            // Create DependsOn edges for dependencies
            if self.config.create_edges {
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

                        let mut graph = self
                            .intent_graph
                            .lock()
                            .map_err(|e| PlannerError::IntentGraph(e.to_string()))?;
                        let _ = futures::executor::block_on(graph.storage.store_edge(edge));

                        trace.events.push(TraceEvent::EdgeCreated {
                            from: intent_id.clone(),
                            to: dep_id.clone(),
                            edge_type: "DependsOn".to_string(),
                        });
                    }
                }
            }
        }

        Ok((root_id, intent_ids))
    }

    /// Generate RTFS plan code from resolved intents
    fn generate_rtfs_plan(
        &self,
        sub_intents: &[SubIntent],
        intent_ids: &[String],
        resolutions: &HashMap<String, ResolvedCapability>,
    ) -> Result<String, PlannerError> {
        if sub_intents.is_empty() {
            return Ok("nil".to_string());
        }

        // Build variable bindings for each step
        let mut bindings: Vec<(String, String)> = Vec::new();

        for (idx, sub_intent) in sub_intents.iter().enumerate() {
            let intent_id = &intent_ids[idx];
            let var_name = format!("step_{}", idx + 1);

            let call_expr = match resolutions.get(intent_id) {
                Some(resolved) => {
                    match resolved {
                        ResolvedCapability::Local { .. } |
                        ResolvedCapability::Remote { .. } |
                        ResolvedCapability::BuiltIn { .. } |
                        ResolvedCapability::Synthesized { .. } => {
                            self.generate_call_expr(resolved, sub_intent, &bindings, sub_intents)
                        }
                        ResolvedCapability::NeedsReferral { suggested_action, .. } => {
                            format!(
                                r#"(call "ccos.user.ask" {{:prompt "Cannot proceed: {}"}})"#,
                                suggested_action.replace('"', "\\\"")
                            )
                        }
                    }
                }
                None => {
                    format!(
                        r#"(call "ccos.user.ask" {{:prompt "No resolution for step: {}"}})"#,
                        sub_intent.description.replace('"', "\\\"")
                    )
                }
            };

            bindings.push((var_name, call_expr));
        }

        // Build nested let expression
        if bindings.len() == 1 {
            return Ok(bindings[0].1.clone());
        }

        let last_var = &bindings[bindings.len() - 1].0;
        let mut expr = last_var.clone();

        for (var, call) in bindings.iter().rev() {
            expr = format!("(let [{} {}]\n  {})", var, call, expr);
        }

        Ok(expr)
    }

    /// Generate a call expression for a capability
    fn generate_call_expr(
        &self,
        resolved_capability: &ResolvedCapability,
        sub_intent: &SubIntent,
        previous_bindings: &[(String, String)],
        all_sub_intents: &[SubIntent],
    ) -> String {
        let capability_id = resolved_capability.capability_id().unwrap_or("unknown");
        let arguments = resolved_capability.arguments().unwrap();

        // Check if this step depends on previous outputs
        let has_dependencies = !sub_intent.dependencies.is_empty();

        let args_map = if has_dependencies {
            // Build args that reference previous step outputs
            let mut args_parts: Vec<String> = Vec::new();
            let mut used_params: std::collections::HashSet<String> = std::collections::HashSet::new();
            
            // Pre-compute param names for ALL dependencies
            let dep_param_names: Vec<(usize, String)> = sub_intent.dependencies.iter()
                .filter_map(|&dep_idx| {
                    if dep_idx < previous_bindings.len() {
                        all_sub_intents.get(dep_idx).map(|dep_intent| {
                            (dep_idx, self.infer_param_name(dep_intent, resolved_capability))
                        })
                    } else {
                        None
                    }
                })
                .collect();
            
            // Add explicit arguments (skip placeholders and params that will come from deps)
            for (key, value) in arguments {
                // Skip placeholder values that indicate "will come from user/previous step"
                let is_placeholder = value == "null" || value == "user_value" || 
                                     value == "user" || value.is_empty() ||
                                     value == "step_1" || value.starts_with("step_");
                
                // Skip if this param will be filled by a dependency
                let is_dep_target = dep_param_names.iter().any(|(_, p)| p == key);
                
                if !is_placeholder && !is_dep_target {
                    args_parts.push(format!(":{} \"{}\"", key, value.replace('"', "\\\"")));
                    used_params.insert(key.clone());
                }
            }
            
            // Add references to ALL previous steps
            // This creates data flow from previous steps to their inferred parameters
            for (dep_idx, param_name) in &dep_param_names {
                // Only add if not already used
                if !used_params.contains(param_name) {
                    let dep_var = &previous_bindings[*dep_idx].0;
                    
                    // Determine coercion type and transform value accordingly
                    let value_expr = match self.get_coercion_type(param_name, resolved_capability) {
                        Some("json") => {
                            // Parse string as JSON (for numbers, booleans)
                            format!("(parse-json {})", dep_var)
                        }
                        Some("array") => {
                            // Wrap single value in array: "rtfs" -> ["rtfs"]
                            // Use vector syntax in RTFS
                            format!("[{}]", dep_var)
                        }
                        Some("array_upper") => {
                            // Wrap in array AND uppercase (for GraphQL enums like IssueState)
                            // "open" -> ["OPEN"]
                            format!("[(string-upper {})]", dep_var)
                        }
                        Some("enum_upper") => {
                            // Uppercase for GraphQL enum (but not array)
                            // "open" -> "OPEN"
                            format!("(string-upper {})", dep_var)
                        }
                        _ => dep_var.clone(),
                    };
                    
                    args_parts.push(format!(":{} {}", param_name, value_expr));
                    used_params.insert(param_name.clone());
                }
            }
            
            if args_parts.is_empty() {
                "{}".to_string()
            } else {
                format!("{{{}}}", args_parts.join(" "))
            }
        } else {
            // Simple args map
            if arguments.is_empty() {
                "{}".to_string()
            } else {
                let parts: Vec<String> = arguments
                    .iter()
                    .map(|(k, v)| format!(":{} \"{}\"", k, v.replace('"', "\\\"")))
                    .collect();
                format!("{{{}}}", parts.join(" "))
            }
        };

        format!(r#"(call "{}" {})"#, capability_id, args_map)
    }

    /// Helper to infer a parameter name from a dependency intent and consumer capability schema
    fn infer_param_name(&self, producer_intent: &SubIntent, consumer_capability: &ResolvedCapability) -> String {
        // 1. Try schema-based matching if available
        let schema = match consumer_capability {
            ResolvedCapability::Remote { input_schema: Some(schema), .. } => Some(schema),
            ResolvedCapability::Local { input_schema: Some(schema), .. } => Some(schema),
            _ => None,
        };
        
        if let Some(schema) = schema {
            if let Some(best_match) = self.match_topic_to_schema(producer_intent, schema) {
                return best_match;
            }
        }

        // 2. Fallback: extract first meaningful word from prompt topic
        // This prevents invalid parameter names with special characters
        match &producer_intent.intent_type {
            IntentType::UserInput { prompt_topic } => {
                let normalized = prompt_topic.trim().to_lowercase();
                
                // Common stop words to skip
                let stop_words = ["the", "for", "and", "please", "provide", "etc", "with", "from", "your", "enter"];
                
                // Extract meaningful words (alphanumeric only, length > 2)
                let words: Vec<&str> = normalized
                    .split(|c: char| !c.is_alphanumeric())
                    .filter(|w| w.len() > 2 && !stop_words.contains(w))
                    .collect();
                
                // Return first meaningful word, or fallback
                words.first()
                    .map(|w| w.to_string())
                    .unwrap_or_else(|| "_input".to_string())
            }
            _ => "_previous_result".to_string(),
        }
    }

    /// Determine what kind of coercion is needed for a parameter
    /// Returns: None (no coercion), Some("json") for numbers/bools, 
    /// Some("array") for arrays, Some("array_upper") for enum arrays needing uppercase,
    /// Some("enum_upper") for string enums needing uppercase
    fn get_coercion_type(&self, param_name: &str, capability: &ResolvedCapability) -> Option<&'static str> {
        let schema = match capability {
            ResolvedCapability::Remote { input_schema: Some(schema), .. } => Some(schema),
            ResolvedCapability::Local { input_schema: Some(schema), .. } => Some(schema),
            _ => None,
        };
        
        if let Some(schema) = schema {
            if let Some(props) = schema.get("properties").and_then(|p| p.as_object()) {
                // Try exact match first
                if let Some(prop_def) = props.get(param_name) {
                    let type_val = prop_def.get("type").and_then(|t| t.as_str());
                    let has_enum = prop_def.get("enum").is_some();
                    
                    // Check if enum values are uppercase (indicates GraphQL enum)
                    let is_uppercase_enum = if let Some(enum_arr) = prop_def.get("enum").and_then(|e| e.as_array()) {
                        enum_arr.iter()
                            .filter_map(|v| v.as_str())
                            .any(|s| s.chars().all(|c| c.is_uppercase() || !c.is_alphabetic()))
                    } else {
                        false
                    };
                    
                    return match type_val {
                        Some("integer") | Some("number") | Some("boolean") => Some("json"),
                        Some("array") => {
                            // Check if array items need uppercasing
                            let needs_uppercase = is_uppercase_enum || matches!(
                                param_name.to_lowercase().as_str(),
                                "state" | "states" | "direction"
                            );
                            if needs_uppercase {
                                Some("array_upper")
                            } else {
                                Some("array")
                            }
                        }
                        Some("string") if is_uppercase_enum => {
                            // String with uppercase enum values - need to uppercase user input
                            Some("enum_upper")
                        }
                        _ => None,
                    };
                }
            }
        }
        None
    }
    
    /// Check if a parameter should be coerced from string to JSON value (number/bool)
    /// Legacy helper - delegates to get_coercion_type
    fn should_coerce(&self, param_name: &str, capability: &ResolvedCapability) -> bool {
        self.get_coercion_type(param_name, capability) == Some("json")
    }

    /// Match intent topic to schema properties using fuzzy matching
    fn match_topic_to_schema(&self, intent: &SubIntent, schema: &serde_json::Value) -> Option<String> {
        let topic = match &intent.intent_type {
            IntentType::UserInput { prompt_topic } => prompt_topic.to_lowercase(),
            _ => return None,
        };

        // Get properties from JSON schema
        let properties = schema.get("properties")?.as_object()?;
        
        let mut best_match = None;
        let mut best_score = 0.0;

        for (prop_name, prop_def) in properties {
            let prop_lower = prop_name.to_lowercase();
            let mut score = 0.0;

            // 1. Exact match
            if prop_lower == topic {
                return Some(prop_name.clone());
            }

            // 2. Contains match (e.g. "page size" contains "page")
            if topic.contains(&prop_lower) || prop_lower.contains(&topic) {
                score += 0.6;
            }

            // 3. Token overlap
            let topic_tokens: Vec<&str> = topic.split_whitespace().collect();
            
            // Split property by underscore and camelCase
            let mut prop_tokens = Vec::new();
            let mut current_token = String::new();
            for c in prop_name.chars() {
                if c == '_' {
                    if !current_token.is_empty() {
                        prop_tokens.push(current_token.to_lowercase());
                        current_token = String::new();
                    }
                } else if c.is_uppercase() {
                    if !current_token.is_empty() {
                        prop_tokens.push(current_token.to_lowercase());
                    }
                    current_token = String::new();
                    current_token.push(c);
                } else {
                    current_token.push(c);
                }
            }
            if !current_token.is_empty() {
                prop_tokens.push(current_token.to_lowercase());
            }
            
            let matches = topic_tokens.iter().filter(|t| 
                prop_tokens.iter().any(|pt| pt == *t)
            ).count();

            if matches > 0 {
                score += 0.6 * (matches as f64);
            }

            // 4. Description match (if available in schema)
            if let Some(desc) = prop_def.get("description").and_then(|d| d.as_str()) {
                let desc_lower = desc.to_lowercase();
                if desc_lower.contains(&topic) {
                    score += 0.5;
                }
            }

            if score > best_score && score > 0.5 {
                best_score = score;
                best_match = Some(prop_name.clone());
            }
        }

        best_match
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::intent_graph::config::IntentGraphConfig;
    use crate::planner::modular_planner::decomposition::PatternDecomposition;
    use crate::planner::modular_planner::resolution::CatalogResolution;
    use crate::planner::modular_planner::resolution::semantic::CapabilityCatalog;
    use crate::planner::modular_planner::resolution::semantic::CapabilityInfo;

    struct MockCatalog;

    #[async_trait::async_trait(?Send)]
    impl CapabilityCatalog for MockCatalog {
        async fn list_capabilities(&self, _domain: Option<&str>) -> Vec<CapabilityInfo> {
            vec![]
        }

        async fn get_capability(&self, _id: &str) -> Option<CapabilityInfo> {
            None
        }

        async fn search(&self, _query: &str, _limit: usize) -> Vec<CapabilityInfo> {
            vec![]
        }
    }

    #[tokio::test]
    async fn test_modular_planner_pattern_decomposition() {
        let intent_graph = Arc::new(Mutex::new(
            IntentGraph::with_config(IntentGraphConfig::with_in_memory_storage()).unwrap(),
        ));

        let catalog = Arc::new(MockCatalog);

        let mut planner = ModularPlanner::new(
            Box::new(PatternDecomposition::new()),
            Box::new(CatalogResolution::new(catalog)),
            intent_graph.clone(),
        );

        let result = planner
            .plan("list issues but ask me for page size")
            .await
            .expect("Should plan");

        // Should create root + 2 sub-intents
        assert_eq!(result.intent_ids.len(), 2);

        // First should be user input (resolved to builtin)
        let first_resolution = &result.resolutions[&result.intent_ids[0]];
        assert!(matches!(first_resolution, ResolvedCapability::BuiltIn { .. }));

        // Should have generated RTFS
        assert!(result.rtfs_plan.contains("call"));
        assert!(result.rtfs_plan.contains("ccos.user.ask"));

        // Check intents were stored in graph
        let graph = intent_graph.lock().unwrap();
        let root = graph.get_intent(&result.root_intent_id);
        assert!(root.is_some());
    }

    #[test]
    fn test_generate_call_expr() {
        let intent_graph = Arc::new(Mutex::new(
            IntentGraph::with_config(IntentGraphConfig::with_in_memory_storage()).unwrap(),
        ));
        let catalog = Arc::new(MockCatalog);
        let planner = ModularPlanner::new(
            Box::new(PatternDecomposition::new()),
            Box::new(CatalogResolution::new(catalog)),
            intent_graph,
        );

        let mut args = HashMap::new();
        args.insert("owner".to_string(), "mandubian".to_string());
        args.insert("repo".to_string(), "ccos".to_string());

        let sub_intent = SubIntent::new("test", IntentType::ApiCall { 
            action: crate::planner::modular_planner::types::ApiAction::List 
        });

        let resolved = ResolvedCapability::Remote {
            capability_id: "mcp.github.list_issues".to_string(),
            server_url: "http://test".to_string(),
            arguments: args,
            input_schema: None,
            confidence: 1.0,
        };

        let expr = planner.generate_call_expr(
            &resolved,
            &sub_intent,
            &[],
            &[sub_intent.clone()],
        );

        assert!(expr.contains("mcp.github.list_issues"));
        assert!(expr.contains(":owner"));
        assert!(expr.contains("mandubian"));
    }
}

