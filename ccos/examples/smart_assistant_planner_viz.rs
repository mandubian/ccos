use std::borrow::Cow;
use std::cmp::{min, Ordering};
use std::collections::{BTreeMap, HashMap, HashSet};
use std::error::Error;
use std::fmt::Write as _;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::SystemTime;

use clap::{ArgAction, Parser};
use crossterm::style::Stylize;
use futures::FutureExt;
use rtfs::ast::{Keyword, MapKey};
use rtfs::config::profile_selection::expand_profiles;
use rtfs::config::types::{AgentConfig, LlmProfile};
use rtfs::runtime::error::{RuntimeError, RuntimeResult};
use rtfs::runtime::security::RuntimeContext;
use rtfs::runtime::values::Value;

use ccos::arbiter::arbiter_config::LlmProviderType;
use ccos::arbiter::prompt::{FilePromptStore, PromptManager};
use ccos::arbiter::ArbiterEngine;
use ccos::capabilities::{MCPSessionHandler, SessionPoolManager};
use ccos::capability_marketplace::CapabilityMarketplace;
use ccos::catalog::{CatalogEntryKind, CatalogFilter, CatalogService};
use ccos::causal_chain::CausalChain;
use ccos::examples_common::capability_helpers::{
    count_token_matches, load_override_parameters, minimum_token_matches,
    preload_discovered_capabilities, score_manifest_against_tokens, tokenize_identifier,
};
use ccos::intent_graph::config::IntentGraphConfig;
use ccos::planner::coverage::{
    CoverageStatus, DefaultGoalCoverageAnalyzer, GoalCoverageAnalyzer, PlanStepSummary,
};
use ccos::planner::menu::{menu_entry_from_manifest, CapabilityMenuEntry};
use ccos::planner::resolution::{
    CapabilityProvisionAction, CapabilityProvisionFn, PendingCapabilityRequest,
    RequirementResolutionOutcome, RequirementResolver, ResolvedCapabilityInfo,
};
use ccos::planner::signals::{
    CapabilityProvisionSource, GoalRequirement, GoalRequirementKind, GoalSignalSource, GoalSignals,
    RequirementPriority, RequirementReadiness,
};
use ccos::synthesis::missing_capability_resolver::{
    MissingCapabilityRequest, ResolutionEvent, ResolutionObserver, ResolutionResult,
};
use ccos::synthesis::schema_serializer::type_expr_to_rtfs_pretty;
use ccos::types::{Action, ActionType, ExecutionResult, Intent, Plan, PlanBody};
use ccos::{PlanAutoRepairOptions, CCOS};
use once_cell::sync::Lazy;
use regex::Regex;
use serde::{Deserialize, Serialize};
use serde_json::{json, Map as JsonMap, Value as JsonValue};
use uuid::Uuid;

mod planner_viz_common;
use planner_viz_common::{load_agent_config, print_architecture_summary};

static PLAN_CONVERSION_PROMPT_MANAGER: Lazy<PromptManager<FilePromptStore>> = Lazy::new(|| {
    let base_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("assets/prompts/arbiter");
    PromptManager::new(FilePromptStore::new(&base_dir))
});

static RTFS_CODE_BLOCK_RE: Lazy<Regex> =
    Lazy::new(|| Regex::new(r"```(?:rtfs|lisp|scheme)?\s*([\s\S]*?)```").unwrap());

const GENERIC_OPERATION_HINTS: [&str; 8] = [
    "list",
    "search",
    "fetch",
    "filter",
    "summarize",
    "analyze",
    "classify",
    "compare",
];
const MAX_DISCOVERY_HINT_TOKENS: usize = 8;
const MAX_COMBINATION_TOKENS: usize = 4;

#[derive(Debug, Serialize)]
struct PlanStepJsonSerialized {
    id: String,
    name: String,
    capability_id: String,
    inputs: HashMap<String, serde_json::Value>,
    outputs: Vec<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    notes: Option<String>,
}

#[derive(Parser, Debug)]
#[command(
    author,
    version,
    about = "End-to-end smart assistant planner with capability discovery timeline",
    long_about = None
)]
struct Args {
    /// Path to agent configuration (TOML/JSON)
    #[arg(long, default_value = "config/agent_config.toml")]
    config: String,

    /// Optional LLM profile name defined in agent_config
    #[arg(long)]
    profile: Option<String>,

    /// Natural language goal to solve
    #[arg(long)]
    goal: Option<String>,

    /// Expand all events or specific stages (e.g. --show mcp --show result)
    #[arg(long = "show", action = ArgAction::Append)]
    show_filters: Vec<String>,

    /// Print raw prompts/responses during LLM interactions
    #[arg(long, default_value_t = false)]
    debug_prompts: bool,

    /// Stream resolver's own logs (noisy). Off by default; use --trace to see them.
    #[arg(long, default_value_t = false)]
    trace: bool,

    /// Maximum number of plan synthesis attempts before giving up
    #[arg(long, default_value_t = 3)]
    max_attempts: usize,

    /// Execute the synthesized plan via the orchestrator once generated
    #[arg(long, default_value_t = false)]
    execute_plan: bool,

    /// When provided, attempt automatic plan repair using LLM-driven auto-repair flow
    #[arg(long, default_value_t = false)]
    auto_repair: bool,

    /// Export the final JSON plan steps to a file
    #[arg(long)]
    export_plan_json: Option<String>,

    /// Export the final RTFS plan body to a file
    #[arg(long)]
    export_plan_rtfs: Option<String>,

    /// Provide additional plan input bindings (key=value). Repeat flag to add multiple inputs.
    #[arg(long = "plan-input", value_parser = parse_plan_input, action = ArgAction::Append)]
    plan_inputs: Vec<(String, String)>,
}

fn parse_plan_input(raw: &str) -> Result<(String, String), String> {
    let mut parts = raw.splitn(2, '=');
    let key = parts
        .next()
        .map(str::trim)
        .filter(|k| !k.is_empty())
        .ok_or_else(|| "plan-input entries must use key=value syntax".to_string())?;
    let value = parts
        .next()
        .map(str::trim)
        .filter(|v| !v.is_empty())
        .ok_or_else(|| "plan-input entries require a non-empty value".to_string())?;
    Ok((key.to_string(), value.to_string()))
}

#[allow(dead_code)]
#[derive(Default)]
struct MultiTimelineObserver {
    events: Mutex<Vec<ResolutionEvent>>,
}

impl MultiTimelineObserver {
    #[allow(dead_code)]
    fn drain_grouped(&self) -> Vec<(String, Vec<ResolutionEvent>)> {
        let mut guard = self.events.lock().unwrap();
        if guard.is_empty() {
            return Vec::new();
        }

        let mut order = Vec::new();
        let mut map: HashMap<String, Vec<ResolutionEvent>> = HashMap::new();

        for event in guard.drain(..) {
            if !map.contains_key(&event.capability_id) {
                order.push(event.capability_id.clone());
            }
            map.entry(event.capability_id.clone())
                .or_insert_with(Vec::new)
                .push(event);
        }

        let mut grouped = Vec::with_capacity(order.len());
        for capability_id in order {
            if let Some(events) = map.remove(&capability_id) {
                grouped.push((capability_id, events));
            }
        }
        grouped
    }
}

impl ResolutionObserver for MultiTimelineObserver {
    fn on_event(&self, event: ResolutionEvent) {
        if let Ok(mut guard) = self.events.lock() {
            guard.push(event);
        }
    }
}

#[allow(dead_code)]
#[derive(Debug, Default)]
struct DisplayFilter {
    show_all: bool,
    tokens: HashSet<String>,
}

impl DisplayFilter {
    #[allow(dead_code)]
    fn from_args(args: &[String]) -> Self {
        if args.iter().any(|value| value.eq_ignore_ascii_case("all")) {
            return Self {
                show_all: true,
                tokens: HashSet::new(),
            };
        }

        let tokens = args
            .iter()
            .map(|value| value.to_lowercase())
            .collect::<HashSet<_>>();

        Self {
            show_all: false,
            tokens,
        }
    }

    fn should_expand(&self, stage: &str) -> bool {
        if self.show_all {
            return true;
        }

        let stage_lower = stage.to_lowercase();
        if self.tokens.contains(&stage_lower) {
            return true;
        }

        self.tokens.iter().any(|token| stage_lower.contains(token))
    }
}

#[allow(dead_code)]
#[derive(Clone)]
struct StageDescriptor {
    label: Cow<'static, str>,
    depth: usize,
}

#[derive(Debug, Clone)]
struct PlanStep {
    id: String,
    name: String,
    capability_id: String,
    inputs: Vec<(String, StepInputBinding)>,
    outputs: Vec<StepOutput>,
    notes: Option<String>,
}

#[derive(Debug, Clone)]
enum StepInputBinding {
    Literal(String),
    Variable(String),
    StepOutput {
        step_id: String,
        output: String,
    },
    /// RTFS code to be embedded directly (for function parameters)
    RtfsCode(String),
}

#[derive(Debug, Clone)]
struct StepOutput {
    alias: String,
    source: String,
}

#[derive(Debug, Deserialize)]
struct PlanStepJson {
    id: String,
    #[serde(default)]
    name: String,
    #[serde(rename = "capability_id")]
    capability_id: String,
    inputs: HashMap<String, JsonValue>,
    outputs: Vec<PlanStepOutputJson>,
    #[serde(default)]
    notes: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(untagged)]
enum PlanStepOutputJson {
    Name(String),
    Mapping {
        #[serde(default)]
        name: String,
        #[serde(default)]
        output: Option<String>,
        #[serde(default)]
        field: Option<String>,
    },
}

#[derive(Debug, Clone)]
struct UnknownCapabilityUsage {
    step_id: String,
    step_name: Option<String>,
    capability_id: String,
    notes: Option<String>,
    outputs: Vec<String>,
}

#[derive(Debug, Default)]
struct PlanValidationOutcome {
    schema_errors: Vec<String>,
    unknown_capabilities: Vec<UnknownCapabilityUsage>,
}

#[derive(Debug, Clone)]
enum CapabilityDiscoveryStatus {
    Synthesized,
    Discovered,
    PendingExternal,
    Failed,
}

#[derive(Debug, Clone)]
struct CapabilityDiscoveryEvent {
    capability_id: String,
    capability_name: Option<String>,
    status: CapabilityDiscoveryStatus,
    source: Option<String>,
    resolution_method: Option<String>,
    request_id: Option<String>,
    notes: Vec<String>,
}

impl CapabilityDiscoveryEvent {
    fn status_label(&self) -> &'static str {
        match self.status {
            CapabilityDiscoveryStatus::Synthesized => "Synthesized",
            CapabilityDiscoveryStatus::Discovered => "Discovered",
            CapabilityDiscoveryStatus::PendingExternal => "Pending",
            CapabilityDiscoveryStatus::Failed => "Failed",
        }
    }

    fn to_json(&self) -> serde_json::Value {
        json!({
            "capability_id": self.capability_id,
            "capability_name": self.capability_name,
            "status": self.status_label(),
            "source": self.source,
            "resolution_method": self.resolution_method,
            "request_id": self.request_id,
            "notes": self.notes,
        })
    }
}

#[derive(Clone)]
struct PlannerAuditRecorder {
    chain: Option<Arc<Mutex<CausalChain>>>,
    plan_id: String,
    intent_id: String,
}

impl PlannerAuditRecorder {
    fn new(
        chain: Option<Arc<Mutex<CausalChain>>>,
        plan_id: impl Into<String>,
        intent_id: impl Into<String>,
    ) -> Self {
        Self {
            chain,
            plan_id: plan_id.into(),
            intent_id: intent_id.into(),
        }
    }

    fn log_json(&self, stage: &str, payload: &serde_json::Value) {
        if let Ok(serialized) = serde_json::to_string(payload) {
            self.log_internal(stage, Some(serialized), None);
        } else {
            self.log_internal(
                stage,
                None,
                Some("⚠️ Failed to serialize payload for planner audit"),
            );
        }
    }

    fn log_text(&self, stage: &str, text: &str) {
        self.log_internal(stage, None, Some(text));
    }

    fn log_internal(&self, stage: &str, payload_json: Option<String>, note: Option<&str>) {
        let Some(chain) = &self.chain else {
            return;
        };

        let mut action = Action::new(
            ActionType::InternalStep,
            self.plan_id.clone(),
            self.intent_id.clone(),
        )
        .with_name(&format!("planner.{}", stage));
        action.metadata.insert(
            "planner_stage".to_string(),
            Value::String(stage.to_string()),
        );
        if let Some(payload) = payload_json {
            action
                .metadata
                .insert("payload_json".to_string(), Value::String(payload));
        }
        if let Some(text) = note {
            action
                .metadata
                .insert("note".to_string(), Value::String(text.to_string()));
        }

        if let Ok(mut guard) = chain.lock() {
            if let Err(err) = guard.append(&action) {
                eprintln!("⚠️  Failed to append planner audit action: {}", err);
            }
        }
    }
}

fn render_capability_discovery_summary(events: &[CapabilityDiscoveryEvent]) {
    if events.is_empty() {
        return;
    }

    println!("\n{}", "Capability Discovery Summary".bold().cyan());
    for event in events {
        let label = match event.status {
            CapabilityDiscoveryStatus::Synthesized => "🛠 synthesized".green(),
            CapabilityDiscoveryStatus::Discovered => "✨ discovered".green(),
            CapabilityDiscoveryStatus::PendingExternal => "⏳ pending".yellow(),
            CapabilityDiscoveryStatus::Failed => "❌ failed".red(),
        };
        let name = event
            .capability_name
            .as_ref()
            .map(|n| format!(" ({})", n))
            .unwrap_or_default();
        println!(
            "  {} {}{}",
            label,
            event.capability_id.as_str().bold(),
            name.dim()
        );
        if let Some(source) = &event.source {
            println!("     source: {}", source);
        }
        if let Some(method) = &event.resolution_method {
            println!("     method: {}", method);
        }
        if let Some(request_id) = &event.request_id {
            println!("     request id: {}", request_id);
        }
        for note in &event.notes {
            println!("     - {}", note);
        }
    }
}

fn discovery_events_to_json(events: &[CapabilityDiscoveryEvent]) -> serde_json::Value {
    serde_json::Value::Array(events.iter().map(|event| event.to_json()).collect())
}

fn serialize_plan_steps_for_logging(steps: &[PlanStep]) -> serde_json::Value {
    let mut serialized = Vec::with_capacity(steps.len());
    for step in steps {
        let mut inputs = JsonMap::new();
        for (key, binding) in &step.inputs {
            inputs.insert(key.clone(), binding_to_json(binding));
        }
        let outputs: Vec<JsonValue> = step.outputs.iter().map(step_output_to_json).collect();
        serialized.push(json!({
            "id": step.id,
            "name": step.name,
            "capability_id": step.capability_id,
            "inputs": inputs,
            "outputs": outputs,
            "notes": step.notes,
        }));
    }
    serde_json::Value::Array(serialized)
}

fn binding_to_json(binding: &StepInputBinding) -> serde_json::Value {
    match binding {
        StepInputBinding::Literal(value) => json!({ "literal": value }),
        StepInputBinding::Variable(name) => json!({ "var": name }),
        StepInputBinding::StepOutput { step_id, output } => {
            json!({ "step": step_id, "output": output })
        }
        StepInputBinding::RtfsCode(code) => json!({ "rtfs": code }),
    }
}

fn step_output_to_json(output: &StepOutput) -> JsonValue {
    let alias = output.alias.trim();
    let source = output.source.trim();
    if alias.is_empty() {
        JsonValue::String(String::new())
    } else if source.is_empty() || alias == source {
        JsonValue::String(alias.to_string())
    } else {
        let mut obj = JsonMap::with_capacity(2);
        obj.insert("name".to_string(), JsonValue::String(alias.to_string()));
        obj.insert("output".to_string(), JsonValue::String(source.to_string()));
        JsonValue::Object(obj)
    }
}

fn display_step_output(output: &StepOutput) -> String {
    if output.alias.trim().is_empty() {
        return String::new();
    }
    if output.source.trim().is_empty() || output.alias == output.source {
        output.alias.clone()
    } else {
        format!("{} ⇐ {}", output.alias, output.source)
    }
}

fn constraint_map_to_json(values: &HashMap<String, Value>) -> serde_json::Value {
    let mut map = JsonMap::new();
    for (key, value) in values {
        map.insert(
            key.clone(),
            serde_json::Value::String(value_to_string_repr(value)),
        );
    }
    serde_json::Value::Object(map)
}

fn write_text_file(path: &str, contents: &str) -> std::io::Result<()> {
    let file_path = Path::new(path);
    if let Some(parent) = file_path.parent() {
        if !parent.as_os_str().is_empty() {
            fs::create_dir_all(parent)?;
        }
    }
    fs::write(file_path, contents)
}

fn execution_result_to_json(result: &ExecutionResult) -> serde_json::Value {
    json!({
        "success": result.success,
        "value": value_to_string_repr(&result.value),
        "metadata": constraint_map_to_json(&result.metadata),
    })
}

async fn build_capability_menu_from_catalog(
    catalog: Arc<CatalogService>,
    marketplace: Arc<CapabilityMarketplace>,
    goal: &str,
    intent: &Intent,
    limit: usize,
) -> RuntimeResult<Vec<CapabilityMenuEntry>> {
    let query = build_catalog_query(goal, intent);
    let filter = CatalogFilter::for_kind(CatalogEntryKind::Capability);

    let mut raw_hits = catalog.search_semantic(&query, Some(&filter), limit * 2);
    if raw_hits.is_empty() {
        raw_hits = catalog.search_keyword(&query, Some(&filter), limit * 2);
    }

    let mut menu = Vec::new();
    for hit in raw_hits {
        if menu.len() >= limit {
            break;
        }
        // Filter out meta-capabilities: planner.* are internal planner capabilities
        // and ccos.* are system capabilities, both should not appear in execution plans
        let is_meta = hit.entry.id.starts_with("planner.") || hit.entry.id.starts_with("ccos.");
        let is_allowed_util = hit.entry.id == "ccos.echo" || hit.entry.id == "ccos.user.ask";
        if is_meta && !is_allowed_util {
            continue;
        }
        if let Some(manifest) = marketplace.get_capability(&hit.entry.id).await {
            let trimmed = manifest.id.trim();
            if trimmed.is_empty() || !trimmed.contains('.') {
                continue;
            }
            let mut entry = menu_entry_from_manifest(&manifest, Some(hit.score as f64));
            // Debug: log manifest input schema and extracted required/optional fields
            if let Some(schema) = &manifest.input_schema {
                eprintln!(
                    "DEBUG: manifest={} input_schema:\n{}",
                    manifest.id,
                    type_expr_to_rtfs_pretty(schema)
                );
            } else {
                eprintln!("DEBUG: manifest={} has no input_schema", manifest.id);
            }
            eprintln!(
                "DEBUG: manifest={} required_inputs={:?} optional_inputs={:?}",
                manifest.id, entry.required_inputs, entry.optional_inputs
            );
            apply_input_overrides(&mut entry);
            menu.push(entry);
        }
    }

    if menu.len() < limit {
        let tokens = tokenize_identifier(&query);
        if !tokens.is_empty() {
            let min_matches = minimum_token_matches(tokens.len());
            let required = min(min_matches, 2);
            let mut seen = menu
                .iter()
                .map(|entry| entry.id.clone())
                .collect::<HashSet<_>>();
            let mut matched_entries = Vec::new();
            let mut backup_entries = Vec::new();
            for manifest in marketplace.list_capabilities().await {
                // Filter out meta-capabilities: planner.* and ccos.* should not appear in execution plans
                if manifest.id.starts_with("planner.") || manifest.id.starts_with("ccos.") {
                    continue;
                }
                if seen.contains(&manifest.id) {
                    continue;
                }
                let trimmed = manifest.id.trim();
                if trimmed.is_empty() || !trimmed.contains('.') {
                    continue;
                }
                let score = score_manifest_against_tokens(&manifest, &tokens) as f64;
                let mut entry = menu_entry_from_manifest(&manifest, Some(score));
                if let Some(schema) = &manifest.input_schema {
                    eprintln!(
                        "DEBUG: manifest={} input_schema:\n{}",
                        manifest.id,
                        type_expr_to_rtfs_pretty(schema)
                    );
                } else {
                    eprintln!("DEBUG: manifest={} has no input_schema", manifest.id);
                }
                eprintln!(
                    "DEBUG: manifest={} required_inputs={:?} optional_inputs={:?}",
                    manifest.id, entry.required_inputs, entry.optional_inputs
                );
                apply_input_overrides(&mut entry);
                seen.insert(entry.id.clone());
                let matches = count_token_matches(&manifest, &tokens);
                if matches >= required && required > 0 {
                    matched_entries.push(entry);
                } else if matches > 0 || required == 0 {
                    matched_entries.push(entry);
                } else {
                    backup_entries.push(entry);
                }
            }
            matched_entries.sort_by(compare_entries_by_score_desc);
            backup_entries.sort_by(compare_entries_by_score_desc);

            let mut remaining = limit.saturating_sub(menu.len());
            for entry in matched_entries.into_iter() {
                if remaining == 0 {
                    break;
                }
                menu.push(entry);
                remaining -= 1;
            }
            if remaining > 0 {
                for entry in backup_entries.into_iter() {
                    if remaining == 0 {
                        break;
                    }
                    menu.push(entry);
                    remaining -= 1;
                }
            }
            menu.sort_by(compare_entries_by_score_desc);
            if menu.len() > limit {
                menu.truncate(limit);
            }
        }
    }

    // Final fallback: if menu is still empty, try to get any capabilities from marketplace
    // (bypassing catalog search - this handles the case where discovery found capabilities
    // but catalog search didn't match them)
    if menu.is_empty() {
        eprintln!(
            "⚠️ Catalog search returned no capabilities - falling back to marketplace listing"
        );

        let fallback_limit = limit.saturating_sub(menu.len());
        if fallback_limit > 0 {
            let fallback_tokens = ensure_goal_tokens(goal, intent);
            let derived_entries = goal_aligned_marketplace_fallbacks(
                Arc::clone(&marketplace),
                &fallback_tokens,
                fallback_limit,
            )
            .await;

            if !derived_entries.is_empty() {
                eprintln!(
                    "   ✅ Added {} fallback capability(ies) derived from goal context",
                    derived_entries.len()
                );
                for entry in derived_entries {
                    eprintln!("      ➕ {}", entry.id);
                    menu.push(entry);
                }
            }
        }

        // If still empty, try to get ANY capabilities from marketplace (last resort)
        if menu.is_empty() {
            eprintln!("⚠️ Trying to list all marketplace capabilities as last resort");
            let all_capabilities = marketplace.list_capabilities().await;
            eprintln!(
                "   Found {} total capability(ies) in marketplace",
                all_capabilities.len()
            );

            let mut filtered_count = 0;
            for manifest in all_capabilities {
                // Filter out meta-capabilities, but allow specific util capabilities like ccos.echo
                let is_meta = manifest.id.starts_with("planner.") || manifest.id.starts_with("ccos.");
                let is_allowed_util = manifest.id == "ccos.echo" || manifest.id == "ccos.user.ask"; // Allow echo and ask
                if is_meta && !is_allowed_util {
                    filtered_count += 1;
                    eprintln!("   ⏭️  Filtered out meta-capability: {}", manifest.id);
                    continue;
                }
                let trimmed = manifest.id.trim();
                if trimmed.is_empty() || !trimmed.contains('.') {
                    filtered_count += 1;
                    eprintln!(
                        "   ⏭️  Filtered out invalid capability ID: '{}'",
                        manifest.id
                    );
                    continue;
                }
                let mut entry = menu_entry_from_manifest(&manifest, Some(0.5));
                if let Some(schema) = &manifest.input_schema {
                    eprintln!(
                        "DEBUG: manifest={} input_schema:\n{}",
                        manifest.id,
                        type_expr_to_rtfs_pretty(schema)
                    );
                } else {
                    eprintln!("DEBUG: manifest={} has no input_schema", manifest.id);
                }
                eprintln!(
                    "DEBUG: manifest={} required_inputs={:?} optional_inputs={:?}",
                    manifest.id, entry.required_inputs, entry.optional_inputs
                );
                apply_input_overrides(&mut entry);
                menu.push(entry);
                eprintln!("   ✅ Added capability to menu: {}", manifest.id);
                if menu.len() >= limit {
                    break;
                }
            }
            eprintln!(
                "   📊 Filtered out {} capability(ies), added {} to menu",
                filtered_count,
                menu.len()
            );
        }
    }

    if menu.is_empty() {
        // More accurate error message
        let marketplace_count = marketplace.list_capabilities().await.len();
        if marketplace_count > 0 {
            Err(RuntimeError::Generic(format!(
                "Catalog query returned no capabilities and all {} marketplace capability(ies) were filtered out (likely all meta-capabilities: planner.* or ccos.*). Try running with MCP discovery enabled or restoring capabilities.",
                marketplace_count
            )))
        } else {
            Err(RuntimeError::Generic(
                "Catalog query returned no capabilities and marketplace is empty. Try running with MCP discovery enabled or restoring capabilities.".to_string(),
            ))
        }
    } else {
        Ok(menu)
    }
}

async fn refresh_capability_menu(
    catalog: Arc<CatalogService>,
    marketplace: Arc<CapabilityMarketplace>,
    goal: &str,
    intent: &Intent,
    signals: &mut GoalSignals,
    limit: usize,
) -> RuntimeResult<Vec<CapabilityMenuEntry>> {
    catalog.ingest_marketplace(marketplace.as_ref()).await;

    // Check if marketplace is empty OR only contains meta-capabilities (planner.*, ccos.*)
    // Meta-capabilities are internal system capabilities that shouldn't appear in execution plans
    let all_capabilities_before = marketplace.list_capabilities().await;
    let executable_capabilities: Vec<_> = all_capabilities_before
        .iter()
        .filter(|manifest| {
            let is_meta = manifest.id.starts_with("planner.") || manifest.id.starts_with("ccos.");
            let is_allowed_util = manifest.id == "ccos.echo" || manifest.id == "ccos.user.ask";
            !is_meta || is_allowed_util
        })
        .collect();

    let marketplace_count_before = all_capabilities_before.len();
    let executable_count_before = executable_capabilities.len();

    eprintln!("📊 Marketplace state: {} total capability(ies), {} executable (non-meta) capability(ies) before discovery", 
        marketplace_count_before, executable_count_before);

    // Extract capability hints from goal/intent to check if we need discovery
    let capability_hints = extract_capability_hints_from_goal(goal, intent);
    // Check if catalog search finds relevant SPECIFIC capabilities for the goal hints
    // This detects if we have a semantic gap (e.g. goal asks for "github issues" but we only have "ccos.user.ask")
    let query = build_catalog_query(goal, intent);
    let filter = CatalogFilter::for_kind(CatalogEntryKind::Capability);
    
    let mut gap_detected = false;
    // Check if the main query itself has poor coverage
    let main_hits = catalog.search_semantic(&query, Some(&filter), 3);
    let best_main_score = main_hits.iter()
        .filter(|h| h.entry.id != "ccos.user.ask" && h.entry.id != "ccos.echo") // Ignore generic fallbacks
        .map(|h| h.score)
        .next() // Hits are sorted by score desc
        .unwrap_or(0.0);
        
    if best_main_score < 0.65 {
        eprintln!("🔍 Goal coverage is low (score {:.2}) - checking hints for specific gaps...", best_main_score);
        
        // If main query is weak, check specific hints to confirm if we really need external tools
        // or if the query is just vague.
        for hint in &capability_hints {
            // Skip very generic hints to avoid false positives
            if hint.contains("general.") { continue; }
            
            let hits = catalog.search_semantic(hint, Some(&filter), 1);
            let best_score = hits.iter()
                .filter(|h| h.entry.id != "ccos.user.ask" && h.entry.id != "ccos.echo")
                .map(|h| h.score)
                .next()
                .unwrap_or(0.0);
            
            if best_score < 0.65 { 
                eprintln!("🔍 Hint '{}' has low coverage (score {:.2}) - signaling discovery need", hint, best_score);
                gap_detected = true;
                break;
            }
        }
    }

    // Trigger discovery if:
    // 1. Marketplace is empty OR only has meta-capabilities
    // 2. OR we detected a semantic gap in capabilities for the goal
    let should_trigger_discovery = executable_count_before == 0 || gap_detected;

    if should_trigger_discovery {
        if executable_count_before == 0 {
            if marketplace_count_before > 0 {
                eprintln!("🔍 Marketplace only contains meta-capabilities ({} total, {} executable) - triggering MCP discovery", 
                    marketplace_count_before, executable_count_before);
            } else {
                eprintln!("🔍 Marketplace is empty - triggering MCP discovery based on goal/intent");
            }
        } else {
            eprintln!("🔍 Goal suggests external capabilities (based on low semantic coverage) - triggering MCP discovery");
            eprintln!("   Catalog search query: '{}'", query);
            // Show the score that triggered this decision
            eprintln!("   Main query score: {:.2}", best_main_score);
        }
        eprintln!("   Goal: {}", goal);
        eprintln!(
            "   Extracted {} capability hint(s): {:?}",
            capability_hints.len(),
            capability_hints
        );

        // Create discovery engine to trigger MCP introspection
        use ccos::discovery::engine::DiscoveryEngine;
        use ccos::discovery::need_extractor::CapabilityNeed;
        use ccos::intent_graph::IntentGraph;
        use std::sync::Mutex;

        eprintln!("   Creating discovery engine...");
        let intent_graph = Arc::new(Mutex::new(
            IntentGraph::new_async(IntentGraphConfig::default())
                .await
                .map_err(|e| {
                    RuntimeError::Generic(format!("Failed to create IntentGraph: {}", e))
                })?,
        ));
        let discovery_engine = DiscoveryEngine::new(Arc::clone(&marketplace), intent_graph);

        let hints_to_use = if capability_hints.is_empty() {
            eprintln!("⚠️ No capability hints extracted from goal - using generic discovery hints");
            default_discovery_hints()
        } else {
            capability_hints
        };

        // Discover capabilities that match the goal/intent
        let mut discovered_count = 0;
        let mut discovered_capability_ids = Vec::new();
        for hint in hints_to_use {
            eprintln!("🔍 Discovering capabilities matching: {}", hint);
            let need = CapabilityNeed::new(
                hint.clone(),
                Vec::new(), // No specific inputs required
                Vec::new(), // No specific outputs required
                format!("Need for goal: {}", goal),
            );

            // Try to discover via MCP registry
            match discovery_engine.discover_capability(&need).await {
                Ok(ccos::discovery::DiscoveryResult::Found(manifest)) => {
                    eprintln!("✅ Discovered capability: {}", manifest.id);
                    discovered_count += 1;
                    discovered_capability_ids.push(manifest.id.clone());
                }
                Ok(ccos::discovery::DiscoveryResult::Incomplete(manifest)) => {
                    eprintln!("⚠️ Discovered incomplete capability: {}", manifest.id);
                    discovered_count += 1;
                    discovered_capability_ids.push(manifest.id.clone());
                }
                Ok(ccos::discovery::DiscoveryResult::NotFound) => {
                    eprintln!("❌ No capability found for: {}", hint);
                }
                Err(e) => {
                    eprintln!("⚠️ Discovery error for '{}': {}", hint, e);
                }
            }
        }

        eprintln!(
            "📊 Discovery summary: {} capability(ies) discovered: {:?}",
            discovered_count, discovered_capability_ids
        );

        // Re-ingest marketplace after discovery
        let marketplace_count_after = marketplace.list_capabilities().await.len();
        catalog.ingest_marketplace(marketplace.as_ref()).await;
        eprintln!(
            "📊 Marketplace now contains {} capability(ies)",
            marketplace_count_after
        );

        // Build menu from catalog first
        let mut menu = build_capability_menu_from_catalog(
            catalog.clone(),
            marketplace.clone(),
            goal,
            intent,
            limit,
        )
        .await?;

        // CRITICAL: Also add all discovered capabilities to the menu, even if catalog search didn't match them
        // This ensures discovered capabilities (like filter) are available even if they don't match the search query
        let mut seen_ids: std::collections::HashSet<String> =
            menu.iter().map(|e| e.id.clone()).collect();
        for capability_id in discovered_capability_ids {
            if seen_ids.contains(&capability_id) {
                continue; // Already in menu
            }

            // Filter out meta-capabilities
            let is_meta = capability_id.starts_with("planner.") || capability_id.starts_with("ccos.");
            let is_allowed_util = capability_id == "ccos.echo" || capability_id == "ccos.user.ask";
            if is_meta && !is_allowed_util {
                continue;
            }

            if let Some(manifest) = marketplace.get_capability(&capability_id).await {
                let trimmed = manifest.id.trim();
                if trimmed.is_empty() || !trimmed.contains('.') {
                    continue;
                }
                let mut entry = menu_entry_from_manifest(&manifest, Some(0.8)); // High score for discovered capabilities
                if let Some(schema) = &manifest.input_schema {
                    eprintln!(
                        "DEBUG: manifest={} input_schema:\n{}",
                        manifest.id,
                        type_expr_to_rtfs_pretty(schema)
                    );
                } else {
                    eprintln!("DEBUG: manifest={} has no input_schema", manifest.id);
                }
                eprintln!(
                    "DEBUG: manifest={} required_inputs={:?} optional_inputs={:?}",
                    manifest.id, entry.required_inputs, entry.optional_inputs
                );
                apply_input_overrides(&mut entry);
                menu.push(entry);
                seen_ids.insert(capability_id.clone());
                eprintln!(
                    "   ➕ Added discovered capability to menu: {}",
                    capability_id
                );
            }
        }

        // Re-sort menu by score after adding discovered capabilities
        menu.sort_by(compare_entries_by_score_desc);
        if menu.len() > limit {
            menu.truncate(limit);
        }

        annotate_menu_with_readiness(signals, &mut menu);
        return Ok(menu);
    }

    let mut menu =
        build_capability_menu_from_catalog(catalog, marketplace.clone(), goal, intent, limit).await?;

    // Always inject ccos.echo into the menu as a utility
    if !menu.iter().any(|e| e.id == "ccos.echo") {
        if let Some(manifest) = marketplace.get_capability("ccos.echo").await {
             let mut entry = menu_entry_from_manifest(&manifest, Some(0.1));
             apply_input_overrides(&mut entry);
             menu.push(entry);
        }
    }

    annotate_menu_with_readiness(signals, &mut menu);
    Ok(menu)
}

/// Extract capability hints from goal and intent to guide discovery
fn extract_capability_hints_from_goal(goal: &str, intent: &Intent) -> Vec<String> {
    let tokens = ensure_goal_tokens(goal, intent);
    let hints = build_semantic_hints_from_tokens(&tokens);
    if hints.is_empty() {
        default_discovery_hints()
    } else {
        hints
    }
}

fn default_discovery_hints() -> Vec<String> {
    GENERIC_OPERATION_HINTS
        .iter()
        .map(|hint| format!("general.{}", hint))
        .collect()
}

fn annotate_menu_with_readiness(signals: &GoalSignals, entries: &mut [CapabilityMenuEntry]) {
    for entry in entries {
        if let Some(readiness) = signals.capability_readiness(&entry.id) {
            let status_label = match readiness {
                RequirementReadiness::Unknown => None,
                RequirementReadiness::Identified => Some("identified"),
                RequirementReadiness::Incomplete => Some("incomplete"),
                RequirementReadiness::PendingExternal => Some("pending-external"),
                RequirementReadiness::Available => Some("available"),
            };
            if let Some(label) = status_label {
                entry
                    .metadata
                    .insert("capability_status".to_string(), label.to_string());
            }
        }

        if let Some(requirement) = signals.capability_requirement(&entry.id) {
            if let Some(value) = requirement.metadata.get("pending_request_id") {
                entry.metadata.insert(
                    "pending_request_id".to_string(),
                    value_to_string_repr(value),
                );
            }
        }
    }
}

fn collect_goal_intent_tokens(goal: &str, intent: &Intent) -> Vec<String> {
    let mut seen = HashSet::new();
    let mut ordered = Vec::new();

    fn push_tokens_from_text(text: &str, seen: &mut HashSet<String>, ordered: &mut Vec<String>) {
        for token in tokenize_identifier(text) {
            if seen.insert(token.clone()) {
                ordered.push(token);
            }
        }
    }

    push_tokens_from_text(goal, &mut seen, &mut ordered);
    for (key, value) in &intent.constraints {
        push_tokens_from_text(key, &mut seen, &mut ordered);
        push_tokens_from_text(&value_to_string_repr(value), &mut seen, &mut ordered);
    }
    for (key, value) in &intent.preferences {
        push_tokens_from_text(key, &mut seen, &mut ordered);
        push_tokens_from_text(&value_to_string_repr(value), &mut seen, &mut ordered);
    }
    if let Some(success) = &intent.success_criteria {
        push_tokens_from_text(&value_to_string_repr(success), &mut seen, &mut ordered);
    }

    ordered
}

fn ensure_tokens_with_generic_defaults(mut tokens: Vec<String>) -> Vec<String> {
    if tokens.is_empty() {
        tokens.extend(GENERIC_OPERATION_HINTS.iter().map(|hint| hint.to_string()));
        return tokens;
    }

    for hint in GENERIC_OPERATION_HINTS {
        if !tokens.iter().any(|token| token == hint) {
            tokens.push(hint.to_string());
        }
    }
    tokens
}

fn build_semantic_hints_from_tokens(tokens: &[String]) -> Vec<String> {
    let mut hints = Vec::new();
    let mut seen = HashSet::new();

    let focus_tokens: Vec<_> = tokens
        .iter()
        .take(MAX_DISCOVERY_HINT_TOKENS)
        .cloned()
        .collect();

    let combination_tokens: Vec<_> = focus_tokens
        .iter()
        .filter(|token| !GENERIC_OPERATION_HINTS.contains(&token.as_str()))
        .take(MAX_COMBINATION_TOKENS)
        .cloned()
        .collect();

    // 1. Generate combinations first (they are more specific/valuable)
    for noun in &combination_tokens {
        for operation in GENERIC_OPERATION_HINTS.iter().take(MAX_COMBINATION_TOKENS) {
            let combo_a = format!("{}.{}", noun, operation);
            if seen.insert(combo_a.clone()) {
                hints.push(combo_a);
            }
            let combo_b = format!("{}.{}", operation, noun);
            if seen.insert(combo_b.clone()) {
                hints.push(combo_b);
            }
        }
    }

    // 2. Add single tokens only if they are not generic operations
    // And ideally only if we don't have enough hints or specific needs
    for token in &focus_tokens {
        // Skip generic operation tokens as single-word hints (e.g., "list", "search")
        if GENERIC_OPERATION_HINTS.contains(&token.as_str()) {
            continue;
        }
        
        if seen.insert(token.clone()) {
            hints.push(token.clone());
        }
    }

    if hints.is_empty() {
        for default_hint in GENERIC_OPERATION_HINTS.iter().take(3) {
            let value = (*default_hint).to_string();
            if seen.insert(value.clone()) {
                hints.push(value);
            }
        }
    }

    hints
}

fn ensure_goal_tokens(goal: &str, intent: &Intent) -> Vec<String> {
    let tokens = collect_goal_intent_tokens(goal, intent);
    ensure_tokens_with_generic_defaults(tokens)
}

async fn goal_aligned_marketplace_fallbacks(
    marketplace: Arc<CapabilityMarketplace>,
    tokens: &[String],
    limit: usize,
) -> Vec<CapabilityMenuEntry> {
    let mut scored_entries = Vec::new();

    let all_capabilities = marketplace.list_capabilities().await;
    for manifest in all_capabilities {
        if manifest.id.starts_with("planner.") || manifest.id.starts_with("ccos.") {
            continue;
        }
        let trimmed = manifest.id.trim();
        if trimmed.is_empty() || !trimmed.contains('.') {
            continue;
        }
        let score = score_manifest_against_tokens(&manifest, tokens) as f64;
        let effective_score = if score > 0.0 { score } else { 0.1 };
        let mut entry = menu_entry_from_manifest(&manifest, Some(effective_score));
        if let Some(schema) = &manifest.input_schema {
            eprintln!(
                "DEBUG: manifest={} input_schema:\n{}",
                manifest.id,
                type_expr_to_rtfs_pretty(schema)
            );
        } else {
            eprintln!("DEBUG: manifest={} has no input_schema", manifest.id);
        }
        eprintln!(
            "DEBUG: manifest={} required_inputs={:?} optional_inputs={:?}",
            manifest.id, entry.required_inputs, entry.optional_inputs
        );
        apply_input_overrides(&mut entry);
        scored_entries.push((effective_score, entry));
    }

    scored_entries.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap_or(Ordering::Equal));
    scored_entries
        .into_iter()
        .take(limit)
        .map(|(_, entry)| entry)
        .collect()
}

fn build_catalog_query(goal: &str, intent: &Intent) -> String {
    let mut lines = Vec::new();
    lines.push(format!("Goal: {}", goal));
    if !intent.constraints.is_empty() {
        let mut entries = Vec::new();
        for (k, v) in &intent.constraints {
            entries.push(format!("{}={}", k, value_to_string_repr(v)));
        }
        lines.push(format!("Constraints: {}", entries.join("; ")));
    }
    if !intent.preferences.is_empty() {
        let mut entries = Vec::new();
        for (k, v) in &intent.preferences {
            entries.push(format!("{}={}", k, value_to_string_repr(v)));
        }
        lines.push(format!("Preferences: {}", entries.join("; ")));
    }
    if let Some(success) = &intent.success_criteria {
        lines.push(format!("Success: {}", value_to_string_repr(success)));
    }
    lines.join(" | ")
}

fn entry_score(entry: &CapabilityMenuEntry) -> f64 {
    entry.score.unwrap_or_default()
}

fn compare_entries_by_score_desc(a: &CapabilityMenuEntry, b: &CapabilityMenuEntry) -> Ordering {
    entry_score(b)
        .partial_cmp(&entry_score(a))
        .unwrap_or(Ordering::Equal)
}

fn apply_input_overrides(entry: &mut CapabilityMenuEntry) {
    if entry.required_inputs.is_empty() && entry.optional_inputs.is_empty() {
        if let Some((required, optional)) = load_override_parameters(entry.id.as_str()) {
            entry.required_inputs = required;
            entry.optional_inputs = optional;
        }
    }
}

fn format_capability_menu(entries: &[CapabilityMenuEntry]) -> String {
    let mut buffer = String::new();
    for (idx, entry) in entries.iter().enumerate() {
        let _ = writeln!(
            buffer,
            "{}. {} (score: {:.1})",
            idx + 1,
            entry.id,
            entry_score(entry)
        );
        if !entry.provider_label.is_empty() {
            let _ = writeln!(buffer, "   provider: {}", entry.provider_label);
        }
        if !entry.description.is_empty() {
            let _ = writeln!(buffer, "   description: {}", entry.description);
        }
        if !entry.required_inputs.is_empty() {
            let _ = writeln!(
                buffer,
                "   required inputs: {}",
                entry.required_inputs.join(", ")
            );
        }
        if !entry.optional_inputs.is_empty() {
            let _ = writeln!(
                buffer,
                "   optional inputs: {}",
                entry.optional_inputs.join(", ")
            );
        }
        let func_params = entry.function_parameters();
        if !func_params.is_empty() {
            let _ = writeln!(
                buffer,
                "   function parameters (use {{rtfs}}: ...): {}",
                func_params.join(", ")
            );
        }
        if !entry.outputs.is_empty() {
            let _ = writeln!(buffer, "   outputs: {}", entry.outputs.join(", "));
        }
        if let Some(status) = entry.metadata.get("capability_status") {
            let _ = writeln!(buffer, "   status: {}", status);
        }
        if let Some(request) = entry.metadata.get("pending_request_id") {
            let _ = writeln!(buffer, "   pending request: {}", request);
        }
        let _ = writeln!(buffer);
    }
    buffer
}

fn format_known_inputs(constraints: &HashMap<String, Value>) -> String {
    if constraints.is_empty() {
        return "(none)".to_string();
    }
    let mut parts = Vec::new();
    for (key, value) in constraints {
        parts.push(format!("{} = {}", key, value_to_string_repr(value)));
    }
    parts.join(", ")
}

fn value_to_string_repr(value: &Value) -> String {
    match value {
        Value::String(s) => s.clone(),
        Value::Integer(i) => i.to_string(),
        Value::Float(f) => format!("{:.2}", f),
        Value::Boolean(b) => b.to_string(),
        Value::Keyword(k) => format!(":{}", k.0),
        Value::Symbol(sym) => sym.0.clone(),
        Value::Vector(vec) => {
            let inner = vec.iter().map(value_to_string_repr).collect::<Vec<_>>();
            format!("[{}]", inner.join(", "))
        }
        Value::List(list) => {
            let inner = list.iter().map(value_to_string_repr).collect::<Vec<_>>();
            format!("({})", inner.join(" "))
        }
        Value::Map(map) => {
            let mut entries = Vec::new();
            for (key, value) in map {
                let key_str = match key {
                    MapKey::String(s) => s.clone(),
                    MapKey::Keyword(k) => format!(":{}", k.0),
                    MapKey::Integer(i) => i.to_string(),
                };
                entries.push(format!("{}: {}", key_str, value_to_string_repr(value)));
            }
            format!("{{{}}}", entries.join(", "))
        }
        Value::Nil => "nil".to_string(),
        other => format!("{:?}", other),
    }
}

async fn preload_discovered_capabilities_if_needed(
    marketplace: &CapabilityMarketplace,
) -> RuntimeResult<()> {
    let mut total_loaded = 0;

    // Load from core capabilities directory
    let core_root = Path::new("capabilities/core");
    if core_root.exists() {
        let loaded = preload_discovered_capabilities(marketplace, core_root).await?;
        if loaded > 0 {
            println!(
                "{}",
                format!("ℹ️  Loaded {} core capability manifest(s)", loaded).blue()
            );
            total_loaded += loaded;
        }
    }

    // Load from discovered capabilities directory
    let discovered_root = Path::new("capabilities/discovered");
    if discovered_root.exists() {
        let loaded = preload_discovered_capabilities(marketplace, discovered_root).await?;
        if loaded > 0 {
            println!(
                "{}",
                format!("ℹ️  Loaded {} discovered capability manifest(s)", loaded).blue()
            );
            total_loaded += loaded;
        }
    }

    if total_loaded > 0 {
        println!(
            "{}",
            format!("ℹ️  Total capabilities loaded: {}", total_loaded).blue()
        );
    }

    Ok(())
}

/// Extract notes from partially parsed JSON, even if some fields are missing
fn extract_notes_from_partial_json(response: &str) -> Option<String> {
    let json_block = extract_json_block(response)?;

    // Try to parse as a generic JSON array
    if let Ok(Value::Vector(items)) = serde_json::from_str::<Value>(json_block) {
        let mut notes = Vec::new();
        for (idx, item) in items.iter().enumerate() {
            if let Value::Map(map) = item {
                if let Some(Value::String(note)) = map.get(&MapKey::String("notes".to_string())) {
                    if !note.trim().is_empty() {
                        notes.push(format!("  Step {}: {}", idx + 1, note));
                    }
                }
            }
        }
        if !notes.is_empty() {
            return Some(notes.join("\n"));
        }
    }
    None
}

async fn propose_plan_steps_with_menu_and_capture(
    delegating: &ccos::arbiter::delegating_arbiter::DelegatingArbiter,
    goal: &str,
    intent: &Intent,
    known_inputs: &HashMap<String, Value>,
    menu: &[CapabilityMenuEntry],
    debug_prompts: bool,
    feedback: Option<&str>,
    previous_plan: Option<&str>,
    signals: Option<&GoalSignals>,
) -> Result<(String, Vec<PlanStep>), (Option<String>, RuntimeError)> {
    let menu_text = format_capability_menu(menu);
    let input_text = format_known_inputs(known_inputs);
    let mut context_lines = Vec::new();
    if !intent.preferences.is_empty() {
        let prefs = intent
            .preferences
            .iter()
            .map(|(k, v)| format!("{} = {}", k, value_to_string_repr(v)))
            .collect::<Vec<_>>()
            .join(", ");
        context_lines.push(format!("Preferences: {}", prefs));
    }
    if let Some(success) = &intent.success_criteria {
        context_lines.push(format!(
            "Success criteria: {}",
            value_to_string_repr(success)
        ));
    }

    // Include extracted requirements in the initial prompt (especially MustFilter)
    if let Some(signals) = signals {
        let mut requirement_lines = Vec::new();
        for req in &signals.requirements {
            match &req.kind {
                GoalRequirementKind::MustFilter {
                    field,
                    expected_value,
                } => {
                    let mut req_desc =
                        "REQUIRED: The plan must include a filtering step".to_string();
                    if let Some(value) = expected_value {
                        let value_str = match value {
                            Value::String(s) => s.clone(),
                            Value::Integer(i) => i.to_string(),
                            Value::Boolean(b) => b.to_string(),
                            _ => format!("{:?}", value),
                        };
                        req_desc.push_str(&format!(" that filters data to match '{}'", value_str));
                    }
                    if let Some(field_name) = field {
                        req_desc.push_str(&format!(" in field '{}'", field_name));
                    }
                    req_desc.push_str(". Use a filtering capability (e.g., one that accepts a predicate function parameter) as a separate step after fetching the data.");
                    req_desc.push_str(" If an upstream capability returns serialized text (like JSON), add an intermediate step or adapter that parses it into a collection before the filter runs.");
                    req_desc.push_str(" Use the adapter's declared output schema so downstream steps reference the exact field names it exposes instead of inventing new ones.");
                    requirement_lines.push(req_desc);
                }
                GoalRequirementKind::MustCallCapability { capability_id } => {
                    requirement_lines.push(format!(
                        "REQUIRED: The plan must include a step that calls capability '{}'",
                        capability_id
                    ));
                }
                GoalRequirementKind::MustProduceOutput { key } => {
                    requirement_lines
                        .push(format!("REQUIRED: The plan must produce output '{}'", key));
                }
                _ => {}
            }
        }
        if !requirement_lines.is_empty() {
            context_lines.push(format!("Requirements:\n{}", requirement_lines.join("\n")));
        }
    }

    let additional_context = if context_lines.is_empty() {
        String::new()
    } else {
        format!("\nAdditional context:\n{}\n", context_lines.join("\n"))
    };
    let feedback_block = feedback
        .map(|text| format!("\nPrevious attempt feedback:\n{}\n", text))
        .unwrap_or_default();

    let previous_plan_block = previous_plan
        .map(|plan| format!("\nPrevious plan attempt (for reference):\n{}\n", plan))
        .unwrap_or_default();

    let prompt = format!(
        r#"You are designing a plan to achieve the following goal.

Goal: {goal}

Known parameters (use via bindings var::name, e.g., var::user): {inputs}

Capability menu (choose from these only):
{menu}
{additional}{feedback}{previous_plan}
Output requirements:
- Respond with a JSON array (no markdown fences) where each element is an object with keys: id, capability_id, inputs, outputs, notes.
- The `name` field is optional (will be auto-generated if missing).
- inputs must be a JSON object mapping capability parameter names to values:
    * String literals: Use plain strings (e.g., "RTFS", "mandubian", "ccos")
    * Number literals: Use plain numbers (e.g., 100, 1, 3.14)
    * Boolean literals: Use true or false
    * Arrays: Use JSON arrays (e.g., ["item1", "item2"])
    * Variables: Use object with "var" key (e.g., {{"var": "user"}} references the 'user' parameter)
    * Step outputs: Use object with "step" and "output" keys (e.g., {{"step": "step_1", "output": "issues"}} references the 'issues' output from step_1)
    * Accessing Step Results in RTFS:
      - When using RTFS expressions (like in "rtfs" capability or function parameters), access previous step results using (get step_N :key).
      - CRITICAL: :key must be the ACTUAL output key returned by the tool (as listed in the Capability Menu), NOT the custom name you assigned in the 'outputs' list.
      - For MCP tools, the key is almost always :content. Example: (get step_0 :content).
      - For `rtfs` capability steps, `step_N` IS the direct value returned by the expression. Do NOT wrap it or access it via the output name.
    * Function parameters (ONLY for parameters marked as "(function - cannot be passed directly)" in the menu): Use object with "rtfs" key containing RTFS code (e.g., {{"rtfs": "(fn [item] ...)"}})
    * Pure Logic / Data Transformations:
      - Use a step with capability_id: "rtfs" (this is a special built-in capability for logic)
      - Inputs: {{ "expression": "(your rtfs code)" }}
      - Note on MCP tools: content is usually a list of objects like [{{ "type": "text", "text": "JSON..." }}].
      - When parsing JSON content:
        1. `parse-json` expects a STRING.
        2. MCP tools return `content` as a VECTOR of objects (e.g., `[{{:type "text" :text "..."}}]`).
        3. You MUST extract the text string first: `(get (first (get step_0 :content)) :text)`.
        4. Then parse: `(parse-json (get (first (get step_0 :content)) :text))`.
        5. `parse-json` returns a Map (for objects) or Vector (for arrays).
        6. CRITICAL: `first` throws an error on Maps. You CANNOT do `(first (parse-json ...))` if the JSON is an object (like a wrapper).
        7. Most API lists are wrapped (e.g. {{ "items": [...] }} or {{ "issues": [...] }}). You MUST extract the list key first.
           (get (parse-json ...) "items") or (get (parse-json ...) "issues")
        8. ALWAYS try to extract a key like "items", "data", "results", "issues" before calling `first`.
      - Example (SAFE pattern): {{ "capability_id": "rtfs", "inputs": {{ "expression": "(first (get (parse-json (get (first (get step_0 :content)) :text)) \"issues\"))" }}, "outputs": ["first_item"] }}
      - Available functions: first, get, parse-json, str, map, filter, etc.
      - Do NOT use `ccos.echo` or invent other capabilities.
- outputs must list the symbolic names of values produced by that step.
- Use capability_id exactly as listed in the menu.
- Steps must be ordered so dependencies appear earlier than the steps that use them.
- Include only the steps needed to satisfy the goal.
- notes is optional but may include rationale.
"#,
        goal = goal,
        inputs = input_text,
        menu = menu_text,
        additional = additional_context,
        feedback = feedback_block,
        previous_plan = previous_plan_block,
    );

    if debug_prompts {
        println!("\n{}", "=== Plan Synthesis Prompt ===".bold());
        println!("{}", prompt);
    }

    let response = match delegating.generate_raw_text(&prompt).await {
        Ok(r) => r,
        Err(e) => return Err((None, e)),
    };

    if debug_prompts {
        println!("\n{}", "=== Plan Synthesis Response ===".bold());
        println!("{}", response);
    }

    match parse_plan_steps_from_json(&response) {
        Ok(steps) => Ok((response, steps)),
        Err(e) => Err((Some(response), e)),
    }
}

fn parse_plan_steps_from_json(response: &str) -> RuntimeResult<Vec<PlanStep>> {
    let json_block = extract_json_block(response).ok_or_else(|| {
        RuntimeError::Generic(
            "LLM response did not contain a JSON array describing plan steps".to_string(),
        )
    })?;

    let raw_steps: Vec<PlanStepJson> = serde_json::from_str(json_block)
        .map_err(|e| RuntimeError::Generic(format!("Failed to parse plan steps JSON: {}", e)))?;

    if raw_steps.is_empty() {
        return Err(RuntimeError::Generic(
            "Plan synthesis produced an empty step list".to_string(),
        ));
    }

    let mut steps = Vec::new();
    for raw in raw_steps {
        steps.push(plan_step_from_json(raw)?);
    }

    Ok(steps)
}

fn extract_json_block(response: &str) -> Option<&str> {
    if let Some(start) = response.find('[') {
        if let Some(end) = response.rfind(']') {
            if end >= start {
                return Some(&response[start..=end]);
            }
        }
    }
    None
}

fn plan_step_from_json(raw: PlanStepJson) -> RuntimeResult<PlanStep> {
    if raw.id.trim().is_empty() {
        return Err(RuntimeError::Generic(
            "Plan step is missing an :id field".to_string(),
        ));
    }
    if raw.capability_id.trim().is_empty() {
        return Err(RuntimeError::Generic(format!(
            "Step '{}' is missing capability_id",
            raw.id
        )));
    }

    let mut inputs = Vec::new();
    for (name, binding_value) in raw.inputs {
        if name.trim().is_empty() {
            continue;
        }
        if binding_value.is_null() {
            continue;
        }
        let parsed = interpret_binding(&binding_value).ok_or_else(|| {
            RuntimeError::Generic(format!(
                "Step '{}' has invalid binding {:?} for input '{}'",
                raw.id, binding_value, name
            ))
        })?;
        inputs.push((name, parsed));
    }

    // Auto-generate name from capability_id if missing
    let step_name = if raw.name.trim().is_empty() {
        // Generate a readable name from capability_id (e.g., "mcp.example.catalog.get_items" -> "Get Items")
        raw.capability_id
            .split('.')
            .last()
            .map(|last| {
                last.split('_')
                    .map(|word| {
                        let mut chars = word.chars();
                        match chars.next() {
                            None => String::new(),
                            Some(first) => {
                                first.to_uppercase().collect::<String>() + chars.as_str()
                            }
                        }
                    })
                    .collect::<Vec<_>>()
                    .join(" ")
            })
            .unwrap_or_else(|| format!("Step {}", raw.id))
    } else {
        raw.name
    };

    let mut outputs: Vec<StepOutput> = Vec::new();
    for item in raw.outputs {
        match item {
            PlanStepOutputJson::Name(name) => {
                let trimmed = name.trim();
                if trimmed.is_empty() {
                    continue;
                }
                outputs.push(StepOutput {
                    alias: trimmed.to_string(),
                    source: trimmed.to_string(),
                });
            }
            PlanStepOutputJson::Mapping {
                name,
                output,
                field,
            } => {
                let mut alias = name.trim().to_string();
                let mut source = output
                    .as_deref()
                    .map(str::trim)
                    .unwrap_or_default()
                    .to_string();
                if source.is_empty() {
                    source = field
                        .as_deref()
                        .map(str::trim)
                        .unwrap_or_default()
                        .to_string();
                }
                if alias.is_empty() {
                    if source.is_empty() {
                        continue;
                    }
                    alias = source.clone();
                }
                if source.is_empty() {
                    source = alias.clone();
                }
                outputs.push(StepOutput { alias, source });
            }
        }
    }

    Ok(PlanStep {
        id: raw.id,
        name: step_name,
        capability_id: raw.capability_id,
        inputs,
        outputs,
        notes: raw.notes,
    })
}

/// Detect if a string literal looks like RTFS code (function calls, etc.)
/// This helps catch cases where the LLM passes code as a string to non-function parameters
fn looks_like_rtfs_code(s: &str) -> bool {
    let trimmed = s.trim();
    // Check for common RTFS code patterns
    (trimmed.starts_with('(') && trimmed.contains('(') && trimmed.contains(')'))
        // Check for function definitions
        || trimmed.starts_with("(fn ")
        || trimmed.starts_with("fn [")
        // Check for external namespace calls (clojure.*, etc.)
        || trimmed.contains("clojure.")
        || trimmed.contains("lisp.")
        || trimmed.contains("common-lisp.")
        // Check for RTFS function calls with colons
        || (trimmed.contains("(get ") && trimmed.contains(" :"))
        || (trimmed.contains("(call ") && trimmed.contains(" :"))
        // Check for lambda-like patterns
        || trimmed.contains("lambda")
        || trimmed.contains("λ")
}

fn looks_like_keyword_identifier(name: &str) -> bool {
    let trimmed = name.trim();
    !trimmed.is_empty()
        && trimmed
            .chars()
            // Accept ASCII letters (lower & upper), digits, hyphen and underscore.
            // We allow uppercase because MCPs may return keys like `isError`.
            .all(|c| c.is_ascii_alphabetic() || c.is_ascii_digit() || c == '-' || c == '_')
}

fn validate_plan_steps_against_menu(
    steps: &[PlanStep],
    menu: &[CapabilityMenuEntry],
) -> PlanValidationOutcome {
    let mut outcome = PlanValidationOutcome::default();
    let menu_map = menu
        .iter()
        .map(|entry| (entry.id.as_str(), entry))
        .collect::<HashMap<_, _>>();

    for step in steps {
        // Special case for inline RTFS steps (pseudo-capability)
        if step.capability_id == "rtfs" {
            continue;
        }
        let Some(entry) = menu_map.get(step.capability_id.as_str()) else {
            outcome.unknown_capabilities.push(UnknownCapabilityUsage {
                step_id: step.id.clone(),
                step_name: if step.name.trim().is_empty() {
                    None
                } else {
                    Some(step.name.clone())
                },
                capability_id: step.capability_id.clone(),
                notes: step.notes.clone(),
                outputs: step.outputs.iter().map(|o| o.alias.clone()).collect(),
            });
            continue;
        };

        if entry.required_inputs.is_empty() && entry.optional_inputs.is_empty() {
            continue;
        }

        let input_keys = step
            .inputs
            .iter()
            .map(|(name, _)| name.as_str())
            .collect::<HashSet<_>>();

        // Normalize parameter names by stripping annotations (e.g., "predicate (function - cannot be passed directly)" -> "predicate")
        let normalize_param_name =
            |name: &str| -> String { name.split(" (function").next().unwrap_or(name).to_string() };

        // Build set of allowed parameter names (base names without annotation)
        let mut allowed_inputs_base = HashSet::new();
        for key in &entry.required_inputs {
            let base = normalize_param_name(key);
            allowed_inputs_base.insert(base);
        }
        for key in &entry.optional_inputs {
            let base = normalize_param_name(key);
            allowed_inputs_base.insert(base);
        }

        // Build set of function parameter names (without the annotation suffix)
        let function_param_names: HashSet<String> =
            entry.function_parameters().into_iter().collect();

        // Check for missing required inputs (compare using normalized names)
        for required in &entry.required_inputs {
            let required_base = normalize_param_name(required);
            let is_provided = input_keys.iter().any(|provided| {
                let provided_base = normalize_param_name(provided);
                provided_base == required_base
            });

            if !is_provided {
                outcome.schema_errors.push(format!(
                    "Step '{}' using '{}' is missing required input '{}' (required: {}; optional: {})",
                    step.id,
                    step.capability_id,
                    required_base,
                    if entry.required_inputs.is_empty() {
                        "(none)".to_string()
                    } else {
                        entry.required_inputs.iter().map(|s| normalize_param_name(s)).collect::<Vec<_>>().join(", ")
                    },
                    if entry.optional_inputs.is_empty() {
                        "(none)".to_string()
                    } else {
                        entry.optional_inputs.iter().map(|s| normalize_param_name(s)).collect::<Vec<_>>().join(", ")
                    }
                ));
            }
        }

        // Check for unsupported inputs (compare using normalized names)
        for provided in &input_keys {
            let provided_base = normalize_param_name(provided);
            if !allowed_inputs_base.contains(&provided_base) {
                outcome.schema_errors.push(format!(
                    "Step '{}' using '{}' provided unsupported input '{}' (required: {}; optional: {})",
                    step.id,
                    step.capability_id,
                    provided,
                    if entry.required_inputs.is_empty() {
                        "(none)".to_string()
                    } else {
                        entry.required_inputs.iter().map(|s| normalize_param_name(s)).collect::<Vec<_>>().join(", ")
                    },
                    if entry.optional_inputs.is_empty() {
                        "(none)".to_string()
                    } else {
                        entry.optional_inputs.iter().map(|s| normalize_param_name(s)).collect::<Vec<_>>().join(", ")
                    }
                ));
            } else {
                // Check if RTFS code is used for non-function parameters
                if let Some(binding) = step
                    .inputs
                    .iter()
                    .find(|(name, _)| normalize_param_name(name) == provided_base)
                    .map(|(_, b)| b)
                {
                    // Use the normalized base name
                    let base_name = provided_base;

                    // Check for explicit RTFS code binding
                    if let StepInputBinding::RtfsCode(_) = binding {
                        if !function_param_names.contains(&base_name) {
                            outcome.schema_errors.push(format!(
                                "Step '{}' using '{}' provided RTFS code (rtfs: ...) for parameter '{}', but this parameter is NOT a function type. RTFS code can only be used for parameters marked as (function - cannot be passed directly) in the menu.",
                                step.id,
                                step.capability_id,
                                base_name
                            ));
                        }
                    }
                    // Check for string literals that look like RTFS code
                    else if let StepInputBinding::Literal(text) = binding {
                        if looks_like_rtfs_code(text) && !function_param_names.contains(&base_name)
                        {
                            outcome.schema_errors.push(format!(
                                "Step '{}' using '{}' provided a string value that looks like RTFS code for parameter '{}', but this parameter is NOT a function type. Parameters must receive values matching their declared type (string, number, etc.). If you need filtering or transformation, use a separate step with a capability that accepts function parameters.",
                                step.id,
                                step.capability_id,
                                base_name
                            ));
                        }
                    }
                }
            }
        }

        for output in &step.outputs {
            if !looks_like_keyword_identifier(&output.alias) {
                outcome.schema_errors.push(format!(
                    "Step '{}' output '{}' cannot be converted to an RTFS keyword. Use lowercase letters, digits, '-' or '_' only.",
                    step.id,
                    output.alias
                ));
            }
            if !output.source.trim().is_empty() && !looks_like_keyword_identifier(&output.source) {
                outcome.schema_errors.push(format!(
                    "Step '{}' output source '{}' is not a valid RTFS keyword. Ensure capability outputs use RTFS-compatible identifiers.",
                    step.id,
                    output.source
                ));
            }
        }
    }

    outcome
}

/// Interpret a JSON value as a step input binding using RTFS-native representation:
/// - String/Number literals: Use plain values (e.g., "RTFS", 100)
/// - Variables: Use object with "var" key (e.g., {"var": "user"})
/// - Step outputs: Use object with "step" and "output" keys (e.g., {"step": "step_1", "output": "issues"})
fn interpret_binding(value: &JsonValue) -> Option<StepInputBinding> {
    match value {
        // Plain strings and numbers are literals (RTFS-native)
        JsonValue::String(s) => Some(StepInputBinding::Literal(s.clone())),
        JsonValue::Number(n) => Some(StepInputBinding::Literal(n.to_string())),
        JsonValue::Bool(b) => Some(StepInputBinding::Literal(b.to_string())),
        JsonValue::Null => None,

        // Objects encode variable or step references
        JsonValue::Object(obj) => {
            // Variable reference: {"var": "user"}
            if let Some(var_name) = obj.get("var").and_then(|v| v.as_str()) {
                if !var_name.trim().is_empty() {
                    return Some(StepInputBinding::Variable(var_name.to_string()));
                }
            }

            // Step output reference: {"step": "step_1", "output": "issues"}
            if let (Some(step_id_val), Some(output_val)) = (obj.get("step"), obj.get("output")) {
                if let (Some(step_id), Some(output)) = (step_id_val.as_str(), output_val.as_str()) {
                    if !step_id.trim().is_empty() && !output.trim().is_empty() {
                        return Some(StepInputBinding::StepOutput {
                            step_id: step_id.to_string(),
                            output: output.to_string(),
                        });
                    }
                }
            }

            // RTFS code for function parameters: {"rtfs": "(fn [item] (...))"}
            if let Some(rtfs_code) = obj.get("rtfs").and_then(|v| v.as_str()) {
                if !rtfs_code.trim().is_empty() {
                    return Some(StepInputBinding::RtfsCode(rtfs_code.to_string()));
                }
            }

            // Unknown object structure - try to serialize as JSON string literal
            serde_json::to_string(value)
                .ok()
                .map(StepInputBinding::Literal)
        }

        // Arrays are serialized as JSON string literals
        JsonValue::Array(_) => serde_json::to_string(value)
            .ok()
            .map(StepInputBinding::Literal),
    }
}

fn summarize_plan_steps(steps: &[PlanStep]) -> Vec<PlanStepSummary> {
    steps
        .iter()
        .map(|step| {
            let mut provided_inputs = BTreeMap::new();
            for (key, binding) in &step.inputs {
                provided_inputs.insert(key.clone(), format_binding(binding));
            }

            PlanStepSummary {
                id: step.id.clone(),
                capability_id: Some(step.capability_id.clone()),
                capability_class: None,
                provided_inputs,
                produced_outputs: step.outputs.iter().map(|o| o.alias.clone()).collect(),
                notes: step.notes.clone(),
            }
        })
        .collect()
}

fn build_resolution_context(signals: &GoalSignals, capability_id: &str) -> HashMap<String, String> {
    let mut context = HashMap::new();
    context.insert("goal".to_string(), signals.goal_text.clone());
    context.insert(
        "requested_capability".to_string(),
        capability_id.to_string(),
    );

    if let Some(intent_id) = signals
        .contextual_facts
        .get("intent_id")
        .and_then(value_to_string)
    {
        context.insert("intent_id".to_string(), intent_id);
    }

    context
}

fn register_external_capability_request(
    capability_id: &str,
    signals: &GoalSignals,
    reason: &str,
) -> Option<String> {
    let timestamp = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .ok()
        .map(|dur| dur.as_millis())
        .unwrap_or(0);
    let sanitized = capability_id.replace(':', "-");
    let request_id = format!("cap-request-{}-{}", sanitized, timestamp);
    println!(
        "{}",
        format!(
            "📨 Escalating capability '{}' for external provisioning (request id: {}): {}",
            capability_id, request_id, reason
        )
        .yellow()
    );
    println!("    goal context: {}", signals.goal_text);
    Some(request_id)
}

fn register_pending_requests(
    signals: &mut GoalSignals,
    menu: &mut Vec<CapabilityMenuEntry>,
    requests: Vec<PendingCapabilityRequest>,
) -> (Vec<String>, Vec<String>) {
    let mut pending_ids = Vec::new();
    let mut suggested_actions = Vec::new();
    for pending in requests {
        let capability_id = pending.capability_id.clone();
        let mut metadata = vec![(
            "capability_status".to_string(),
            Value::String("pending-external".to_string()),
        )];
        if let Some(ticket) = pending.request_id.as_ref() {
            metadata.push((
                "pending_request_id".to_string(),
                Value::String(ticket.clone()),
            ));
        }
        if let Some(action) = pending.suggested_human_action.as_ref() {
            metadata.push((
                "suggested_human_action".to_string(),
                Value::String(action.clone()),
            ));
            suggested_actions.push(format!("{}: {}", capability_id, action));
        }
        signals.update_capability_requirement(
            &capability_id,
            RequirementReadiness::PendingExternal,
            metadata,
        );
        signals.set_pending_request_id(&capability_id, pending.request_id.clone());
        pending_ids.push(capability_id);
    }
    annotate_menu_with_readiness(signals, menu);
    (pending_ids, suggested_actions)
}

fn value_to_string(value: &Value) -> Option<String> {
    match value {
        Value::String(s) => Some(s.clone()),
        Value::Keyword(k) => Some(k.0.clone()),
        Value::Symbol(sym) => Some(sym.0.clone()),
        _ => None,
    }
}

fn render_plan_steps(steps: &[PlanStep]) {
    println!("\n{}", "Proposed Steps".bold().cyan());
    for (idx, step) in steps.iter().enumerate() {
        println!("  {}. {} ({})", idx + 1, step.name, step.capability_id);
        if !step.inputs.is_empty() {
            println!("     inputs:");
            for (key, binding) in &step.inputs {
                println!("       - {} ⇢ {}", key, format_binding(binding));
            }
        }
        if !step.outputs.is_empty() {
            let labels: Vec<String> = step.outputs.iter().map(display_step_output).collect();
            println!("     outputs: {}", labels.join(", "));
        }
        if let Some(notes) = &step.notes {
            println!("     notes: {}", notes);
        }
    }
}

fn format_binding(binding: &StepInputBinding) -> String {
    match binding {
        StepInputBinding::Literal(value) => format!("literal('{}')", value),
        StepInputBinding::Variable(name) => format!("var({})", name),
        StepInputBinding::StepOutput { step_id, output } => {
            format!("step({}->{})", step_id, output)
        }
        StepInputBinding::RtfsCode(code) => format!("rtfs({})", code),
    }
}

async fn assemble_plan_from_steps(
    steps: &[PlanStep],
    intent: &Intent,
    plan_id_override: Option<&str>,
    delegating: Option<&ccos::arbiter::delegating_arbiter::DelegatingArbiter>,
) -> RuntimeResult<Plan> {
    let mut step_index = HashMap::new();
    for (idx, step) in steps.iter().enumerate() {
        step_index.insert(step.id.clone(), idx);
    }

    let mut external_vars: HashSet<String> = HashSet::new();
    let mut capability_ids: HashSet<String> = HashSet::new();
    let mut output_map: HashMap<String, usize> = HashMap::new();

    for (idx, step) in steps.iter().enumerate() {
        capability_ids.insert(step.capability_id.clone());
        for (_, binding) in &step.inputs {
            if let StepInputBinding::Variable(name) = binding {
                external_vars.insert(name.clone());
            }
            // Extract variables from RTFS code (var::name pattern in prompts)
            if let StepInputBinding::RtfsCode(code) = binding {
                // Extract variable references from RTFS code (var::name pattern)
                // Simple string-based extraction for var::name pattern
                let mut search_pos = 0;
                while let Some(pos) = code[search_pos..].find("var::") {
                    let actual_pos = search_pos + pos + 5; // Skip "var::"
                    let mut var_name = String::new();
                    for ch in code[actual_pos..].chars() {
                        if ch.is_alphanumeric() || ch == '_' {
                            var_name.push(ch);
                        } else {
                            break;
                        }
                    }
                    if !var_name.is_empty() {
                        external_vars.insert(var_name.clone());
                    }
                    search_pos = actual_pos + var_name.len();
                    if search_pos >= code.len() {
                        break;
                    }
                }
            }
            if let StepInputBinding::StepOutput { step_id, .. } = binding {
                if !step_index.contains_key(step_id) {
                    return Err(RuntimeError::Generic(format!(
                        "Step '{}' references unknown step '{}'",
                        step.id, step_id
                    )));
                }
                if step_index[step_id] >= idx {
                    return Err(RuntimeError::Generic(format!(
                        "Step '{}' references future step '{}'",
                        step.id, step_id
                    )));
                }
            }
        }

        for output in &step.outputs {
            if !output.alias.trim().is_empty() {
                output_map.insert(output.alias.clone(), idx);
            }
            if output.source != output.alias && !output.source.trim().is_empty() {
                output_map.insert(output.source.clone(), idx);
            }
        }
    }

    // Try LLM-based conversion first, fallback to manual if it fails
    let use_llm = std::env::var("CCOS_USE_LLM_PLAN_CONVERSION")
        .map(|v| v == "1" || v.to_lowercase() == "true")
        .unwrap_or(true);

    let body = if use_llm && delegating.is_some() {
        match render_plan_body_with_llm(steps, intent, delegating.unwrap()).await {
            Ok(rtfs) => rtfs,
            Err(e) => {
                eprintln!(
                    "⚠️  LLM plan conversion failed: {}. Falling back to manual conversion.",
                    e
                );
                render_plan_body(steps, &step_index)?
            }
        }
    } else {
        render_plan_body(steps, &step_index)?
    };

    let mut input_schema_entries = HashMap::new();
    let mut sorted_vars: Vec<String> = external_vars.into_iter().collect();
    sorted_vars.sort();
    for name in &sorted_vars {
        input_schema_entries.insert(
            MapKey::Keyword(Keyword(name.clone())),
            Value::String("any".to_string()),
        );
    }

    let input_schema = if input_schema_entries.is_empty() {
        None
    } else {
        Some(Value::Map(input_schema_entries))
    };

    let mut output_schema_entries = HashMap::new();
    let mut sorted_outputs: Vec<(String, usize)> = output_map.into_iter().collect();
    sorted_outputs.sort_by(|a, b| a.0.cmp(&b.0));
    for (name, _) in &sorted_outputs {
        output_schema_entries.insert(
            MapKey::Keyword(Keyword(name.clone())),
            Value::String("any".to_string()),
        );
    }

    let output_schema = if output_schema_entries.is_empty() {
        None
    } else {
        Some(Value::Map(output_schema_entries))
    };

    let mut capabilities_required: Vec<String> = capability_ids.into_iter().collect();
    capabilities_required.sort();

    let mut plan = Plan::new_with_schemas(
        Some(
            intent
                .name
                .clone()
                .unwrap_or_else(|| "planner_viz.auto_plan".to_string()),
        ),
        vec![intent.intent_id.clone()],
        PlanBody::Rtfs(body),
        input_schema,
        output_schema,
        HashMap::new(),
        capabilities_required.clone(),
        HashMap::new(),
    );
    plan.capabilities_required = capabilities_required;
    if let Some(plan_id) = plan_id_override {
        plan.plan_id = plan_id.to_string();
    }
    plan.metadata.insert(
        "planning.pipeline".to_string(),
        Value::String("planner_viz_v2".to_string()),
    );

    Ok(plan)
}

async fn render_plan_body_with_llm(
    steps: &[PlanStep],
    intent: &Intent,
    delegating: &ccos::arbiter::delegating_arbiter::DelegatingArbiter,
) -> RuntimeResult<String> {
    // Serialize steps to JSON
    let mut step_json_vec = Vec::new();
    let mut plan_variables = HashSet::new();
    let mut step_dependencies = Vec::new();

    for step in steps {
        let mut inputs = HashMap::new();
        for (name, binding) in &step.inputs {
            let value = match binding {
                StepInputBinding::Literal(text) => JsonValue::String(text.clone()),
                StepInputBinding::Variable(var) => {
                    plan_variables.insert(var.clone());
                    serde_json::json!({"var": var})
                }
                StepInputBinding::StepOutput { step_id, output } => {
                    step_dependencies.push(format!("{} -> {}", step_id, output));
                    serde_json::json!({"step": step_id, "output": output})
                }
                StepInputBinding::RtfsCode(code) => {
                    // Extract variables from RTFS code
                    let mut search_pos = 0;
                    while let Some(pos) = code[search_pos..].find("var::") {
                        let actual_pos = search_pos + pos + 5;
                        let mut var_name = String::new();
                        for ch in code[actual_pos..].chars() {
                            if ch.is_alphanumeric() || ch == '_' {
                                var_name.push(ch);
                            } else {
                                break;
                            }
                        }
                        if !var_name.is_empty() {
                            let var_name_clone = var_name.clone();
                            plan_variables.insert(var_name_clone);
                        }
                        search_pos = actual_pos + var_name.len();
                        if search_pos >= code.len() {
                            break;
                        }
                    }
                    serde_json::json!({"rtfs": code})
                }
            };
            inputs.insert(name.clone(), value);
        }

        step_json_vec.push(PlanStepJsonSerialized {
            id: step.id.clone(),
            name: step.name.clone(),
            capability_id: step.capability_id.clone(),
            inputs,
            outputs: step.outputs.iter().map(step_output_to_json).collect(),
            notes: step.notes.clone(),
        });
    }

    let json_steps = serde_json::to_string_pretty(&step_json_vec)
        .map_err(|e| RuntimeError::Generic(format!("Failed to serialize steps: {}", e)))?;

    let mut vars = HashMap::new();
    vars.insert("json_steps".to_string(), json_steps);
    vars.insert("intent_id".to_string(), intent.intent_id.clone());
    vars.insert(
        "intent_name".to_string(),
        intent
            .name
            .as_deref()
            .unwrap_or("unnamed_intent")
            .to_string(),
    );
    vars.insert("plan_variables".to_string(), {
        let mut vars: Vec<String> = plan_variables.into_iter().collect();
        vars.sort();
        vars.join(", ")
    });
    vars.insert(
        "step_dependencies".to_string(),
        if step_dependencies.is_empty() {
            "(none)".to_string()
        } else {
            step_dependencies.join(", ")
        },
    );

    let prompt = PLAN_CONVERSION_PROMPT_MANAGER
        .render("plan_rtfs_conversion", "v1", &vars)
        .map_err(|e| RuntimeError::Generic(format!("Failed to render prompt: {}", e)))?;

    if std::env::var("CCOS_DEBUG_PROMPTS").is_ok() {
        println!("\n{}", "=== RTFS Plan Conversion Prompt ===".bold());
        println!("{}", prompt);
    }

    let response = delegating
        .generate_raw_text(&prompt)
        .await
        .map_err(|e| RuntimeError::Generic(format!("LLM conversion request failed: {}", e)))?;

    if std::env::var("CCOS_DEBUG_PROMPTS").is_ok() {
        println!("\n{}", "=== RTFS Plan Conversion Response ===".bold());
        println!("{}", response);
    }

    // Extract RTFS code from response
    let rtfs_code = if let Some(caps) = RTFS_CODE_BLOCK_RE.captures(&response) {
        caps.get(1).map(|m| m.as_str().trim().to_string())
    } else {
        None
    };

    let rtfs_code = rtfs_code
        .or_else(|| {
            let trimmed = response.trim().trim_matches('`').trim();
            if trimmed.starts_with("(do") {
                Some(trimmed.to_string())
            } else {
                None
            }
        })
        .ok_or_else(|| {
            RuntimeError::Generic("LLM response did not contain RTFS plan code".to_string())
        })?;

    Ok(rtfs_code)
}

fn rewrite_rtfs_references(code: &str, step_index: &HashMap<String, usize>) -> String {
    let mut rewritten = code.to_string();
    // Sort by length descending to handle prefixes correctly (e.g. step_10 vs step_1)
    let mut sorted_ids: Vec<_> = step_index.keys().collect();
    sorted_ids.sort_by(|a, b| b.len().cmp(&a.len()));

    for id in sorted_ids {
        let index = step_index[id];
        let target = format!("step_{}", index);
        if id == &target {
            continue; // No change needed
        }
        
        // Only replace if surrounded by boundaries to avoid partial matches
        // e.g. don't replace "step_1" in "step_10"
        let mut result = String::with_capacity(rewritten.len());
        let mut last_end = 0;
        
        // Find all occurrences
        let matches: Vec<(usize, &str)> = rewritten.match_indices(id).collect();
        for (start, _part) in matches {
             if start < last_end { continue; } // Already processed (overlapping?)

             let before = if start > 0 { rewritten.chars().nth(start - 1) } else { None };
             let after = rewritten.chars().nth(start + id.len());
             
             let boundary_start = before.map(|c| !c.is_alphanumeric() && c != '_').unwrap_or(true);
             let boundary_end = after.map(|c| !c.is_alphanumeric() && c != '_').unwrap_or(true);
             
             result.push_str(&rewritten[last_end..start]);
             
             if boundary_start && boundary_end {
                 result.push_str(&target);
             } else {
                 result.push_str(id);
             }
             last_end = start + id.len();
        }
        result.push_str(&rewritten[last_end..]);
        rewritten = result;
    }
    rewritten
}

fn render_plan_body(
    steps: &[PlanStep],
    step_index: &HashMap<String, usize>,
) -> RuntimeResult<String> {
    if steps.is_empty() {
        return Err(RuntimeError::Generic(
            "Cannot generate plan: no steps provided".to_string(),
        ));
    }

    let mut body = String::new();
    body.push_str("(do\n");
    body.push_str("    (let [\n");

    for (idx, step) in steps.iter().enumerate() {
        let args = render_step_arguments(step, step_index)?;
        if step.capability_id == "rtfs" {
            // Handle pseudo-capability for inline RTFS
            // Extract 'expression' or 'code' or use args map if not found
            // We expect a direct RTFS string.
            // But render_step_arguments rendered inputs as a Map string.
            // We need to be careful.
            
            // Try to find the "expression" or "code" input
            let expr = step.inputs.iter().find_map(|(k, v)| {
                if k == "expression" || k == "code" {
                    match v {
                         StepInputBinding::Literal(s) => Some(s.clone()),
                         StepInputBinding::RtfsCode(s) => Some(s.clone()),
                         _ => None
                    }
                } else {
                    None
                }
            }).unwrap_or_else(|| "nil".to_string());
            
            let rewritten_expr = rewrite_rtfs_references(&expr, step_index);

            body.push_str(&format!(
                "      step_{} {}\n",
                idx,
                rewritten_expr
            ));
        } else {
            body.push_str(&format!(
                "      step_{} (call :{} {})\n",
                idx,
                sanitize_capability_id(&step.capability_id),
                args
            ));
        }
    }

    body.push_str("    ]\n");
    body.push_str("      {\n");

    let mut final_outputs: Vec<(String, usize, String)> = Vec::new();
    for (idx, step) in steps.iter().enumerate() {
        for output in &step.outputs {
            let alias = output.alias.trim();
            if alias.is_empty() {
                continue;
            }
            let source = output.source.trim();
            let source_name = if source.is_empty() {
                alias.to_string()
            } else {
                source.to_string()
            };
            final_outputs.push((alias.to_string(), idx, source_name));
        }
    }

    if final_outputs.is_empty() {
        body.push_str("        :result step_");
        body.push_str("0\n");
    } else {
        final_outputs.sort_by(|a, b| a.0.cmp(&b.0));
        for (idx, (alias, step_idx, source)) in final_outputs.iter().enumerate() {
            let step_def = &steps[*step_idx];
            let is_rtfs = step_def.capability_id == "rtfs";
            let val_expr = if is_rtfs {
                 format!("step_{}", step_idx)
            } else {
                 format!("(get step_{} :{})", step_idx, sanitize_keyword_name(source))
            };

            body.push_str(&format!(
                "        :{} {}",
                sanitize_keyword_name(alias),
                val_expr
            ));
            if idx + 1 != final_outputs.len() {
                body.push_str("\n");
            } else {
                body.push_str("\n");
            }
        }
    }

    body.push_str("      })\n");
    body.push_str("  )");

    Ok(body)
}

fn render_step_arguments(
    step: &PlanStep,
    step_index: &HashMap<String, usize>,
) -> RuntimeResult<String> {
    if step.inputs.is_empty() {
        return Ok("{}".to_string());
    }

    let mut parts = Vec::new();
    for (name, binding) in &step.inputs {
        let value = match binding {
            StepInputBinding::Literal(text) => format!("\"{}\"", escape_string(text)),
            StepInputBinding::Variable(var) => sanitize_symbol_name(var),
            StepInputBinding::StepOutput { step_id, output } => {
                let referenced = step_index.get(step_id).ok_or_else(|| {
                    RuntimeError::Generic(format!(
                        "Step '{}' references unknown step '{}'",
                        step.id, step_id
                    ))
                })?;
                format!(
                    "(get step_{} :{})",
                    referenced,
                    sanitize_keyword_name(output)
                )
            }
            StepInputBinding::RtfsCode(code) => {
                // RTFS code is embedded directly (for function parameters)
                // Convert var::name to symbol name (var:: is prompt convention, not RTFS syntax)
                let mut rtfs_code = code.clone();
                // Simple replacement: var::name -> name (keeping it simple, full parsing would require RTFS parser)
                // Note: This is a basic fix; in production you might want proper parsing
                rtfs_code = rtfs_code.replace("var::", "");
                
                // Also rewrite step references (e.g. step_ID -> step_INDEX)
                rtfs_code = rewrite_rtfs_references(&rtfs_code, step_index);
                
                rtfs_code
            }
        };
        parts.push(format!("    :{} {}", sanitize_keyword_name(name), value));
    }

    let mut map_repr = String::new();
    map_repr.push_str("{\n");
    map_repr.push_str(&parts.join("\n"));
    map_repr.push_str("\n  }");
    Ok(map_repr)
}

fn sanitize_capability_id(id: &str) -> String {
    id.replace(' ', "-")
}

fn sanitize_symbol_name(name: &str) -> String {
    let mut out = String::new();
    for c in name.chars() {
        if c.is_alphanumeric() || c == '_' || c == '-' {
            out.push(c);
        } else {
            out.push('_');
        }
    }
    if out.is_empty() {
        "value".to_string()
    } else {
        out
    }
}

fn sanitize_keyword_name(name: &str) -> String {
    sanitize_symbol_name(name)
}

fn escape_string(text: &str) -> String {
    text.replace('\\', "\\\\").replace('"', "\\\"")
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    let args = Args::parse();

    let goal = match args.goal {
        Some(goal) if !goal.trim().is_empty() => goal,
        _ => {
            eprintln!("❗  Please provide a natural-language goal with --goal");
            return Ok(());
        }
    };

    if args.debug_prompts {
        std::env::set_var("CCOS_DEBUG_PROMPTS", "1");
    }
    if !args.trace {
        std::env::set_var("CCOS_QUIET_RESOLVER", "1");
    }

    let agent_config = load_agent_config(&args.config)?;
    apply_llm_profile(&agent_config, args.profile.as_deref())?;

    if std::env::var("CCOS_DELEGATING_MODEL").is_err() {
        std::env::set_var("CCOS_DELEGATING_MODEL", "stub");
        std::env::set_var("CCOS_LLM_MODEL", "stub");
        std::env::set_var("CCOS_LLM_PROVIDER", "stub");
        std::env::set_var("CCOS_ALLOW_STUB_PROVIDER", "1");
    }

    let plan_archive_path = plan_archive_dir();
    ensure_directory(&plan_archive_path)?;

    let ccos = Arc::new(
        CCOS::new_with_agent_config_and_configs_and_debug_callback(
            IntentGraphConfig::default(),
            Some(plan_archive_path),
            Some(agent_config.clone()),
            None,
        )
        .await
        .map_err(runtime_error)?,
    );

    configure_session_pool(&ccos).await?;

    let delegating_arc = ccos.get_delegating_arbiter().ok_or_else(|| {
        runtime_error(RuntimeError::Generic(
            "Delegating arbiter is not configured".to_string(),
        ))
    })?;
    let delegating = delegating_arc.as_ref();

    if delegating.get_llm_config().provider_type.clone() == LlmProviderType::Stub {
        eprintln!(
            "⚠️  WARNING: Delegating arbiter is running with the stub LLM provider (no external model available)."
        );
        eprintln!(
            "    Check CCOS_LLM_API_KEY / OPENROUTER_API_KEY or select a different profile before retrying."
        );
    }

    let intent = delegating
        .natural_language_to_intent(&goal, None)
        .await
        .map_err(runtime_error)?;

    let plan_run_id = format!("planviz-{}", Uuid::new_v4());
    let planner_audit = PlannerAuditRecorder::new(
        Some(ccos.get_causal_chain()),
        &plan_run_id,
        &intent.intent_id,
    );
    planner_audit.log_json(
        "intent_initialized",
        &json!({
            "goal": goal,
            "intent_id": intent.intent_id,
            "plan_id": plan_run_id,
        }),
    );

    let marketplace = ccos.get_capability_marketplace();
    preload_discovered_capabilities_if_needed(marketplace.as_ref())
        .await
        .map_err(runtime_error)?;

    // FIX: Register adapters.mcp.parse-json-from-text-content manually with native implementation
    // because :local provider loaded from RTFS cannot execute 'call' or 'ccos.data.parse-json'.
    // marketplace.register_local_capability(
    //     "adapters.mcp.parse-json-from-text-content".to_string(),
    //     "Parse MCP Text Content as JSON".to_string(),
    //     "Extracts and parses a JSON string from a standard MCP text content envelope.".to_string(),
    //     Arc::new(|input: &Value| -> RuntimeResult<Value> {
    //         if std::env::var("CCOS_DEBUG").is_ok() {
    //             eprintln!("DEBUG: adapters.mcp.parse-json input: {:?}", input);
    //         }
    //         // Expect input: { :content [ { :text "..." } ] }
    //         // Fallback for LLM using text_content instead of content
    //         let content = match input {
    //             Value::Map(m) => {
    //                 m.get(&rtfs::ast::MapKey::Keyword(rtfs::ast::Keyword("content".to_string())))
    //                     .or_else(|| m.get(&rtfs::ast::MapKey::Keyword(rtfs::ast::Keyword("text_content".to_string()))))
    //             },
    //             _ => None
    //         }.ok_or_else(|| RuntimeError::Generic("Missing :content (or :text_content) in input".to_string()))?;

    //         let text = match content {
    //             Value::Vector(v) => {
    //                 if let Some(first) = v.get(0) {
    //                     match first {
    //                         Value::Map(m) => {
    //                             m.get(&rtfs::ast::MapKey::Keyword(rtfs::ast::Keyword("text".to_string())))
    //                              .and_then(|v| v.as_string())
    //                         },
    //                         _ => None
    //                     }
    //                 } else {
    //                     None
    //                 }
    //             },
    //             _ => None
    //         }.ok_or_else(|| RuntimeError::Generic("Invalid content structure: expected vector with map containing :text".to_string()))?;

    //         let json_val: serde_json::Value = serde_json::from_str(text)
    //             .map_err(|e| RuntimeError::Generic(format!("Failed to parse JSON: {}", e)))?;

    //         // Convert serde_json::Value to RTFS Value
    //         fn json_to_rtfs(v: &serde_json::Value) -> Value {
    //             match v {
    //                 serde_json::Value::Null => Value::Nil,
    //                 serde_json::Value::Bool(b) => Value::Boolean(*b),
    //                 serde_json::Value::Number(n) => {
    //                     if let Some(i) = n.as_i64() { Value::Integer(i) }
    //                     else if let Some(f) = n.as_f64() { Value::Float(f) }
    //                     else { Value::String(n.to_string()) }
    //                 },
    //                 serde_json::Value::String(s) => Value::String(s.clone()),
    //                 serde_json::Value::Array(a) => Value::Vector(a.iter().map(json_to_rtfs).collect()),
    //                 serde_json::Value::Object(o) => {
    //                     let mut map = HashMap::new();
    //                     for (k, v) in o {
    //                         map.insert(
    //                             rtfs::ast::MapKey::Keyword(rtfs::ast::Keyword(k.clone())),
    //                             json_to_rtfs(v)
    //                         );
    //                     }
    //                     Value::Map(map)
    //                 }
    //             }
    //         }

    //         let result = json_to_rtfs(&json_val);
    //         if std::env::var("CCOS_DEBUG").is_ok() {
    //             eprintln!("DEBUG: adapters.mcp.parse-json result: {:?}", result);
    //         }
    //         Ok(result)
    //     })
    // ).await.map_err(runtime_error)?;

    let catalog = ccos.get_catalog();
    let mut signals = GoalSignals::from_goal_and_intent(&goal, &intent);
    signals.apply_catalog_search(&catalog, 0.5, 10);
    if let Ok(signals_json) = serde_json::to_value(&signals) {
        planner_audit.log_json("signals_initialized", &signals_json);
    }
    let marketplace_for_resolver = marketplace.clone();
    let resolver_available = ccos.get_missing_capability_resolver().is_some();
    if !resolver_available {
        eprintln!("⚠️ WARNING: MissingCapabilityResolver is not available - capability discovery will be disabled");
        eprintln!("    This means missing capabilities like 'get_issues' cannot be automatically discovered");
        eprintln!("    Make sure the resolver is configured in CCOS initialization");
    } else {
        eprintln!("✅ MissingCapabilityResolver is available - capability discovery is enabled");
    }

    let resolver_adapter = ccos.get_missing_capability_resolver().map(|resolver| {
        let resolver = resolver.clone();
        let marketplace = marketplace_for_resolver.clone();
        Arc::new(move |capability_id: String, signals: &GoalSignals| {
            let resolver = resolver.clone();
            let marketplace = marketplace.clone();
            let context = build_resolution_context(signals, &capability_id);
            let signals_snapshot = signals.clone();
            async move {
                let request = MissingCapabilityRequest {
                    capability_id: capability_id.clone(),
                    arguments: Vec::new(),
                    context,
                    requested_at: SystemTime::now(),
                    attempt_count: 0,
                };
                let capability_ref = capability_id.clone();
                match resolver.resolve_capability(&request).await {
                    Ok(ResolutionResult::Resolved {
                        capability_id: resolved_id,
                        resolution_method,
                        provider_info,
                    }) => {
                        if let Some(manifest) = marketplace.get_capability(&resolved_id).await {
                            let capability = ResolvedCapabilityInfo {
                                manifest,
                                resolution_method: Some(resolution_method.clone()),
                                provider_info,
                            };
                            if resolution_method == "llm_synthesis" {
                                Ok(CapabilityProvisionAction::Synthesized {
                                    capability,
                                    tests_run: Vec::new(),
                                })
                            } else {
                                Ok(CapabilityProvisionAction::Discovered { capability })
                            }
                        } else {
                            let reason =
                                "Capability manifest not found after resolution".to_string();
                            let ticket = register_external_capability_request(
                                &resolved_id,
                                &signals_snapshot,
                                &reason,
                            );
                            Ok(CapabilityProvisionAction::PendingExternal {
                                capability_id: resolved_id,
                                request_id: ticket,
                                suggested_human_action: Some(
                                    "Restore or synthesize the capability manifest".to_string(),
                                ),
                            })
                        }
                    }
                    Ok(ResolutionResult::Failed {
                        capability_id: failed_id,
                        reason,
                        ..
                    }) => Ok(CapabilityProvisionAction::Failed {
                        capability_id: failed_id,
                        reason,
                        recoverable: true,
                    }),
                    Ok(ResolutionResult::PermanentlyFailed {
                        capability_id: failed_id,
                        reason,
                    }) => {
                        let ticket = register_external_capability_request(
                            &failed_id,
                            &signals_snapshot,
                            &reason,
                        );
                        Ok(CapabilityProvisionAction::PendingExternal {
                            capability_id: failed_id,
                            request_id: ticket,
                            suggested_human_action: Some(reason),
                        })
                    }
                    Err(err) => Ok(CapabilityProvisionAction::Failed {
                        capability_id: capability_ref,
                        reason: err.to_string(),
                        recoverable: false,
                    }),
                }
            }
            .boxed_local()
        }) as CapabilityProvisionFn
    });
    let requirement_resolver = RequirementResolver::new(resolver_adapter);
    let mut menu = refresh_capability_menu(
        catalog.clone(),
        marketplace.clone(),
        &goal,
        &intent,
        &mut signals,
        12,
    )
    .await
    .map_err(runtime_error)?;
    if let Ok(menu_json) = serde_json::to_value(&menu) {
        planner_audit.log_json("menu_refreshed", &menu_json);
    }

    println!("\n{}", "Capability Menu".bold().cyan());
    for entry in &menu {
        println!(
            "  - {} (score: {:.1}) / required: {} / outputs: {}",
            entry.id,
            entry_score(entry),
            if entry.required_inputs.is_empty() {
                "(none)".to_string()
            } else {
                entry.required_inputs.join(", ")
            },
            if entry.outputs.is_empty() {
                "(unspecified)".to_string()
            } else {
                entry.outputs.join(", ")
            }
        );
    }

    let max_plan_attempts = args.max_attempts.clamp(1, 10);
    let mut attempt = 0usize;
    let mut feedback: Option<String> = None;
    let mut previous_plan: Option<String> = None;
    let analyzer = DefaultGoalCoverageAnalyzer;
    let steps = loop {
        attempt += 1;
        let known_inputs = signals.constraints_map();
        planner_audit.log_json(
            &format!("plan_attempt_{}_inputs", attempt),
            &json!({
                "attempt": attempt,
                "known_inputs": constraint_map_to_json(&known_inputs),
            }),
        );
        let (raw_response, steps_result) = match propose_plan_steps_with_menu_and_capture(
            delegating,
            &goal,
            &intent,
            &known_inputs,
            &menu,
            args.debug_prompts,
            feedback.as_deref(),
            previous_plan.as_deref(),
            Some(&signals),
        )
        .await
        {
            Ok((raw, steps)) => {
                // Store the successfully parsed plan for next iteration
                previous_plan = Some(raw.clone());
                planner_audit.log_text(&format!("plan_attempt_{}_raw_response", attempt), &raw);
                (Some(raw), Ok(steps))
            }
            Err((raw, err)) => {
                if let Some(ref raw_text) = raw {
                    planner_audit
                        .log_text(&format!("plan_attempt_{}_raw_response", attempt), raw_text);
                }
                (raw, Err(err))
            }
        };

        let mut steps = match steps_result {
            Ok(steps) => steps,
            Err(err) => {
                let err_msg = err.to_string();
                planner_audit.log_text(&format!("plan_attempt_{}_parse_error", attempt), &err_msg);
                println!(
                    "\n{}",
                    "Plan synthesis failed to produce valid JSON steps:"
                        .red()
                        .bold()
                );
                println!("  - {}", err_msg.as_str().red());

                if attempt >= max_plan_attempts {
                    return Err(runtime_error(err));
                }

                // Extract notes from the raw response if available
                let notes_hint = if let Some(ref raw) = raw_response {
                    extract_notes_from_partial_json(raw)
                } else {
                    None
                };

                let mut feedback_parts = vec![format!(
                    "Previous attempt failed to produce valid plan steps JSON ({}). Ensure each step object includes the keys id, name, capability_id, inputs, outputs, and optional notes.",
                    err_msg
                )];

                if let Some(notes) = notes_hint {
                    feedback_parts.push(format!(
                        "\nNote: The previous attempt's plan steps contained these insights in their notes:\n{}",
                        notes
                    ));
                }

                if let Some(ref prev) = previous_plan {
                    feedback_parts.push(format!(
                        "\nPrevious plan attempt (for reference):\n{}",
                        prev
                    ));
                }

                feedback = Some(feedback_parts.join("\n"));
                previous_plan = raw_response;

                println!(
                    "{}",
                    "Retrying plan synthesis with corrective feedback...".yellow()
                );
                continue;
            }
        };
        let steps_snapshot = serialize_plan_steps_for_logging(&steps);
        planner_audit.log_json(
            &format!("plan_attempt_{}_steps", attempt),
            &json!({
                "attempt": attempt,
                "steps": steps_snapshot,
            }),
        );

        // Note: previous_plan is already stored in the Ok branch above
        let validation = validate_plan_steps_against_menu(&steps, &menu);

        if !validation.schema_errors.is_empty() {
            planner_audit.log_json(
                &format!("plan_attempt_{}_validation_failed", attempt),
                &json!({
                    "attempt": attempt,
                    "errors": validation.schema_errors,
                }),
            );
            println!(
                "\n{}",
                "Schema validation failed for the proposed steps:"
                    .red()
                    .bold()
            );
            for message in &validation.schema_errors {
                println!("  - {}", message.as_str().red());
            }

            if attempt >= max_plan_attempts {
                let summary = validation.schema_errors.join("; ");
                return Err(runtime_error(RuntimeError::Generic(format!(
                    "Planner could not produce schema-compliant steps after {} attempt(s): {}",
                    attempt, summary
                ))));
            }

            let summary = validation
                .schema_errors
                .iter()
                .map(|msg| format!("- {}", msg))
                .collect::<Vec<_>>()
                .join("\n");

            // Check if any errors are about RTFS code misuse
            let has_rtfs_misuse = validation
                .schema_errors
                .iter()
                .any(|e| e.contains("RTFS code") || e.contains("looks like RTFS code"));

            let mut feedback_parts = vec![format!(
                "Schema validation errors:\n{}\nEnsure you only use the required/optional inputs listed for each capability and provide every required field.",
                summary
            )];

            if has_rtfs_misuse {
                feedback_parts.push(
                    "IMPORTANT: Do NOT pass code or function-like syntax as string values to regular parameters. Parameters must receive values matching their declared type:\n  - String parameters expect plain text (e.g., \"label1\", \"RTFS\")\n  - Number parameters expect numbers (e.g., 100, 1)\n  - Array parameters expect JSON arrays (e.g., [\"item1\", \"item2\"])\n  - Function parameters (marked as \"(function - cannot be passed directly)\") can accept RTFS code via {\"rtfs\": \"...\"}\nIf you need filtering or transformation, use a separate step with a capability that accepts function parameters.".to_string()
                );
            }

            // Include previous plan for context
            if let Some(ref prev) = previous_plan {
                feedback_parts.push(format!(
                    "\nPrevious plan attempt (for reference):\n{}",
                    prev
                ));
            }

            feedback = Some(feedback_parts.join("\n"));
            println!(
                "{}",
                "Re-submitting plan synthesis request with schema feedback...".yellow()
            );
            continue;
        }

        // Clean up stale requirements: remove requirements for capabilities that are no longer in the plan
        // This prevents old requirements (e.g., from previous plan iterations) from causing false negatives
        let plan_capability_ids: HashSet<String> = steps
            .iter()
            .map(|step| step.capability_id.clone())
            .collect();
        signals.requirements.retain(|req| {
            if let Some(cap_id) = req.capability_id() {
                // Keep requirement if capability is in current plan OR if it's not a MustCallCapability
                plan_capability_ids.contains(cap_id)
                    || !matches!(req.kind, GoalRequirementKind::MustCallCapability { .. })
            } else {
                // Keep non-capability requirements (filter, class, etc.)
                true
            }
        });

        if !validation.unknown_capabilities.is_empty() {
            println!(
                "\n{}",
                "Plan references capabilities that are not yet available:".yellow()
            );
        }
        for unknown in &validation.unknown_capabilities {
            println!(
                "  - Step '{}' requests '{}'",
                unknown.step_id, unknown.capability_id
            );
            let summary = unknown
                .notes
                .clone()
                .or_else(|| unknown.step_name.clone())
                .unwrap_or_else(|| {
                    format!(
                        "Plan step {} requires capability {}",
                        unknown.step_id, unknown.capability_id
                    )
                });
            signals.ensure_must_call_capability(&unknown.capability_id, Some(summary.clone()));
            let mut metadata = Vec::new();
            metadata.push((
                "origin_step_id".to_string(),
                Value::String(unknown.step_id.clone()),
            ));
            if let Some(name) = unknown.step_name.as_ref() {
                metadata.push(("origin_step_name".to_string(), Value::String(name.clone())));
            }
            if let Some(notes) = unknown.notes.as_ref() {
                metadata.push(("origin_notes".to_string(), Value::String(notes.clone())));
            }
            if !unknown.outputs.is_empty() {
                metadata.push((
                    "requested_outputs".to_string(),
                    Value::Vector(unknown.outputs.iter().cloned().map(Value::String).collect()),
                ));
            }
            signals.update_capability_requirement(
                &unknown.capability_id,
                RequirementReadiness::Identified,
                metadata.into_iter(),
            );
            // Capability will be resolved via discovery/synthesis
        }

        let plan_summaries = summarize_plan_steps(&steps);
        let coverage = analyzer.evaluate(&signals, &plan_summaries, &menu);
        if let Ok(coverage_json) = serde_json::to_value(&coverage) {
            planner_audit.log_json(
                &format!("plan_attempt_{}_coverage", attempt),
                &coverage_json,
            );
        }

        if matches!(coverage.status, CoverageStatus::Satisfied) {
            break steps;
        }

        let coverage_summary = if coverage.unmet_requirements.is_empty() {
            "(no detailed explanations available)".to_string()
        } else {
            coverage
                .unmet_requirements
                .iter()
                .map(|gap| format!("- {}", gap.explanation))
                .collect::<Vec<_>>()
                .join("\n")
        };

        let mut feedback_messages = Vec::new();
        if !coverage.unmet_requirements.is_empty() {
            let mut detailed_feedback =
                format!("Goal requirements remain unmet:\n{}", coverage_summary);

            // Add specific hints for MustFilter requirements
            let filter_requirements: Vec<_> = coverage
                .unmet_requirements
                .iter()
                .filter(|gap| {
                    matches!(gap.requirement.kind, GoalRequirementKind::MustFilter { .. })
                })
                .collect();

            if !filter_requirements.is_empty() {
                detailed_feedback.push_str("\n\nHints for filtering:");
                detailed_feedback
                    .push_str("\n- Add a separate filtering step after fetching the data");
                detailed_feedback.push_str("\n- The filtering step should check if the data contains the expected filter value");
                detailed_feedback.push_str("\n- Use a capability that can filter records/data, or synthesize one if needed");

                for gap in filter_requirements {
                    if let GoalRequirementKind::MustFilter { expected_value, .. } =
                        &gap.requirement.kind
                    {
                        if let Some(value) = expected_value {
                            let value_str = match value {
                                rtfs::runtime::values::Value::String(s) => s.clone(),
                                _ => format!("{:?}", value),
                            };
                            detailed_feedback
                                .push_str(&format!("\n- Filter value to match: '{}'", value_str));
                        }
                    }
                }
            }

            feedback_messages.push(detailed_feedback);
        } else {
            feedback_messages.push("Goal requirements remain unmet.".to_string());
        }

        if !validation.unknown_capabilities.is_empty() {
            feedback_messages.push(format!(
                "The plan references new capabilities that need to be provisioned: {}.",
                validation
                    .unknown_capabilities
                    .iter()
                    .map(|c| c.capability_id.clone())
                    .collect::<Vec<_>>()
                    .join(", ")
            ));
        }

        if !coverage.incomplete_capabilities.is_empty() {
            feedback_messages.push(format!(
                "The following capabilities are incomplete and will be synthesized: {}.",
                coverage.incomplete_capabilities.join(", ")
            ));
        }

        if !coverage.pending_capabilities.is_empty() {
            feedback_messages.push(format!(
                "External provisioning is in progress for: {}.",
                coverage.pending_capabilities.join(", ")
            ));
        }

        annotate_menu_with_readiness(&signals, &mut menu);

        // Debug: Check what capabilities need to be provisioned
        let provision_targets = coverage.provision_targets();
        if !provision_targets.is_empty() {
            eprintln!("🔍 PROVISION TARGETS: {:?}", provision_targets);
        }
        let signals_identified: Vec<String> = signals
            .requirements
            .iter()
            .filter(|req| req.readiness == RequirementReadiness::Identified)
            .filter_map(|req| req.capability_id().map(|s| s.to_string()))
            .collect();
        if !signals_identified.is_empty() {
            eprintln!("🔍 SIGNALS IDENTIFIED: {:?}", signals_identified);
        }

        eprintln!(
            "🔍 CALLING ensure_capabilities with coverage: {} missing, {} incomplete, {} pending",
            coverage.missing_capabilities.len(),
            coverage.incomplete_capabilities.len(),
            coverage.pending_capabilities.len()
        );

        let mut discovery_events: Vec<CapabilityDiscoveryEvent> = Vec::new();
        match requirement_resolver
            .ensure_capabilities(&coverage, &signals)
            .await
        {
            Ok(RequirementResolutionOutcome::Synthesized {
                capabilities,
                tests_run,
                pending_requests,
            }) => {
                let mut synthesized_ids = Vec::new();
                let test_summaries: Vec<String> = tests_run
                    .iter()
                    .map(|test| {
                        let status = if test.success { "pass" } else { "fail" };
                        let mut summary = format!("{}:{}ms", status, test.execution_time_ms);
                        if !test.success {
                            if let Some(err) = test.error.as_ref() {
                                summary.push_str(" - ");
                                summary.push_str(err);
                            }
                        }
                        summary
                    })
                    .collect();
                let tests_metadata_value = if test_summaries.is_empty() {
                    None
                } else {
                    Some(Value::Vector(
                        test_summaries.iter().cloned().map(Value::String).collect(),
                    ))
                };
                for capability in &capabilities {
                    let capability_id = capability.manifest.id.clone();
                    synthesized_ids.push(capability_id.clone());
                    discovery_events.push(CapabilityDiscoveryEvent {
                        capability_id: capability_id.clone(),
                        capability_name: Some(capability.manifest.name.clone()),
                        status: CapabilityDiscoveryStatus::Synthesized,
                        source: capability.provider_info.clone(),
                        resolution_method: capability.resolution_method.clone(),
                        request_id: None,
                        notes: test_summaries.clone(),
                    });
                    let mut metadata = vec![
                        (
                            "capability_status".to_string(),
                            Value::String("available".to_string()),
                        ),
                        (
                            "provision_source".to_string(),
                            Value::String("synthesized".to_string()),
                        ),
                        (
                            "capability_name".to_string(),
                            Value::String(capability.manifest.name.clone()),
                        ),
                        (
                            "capability_version".to_string(),
                            Value::String(capability.manifest.version.clone()),
                        ),
                    ];
                    if let Some(method) = capability.resolution_method.as_ref() {
                        metadata.push((
                            "resolution_method".to_string(),
                            Value::String(method.clone()),
                        ));
                    }
                    if let Some(info) = capability.provider_info.as_ref() {
                        metadata.push(("provider_info".to_string(), Value::String(info.clone())));
                    }
                    if let Some(ref tests_value) = tests_metadata_value {
                        metadata.push(("synthesis_tests".to_string(), tests_value.clone()));
                    }
                    signals.update_capability_requirement(
                        &capability_id,
                        RequirementReadiness::Available,
                        metadata,
                    );
                    signals.set_provision_source(
                        &capability_id,
                        Some(CapabilityProvisionSource::Synthesized),
                    );
                    signals.set_pending_request_id(&capability_id, None);
                }
                if !pending_requests.is_empty() {
                    for pending in &pending_requests {
                        discovery_events.push(CapabilityDiscoveryEvent {
                            capability_id: pending.capability_id.clone(),
                            capability_name: None,
                            status: CapabilityDiscoveryStatus::PendingExternal,
                            source: None,
                            resolution_method: None,
                            request_id: pending.request_id.clone(),
                            notes: pending
                                .suggested_human_action
                                .as_ref()
                                .map(|s| vec![s.clone()])
                                .unwrap_or_default(),
                        });
                    }
                    let (pending_ids, actions) =
                        register_pending_requests(&mut signals, &mut menu, pending_requests);
                    if !pending_ids.is_empty() {
                        feedback_messages.push(format!(
                            "Capability sourcing requests were filed for: {}.",
                            pending_ids.join(", ")
                        ));
                    }
                    if !actions.is_empty() {
                        feedback_messages
                            .push(format!("Suggested follow-ups:\n{}", actions.join("\n")));
                    }
                }
                if !synthesized_ids.is_empty() {
                    println!(
                        "{}",
                        format!(
                            "🛠 Synthesized new capabilities: {}",
                            synthesized_ids.join(", ")
                        )
                        .green()
                    );
                    menu = refresh_capability_menu(
                        catalog.clone(),
                        marketplace.clone(),
                        &goal,
                        &intent,
                        &mut signals,
                        12,
                    )
                    .await
                    .map_err(runtime_error)?;
                    if let Ok(menu_json) = serde_json::to_value(&menu) {
                        planner_audit.log_json("menu_refreshed", &menu_json);
                    }
                    if let Ok(menu_json) = serde_json::to_value(&menu) {
                        planner_audit.log_json("menu_refreshed", &menu_json);
                    }
                    let mut loop_feedback = format!(
                        "Synthesized capabilities ({}) are available. Regenerate your plan with the updated menu.",
                        synthesized_ids.join(", ")
                    );
                    if !test_summaries.is_empty() {
                        loop_feedback.push_str("\nSynthesis tests:\n");
                        loop_feedback.push_str(&test_summaries.join("\n"));
                    }
                    feedback = Some(loop_feedback);
                    println!(
                        "{}",
                        "Re-running plan synthesis after capability synthesis...".yellow()
                    );
                    continue;
                }
            }
            Ok(RequirementResolutionOutcome::CapabilitiesDiscovered {
                capabilities,
                pending_requests,
            }) => {
                let mut discovered_ids = Vec::new();
                let mut capability_mappings_for_feedback = Vec::new(); // Track mappings for LLM feedback

                // Track mappings between requested IDs and discovered IDs
                // by looking at signals requirements that were identified but now resolved
                for capability in &capabilities {
                    let discovered_id = capability.manifest.id.clone();
                    discovered_ids.push(discovered_id.clone());
                    discovery_events.push(CapabilityDiscoveryEvent {
                        capability_id: discovered_id.clone(),
                        capability_name: Some(capability.manifest.name.clone()),
                        status: CapabilityDiscoveryStatus::Discovered,
                        source: capability.provider_info.clone(),
                        resolution_method: capability.resolution_method.clone(),
                        request_id: None,
                        notes: Vec::new(),
                    });

                    // Find the requested ID by checking unknown capabilities from validation
                    // These are capabilities that were in the plan but not found in the menu
                    let mut metadata = vec![
                        (
                            "capability_status".to_string(),
                            Value::String("available".to_string()),
                        ),
                        (
                            "provision_source".to_string(),
                            Value::String("missing_capability_resolver".to_string()),
                        ),
                        (
                            "capability_name".to_string(),
                            Value::String(capability.manifest.name.clone()),
                        ),
                        (
                            "capability_version".to_string(),
                            Value::String(capability.manifest.version.clone()),
                        ),
                    ];
                    if let Some(method) = capability.resolution_method.as_ref() {
                        metadata.push((
                            "resolution_method".to_string(),
                            Value::String(method.clone()),
                        ));
                    }
                    if let Some(info) = capability.provider_info.as_ref() {
                        metadata.push(("provider_info".to_string(), Value::String(info.clone())));
                    }
                    let discovered_id = capability.manifest.id.clone();
                    signals.update_capability_requirement(
                        &discovered_id,
                        RequirementReadiness::Available,
                        metadata,
                    );
                    signals.set_provision_source(
                        &discovered_id,
                        Some(CapabilityProvisionSource::ExistingManifest),
                    );
                    signals.set_pending_request_id(&discovered_id, None);
                }
                if !pending_requests.is_empty() {
                    for pending in &pending_requests {
                        discovery_events.push(CapabilityDiscoveryEvent {
                            capability_id: pending.capability_id.clone(),
                            capability_name: None,
                            status: CapabilityDiscoveryStatus::PendingExternal,
                            source: None,
                            resolution_method: None,
                            request_id: pending.request_id.clone(),
                            notes: pending
                                .suggested_human_action
                                .as_ref()
                                .map(|s| vec![s.clone()])
                                .unwrap_or_default(),
                        });
                    }
                    let (pending_ids, actions) =
                        register_pending_requests(&mut signals, &mut menu, pending_requests);
                    if !pending_ids.is_empty() {
                        feedback_messages.push(format!(
                            "Capability sourcing requests were filed for: {}.",
                            pending_ids.join(", ")
                        ));
                    }
                    if !actions.is_empty() {
                        feedback_messages
                            .push(format!("Suggested follow-ups:\n{}", actions.join("\n")));
                    }
                }
                if !discovered_ids.is_empty() {
                    // Update plan steps: if any step references an unknown capability that matches a discovered one
                    // by checking unknown_capabilities from the validation
                    let mut plan_updated = false;
                    if !validation.unknown_capabilities.is_empty() {
                        for unknown in &validation.unknown_capabilities {
                            // Try to find a discovered capability that semantically matches
                            for capability in &capabilities {
                                let discovered_id = &capability.manifest.id;
                                // Check if discovered capability semantically matches requested one
                                // Simple heuristic: if both end with the same word (e.g., "issues"), match them
                                let unknown_last =
                                    unknown.capability_id.split('.').last().unwrap_or("");
                                let discovered_last = discovered_id.split('.').last().unwrap_or("");
                                // Also check for synonym matching (get/list)
                                if unknown_last == discovered_last
                                    || (unknown_last.contains("get")
                                        && discovered_last.contains("list"))
                                    || (unknown_last.contains("list")
                                        && discovered_last.contains("get"))
                                {
                                    // Found a match! Update the step and signals requirements
                                    let requested_id = unknown.capability_id.clone();
                                    if let Some(step) =
                                        steps.iter_mut().find(|s| s.id == unknown.step_id)
                                    {
                                        if step.capability_id == requested_id {
                                            eprintln!(
                                                "🔄 PLAN UPDATE: Step '{}': '{}' -> '{}'",
                                                step.id, step.capability_id, discovered_id
                                            );
                                            step.capability_id = discovered_id.clone();

                                            // Track the mapping for feedback to LLM
                                            if requested_id != *discovered_id {
                                                capability_mappings_for_feedback.push(format!(
                                                    "'{}' maps to '{}'",
                                                    requested_id, discovered_id
                                                ));
                                            }

                                            // Also update signals requirements to map requested ID -> discovered ID
                                            // This ensures coverage checks will find the discovered capability
                                            eprintln!(
                                                "🔄 SIGNALS UPDATE: Mapping requirement '{}' -> '{}'",
                                                requested_id, discovered_id
                                            );

                                            // Remove the old requirement with requested ID and add one with discovered ID
                                            signals.requirements.retain(|req| {
                                                if let Some(req_cap_id) = req.capability_id() {
                                                    req_cap_id != requested_id
                                                } else {
                                                    true
                                                }
                                            });

                                            // Add a new requirement with the discovered ID to ensure it's tracked
                                            signals.ensure_must_call_capability(
                                                discovered_id,
                                                Some(format!(
                                                    "Discovered capability matching requested '{}'",
                                                    requested_id
                                                )),
                                            );
                                            // Mark it as available since we just discovered it
                                            signals.update_capability_requirement(
                                                discovered_id,
                                                RequirementReadiness::Available,
                                                vec![
                                                    (
                                                        "mapped_from".to_string(),
                                                        Value::String(requested_id.clone()),
                                                    ),
                                                    (
                                                        "discovery_status".to_string(),
                                                        Value::String("discovered".to_string()),
                                                    ),
                                                ],
                                            );
                                            plan_updated = true;
                                        }
                                    }
                                }
                            }
                        }
                        // Re-validate after updating (but refresh menu FIRST to ensure schema validation has correct inputs)
                        if plan_updated {
                            eprintln!("✅ Plan updated to use discovered capabilities - refreshing menu and re-validating...");
                            // CRITICAL: Refresh menu FIRST before validation so we have the correct schema
                            menu = refresh_capability_menu(
                                catalog.clone(),
                                marketplace.clone(),
                                &goal,
                                &intent,
                                &mut signals,
                                12,
                            )
                            .await
                            .map_err(runtime_error)?;
                            if let Ok(menu_json) = serde_json::to_value(&menu) {
                                planner_audit.log_json("menu_refreshed", &menu_json);
                            }

                            // NOW validate against the refreshed menu (which includes discovered capabilities with correct schemas)
                            let updated_validation =
                                validate_plan_steps_against_menu(&steps, &menu);

                            // Log validation results
                            if !updated_validation.schema_errors.is_empty() {
                                eprintln!(
                                    "⚠️ Schema validation errors after capability discovery:"
                                );
                                for err in &updated_validation.schema_errors {
                                    eprintln!("  - {}", err);
                                }
                                // Store schema errors in feedback and continue to re-run synthesis
                                let summary = updated_validation
                                    .schema_errors
                                    .iter()
                                    .map(|msg| format!("- {}", msg))
                                    .collect::<Vec<_>>()
                                    .join("\n");

                                feedback = Some(format!(
                                    "Plan was updated to use discovered capability '{}', but schema validation failed:\n{}\n\nPlease update the input field names to match the discovered capability's schema.",
                                    discovered_ids.join(", "),
                                    summary
                                ));
                                println!(
                                    "{}",
                                    "Re-running plan synthesis with schema feedback...".yellow()
                                );
                                continue; // Re-run synthesis with schema feedback
                            }

                            if !updated_validation.unknown_capabilities.is_empty() {
                                eprintln!("⚠️ Plan still has {} unknown capabilities after discovery - continuing discovery loop",
                                    updated_validation.unknown_capabilities.len());
                                // Continue the loop to discover remaining capabilities
                                for unknown in &updated_validation.unknown_capabilities {
                                    signals.ensure_must_call_capability(
                                        &unknown.capability_id,
                                        Some(format!(
                                            "Plan step {} requires capability {}",
                                            unknown.step_id, unknown.capability_id
                                        )),
                                    );
                                }
                                continue;
                            }

                            eprintln!("✅ Plan updated successfully - all steps now reference discovered capabilities with valid inputs");

                            // Re-check coverage to see if all requirements are now satisfied
                            // (e.g., filter requirement might now be satisfied)
                            let updated_plan_summaries = summarize_plan_steps(&steps);
                            let updated_coverage =
                                analyzer.evaluate(&signals, &updated_plan_summaries, &menu);

                            if matches!(updated_coverage.status, CoverageStatus::Satisfied) {
                                eprintln!(
                                    "✅ All requirements satisfied after capability discovery"
                                );
                                break steps; // Exit loop with updated plan
                            } else {
                                eprintln!(
                                    "⚠️ Some requirements still unmet after capability discovery:"
                                );
                                for gap in &updated_coverage.unmet_requirements {
                                    eprintln!("  - {}", gap.explanation);
                                }

                                // Check if there are missing capabilities that need to be discovered
                                // (e.g., filter capability might need to be discovered/scaffolded)
                                let updated_provision_targets =
                                    updated_coverage.provision_targets();
                                if !updated_provision_targets.is_empty() {
                                    eprintln!(
                                        "🔍 Attempting to discover remaining capabilities: {:?}",
                                        updated_provision_targets
                                    );
                                    // Continue the loop - the normal flow will handle discovery of remaining capabilities
                                } else {
                                    eprintln!("⚠️ No missing capabilities to discover - continuing with synthesis loop");
                                }
                                // Continue the loop to handle remaining unmet requirements
                            }
                        }
                    }

                    println!(
                        "{}",
                        format!(
                            "✨ Discovered new capabilities: {}",
                            discovered_ids.join(", ")
                        )
                        .green()
                    );

                    // CRITICAL: Refresh menu FIRST with all discovered capabilities before checking for unknowns
                    // This ensures that when we check for unknown capabilities, the menu includes the newly discovered ones
                    menu = refresh_capability_menu(
                        catalog.clone(),
                        marketplace.clone(),
                        &goal,
                        &intent,
                        &mut signals,
                        12,
                    )
                    .await
                    .map_err(runtime_error)?;

                    // After discovering capabilities and updating the plan,
                    // re-validate to check if there are OTHER missing capabilities to discover
                    // BEFORE re-running synthesis. This is more efficient than re-running synthesis
                    // after each individual discovery.
                    if plan_updated {
                        let updated_validation = validate_plan_steps_against_menu(&steps, &menu);
                        if !updated_validation.unknown_capabilities.is_empty() {
                            eprintln!("⚠️ Plan still has {} unknown capabilities after discovery - discovering remaining ones...",
                                updated_validation.unknown_capabilities.len());
                            // Continue the loop to discover remaining capabilities
                            // Add them to signals so they get discovered in the next iteration
                            for unknown in &updated_validation.unknown_capabilities {
                                signals.ensure_must_call_capability(
                                    &unknown.capability_id,
                                    Some(format!(
                                        "Plan step {} requires capability {}",
                                        unknown.step_id, unknown.capability_id
                                    )),
                                );
                            }
                            // Continue the loop to discover the remaining capabilities
                            continue;
                        }
                    }

                    // Re-check coverage after all discoveries
                    let updated_plan_summaries = summarize_plan_steps(&steps);
                    let updated_coverage =
                        analyzer.evaluate(&signals, &updated_plan_summaries, &menu);

                    // Check if there are still missing capabilities to discover
                    let remaining_provision_targets = updated_coverage.provision_targets();
                    if !remaining_provision_targets.is_empty() {
                        eprintln!("🔍 Still have {} capabilities to discover: {:?} - continuing discovery loop",
                            remaining_provision_targets.len(), remaining_provision_targets);
                        // Continue the loop to discover remaining capabilities
                        continue;
                    }

                    // Only re-run synthesis if all capabilities are discovered but requirements still unmet
                    if matches!(updated_coverage.status, CoverageStatus::Satisfied) {
                        eprintln!("✅ All requirements satisfied after capability discovery");
                        break steps; // Exit with updated plan
                    } else {
                        // All capabilities discovered but requirements still unmet - re-run synthesis
                        // Add feedback about discovered capabilities so LLM knows about them
                        let mut capability_info = format!(
                            "New capabilities ({}) were registered and are now available in the menu.",
                            discovered_ids.join(", ")
                        );

                        // Inform about capability mappings so LLM knows to use the discovered IDs
                        if !capability_mappings_for_feedback.is_empty() {
                            capability_info.push_str("\n\nImportant capability mappings:");
                            for mapping in &capability_mappings_for_feedback {
                                capability_info.push_str(&format!("\n- {}", mapping));
                            }
                            capability_info.push_str("\nPlease use the discovered capability IDs (not the requested ones) in your plan.");
                        }

                        // Also inform about plan update if any
                        if plan_updated {
                            capability_info.push_str("\nNote: The current plan has been updated to use the discovered capabilities.");
                        }

                        feedback = Some(capability_info);
                        println!(
                            "{}",
                            "All missing capabilities discovered - re-running plan synthesis..."
                                .yellow()
                        );
                        continue;
                    }
                }
            }
            Ok(RequirementResolutionOutcome::AwaitingExternal {
                capability_requests,
            }) => {
                if !capability_requests.is_empty() {
                    for pending in &capability_requests {
                        discovery_events.push(CapabilityDiscoveryEvent {
                            capability_id: pending.capability_id.clone(),
                            capability_name: None,
                            status: CapabilityDiscoveryStatus::PendingExternal,
                            source: None,
                            resolution_method: None,
                            request_id: pending.request_id.clone(),
                            notes: pending
                                .suggested_human_action
                                .as_ref()
                                .map(|s| vec![s.clone()])
                                .unwrap_or_default(),
                        });
                    }
                    let (pending_ids, actions) =
                        register_pending_requests(&mut signals, &mut menu, capability_requests);
                    if !pending_ids.is_empty() {
                        feedback_messages.push(format!(
                            "Awaiting external implementations for: {}.",
                            pending_ids.join(", ")
                        ));
                    }
                    if !actions.is_empty() {
                        feedback_messages
                            .push(format!("Suggested follow-ups:\n{}", actions.join("\n")));
                    }
                }
            }
            Ok(RequirementResolutionOutcome::Failed {
                reason,
                recoverable,
            }) => {
                println!(
                    "{}",
                    format!("Requirement resolution failed: {}", reason).red()
                );
                discovery_events.push(CapabilityDiscoveryEvent {
                    capability_id: "unknown".to_string(),
                    capability_name: None,
                    status: CapabilityDiscoveryStatus::Failed,
                    source: None,
                    resolution_method: None,
                    request_id: None,
                    notes: vec![reason.clone()],
                });
                feedback_messages.push(if recoverable {
                    format!(
                        "Automatic capability provisioning failed but is retryable: {}.",
                        reason
                    )
                } else {
                    format!(
                        "Automatic capability provisioning failed and appears fatal: {}.",
                        reason
                    )
                });
            }
            Ok(RequirementResolutionOutcome::NoAction) => {
                eprintln!("⚠️ RESOLUTION: NoAction - no capabilities were provisioned");

                // Even if no capabilities need to be provisioned, check if requirements are still unmet
                // (e.g., MustFilter requirement might not be satisfied even though all capabilities exist)
                if !coverage.unmet_requirements.is_empty() {
                    // Check for unmet MustFilter requirements and try to synthesize a filtering capability
                    let filter_requirements: Vec<_> = coverage
                        .unmet_requirements
                        .iter()
                        .filter(|gap| {
                            matches!(gap.requirement.kind, GoalRequirementKind::MustFilter { .. })
                        })
                        .collect();

                    if !filter_requirements.is_empty() {
                        // Check if there's a filtering capability in the menu
                        let has_filter_capability = menu.iter().any(|entry| {
                            entry.id.contains("filter")
                                || entry.description.to_lowercase().contains("filter")
                                || entry.id == "mcp.core.filter"
                        });

                        if !has_filter_capability && attempt < max_plan_attempts {
                            eprintln!("⚠️ MustFilter requirement unmet but no filtering capability found - attempting synthesis");

                            // Try to synthesize a filtering capability by adding it to signals as an identified requirement
                            // This will cause the resolver to attempt synthesis/discovery in the next iteration
                            let filter_capability_id = "mcp.core.filter".to_string();

                            // Add as identified requirement so resolver picks it up
                            let mut metadata = BTreeMap::new();
                            metadata.insert(
                                "purpose".to_string(),
                                Value::String("filtering".to_string()),
                            );
                            metadata.insert(
                                "reason".to_string(),
                                Value::String("MustFilter requirement unmet".to_string()),
                            );

                            signals.add_requirement(GoalRequirement {
                                id: format!("filter-requirement-{}", filter_capability_id),
                                kind: GoalRequirementKind::MustCallCapability {
                                    capability_id: filter_capability_id.clone(),
                                },
                                priority: RequirementPriority::Must,
                                source: GoalSignalSource::Derived {
                                    rationale: Some(
                                        "MustFilter requirement unmet - need filtering capability"
                                            .to_string(),
                                    ),
                                },
                                metadata,
                                readiness: RequirementReadiness::Identified,
                                provision_source: None,
                                pending_request_id: None,
                                scaffold_summary: None,
                            });

                            // Force a capability provisioning attempt by calling ensure_capabilities again
                            // This will trigger synthesis/discovery of the filtering capability
                            let mut test_coverage = coverage.clone();
                            test_coverage
                                .missing_capabilities
                                .push(filter_capability_id.clone());

                            match requirement_resolver
                                .ensure_capabilities(&test_coverage, &signals)
                                .await
                            {
                                Ok(RequirementResolutionOutcome::Synthesized {
                                    capabilities,
                                    ..
                                })
                                | Ok(RequirementResolutionOutcome::CapabilitiesDiscovered {
                                    capabilities,
                                    ..
                                }) => {
                                    // Add synthesized/discovered capabilities to marketplace
                                    for capability in &capabilities {
                                        marketplace
                                            .register_capability_manifest(
                                                capability.manifest.clone(),
                                            )
                                            .await?;
                                        eprintln!(
                                            "✅ Added filtering capability: {}",
                                            capability.manifest.id
                                        );
                                    }

                                    // Refresh menu to include the new capability
                                    menu = build_capability_menu_from_catalog(
                                        catalog.clone(),
                                        marketplace.clone(),
                                        &goal,
                                        &intent,
                                        20,
                                    )
                                    .await?;

                                    feedback = Some(format!(
                                        "A filtering capability has been synthesized and added to the menu. Please add a filtering step to your plan using a filtering capability (e.g., 'mcp.core.filter')."
                                    ));
                                    println!(
                                        "{}",
                                        format!(
                                            "Re-running plan synthesis with newly synthesized filtering capability (attempt {}/{})...",
                                            attempt + 1,
                                            max_plan_attempts
                                        )
                                        .yellow()
                                    );
                                    continue; // Retry with new capability in menu
                                }
                                _ => {
                                    // Synthesis/discovery failed, continue with regular feedback
                                    eprintln!("⚠️ Could not synthesize/discover filtering capability, using feedback instead");
                                }
                            }
                        }
                    }

                    // Check if we should retry with feedback
                    if attempt < max_plan_attempts {
                        eprintln!("⚠️ Requirements still unmet despite all capabilities available - providing feedback for retry");
                        // Feedback has already been prepared above (lines 1741-1775)
                        if feedback.is_none() {
                            feedback = Some(feedback_messages.join("\n"));
                        }
                        println!(
                            "{}",
                            format!(
                                "Re-running plan synthesis with requirement feedback (attempt {}/{})...",
                                attempt + 1,
                                max_plan_attempts
                            )
                            .yellow()
                        );
                        continue; // Retry with feedback
                    } else {
                        // Max attempts reached - provide final error
                        let mut error_parts = vec![
                            "Planner could not satisfy all requirements after maximum attempts."
                                .to_string(),
                        ];
                        error_parts.extend(feedback_messages);
                        return Err(runtime_error(RuntimeError::Generic(error_parts.join("\n"))));
                    }
                }
                // No unmet requirements and no action needed - this shouldn't happen if coverage.check passed
                // but handle it gracefully
            }
            Err(err) => {
                println!("{}", format!("Requirement resolution error: {}", err).red());
                discovery_events.push(CapabilityDiscoveryEvent {
                    capability_id: "unknown".to_string(),
                    capability_name: None,
                    status: CapabilityDiscoveryStatus::Failed,
                    source: None,
                    resolution_method: None,
                    request_id: None,
                    notes: vec![err.to_string()],
                });
                feedback_messages.push(format!(
                    "Failed to resolve missing capabilities automatically ({}).",
                    err
                ));
            }
        }

        if !discovery_events.is_empty() {
            render_capability_discovery_summary(&discovery_events);
            planner_audit.log_json(
                &format!("plan_attempt_{}_discovery", attempt),
                &json!({
                    "attempt": attempt,
                    "events": discovery_events_to_json(&discovery_events),
                }),
            );
        }

        annotate_menu_with_readiness(&signals, &mut menu);

        if feedback_messages.is_empty() {
            feedback_messages.push(
                "Goal requirements remain unmet, but no additional diagnostics were produced."
                    .to_string(),
            );
        }

        let final_feedback = feedback_messages.join("\n\n");
        println!("{}", final_feedback);
        planner_audit.log_text("plan_failure", &final_feedback);
        return Err(runtime_error(RuntimeError::Generic(format!(
            "Planner awaiting external capability provisioning or intervention.\n{}",
            final_feedback
        ))));
    };

    render_intent_summary(&intent);
    render_plan_steps(&steps);
    planner_audit.log_json(
        "plan_steps_final",
        &serialize_plan_steps_for_logging(&steps),
    );

    let plan = assemble_plan_from_steps(&steps, &intent, Some(&plan_run_id), Some(delegating))
        .await
        .map_err(runtime_error)?;
    if let PlanBody::Rtfs(rtfs_body) = &plan.body {
        planner_audit.log_text("plan_body_rtfs", rtfs_body);
    }
    planner_audit.log_json(
        "plan_metadata",
        &json!({
            "plan_id": plan.plan_id,
            "capabilities_required": plan.capabilities_required,
            "input_schema": plan.input_schema.as_ref().map(|value| value_to_string_repr(value)),
            "output_schema": plan.output_schema.as_ref().map(|value| value_to_string_repr(value)),
        }),
    );
    render_plan_summary(&plan);

    if let Some(path) = args.export_plan_json.as_deref() {
        let steps_json = serialize_plan_steps_for_logging(&steps);
        match serde_json::to_string_pretty(&steps_json) {
            Ok(pretty) => match write_text_file(path, &pretty) {
                Ok(()) => {
                    println!("💾 Plan steps JSON saved to {}", path);
                    planner_audit.log_text(
                        "plan_export_json",
                        &format!("wrote plan steps JSON to {}", path),
                    );
                }
                Err(err) => eprintln!("⚠️  Failed to write plan steps JSON to {}: {}", path, err),
            },
            Err(err) => eprintln!("⚠️  Failed to serialize plan steps JSON: {}", err),
        }
    }

    if let Some(path) = args.export_plan_rtfs.as_deref() {
        match &plan.body {
            PlanBody::Rtfs(rtfs_code) => match write_text_file(path, rtfs_code) {
                Ok(()) => {
                    println!("💾 Plan RTFS saved to {}", path);
                    planner_audit
                        .log_text("plan_export_rtfs", &format!("wrote plan RTFS to {}", path));
                }
                Err(err) => eprintln!("⚠️  Failed to write plan RTFS to {}: {}", path, err),
            },
            _ => eprintln!(
                "⚠️  Plan body is not RTFS; skipping RTFS export to {}",
                path
            ),
        }
    }

    if args.execute_plan {
        println!("\n{}", "Plan Execution".bold().cyan());
        let mut context = RuntimeContext::full();
        let mut runtime_inputs = signals.constraints_map();
        for (key, value) in runtime_inputs.drain() {
            context.cross_plan_params.insert(key, value);
        }
        for (key, value) in &args.plan_inputs {
            context
                .cross_plan_params
                .insert(key.clone(), Value::String(value.clone()));
        }
        let causal_chain_arc = ccos.get_causal_chain();
        if args.auto_repair {
            println!("{}", "🔧 Running with auto-repair enabled...".yellow());
            let mut repair_options = PlanAutoRepairOptions::default();
            let context_lines = vec![format!("Goal: {}", goal)];
            repair_options.additional_context = Some(context_lines.join("\n"));
            repair_options.debug_responses = args.debug_prompts;

            match ccos
                .validate_and_execute_plan_with_auto_repair(plan.clone(), &context, repair_options)
                .await
            {
                Ok(result) => {
                    println!(
                        "{} {}",
                        "✅ Execution result:".green(),
                        value_to_string_repr(&result.value)
                    );
                    planner_audit
                        .log_json("plan_execution_result", &execution_result_to_json(&result));

                    // Export and summarize causal chain for audit/replay
                    // EXTRACT DATA UNDER LOCK THEN RELEASE TO AVOID DEADLOCK with planner_audit.log_json
                    let (plan_actions_len, integrity_result, trace) = if let Ok(chain_guard) = causal_chain_arc.lock() {
                        let plan_actions = chain_guard.export_plan_actions(&plan.plan_id);
                        let len = plan_actions.len();
                        let integrity = chain_guard.verify_and_summarize();
                        // Clone actions so we can release the lock
                        let trace: Vec<Action> = chain_guard
                            .get_plan_execution_trace(&plan.plan_id)
                            .into_iter()
                            .cloned()
                            .collect();
                        (len, integrity, trace)
                    } else {
                        eprintln!("⚠️ Failed to lock causal chain for export");
                        (
                            0,
                            Err(RuntimeError::Generic(
                                "Failed to lock causal chain".to_string(),
                            )),
                            Vec::new(),
                        )
                    };

                    if plan_actions_len > 0 {
                        println!(
                            "\n{} {} actions logged to causal chain",
                            "📋 Causal Chain:".bold().cyan(),
                            plan_actions_len
                        );

                        // Verify chain integrity
                        match integrity_result {
                            Ok((is_valid, total_actions, first_ts, last_ts)) => {
                                if is_valid {
                                    println!("  ✅ Chain integrity verified");
                                } else {
                                    eprintln!("  ⚠️ Chain integrity check failed");
                                }
                                println!("  📊 Total actions in chain: {}", total_actions);
                                if let (Some(first), Some(last)) = (first_ts, last_ts) {
                                    let duration_ms = last.saturating_sub(first);
                                    println!("  ⏱️ Duration: {}ms", duration_ms);
                                }

                                // Log plan trace for audit - NOW SAFE as lock is released
                                planner_audit.log_json(
                                    "plan_execution_trace",
                                    &json!({
                                        "plan_id": plan.plan_id,
                                        "action_count": trace.len(),
                                        "actions": trace.iter().map(|a| json!({
                                            "action_id": a.action_id,
                                            "type": format!("{:?}", a.action_type),
                                            "function_name": a.function_name,
                                            "timestamp": a.timestamp,
                                            "parent_action_id": a.parent_action_id,
                                            "success": a.result.as_ref().map(|r| r.success),
                                        })).collect::<Vec<_>>(),
                                    }),
                                );
                            }
                            Err(err) => {
                                eprintln!("  ⚠️ Failed to verify chain: {}", err);
                            }
                        }
                    }
                }
                Err(err) => {
                    eprintln!("❌ Plan execution failed: {}", err);
                    planner_audit.log_text("plan_execution_error", &err.to_string());

                    // Still export what we have in the causal chain
                    if let Ok(chain_guard) = causal_chain_arc.lock() {
                        let plan_actions = chain_guard.export_plan_actions(&plan.plan_id);
                        if !plan_actions.is_empty() {
                            println!(
                                "\n{} {} actions logged before failure",
                                "📋 Causal Chain:".bold().cyan(),
                                plan_actions.len()
                            );
                        }
                    } else {
                        eprintln!("⚠️ Failed to lock causal chain for export after failure");
                    }
                }
            }
        } else {
            match ccos.validate_and_execute_plan(plan.clone(), &context).await {
                Ok(result) => {
                    println!(
                        "{} {}",
                        "✅ Execution result:".green(),
                        value_to_string_repr(&result.value)
                    );
                    planner_audit
                        .log_json("plan_execution_result", &execution_result_to_json(&result));

                    // Export and summarize causal chain for audit/replay
                    // EXTRACT DATA UNDER LOCK THEN RELEASE TO AVOID DEADLOCK with planner_audit.log_json
                    let (plan_actions_len, integrity_result, trace) = if let Ok(chain_guard) = causal_chain_arc.lock() {
                        let plan_actions = chain_guard.export_plan_actions(&plan.plan_id);
                        let len = plan_actions.len();
                        let integrity = chain_guard.verify_and_summarize();
                        // Clone actions so we can release the lock
                        let trace: Vec<Action> = chain_guard
                            .get_plan_execution_trace(&plan.plan_id)
                            .into_iter()
                            .cloned()
                            .collect();
                        (len, integrity, trace)
                    } else {
                        eprintln!("⚠️ Failed to lock causal chain for export");
                        (
                            0,
                            Err(RuntimeError::Generic(
                                "Failed to lock causal chain".to_string(),
                            )),
                            Vec::new(),
                        )
                    };

                    if plan_actions_len > 0 {
                        println!(
                            "\n{} {} actions logged to causal chain",
                            "📋 Causal Chain:".bold().cyan(),
                            plan_actions_len
                        );

                        // Verify chain integrity
                        match integrity_result {
                            Ok((is_valid, total_actions, first_ts, last_ts)) => {
                                if is_valid {
                                    println!("  ✅ Chain integrity verified");
                                } else {
                                    eprintln!("  ⚠️ Chain integrity check failed");
                                }
                                println!("  📊 Total actions in chain: {}", total_actions);
                                if let (Some(first), Some(last)) = (first_ts, last_ts) {
                                    let duration_ms = last.saturating_sub(first);
                                    println!("  ⏱️ Duration: {}ms", duration_ms);
                                }

                                // Log plan trace for audit - NOW SAFE as lock is released
                                planner_audit.log_json(
                                    "plan_execution_trace",
                                    &json!({
                                        "plan_id": plan.plan_id,
                                        "action_count": trace.len(),
                                        "actions": trace.iter().map(|a| json!({
                                            "action_id": a.action_id,
                                            "type": format!("{:?}", a.action_type),
                                            "function_name": a.function_name,
                                            "timestamp": a.timestamp,
                                            "parent_action_id": a.parent_action_id,
                                            "success": a.result.as_ref().map(|r| r.success),
                                        })).collect::<Vec<_>>(),
                                    }),
                                );
                            }
                            Err(err) => {
                                eprintln!("  ⚠️ Failed to verify chain: {}", err);
                            }
                        }
                    }
                }
                Err(err) => {
                    eprintln!("❌ Plan execution failed: {}", err);
                    planner_audit.log_text("plan_execution_error", &err.to_string());

                    // Still export what we have in the causal chain
                    if let Ok(chain_guard) = causal_chain_arc.lock() {
                        let plan_actions = chain_guard.export_plan_actions(&plan.plan_id);
                        if !plan_actions.is_empty() {
                            println!(
                                "\n{} {} actions logged before failure",
                                "📋 Causal Chain:".bold().cyan(),
                                plan_actions.len()
                            );
                        }
                    } else {
                        eprintln!("⚠️ Failed to lock causal chain for export after failure");
                    }
                }
            }
        }
    }
    print_architecture_summary(&agent_config, args.profile.as_deref());

    Ok(())
}

fn plan_archive_dir() -> PathBuf {
    std::env::var("CCOS_PLAN_ARCHIVE_PATH")
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from("demo_storage/plans"))
}

fn ensure_directory(path: &PathBuf) -> Result<(), Box<dyn Error>> {
    if let Err(err) = std::fs::create_dir_all(path) {
        if !path.exists() {
            return Err(Box::new(err));
        }
    }
    Ok(())
}

fn render_intent_summary(intent: &Intent) {
    println!("\n{}", "Intent".bold().cyan());
    println!("  id: {}", intent.intent_id);
    println!("  goal: {}", intent.goal);
    if let Some(name) = &intent.name {
        println!("  name: {}", name);
    }
    if !intent.constraints.is_empty() {
        println!("  constraints: {} entries", intent.constraints.len());
    }
    if !intent.preferences.is_empty() {
        println!("  preferences: {} entries", intent.preferences.len());
    }
}

fn render_plan_summary(plan: &Plan) {
    println!("\n{}", "Plan".bold().cyan());
    println!("  id: {}", plan.plan_id);
    if let Some(name) = &plan.name {
        println!("  name: {}", name);
    }
    println!("  language: {:?}", plan.language);
    println!(
        "  capabilities required: {}",
        if plan.capabilities_required.is_empty() {
            "(none)".to_string()
        } else {
            plan.capabilities_required.join(", ")
        }
    );
    match &plan.body {
        PlanBody::Rtfs(rtfs) => {
            println!("\n{}", "Plan RTFS".bold());
            for line in rtfs.lines() {
                println!("    {}", line);
            }
        }
        other => {
            println!("  plan body: {:?}", other);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_keyword_identifier_detection() {
        assert!(looks_like_keyword_identifier("valid_output"));
        assert!(!looks_like_keyword_identifier("Invalid Output"));
        assert!(!looks_like_keyword_identifier("with.dot"));
        // Accept camelCase / mixed-case names like 'isError' produced by external MCPs
        assert!(looks_like_keyword_identifier("isError"));
    }

    #[test]
    fn test_serialize_plan_steps_for_logging_contains_bindings() {
        let step = PlanStep {
            id: "step_1".to_string(),
            name: "Fetch Issues".to_string(),
            capability_id: "example.list_items".to_string(),
            inputs: vec![(
                "owner".to_string(),
                StepInputBinding::Literal("mandubian".to_string()),
            )],
            outputs: vec!["issues".to_string()],
            notes: None,
        };

        let serialized = serialize_plan_steps_for_logging(&[step]);
        let serialized_str = serialized.to_string();
        assert!(
            serialized_str.contains("\"owner\""),
            "expected owner binding in serialized plan steps"
        );
        assert!(
            serialized_str.contains("\"literal\""),
            "expected literal binding marker in serialized plan steps"
        );
    }
}

#[allow(dead_code)]
fn render_capability_timelines(
    grouped_events: Vec<(String, Vec<ResolutionEvent>)>,
    filter: &DisplayFilter,
) {
    for (capability_id, events) in grouped_events {
        println!("\n{}", "═".repeat(80));
        println!(
            "{} {}",
            "Capability".bold().cyan(),
            capability_id.as_str().bold()
        );

        if events.is_empty() {
            println!("    - No events captured.");
            continue;
        }

        for event in events {
            let descriptor = stage_descriptor(event.stage);
            let indent = "    ".repeat(descriptor.depth);
            println!(
                "  {}- {}: {}",
                indent,
                descriptor.label.as_ref().bold(),
                event.summary
            );
            if filter.should_expand(event.stage) {
                if let Some(detail) = event.detail {
                    for line in detail.lines() {
                        let trimmed = line.trim_end();
                        if trimmed.is_empty() {
                            continue;
                        }
                        println!("  {}    {}", indent, trimmed);
                    }
                }
            }
        }
    }
}

fn stage_descriptor(stage: &str) -> StageDescriptor {
    match stage {
        "start" => StageDescriptor {
            label: Cow::Borrowed("Start"),
            depth: 0,
        },
        "alias_lookup" => StageDescriptor {
            label: Cow::Borrowed("Alias cache"),
            depth: 1,
        },
        "discovery" => StageDescriptor {
            label: Cow::Borrowed("Discovery"),
            depth: 1,
        },
        "marketplace" | "marketplace_search" => StageDescriptor {
            label: Cow::Borrowed("Marketplace"),
            depth: 2,
        },
        "local_scan" => StageDescriptor {
            label: Cow::Borrowed("Local manifests"),
            depth: 2,
        },
        "mcp_registry" | "mcp_search" => StageDescriptor {
            label: Cow::Borrowed("MCP registry"),
            depth: 2,
        },
        "mcp_introspection" => StageDescriptor {
            label: Cow::Borrowed("MCP introspection"),
            depth: 3,
        },
        "heuristic_match" => StageDescriptor {
            label: Cow::Borrowed("Heuristic match"),
            depth: 2,
        },
        "tool_selector" => StageDescriptor {
            label: Cow::Borrowed("Tool selector"),
            depth: 3,
        },
        "llm_selection" => StageDescriptor {
            label: Cow::Borrowed("LLM selection"),
            depth: 3,
        },
        "llm_synthesis" => StageDescriptor {
            label: Cow::Borrowed("LLM synthesis"),
            depth: 3,
        },
        "result" => StageDescriptor {
            label: Cow::Borrowed("Result"),
            depth: 1,
        },
        other => StageDescriptor {
            label: Cow::Owned(other.replace('_', " ")),
            depth: 1,
        },
    }
}

async fn configure_session_pool(ccos: &Arc<CCOS>) -> Result<(), Box<dyn Error>> {
    let mut session_pool = SessionPoolManager::new();
    session_pool.register_handler("mcp", Arc::new(MCPSessionHandler::new()));
    let session_pool = Arc::new(session_pool);

    let marketplace = ccos.get_capability_marketplace();
    marketplace.set_session_pool(session_pool).await;

    Ok(())
}

fn runtime_error(err: RuntimeError) -> Box<dyn Error> {
    Box::new(err)
}

fn apply_llm_profile(
    config: &AgentConfig,
    profile_name: Option<&str>,
) -> Result<(), Box<dyn Error>> {
    std::env::set_var("CCOS_ENABLE_DELEGATION", "1");

    if let Some(llm_profiles) = &config.llm_profiles {
        let (profiles, _meta, _why) = expand_profiles(config);
        let chosen = profile_name
            .map(|s| s.to_string())
            .or_else(|| llm_profiles.default.clone())
            .or_else(|| profiles.first().map(|p| p.name.clone()));

        if let Some(name) = chosen {
            if let Some(profile) = profiles.iter().find(|p| p.name == name) {
                apply_profile_env(profile);
            }
        } else if let Some(first) = profiles.first() {
            apply_profile_env(first);
        }
    }

    Ok(())
}

fn apply_profile_env(profile: &LlmProfile) {
    std::env::set_var("CCOS_DELEGATING_MODEL", &profile.model);
    std::env::set_var("CCOS_LLM_MODEL", &profile.model);
    std::env::set_var("CCOS_LLM_PROVIDER_HINT", &profile.provider);

    if let Some(url) = &profile.base_url {
        std::env::set_var("CCOS_LLM_BASE_URL", url);
    } else if profile.provider == "openrouter" {
        if std::env::var("CCOS_LLM_BASE_URL").is_err() {
            std::env::set_var("CCOS_LLM_BASE_URL", "https://openrouter.ai/api/v1");
        }
    }

    if let Some(api_key) = profile.api_key.as_ref() {
        set_api_key(&profile.provider, api_key);
    } else if let Some(env) = &profile.api_key_env {
        if let Ok(value) = std::env::var(env) {
            set_api_key(&profile.provider, &value);
        }
    }

    match profile.provider.as_str() {
        "openai" => std::env::set_var("CCOS_LLM_PROVIDER", "openai"),
        "claude" | "anthropic" => std::env::set_var("CCOS_LLM_PROVIDER", "anthropic"),
        "openrouter" => {
            std::env::set_var("CCOS_LLM_PROVIDER", "openrouter");
            if std::env::var("CCOS_LLM_BASE_URL").is_err() {
                std::env::set_var("CCOS_LLM_BASE_URL", "https://openrouter.ai/api/v1");
            }
        }
        "local" => std::env::set_var("CCOS_LLM_PROVIDER", "local"),
        "stub" => {
            eprintln!("⚠️  WARNING: Using stub LLM provider (testing only - not realistic)");
            eprintln!(
                "   Set a real provider in agent_config.toml or use --profile with a real provider"
            );
            std::env::set_var("CCOS_LLM_PROVIDER", "stub");
            std::env::set_var("CCOS_ALLOW_STUB_PROVIDER", "1");
        }
        other => std::env::set_var("CCOS_LLM_PROVIDER", other),
    }
}

fn set_api_key(provider: &str, key: &str) {
    match provider {
        "openrouter" => std::env::set_var("OPENROUTER_API_KEY", key),
        "claude" | "anthropic" => std::env::set_var("ANTHROPIC_API_KEY", key),
        "gemini" => std::env::set_var("GEMINI_API_KEY", key),
        "stub" => {}
        _ => std::env::set_var("OPENAI_API_KEY", key),
    }
}
