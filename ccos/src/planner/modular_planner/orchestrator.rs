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
use serde_json::json;

use super::decomposition::hybrid::HybridConfig;
use super::decomposition::{DecompositionContext, DecompositionError, DecompositionStrategy};
use super::resolution::{
    ResolutionContext, ResolutionError, ResolutionStrategy, ResolvedCapability,
};
use super::safe_executor::SafeCapabilityExecutor;
use super::types::ToolSummary;
use super::types::{ApiAction, DomainHint, IntentType, SubIntent};
use crate::capability_marketplace::CapabilityMarketplace;
use crate::intent_graph::storage::Edge;
use crate::intent_graph::IntentGraph;
use crate::plan_archive::PlanArchive;
use crate::synthesis::core::{SynthesizedCapability, SynthesizedCapabilityStorage};
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
    // Truncate to max 30 chars for shorter capability IDs
    let slug = if slug.len() > 30 {
        slug[..30].trim_end_matches('-').to_string()
    } else {
        slug
    };
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

/// Sanitize LLM-generated RTFS expressions to fix common patterns that RTFS doesn't support.
///
/// This is a generic post-processor that handles:
/// 1. **Namespace prefixes**: `str/split` → `split` (remove any `foo/` prefix)
/// 2. **Regex literals**: `#"..."` → `"..."` (remove the `#` prefix)
/// 3. **Python-style methods**: `.lower()`, `.upper()` → `(lower ...)`, `(upper ...)`
///
/// This allows the LLM to generate Clojure-like or Python-like syntax while producing valid RTFS.
fn sanitize_llm_rtfs(expr: &str) -> String {
    lazy_static::lazy_static! {
        // Match namespace/function patterns like str/split, clojure.string/join
        static ref NAMESPACE_FN: Regex = Regex::new(r"\b([a-zA-Z][a-zA-Z0-9._-]*)/([a-zA-Z][a-zA-Z0-9_-]*)").unwrap();
        // Match regex literals like #"pattern" - convert to regular strings
        static ref REGEX_LITERAL: Regex = Regex::new(r#"#"([^"\\]*(?:\\.[^"\\]*)*)""#).unwrap();
    }

    let mut result = expr.to_string();
    let mut changes: Vec<&str> = vec![];

    // Remove namespace prefixes: str/split → split
    let after_ns = NAMESPACE_FN.replace_all(&result, "$2").to_string();
    if after_ns != result {
        changes.push("namespace_prefix");
        result = after_ns;
    }

    // Convert regex literals to regular strings: #"..." → "..."
    let after_regex = REGEX_LITERAL.replace_all(&result, r#""$1""#).to_string();
    if after_regex != result {
        changes.push("regex_literal");
        result = after_regex;
    }

    if !changes.is_empty() {
        log::info!(
            "[sanitize_llm_rtfs] Applied {} fix(es): [{}]",
            changes.len(),
            changes.join(", ")
        );
        log::debug!(
            "[sanitize_llm_rtfs] Before: {}\n              After:  {}",
            &expr[..expr.len().min(100)],
            &result[..result.len().min(100)]
        );
    }

    result
}

/// Detect unresolved capabilities in an RTFS expression.
///
/// Scans the expression for `(call "generated/..." ...)` or `(call "pending/..." ...)`
/// patterns and returns the list of capability IDs that need synthesis.
///
/// This is used to identify when an LLM-generated expression references
/// capabilities that don't exist yet and need to be created.
pub fn detect_unresolved_capabilities(expr: &str) -> Vec<String> {
    use rtfs::ast::{Expression, Literal, Symbol};

    fn collect_unresolved(expr: &Expression, acc: &mut Vec<String>) {
        match expr {
            Expression::FunctionCall { callee, arguments } => {
                // Check if this is a (call "generated/..." ...) or (call "pending/..." ...)
                if let Expression::Symbol(Symbol(sym)) = callee.as_ref() {
                    if sym == "call" {
                        if let Some(Expression::Literal(Literal::String(cap_id))) =
                            arguments.first()
                        {
                            if cap_id.starts_with("generated/") || cap_id.starts_with("pending/") {
                                if !acc.contains(cap_id) {
                                    acc.push(cap_id.clone());
                                }
                            }
                        }
                    }
                }
                // Recurse into callee and arguments
                collect_unresolved(callee, acc);
                for arg in arguments {
                    collect_unresolved(arg, acc);
                }
            }
            Expression::List(items) | Expression::Vector(items) => {
                for item in items {
                    collect_unresolved(item, acc);
                }
            }
            Expression::Map(map) => {
                for (_, v) in map {
                    collect_unresolved(v, acc);
                }
            }
            Expression::If(if_expr) => {
                collect_unresolved(&if_expr.condition, acc);
                collect_unresolved(&if_expr.then_branch, acc);
                if let Some(else_branch) = &if_expr.else_branch {
                    collect_unresolved(else_branch, acc);
                }
            }
            Expression::Let(let_expr) => {
                for binding in &let_expr.bindings {
                    collect_unresolved(&binding.value, acc);
                }
                for body_expr in &let_expr.body {
                    collect_unresolved(body_expr, acc);
                }
            }
            Expression::Do(do_expr) => {
                for e in &do_expr.expressions {
                    collect_unresolved(e, acc);
                }
            }
            Expression::Fn(fn_expr) => {
                for body_expr in &fn_expr.body {
                    collect_unresolved(body_expr, acc);
                }
            }
            Expression::Quasiquote(inner)
            | Expression::Unquote(inner)
            | Expression::UnquoteSplicing(inner)
            | Expression::Deref(inner) => {
                collect_unresolved(inner, acc);
            }
            Expression::WithMetadata { expr, .. } => {
                collect_unresolved(expr, acc);
            }
            // Other expressions don't contain nested calls
            _ => {}
        }
    }

    let mut unresolved = Vec::new();
    if let Ok(parsed) = rtfs::parser::parse_expression(expr) {
        collect_unresolved(&parsed, &mut unresolved);
    }
    unresolved
}

/// Normalize parameter names for common API naming variations.
///
/// This handles cases where the LLM uses different names than the API expects,
/// without hardcoding specific mappings in generic code.
fn normalize_param_name(name: &str) -> String {
    match name {
        // Common variations - these are generic patterns, not specific to any API
        "repository" | "name" => "repo".to_string(),
        "issue" => "issue_number".to_string(),
        // Internal params pass through
        s if s.starts_with('_') => name.to_string(),
        // Everything else unchanged
        _ => name.to_string(),
    }
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
    /// Max attempts to retry decomposition if it produces pending capabilities
    pub max_decomposition_retries: usize,
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
            max_decomposition_retries: 2,    // Retry decomposition if pending caps found
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
    /// Plan status (Draft, Ready, PendingSynthesis, etc.)
    pub plan_status: PlanStatus,
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
    /// Discovery search completed for a query
    DiscoverySearchCompleted {
        query: String,
        num_results: usize,
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
    /// Optional delegating arbiter for LLM-based adapter generation
    delegating_arbiter: Option<Arc<crate::arbiter::DelegatingArbiter>>,
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
            delegating_arbiter: None,
            trace_callback: None,
        }
    }

    /// Create with default hybrid decomposition (pattern-only, no LLM)
    pub fn with_patterns(intent_graph: Arc<Mutex<IntentGraph>>) -> Self {
        Self {
            decomposition: Box::new(
                crate::planner::modular_planner::decomposition::HybridDecomposition::pattern_only(),
            ),
            resolution: Box::new(
                crate::planner::modular_planner::resolution::CompositeResolution::new(),
            ),
            intent_graph,
            config: PlannerConfig::default(),
            safe_executor: None,
            missing_capability_resolver: None,
            delegating_arbiter: None,
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

    /// Inject a delegating arbiter for LLM-based adapter generation.
    pub fn with_delegating_arbiter(
        mut self,
        arbiter: Arc<crate::arbiter::DelegatingArbiter>,
    ) -> Self {
        self.delegating_arbiter = Some(arbiter);
        self
    }

    /// Enable safe execution using the provided marketplace
    pub fn with_safe_executor(mut self, marketplace: Arc<CapabilityMarketplace>) -> Self {
        self.safe_executor = Some(SafeCapabilityExecutor::new(marketplace));
        self
    }

    /// Enable safe execution with approval queue for human-in-the-loop governance
    pub fn with_safe_executor_and_approval(
        mut self,
        marketplace: Arc<CapabilityMarketplace>,
        _approval_queue: std::sync::Arc<
            crate::approval::UnifiedApprovalQueue<
                crate::approval::storage_file::FileApprovalStorage,
            >,
        >,
        _constraints: Option<crate::agents::identity::AgentConstraints>,
    ) -> Self {
        // For now, just use the basic safe executor
        // TODO: Wire approval queue into SafeCapabilityExecutor when governance gates are enabled
        self.safe_executor = Some(SafeCapabilityExecutor::new(marketplace));
        self
    }

    /// Emit a trace event - pushes to the Vec AND calls the callback if set.
    /// This enables real-time streaming of trace events to the TUI.
    #[allow(dead_code)]
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

        // Decomposition retry loop: if resolution produces pending capabilities, retry decomposition
        let max_decomp_retries = self.config.max_decomposition_retries;
        let mut decomp_attempt = 0;
        let decomp_result;

        'decomp_retry: loop {
            decomp_attempt += 1;

            let attempt_result = self
                .decomposition
                .decompose(goal, tools_slice, &decomp_context)
                .await?;

            // For first attempt or if max retries reached, use this result
            if decomp_attempt >= max_decomp_retries {
                decomp_result = attempt_result;
                break 'decomp_retry;
            }

            // Quick check: resolve intents and see if any are pending
            let temp_resolution_context = ResolutionContext::new();
            let mut has_pending = false;

            for sub_intent in &attempt_result.sub_intents {
                match self
                    .resolution
                    .resolve(sub_intent, &temp_resolution_context)
                    .await
                {
                    Ok(resolved) => {
                        if resolved.is_pending() {
                            has_pending = true;
                            break;
                        }
                    }
                    Err(_) => {
                        // Resolution error means we'll get a fallback/pending
                        has_pending = true;
                        break;
                    }
                }
            }

            if has_pending && decomp_attempt < max_decomp_retries {
                ccos_println!(
                    "🔄 Decomposition attempt {} produced pending capabilities, retrying...",
                    decomp_attempt
                );
                continue 'decomp_retry;
            }

            decomp_result = attempt_result;
            break 'decomp_retry;
        }

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
                                            input_schema: None,
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
        let mut rtfs_plan = self
            .generate_rtfs_plan(
                &decomp_result.sub_intents,
                &intent_ids,
                &resolutions,
                &grounding_params,
            )
            .await?;

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

            // Save synthesized capabilities for reuse
            if has_pending_synth {
                self.save_synthesized_capabilities(&decomp_result.sub_intents, &resolutions, goal);
            }
        }

        // Clean up resolutions to only include entries for the original sub_intents
        // During refinement, additional intent_ids and placeholder resolutions may be added
        // but we only want to return the final working set that matches sub_intents
        let original_intent_count = decomp_result.sub_intents.len();
        let valid_intent_ids: HashSet<&String> =
            intent_ids.iter().take(original_intent_count).collect();
        resolutions.retain(|k, _| valid_intent_ids.contains(k));

        // Also truncate intent_ids to match the original sub_intents count
        intent_ids.truncate(original_intent_count);

        Ok(PlanResult {
            root_intent_id: root_id,
            intent_ids,
            sub_intents: decomp_result.sub_intents,
            resolutions,
            rtfs_plan,
            trace,
            plan_status,
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
    pub(crate) async fn generate_rtfs_plan(
        &self,
        sub_intents: &[SubIntent],
        intent_ids: &[String],
        resolutions: &HashMap<String, ResolvedCapability>,
        grounding_params: &HashMap<String, String>,
    ) -> Result<String, PlannerError> {
        if sub_intents.is_empty() {
            return Ok("nil".to_string());
        }

        // Log grounding summary
        let grounded_steps: Vec<(usize, &str)> = intent_ids
            .iter()
            .enumerate()
            .filter_map(|(idx, id)| {
                if grounding_params.contains_key(&format!("result_{}", id)) {
                    Some((
                        idx + 1,
                        sub_intents
                            .get(idx)
                            .map(|s| s.description.as_str())
                            .unwrap_or("?"),
                    ))
                } else {
                    None
                }
            })
            .collect();

        if !grounded_steps.is_empty() {
            ccos_println!(
                "📊 Grounding Summary: {} of {} steps grounded",
                grounded_steps.len(),
                sub_intents.len()
            );
            for (step_num, desc) in &grounded_steps {
                let desc_preview: String = desc.chars().take(50).collect();
                ccos_println!(
                    "   ✅ step_{}: {}{}",
                    step_num,
                    desc_preview,
                    if desc.len() > 50 { "..." } else { "" }
                );
            }
        } else {
            ccos_println!(
                "📊 Grounding Summary: 0 steps grounded (no safe-exec results available)"
            );
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
                        Some(grounding_params),
                        &intent_ids,
                    ),
                    ResolvedCapability::NeedsReferral {
                        suggested_action, ..
                    } => {
                        let mut resolved_expr = None;

                        // Try LLM fallback if we have an arbiter and exactly one dependency with grounding data
                        if let Some(arbiter) = &self.delegating_arbiter {
                            if sub_intent.dependencies.len() == 1 {
                                let dep_idx = sub_intent.dependencies[0];
                                let dep_intent_id = &intent_ids[dep_idx];
                                if let Some(grounded_str) =
                                    grounding_params.get(&format!("raw_result_{}", dep_intent_id))
                                {
                                    if let Ok(grounded_json) =
                                        serde_json::from_str::<serde_json::Value>(grounded_str)
                                    {
                                        let dep_desc = &sub_intents[dep_idx].description;
                                        if let Some(adapter) = self
                                            .request_llm_adapter(
                                                dep_desc,
                                                &grounded_json,
                                                grounding_params
                                                    .get(&format!("rtfs_schema_{}", dep_intent_id))
                                                    .map(|s| s.as_str()),
                                                None,
                                                &sub_intent.description,
                                                &logical_bindings,
                                            )
                                            .await
                                        {
                                            let dep_var = format!("step_{}", dep_idx + 1);
                                            resolved_expr =
                                                Some(adapter.replace("input", &dep_var));

                                            ccos_println!(
                                                "   🧠 LLM solved intent: {} → {}",
                                                sub_intent.description,
                                                resolved_expr.as_ref().unwrap()
                                            );
                                        }
                                    }
                                }
                            }
                        }

                        resolved_expr.unwrap_or_else(|| {
                            // Generate a pending capability call for synthesis queue
                            let cap_id =
                                generated_capability_id_from_description(&suggested_action);
                            format!(
                                r#"(call "pending/{}" {{:description "{}"}})"#,
                                cap_id,
                                suggested_action.replace('"', "\\\"")
                            )
                        })
                    }
                },
                None => {
                    // Generate a pending capability call for synthesis queue
                    let cap_id = generated_capability_id_from_description(&sub_intent.description);
                    format!(
                        r#"(call "pending/{}" {{:description "{}"}})"#,
                        cap_id,
                        sub_intent.description.replace('"', "\\\"")
                    )
                }
            };

            // DISABLED:             // Optionally annotate with grounded preview as a harmless let (no context writes)
            // DISABLED:             if let Some(grounded) = grounding_params.get(&format!("result_{}", intent_id)) {
            // DISABLED:                 let grounded = truncate_grounding(grounded);
            // DISABLED:                 let escaped = grounded.replace('"', "\\\"");
            // DISABLED:                 call_expr = format!(
            // DISABLED:                     "(let [_grounding_comment \"{}\"]\n  {})",
            // DISABLED:                     escaped, call_expr
            // DISABLED:                 );
            // DISABLED:             }

            bindings.push((var_name, call_expr));
        }

        // Post-process: detect schema mismatches and insert inline adapters
        // For each step with grounding data, check if downstream consumers expect a different type
        let mut adapter_mappings: HashMap<String, (String, String)> = HashMap::new();
        let logical_bindings: Vec<(String, String)> = bindings
            .iter()
            .filter(|(name, _)| !name.starts_with('_'))
            .cloned()
            .collect();

        for (idx, _) in sub_intents.iter().enumerate() {
            let intent_id = &intent_ids[idx];
            let sub_intent = &sub_intents[idx];
            let var_name = format!("step_{}", idx + 1);

            // Skip adapter generation if this step's dependencies already have adapters
            // This prevents redundant extraction: if step_1_data already extracted the list,
            // step_2 shouldn't try to extract again from the same source
            let has_adapted_dependency = sub_intent.dependencies.iter().any(|dep_idx| {
                let dep_var = format!("step_{}", dep_idx + 1);
                adapter_mappings.contains_key(&dep_var)
            });

            if has_adapted_dependency {
                log::debug!(
                    "   ⏭️ Skipping adapter for {} (dependency already adapted)",
                    var_name
                );
                continue;
            }

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
                                match r {
                                    ResolvedCapability::Remote { input_schema, .. } => {
                                        input_schema.as_ref()
                                    }
                                    ResolvedCapability::Local { input_schema, .. } => {
                                        input_schema.as_ref()
                                    }
                                    // Synthesized and BuiltIn types don't have input_schema
                                    _ => None,
                                }
                            });

                            // Try to narrow the schema to the *specific param* that references this producer.
                            // This matters for object-shaped inputs like {:data [...], :criteria ...} where
                            // the overall schema is "object" but the param schema is "array".
                            let producer_ref = format!("step_{}", idx);
                            let consumer_param_name =
                                consumer_intent.extracted_params.iter().find_map(|(k, v)| {
                                    if k.starts_with('_') {
                                        return None;
                                    }
                                    if v == &producer_ref {
                                        Some(k.clone())
                                    } else {
                                        None
                                    }
                                });

                            let mut synthetic_target_schema: Option<serde_json::Value> = None;
                            let target_param_schema: Option<&serde_json::Value> =
                                consumer_param_name.as_ref().and_then(|param| {
                                    consumer_schema
                                        .and_then(|s| s.get("properties"))
                                        .and_then(|props| props.get(param))
                                });

                            // If we don't have a formal schema (e.g. local capability), use a minimal
                            // hint based on common list-carrying param names.
                            let target_input_schema: Option<&serde_json::Value> =
                                if target_param_schema.is_some() {
                                    target_param_schema
                                } else if consumer_schema.is_some() {
                                    consumer_schema
                                } else if matches!(
                                    consumer_param_name.as_deref(),
                                    Some("data")
                                        | Some("items")
                                        | Some("rows")
                                        | Some("issues")
                                        | Some("results")
                                        | Some("records")
                                        | Some("entries")
                                ) {
                                    synthetic_target_schema = Some(json!({ "type": "array" }));
                                    synthetic_target_schema.as_ref()
                                } else if matches!(
                                    consumer_param_name.as_deref(),
                                    Some("perPage")
                                        | Some("per_page")
                                        | Some("page")
                                        | Some("pageSize")
                                        | Some("page_size")
                                        | Some("count")
                                        | Some("limit")
                                        | Some("offset")
                                        | Some("max")
                                        | Some("min")
                                        | Some("size")
                                        | Some("num")
                                        | Some("number")
                                ) {
                                    synthetic_target_schema = Some(json!({ "type": "number" }));
                                    synthetic_target_schema.as_ref()
                                } else {
                                    None
                                };

                            // Detect if we need an adapter
                            let bridge = SchemaBridge::detect(
                                None,
                                target_input_schema,
                                Some(&grounded_json),
                            );

                            let mut adapter_expr = None;
                            let mut bridge_desc = String::new();

                            if bridge.needs_adapter() {
                                adapter_expr = Some(bridge.generate_rtfs_expr(&var_name));
                                bridge_desc = bridge.description.clone();
                            } else if self.delegating_arbiter.is_some() {
                                // Fallback: try LLM if static bridge didn't find anything
                                if let Some(llm_adapter) = self
                                    .request_llm_adapter(
                                        &sub_intent.description,
                                        &grounded_json,
                                        grounding_params
                                            .get(&format!("rtfs_schema_{}", intent_id))
                                            .map(|s| s.as_str()),
                                        consumer_schema,
                                        &consumer_intent.description,
                                        &logical_bindings,
                                    )
                                    .await
                                {
                                    adapter_expr = Some(llm_adapter.replace("input", &var_name));
                                    bridge_desc = "LLM-generated adapter".to_string();
                                }
                            }

                            if let Some(expr) = adapter_expr {
                                let adapted_var = format!("{}_data", var_name);
                                adapter_mappings
                                    .insert(var_name.clone(), (adapted_var.clone(), expr.clone()));

                                log::debug!(
                                    "🔌 Inserting adapter for {}: {} → {}",
                                    var_name,
                                    bridge_desc,
                                    expr
                                );
                                ccos_println!(
                                    "   🔌 Adapter: {} → {} ({})",
                                    var_name,
                                    adapted_var,
                                    bridge_desc
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
                if let Some((adapted_var, adapter_expr)) = adapter_mappings.get(var_name) {
                    new_bindings.push((adapted_var.clone(), adapter_expr.clone()));
                }
            }

            // Update call expressions to use adapted variables
            bindings = new_bindings
                .into_iter()
                .map(|(name, mut expr)| {
                    // Replace references to original vars with adapted vars in call expressions
                    for (orig, (adapted, adapter_expr)) in &adapter_mappings {
                        // Only replace in argument positions, not in the variable binding itself
                        if !name.ends_with("_data") {
                            // If the adapter is a simple field extraction like `(get step_1 :issues)`,
                            // replace the *whole extraction expression* with the adapted var.
                            // This prevents generating invalid expressions like `(get step_1_data :issues)`.
                            if let Ok(re) =
                                regex::Regex::new(r"^\(get\s+([^\s\)]+)\s+(:[^\s\)]+)\)$")
                            {
                                if let Some(caps) = re.captures(adapter_expr) {
                                    if let (Some(src_var), Some(keyword)) =
                                        (caps.get(1), caps.get(2))
                                    {
                                        if src_var.as_str() == orig {
                                            let pattern = format!(
                                                r"\(\s*get\s+{}\s+{}\s*\)",
                                                regex::escape(orig),
                                                regex::escape(keyword.as_str())
                                            );
                                            if let Ok(expr_re) = regex::Regex::new(&pattern) {
                                                expr = expr_re
                                                    .replace_all(&expr, adapted.as_str())
                                                    .to_string();
                                            }
                                        }
                                    }
                                }
                            }

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

        // Proactive validation: check for type mismatches and auto-repair
        let var_schemas: std::collections::HashMap<String, String> = grounding_params
            .iter()
            .filter_map(|(k, v)| {
                if k.starts_with("rtfs_schema_") {
                    // Extract var name from "rtfs_schema_{intent_id}"
                    // Map intent_id to step_N variable
                    // For simplicity, check if any binding contains a vector schema
                    Some((k.replace("rtfs_schema_", "step_"), v.clone()))
                } else {
                    None
                }
            })
            .collect();

        if let Some(repaired) = super::repair_rules::validate_get_expression(&expr, &var_schemas) {
            log::info!("[generate_rtfs_plan] Proactive repair applied: type mismatch corrected");
            return Ok(repaired);
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
        // Check for RTFS step variable references like "step_0", "step_1"
        // These are 0-indexed from LLM but need to map to actual binding names
        lazy_static::lazy_static! {
            static ref RTFS_STEP_VAR: Regex = Regex::new(r"^step_(\d+)$").unwrap();
            // Match any expression starting with ( that contains step_N anywhere inside
            // This handles nested cases like (get (nth step_2 0) :number)
            static ref RTFS_STEP_IN_EXPR: Regex = Regex::new(r"step_\d+").unwrap();
            static ref STEP_REF: Regex = Regex::new(r"\{\{step(\d+)\.(result|output)\}\}").unwrap();
        }

        // Handle pure RTFS step variable: "step_0" -> step_1 (unquoted)
        if let Some(captures) = RTFS_STEP_VAR.captures(value) {
            if let Some(idx_str) = captures.get(1) {
                if let Ok(idx) = idx_str.as_str().parse::<usize>() {
                    if idx < previous_bindings.len() {
                        let var_name = &previous_bindings[idx].0;
                        return format!(":{} {}", key, var_name);
                    }
                }
            }
        }

        // Handle RTFS expressions like "(get step_0 :issues)", "(nth step_0 0)",
        // or nested ones like "(get (nth step_2 0) :number)"
        // Replace step_N with actual binding name and output unquoted
        if value.starts_with('(') && value.ends_with(')') && RTFS_STEP_IN_EXPR.is_match(value) {
            let mut expr = value.to_string();
            // Replace all step_N references with actual binding names
            for (i, (binding_name, _)) in previous_bindings.iter().enumerate() {
                let pattern = format!("step_{}", i);
                expr = expr.replace(&pattern, binding_name);
            }
            return format!(":{} {}", key, expr);
        }

        // Check for LLM-generated step references like {{step0.result}} (legacy handlebars)
        // The LLM sometimes generates 0-based step indices in handlebars syntax
        // We need to convert these to actual variable names (step_1, step_2, etc.)
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
        grounding_params: Option<&HashMap<String, String>>,
        all_intent_ids: &[String],
    ) -> String {
        let capability_id = resolved_capability.capability_id().unwrap_or("unknown");
        let resolved_args = resolved_capability.arguments().unwrap();

        // Merge resolved arguments with sub_intent.extracted_params as fallback
        // This ensures LLM-provided params reach the RTFS plan even if resolution
        // didn't copy them (common when capability is found by ID match but params weren't copied)
        let mut arguments: std::collections::HashMap<String, String> = sub_intent
            .extracted_params
            .iter()
            .filter(|(k, _)| !k.starts_with('_')) // Skip internal params like _suggested_tool
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect();
        // Override with resolved args (they take precedence)
        for (k, v) in resolved_args {
            arguments.insert(k.clone(), v.clone());
        }

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
        for (key, value) in &arguments {
            // Normalize parameter name (e.g. repository -> repo) at the start
            let key = normalize_param_name(key);
            let key = key.as_str();

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
                static ref STEP_REF: Regex = Regex::new(r"\{\{step(\d+)\.(result|output)\}\}").unwrap();
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

                        // IMPORTANT: Don't inject if the LLM already provided this param name
                        // This prevents duplicate keys like `:data "$.path" :data step_1`
                        let param_name_already_exists = arguments.contains_key(&param_name);

                        if !param_name_already_exists {
                            // If safe-exec inferred the dependency's RTFS schema and the consumer
                            // has a JSON input schema, use deterministic cardinality hinting to
                            // decide whether to wrap this call in a (map ...) over the dependency.
                            if let (Some(grounding_params), Some(consumer_schema)) =
                                (grounding_params, input_schema)
                            {
                                if dep_idx < all_intent_ids.len() {
                                    let dep_intent_id = &all_intent_ids[dep_idx];
                                    if let Some(source_rtfs_schema) = grounding_params
                                        .get(&format!("rtfs_schema_{}", dep_intent_id))
                                    {
                                        use crate::utils::schema_cardinality::{
                                            cardinality_action, CardinalityAction,
                                        };

                                        if cardinality_action(
                                            source_rtfs_schema,
                                            consumer_schema,
                                            &param_name,
                                        ) == CardinalityAction::Map
                                        {
                                            // Build an inner call that consumes a scalar `item`.
                                            let mut mapped_args = args_parts.clone();
                                            mapped_args.push(format!(":{} item", param_name));

                                            let inner_call = if mapped_args.is_empty() {
                                                format!(r#"(call "{}" {{}})"#, capability_id)
                                            } else {
                                                format!(
                                                    r#"(call "{}" {{{}}})"#,
                                                    capability_id,
                                                    mapped_args.join(" ")
                                                )
                                            };

                                            return format!(
                                                "(map (fn [item] {}) {})",
                                                inner_call, dep_var
                                            );
                                        }
                                    }
                                }
                            }

                            args_parts.push(format!(":{} {}", param_name, dep_var));
                        }
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
        step_results: &HashMap<usize, rtfs::runtime::values::Value>,
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
            IntentType::UserInput { ref prompt_topic } => {
                // Phase 2: UserInput Grounding Mock
                // Use explicit _grounding_sample provided by LLM if available
                let mock_val = sub_intent
                    .extracted_params
                    .get("_grounding_sample")
                    .cloned()
                    .unwrap_or_else(|| {
                        format!("<sample-{}>", prompt_topic.to_lowercase().replace(' ', "-"))
                    });

                log::info!(
                    "[Grounding] Providing mock for user_input '{}': {}",
                    prompt_topic,
                    mock_val
                );
                return Ok(Some(rtfs::runtime::values::Value::String(mock_val)));
            }
            _ => {
                log::debug!(
                    "Safe exec skipped for {} (intent type not eligible)",
                    cap_id
                );
                return Ok(None);
            }
        }

        // Build params from both sources (matching generate_call_expr logic):
        // 1. Start with sub_intent.extracted_params
        // 2. Override with resolved_capability.arguments() (takes precedence)
        let mut params: std::collections::HashMap<String, String> = sub_intent
            .extracted_params
            .iter()
            .filter(|(k, _)| !k.starts_with('_')) // Skip internal params
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect();

        // Merge resolved arguments (they take precedence)
        if let Some(resolved_args) = resolved.arguments() {
            for (k, v) in resolved_args {
                params.insert(k.clone(), v.clone());
            }
        }

        // Thread previous_result if present
        if let Some(prev) = previous_result {
            if let Ok(prev_json) = rtfs_value_to_json(prev) {
                if let Ok(s) = serde_json::to_string(&prev_json) {
                    params.insert("_previous_result".to_string(), s);
                }
            }
        }

        // Phase 2: Resolve 'step_N' references in params
        for value in params.values_mut() {
            if value.starts_with("step_") {
                if let Ok(idx) = value["step_".len()..].parse::<usize>() {
                    if let Some(res) = step_results.get(&idx) {
                        if let Ok(json) = rtfs_value_to_json(res) {
                            // If it's a string, use it directly, otherwise JSON stringify
                            if let Some(s) = json.as_str() {
                                *value = s.to_string();
                            } else if let Ok(s) = serde_json::to_string(&json) {
                                *value = s;
                            }
                        }
                    }
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
        // Track skipped/failed steps to cascade skip to dependents
        let mut skipped_steps: std::collections::HashSet<usize> = std::collections::HashSet::new();

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

            // Check if any dependency was skipped - if so, skip this step too
            let has_skipped_dependency = sub_intent
                .dependencies
                .iter()
                .any(|dep_idx| skipped_steps.contains(dep_idx));

            ccos_println!(
                "\n▶️  Step {}/{}: {}",
                step_idx + 1,
                sub_intents.len(),
                sub_intent.description
            );
            ccos_println!("   ├─ Intent ID: {}", intent_id);
            ccos_println!("   ├─ Capability: {}", cap_id);
            ccos_println!("   ├─ Dependencies: {:?}", sub_intent.dependencies);

            if has_skipped_dependency {
                ccos_println!("   ╰─ ⏭️  Skipped (dependency was skipped/failed)");
                skipped_steps.insert(step_idx);
                continue;
            }

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

            if previous_result.is_some() {
                ccos_println!("   ├─ Input: Received _previous_result from upstream");
            }

            // Execute the step with dependency data
            match self
                .maybe_execute_and_ground(
                    executor,
                    sub_intent,
                    resolved,
                    previous_result,
                    &step_results,
                )
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

                    // Store inferred RTFS schema (from safe-exec result) for schema-guided adapter synthesis
                    let inferred_rtfs_schema =
                        crate::synthesis::introspection::schema_inferrer::infer_schema_from_value(
                            &result,
                        );
                    grounding_params
                        .insert(format!("rtfs_schema_{}", intent_id), inferred_rtfs_schema);

                    // Also store raw JSON for adapter detection
                    if let Ok(raw_json) = rtfs_value_to_json(&result) {
                        if let Ok(raw_str) = serde_json::to_string(&raw_json) {
                            grounding_params.insert(format!("raw_result_{}", intent_id), raw_str);
                        }
                    }
                }
                Ok(None) => {
                    ccos_println!("   ╰─ ⏭️  Skipped (Not safe to execute or no manifest)");
                    skipped_steps.insert(step_idx);
                }
                Err(e) => {
                    ccos_println!("   ╰─ ❌ Error: {}", e);
                    skipped_steps.insert(step_idx);
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

    /// Request an LLM-generated RTFS adapter between two steps
    async fn request_llm_adapter(
        &self,
        source_desc: &str,
        source_data: &serde_json::Value,
        source_rtfs_schema: Option<&str>,
        target_schema: Option<&serde_json::Value>,
        target_desc: &str,
        bindings_context: &[(String, String)], // (var_name, expression)
    ) -> Option<String> {
        let arbiter = self.delegating_arbiter.as_ref()?;

        let prompt = build_llm_adapter_prompt(
            source_desc,
            source_data,
            source_rtfs_schema,
            target_schema,
            target_desc,
            bindings_context,
        );

        log::debug!(
            "[request_llm_adapter] Requesting adapter with prompt: {}",
            prompt
        );
        let response = arbiter.generate_raw_text(&prompt).await.ok()?;
        let mut adapter = response.trim().to_string();

        if adapter.is_empty() || (!adapter.starts_with('(') && !adapter.starts_with('[')) {
            log::warn!(
                "[request_llm_adapter] LLM returned invalid adapter: {}",
                adapter
            );
            return None;
        }

        // Reject bare function definitions - adapter must be an expression, not a fn
        if adapter.starts_with("(fn ") {
            log::warn!(
                "[request_llm_adapter] LLM returned bare function instead of expression: {}",
                adapter
            );
            return None;
        }

        // Verify the adapter references 'input' - otherwise it can't transform the data
        if !adapter.contains("input") {
            log::warn!(
                "[request_llm_adapter] LLM adapter doesn't reference 'input': {}",
                adapter
            );
            return None;
        }

        // Reject call expressions - adapters should transform data, not call capabilities
        // This prevents LLM hallucinations where it tries to re-invoke APIs instead of
        // transforming existing data
        if adapter.contains("(call ") {
            log::warn!(
                "[request_llm_adapter] LLM returned capability call instead of data adapter: {}",
                adapter
            );
            return None;
        }

        // Sanitize common LLM patterns that RTFS doesn't support
        adapter = sanitize_llm_rtfs(&adapter);

        log::info!("[request_llm_adapter] LLM generated adapter: {}", adapter);
        Some(adapter)
    }

    /// Save synthesized capabilities for reuse by future planners.
    ///
    /// When plan decomposition generates inline RTFS code for transformations
    /// (marked as `generated/` or `Synthesized` resolutions), this function
    /// extracts and saves them as proper capability definitions.
    fn save_synthesized_capabilities(
        &self,
        sub_intents: &[SubIntent],
        resolutions: &HashMap<String, ResolvedCapability>,
        goal: &str,
    ) {
        let storage = SynthesizedCapabilityStorage::new();

        for (intent_id, resolution) in resolutions {
            // Extract synthesized RTFS code from resolution
            let (_cap_id, rtfs_code, description) = match resolution {
                ResolvedCapability::Synthesized {
                    capability_id,
                    rtfs_code,
                    ..
                } => {
                    // Find the matching sub-intent for description
                    let desc = sub_intents
                        .iter()
                        .find(|si| intent_id.contains(&si.description))
                        .map(|si| si.description.clone())
                        .unwrap_or_else(|| capability_id.clone());
                    (capability_id.clone(), rtfs_code.clone(), desc)
                }
                ResolvedCapability::NeedsReferral {
                    suggested_action, ..
                } if suggested_action.contains("Synth-or-enqueue") => {
                    // Find the matching sub-intent
                    if let Some(si) = sub_intents.iter().find(|si| {
                        intent_id.ends_with(&format!("{:016x}", fnv1a64(&si.description)))
                    }) {
                        // Check if there's inline RTFS in the extracted params
                        if let Some(inline_code) = si.extracted_params.get("_inline_rtfs") {
                            let cap_id = generated_capability_id_from_description(&si.description);
                            (cap_id, inline_code.clone(), si.description.clone())
                        } else {
                            continue;
                        }
                    } else {
                        continue;
                    }
                }
                _ => continue,
            };

            // Create the synthesized capability first to get its canonical ID
            let capability = SynthesizedCapability::new(&description, &rtfs_code)
                .with_metadata("source_goal", goal)
                .with_metadata("source_intent_id", intent_id);

            // Skip if capability already exists (use capability.id for consistent dedup check)
            if storage.exists(&capability.id) {
                log::debug!(
                    "Synthesized capability already exists, skipping: {}",
                    capability.id
                );
                continue;
            }

            match storage.save(&capability) {
                Ok(path) => {
                    ccos_println!(
                        "✨ Synthesized capability saved for reuse: {} → {}",
                        capability.id,
                        path.display()
                    );
                }
                Err(e) => {
                    log::warn!(
                        "Failed to save synthesized capability '{}': {}",
                        capability.id,
                        e
                    );
                }
            }
        }
    }
}

fn build_llm_adapter_prompt(
    _source_desc: &str,
    source_data: &serde_json::Value,
    source_rtfs_schema: Option<&str>,
    target_schema: Option<&serde_json::Value>,
    target_desc: &str,
    bindings_context: &[(String, String)],
) -> String {
    let source_preview =
        truncate_grounding(&serde_json::to_string(source_data).unwrap_or_default());

    let source_rtfs_schema_str = source_rtfs_schema
        .map(truncate_grounding)
        .unwrap_or_else(|| "Unknown".to_string());

    let target_schema_str = target_schema
        .map(|s| serde_json::to_string_pretty(s).unwrap_or_default())
        .unwrap_or_else(|| "Unknown (match data naturally)".to_string());

    // Build bindings context string to show LLM what variables already exist
    let bindings_str = if bindings_context.is_empty() {
        "None (you have access only to 'input')".to_string()
    } else {
        bindings_context
            .iter()
            .map(|(var, expr)| {
                let truncated = if expr.len() > 100 {
                    format!("{}...", &expr[..97])
                } else {
                    expr.clone()
                };
                format!("  - {}: {}", var, truncated)
            })
            .collect::<Vec<_>>()
            .join("\n")
    };

    let input_type_desc = match source_data {
        serde_json::Value::Object(_) => "Map (Object)",
        serde_json::Value::Array(_) => "List (Vector)",
        _ => "Scalar",
    };

    format!(
        "Synthesize a small, pure RTFS expression to achieve this goal: '{}'.\n\n\
         INPUT DATA TYPE: {}\n\
         EXISTING BINDINGS (already defined - DO NOT re-extract if already done!):\n{}\n\n\
         Input data sample: {}\n\n\
         SOURCE REFINED SCHEMA (RTFS type inferred from safe-exec): {}\n\n\
         TARGET INPUT SCHEMA REQUIREMENT: {}\n\n\
         RULES:\n\
         1. Use 'input' as the variable for the upstream data.\n\
         2. RETURN ONLY the expression. No markdown, no commentary.\n\
         3. NO Clojure namespaces (e.g. avoid 'str/split', just use 'split').\n\
         4. Regex IS supported. Use: (re-matches pattern text), (re-find pattern text), (re-seq pattern text).\n\
            Pattern syntax follows Rust regex. Example: (re-matches \"[A-Z]+\" title)\n\
         5. RTFS 'get' syntax: (get map :key) NOT (get :key map) - map comes FIRST.\n\
         6. Use RTFS functions: 'map', 'filter', 'get', 'assoc', 'str', 'split', 'substring', 'join', 're-matches', 're-find', 're-seq', 'group-by'.\n\
         7. IMPORTANT: If INPUT DATA TYPE is 'Map' but you need to iterate, EXTRACT the array field first (e.g. (get input :issues)).\n\
         8. IMPORTANT: If INPUT DATA TYPE is 'List', work on it directly with 'map' or 'filter'.\n\
         9. FORBIDDEN: Do NOT use (call ...). This is a DATA ADAPTER, not a capability invocation. Transform existing data only.\n\
         10. FORBIDDEN: Do NOT re-fetch data. The data is already in 'input'. Just transform it.\n\n\
         GOOD examples:\n\
           - (get input :issues)\n\
           - (map (fn [x] (get x :title)) input)\n\
           - (filter (fn [x] (= (get x :state) \"open\")) (get input :issues))\n\n\
         BAD examples (NEVER do this):\n\
           - (call \"github-mcp.list_issues\" {{...}})  ← FORBIDDEN\n\
           - (call :capability.id {{...}})              ← FORBIDDEN",
        target_desc,
        input_type_desc,
        bindings_str,
        source_preview,
        source_rtfs_schema_str,
        target_schema_str
    )
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
    fn test_adapter_rewrite_replaces_get_expression() {
        let mut adapter_mappings: HashMap<String, (String, String)> = HashMap::new();
        adapter_mappings.insert(
            "step_1".to_string(),
            (
                "step_1_data".to_string(),
                "(get step_1 :issues)".to_string(),
            ),
        );

        let name = "step_2";
        let mut expr = "(map (fn [x] x) (get step_1 :issues))".to_string();

        for (orig, (adapted, adapter_expr)) in &adapter_mappings {
            if !name.ends_with("_data") {
                if let Ok(re) = regex::Regex::new(r"^\(get\s+([^\s\)]+)\s+(:[^\s\)]+)\)$") {
                    if let Some(caps) = re.captures(adapter_expr) {
                        if let (Some(src_var), Some(keyword)) = (caps.get(1), caps.get(2)) {
                            if src_var.as_str() == orig {
                                let pattern = format!(
                                    r"\(\s*get\s+{}\s+{}\s*\)",
                                    regex::escape(orig),
                                    regex::escape(keyword.as_str())
                                );
                                if let Ok(expr_re) = regex::Regex::new(&pattern) {
                                    expr = expr_re.replace_all(&expr, adapted.as_str()).to_string();
                                }
                            }
                        }
                    }
                }

                expr = expr.replace(&format!(" {}}}", orig), &format!(" {}}}", adapted));
                expr = expr.replace(&format!(" {} ", orig), &format!(" {} ", adapted));
            }
        }

        assert!(expr.contains(" step_1_data"));
        assert!(!expr.contains("(get step_1_data :issues)"));
        assert!(!expr.contains("(get step_1 :issues)"));
    }

    #[test]
    fn test_build_llm_adapter_prompt_includes_regex_support() {
        let prompt = build_llm_adapter_prompt(
            "source",
            &serde_json::json!({"issues": []}),
            None,
            None,
            "target",
            &[],
        );

        // Regex IS supported in the adapter prompt
        assert!(prompt.contains("Regex IS supported"));
        assert!(prompt.contains("re-matches"));
        assert!(prompt.contains("re-find"));
        assert!(prompt.contains("re-seq"));
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

        let expr = planner.generate_call_expr(
            &resolved,
            &sub_intent,
            &[],
            &[sub_intent.clone()],
            None,
            &[],
        );

        assert!(expr.contains("mcp.github.list_issues"));
        assert!(expr.contains(":owner"));
        assert!(expr.contains("mandubian"));
    }

    #[test]
    fn test_generate_call_expr_wraps_map_for_collection_dependency() {
        let intent_graph = Arc::new(Mutex::new(
            IntentGraph::with_config(IntentGraphConfig::with_in_memory_storage()).unwrap(),
        ));
        let catalog = Arc::new(MockCatalog);
        let planner = ModularPlanner::new(
            Box::new(PatternDecomposition::new()),
            Box::new(CatalogResolution::new(catalog)),
            intent_graph,
        );

        // Producer (idx 0) is a prior step whose safe-exec schema we treat as a collection.
        let producer = SubIntent::new(
            "producer",
            IntentType::ApiCall {
                action: crate::planner::modular_planner::types::ApiAction::List,
            },
        );

        // Consumer depends on producer and has a schema where the inferred injected param "data"
        // is a scalar (string), which should trigger a map wrapper.
        let consumer = SubIntent::new(
            "consumer",
            IntentType::ApiCall {
                action: crate::planner::modular_planner::types::ApiAction::Get,
            },
        )
        .with_dependencies(vec![0]);

        let consumer_schema = serde_json::json!({
            "type": "object",
            "properties": {
                "data": {"type": "string"}
            }
        });

        let resolved = ResolvedCapability::Remote {
            capability_id: "mcp.test.consume".to_string(),
            server_url: "http://test".to_string(),
            arguments: HashMap::new(),
            input_schema: Some(consumer_schema),
            confidence: 1.0,
        };

        let mut grounding_params: HashMap<String, String> = HashMap::new();
        grounding_params.insert(
            "rtfs_schema_intent0".to_string(),
            "[:vector [:map {:x :string}]]".to_string(),
        );

        let expr = planner.generate_call_expr(
            &resolved,
            &consumer,
            &[(
                "step_1".to_string(),
                "(call \"mcp.test.produce\" {})".to_string(),
            )],
            &[producer, consumer.clone()],
            Some(&grounding_params),
            &["intent0".to_string(), "intent1".to_string()],
        );

        assert!(expr.starts_with("(map (fn [item]"));
        assert!(expr.contains("mcp.test.consume"));
        assert!(expr.contains(":data item"));
        assert!(expr.contains("step_1"));
    }

    #[test]
    fn test_build_llm_adapter_prompt_includes_source_schema() {
        let source_data = serde_json::json!([{ "title": "hello" }]);
        let prompt = build_llm_adapter_prompt(
            "source",
            &source_data,
            Some("[:vector [:map {:title :string}]]"),
            None,
            "target",
            &[],
        );

        assert!(prompt.contains("SOURCE REFINED SCHEMA"));
        assert!(prompt.contains("[:vector"));
    }
}
