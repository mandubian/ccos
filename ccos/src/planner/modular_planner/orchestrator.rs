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
use super::types::{IntentType, SubIntent};
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
            intent_graph,
            config: PlannerConfig::default(),
        }
    }

    /// Create with default hybrid decomposition (pattern-only, no LLM)
    pub fn with_patterns(intent_graph: Arc<Mutex<IntentGraph>>) -> Self {
        Self {
            decomposition: Box::new(HybridDecomposition::pattern_only()),
            resolution: Box::new(CompositeResolution::new()),
            intent_graph,
            config: PlannerConfig::default(),
        }
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

        let decomp_result = self.decomposition.decompose(goal, None, &decomp_context).await?;

        trace.events.push(TraceEvent::DecompositionCompleted {
            num_intents: decomp_result.sub_intents.len(),
            confidence: decomp_result.confidence,
        });

        // 2. Store intents in graph and create edges
        let (root_id, intent_ids) = self
            .store_intents_in_graph(goal, &decomp_result.sub_intents, &mut trace)
            .await?;

        // 3. Resolve each intent to a capability
        let mut resolutions = HashMap::new();
        let resolution_context = ResolutionContext::new();

        for (idx, sub_intent) in decomp_result.sub_intents.iter().enumerate() {
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
        let rtfs_plan = self.generate_rtfs_plan(&decomp_result.sub_intents, &intent_ids, &resolutions)?;

        Ok(PlanResult {
            root_intent_id: root_id,
            intent_ids,
            resolutions,
            rtfs_plan,
            trace,
        })
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
                Some(ResolvedCapability::Local { capability_id, arguments, .. }) |
                Some(ResolvedCapability::Remote { capability_id, arguments, .. }) => {
                    self.generate_call_expr(capability_id, arguments, sub_intent, &bindings)
                }
                Some(ResolvedCapability::BuiltIn { capability_id, arguments }) => {
                    self.generate_call_expr(capability_id, arguments, sub_intent, &bindings)
                }
                Some(ResolvedCapability::Synthesized { capability_id, arguments, .. }) => {
                    self.generate_call_expr(capability_id, arguments, sub_intent, &bindings)
                }
                Some(ResolvedCapability::NeedsReferral { reason, .. }) => {
                    format!(
                        r#"(call "ccos.user.ask" {{:prompt "Cannot proceed: {}"}})"#,
                        reason.replace('"', "\\\"")
                    )
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
        capability_id: &str,
        arguments: &HashMap<String, String>,
        sub_intent: &SubIntent,
        previous_bindings: &[(String, String)],
    ) -> String {
        // Check if this step depends on previous outputs
        let has_dependencies = !sub_intent.dependencies.is_empty();

        let args_map = if has_dependencies && !sub_intent.dependencies.is_empty() {
            // Build args that reference previous step outputs
            let mut args_parts: Vec<String> = Vec::new();
            
            // Add explicit arguments
            for (key, value) in arguments {
                args_parts.push(format!(":{} \"{}\"", key, value.replace('"', "\\\"")));
            }
            
            // Add reference to previous step if not already in args
            // This creates data flow from previous steps
            if sub_intent.dependencies.len() == 1 {
                let dep_idx = sub_intent.dependencies[0];
                if dep_idx < previous_bindings.len() {
                    let dep_var = &previous_bindings[dep_idx].0;
                    // Only add if we don't already have this as an argument
                    if !arguments.values().any(|v| v == dep_var) {
                        args_parts.push(format!(":_previous_result {}", dep_var));
                    }
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

        let expr = planner.generate_call_expr(
            "mcp.github.list_issues",
            &args,
            &sub_intent,
            &[],
        );

        assert!(expr.contains("mcp.github.list_issues"));
        assert!(expr.contains(":owner"));
        assert!(expr.contains("mandubian"));
    }
}
