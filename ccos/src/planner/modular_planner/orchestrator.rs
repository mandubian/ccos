//! Modular Planner Orchestrator
//!
//! The main coordinator that uses decomposition and resolution strategies
//! to convert goals into executable plans, storing all intermediate intents
//! in the IntentGraph.

use regex::Regex;
use std::collections::{HashMap, HashSet, VecDeque};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};
use thiserror::Error;
use uuid::Uuid;

use crate::planner::adapters::SchemaBridge;

use super::decomposition::hybrid::HybridConfig;
use super::decomposition::{
    DecompositionContext, DecompositionError, DecompositionStrategy, HybridDecomposition,
};
use super::resolution::{
    CompositeResolution, ResolutionContext, ResolutionError, ResolutionStrategy, ResolvedCapability,
};
use super::safe_executor::SafeCapabilityExecutor;
use super::types::ToolSummary;
use super::types::{ApiAction, DomainHint, IntentType, SubIntent};
use crate::capability_marketplace::CapabilityMarketplace;
use crate::intent_graph::storage::Edge;
use crate::intent_graph::IntentGraph;
use crate::plan_archive::PlanArchive;
use crate::synthesis::enqueue_missing_capability_placeholder;
use crate::synthesis::missing_capability_resolver::{
    MissingCapabilityRequest, MissingCapabilityResolver, ResolutionResult,
};
use crate::types::{
    EdgeType, GenerationContext, IntentStatus, Plan, PlanStatus, StorableIntent, TriggerSource,
};
use crate::utils::value_conversion::rtfs_value_to_json;

const GROUNDING_VALUE_LIMIT: usize = 800;

fn truncate_grounding(value: &str) -> String {
    let mut truncated: String = value.chars().take(GROUNDING_VALUE_LIMIT).collect();
    if truncated.len() < value.len() {
        truncated.push_str("... [truncated]");
    }
    truncated
}

fn value_preview(value: &rtfs::runtime::values::Value) -> String {
    if let Ok(json) = rtfs_value_to_json(value) {
        if let Ok(s) = serde_json::to_string(&json) {
            return truncate_grounding(&s);
        }
    }
    truncate_grounding(&format!("{:?}", value))
}

fn grounding_preview(value: &rtfs::runtime::values::Value) -> String {
    if let Ok(json) = rtfs_value_to_json(value) {
        // If it's an array of objects, surface schema + first 2 rows
        if let Some(arr) = json.as_array() {
            let mut schema: Vec<String> = vec![];
            let mut rows: Vec<serde_json::Value> = vec![];
            for item in arr.iter().take(2) {
                if let Some(obj) = item.as_object() {
                    for k in obj.keys() {
                        if !schema.contains(k) {
                            schema.push(k.clone());
                        }
                    }
                }
                rows.push(item.clone());
            }
            let preview = serde_json::json!({
                "schema": schema,
                "rows": rows,
            });
            if let Ok(s) = serde_json::to_string(&preview) {
                return s;
            }
        }

        // If it's a map/object, show keys + first 2 entries
        if let Some(obj) = json.as_object() {
            let keys: Vec<String> = obj.keys().cloned().collect();
            let rows: Vec<serde_json::Value> = obj
                .iter()
                .take(2)
                .map(|(k, v)| serde_json::json!({ k: v }))
                .collect();
            let preview = serde_json::json!({
                "keys": keys,
                "sample": rows,
            });
            if let Ok(s) = serde_json::to_string(&preview) {
                return s;
            }
        }
    }

    value_preview(value)
}

fn slugify_description(desc: &str) -> String {
    let mut slug = String::with_capacity(desc.len());
    let mut last_dash = false;
    for c in desc.to_lowercase().chars() {
        if c.is_ascii_alphanumeric() {
            slug.push(c);
            last_dash = false;
        } else if !last_dash {
            slug.push('-');
            last_dash = true;
        }
    }
    let slug = slug.trim_matches('-').to_string();
    if slug.is_empty() {
        "generated".to_string()
    } else {
        slug
    }
}

fn fnv1a64(s: &str) -> u64 {
    const OFFSET_BASIS: u64 = 0xcbf29ce484222325;
    const FNV_PRIME: u64 = 0x100000001b3;
    let mut hash = OFFSET_BASIS;
    for b in s.as_bytes() {
        hash ^= *b as u64;
        hash = hash.wrapping_mul(FNV_PRIME);
    }
    hash
}

fn generated_capability_id_from_description(desc: &str) -> String {
    let slug = slugify_description(desc);
    let hash = fnv1a64(desc);
    format!("generated/{}-{:016x}", slug, hash)
}

// Inline RTFS synthesis has been removed. Missing capabilities are now resolved
// exclusively via the MissingCapabilityResolver to ensure a single source of truth
// (LLM + persistence + logging).

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
    /// Whether to show verbose LLM prompts/responses
    pub verbose_llm: bool,
    /// Whether to show just the LLM prompt
    pub show_prompt: bool,
    /// Whether to confirm before each LLM call
    pub confirm_llm: bool,
    /// Whether to eagerly discover tools before decomposition
    pub eager_discovery: bool,
    /// Whether to execute low-risk capabilities during planning to ground prompts
    pub enable_safe_exec: bool,
    /// Optional seed grounding parameters to feed into decomposition prompts
    pub initial_grounding_params: HashMap<String, String>,
    /// Whether grounded snippets should be pushed into runtime context for prompts
    pub allow_grounding_context: bool,
    /// Optional hybrid strategy configuration
    pub hybrid_config: Option<HybridConfig>,
    /// Whether to enable LLM-based schema validation after refinement
    pub enable_schema_validation: bool,
    /// Whether to enable LLM-based plan validation after generation
    pub enable_plan_validation: bool,
}

impl Default for PlannerConfig {
    fn default() -> Self {
        Self {
            max_depth: 5,
            persist_intents: true,
            create_edges: true,
            intent_namespace: "plan".to_string(),
            verbose_llm: false,
            show_prompt: false,
            confirm_llm: false,
            eager_discovery: true,
            enable_safe_exec: false,
            initial_grounding_params: HashMap::new(),
            allow_grounding_context: true,
            hybrid_config: Some(HybridConfig::default()),
            enable_schema_validation: false, // Disabled by default
            enable_plan_validation: false,   // Disabled by default
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
    /// Sub-intents with full details (description, params, domain hints)
    pub sub_intents: Vec<SubIntent>,
    /// Resolved capabilities for each intent
    pub resolutions: HashMap<String, ResolvedCapability>,
    /// Generated RTFS plan code
    pub rtfs_plan: String,
    /// Planning trace for debugging
    pub trace: PlanningTrace,
    /// Optional plan_id assigned when the plan is archived
    pub plan_id: Option<String>,
    /// Optional content-addressable hash returned by the archive
    pub archive_hash: Option<String>,
    /// Optional path to the archive directory used for persistence
    pub archive_path: Option<PathBuf>,
}

/// Trace of planning decisions for debugging/audit
#[derive(Debug, Default)]
pub struct PlanningTrace {
    pub goal: String,
    pub events: Vec<TraceEvent>,
}

#[derive(Debug)]
pub enum TraceEvent {
    DecompositionStarted {
        strategy: String,
    },
    DecompositionCompleted {
        num_intents: usize,
        confidence: f64,
    },
    IntentCreated {
        intent_id: String,
        description: String,
    },
    EdgeCreated {
        from: String,
        to: String,
        edge_type: String,
    },
    ResolutionStarted {
        intent_id: String,
    },
    ResolutionCompleted {
        intent_id: String,
        capability: String,
    },
    ResolutionFailed {
        intent_id: String,
        reason: String,
    },
    /// LLM call made during planning/resolution
    LlmCalled {
        model: String,
        prompt: String,
        response: Option<String>,
        tokens_prompt: usize,
        tokens_response: usize,
        duration_ms: u64,
    },
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
    /// Optional safe executor for grounding (read-only/network)
    safe_executor: Option<SafeCapabilityExecutor>,
    /// Optional missing capability resolver for immediate synthesis
    missing_capability_resolver: Option<Arc<MissingCapabilityResolver>>,
    /// Optional callback for real-time trace event streaming
    trace_callback: Option<Box<dyn Fn(&TraceEvent) + Send + Sync>>,
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
            safe_executor: None,
            missing_capability_resolver: None,
            trace_callback: None,
        }
    }

    /// Create with default hybrid decomposition (pattern-only, no LLM)
    pub fn with_patterns(intent_graph: Arc<Mutex<IntentGraph>>) -> Self {
        Self {
            decomposition: Box::new(HybridDecomposition::pattern_only()),
            resolution: Box::new(CompositeResolution::new()),
            intent_graph,
            config: PlannerConfig::default(),
            safe_executor: None,
            missing_capability_resolver: None,
            trace_callback: None,
        }
    }

    pub fn with_config(mut self, config: PlannerConfig) -> Self {
        self.config = config;
        self
    }

    /// Set a callback for real-time trace event streaming.
    /// The callback is called whenever a trace event is emitted during planning.
    pub fn with_trace_callback<F>(mut self, callback: F) -> Self
    where
        F: Fn(&TraceEvent) + Send + Sync + 'static,
    {
        self.trace_callback = Some(Box::new(callback));
        self
    }

    /// Inject a missing capability resolver so planner can resolve immediately.
    pub fn with_missing_capability_resolver(
        mut self,
        resolver: Arc<MissingCapabilityResolver>,
    ) -> Self {
        self.missing_capability_resolver = Some(resolver);
        self
    }

    /// Enable safe execution using the provided marketplace
    pub fn with_safe_executor(mut self, marketplace: Arc<CapabilityMarketplace>) -> Self {
        self.safe_executor = Some(SafeCapabilityExecutor::new(marketplace));
        self
    }

    /// Emit a trace event - pushes to the Vec AND calls the callback if set.
    /// This enables real-time streaming of trace events to the TUI.
    fn emit_trace(&self, trace: &mut PlanningTrace, event: TraceEvent) {
        // Call the callback first (for real-time streaming)
        if let Some(ref callback) = self.trace_callback {
            callback(&event);
        }
        // Then push to the Vec (for the final result)
        trace.events.push(event);
    }

    /// Create a fallback resolution when no capability is found.
    ///
    /// This can either:
    /// 1. Ask the user for guidance (if the intent seems user-actionable)
    /// 2. Mark as NeedsReferral for arbiter escalation
    fn create_fallback_resolution(
        &self,
        sub_intent: &SubIntent,
        error: &ResolutionError,
    ) -> ResolvedCapability {
        use crate::planner::modular_planner::types::IntentType;

        // If the intent is about data transformation or output, we might synthesize it
        match &sub_intent.intent_type {
            IntentType::DataTransform { .. } | IntentType::Output { .. } => {
                // Prefer synth-or-enqueue for data/output intents; avoid prompting the user.
                log::info!(
                    "Queuing missing data/output capability for synth/enqueue: {}",
                    sub_intent.description
                );
                ResolvedCapability::NeedsReferral {
                    reason: format!("No capability found: {}", error),
                    suggested_action: format!(
                        "Synth-or-enqueue a capability for: {}",
                        sub_intent.description
                    ),
                }
            }
            IntentType::UserInput { .. } => {
                // User input should always resolve to builtin, something is wrong
                ResolvedCapability::NeedsReferral {
                    reason: format!("User input resolution failed: {}", error),
                    suggested_action: "Check ccos.user.ask registration".to_string(),
                }
            }
            IntentType::ApiCall { .. } => {
                // API calls that don't resolve -> ask user for guidance
                let mut args = std::collections::HashMap::new();
                args.insert(
                    "prompt".to_string(),
                    format!(
                        "I couldn't find a capability for '{}'. How should I proceed?\n\
                         Options:\n\
                         1. Skip this step\n\
                         2. Provide an alternative approach\n\
                         3. Abort the plan",
                        sub_intent.description
                    ),
                );

                ResolvedCapability::BuiltIn {
                    capability_id: "ccos.user.ask".to_string(),
                    arguments: args,
                }
            }
            IntentType::Composite => {
                // Composite intents need further decomposition
                ResolvedCapability::NeedsReferral {
                    reason: "Composite intent requires further decomposition".to_string(),
                    suggested_action: format!(
                        "Break down '{}' into smaller steps",
                        sub_intent.description
                    ),
                }
            }
        }
    }

    /// Plan a goal: decompose → store intents → resolve → generate RTFS
    pub async fn plan(&mut self, goal: &str) -> Result<PlanResult, PlannerError> {
        // Optional: early exit if max depth is zero
        if self.config.max_depth == 0 {
            return Err(
                ResolutionError::NotFound("Max depth is zero; cannot plan".to_string()).into(),
            );
        }

        let mut trace = PlanningTrace {
            goal: goal.to_string(),
            events: vec![],
        };

        // 0. Eager tool discovery (if enabled)
        let available_tools: Option<Vec<ToolSummary>> = if self.config.eager_discovery {
            ccos_println!("\n📦 Discovering available tools...");

            // Infer domain hints to restrict search
            let domain_hints = DomainHint::infer_all_from_text(goal);
            if !domain_hints.is_empty() {
                ccos_println!("   🎯 Inferred domains: {:?}", domain_hints);
            }

            let mut tools = self
                .resolution
                .list_available_tools(Some(&domain_hints))
                .await;

            // Heuristic: surface likely-useful transformers/formatters and canonical CRUD/search tools first.
            // This keeps the top-N hint list meaningful for the LLM.
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

            // Stable sort by rank desc to preserve original discovery order within ranks.
            tools.sort_by(|a, b| tool_rank(b).cmp(&tool_rank(a)));

            if !tools.is_empty() {
                ccos_println!(
                    "   ✅ Found {} tools for grounded decomposition",
                    tools.len()
                );
                Some(tools)
            } else {
                ccos_println!("   ⚠️ No tools discovered, using abstract decomposition");
                None
            }
        } else {
            None
        };

        // 1. Decompose goal into sub-intents
        trace.events.push(TraceEvent::DecompositionStarted {
            strategy: self.decomposition.name().to_string(),
        });

        let mut grounding_params: HashMap<String, String> =
            self.config.initial_grounding_params.clone();

        let mut decomp_context = DecompositionContext::new()
            .with_max_depth(self.config.max_depth)
            .with_verbose_llm(self.config.verbose_llm)
            .with_show_prompt(self.config.show_prompt)
            .with_confirm_llm(self.config.confirm_llm);
        for (k, v) in grounding_params.iter() {
            decomp_context
                .pre_extracted_params
                .insert(k.clone(), v.clone());
        }

        let tools_slice = available_tools.as_ref().map(|v| v.as_slice());

        let decomp_result = self
            .decomposition
            .decompose(goal, tools_slice, &decomp_context)
            .await?;

        trace.events.push(TraceEvent::DecompositionCompleted {
            num_intents: decomp_result.sub_intents.len(),
            confidence: decomp_result.confidence,
        });

        ccos_println!(
            "🧭 Planner decomposition done: {} sub-intents (safe_exec enabled={}, executor={})",
            decomp_result.sub_intents.len(),
            self.config.enable_safe_exec,
            self.safe_executor.is_some()
        );

        // 2. Store intents in graph and create edges
        let (root_id, mut intent_ids) = self
            .store_intents_in_graph(goal, &decomp_result.sub_intents, &mut trace)
            .await?;

        // Print Intent Graph
        ccos_println!(
            "\n╭──────────────────────────────────────────────────────────────────────────────"
        );
        ccos_println!("│ 🧭 Intent Graph");
        ccos_println!(
            "╰──────────────────────────────────────────────────────────────────────────────"
        );
        ccos_println!("ROOT: {}", goal);
        for (i, sub_intent) in decomp_result.sub_intents.iter().enumerate() {
            let id = intent_ids.get(i).map(|s| s.as_str()).unwrap_or("?");
            ccos_println!("  ├─ [{}] {}", i, sub_intent.description);
            ccos_println!("  │    ID: {}", id);
            ccos_println!("  │    Type: {:?}", sub_intent.intent_type);
            ccos_println!("  │    Dependencies: {:?}", sub_intent.dependencies);
            if !sub_intent.extracted_params.is_empty() {
                ccos_println!("  │    Params: {:?}", sub_intent.extracted_params);
            }
        }
        ccos_println!("");

        // 3. Resolve each intent to a capability (without execution)
        let mut resolutions = HashMap::new();
        let resolution_context = ResolutionContext::new();

        // Track which intents have already been refined to avoid infinite refinement loops
        let mut refined_attempts: HashSet<String> = HashSet::new();

        let goal_snapshot = goal.to_string();

        // Working queue for intents to resolve/refine (stack order preserved)
        let mut intent_queue: Vec<(usize, SubIntent)> = decomp_result
            .sub_intents
            .iter()
            .cloned()
            .enumerate()
            .collect();

        while let Some((idx, sub_intent)) = intent_queue.pop() {
            let intent_id = &intent_ids[idx];

            trace.events.push(TraceEvent::ResolutionStarted {
                intent_id: intent_id.clone(),
            });

            match self
                .resolution
                .resolve(&sub_intent, &resolution_context)
                .await
            {
                Ok(resolved) => {
                    let cap_id = resolved.capability_id().unwrap_or("unknown").to_string();
                    trace.events.push(TraceEvent::ResolutionCompleted {
                        intent_id: intent_id.clone(),
                        capability: cap_id.clone(),
                    });
                    resolutions.insert(intent_id.clone(), resolved.clone());
                }
                Err(e) => {
                    // If we still have depth budget and haven't already refined this intent, try a focused refinement.
                    let current_depth = self.config.max_depth.saturating_sub(1);
                    if current_depth > 0 && !refined_attempts.contains(intent_id) {
                        // Skip refinement for simple non-composite intents
                        let is_simple = matches!(
                            sub_intent.intent_type,
                            IntentType::ApiCall { .. }
                                | IntentType::DataTransform { .. }
                                | IntentType::Output { .. }
                        );
                        let desc_lc = sub_intent.description.to_lowercase();
                        let goal_lc = goal_snapshot.to_lowercase();
                        let is_same_as_goal = desc_lc == goal_lc || goal_lc.contains(&desc_lc);
                        let has_params = !sub_intent.extracted_params.is_empty();
                        if !is_simple || (!has_params && !is_same_as_goal) {
                            refined_attempts.insert(intent_id.clone());

                            // Collect sibling intent descriptions for context
                            let sibling_descriptions: Vec<String> = decomp_result
                                .sub_intents
                                .iter()
                                .map(|s| s.description.clone())
                                .collect();

                            let refine_ctx = DecompositionContext::new()
                                .with_max_depth(current_depth)
                                .with_verbose_llm(self.config.verbose_llm)
                                .with_show_prompt(self.config.show_prompt)
                                .with_confirm_llm(self.config.confirm_llm)
                                .with_parent_intent(goal_snapshot.clone())
                                .with_siblings(sibling_descriptions)
                                .with_data_sources(sub_intent.dependencies.clone());

                            let mut refine_ctx = refine_ctx;
                            for (k, v) in grounding_params.iter() {
                                refine_ctx.pre_extracted_params.insert(k.clone(), v.clone());
                            }

                            // CRITICAL: Use ONLY the sub-intent description as the goal.
                            // Do NOT include the original goal - that causes the LLM to re-plan everything.
                            // The sibling context (set via DecompositionContext) tells the LLM what's already done.
                            let refine_goal = sub_intent.description.clone();

                            // CRITICAL: Pass tools_slice so refinement can see available tools
                            match self
                                .decomposition
                                .decompose(&refine_goal, tools_slice, &refine_ctx)
                                .await
                            {
                                Ok(refine_result) if !refine_result.sub_intents.is_empty() => {
                                    // Store refined intents in graph, link as children, and enqueue them
                                    let (refined_ids, _refined_subs) = self
                                        .store_refined_intents(
                                            intent_id,
                                            &sub_intent,
                                            &refine_result.sub_intents,
                                            &mut trace,
                                        )
                                        .await?;

                                    // Enqueue for resolution
                                    for (rid, rsub) in refined_ids
                                        .into_iter()
                                        .zip(refine_result.sub_intents.into_iter())
                                    {
                                        // maintain mapping in intent_ids/resolutions
                                        intent_ids.push(rid.clone());
                                        intent_queue.push((intent_ids.len() - 1, rsub.clone()));
                                        // Placeholder to preserve order in resolutions map
                                        resolutions.insert(rid, ResolvedCapability::NeedsReferral {
                                            reason: "Pending refinement resolution".to_string(),
                                            suggested_action: "Continue resolving refined intents".to_string(),
                                        });
                                    }
                                    continue;
                                }
                                _ => {
                                    // Fall through to fallback if refinement failed
                                }
                            }
                        }
                    }

                    // Refinement not possible or failed: fallback
                    trace.events.push(TraceEvent::ResolutionFailed {
                        intent_id: intent_id.clone(),
                        reason: e.to_string(),
                    });

                    // Last resort: enqueue a placeholder for synthesis for data/output intents
                    if matches!(
                        sub_intent.intent_type,
                        IntentType::DataTransform { .. } | IntentType::Output { .. }
                    ) {
                        let example_input = grounding_params
                            .get("latest_result")
                            .and_then(|s| serde_json::from_str::<serde_json::Value>(s).ok());

                        let capability_id =
                            generated_capability_id_from_description(&sub_intent.description);

                        // Inline synth removed; queue for resolver to handle synthesis
                        let _ = enqueue_missing_capability_placeholder(
                            capability_id.clone(),
                            sub_intent.description.clone(),
                            None, // input_schema
                            None, // output_schema
                            example_input.clone(),
                            None, // example_output
                            Some(sub_intent.description.clone()),
                            Some(format!(
                                "Planner could not resolve; queued for reification. Last grounding sample: {}",
                                example_input
                                    .as_ref()
                                    .map(|v| truncate_grounding(&v.to_string()))
                                    .unwrap_or_else(|| "n/a".to_string())
                            )),
                        );

                        // Optionally trigger resolver immediately for faster availability
                        if let Some(resolver) = &self.missing_capability_resolver {
                            let mut ctx = HashMap::new();
                            ctx.insert("description".to_string(), sub_intent.description.clone());
                            if let Some(ex) = example_input {
                                if let Ok(s) = serde_json::to_string(&ex) {
                                    ctx.insert("example_input".to_string(), s);
                                }
                            }
                            let request = MissingCapabilityRequest {
                                capability_id: capability_id.clone(),
                                context: ctx,
                                attempt_count: 0,
                                arguments: vec![],
                                requested_at: SystemTime::now(),
                            };
                            match resolver.resolve_capability(&request).await {
                                Ok(ResolutionResult::Resolved {
                                    capability_id: cid, ..
                                }) => {
                                    log::info!(
                                        "Resolved missing capability '{}' immediately via resolver",
                                        cid
                                    );
                                    // Fix: Use the resolved capability immediately instead of falling back
                                    resolutions.insert(
                                        intent_id.clone(),
                                        ResolvedCapability::Local {
                                            capability_id: cid,
                                            arguments: HashMap::new(),
                                            confidence: 1.0,
                                        },
                                    );
                                    continue;
                                }
                                Ok(other) => {
                                    log::info!(
                                        "Resolver did not resolve '{}' immediately: {:?}",
                                        capability_id,
                                        other
                                    );
                                }
                                Err(err) => {
                                    log::warn!(
                                        "Immediate resolve failed for '{}': {}",
                                        capability_id,
                                        err
                                    );
                                }
                            }
                        }
                    }

                    let fallback = self.create_fallback_resolution(&sub_intent, &e);
                    resolutions.insert(intent_id.clone(), fallback);
                }
            }
        }

        // 4. Safe execution pass: execute in topological order with data flow
        if self.config.enable_safe_exec {
            if let Some(executor) = &self.safe_executor {
                ccos_println!("🔄 [safe-exec] starting dependency-ordered pass");
                self.execute_safe_in_order(
                    executor,
                    &decomp_result.sub_intents,
                    &intent_ids,
                    &resolutions,
                    &mut grounding_params,
                    &mut trace,
                )
                .await?;
                ccos_println!("✅ [safe-exec] pass completed");
            } else {
                ccos_println!("⚠️ Safe exec enabled but no executor configured");
            }
        }

        // 5. Generate RTFS plan from resolved intents
        let mut rtfs_plan = self.generate_rtfs_plan(
            &decomp_result.sub_intents,
            &intent_ids,
            &resolutions,
            &grounding_params,
        )?;

        // 5b. LLM Plan Validation (if enabled)
        if self.config.enable_plan_validation {
            use crate::synthesis::validation::{auto_repair_plan, validate_plan, ValidationConfig};
            use std::collections::HashMap as StdHashMap;

            let validation_config = ValidationConfig::default();

            // Build resolutions map for context
            let resolutions_map: StdHashMap<String, String> = resolutions
                .iter()
                .map(|(k, v)| {
                    (
                        k.clone(),
                        v.capability_id().unwrap_or("unknown").to_string(),
                    )
                })
                .collect();

            match validate_plan(&rtfs_plan, &resolutions_map, goal, &validation_config).await {
                Ok(validation) => {
                    if validation.is_valid {
                        log::info!("✅ Plan validation passed");
                    } else {
                        log::warn!("⚠️ Plan validation issues: {:?}", validation.errors);
                        for suggestion in &validation.suggestions {
                            log::info!("   💡 Suggestion: {}", suggestion);
                        }

                        // Attempt auto-repair if enabled
                        if validation_config.enable_auto_repair {
                            match auto_repair_plan(
                                &rtfs_plan,
                                &validation.errors,
                                0,
                                &validation_config,
                            )
                            .await
                            {
                                Ok(Some(repaired_plan)) => {
                                    log::info!("🔧 Plan auto-repaired successfully");
                                    rtfs_plan = repaired_plan;
                                }
                                Ok(None) => {
                                    log::warn!("Auto-repair did not produce a valid plan");
                                }
                                Err(e) => {
                                    log::warn!("Auto-repair failed: {}", e);
                                }
                            }
                        }
                    }
                }
                Err(e) => {
                    log::debug!("Plan validation skipped: {}", e);
                }
            }
        }

        // Determine plan status
        let has_pending_synth = resolutions.values().any(|r| {
            // Check for NeedsReferral with synth suggestion
            if let ResolvedCapability::NeedsReferral {
                suggested_action, ..
            } = r
            {
                if suggested_action.contains("Synth-or-enqueue") {
                    return true;
                }
            }

            // Check for usage of generated (placeholder) capabilities
            if let Some(id) = r.capability_id() {
                if id.starts_with("generated/") {
                    return true;
                }
            }

            false
        });

        // Store plan in PlanArchive
        let mut plan_status = PlanStatus::Draft;
        if has_pending_synth {
            plan_status = PlanStatus::PendingSynthesis;
        }

        let mut archived_plan_id: Option<String> = None;
        let mut archived_hash: Option<String> = None;
        let mut archive_path: Option<PathBuf> = None;

        // We need a way to access PlanArchive here. For now, we'll construct one if we can get the path,
        // but ideally it should be passed in.
        // Assuming we are in CLI/runtime context where we can access storage.
        // For this step, we will rely on the fact that PlanArchive is usually initialized from config.
        // But orchestrator doesn't have direct access to CCOS instance or PlanArchive.
        // We might need to inject PlanArchive into ModularPlanner.

        // TODO: Inject PlanArchive into ModularPlanner properly.
        // For now, if we have persist_intents enabled, we likely want to persist the plan too.
        // We will create a transient PlanArchive pointing to default location if possible,
        // or just skip if we can't easily get it without refactoring everything.
        // Given the request is "Save generated RTFS plan... and associated to the intent graph",
        // we should try to do it.

        if self.config.persist_intents {
            let plan_storage_path = crate::utils::fs::default_plan_archive_path();
            if let Ok(_) = std::fs::create_dir_all(&plan_storage_path) {
                if let Ok(archive) = PlanArchive::with_file_storage(plan_storage_path.clone()) {
                    let mut plan = Plan::new_rtfs(rtfs_plan.clone(), intent_ids.clone());
                    plan.status = plan_status;
                    plan.name = Some(goal.to_string());

                    // Add metadata
                    plan.metadata.insert(
                        "goal".to_string(),
                        rtfs::runtime::values::Value::String(goal.to_string()),
                    );

                    let pid = plan.plan_id.clone();
                    archived_plan_id = Some(pid.clone());

                    match archive.archive_plan(&plan) {
                        Ok(hash) => {
                            archived_hash = Some(hash.clone());
                            archive_path = Some(plan_storage_path.clone());
                            ccos_println!(
                                "💾 Plan archived with status {:?}: {}",
                                plan.status,
                                hash
                            );
                        }
                        Err(e) => {
                            log::warn!("Failed to archive plan: {}", e);
                        }
                    }
                }
            }
        }

        Ok(PlanResult {
            root_intent_id: root_id,
            intent_ids,
            sub_intents: decomp_result.sub_intents,
            resolutions,
            rtfs_plan,
            trace,
            plan_id: archived_plan_id,
            archive_hash: archived_hash,
            archive_path,
        })
    }

    /// Store refined sub-intents under a parent intent and return their ids and copies.
    async fn store_refined_intents(
        &self,
        parent_intent_id: &str,
        _parent_sub_intent: &SubIntent,
        refined: &[SubIntent],
        trace: &mut PlanningTrace,
    ) -> Result<(Vec<String>, Vec<SubIntent>), PlannerError> {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs();

        let mut ids = Vec::new();
        let mut subs = Vec::new();

        for (idx, sub_intent) in refined.iter().enumerate() {
            let intent_id = format!("{}:refine-{}", self.config.intent_namespace, Uuid::new_v4());
            ids.push(intent_id.clone());
            subs.push(sub_intent.clone());

            if self.config.persist_intents {
                let storable = StorableIntent {
                    intent_id: intent_id.clone(),
                    name: Some(format!("Refined Step {}", idx + 1)),
                    original_request: sub_intent.description.clone(),
                    rtfs_intent_source: format!(
                        r#"(intent "{}" :goal "{}")"#,
                        intent_id, sub_intent.description
                    ),
                    goal: sub_intent.description.clone(),
                    constraints: HashMap::new(),
                    preferences: HashMap::new(),
                    success_criteria: None,
                    parent_intent: Some(parent_intent_id.to_string()),
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
                        meta
                    },
                };

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
        }

        Ok((ids, subs))
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
        grounding_params: &HashMap<String, String>,
    ) -> Result<String, PlannerError> {
        if sub_intents.is_empty() {
            return Ok("nil".to_string());
        }

        // Build variable bindings for each step
        let mut bindings: Vec<(String, String)> = Vec::new();

        for (idx, sub_intent) in sub_intents.iter().enumerate() {
            let intent_id = &intent_ids[idx];
            let var_name = format!("step_{}", idx + 1);

            // Use only prior step bindings (skip meta like _grounding_metadata) for dependency wiring
            let logical_bindings: Vec<(String, String)> = bindings
                .iter()
                .filter(|(name, _)| !name.starts_with('_'))
                .cloned()
                .collect();

            let mut call_expr = match resolutions.get(intent_id) {
                Some(resolved) => match resolved {
                    ResolvedCapability::Local { .. }
                    | ResolvedCapability::Remote { .. }
                    | ResolvedCapability::BuiltIn { .. }
                    | ResolvedCapability::Synthesized { .. } => self.generate_call_expr(
                        resolved,
                        sub_intent,
                        &logical_bindings,
                        sub_intents,
                    ),
                    ResolvedCapability::NeedsReferral {
                        suggested_action, ..
                    } => {
                        format!(
                            r#"(call "ccos.user.ask" {{:prompt "Cannot proceed: {}"}})"#,
                            suggested_action.replace('"', "\\\"")
                        )
                    }
                },
                None => {
                    format!(
                        r#"(call "ccos.user.ask" {{:prompt "No resolution for step: {}"}})"#,
                        sub_intent.description.replace('"', "\\\"")
                    )
                }
            };

            // Optionally annotate with grounded preview as a harmless let (no context writes)
            if let Some(grounded) = grounding_params.get(&format!("result_{}", intent_id)) {
                let grounded = truncate_grounding(grounded);
                let escaped = grounded.replace('"', "\\\"");
                call_expr = format!(
                    "(let [:_grounding_comment \"{}\"]\n  {})",
                    escaped, call_expr
                );
            }

            bindings.push((var_name, call_expr));
        }

        // Post-process: detect schema mismatches and insert inline adapters
        // For each step with grounding data, check if downstream consumers expect a different type
        let mut adapter_mappings: HashMap<String, String> = HashMap::new();

        for (idx, _) in sub_intents.iter().enumerate() {
            let intent_id = &intent_ids[idx];
            let var_name = format!("step_{}", idx + 1);

            // Get raw grounding data if available (not the preview format)
            if let Some(grounded_str) = grounding_params.get(&format!("raw_result_{}", intent_id)) {
                // Try to parse the grounding data to detect structure
                if let Ok(grounded_json) = serde_json::from_str::<serde_json::Value>(grounded_str) {
                    // Check each consumer of this step
                    for (consumer_idx, consumer_intent) in sub_intents.iter().enumerate() {
                        if consumer_intent.dependencies.contains(&idx) {
                            // Get consumer's expected input schema
                            let consumer_id = &intent_ids[consumer_idx];
                            let consumer_schema = resolutions.get(consumer_id).and_then(|r| {
                                if let ResolvedCapability::Remote { input_schema, .. } = r {
                                    input_schema.as_ref()
                                } else {
                                    // Synthesized and other types don't have input_schema
                                    None
                                }
                            });

                            // Detect if we need an adapter
                            let bridge =
                                SchemaBridge::detect(None, consumer_schema, Some(&grounded_json));
                            if bridge.needs_adapter() {
                                let adapted_var = format!("{}_data", var_name);
                                let adapter_expr = bridge.generate_rtfs_expr(&var_name);
                                adapter_mappings.insert(var_name.clone(), adapted_var.clone());

                                log::debug!(
                                    "🔌 Inserting adapter for {}: {} → {}",
                                    var_name,
                                    bridge.description,
                                    adapter_expr
                                );
                                ccos_println!(
                                    "   🔌 Adapter: {} → {} ({})",
                                    var_name,
                                    adapted_var,
                                    bridge.description
                                );
                                break; // Only need one adapter per producer
                            }
                        }
                    }
                }
            }
        }

        // Insert adapter bindings and update references
        if !adapter_mappings.is_empty() {
            let mut new_bindings: Vec<(String, String)> = Vec::new();

            for (var_name, call_expr) in bindings.iter() {
                new_bindings.push((var_name.clone(), call_expr.clone()));

                // If this step needs an adapter, add it right after
                if let Some(adapted_var) = adapter_mappings.get(var_name) {
                    // Parse grounding to detect the right field to extract
                    let producer_idx: usize = var_name
                        .strip_prefix("step_")
                        .and_then(|s| s.parse().ok())
                        .unwrap_or(1)
                        - 1;

                    if let Some(intent_id) = intent_ids.get(producer_idx) {
                        if let Some(grounded_str) =
                            grounding_params.get(&format!("raw_result_{}", intent_id))
                        {
                            if let Ok(grounded_json) =
                                serde_json::from_str::<serde_json::Value>(grounded_str)
                            {
                                let bridge = SchemaBridge::detect(None, None, Some(&grounded_json));
                                let adapter_expr = bridge.generate_rtfs_expr(var_name);
                                new_bindings.push((adapted_var.clone(), adapter_expr));
                            }
                        }
                    }
                }
            }

            // Update call expressions to use adapted variables
            bindings = new_bindings
                .into_iter()
                .map(|(name, mut expr)| {
                    // Replace references to original vars with adapted vars in call expressions
                    for (orig, adapted) in &adapter_mappings {
                        // Only replace in argument positions, not in the variable binding itself
                        if !name.ends_with("_data") {
                            expr =
                                expr.replace(&format!(" {}}}", orig), &format!(" {}}}", adapted));
                            expr = expr.replace(&format!(" {} ", orig), &format!(" {} ", adapted));
                        }
                    }
                    (name, expr)
                })
                .collect();
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

    /// Format a single argument value for RTFS, using schema for type coercion
    fn format_arg_value(
        &self,
        key: &str,
        value: &str,
        schema: Option<&serde_json::Value>,
        previous_bindings: &[(String, String)],
    ) -> String {
        // Check for LLM-generated step references like {{step0.result}}
        // The LLM sometimes generates 0-based step indices in handlebars syntax
        // We need to convert these to actual variable names (step_1, step_2, etc.)
        lazy_static::lazy_static! {
            static ref STEP_REF: Regex = Regex::new(r"\{\{step(\d+)\.result\}\}").unwrap();
        }

        if let Some(captures) = STEP_REF.captures(value) {
            if let Some(idx_str) = captures.get(1) {
                if let Ok(idx) = idx_str.as_str().parse::<usize>() {
                    // LLM uses 0-based index usually
                    if idx < previous_bindings.len() {
                        let var_name = &previous_bindings[idx].0;
                        return format!(":{} {}", key, var_name);
                    }
                }
            }
        }

        // Check if schema tells us this should be a number
        if let Some(schema) = schema {
            if let Some(props) = schema.get("properties").and_then(|p| p.as_object()) {
                if let Some(prop_def) = props.get(key) {
                    let prop_type = prop_def.get("type").and_then(|t| t.as_str());
                    match prop_type {
                        Some("number") | Some("integer") => {
                            // Try to parse as number, output without quotes
                            if value.parse::<i64>().is_ok() || value.parse::<f64>().is_ok() {
                                return format!(":{} {}", key, value);
                            }
                        }
                        Some("boolean") => {
                            let lower = value.to_lowercase();
                            if lower == "true" || lower == "false" {
                                return format!(":{} {}", key, lower);
                            }
                        }
                        _ => {}
                    }
                }
            }
        }

        // Also try to infer from value itself if it looks like a number or boolean
        if value.parse::<i64>().is_ok() || value.parse::<f64>().is_ok() {
            return format!(":{} {}", key, value);
        }
        let lower = value.to_lowercase();
        if lower == "true" || lower == "false" {
            return format!(":{} {}", key, lower);
        }

        // Default: quote as string
        format!(":{} \"{}\"", key, value.replace('"', "\\\""))
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

        // Get input schema if available (for type coercion)
        let input_schema = match resolved_capability {
            ResolvedCapability::Remote { input_schema, .. } => input_schema.as_ref(),
            _ => None,
        };

        // Check if this step depends on previous outputs
        let has_dependencies = !sub_intent.dependencies.is_empty();

        let mut used_dependency_vars = std::collections::HashSet::new();

        let mut args_parts: Vec<String> = Vec::new();

        // Add explicit arguments (with type coercion and ref replacement)
        for (key, value) in arguments {
            // If the LLM emitted a placeholder like "$0" and we have a single dependency,
            // map it directly to that dependency variable (common for output/println).
            if value == "$0" && has_dependencies && sub_intent.dependencies.len() == 1 {
                let dep_idx = sub_intent.dependencies[0];
                if dep_idx < previous_bindings.len() {
                    let dep_var = &previous_bindings[dep_idx].0;
                    let formatted = format!(":{} {}", key, dep_var);
                    args_parts.push(formatted);
                    used_dependency_vars.insert(dep_var.clone());
                    continue;
                }
            }

            let formatted = self.format_arg_value(key, value, input_schema, previous_bindings);

            // Validate that the formatted argument adheres to RTFS syntax
            // Only allow :keyword value pairs where value is either a quoted string,
            // a number, a boolean, or a variable reference (starting with step_)
            // This is a safety check to ensure no raw handlebars or invalid syntax leaks through
            if formatted.starts_with(':') {
                args_parts.push(formatted);
            } else {
                log::warn!("Skipping invalid RTFS argument syntax: {}", formatted);
            }

            // Check if this argument consumed a dependency
            lazy_static::lazy_static! {
                static ref STEP_REF: Regex = Regex::new(r"\{\{step(\d+)\.result\}\}").unwrap();
            }
            if let Some(captures) = STEP_REF.captures(value) {
                if let Some(idx_str) = captures.get(1) {
                    if let Ok(idx) = idx_str.as_str().parse::<usize>() {
                        if idx < previous_bindings.len() {
                            used_dependency_vars.insert(previous_bindings[idx].0.clone());
                        }
                    }
                }
            }
        }

        if has_dependencies {
            // Add reference to previous step if not already in args
            // This creates data flow from previous steps
            if sub_intent.dependencies.len() == 1 {
                let dep_idx = sub_intent.dependencies[0];
                if dep_idx < previous_bindings.len() {
                    let dep_var = &previous_bindings[dep_idx].0;

                    // Only inject if:
                    // 1. We haven't used this dependency in an explicit argument (via {{step.result}})
                    // 2. We don't have an explicit argument that matches the variable name (old logic)
                    let already_used_explicitly = used_dependency_vars.contains(dep_var);
                    let already_has_arg_value = arguments.values().any(|v| v == dep_var);

                    if !already_used_explicitly && !already_has_arg_value {
                        // Attempt to infer parameter name from dependency and schema
                        let param_name = if let Some(dep_intent) = all_sub_intents.get(dep_idx) {
                            self.infer_param_name(dep_intent, resolved_capability)
                        } else {
                            // Default to a generic name instead of _previous_result
                            "_previous_result".to_string()
                        };
                        args_parts.push(format!(":{} {}", param_name, dep_var));
                    }
                }
            }
        }

        if args_parts.is_empty() {
            format!(r#"(call "{}" {{}})"#, capability_id)
        } else {
            format!(r#"(call "{}" {{{}}})"#, capability_id, args_parts.join(" "))
        }
    }

    /// Helper to infer a parameter name from a dependency intent and consumer capability schema
    fn infer_param_name(
        &self,
        producer_intent: &SubIntent,
        consumer_capability: &ResolvedCapability,
    ) -> String {
        // Prefer well-known output sinks
        if let Some(cap_id) = consumer_capability.capability_id() {
            if cap_id == "ccos.io.println" {
                return "message".to_string();
            }
        }

        // 1. Try schema-based matching if available
        if let ResolvedCapability::Remote {
            input_schema: Some(schema),
            ..
        } = consumer_capability
        {
            if let Some(best_match) = self.match_topic_to_schema(producer_intent, schema) {
                return best_match;
            }
        }

        // 2. Fallback to simple heuristic
        match &producer_intent.intent_type {
            IntentType::UserInput { prompt_topic } => {
                let normalized = prompt_topic.trim().to_lowercase();

                // Manual overrides for common terms
                if normalized.contains("page size")
                    || normalized == "limit"
                    || normalized == "count"
                {
                    return "per_page".to_string();
                }
                if normalized == "page" {
                    return "page".to_string();
                }

                // Fallback: use topic as snake_case param
                normalized.replace(' ', "_")
            }
            _ => "data".to_string(),
        }
    }

    /// Match intent topic to schema properties using fuzzy matching
    fn match_topic_to_schema(
        &self,
        intent: &SubIntent,
        schema: &serde_json::Value,
    ) -> Option<String> {
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
            let prop_tokens: Vec<&str> = prop_lower.split('_').collect(); // snake_case

            let matches = topic_tokens
                .iter()
                .filter(|t| prop_tokens.contains(t))
                .count();
            if matches > 0 {
                score += 0.3 * (matches as f64);
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

    async fn maybe_execute_and_ground(
        &self,
        executor: &SafeCapabilityExecutor,
        sub_intent: &SubIntent,
        resolved: &ResolvedCapability,
        previous_result: Option<&rtfs::runtime::values::Value>,
    ) -> Result<Option<rtfs::runtime::values::Value>, PlannerError> {
        let cap_id = match resolved.capability_id() {
            Some(id) => id,
            None => return Ok(None),
        };

        // Only run for API calls, data transforms, and outputs; skip user_input
        match sub_intent.intent_type {
            IntentType::ApiCall { .. }
            | IntentType::DataTransform { .. }
            | IntentType::Output { .. } => {}
            _ => {
                log::debug!(
                    "Safe exec skipped for {} (intent type not eligible)",
                    cap_id
                );
                return Ok(None);
            }
        }

        // Build params, threading previous_result into map if present and serializing it to JSON string.
        let mut params = sub_intent.extracted_params.clone();
        if let Some(prev) = previous_result {
            if let Ok(prev_json) = rtfs_value_to_json(prev) {
                if let Ok(s) = serde_json::to_string(&prev_json) {
                    params.insert("_previous_result".to_string(), s);
                }
            }
        }

        match executor
            .execute_if_safe(cap_id, &params, previous_result)
            .await
        {
            Ok(Some(val)) => {
                // Post-execution schema introspection for synthesized capabilities
                if cap_id.starts_with("generated/") {
                    use crate::synthesis::introspection::schema_refiner;

                    // Find capability file once
                    if let Some(path) = schema_refiner::find_capability_file(cap_id) {
                        // 1. Schema refinement
                        let result =
                            schema_refiner::infer_output_schema_from_result(cap_id, &val, None);
                        if result.was_updated {
                            match schema_refiner::update_capability_output_schema(
                                &path,
                                &result.inferred_output_schema,
                            ) {
                                Ok(true) => {
                                    log::info!(
                                        "📊 Schema refined for {}: {} → {}",
                                        cap_id,
                                        result.original_output_schema,
                                        result.inferred_output_schema
                                    );

                                    // 1b. LLM Schema Validation (if enabled)
                                    if self.config.enable_schema_validation {
                                        use crate::synthesis::validation::{
                                            validate_schema, ValidationConfig,
                                        };
                                        let validation_config = ValidationConfig::default();
                                        let sample_preview = grounding_preview(&val);

                                        match validate_schema(
                                            &result.inferred_output_schema,
                                            cap_id,
                                            Some(&sample_preview),
                                            &validation_config,
                                        )
                                        .await
                                        {
                                            Ok(validation) => {
                                                if validation.is_valid {
                                                    log::info!(
                                                        "✅ Schema validation passed for {}",
                                                        cap_id
                                                    );
                                                } else {
                                                    log::warn!(
                                                        "⚠️ Schema validation issues for {}: {:?}",
                                                        cap_id,
                                                        validation.errors
                                                    );
                                                    for suggestion in &validation.suggestions {
                                                        log::info!(
                                                            "   💡 Suggestion: {}",
                                                            suggestion
                                                        );
                                                    }
                                                }
                                            }
                                            Err(e) => {
                                                log::debug!(
                                                    "Schema validation skipped for {}: {}",
                                                    cap_id,
                                                    e
                                                );
                                            }
                                        }
                                    }
                                }
                                Ok(false) => {}
                                Err(e) => {
                                    log::warn!("Failed to update schema for {}: {}", cap_id, e);
                                }
                            }
                        }

                        // 2. Metadata sample capture
                        match schema_refiner::update_capability_metadata_samples(
                            &path,
                            previous_result, // input sample
                            &val,            // output sample
                        ) {
                            Ok(true) => {
                                log::debug!("📝 Captured sample output for {}", cap_id);
                            }
                            Ok(false) => {}
                            Err(e) => {
                                log::debug!("Failed to capture sample for {}: {}", cap_id, e);
                            }
                        }
                    }
                }
                Ok(Some(val))
            }
            Ok(None) => {
                log::debug!(
                    "Safe exec skipped for {} (not allowed or no manifest); params keys={:?}",
                    cap_id,
                    params.keys().collect::<Vec<_>>()
                );
                Ok(None)
            }
            Err(e) => {
                log::warn!("Safe exec error for {}: {}", cap_id, e);
                ccos_eprintln!("DEBUG: Safe exec error for {}: {}", cap_id, e);
                // Don't propagate error - just skip this step
                Ok(None)
            }
        }
    }

    /// Execute safe capabilities in topological order, passing results through the pipeline.
    ///
    /// This method:
    /// 1. Computes topological order based on SubIntent.dependencies
    /// 2. Executes each step in order, passing _previous_result from dependencies
    /// 3. Stores results in grounding_params for downstream use
    async fn execute_safe_in_order(
        &self,
        executor: &SafeCapabilityExecutor,
        sub_intents: &[SubIntent],
        intent_ids: &[String],
        resolutions: &HashMap<String, ResolvedCapability>,
        grounding_params: &mut HashMap<String, String>,
        trace: &mut PlanningTrace,
    ) -> Result<(), PlannerError> {
        use rtfs::runtime::values::Value;

        // Compute topological order based on dependencies
        let order = self.topological_sort(sub_intents);

        ccos_println!(
            "\n╭──────────────────────────────────────────────────────────────────────────────"
        );
        ccos_println!(
            "│ 🔄 Safe Execution Pass ({} steps in dependency order)",
            order.len()
        );
        ccos_println!(
            "╰──────────────────────────────────────────────────────────────────────────────"
        );

        // Store execution results by step index
        let mut step_results: HashMap<usize, Value> = HashMap::new();

        for step_idx in order {
            if step_idx >= sub_intents.len() {
                continue;
            }

            let sub_intent = &sub_intents[step_idx];
            let intent_id = if step_idx < intent_ids.len() {
                &intent_ids[step_idx]
            } else {
                continue;
            };

            let resolved = match resolutions.get(intent_id) {
                Some(r) => r,
                None => continue,
            };

            let cap_id = resolved.capability_id().unwrap_or("unknown");

            // Get the previous result from the most recent dependency
            let previous_result: Option<&Value> = if !sub_intent.dependencies.is_empty() {
                // Use the result from the last dependency (most recent in pipeline)
                sub_intent
                    .dependencies
                    .iter()
                    .filter_map(|dep_idx| step_results.get(dep_idx))
                    .last()
            } else {
                None
            };

            ccos_println!(
                "\n▶️  Step {}/{}: {}",
                step_idx + 1,
                sub_intents.len(),
                sub_intent.description
            );
            ccos_println!("   ├─ Intent ID: {}", intent_id);
            ccos_println!("   ├─ Capability: {}", cap_id);
            ccos_println!("   ├─ Dependencies: {:?}", sub_intent.dependencies);
            if previous_result.is_some() {
                ccos_println!("   ├─ Input: Received _previous_result from upstream");
            }

            // Execute the step with dependency data
            match self
                .maybe_execute_and_ground(executor, sub_intent, resolved, previous_result)
                .await
            {
                Ok(Some(result)) => {
                    let grounded_text = grounding_preview(&result);

                    trace.events.push(TraceEvent::ResolutionCompleted {
                        intent_id: intent_id.clone(),
                        capability: format!("{} (executed)", cap_id),
                    });

                    ccos_println!(
                        "   ╰─ ✅ Result Captured ({} chars): {}",
                        grounded_text.len(),
                        grounded_text
                            .chars()
                            .take(100)
                            .collect::<String>()
                            .replace("\n", " ")
                    );

                    // Store for downstream steps
                    step_results.insert(step_idx, result.clone());

                    grounding_params.insert(format!("result_{}", intent_id), grounded_text.clone());
                    grounding_params
                        .insert(format!("step_{}_result", step_idx), grounded_text.clone());
                    grounding_params.insert("latest_result".to_string(), grounded_text);

                    // Also store raw JSON for adapter detection
                    if let Ok(raw_json) = rtfs_value_to_json(&result) {
                        if let Ok(raw_str) = serde_json::to_string(&raw_json) {
                            grounding_params.insert(format!("raw_result_{}", intent_id), raw_str);
                        }
                    }
                }
                Ok(None) => {
                    ccos_println!("   ╰─ ⏭️  Skipped (Not safe to execute or no manifest)");
                }
                Err(e) => {
                    ccos_println!("   ╰─ ❌ Error: {}", e);
                    // Continue with other steps
                }
            }
        }

        ccos_println!(
            "\n✅ Safe execution pass complete ({} results captured)\n",
            step_results.len()
        );

        Ok(())
    }

    /// Compute topological order of sub-intents based on their dependencies.
    /// Uses a stable Kahn's algorithm (preserves original order among roots).
    fn topological_sort(&self, sub_intents: &[SubIntent]) -> Vec<usize> {
        let n = sub_intents.len();
        if n == 0 {
            return vec![];
        }

        // Compute in-degree for each node
        let mut in_degree = vec![0usize; n];
        for (idx, intent) in sub_intents.iter().enumerate() {
            // Each dependency means an incoming edge
            for _dep in &intent.dependencies {
                if idx < n {
                    in_degree[idx] += 1;
                }
            }
        }

        // Start with nodes that have no dependencies (in-degree 0), preserve index order
        let mut queue: VecDeque<usize> = VecDeque::new();
        let mut enqueued = vec![false; n];
        for i in 0..n {
            if sub_intents[i].dependencies.is_empty() {
                queue.push_back(i);
                enqueued[i] = true;
            }
        }

        let mut result = Vec::with_capacity(n);

        while let Some(node) = queue.pop_front() {
            result.push(node);

            // For each node that depends on this one, decrease in-degree
            for (idx, intent) in sub_intents.iter().enumerate() {
                if intent.dependencies.contains(&node) {
                    in_degree[idx] = in_degree[idx].saturating_sub(1);
                    if in_degree[idx] == 0 && !enqueued[idx] {
                        queue.push_back(idx);
                        enqueued[idx] = true;
                    }
                }
            }
        }

        // If we couldn't process all nodes, there's a cycle - fall back to natural order
        if result.len() < n {
            log::warn!("Dependency cycle detected, falling back to natural order");
            return (0..n).collect();
        }

        result
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::intent_graph::config::IntentGraphConfig;
    use crate::planner::modular_planner::decomposition::PatternDecomposition;
    use crate::planner::modular_planner::resolution::semantic::CapabilityCatalog;
    use crate::planner::modular_planner::resolution::semantic::CapabilityInfo;
    use crate::planner::modular_planner::resolution::CatalogResolution;

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
        assert!(matches!(
            first_resolution,
            ResolvedCapability::BuiltIn { .. }
        ));

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

        let sub_intent = SubIntent::new(
            "test",
            IntentType::ApiCall {
                action: crate::planner::modular_planner::types::ApiAction::List,
            },
        );

        let resolved = ResolvedCapability::Remote {
            capability_id: "mcp.github.list_issues".to_string(),
            server_url: "http://test".to_string(),
            arguments: args,
            input_schema: None,
            confidence: 1.0,
        };

        let expr = planner.generate_call_expr(&resolved, &sub_intent, &[], &[sub_intent.clone()]);

        assert!(expr.contains("mcp.github.list_issues"));
        assert!(expr.contains(":owner"));
        assert!(expr.contains("mandubian"));
    }
}
