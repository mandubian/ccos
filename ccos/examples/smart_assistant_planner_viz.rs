use std::borrow::Cow;
use std::cmp::{min, Ordering};
use std::collections::{HashMap, HashSet};
use std::error::Error;
use std::fmt::Write as _;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use clap::{ArgAction, Parser};
use crossterm::style::Stylize;
use rtfs::ast::{Keyword, MapKey, TypeExpr};
use rtfs::config::profile_selection::expand_profiles;
use rtfs::config::types::{AgentConfig, LlmProfile};
use rtfs::runtime::error::{RuntimeError, RuntimeResult};
use rtfs::runtime::values::Value;

use ccos::arbiter::arbiter_config::LlmProviderType;
use ccos::arbiter::ArbiterEngine;
use ccos::capabilities::{MCPSessionHandler, SessionPoolManager};
use ccos::capability_marketplace::types::{CapabilityManifest, ProviderType};
use ccos::capability_marketplace::CapabilityMarketplace;
use ccos::catalog::{CatalogEntryKind, CatalogFilter, CatalogService};
use ccos::examples_common::capability_helpers::{
    count_token_matches, load_override_parameters, minimum_token_matches,
    preload_discovered_capabilities, score_manifest_against_tokens, tokenize_identifier,
};
use ccos::intent_graph::config::IntentGraphConfig;
use ccos::synthesis::missing_capability_resolver::{ResolutionEvent, ResolutionObserver};
use ccos::types::Plan;
use ccos::types::{Intent, PlanBody};
use ccos::CCOS;
use serde::Deserialize;

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
struct CapabilityMenuEntry {
    id: String,
    provider: String,
    description: String,
    required_inputs: Vec<String>,
    optional_inputs: Vec<String>,
    outputs: Vec<String>,
    score: f64,
}

#[derive(Debug, Clone)]
struct PlanStep {
    id: String,
    name: String,
    capability_id: String,
    inputs: Vec<(String, StepInputBinding)>,
    outputs: Vec<String>,
    notes: Option<String>,
}

#[derive(Debug, Clone)]
enum StepInputBinding {
    Literal(String),
    Variable(String),
    StepOutput { step_id: String, output: String },
}

#[derive(Debug, Deserialize)]
struct PlanStepJson {
    id: String,
    name: String,
    #[serde(rename = "capability_id")]
    capability_id: String,
    inputs: HashMap<String, String>,
    outputs: Vec<String>,
    #[serde(default)]
    notes: Option<String>,
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
        if hit.entry.id.starts_with("ccos.") {
            continue;
        }
        if let Some(manifest) = marketplace.get_capability(&hit.entry.id).await {
            if let Some(entry) = describe_manifest(&manifest, hit.score as f64) {
                menu.push(entry);
            }
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
                if manifest.id.starts_with("ccos.") {
                    continue;
                }
                if seen.contains(&manifest.id) {
                    continue;
                }
                let score = score_manifest_against_tokens(&manifest, &tokens);
                if let Some(entry) = describe_manifest(&manifest, score as f64) {
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
            }
            matched_entries
                .sort_by(|a, b| b.score.partial_cmp(&a.score).unwrap_or(Ordering::Equal));
            backup_entries.sort_by(|a, b| b.score.partial_cmp(&a.score).unwrap_or(Ordering::Equal));

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
            menu.sort_by(|a, b| b.score.partial_cmp(&a.score).unwrap_or(Ordering::Equal));
            if menu.len() > limit {
                menu.truncate(limit);
            }
        }
    }

    if menu.is_empty() {
        if let Some(manifest) = marketplace
            .get_capability("mcp.github.github-mcp.list_issues")
            .await
        {
            if let Some(entry) = describe_manifest(&manifest, 1.0) {
                menu.push(entry);
            }
        }
    }

    if menu.is_empty() {
        Err(RuntimeError::Generic(
            "Catalog query returned no capabilities".to_string(),
        ))
    } else {
        Ok(menu)
    }
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

fn describe_manifest(manifest: &CapabilityManifest, score: f64) -> Option<CapabilityMenuEntry> {
    let id = manifest.id.trim();
    if id.is_empty() || !id.contains('.') {
        return None;
    }

    let (required_inputs, optional_inputs) = extract_input_fields(manifest);
    let outputs = extract_output_fields(manifest);
    if required_inputs.is_empty() && optional_inputs.is_empty() {
        if let Some(schema) = &manifest.input_schema {
            eprintln!(
                "⚠️  Input schema present but no fields extracted for {}: {:?}",
                manifest.id, schema
            );
        } else {
            eprintln!("⚠️  No input schema available for {}", manifest.id);
        }
    }

    Some(CapabilityMenuEntry {
        id: id.to_string(),
        provider: manifest
            .metadata
            .get("capability_source")
            .cloned()
            .unwrap_or_else(|| provider_to_label(&manifest.provider)),
        description: manifest.description.clone(),
        required_inputs,
        optional_inputs,
        outputs,
        score,
    })
}

fn provider_to_label(provider: &ProviderType) -> String {
    match provider {
        ProviderType::Local(_) => "local".to_string(),
        ProviderType::Http(http) => format!("http:{}", http.base_url),
        ProviderType::MCP(mcp) => format!("mcp:{}", mcp.server_url),
        ProviderType::A2A(a2a) => format!("a2a:{}", a2a.agent_id),
        ProviderType::OpenApi(api) => format!("openapi:{}", api.base_url),
        ProviderType::Plugin(plugin) => format!("plugin:{}", plugin.function_name),
        ProviderType::RemoteRTFS(remote) => format!("remote_rtfs:{}", remote.endpoint),
        ProviderType::Stream(stream) => format!("stream:{:?}", stream.stream_type),
        ProviderType::Registry(registry) => format!("registry:{}", registry.capability_id),
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
            entry.score
        );
        if !entry.provider.is_empty() {
            let _ = writeln!(buffer, "   provider: {}", entry.provider);
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
        if !entry.outputs.is_empty() {
            let _ = writeln!(buffer, "   outputs: {}", entry.outputs.join(", "));
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

fn extract_input_fields(manifest: &CapabilityManifest) -> (Vec<String>, Vec<String>) {
    let mut required = Vec::new();
    let mut optional = Vec::new();
    if let Some(schema) = &manifest.input_schema {
        collect_input_keys(schema, &mut required, &mut optional);
    }
    if required.is_empty() && optional.is_empty() {
        if let Some((fallback_required, fallback_optional)) =
            load_override_parameters(manifest.id.as_str())
        {
            required = fallback_required;
            optional = fallback_optional;
        }
    }
    (required, optional)
}

fn collect_input_keys(schema: &TypeExpr, required: &mut Vec<String>, optional: &mut Vec<String>) {
    match schema {
        TypeExpr::Map { entries, .. } => {
            for entry in entries {
                if entry.optional {
                    optional.push(entry.key.0.clone());
                } else {
                    required.push(entry.key.0.clone());
                }
            }
        }
        TypeExpr::Optional(inner) => collect_input_keys(inner, required, optional),
        TypeExpr::Union(options) => {
            for option in options {
                collect_input_keys(option, required, optional);
            }
        }
        _ => {}
    }
}

fn extract_output_fields(manifest: &CapabilityManifest) -> Vec<String> {
    manifest
        .output_schema
        .as_ref()
        .map(collect_output_keys)
        .unwrap_or_default()
}

fn collect_output_keys(schema: &TypeExpr) -> Vec<String> {
    match schema {
        TypeExpr::Map { entries, .. } => entries.iter().map(|entry| entry.key.0.clone()).collect(),
        TypeExpr::Vector(inner) | TypeExpr::Optional(inner) => collect_output_keys(inner),
        TypeExpr::Union(options) => options.iter().flat_map(collect_output_keys).collect(),
        _ => Vec::new(),
    }
}

async fn preload_discovered_capabilities_if_needed(
    marketplace: &CapabilityMarketplace,
) -> RuntimeResult<()> {
    let root = Path::new("capabilities/discovered");
    if !root.exists() {
        return Ok(());
    }

    let loaded = preload_discovered_capabilities(marketplace, root).await?;
    if loaded > 0 {
        println!(
            "{}",
            format!("ℹ️  Loaded {} discovered capability manifest(s)", loaded).blue()
        );
    }

    Ok(())
}

async fn propose_plan_steps_with_menu(
    delegating: &ccos::arbiter::delegating_arbiter::DelegatingArbiter,
    goal: &str,
    intent: &Intent,
    known_inputs: &HashMap<String, Value>,
    menu: &[CapabilityMenuEntry],
    debug_prompts: bool,
    feedback: Option<&str>,
) -> RuntimeResult<Vec<PlanStep>> {
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
    let additional_context = if context_lines.is_empty() {
        String::new()
    } else {
        format!("\nAdditional context:\n{}\n", context_lines.join("\n"))
    };
    let feedback_block = feedback
        .map(|text| format!("\nPrevious attempt feedback:\n{}\n", text))
        .unwrap_or_default();

    let prompt = format!(
        r#"You are designing a plan to achieve the following goal.

Goal: {goal}

Known parameters (use via bindings var::<name>): {inputs}

Capability menu (choose from these only):
{menu}
{additional}{feedback}
Output requirements:
- Respond with a JSON array (no markdown fences) where each element is an object with keys: id, name, capability_id, inputs, outputs, notes.
- inputs must be a JSON object mapping capability parameter names to bindings using one of:
    * "var::<name>" to reference a plan-level variable (from the known parameters list).
    * "literal::<value>" to pass a string literal.
    * "step::<step_id>::<output>" to reference an output from a previous step.
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
    );

    if debug_prompts {
        println!("\n{}", "=== Plan Synthesis Prompt ===".bold());
        println!("{}", prompt);
    }

    let response = delegating.generate_raw_text(&prompt).await?;

    if debug_prompts {
        println!("\n{}", "=== Plan Synthesis Response ===".bold());
        println!("{}", response);
    }

    parse_plan_steps_from_json(&response, menu)
}

fn parse_plan_steps_from_json(
    response: &str,
    menu: &[CapabilityMenuEntry],
) -> RuntimeResult<Vec<PlanStep>> {
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

    let valid_ids: HashSet<&str> = menu.iter().map(|entry| entry.id.as_str()).collect();
    let mut steps = Vec::new();
    for raw in raw_steps {
        if !valid_ids.contains(raw.capability_id.as_str()) {
            return Err(RuntimeError::Generic(format!(
                "Step '{}' referenced unknown capability_id '{}'",
                raw.id, raw.capability_id
            )));
        }
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
    for (name, binding) in raw.inputs {
        if name.trim().is_empty() {
            continue;
        }
        let parsed = interpret_binding(&binding).ok_or_else(|| {
            RuntimeError::Generic(format!(
                "Step '{}' has invalid binding '{}' for input '{}'",
                raw.id, binding, name
            ))
        })?;
        inputs.push((name, parsed));
    }

    Ok(PlanStep {
        id: raw.id,
        name: if raw.name.trim().is_empty() {
            "Unnamed Step".to_string()
        } else {
            raw.name
        },
        capability_id: raw.capability_id,
        inputs,
        outputs: raw.outputs,
        notes: raw.notes,
    })
}

fn validate_plan_steps_against_menu(
    steps: &[PlanStep],
    menu: &[CapabilityMenuEntry],
) -> Result<(), Vec<String>> {
    let mut errors = Vec::new();
    let menu_map = menu
        .iter()
        .map(|entry| (entry.id.as_str(), entry))
        .collect::<HashMap<_, _>>();

    for step in steps {
        let Some(entry) = menu_map.get(step.capability_id.as_str()) else {
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

        let mut allowed_inputs = HashSet::new();
        for key in &entry.required_inputs {
            allowed_inputs.insert(key.as_str());
        }
        for key in &entry.optional_inputs {
            allowed_inputs.insert(key.as_str());
        }

        for required in &entry.required_inputs {
            if !input_keys.contains(required.as_str()) {
                errors.push(format!(
                    "Step '{}' using '{}' is missing required input '{}' (required: {}; optional: {})",
                    step.id,
                    step.capability_id,
                    required,
                    if entry.required_inputs.is_empty() {
                        "(none)".to_string()
                    } else {
                        entry.required_inputs.join(", ")
                    },
                    if entry.optional_inputs.is_empty() {
                        "(none)".to_string()
                    } else {
                        entry.optional_inputs.join(", ")
                    }
                ));
            }
        }

        for provided in &input_keys {
            if !allowed_inputs.contains(provided) {
                errors.push(format!(
                    "Step '{}' using '{}' provided unsupported input '{}' (required: {}; optional: {})",
                    step.id,
                    step.capability_id,
                    provided,
                    if entry.required_inputs.is_empty() {
                        "(none)".to_string()
                    } else {
                        entry.required_inputs.join(", ")
                    },
                    if entry.optional_inputs.is_empty() {
                        "(none)".to_string()
                    } else {
                        entry.optional_inputs.join(", ")
                    }
                ));
            }
        }
    }

    if errors.is_empty() {
        Ok(())
    } else {
        Err(errors)
    }
}

fn interpret_binding(raw: &str) -> Option<StepInputBinding> {
    let trimmed = raw.trim();
    if let Some(rest) = trimmed.strip_prefix("var::") {
        let name = rest.trim();
        if name.is_empty() {
            return None;
        }
        return Some(StepInputBinding::Variable(name.to_string()));
    }
    if let Some(rest) = trimmed.strip_prefix("literal::") {
        return Some(StepInputBinding::Literal(rest.to_string()));
    }
    if let Some(rest) = trimmed.strip_prefix("step::") {
        if let Some((step_id, output)) = rest.split_once("::") {
            let step_id = step_id.trim();
            let output = output.trim();
            if !step_id.is_empty() && !output.is_empty() {
                return Some(StepInputBinding::StepOutput {
                    step_id: step_id.to_string(),
                    output: output.to_string(),
                });
            }
        }
    }
    if trimmed.is_empty() {
        None
    } else {
        Some(StepInputBinding::Literal(trimmed.to_string()))
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
            println!("     outputs: {}", step.outputs.join(", "));
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
    }
}

fn assemble_plan_from_steps(steps: &[PlanStep], intent: &Intent) -> RuntimeResult<Plan> {
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
            if !output.trim().is_empty() {
                output_map.insert(output.clone(), idx);
            }
        }
    }

    let body = render_plan_body(steps, &step_index)?;

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
    plan.metadata.insert(
        "planning.pipeline".to_string(),
        Value::String("planner_viz_v2".to_string()),
    );

    Ok(plan)
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
        body.push_str(&format!(
            "      step_{} (call :{} {})\n",
            idx,
            sanitize_capability_id(&step.capability_id),
            args
        ));
    }

    body.push_str("    ]\n");
    body.push_str("      {\n");

    let mut final_outputs = Vec::new();
    for (idx, step) in steps.iter().enumerate() {
        for output in &step.outputs {
            if !output.trim().is_empty() {
                final_outputs.push((output.clone(), idx));
            }
        }
    }

    if final_outputs.is_empty() {
        body.push_str("        :result step_");
        body.push_str("0\n");
    } else {
        final_outputs.sort_by(|a, b| a.0.cmp(&b.0));
        for (idx, (name, step_idx)) in final_outputs.iter().enumerate() {
            body.push_str(&format!(
                "        :{} (get step_{} :{})",
                sanitize_keyword_name(name),
                step_idx,
                sanitize_keyword_name(name)
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

    let marketplace = ccos.get_capability_marketplace();
    preload_discovered_capabilities_if_needed(marketplace.as_ref())
        .await
        .map_err(runtime_error)?;

    let catalog = ccos.get_catalog();
    catalog.ingest_marketplace(marketplace.as_ref()).await;

    let menu = build_capability_menu_from_catalog(catalog, marketplace, &goal, &intent, 12)
        .await
        .map_err(runtime_error)?;

    println!("\n{}", "Capability Menu".bold().cyan());
    for entry in &menu {
        println!(
            "  - {} (score: {:.1}) / required: {} / outputs: {}",
            entry.id,
            entry.score,
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

    const MAX_PLAN_ATTEMPTS: usize = 3;
    let mut attempt = 0usize;
    let mut feedback: Option<String> = None;
    let steps = loop {
        attempt += 1;
        let steps = match propose_plan_steps_with_menu(
            delegating,
            &goal,
            &intent,
            &intent.constraints,
            &menu,
            args.debug_prompts,
            feedback.as_deref(),
        )
        .await
        {
            Ok(steps) => steps,
            Err(err) => {
                let err_msg = err.to_string();
                println!(
                    "\n{}",
                    "Plan synthesis failed to produce valid JSON steps:"
                        .red()
                        .bold()
                );
                println!("  - {}", err_msg.as_str().red());

                if attempt >= MAX_PLAN_ATTEMPTS {
                    return Err(runtime_error(err));
                }

                feedback = Some(format!(
                    "Previous attempt failed to produce valid plan steps JSON ({}). Ensure each step object includes the keys id, name, capability_id, inputs, outputs, and optional notes.",
                    err_msg
                ));
                println!(
                    "{}",
                    "Retrying plan synthesis with corrective feedback...".yellow()
                );
                continue;
            }
        };

        match validate_plan_steps_against_menu(&steps, &menu) {
            Ok(()) => break steps,
            Err(messages) => {
                println!(
                    "\n{}",
                    "Schema validation failed for the proposed steps:"
                        .red()
                        .bold()
                );
                for message in &messages {
                    println!("  - {}", message.as_str().red());
                }

                if attempt >= MAX_PLAN_ATTEMPTS {
                    let summary = messages.join("; ");
                    return Err(runtime_error(RuntimeError::Generic(format!(
                        "Planner could not produce schema-compliant steps after {} attempt(s): {}",
                        attempt, summary
                    ))));
                }

                let summary = messages
                    .iter()
                    .map(|msg| format!("- {}", msg))
                    .collect::<Vec<_>>()
                    .join("\n");
                feedback = Some(format!(
                    "Schema validation errors:\n{}\nEnsure you only use the required/optional inputs listed for each capability and provide every required field.",
                    summary
                ));
                println!(
                    "{}",
                    "Re-submitting plan synthesis request with schema feedback...".yellow()
                );
            }
        }
    };

    render_intent_summary(&intent);
    render_plan_steps(&steps);

    let plan = assemble_plan_from_steps(&steps, &intent).map_err(runtime_error)?;
    render_plan_summary(&plan);

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

fn load_agent_config(path: &str) -> Result<AgentConfig, Box<dyn Error>> {
    let data = std::fs::read_to_string(path)?;
    let config = if path.ends_with(".json") {
        serde_json::from_str(&data)?
    } else {
        toml::from_str(&data)?
    };
    Ok(config)
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

fn print_architecture_summary(config: &AgentConfig, profile_name: Option<&str>) {
    println!("\n{}", "═".repeat(80).bold());
    println!(
        "{}",
        "🏗️  CCOS Smart Assistant - Architecture Summary"
            .bold()
            .cyan()
    );
    println!("{}", "═".repeat(80).bold());

    println!("\n{}", "📋 Architecture Overview".bold());
    println!("  ┌─────────────────────────────────────────────────────────────┐");
    println!("  │ User Goal → Intent Extraction → Plan Generation → Execution │");
    println!("  └─────────────────────────────────────────────────────────────┘");
    println!("\n  {} Flow:", "1.".bold());
    println!("     • Natural language goal → Intent (constraints, preferences)");
    println!("     • Intent → Plan generation (delegating arbiter)");
    println!("     • Plan → Capability discovery (aliases → marketplace → MCP)");
    println!("     • Resolver timelines show how missing tools are synthesized");
    println!("     • Final plan executes via orchestrator");

    println!("\n  {} Key Components:", "2.".bold());
    println!(
        "     • {}: Governs intent extraction and plan synthesis",
        "DelegatingArbiter".cyan()
    );
    println!(
        "     • {}: Runs marketplace/MCP discovery pipeline",
        "MissingCapabilityResolver".cyan()
    );
    println!(
        "     • {}: Stores and ranks capabilities",
        "CapabilityMarketplace".cyan()
    );
    println!(
        "     • {}: Tracks intent relationships and checkpoints",
        "IntentGraph".cyan()
    );

    let discovery = &config.discovery;
    println!("\n  {} Discovery/Search Settings:", "3.".bold());
    if discovery.use_embeddings {
        let model = discovery
            .embedding_model
            .as_deref()
            .or(discovery.local_embedding_model.as_deref())
            .unwrap_or("unspecified model");
        println!(
            "     • Embedding search: {} ({})",
            "enabled".green(),
            model.cyan()
        );
    } else {
        println!(
            "     • Embedding search: {} (keyword + schema heuristics)",
            "disabled".yellow()
        );
    }
    println!("     • Match threshold: {:.2}", discovery.match_threshold);
    println!(
        "     • Action verb weight / threshold: {:.2} / {:.2}",
        discovery.action_verb_weight, discovery.action_verb_threshold
    );
    println!(
        "     • Capability class weight: {:.2}",
        discovery.capability_class_weight
    );

    if let Some(llm_profiles) = &config.llm_profiles {
        let (profiles, _meta, _why) = expand_profiles(config);
        let chosen = profile_name
            .map(|s| s.to_string())
            .or_else(|| llm_profiles.default.clone())
            .or_else(|| profiles.first().map(|p| p.name.clone()));

        println!("\n  {} LLM Profile:", "4.".bold());
        if let Some(name) = chosen {
            if let Some(profile) = profiles.iter().find(|p| p.name == name) {
                println!("     • Active profile: {}", name.cyan());
                println!("     • Provider: {}", profile.provider.as_str().cyan());
                println!("     • Model: {}", profile.model.as_str().cyan());
                if let Some(base) = &profile.base_url {
                    println!("     • Base URL: {}", base);
                }
            } else {
                println!("     • Active profile name: {} (details unavailable)", name);
            }
        } else {
            println!("     • No LLM profile configured");
        }
    }

    println!("\n{}", "═".repeat(80).bold());
}
