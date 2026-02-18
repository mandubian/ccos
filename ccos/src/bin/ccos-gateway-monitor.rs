//! CCOS Gateway Monitor
//!
//! Real-time monitoring TUI for the CCOS Gateway.
//! Shows connected sessions, spawned agents, and system events.

use clap::Parser;
use crossterm::{
    event::{self, DisableMouseCapture, EnableMouseCapture, Event, KeyCode, MouseButton, MouseEventKind},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use futures::StreamExt;
use ratatui::{
    backend::CrosstermBackend,
    layout::{Alignment, Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Cell, List, ListItem, ListState, Paragraph, Row, Table, Wrap},
    Frame, Terminal,
};
use reqwest::Client;
use serde::Deserialize;
use std::collections::{HashMap, HashSet};
use std::io;
use std::time::{Duration, Instant};
use tokio::sync::mpsc;
use tokio_tungstenite::{connect_async, tungstenite::protocol::Message};
use tracing::{error, info, warn};

#[derive(Parser, Debug)]
#[command(name = "ccos-gateway-monitor")]
struct Args {
    /// Gateway URL (WebSocket will use ws:// or wss://)
    #[arg(long, default_value = "http://127.0.0.1:8822")]
    gateway_url: String,

    /// Admin token for authentication
    #[arg(long, default_value = "admin-token")]
    token: String,

    /// Refresh interval for metrics (seconds)
    #[arg(long, default_value = "5")]
    refresh_interval: u64,

    /// Path to agent configuration file used for available LLM profile choices
    #[arg(long, default_value = "config/agent_config.toml")]
    config_path: String,
}

/// Session information from the gateway
#[derive(Debug, Clone, Deserialize)]
struct SessionInfo {
    session_id: String,
    status: String,
    agent_pid: Option<u32>,
    current_step: Option<u32>,
    memory_mb: Option<u64>,
    #[allow(dead_code)]
    created_at: String,
    #[allow(dead_code)]
    last_activity: String,
}

/// Agent information
#[derive(Debug, Clone)]
struct AgentInfo {
    pid: u32,
    #[allow(dead_code)]
    session_id: String,
    current_step: u32,
    memory_mb: Option<u64>,
    is_healthy: bool,
    #[allow(dead_code)]
    last_heartbeat: Instant,
}

impl Default for AgentInfo {
    fn default() -> Self {
        Self {
            pid: 0,
            session_id: String::new(),
            current_step: 0,
            memory_mb: None,
            is_healthy: false,
            last_heartbeat: Instant::now(),
        }
    }
}

/// Code execution details for detailed display
#[derive(Debug, Clone)]
struct CodeExecutionDetails {
    language: String,
    code: String,
    stdout: String,
    stderr: String,
    exit_code: Option<i32>,
    duration_ms: Option<u64>,
}

/// System event
#[derive(Debug, Clone)]
struct SystemEvent {
    id: u64,
    timestamp: Instant,
    event_type: String,
    session_id: String,
    details: String,
    /// Full details for detailed view (optional)
    full_details: Option<String>,
    /// Code execution specific details
    code_execution: Option<CodeExecutionDetails>,
    /// Raw metadata for flexible display
    metadata: Option<serde_json::Value>,
}

/// LLM Consultation event details
#[derive(Debug, Clone)]
struct TokenUsageDetails {
    prompt_tokens: u64,
    completion_tokens: u64,
    total_tokens: u64,
}

/// LLM Consultation event details
#[derive(Debug, Clone)]
struct LlmConsultation {
    iteration: u32,
    is_initial: bool,
    understanding: String,
    reasoning: String,
    task_complete: bool,
    planned_capabilities: Vec<String>,
    model: Option<String>,
    prompt: Option<String>,
    response: Option<String>,
    token_usage: Option<TokenUsageDetails>,
}

/// Detailed action event data for rich display
#[derive(Debug, Clone)]
struct ActionEventDetails {
    action_type: String,
    function_name: String,
    success: Option<bool>,
    duration_ms: Option<u64>,
    summary: String,
    /// Code execution specific details
    code_execution: Option<CodeExecutionDetails>,
    /// Raw metadata for additional display
    metadata: Option<serde_json::Value>,
}

#[derive(Debug, Clone)]
struct RunCreateProgramDetails {
    goal: String,
    target_session_id: Option<String>,
    schedule: Option<String>,
    next_run_at: Option<String>,
    max_run: Option<String>,
    budget: Option<String>,
    execution_mode: Option<String>,
    trigger_capability_id: Option<String>,
    trigger_inputs: Option<String>,
    parent_run_id: Option<String>,
}

#[derive(Debug, Clone)]
struct MemoryOperationDetails {
    operation: String,
    key: Option<String>,
    store_value: Option<String>,
    store_entry_id: Option<String>,
    store_success: Option<bool>,
    get_found: Option<bool>,
    get_expired: Option<bool>,
    get_value: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
struct GatewayRunSummary {
    run_id: String,
    goal: String,
    state: String,
    steps_taken: u32,
    elapsed_secs: u64,
    created_at: String,
    #[allow(dead_code)]
    updated_at: String,
    #[allow(dead_code)]
    current_step_id: Option<String>,
    next_run_at: Option<String>,
    /// Cron/interval schedule expression
    #[serde(default)]
    schedule: Option<String>,
    /// Stable ID shared by all recurrences of the same scheduled task
    #[serde(default)]
    schedule_group_id: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
struct GatewayListRunsResponse {
    session_id: String,
    runs: Vec<GatewayRunSummary>,
}

#[derive(Debug, Deserialize)]
struct GatewayCancelRunResponse {
    run_id: String,
    cancelled: bool,
    previous_state: String,
}

/// A row in the Runs tab tree view.
#[derive(Debug, Clone)]
enum RunDisplayItem {
    /// Header row for a group of runs sharing the same recurring schedule.
    ScheduleGroup {
        #[allow(dead_code)]
        group_id: String,
        goal: String,
        schedule: String,
        instance_count: usize,
        next_run_at: Option<String>,
        /// Run ID of the currently-`Scheduled` run in this group.
        /// Cancelling the group cancels this run, stopping future firings.
        pending_run_id: Option<String>,
    },
    /// An individual run instance (child under a group or standalone).
    RunInstance {
        run: GatewayRunSummary,
        grouped: bool,
        is_last_in_group: bool,
    },
}

impl RunDisplayItem {
    /// Run ID to cancel when the user presses `c` on this item.
    fn cancel_run_id(&self) -> Option<String> {
        match self {
            RunDisplayItem::ScheduleGroup { pending_run_id, .. } => pending_run_id.clone(),
            RunDisplayItem::RunInstance { run, .. } => Some(run.run_id.clone()),
        }
    }
}

#[derive(Debug, Clone)]
enum MonitorEvent {
    Input(Event),
    SessionUpdate(Vec<SessionInfo>),
    AgentHeartbeat(String, AgentInfo),
    AgentCrashed(String, u32),
    ActionEvent(String, ActionEventDetails), // session_id, action_details
    LlmConsultation(String, LlmConsultation), // session_id, consultation_details
    Tick,
}

struct MonitorState {
    sessions: HashMap<String, SessionInfo>,
    agents: HashMap<String, AgentInfo>,
    /// Ordered list of session IDs for selection
    agent_session_ids: Vec<String>,
    events: Vec<SystemEvent>,
    llm_consultations: Vec<(String, LlmConsultation)>, // (session_id, consultation)
    selected_tab: usize,
    selected_agent_index: usize,
    /// All known session IDs (sorted) — used for Runs tab session navigation independent of active agents
    ordered_session_ids: Vec<String>,
    /// Selected index into ordered_session_ids for the Runs tab session picker
    selected_runs_session_idx: usize,
    /// Runs loaded for currently selected session
    session_runs: Vec<GatewayRunSummary>,
    /// Flat tree-view items built from session_runs (groups + instances)
    display_items: Vec<RunDisplayItem>,
    selected_run_index: usize,
    runs_session_id: Option<String>,
    /// Selected event index in Events tab
    selected_event_index: usize,
    /// Stable selected event ID (prevents selection drift when new events arrive)
    selected_event_id: Option<u64>,
    /// Monotonic event ID counter
    next_event_id: u64,
    /// Whether event detail popup is shown
    show_event_detail: bool,
    /// Scroll offset for event detail popup
    detail_scroll: u16,
    /// Show noisy internal-step events in Events tab
    show_internal_steps: bool,
    #[allow(dead_code)]
    last_refresh: Instant,
    show_profile_selector: bool,
    selected_profile_index: usize,
    available_llm_profiles: Vec<String>,
    active_spawn_llm_profile: Option<String>,
    should_quit: bool,
    status_message: String,
}

impl MonitorState {
    fn new(available_llm_profiles: Vec<String>, active_spawn_llm_profile: Option<String>) -> Self {
        Self {
            sessions: HashMap::new(),
            agents: HashMap::new(),
            agent_session_ids: Vec::new(),
            ordered_session_ids: Vec::new(),
            selected_runs_session_idx: 0,
            events: Vec::new(),
            llm_consultations: Vec::new(),
            selected_tab: 0,
            selected_agent_index: 0,
            session_runs: Vec::new(),
            display_items: Vec::new(),
            selected_run_index: 0,
            runs_session_id: None,
            selected_event_index: 0,
            selected_event_id: None,
            next_event_id: 1,
            show_event_detail: false,
            detail_scroll: 0,
            show_internal_steps: false,
            last_refresh: Instant::now(),
            show_profile_selector: false,
            selected_profile_index: 0,
            available_llm_profiles,
            active_spawn_llm_profile,
            should_quit: false,
            status_message: "Connected to gateway".to_string(),
        }
    }

    fn profile_options_len(&self) -> usize {
        1 + self.available_llm_profiles.len()
    }

    fn selected_profile_option(&self) -> Option<String> {
        if self.selected_profile_index == 0 {
            None
        } else {
            self.available_llm_profiles
                .get(self.selected_profile_index - 1)
                .cloned()
        }
    }

    fn open_profile_selector(&mut self) {
        self.show_profile_selector = true;
        self.selected_profile_index = 0;

        if let Some(active) = &self.active_spawn_llm_profile {
            if let Some((idx, _)) = self
                .available_llm_profiles
                .iter()
                .enumerate()
                .find(|(_, p)| *p == active)
            {
                self.selected_profile_index = idx + 1;
            }
        }
    }

    fn get_selected_session_id(&self) -> Option<&str> {
        self.agent_session_ids
            .get(self.selected_agent_index)
            .map(|s| s.as_str())
    }

    /// Returns the session ID to use for the Runs tab:
    /// prefers the active agent selection, falls back to ordered_session_ids (all sessions).
    fn get_runs_session_id(&self) -> Option<&str> {
        if let Some(id) = self.agent_session_ids.get(self.selected_agent_index) {
            return Some(id.as_str());
        }
        self.ordered_session_ids
            .get(self.selected_runs_session_idx)
            .map(|s| s.as_str())
    }

    fn get_filtered_llm_consultations(&self) -> Vec<&(String, LlmConsultation)> {
        if let Some(selected_id) = self.get_selected_session_id() {
            self.llm_consultations
                .iter()
                .filter(|(session_id, _)| session_id == selected_id)
                .collect()
        } else {
            self.llm_consultations.iter().collect()
        }
    }

    fn selected_display_item(&self) -> Option<&RunDisplayItem> {
        self.display_items.get(self.selected_run_index)
    }

    fn sync_run_selection(&mut self) {
        if self.display_items.is_empty() {
            self.selected_run_index = 0;
        } else if self.selected_run_index >= self.display_items.len() {
            self.selected_run_index = self.display_items.len() - 1;
        }
    }

    /// Rebuild the tree-view display items from the raw session_runs list.
    /// Groups runs that share a `schedule_group_id` under a common header row.
    fn rebuild_display_items(&mut self) {
        use std::collections::HashMap as Map;
        self.display_items.clear();

        // Partition into grouped (has schedule_group_id) and singletons.
        let mut group_map: Map<String, Vec<GatewayRunSummary>> = Map::new();
        let mut singletons: Vec<GatewayRunSummary> = Vec::new();

        for run in &self.session_runs {
            if let Some(ref gid) = run.schedule_group_id {
                group_map.entry(gid.clone()).or_default().push(run.clone());
            } else {
                singletons.push(run.clone());
            }
        }

        // Build group header + child rows, ordered by most-recent activity in the group.
        let mut groups: Vec<(String, Vec<GatewayRunSummary>)> = group_map.into_iter().collect();
        // Sort groups so the most recently active one comes first.
        groups.sort_by(|(_, a), (_, b)| {
            let ta = a.iter().map(|r| r.created_at.as_str()).max().unwrap_or("");
            let tb = b.iter().map(|r| r.created_at.as_str()).max().unwrap_or("");
            tb.cmp(ta)
        });

        for (group_id, mut instances) in groups {
            let goal = instances.first().map(|r| r.goal.clone()).unwrap_or_default();
            let schedule = instances
                .first()
                .and_then(|r| r.schedule.as_deref())
                .unwrap_or("?")
                .to_string();

            let pending_run_id = instances
                .iter()
                .find(|r| r.state.contains("Scheduled"))
                .map(|r| r.run_id.clone());
            let next_run_at = instances
                .iter()
                .find(|r| r.state.contains("Scheduled"))
                .and_then(|r| r.next_run_at.clone());

            self.display_items.push(RunDisplayItem::ScheduleGroup {
                group_id,
                goal,
                schedule,
                instance_count: instances.len(),
                next_run_at,
                pending_run_id,
            });

            // Sort instances newest-first (RFC3339 strings sort lexicographically).
            instances.sort_by(|a, b| b.created_at.cmp(&a.created_at));
            let last = instances.len().saturating_sub(1);
            for (idx, run) in instances.into_iter().enumerate() {
                self.display_items.push(RunDisplayItem::RunInstance {
                    run,
                    grouped: true,
                    is_last_in_group: idx == last,
                });
            }
        }

        // Append ungrouped (one-off / plain) runs, newest-first.
        singletons.sort_by(|a, b| b.created_at.cmp(&a.created_at));
        for run in singletons {
            self.display_items.push(RunDisplayItem::RunInstance {
                run,
                grouped: false,
                is_last_in_group: false,
            });
        }
    }

    /// Get filtered event indices for selection navigation
    fn get_filtered_event_indices(&self) -> Vec<usize> {
        let event_visible = |event: &SystemEvent| {
            self.show_internal_steps || !is_internal_step_event(event)
        };

        if let Some(selected_id) = self.get_selected_session_id() {
            self.events
                .iter()
                .enumerate()
                .filter(|(_, e)| e.session_id == selected_id && event_visible(e))
                .map(|(i, _)| i)
                .collect()
        } else {
            self.events
                .iter()
                .enumerate()
                .filter(|(_, e)| event_visible(e))
                .map(|(i, _)| i)
                .collect()
        }
    }

    fn add_event(&mut self, event_type: &str, session_id: &str, details: &str) {
        self.add_event_full(event_type, session_id, details, None, None, None);
    }

    fn add_event_full(
        &mut self,
        event_type: &str,
        session_id: &str,
        details: &str,
        full_details: Option<String>,
        code_execution: Option<CodeExecutionDetails>,
        metadata: Option<serde_json::Value>,
    ) {
        let event_id = self.next_event_id;
        self.next_event_id = self.next_event_id.saturating_add(1);

        self.events.push(SystemEvent {
            id: event_id,
            timestamp: Instant::now(),
            event_type: event_type.to_string(),
            session_id: session_id.to_string(),
            details: details.to_string(),
            full_details,
            code_execution,
            metadata,
        });

        // Keep only last 100 events
        if self.events.len() > 100 {
            self.events.remove(0);
            // Adjust selected_event_index if needed
            if self.selected_event_index > 0 {
                self.selected_event_index -= 1;
            }
        }
    }

    fn add_action_event(
        &mut self,
        session_id: &str,
        details: &str,
        full_details: Option<String>,
        code_execution: Option<CodeExecutionDetails>,
        metadata: Option<serde_json::Value>,
    ) {
        let heartbeat_key = metadata
            .as_ref()
            .and_then(|m| m.get("_heartbeat_key"))
            .and_then(|v| v.as_str())
            .map(|s| s.to_string());

        if let Some(ref hb_key) = heartbeat_key {
            if let Some(last) = self.events.last_mut() {
                let last_hb_key = last
                    .metadata
                    .as_ref()
                    .and_then(|m| m.get("_heartbeat_key"))
                    .and_then(|v| v.as_str());

                if last.event_type == "ACTION"
                    && last.session_id == session_id
                    && last_hb_key == Some(hb_key.as_str())
                {
                    let mut count = last
                        .metadata
                        .as_ref()
                        .and_then(|m| m.get("_heartbeat_count"))
                        .and_then(|v| v.as_u64())
                        .unwrap_or(1);
                    count += 1;

                    let base_details = last
                        .metadata
                        .as_ref()
                        .and_then(|m| m.get("_heartbeat_base_details"))
                        .and_then(|v| v.as_str())
                        .unwrap_or(details)
                        .to_string();

                    last.details = format!("{} ×{}", base_details, count);
                    last.timestamp = Instant::now();

                    if let Some(serde_json::Value::Object(map)) = last.metadata.as_mut() {
                        map.insert(
                            "_heartbeat_count".to_string(),
                            serde_json::Value::from(count),
                        );
                    }
                    return;
                }
            }
        }

        self.add_event_full(
            "ACTION",
            session_id,
            details,
            full_details,
            code_execution,
            metadata,
        );
    }

    /// Get the currently selected event (if any)
    fn get_selected_event(&self) -> Option<&SystemEvent> {
        if let Some(selected_id) = self.selected_event_id {
            return self.events.iter().find(|e| e.id == selected_id);
        }

        let filtered_indices = self.get_filtered_event_indices();
        filtered_indices
            .iter()
            .rev()
            .nth(self.selected_event_index)
            .and_then(|&idx| self.events.get(idx))
    }

    fn event_id_for_display_index(&self, display_idx: usize) -> Option<u64> {
        let filtered_indices = self.get_filtered_event_indices();
        filtered_indices
            .iter()
            .rev()
            .nth(display_idx)
            .and_then(|&idx| self.events.get(idx))
            .map(|event| event.id)
    }

    fn sync_event_selection(&mut self) {
        let filtered_indices = self.get_filtered_event_indices();
        if filtered_indices.is_empty() {
            self.selected_event_index = 0;
            self.selected_event_id = None;
            self.show_event_detail = false;
            return;
        }

        // Keep popup content stable across incoming events by anchoring on event ID
        // only while detail popup is open.
        if self.show_event_detail {
            if let Some(selected_id) = self.selected_event_id {
                if let Some((display_idx, _)) = filtered_indices
                    .iter()
                    .rev()
                    .enumerate()
                    .find(|(_, &idx)| self.events.get(idx).map(|e| e.id) == Some(selected_id))
                {
                    self.selected_event_index = display_idx;
                    return;
                }

                // Selected event disappeared (rotation/filter change) - drop popup anchor.
                self.selected_event_id = None;
                self.show_event_detail = false;
            }
        }

        // Index-driven selection mode (keyboard/mouse hover without popup).
        if self.selected_event_index >= filtered_indices.len() {
            self.selected_event_index = filtered_indices.len() - 1;
        }
        self.selected_event_id = self.event_id_for_display_index(self.selected_event_index);
    }

    fn add_llm_consultation(&mut self, session_id: String, consultation: LlmConsultation) {
        // Also add to events for the Events tab
        let caps = if consultation.planned_capabilities.is_empty() {
            "none".to_string()
        } else {
            consultation.planned_capabilities.join(", ")
        };
        let model = consultation
            .model
            .clone()
            .unwrap_or_else(|| "unknown".to_string());
        let details = format!(
            "iter={} | {} | complete={} | model={} | caps=[{}]",
            consultation.iteration,
            if consultation.is_initial {
                "initial"
            } else {
                "follow-up"
            },
            consultation.task_complete,
            model,
            caps
        );

        let mut full_lines = vec![
            format!("Iteration: {}", consultation.iteration),
            format!(
                "Type: {}",
                if consultation.is_initial {
                    "initial"
                } else {
                    "follow-up"
                }
            ),
            format!("Task complete: {}", consultation.task_complete),
            format!("Model: {}", model),
            format!("Understanding: {}", consultation.understanding),
            format!("Reasoning: {}", consultation.reasoning),
            format!("Planned capabilities: {}", caps),
        ];
        if let Some(ref prompt) = consultation.prompt {
            full_lines.push(format!("Prompt: {}", prompt));
        }
        if let Some(ref response) = consultation.response {
            full_lines.push(format!("Response: {}", response));
        }
        if let Some(ref usage) = consultation.token_usage {
            full_lines.push(format!(
                "Token usage: prompt={} completion={} total={}",
                usage.prompt_tokens, usage.completion_tokens, usage.total_tokens
            ));
        }
        let full_details = full_lines.join("\n");

        let mut metadata_map = serde_json::Map::new();
        metadata_map.insert("iteration".to_string(), serde_json::Value::from(consultation.iteration));
        metadata_map.insert("is_initial".to_string(), serde_json::Value::from(consultation.is_initial));
        metadata_map.insert(
            "task_complete".to_string(),
            serde_json::Value::from(consultation.task_complete),
        );
        if let Some(ref m) = consultation.model {
            metadata_map.insert("model".to_string(), serde_json::Value::String(m.clone()));
        }
        if let Some(ref p) = consultation.prompt {
            metadata_map.insert("prompt".to_string(), serde_json::Value::String(p.clone()));
        }
        if let Some(ref r) = consultation.response {
            metadata_map.insert("response".to_string(), serde_json::Value::String(r.clone()));
        }
        if let Some(ref usage) = consultation.token_usage {
            metadata_map.insert(
                "token_usage".to_string(),
                serde_json::json!({
                    "prompt_tokens": usage.prompt_tokens,
                    "completion_tokens": usage.completion_tokens,
                    "total_tokens": usage.total_tokens
                }),
            );
        }

        self.add_event_full(
            "LLM",
            &session_id,
            &details,
            Some(full_details),
            None,
            Some(serde_json::Value::Object(metadata_map)),
        );

        // Store full consultation for LLM tab
        self.llm_consultations.push((session_id, consultation));

        // Keep only last 50 consultations
        if self.llm_consultations.len() > 50 {
            self.llm_consultations.remove(0);
        }
    }
}

#[derive(Debug, Deserialize)]
struct SpawnLlmProfileResponse {
    profile: Option<String>,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // Initialize tracing to write to a log file instead of stdout/stderr
    // This prevents interference with the TUI
    let log_path = std::env::temp_dir().join("ccos-gateway-monitor.log");
    let log_file = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&log_path)
        .expect("Failed to create log file");

    tracing_subscriber::fmt()
        .with_env_filter("ccos_gateway_monitor=warn")
        .with_writer(std::sync::Mutex::new(log_file))
        .init();

    let args = Args::parse();
    info!("Starting CCOS Gateway Monitor...");
    info!("Gateway URL: {}", args.gateway_url);

    let available_profiles = load_available_llm_profiles(&args.config_path);
    let client = Client::new();
    let active_spawn_llm_profile =
        match fetch_gateway_llm_profile_with_retry(&client, &args.gateway_url, &args.token).await
        {
            Ok(profile) => profile,
            Err(e) => {
                let hint = if e.to_string().contains("503") {
                    " (503 usually means: gateway not ready yet, wrong --gateway-url, or a proxy; ensure ccos-chat-gateway is running and --token matches --admin-tokens)"
                } else if e.to_string().contains("401") {
                    " (401: ensure --token matches the gateway's --admin-tokens / CCOS_ADMIN_TOKENS)"
                } else {
                    ""
                };
                warn!(
                    "Failed to fetch current gateway spawn profile (continuing): {}{}",
                    e, hint
                );
                None
            }
        };

    // Setup terminal
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen, EnableMouseCapture)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    // Create channels
    let (tx, mut rx) = mpsc::channel::<MonitorEvent>(100);

    // Spawn input handler
    let tx_input = tx.clone();
    tokio::spawn(async move {
        loop {
            if event::poll(Duration::from_millis(100)).expect("poll failed") {
                let input_event = event::read().expect("read failed");
                if tx_input
                    .send(MonitorEvent::Input(input_event))
                    .await
                    .is_err()
                {
                    break;
                }
            }
        }
    });

    // Spawn tick handler
    let tx_tick = tx.clone();
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(Duration::from_secs(1));
        loop {
            interval.tick().await;
            if tx_tick.send(MonitorEvent::Tick).await.is_err() {
                break;
            }
        }
    });

    // Spawn session poller
    let gateway_url = args.gateway_url.clone();
    let token = args.token.clone();
    let refresh_interval = args.refresh_interval;
    let tx_sessions = tx.clone();
    tokio::spawn(async move {
        let client = Client::new();
        let mut interval = tokio::time::interval(Duration::from_secs(refresh_interval));

        loop {
            interval.tick().await;

            // Fetch sessions from gateway
            match fetch_sessions(&client, &gateway_url, &token).await {
                Ok(sessions) => {
                    if tx_sessions
                        .send(MonitorEvent::SessionUpdate(sessions))
                        .await
                        .is_err()
                    {
                        break;
                    }
                }
                Err(e) => {
                    warn!("Failed to fetch sessions: {}", e);
                }
            }
        }
    });

    // Spawn WebSocket monitor for each session
    let gateway_url = args.gateway_url.clone();
    let token = args.token.clone();
    let tx_ws = tx.clone();
    tokio::spawn(async move {
        // First, get initial list of sessions
        let client = Client::new();
        match fetch_sessions(&client, &gateway_url, &token).await {
            Ok(sessions) => {
                // Connect to each session's event stream
                for session in sessions {
                    let gateway_url = gateway_url.clone();
                    let token = token.clone();
                    let tx_ws = tx_ws.clone();
                    let session_id = session.session_id.clone();

                    tokio::spawn(async move {
                        connect_to_session_stream(&gateway_url, &session_id, &token, tx_ws).await;
                    });
                }
            }
            Err(e) => {
                error!("Failed to get initial sessions: {}", e);
            }
        }
    });

    // Main loop
    let mut state = MonitorState::new(available_profiles, active_spawn_llm_profile);

    loop {
        state.sync_event_selection();

        // Draw UI
        terminal.draw(|f| draw_ui(f, &state))?;

        // Handle events
        if let Some(event) = rx.recv().await {
            match event {
                MonitorEvent::Input(input_event) => {
                    match input_event {
                        Event::Key(key) => {
                            if state.show_profile_selector {
                                match key.code {
                                    KeyCode::Esc => {
                                        state.show_profile_selector = false;
                                    }
                                    KeyCode::Up => {
                                        let len = state.profile_options_len();
                                        if len > 0 {
                                            if state.selected_profile_index > 0 {
                                                state.selected_profile_index -= 1;
                                            } else {
                                                state.selected_profile_index = len - 1;
                                            }
                                        }
                                    }
                                    KeyCode::Down => {
                                        let len = state.profile_options_len();
                                        if len > 0 {
                                            state.selected_profile_index =
                                                (state.selected_profile_index + 1) % len;
                                        }
                                    }
                                    KeyCode::Enter => {
                                        let selected = state.selected_profile_option();
                                        match set_gateway_llm_profile(
                                            &client,
                                            &args.gateway_url,
                                            &args.token,
                                            selected.as_deref(),
                                        )
                                        .await
                                        {
                                            Ok(applied) => {
                                                state.active_spawn_llm_profile = applied.clone();
                                                state.status_message = match applied {
                                                    Some(profile) => format!(
                                                        "Spawn LLM profile set to '{}'",
                                                        profile
                                                    ),
                                                    None => "Spawn LLM profile override cleared"
                                                        .to_string(),
                                                };
                                            }
                                            Err(e) => {
                                                state.status_message = format!(
                                                    "Failed to set spawn profile: {}",
                                                    e
                                                );
                                            }
                                        }
                                        state.show_profile_selector = false;
                                    }
                                    _ => {}
                                }
                                continue;
                            }

                            // Handle keys while detail popup is open
                            if state.show_event_detail {
                                match key.code {
                                    KeyCode::Esc => {
                                        state.show_event_detail = false;
                                        state.detail_scroll = 0;
                                    }
                                    KeyCode::Up => {
                                        state.detail_scroll = state.detail_scroll.saturating_sub(1);
                                    }
                                    KeyCode::Down => {
                                        state.detail_scroll = state.detail_scroll.saturating_add(1);
                                    }
                                    KeyCode::PageUp => {
                                        state.detail_scroll = state.detail_scroll.saturating_sub(10);
                                    }
                                    KeyCode::PageDown => {
                                        state.detail_scroll = state.detail_scroll.saturating_add(10);
                                    }
                                    KeyCode::Char('y') | KeyCode::Char('c') => {
                                        if let Some(event) = state.get_selected_event() {
                                            let content = build_copy_text(event);
                                            match std::fs::write("/tmp/ccos-monitor-copy.txt", &content) {
                                                Ok(_) => state.status_message = "Copied to /tmp/ccos-monitor-copy.txt".to_string(),
                                                Err(e) => state.status_message = format!("Copy failed: {}", e),
                                            }
                                        }
                                    }
                                    _ => {}
                                }
                                continue;
                            }

                            match key.code {
                                KeyCode::Char('q') => state.should_quit = true,
                                KeyCode::Char('p') => {
                                    state.open_profile_selector();
                                }
                                KeyCode::Char('i') => {
                                    if state.selected_tab == 2 {
                                        state.show_internal_steps = !state.show_internal_steps;
                                        let filtered_count = state.get_filtered_event_indices().len();
                                        if filtered_count == 0 {
                                            state.selected_event_index = 0;
                                        } else if state.selected_event_index >= filtered_count {
                                            state.selected_event_index = filtered_count - 1;
                                        }
                                    }
                                }
                                KeyCode::Char('s') => {
                                    // Runs tab: cycle to next session in ordered_session_ids
                                    if state.selected_tab == 4
                                        && !state.ordered_session_ids.is_empty()
                                    {
                                        state.selected_runs_session_idx = (state
                                            .selected_runs_session_idx
                                            + 1)
                                            % state.ordered_session_ids.len();
                                        match refresh_runs_for_selected_session(
                                            &mut state,
                                            &client,
                                            &args.gateway_url,
                                            &args.token,
                                        )
                                        .await
                                        {
                                            Ok(_) => {
                                                state.status_message = format!(
                                                    "Session {}/{}: {} ({} run(s))",
                                                    state.selected_runs_session_idx + 1,
                                                    state.ordered_session_ids.len(),
                                                    state
                                                        .ordered_session_ids
                                                        .get(state.selected_runs_session_idx)
                                                        .map(|s| s.as_str())
                                                        .unwrap_or("-"),
                                                    state.session_runs.len(),
                                                );
                                            }
                                            Err(e) => {
                                                state.status_message =
                                                    format!("Failed to load runs: {}", e);
                                            }
                                        }
                                    }
                                }
                                KeyCode::Char('r') => {
                                    if state.selected_tab == 4 {
                                        match refresh_runs_for_selected_session(
                                            &mut state,
                                            &client,
                                            &args.gateway_url,
                                            &args.token,
                                        )
                                        .await
                                        {
                                            Ok(_) => {
                                                state.status_message = format!(
                                                    "Loaded {} run(s) for selected session ({} display rows)",
                                                    state.session_runs.len(),
                                                    state.display_items.len(),
                                                );
                                            }
                                            Err(e) => {
                                                state.status_message =
                                                    format!("Failed to refresh runs: {}", e);
                                            }
                                        }
                                    }
                                }
                                KeyCode::Char('c') => {
                                    if state.selected_tab == 4 {
                                        if let Some(run_id_to_cancel) =
                                            state.selected_display_item().and_then(|item| item.cancel_run_id())
                                        {
                                            match cancel_run(
                                                &client,
                                                &args.gateway_url,
                                                &args.token,
                                                &run_id_to_cancel,
                                            )
                                            .await
                                            {
                                                Ok(result) => {
                                                    state.status_message = format!(
                                                        "Run {} cancelled={} (was {})",
                                                        result.run_id,
                                                        result.cancelled,
                                                        result.previous_state
                                                    );
                                                    if let Err(e) = refresh_runs_for_selected_session(
                                                        &mut state,
                                                        &client,
                                                        &args.gateway_url,
                                                        &args.token,
                                                    )
                                                    .await
                                                    {
                                                        state.status_message = format!(
                                                            "Run cancelled, refresh failed: {}",
                                                            e
                                                        );
                                                    }
                                                }
                                                Err(e) => {
                                                    state.status_message =
                                                        format!("Failed to cancel run: {}", e);
                                                }
                                            }
                                        } else {
                                            state.status_message =
                                                "No run/schedule selected to cancel".to_string();
                                        }
                                    }
                                }
                                KeyCode::Tab => {
                                    state.selected_tab = (state.selected_tab + 1) % 5;
                                    if state.selected_tab == 4 {
                                        if let Err(e) = refresh_runs_for_selected_session(
                                            &mut state,
                                            &client,
                                            &args.gateway_url,
                                            &args.token,
                                        )
                                        .await
                                        {
                                            state.status_message =
                                                format!("Failed to load runs: {}", e);
                                        }
                                    }
                                }
                                KeyCode::BackTab => {
                                    state.selected_tab = if state.selected_tab == 0 {
                                        4
                                    } else {
                                        state.selected_tab - 1
                                    };

                                    if state.selected_tab == 4 {
                                        if let Err(e) = refresh_runs_for_selected_session(
                                            &mut state,
                                            &client,
                                            &args.gateway_url,
                                            &args.token,
                                        )
                                        .await
                                        {
                                            state.status_message =
                                                format!("Failed to load runs: {}", e);
                                        }
                                    }
                                }
                                KeyCode::Up => {
                                    // Navigate agents in Agents tab or events in Events tab
                                    if state.selected_tab == 1 && !state.agent_session_ids.is_empty() {
                                        if state.selected_agent_index > 0 {
                                            state.selected_agent_index -= 1;
                                        } else {
                                            state.selected_agent_index =
                                                state.agent_session_ids.len() - 1;
                                        }
                                    } else if state.selected_tab == 2 {
                                        // Navigate events in display order (top to bottom)
                                        let filtered_count = state.get_filtered_event_indices().len();
                                        if filtered_count > 0 {
                                            if state.selected_event_index > 0 {
                                                state.selected_event_index -= 1;
                                            } else {
                                                state.selected_event_index = filtered_count - 1;
                                            }
                                        }
                                    } else if state.selected_tab == 4 {
                                        if !state.display_items.is_empty() {
                                            if state.selected_run_index > 0 {
                                                state.selected_run_index -= 1;
                                            } else {
                                                state.selected_run_index = state.display_items.len() - 1;
                                            }
                                        }
                                    }
                                }
                                KeyCode::Down => {
                                    // Navigate agents in Agents tab or events in Events tab
                                    if state.selected_tab == 1 && !state.agent_session_ids.is_empty() {
                                        state.selected_agent_index = (state.selected_agent_index + 1)
                                            % state.agent_session_ids.len();
                                    } else if state.selected_tab == 2 {
                                        // Navigate events in display order (top to bottom)
                                        let filtered_count = state.get_filtered_event_indices().len();
                                        if filtered_count > 0 {
                                            if state.selected_event_index < filtered_count - 1 {
                                                state.selected_event_index += 1;
                                            } else {
                                                state.selected_event_index = 0;
                                            }
                                        }
                                    } else if state.selected_tab == 4 {
                                        if !state.display_items.is_empty() {
                                            state.selected_run_index =
                                                (state.selected_run_index + 1) % state.display_items.len();
                                        }
                                    }
                                }
                                KeyCode::PageUp => {
                                    if state.selected_tab == 2 {
                                        let filtered_count = state.get_filtered_event_indices().len();
                                        if filtered_count > 0 {
                                            let events_area = get_events_tab_area(terminal.size()?);
                                            let page_size = events_area.height.saturating_sub(2).max(1) as usize;
                                            state.selected_event_index =
                                                state.selected_event_index.saturating_sub(page_size);
                                        }
                                    }
                                }
                                KeyCode::PageDown => {
                                    if state.selected_tab == 2 {
                                        let filtered_count = state.get_filtered_event_indices().len();
                                        if filtered_count > 0 {
                                            let events_area = get_events_tab_area(terminal.size()?);
                                            let page_size = events_area.height.saturating_sub(2).max(1) as usize;
                                            state.selected_event_index = (state.selected_event_index + page_size)
                                                .min(filtered_count - 1);
                                        }
                                    }
                                }
                                KeyCode::Home => {
                                    if state.selected_tab == 2 {
                                        if !state.get_filtered_event_indices().is_empty() {
                                            state.selected_event_index = 0;
                                        }
                                    }
                                }
                                KeyCode::End => {
                                    if state.selected_tab == 2 {
                                        let filtered_count = state.get_filtered_event_indices().len();
                                        if filtered_count > 0 {
                                            state.selected_event_index = filtered_count - 1;
                                        }
                                    }
                                }
                                KeyCode::Enter => {
                                    // Show event detail in Events tab
                                    if state.selected_tab == 2 {
                                        let filtered_indices = state.get_filtered_event_indices();
                                        if !filtered_indices.is_empty() {
                                            state.selected_event_id =
                                                state.event_id_for_display_index(state.selected_event_index);
                                            state.show_event_detail = true;
                                            state.detail_scroll = 0;
                                        }
                                    }
                                }
                                _ => {}
                            }
                        }
                        Event::Mouse(mouse) => {
                            // Allow right-click to close popup
                            if state.show_event_detail {
                                if let MouseEventKind::Down(MouseButton::Right) = mouse.kind {
                                    state.show_event_detail = false;
                                }
                                continue;
                            }

                            if state.selected_tab == 2 {
                                let events_area = get_events_tab_area(terminal.size()?);
                                let in_events_area = mouse.column >= events_area.x
                                    && mouse.column < events_area.x + events_area.width
                                    && mouse.row >= events_area.y
                                    && mouse.row < events_area.y + events_area.height;

                                if in_events_area {
                                    let filtered_count = state.get_filtered_event_indices().len();

                                    if filtered_count > 0 {
                                        match mouse.kind {
                                            MouseEventKind::ScrollUp => {
                                                if state.selected_event_index > 0 {
                                                    state.selected_event_index -= 1;
                                                }
                                            }
                                            MouseEventKind::ScrollDown => {
                                                if state.selected_event_index < filtered_count - 1 {
                                                    state.selected_event_index += 1;
                                                }
                                            }
                                            MouseEventKind::Down(MouseButton::Left) => {
                                                // Select clicked row inside the list viewport (inside block borders)
                                                if events_area.height > 2 && mouse.row > events_area.y {
                                                    let viewport_height = events_area.height.saturating_sub(2) as usize;
                                                    let inner_row = mouse.row.saturating_sub(events_area.y + 1) as usize;

                                                    if inner_row < viewport_height {
                                                        let (_, scroll_offset) = compute_events_viewport(
                                                            filtered_count,
                                                            state.selected_event_index,
                                                            viewport_height,
                                                        );
                                                        let clicked_display_idx = scroll_offset + inner_row;
                                                        if clicked_display_idx < filtered_count {
                                                            let clicked_event_id =
                                                                state.event_id_for_display_index(clicked_display_idx);
                                                            if state.show_event_detail
                                                                && state.selected_event_id == clicked_event_id
                                                                && clicked_event_id.is_some()
                                                                && state.show_event_detail
                                                            {
                                                                state.show_event_detail = false;
                                                            } else {
                                                                state.selected_event_index = clicked_display_idx;
                                                                state.selected_event_id = clicked_event_id;
                                                                state.show_event_detail = true;
                                                            }
                                                        }
                                                    }
                                                }
                                            }
                                            _ => {}
                                        }
                                    }
                                }
                            } else if state.selected_tab == 4 {
                                let runs_area = get_runs_tab_area(terminal.size()?);
                                let in_runs_area = mouse.column >= runs_area.x
                                    && mouse.column < runs_area.x + runs_area.width
                                    && mouse.row >= runs_area.y
                                    && mouse.row < runs_area.y + runs_area.height;

                                if in_runs_area {
                                    let items_count = state.display_items.len();

                                    if items_count > 0 {
                                        match mouse.kind {
                                            MouseEventKind::ScrollUp => {
                                                if state.selected_run_index > 0 {
                                                    state.selected_run_index -= 1;
                                                }
                                            }
                                            MouseEventKind::ScrollDown => {
                                                if state.selected_run_index < items_count - 1 {
                                                    state.selected_run_index += 1;
                                                }
                                            }
                                            MouseEventKind::Down(MouseButton::Left) => {
                                                if runs_area.height > 2 && mouse.row > runs_area.y {
                                                    let viewport_height =
                                                        runs_area.height.saturating_sub(2) as usize;
                                                    let inner_row =
                                                        mouse.row.saturating_sub(runs_area.y + 1) as usize;

                                                    if inner_row < viewport_height {
                                                        let max_offset =
                                                            items_count.saturating_sub(viewport_height);
                                                        let scroll_offset = state
                                                            .selected_run_index
                                                            .saturating_sub(viewport_height / 2)
                                                            .min(max_offset);
                                                        let clicked_index = scroll_offset + inner_row;

                                                        if clicked_index < items_count {
                                                            state.selected_run_index = clicked_index;
                                                        }
                                                    }
                                                }
                                            }
                                            _ => {}
                                        }
                                    }
                                }
                            }
                        }
                        _ => {}
                    }
                }
                MonitorEvent::SessionUpdate(sessions) => {
                    // Track which sessions we're already connected to
                    let existing_session_ids: std::collections::HashSet<_> =
                        state.sessions.keys().cloned().collect();

                    state.sessions.clear();
                    state.agents.clear();
                    state.agent_session_ids.clear();

                    // Rebuild ordered list of all session IDs for Runs tab navigation.
                    let mut all_ids: Vec<String> =
                        sessions.iter().map(|s| s.session_id.clone()).collect();
                    all_ids.sort();
                    state.ordered_session_ids = all_ids;
                    if state.selected_runs_session_idx >= state.ordered_session_ids.len() {
                        state.selected_runs_session_idx =
                            state.ordered_session_ids.len().saturating_sub(1);
                    }

                    for session in &sessions {
                        state
                            .sessions
                            .insert(session.session_id.clone(), session.clone());

                        // Populate agents from session info (only sessions with active agents)
                        if let Some(pid) = session.agent_pid {
                            if pid > 0 {
                                state.agents.insert(
                                    session.session_id.clone(),
                                    AgentInfo {
                                        pid,
                                        session_id: session.session_id.clone(),
                                        current_step: session.current_step.unwrap_or(0),
                                        memory_mb: session.memory_mb,
                                        is_healthy: session.status == "Active",
                                        last_heartbeat: Instant::now(),
                                    },
                                );
                                // Track ordered list of agent session IDs for selection
                                state.agent_session_ids.push(session.session_id.clone());
                            }
                        }

                        // Spawn WebSocket connection for new sessions
                        if !existing_session_ids.contains(&session.session_id) {
                            let gateway_url = args.gateway_url.clone();
                            let token = args.token.clone();
                            let session_id = session.session_id.clone();
                            let tx_ws = tx.clone();

                            tokio::spawn(async move {
                                connect_to_session_stream(&gateway_url, &session_id, &token, tx_ws)
                                    .await;
                            });
                        }
                    }

                    // Ensure selected_agent_index is valid
                    if state.selected_agent_index >= state.agent_session_ids.len() {
                        state.selected_agent_index = if state.agent_session_ids.is_empty() {
                            0
                        } else {
                            state.agent_session_ids.len() - 1
                        };
                    }

                    let runs_refresh_error = if let Err(e) = refresh_runs_for_selected_session(
                        &mut state,
                        &client,
                        &args.gateway_url,
                        &args.token,
                    )
                    .await
                    {
                        Some(e.to_string())
                    } else {
                        None
                    };

                    // Ensure selected_event_index remains valid for current Events filter
                    let filtered_count = state.get_filtered_event_indices().len();
                    if filtered_count == 0 {
                        state.selected_event_index = 0;
                    } else if state.selected_event_index >= filtered_count {
                        state.selected_event_index = filtered_count - 1;
                    }

                    state.status_message = if let Some(err) = runs_refresh_error {
                        format!(
                            "Updated {} sessions (runs refresh error: {})",
                            state.sessions.len(),
                            err
                        )
                    } else {
                        format!("Updated {} sessions", state.sessions.len())
                    };
                }

                MonitorEvent::AgentHeartbeat(session_id, agent_info) => {
                    state.agents.insert(session_id, agent_info);
                }
                MonitorEvent::AgentCrashed(session_id, pid) => {
                    state.add_event("CRASH", &session_id, &format!("Agent PID {} crashed", pid));
                    state.status_message = format!("⚠️  Agent crashed: {}", session_id);
                }
                MonitorEvent::ActionEvent(session_id, action_details) => {
                    let error_text = action_details
                        .metadata
                        .as_ref()
                        .and_then(|m| m.get("error"))
                        .and_then(|v| v.as_str())
                        .map(|s| s.to_string());

                    let is_internal_step = is_internal_step_action(&action_details);
                    let is_heartbeat = is_get_status_heartbeat(&action_details);

                    let call_name = if !action_details.function_name.is_empty() {
                        action_details.function_name.clone()
                    } else {
                        action_details.action_type.clone()
                    };

                    let duration_suffix = action_details
                        .duration_ms
                        .map(|ms| format!(" ({}ms)", ms))
                        .unwrap_or_default();

                    let status_label = match action_details.success {
                        Some(true) => "ok",
                        Some(false) => "error",
                        None => "done",
                    };

                    let mut result_summary = if let Some(err) = &error_text {
                        format!("error: {}", single_line(err, 120))
                    } else if !action_details.summary.trim().is_empty()
                        && action_details.summary != action_details.action_type
                    {
                        single_line(&action_details.summary, 120)
                    } else {
                        String::new()
                    };

                    let run_create_program = action_details
                        .metadata
                        .as_ref()
                        .and_then(extract_run_create_program_details);
                    let memory_operation = action_details
                        .metadata
                        .as_ref()
                        .and_then(extract_memory_operation_details);

                    if let Some(program) = run_create_program.as_ref() {
                        let program_summary = single_line(&format_run_create_program_summary(program), 120);
                        if result_summary.is_empty() {
                            result_summary = format!("program: {}", program_summary);
                        } else {
                            result_summary = format!("{} | program: {}", result_summary, program_summary);
                        }
                    }

                    if let Some(memory) = memory_operation.as_ref() {
                        let memory_summary = single_line(&format_memory_operation_summary(memory), 120);
                        if result_summary.is_empty() {
                            result_summary = format!("memory: {}", memory_summary);
                        } else {
                            result_summary = format!("{} | memory: {}", result_summary, memory_summary);
                        }
                    }

                    if result_summary == status_label {
                        result_summary.clear();
                    }

                    // Build summary string for the event list (single-line call → result)
                    let details = if result_summary.is_empty() {
                        format!("{} -> {}{}", call_name, status_label, duration_suffix)
                    } else {
                        format!(
                            "{} -> {}{} | {}",
                            call_name, status_label, duration_suffix, result_summary
                        )
                    };

                    // Build full details for detail view
                    let mut full_lines = vec![format!("Action: {}", action_details.action_type)];
                    if !action_details.function_name.is_empty() {
                        full_lines.push(format!("Function: {}", action_details.function_name));
                    }
                    if let Some(s) = action_details.success {
                        full_lines.push(format!("Success: {}", s));
                    }
                    if let Some(ms) = action_details.duration_ms {
                        full_lines.push(format!("Duration: {}ms", ms));
                    }
                    if !action_details.summary.is_empty() {
                        full_lines.push(format!("Summary: {}", action_details.summary));
                    }
                    if let Some(program) = run_create_program.as_ref() {
                        full_lines.push("Programmed Run:".to_string());
                        full_lines.push(format!("  Goal: {}", program.goal));
                        if let Some(session_id) = &program.target_session_id {
                            full_lines.push(format!("  Target Session: {}", session_id));
                        }
                        full_lines.push(format!(
                            "  Schedule: {}",
                            program.schedule.as_deref().unwrap_or("none")
                        ));
                        full_lines.push(format!(
                            "  Next Run At: {}",
                            program.next_run_at.as_deref().unwrap_or("none")
                        ));
                        full_lines.push(format!(
                            "  Max Runs: {}",
                            program.max_run.as_deref().unwrap_or("none")
                        ));
                        full_lines.push(format!(
                            "  Budget: {}",
                            program.budget.as_deref().unwrap_or("none")
                        ));
                        full_lines.push(format!(
                            "  Execution Mode: {}",
                            program.execution_mode.as_deref().unwrap_or("llm_agent")
                        ));
                        if let Some(trigger_capability_id) = &program.trigger_capability_id {
                            full_lines.push(format!("  Trigger Capability: {}", trigger_capability_id));
                        }
                        if let Some(trigger_inputs) = &program.trigger_inputs {
                            full_lines.push(format!("  Trigger Inputs: {}", trigger_inputs));
                        }
                        if let Some(parent_run_id) = &program.parent_run_id {
                            full_lines.push(format!("  Parent Run: {}", parent_run_id));
                        }
                    }
                    if let Some(memory) = memory_operation.as_ref() {
                        full_lines.push("Memory Operation:".to_string());
                        full_lines.push(format!("  Type: {}", memory.operation));
                        if let Some(key) = &memory.key {
                            full_lines.push(format!("  Key: {}", key));
                        }
                        if memory.operation == "store" {
                            full_lines.push(format!(
                                "  Value Stored: {}",
                                memory.store_value.as_deref().unwrap_or("none")
                            ));
                            full_lines.push(format!(
                                "  Entry ID: {}",
                                memory.store_entry_id.as_deref().unwrap_or("none")
                            ));
                            full_lines.push(format!(
                                "  Store Success: {}",
                                memory
                                    .store_success
                                    .map(|v| v.to_string())
                                    .unwrap_or_else(|| "none".to_string())
                            ));
                        } else {
                            full_lines.push(format!(
                                "  Found: {}",
                                memory
                                    .get_found
                                    .map(|v| v.to_string())
                                    .unwrap_or_else(|| "none".to_string())
                            ));
                            full_lines.push(format!(
                                "  Expired: {}",
                                memory
                                    .get_expired
                                    .map(|v| v.to_string())
                                    .unwrap_or_else(|| "none".to_string())
                            ));
                            full_lines.push(format!(
                                "  Value Retrieved: {}",
                                memory.get_value.as_deref().unwrap_or("none")
                            ));
                        }
                    }
                    if let Some(err) = &error_text {
                        full_lines.push("Error:".to_string());
                        for line in err.lines().take(20) {
                            full_lines.push(format!("  {}", line));
                        }
                        if err.lines().count() > 20 {
                            full_lines.push("  ... (truncated)".to_string());
                        }
                    }
                    let full_details = full_lines.join("\n");

                    let mut event_metadata = action_details.metadata.clone();
                    if !matches!(event_metadata, Some(serde_json::Value::Object(_))) {
                        event_metadata = Some(serde_json::Value::Object(serde_json::Map::new()));
                    }
                    if let Some(serde_json::Value::Object(map)) = event_metadata.as_mut() {
                        map.insert(
                            "_is_internal_step".to_string(),
                            serde_json::Value::from(is_internal_step),
                        );
                        if is_heartbeat {
                            map.insert(
                                "_heartbeat_key".to_string(),
                                serde_json::Value::String(format!("{}:{}", session_id, call_name)),
                            );
                            map.insert(
                                "_heartbeat_count".to_string(),
                                serde_json::Value::from(1_u64),
                            );
                            map.insert(
                                "_heartbeat_base_details".to_string(),
                                serde_json::Value::String(details.clone()),
                            );
                        }
                    }

                    state.add_action_event(
                        &session_id,
                        &details,
                        Some(full_details),
                        action_details.code_execution.clone(),
                        event_metadata,
                    );
                }
                MonitorEvent::LlmConsultation(session_id, consultation) => {
                    state.add_llm_consultation(session_id, consultation);
                }
                MonitorEvent::Tick => {}
            }
        }

        state.sync_event_selection();

        if state.should_quit {
            break;
        }
    }

    // Cleanup
    disable_raw_mode()?;
    execute!(
        terminal.backend_mut(),
        LeaveAlternateScreen,
        DisableMouseCapture
    )?;
    terminal.show_cursor()?;

    Ok(())
}

fn load_available_llm_profiles(config_path: &str) -> Vec<String> {
    let mut profiles = Vec::new();
    let mut seen = HashSet::new();

    let path = std::path::Path::new(config_path);
    let resolved_path = if path.exists() {
        path.to_path_buf()
    } else {
        std::path::Path::new("..").join(config_path)
    };

    let content = match std::fs::read_to_string(&resolved_path) {
        Ok(content) => content,
        Err(e) => {
            warn!(
                "Failed to read agent config '{}': {}",
                resolved_path.display(),
                e
            );
            return profiles;
        }
    };

    let normalized = if content.starts_with("# RTFS") {
        content.lines().skip(1).collect::<Vec<_>>().join("\n")
    } else {
        content
    };

    let config = match toml::from_str::<ccos::config::types::AgentConfig>(&normalized) {
        Ok(config) => config,
        Err(e) => {
            warn!(
                "Failed to parse agent config '{}': {}",
                resolved_path.display(),
                e
            );
            return profiles;
        }
    };

    if let Some(llm_profiles) = config.llm_profiles {
        for profile in llm_profiles.profiles {
            if seen.insert(profile.name.clone()) {
                profiles.push(profile.name);
            }
        }

        if let Some(model_sets) = llm_profiles.model_sets {
            for set in model_sets {
                for model in set.models {
                    let synthetic = format!("{}:{}", set.name, model.name);
                    if seen.insert(synthetic.clone()) {
                        profiles.push(synthetic);
                    }
                }
            }
        }
    }

    profiles.sort();
    profiles
}

/// Retry count and delay for initial profile fetch (gateway may not be ready yet).
const PROFILE_FETCH_RETRIES: u32 = 3;
const PROFILE_FETCH_DELAY_SECS: u64 = 2;

/// Fetch current spawn LLM profile from the gateway, with retries.
/// Retries help when the monitor is started at the same time as the gateway (e.g. by run_demo or ccos-chat).
async fn fetch_gateway_llm_profile_with_retry(
    client: &Client,
    gateway_url: &str,
    token: &str,
) -> anyhow::Result<Option<String>> {
    let mut last_err = None;
    for attempt in 0..=PROFILE_FETCH_RETRIES {
        if attempt > 0 {
            tokio::time::sleep(Duration::from_secs(PROFILE_FETCH_DELAY_SECS)).await;
        }
        match fetch_gateway_llm_profile(client, gateway_url, token).await {
            Ok(profile) => return Ok(profile),
            Err(e) => {
                last_err = Some(e);
            }
        }
    }
    Err(last_err.unwrap_or_else(|| anyhow::anyhow!("No attempts")))
}

async fn fetch_gateway_llm_profile(
    client: &Client,
    gateway_url: &str,
    token: &str,
) -> anyhow::Result<Option<String>> {
    let url = format!("{}/chat/admin/llm-profile", gateway_url);
    let resp = client.get(&url).header("X-Admin-Token", token).send().await?;

    if !resp.status().is_success() {
        return Err(anyhow::anyhow!(
            "Failed to get spawn profile: HTTP {}",
            resp.status()
        ));
    }

    let body = resp.json::<SpawnLlmProfileResponse>().await?;
    Ok(body.profile)
}

async fn set_gateway_llm_profile(
    client: &Client,
    gateway_url: &str,
    token: &str,
    profile: Option<&str>,
) -> anyhow::Result<Option<String>> {
    let url = format!("{}/chat/admin/llm-profile", gateway_url);
    let resp = client
        .post(&url)
        .header("X-Admin-Token", token)
        .json(&serde_json::json!({ "profile": profile }))
        .send()
        .await?;

    if !resp.status().is_success() {
        return Err(anyhow::anyhow!(
            "Failed to set spawn profile: HTTP {}",
            resp.status()
        ));
    }

    let body = resp.json::<SpawnLlmProfileResponse>().await?;
    Ok(body.profile)
}

async fn fetch_sessions(
    client: &Client,
    gateway_url: &str,
    token: &str,
) -> anyhow::Result<Vec<SessionInfo>> {
    let url = format!("{}/chat/sessions", gateway_url);
    let resp = client
        .get(&url)
        .header("X-Agent-Token", token)
        .send()
        .await?;

    if !resp.status().is_success() {
        return Err(anyhow::anyhow!(
            "Failed to fetch sessions: HTTP {}",
            resp.status()
        ));
    }

    let sessions = resp.json::<Vec<SessionInfo>>().await?;
    Ok(sessions)
}

async fn fetch_session_runs(
    client: &Client,
    gateway_url: &str,
    token: &str,
    session_id: &str,
) -> anyhow::Result<GatewayListRunsResponse> {
    let url = format!("{}/chat/run?session_id={}", gateway_url, session_id);
    let resp = client
        .get(&url)
        .header("X-Agent-Token", token)
        .send()
        .await?;

    if !resp.status().is_success() {
        return Err(anyhow::anyhow!(
            "Failed to fetch runs for session {}: HTTP {}",
            session_id,
            resp.status()
        ));
    }

    let runs = resp.json::<GatewayListRunsResponse>().await?;
    Ok(runs)
}

async fn cancel_run(
    client: &Client,
    gateway_url: &str,
    token: &str,
    run_id: &str,
) -> anyhow::Result<GatewayCancelRunResponse> {
    let url = format!("{}/chat/run/{}/cancel", gateway_url, run_id);
    let resp = client
        .post(&url)
        .header("X-Agent-Token", token)
        .send()
        .await?;

    if !resp.status().is_success() {
        return Err(anyhow::anyhow!(
            "Failed to cancel run {}: HTTP {}",
            run_id,
            resp.status()
        ));
    }

    let body = resp.json::<GatewayCancelRunResponse>().await?;
    Ok(body)
}

async fn refresh_runs_for_selected_session(
    state: &mut MonitorState,
    client: &Client,
    gateway_url: &str,
    token: &str,
) -> anyhow::Result<()> {
    // Use get_runs_session_id() which falls back to all known sessions, not just active agents.
    let Some(session_id) = state.get_runs_session_id().map(|s| s.to_string()) else {
        state.session_runs.clear();
        state.runs_session_id = None;
        state.selected_run_index = 0;
        return Ok(());
    };

    let response = fetch_session_runs(client, gateway_url, token, &session_id).await?;
    state.runs_session_id = Some(response.session_id);
    // Store raw runs and rebuild the grouped tree view.
    state.session_runs = response.runs;
    state.rebuild_display_items();
    state.sync_run_selection();
    Ok(())
}

async fn connect_to_session_stream(
    gateway_url: &str,
    session_id: &str,
    token: &str,
    tx: mpsc::Sender<MonitorEvent>,
) {
    let ws_url = gateway_url
        .replace("http://", "ws://")
        .replace("https://", "wss://");
    let url = format!("{}/chat/stream/{}?token={}", ws_url, session_id, token);

    loop {
        match connect_async(&url).await {
            Ok((ws_stream, _)) => {
                info!("Connected to session stream: {}", session_id);
                let (_, mut read) = ws_stream.split();

                while let Some(msg) = read.next().await {
                    match msg {
                        Ok(Message::Text(text)) => {
                            if let Ok(event) = serde_json::from_str::<serde_json::Value>(&text) {
                                let event_type = event
                                    .get("event_type")
                                    .and_then(|v| v.as_str())
                                    .unwrap_or("unknown");

                                match event_type {
                                    "historical" | "action" => {
                                        if let Some(action) = event.get("action") {
                                            let action_type = action
                                                .get("action_type")
                                                .and_then(|v| v.as_str())
                                                .unwrap_or("unknown");

                                            // Check if this is an AgentLlmConsultation
                                            if action_type == "AgentLlmConsultation" {
                                                // Extract LLM consultation details from metadata
                                                if let Some(metadata) = action.get("metadata") {
                                                    let consultation = LlmConsultation {
                                                        iteration: metadata
                                                            .get("iteration")
                                                            .and_then(|v| v.as_u64())
                                                            .unwrap_or(0)
                                                            as u32,
                                                        is_initial: metadata
                                                            .get("is_initial")
                                                            .and_then(|v| v.as_bool())
                                                            .unwrap_or(false),
                                                        understanding: metadata
                                                            .get("understanding")
                                                            .and_then(|v| v.as_str())
                                                            .unwrap_or("")
                                                            .to_string(),
                                                        reasoning: metadata
                                                            .get("reasoning")
                                                            .and_then(|v| v.as_str())
                                                            .unwrap_or("")
                                                            .to_string(),
                                                        task_complete: metadata
                                                            .get("task_complete")
                                                            .and_then(|v| v.as_bool())
                                                            .unwrap_or(false),
                                                        planned_capabilities: metadata
                                                            .get("planned_capabilities")
                                                            .and_then(|v| v.as_array())
                                                            .map(|arr| {
                                                                arr.iter()
                                                                    .filter_map(|v| {
                                                                        // planned_capabilities is an array of objects with capability_id field
                                                                        v.get("capability_id")
                                                                            .and_then(|c| {
                                                                                c.as_str()
                                                                            })
                                                                            .map(|s| s.to_string())
                                                                    })
                                                                    .collect()
                                                            })
                                                            .unwrap_or_default(),
                                                        model: metadata
                                                            .get("model")
                                                            .and_then(|v| v.as_str())
                                                            .map(|s| s.to_string()),
                                                        prompt: metadata
                                                            .get("prompt")
                                                            .and_then(|v| v.as_str())
                                                            .map(|s| s.to_string()),
                                                        response: metadata
                                                            .get("response")
                                                            .and_then(|v| v.as_str())
                                                            .map(|s| s.to_string()),
                                                        token_usage: metadata
                                                            .get("token_usage")
                                                            .and_then(|v| v.as_object())
                                                            .map(|usage| TokenUsageDetails {
                                                                prompt_tokens: usage
                                                                    .get("prompt_tokens")
                                                                    .and_then(|v| v.as_u64())
                                                                    .unwrap_or(0),
                                                                completion_tokens: usage
                                                                    .get("completion_tokens")
                                                                    .and_then(|v| v.as_u64())
                                                                    .unwrap_or(0),
                                                                total_tokens: usage
                                                                    .get("total_tokens")
                                                                    .and_then(|v| v.as_u64())
                                                                    .unwrap_or(0),
                                                            }),
                                                    };
                                                    let _ = tx
                                                        .send(MonitorEvent::LlmConsultation(
                                                            session_id.to_string(),
                                                            consultation,
                                                        ))
                                                        .await;
                                                }
                                            } else {
                                                // Regular action - extract all details
                                                let function_name = action
                                                    .get("function_name")
                                                    .and_then(|v| v.as_str())
                                                    .unwrap_or("")
                                                    .to_string();
                                                let success =
                                                    action.get("success").and_then(|v| v.as_bool());
                                                let duration_ms = action
                                                    .get("duration_ms")
                                                    .and_then(|v| v.as_u64());
                                                let summary = action
                                                    .get("summary")
                                                    .and_then(|v| v.as_str())
                                                    .unwrap_or("")
                                                    .to_string();

                                                // Extract code execution details from metadata
                                                let metadata_val = action.get("metadata").cloned();
                                                let metadata = action.get("metadata");
                                                let code = metadata
                                                    .and_then(|m| m.get("code"))
                                                    .and_then(|v| v.as_str())
                                                    .map(|s| s.to_string());
                                                let language = metadata
                                                    .and_then(|m| m.get("language"))
                                                    .and_then(|v| v.as_str())
                                                    .map(|s| s.to_string());
                                                let stdout = metadata
                                                    .and_then(|m| m.get("stdout"))
                                                    .and_then(|v| v.as_str())
                                                    .map(|s| s.to_string());
                                                let stderr = metadata
                                                    .and_then(|m| m.get("stderr"))
                                                    .and_then(|v| v.as_str())
                                                    .map(|s| s.to_string());
                                                let exit_code = metadata
                                                    .and_then(|m| m.get("exit_code"))
                                                    .and_then(|v| v.as_i64());

                                                // Build code execution details if any code-related field is present
                                                let code_execution = if code.is_some()
                                                    || stdout.is_some()
                                                    || stderr.is_some()
                                                {
                                                    Some(CodeExecutionDetails {
                                                        language: language.unwrap_or_else(|| {
                                                            "unknown".to_string()
                                                        }),
                                                        code: code.unwrap_or_default(),
                                                        stdout: stdout.unwrap_or_default(),
                                                        stderr: stderr.unwrap_or_default(),
                                                        exit_code: exit_code.map(|c| c as i32),
                                                        duration_ms,
                                                    })
                                                } else {
                                                    None
                                                };

                                                let action_details = ActionEventDetails {
                                                    action_type: action_type.to_string(),
                                                    function_name,
                                                    success,
                                                    duration_ms,
                                                    summary,
                                                    code_execution,
                                                    metadata: metadata_val,
                                                };

                                                let _ = tx
                                                    .send(MonitorEvent::ActionEvent(
                                                        session_id.to_string(),
                                                        action_details,
                                                    ))
                                                    .await;
                                            }
                                        }
                                    }
                                    "state_update" => {
                                        if let Some(state) = event.get("state") {
                                            let agent_info = AgentInfo {
                                                pid: state
                                                    .get("agent_pid")
                                                    .and_then(|v| v.as_u64())
                                                    .unwrap_or(0)
                                                    as u32,
                                                session_id: session_id.to_string(),
                                                current_step: state
                                                    .get("current_step")
                                                    .and_then(|v| v.as_u64())
                                                    .unwrap_or(0)
                                                    as u32,
                                                memory_mb: state
                                                    .get("memory_mb")
                                                    .and_then(|v| v.as_u64()),
                                                is_healthy: state
                                                    .get("is_healthy")
                                                    .and_then(|v| v.as_bool())
                                                    .unwrap_or(false),
                                                last_heartbeat: Instant::now(),
                                            };
                                            let _ = tx
                                                .send(MonitorEvent::AgentHeartbeat(
                                                    session_id.to_string(),
                                                    agent_info,
                                                ))
                                                .await;
                                        }
                                    }
                                    "agent_crashed" => {
                                        if let Some(crash) = event.get("agent_crashed") {
                                            let pid = crash
                                                .get("pid")
                                                .and_then(|v| v.as_u64())
                                                .unwrap_or(0)
                                                as u32;
                                            let _ = tx
                                                .send(MonitorEvent::AgentCrashed(
                                                    session_id.to_string(),
                                                    pid,
                                                ))
                                                .await;
                                        }
                                    }
                                    _ => {}
                                }
                            }
                        }
                        Ok(Message::Close(_)) => {
                            warn!("Session stream closed: {}", session_id);
                            break;
                        }
                        Err(e) => {
                            warn!("WebSocket error for {}: {}", session_id, e);
                            break;
                        }
                        _ => {}
                    }
                }
            }
            Err(e) => {
                warn!("Failed to connect to {}: {}, retrying...", session_id, e);
            }
        }

        tokio::time::sleep(Duration::from_secs(5)).await;
    }
}

fn get_events_tab_area(screen: Rect) -> Rect {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .margin(1)
        .constraints([
            Constraint::Length(3),
            Constraint::Min(10),
            Constraint::Length(3),
        ])
        .split(screen);

    let main_chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(3), Constraint::Min(10)])
        .split(chunks[1]);

    main_chunks[1]
}

fn get_runs_tab_area(screen: Rect) -> Rect {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .margin(1)
        .constraints([
            Constraint::Length(3),
            Constraint::Min(10),
            Constraint::Length(3),
        ])
        .split(screen);

    let main_chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(3), Constraint::Min(10)])
        .split(chunks[1]);

    main_chunks[1]
}

fn compute_events_viewport(
    filtered_count: usize,
    selected_event_index: usize,
    viewport_height: usize,
) -> (usize, usize) {
    let selected_display_idx = if filtered_count == 0 {
        0
    } else {
        selected_event_index.min(filtered_count - 1)
    };

    let scroll_offset = if viewport_height == 0 {
        0
    } else {
        selected_display_idx.saturating_sub(viewport_height.saturating_sub(1))
    };

    (selected_display_idx, scroll_offset)
}

fn single_line(text: &str, max_chars: usize) -> String {
    let normalized = text.split_whitespace().collect::<Vec<_>>().join(" ");
    if normalized.chars().count() > max_chars {
        let truncated: String = normalized.chars().take(max_chars.saturating_sub(3)).collect();
        format!("{}...", truncated)
    } else {
        normalized
    }
}

fn json_value_to_display(value: &serde_json::Value) -> String {
    match value {
        serde_json::Value::Null => "null".to_string(),
        serde_json::Value::Bool(v) => v.to_string(),
        serde_json::Value::Number(v) => v.to_string(),
        serde_json::Value::String(v) => v.clone(),
        other => serde_json::to_string(other).unwrap_or_else(|_| "<invalid>".to_string()),
    }
}

fn extract_run_create_program_details(
    metadata: &serde_json::Value,
) -> Option<RunCreateProgramDetails> {
    let goal = metadata
        .get("run_create_goal")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string())?;

    let target_session_id = metadata
        .get("run_create_session_id")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());
    let schedule = metadata
        .get("run_create_schedule")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());
    let next_run_at = metadata
        .get("run_create_next_run_at")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());
    let max_run = metadata
        .get("run_create_max_run")
        .map(json_value_to_display);
    let budget = metadata
        .get("run_create_budget")
        .map(json_value_to_display)
        .map(|s| single_line(&s, 120));
    let execution_mode = metadata
        .get("run_create_execution_mode")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());
    let trigger_capability_id = metadata
        .get("run_create_trigger_capability_id")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());
    let trigger_inputs = metadata
        .get("run_create_trigger_inputs")
        .map(json_value_to_display)
        .map(|s| single_line(&s, 120));
    let parent_run_id = metadata
        .get("run_create_parent_run_id")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());

    Some(RunCreateProgramDetails {
        goal,
        target_session_id,
        schedule,
        next_run_at,
        max_run,
        budget,
        execution_mode,
        trigger_capability_id,
        trigger_inputs,
        parent_run_id,
    })
}

fn format_run_create_program_summary(program: &RunCreateProgramDetails) -> String {
    format!(
        "goal=\"{}\", schedule={}, next_run_at={}, mode={}, trigger={}, max_run={}, budget={}",
        single_line(&program.goal, 60),
        program.schedule.as_deref().unwrap_or("none"),
        program.next_run_at.as_deref().unwrap_or("none"),
        program.execution_mode.as_deref().unwrap_or("llm_agent"),
        program.trigger_capability_id.as_deref().unwrap_or("none"),
        program.max_run.as_deref().unwrap_or("none"),
        program.budget.as_deref().unwrap_or("none")
    )
}

fn extract_memory_operation_details(
    metadata: &serde_json::Value,
) -> Option<MemoryOperationDetails> {
    let operation = metadata
        .get("memory_operation")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string())?;

    let key = metadata
        .get("memory_key")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());
    let store_value = metadata
        .get("memory_store_value")
        .map(json_value_to_display)
        .map(|s| single_line(&s, 120));
    let store_entry_id = metadata
        .get("memory_store_entry_id")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());
    let store_success = metadata
        .get("memory_store_success")
        .and_then(|v| v.as_bool());
    let get_found = metadata
        .get("memory_get_found")
        .and_then(|v| v.as_bool());
    let get_expired = metadata
        .get("memory_get_expired")
        .and_then(|v| v.as_bool());
    let get_value = metadata
        .get("memory_get_value")
        .map(json_value_to_display)
        .map(|s| single_line(&s, 120));

    Some(MemoryOperationDetails {
        operation,
        key,
        store_value,
        store_entry_id,
        store_success,
        get_found,
        get_expired,
        get_value,
    })
}

fn format_memory_operation_summary(memory: &MemoryOperationDetails) -> String {
    let key = memory.key.as_deref().unwrap_or("none");
    if memory.operation == "store" {
        format!(
            "store key={} value={} entry_id={} success={}",
            key,
            memory.store_value.as_deref().unwrap_or("none"),
            memory.store_entry_id.as_deref().unwrap_or("none"),
            memory
                .store_success
                .map(|v| v.to_string())
                .unwrap_or_else(|| "none".to_string())
        )
    } else {
        format!(
            "get key={} found={} expired={} value={}",
            key,
            memory
                .get_found
                .map(|v| v.to_string())
                .unwrap_or_else(|| "none".to_string()),
            memory
                .get_expired
                .map(|v| v.to_string())
                .unwrap_or_else(|| "none".to_string()),
            memory.get_value.as_deref().unwrap_or("none")
        )
    }
}

fn is_internal_step_action(action: &ActionEventDetails) -> bool {
    let action_type = action.action_type.to_ascii_lowercase();
    let function_name = action.function_name.to_ascii_lowercase();
    let summary = action.summary.to_ascii_lowercase();

    action_type.contains("internalstep")
        || action_type.contains("internal_step")
        || function_name.contains("internalstep")
        || function_name.contains("internal_step")
        || summary.contains("internalstep")
        || summary.contains("internal_step")
}

fn is_get_status_heartbeat(action: &ActionEventDetails) -> bool {
    let function_name = action.function_name.to_ascii_lowercase();
    let normalized_function = function_name.replace('_', ".");
    let metadata_capability = action
        .metadata
        .as_ref()
        .and_then(|m| m.get("capability_id"))
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_ascii_lowercase()
        .replace('_', ".");

    normalized_function == "get.status"
        || normalized_function.ends_with(".get.status")
        || normalized_function.contains("get.status")
        || metadata_capability.ends_with(".get.status")
        || metadata_capability.contains("get.status")
}

fn is_internal_step_event(event: &SystemEvent) -> bool {
    event
        .metadata
        .as_ref()
        .and_then(|m| m.get("_is_internal_step"))
        .and_then(|v| v.as_bool())
        .unwrap_or(false)
}

fn draw_ui(f: &mut Frame, state: &MonitorState) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .margin(1)
        .constraints([
            Constraint::Length(3), // Title
            Constraint::Min(10),   // Main content
            Constraint::Length(3), // Status bar
        ])
        .split(f.size());

    // Title
    let title = Paragraph::new("CCOS Gateway Monitor")
        .style(
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        )
        .alignment(Alignment::Center)
        .block(Block::default().borders(Borders::ALL));
    f.render_widget(title, chunks[0]);

    // Main content tabs
    let tab_titles = vec!["Sessions", "Agents", "Events", "LLM", "Runs"];
    let tabs = ratatui::widgets::Tabs::new(
        tab_titles
            .iter()
            .map(|t| Line::from(vec![Span::styled(*t, Style::default().fg(Color::White))]))
            .collect::<Vec<_>>(),
    )
    .select(state.selected_tab)
    .block(Block::default().borders(Borders::ALL))
    .highlight_style(
        Style::default()
            .fg(Color::Green)
            .add_modifier(Modifier::BOLD),
    );

    let main_chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(3), Constraint::Min(10)])
        .split(chunks[1]);

    f.render_widget(tabs, main_chunks[0]);

    // Tab content
    match state.selected_tab {
        0 => draw_sessions_tab(f, main_chunks[1], state),
        1 => draw_agents_tab(f, main_chunks[1], state),
        2 => draw_events_tab(f, main_chunks[1], state),
        3 => draw_llm_tab(f, main_chunks[1], state),
        4 => draw_runs_tab(f, main_chunks[1], state),
        _ => {}
    }

    if state.show_profile_selector {
        draw_profile_selector_popup(f, main_chunks[1], state);
    }

    let profile_label = state
        .active_spawn_llm_profile
        .as_deref()
        .unwrap_or("<unset>");

    // Status bar
    let status = Paragraph::new(format!(
        " [{}] Sessions: {} | Agents: {} | Spawn Profile: {} | [p] profile | {}",
        chrono::Local::now().format("%H:%M:%S"),
        state.sessions.len(),
        state.agents.len(),
        profile_label,
        state.status_message
    ))
    .style(Style::default().fg(Color::Yellow))
    .block(Block::default().borders(Borders::ALL));
    f.render_widget(status, chunks[2]);
}

fn draw_profile_selector_popup(f: &mut Frame, area: Rect, state: &MonitorState) {
    let popup_area = centered_rect(70, 70, area);
    f.render_widget(ratatui::widgets::Clear, popup_area);

    let mut lines: Vec<Line> = Vec::new();
    lines.push(Line::from(vec![Span::styled(
        "Select Gateway Spawn LLM Profile",
        Style::default()
            .fg(Color::Cyan)
            .add_modifier(Modifier::BOLD),
    )]));
    lines.push(Line::from(vec![Span::styled(
        "Enter=apply  Esc=cancel  Up/Down=navigate",
        Style::default().fg(Color::DarkGray),
    )]));
    lines.push(Line::from(""));

    let active = state
        .active_spawn_llm_profile
        .as_deref()
        .unwrap_or("<unset>")
        .to_string();
    lines.push(Line::from(vec![
        Span::styled("Current: ", Style::default().fg(Color::Yellow)),
        Span::styled(active, Style::default().fg(Color::White)),
    ]));
    lines.push(Line::from(""));

    let options = std::iter::once("<unset>")
        .chain(state.available_llm_profiles.iter().map(|p| p.as_str()));
    for (idx, option) in options.enumerate() {
        let is_selected = idx == state.selected_profile_index;
        let style = if is_selected {
            Style::default()
                .fg(Color::Black)
                .bg(Color::Cyan)
                .add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(Color::White)
        };
        lines.push(Line::from(vec![
            Span::styled(if is_selected { "▶ " } else { "  " }, style),
            Span::styled(option.to_string(), style),
        ]));
    }

    let paragraph = Paragraph::new(lines)
        .block(
            Block::default()
                .title(" LLM Profile ")
                .borders(Borders::ALL)
                .border_style(Style::default().fg(Color::Cyan)),
        )
        .wrap(Wrap { trim: false });
    f.render_widget(paragraph, popup_area);
}

fn draw_sessions_tab(f: &mut Frame, area: Rect, state: &MonitorState) {
    let header = Row::new(vec!["Session ID", "Status", "Agent PID", "Step", "Memory"]).style(
        Style::default()
            .fg(Color::Yellow)
            .add_modifier(Modifier::BOLD),
    );

    let rows: Vec<Row> = state
        .sessions
        .values()
        .map(|session| {
            let status_color = match session.status.as_str() {
                "Active" => Color::Green,
                "Idle" => Color::Yellow,
                "Terminated" => Color::Red,
                _ => Color::Gray,
            };

            Row::new(vec![
                Cell::from(session.session_id.clone()),
                Cell::from(session.status.clone()).style(Style::default().fg(status_color)),
                Cell::from(session.agent_pid.map(|p| p.to_string()).unwrap_or_default()),
                Cell::from(
                    session
                        .current_step
                        .map(|s| s.to_string())
                        .unwrap_or_default(),
                ),
                Cell::from(
                    session
                        .memory_mb
                        .map(|m| format!("{} MB", m))
                        .unwrap_or_default(),
                ),
            ])
        })
        .collect();

    let widths = [
        Constraint::Ratio(2, 5),
        Constraint::Ratio(1, 5),
        Constraint::Ratio(1, 5),
        Constraint::Ratio(1, 5),
        Constraint::Ratio(1, 5),
    ];
    let table = Table::new(rows, widths)
        .header(header)
        .block(Block::default().title("Sessions").borders(Borders::ALL));

    f.render_widget(table, area);
}

fn draw_agents_tab(f: &mut Frame, area: Rect, state: &MonitorState) {
    let header = Row::new(vec!["Session", "PID", "Step", "Memory", "Health"]).style(
        Style::default()
            .fg(Color::Yellow)
            .add_modifier(Modifier::BOLD),
    );

    let selected_session_id = state.get_selected_session_id();

    let rows: Vec<Row> = state
        .agent_session_ids
        .iter()
        .enumerate()
        .map(|(_idx, session_id)| {
            let agent = state.agents.get(session_id);
            let is_selected = selected_session_id == Some(session_id.as_str());

            let health_icon = if let Some(a) = agent {
                if a.is_healthy {
                    "🟢 Healthy"
                } else {
                    "🔴 Unhealthy"
                }
            } else {
                "❓ Unknown"
            };
            let health_color = if let Some(a) = agent {
                if a.is_healthy {
                    Color::Green
                } else {
                    Color::Red
                }
            } else {
                Color::Gray
            };

            let row_style = if is_selected {
                Style::default()
                    .fg(Color::Black)
                    .bg(Color::Cyan)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default()
            };

            let row = Row::new(vec![
                Cell::from(session_id.clone()),
                Cell::from(agent.map(|a| a.pid.to_string()).unwrap_or_default()),
                Cell::from(
                    agent
                        .map(|a| a.current_step.to_string())
                        .unwrap_or_default(),
                ),
                Cell::from(
                    agent
                        .and_then(|a| a.memory_mb.map(|m| format!("{} MB", m)))
                        .unwrap_or_default(),
                ),
                Cell::from(health_icon).style(Style::default().fg(health_color)),
            ])
            .style(row_style);

            row
        })
        .collect();

    let widths = [
        Constraint::Ratio(2, 5),
        Constraint::Ratio(1, 5),
        Constraint::Ratio(1, 5),
        Constraint::Ratio(1, 5),
        Constraint::Ratio(1, 5),
    ];

    let title = if selected_session_id.is_some() {
        format!("Agents (selected: {})", state.selected_agent_index + 1)
    } else {
        "Agents (use ↑↓ to select)".to_string()
    };

    let table = Table::new(rows, widths)
        .header(header)
        .block(Block::default().title(title).borders(Borders::ALL));

    f.render_widget(table, area);
}

fn draw_runs_tab(f: &mut Frame, area: Rect, state: &MonitorState) {
    let selected_session = state.get_runs_session_id().map(|s| s.to_string());

    let Some(session_id) = selected_session else {
        let placeholder = Paragraph::new(
            "No sessions available. Connect a session first.",
        )
        .style(Style::default().fg(Color::DarkGray))
        .block(Block::default().title("Runs").borders(Borders::ALL))
        .wrap(Wrap { trim: true });
        f.render_widget(placeholder, area);
        return;
    };

    let loaded_for = state.runs_session_id.as_deref().unwrap_or("<none>");
    let session_nav = if state.ordered_session_ids.len() > 1 {
        format!(
            "  [s] session {}/{}",
            state.selected_runs_session_idx + 1,
            state.ordered_session_ids.len()
        )
    } else {
        String::new()
    };
    let title = format!(
        "Runs for {} (loaded: {}) · [r] refresh  [c] cancel  [↑↓] navigate{}",
        session_id, loaded_for, session_nav
    );

    if state.display_items.is_empty() {
        let placeholder = Paragraph::new("No runs. Press [r] to refresh.")
            .style(Style::default().fg(Color::DarkGray))
            .block(Block::default().title(title).borders(Borders::ALL));
        f.render_widget(placeholder, area);
        return;
    }

    let items: Vec<ListItem> = state
        .display_items
        .iter()
        .map(|item| match item {
            RunDisplayItem::ScheduleGroup {
                goal,
                schedule,
                instance_count,
                next_run_at,
                pending_run_id,
                ..
            } => {
                let next_str = next_run_at
                    .as_deref()
                    .map(|s| {
                        // Trim to short form: keep only time part of RFC3339 for compactness
                        s.get(11..19).unwrap_or(s)
                    })
                    .unwrap_or("-");
                let cancel_hint = if pending_run_id.is_some() {
                    " [c=cancel]"
                } else {
                    " [completed]"
                };
                let line = Line::from(vec![
                    Span::styled("📅 ", Style::default().fg(Color::Yellow)),
                    Span::styled(
                        single_line(goal, 40),
                        Style::default()
                            .fg(Color::White)
                            .add_modifier(Modifier::BOLD),
                    ),
                    Span::styled(
                        format!("  {}  ", schedule),
                        Style::default().fg(Color::Cyan),
                    ),
                    Span::styled(
                        format!("[{} runs]", instance_count),
                        Style::default().fg(Color::DarkGray),
                    ),
                    Span::styled(
                        format!("  next: {}", next_str),
                        Style::default().fg(Color::Green),
                    ),
                    Span::styled(
                        cancel_hint.to_string(),
                        Style::default().fg(Color::DarkGray),
                    ),
                ]);
                ListItem::new(line)
            }
            RunDisplayItem::RunInstance {
                run,
                grouped,
                is_last_in_group,
            } => {
                let prefix = if *grouped {
                    if *is_last_in_group {
                        "  └─ "
                    } else {
                        "  ├─ "
                    }
                } else {
                    ""
                };
                let state_color = if run.state.contains("Active") {
                    Color::Green
                } else if run.state.contains("Done") {
                    Color::DarkGray
                } else if run.state.contains("Cancelled") || run.state.contains("Failed") {
                    Color::Red
                } else if run.state.contains("Scheduled") {
                    Color::Cyan
                } else if run.state.contains("Paused") {
                    Color::Yellow
                } else {
                    Color::White
                };
                let short_id = run.run_id.get(4..12).unwrap_or(&run.run_id);
                let mut spans = vec![
                    Span::styled(prefix.to_string(), Style::default().fg(Color::DarkGray)),
                    Span::styled(
                        short_id.to_string(),
                        Style::default().fg(Color::White),
                    ),
                    Span::raw("  "),
                    Span::styled(
                        format!("{:<20}", &run.state),
                        Style::default().fg(state_color),
                    ),
                    Span::styled(
                        format!("  {:>3}steps  {}s", run.steps_taken, run.elapsed_secs),
                        Style::default().fg(Color::DarkGray),
                    ),
                ];
                if !*grouped {
                    spans.push(Span::raw("  "));
                    spans.push(Span::styled(
                        single_line(&run.goal, 40),
                        Style::default().fg(Color::White),
                    ));
                }
                ListItem::new(Line::from(spans))
            }
        })
        .collect();

    let viewport_height = area.height.saturating_sub(2) as usize;
    let scroll_offset = state
        .selected_run_index
        .saturating_sub(viewport_height / 2)
        .min(state.display_items.len().saturating_sub(viewport_height));

    let mut list_state = ListState::default();
    list_state.select(Some(state.selected_run_index));

    // ListState doesn't have direct offset control in all ratatui versions;
    // clip the items to the viewport manually for correct scrolling.
    let visible_items: Vec<ListItem> = items
        .into_iter()
        .skip(scroll_offset)
        .take(viewport_height + 1)
        .collect();
    let adjusted_selected = state.selected_run_index.saturating_sub(scroll_offset);
    let mut visible_state = ListState::default();
    visible_state.select(Some(adjusted_selected));

    let list = List::new(visible_items)
        .block(Block::default().title(title).borders(Borders::ALL))
        .highlight_style(
            Style::default()
                .fg(Color::Black)
                .bg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        )
        .highlight_symbol("▶ ");

    f.render_stateful_widget(list, area, &mut visible_state);
}

fn draw_events_tab(f: &mut Frame, area: Rect, state: &MonitorState) {
    let filtered_indices = state.get_filtered_event_indices();
    let filtered_count = filtered_indices.len();
    let viewport_height = area.height.saturating_sub(2) as usize;
    let (selected_display_idx, scroll_offset) =
        compute_events_viewport(filtered_count, state.selected_event_index, viewport_height);

    let text: Vec<Line> = filtered_indices
        .iter()
        .rev()
        .enumerate()
        .map(|(display_idx, &event_idx)| {
            let event = &state.events[event_idx];
            let elapsed = event.timestamp.elapsed().as_secs();
            let time_str = if elapsed < 60 {
                format!("{}s ago", elapsed)
            } else {
                format!("{}m ago", elapsed / 60)
            };

            let color = match event.event_type.as_str() {
                "CRASH" => Color::Red,
                "ACTION" => Color::Cyan,
                "STATE" => Color::Green,
                "LLM" => Color::Magenta,
                _ => Color::Gray,
            };

            // Calculate the actual selected index in reversed list
            let is_selected = display_idx == selected_display_idx;

            let base_style = if is_selected {
                Style::default()
                    .fg(Color::Black)
                    .bg(Color::Cyan)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default()
            };

            let prefix = format!("[{}] [{}] ", time_str, event.event_type);
            let max_detail_chars = area.width.saturating_sub(prefix.len() as u16 + 4) as usize;
            let detail_text = single_line(
                &format!("{}: {}", event.session_id, event.details),
                max_detail_chars.max(20),
            );

            Line::from(vec![
                Span::styled(
                    format!("[{}] ", time_str),
                    if is_selected {
                        base_style
                    } else {
                        Style::default().fg(Color::DarkGray)
                    },
                ),
                Span::styled(
                    format!("[{}] ", event.event_type),
                    if is_selected {
                        base_style
                    } else {
                        Style::default().fg(color)
                    },
                ),
                Span::styled(detail_text, base_style),
            ])
        })
        .collect();

    let title = if state.get_selected_session_id().is_some() {
        format!(
            "Recent Events (filtered: {}) - ↑↓ PgUp/PgDn Home/End Enter | i: {}",
            filtered_count,
            if state.show_internal_steps {
                "hide internal"
            } else {
                "show internal"
            }
        )
    } else {
        format!(
            "Recent Events ({}) - ↑↓ PgUp/PgDn Home/End Enter | i: {}",
            filtered_count,
            if state.show_internal_steps {
                "hide internal"
            } else {
                "show internal"
            }
        )
    };

    let paragraph = Paragraph::new(text)
        .block(Block::default().title(title).borders(Borders::ALL))
        .scroll((scroll_offset as u16, 0));

    f.render_widget(paragraph, area);

    // Draw event detail popup if enabled
    if state.show_event_detail {
        if let Some(selected_event) = state.get_selected_event() {
            draw_event_detail_popup(f, area, selected_event, state.detail_scroll);
        }
    }
}

/// Build a plain-text copy of an event's full details for writing to file.
fn build_copy_text(event: &SystemEvent) -> String {
    let mut out = String::new();
    out.push_str(&format!("Type:    {}\n", event.event_type));
    out.push_str(&format!("Session: {}\n", event.session_id));
    out.push_str(&format!("Details: {}\n", event.details));
    if let Some(ref fd) = event.full_details {
        out.push_str("\n--- Full Details ---\n");
        out.push_str(fd);
        out.push('\n');
    }
    if let Some(ref ce) = event.code_execution {
        out.push_str("\n--- Code Execution ---\n");
        out.push_str(&format!("Language: {}\n", ce.language));
        if let Some(ec) = ce.exit_code { out.push_str(&format!("Exit Code: {}\n", ec)); }
        if let Some(ms) = ce.duration_ms { out.push_str(&format!("Duration: {}ms\n", ms)); }
        if !ce.code.is_empty() {
            out.push_str("\nCode:\n");
            out.push_str(&ce.code);
            out.push('\n');
        }
        if !ce.stdout.is_empty() {
            out.push_str("\nStdout:\n");
            out.push_str(&ce.stdout);
            out.push('\n');
        }
        if !ce.stderr.is_empty() {
            out.push_str("\nStderr:\n");
            out.push_str(&ce.stderr);
            out.push('\n');
        }
    }
    if let Some(ref meta) = event.metadata {
        out.push_str("\n--- Metadata ---\n");
        if let Ok(s) = serde_json::to_string_pretty(meta) {
            out.push_str(&s);
            out.push('\n');
        }
    }
    out
}

/// Draw a popup showing detailed event information
fn draw_event_detail_popup(f: &mut Frame, area: Rect, event: &SystemEvent, scroll: u16) {
    // Calculate popup size (centered, 80% width, 80% height)
    let popup_area = centered_rect(80, 80, area);

    // Clear the area first
    f.render_widget(ratatui::widgets::Clear, popup_area);

    let mut lines: Vec<Line> = Vec::new();

    // Header
    lines.push(Line::from(vec![Span::styled(
        "Event Details",
        Style::default()
            .fg(Color::Cyan)
            .add_modifier(Modifier::BOLD),
    )]));
    lines.push(Line::from(""));

    // Basic info
    let elapsed = event.timestamp.elapsed().as_secs();
    let time_str = if elapsed < 60 {
        format!("{} seconds ago", elapsed)
    } else if elapsed < 3600 {
        format!("{} minutes ago", elapsed / 60)
    } else {
        format!("{} hours ago", elapsed / 3600)
    };

    lines.push(Line::from(vec![
        Span::styled("Type: ", Style::default().fg(Color::Yellow)),
        Span::styled(&event.event_type, Style::default().fg(Color::White)),
    ]));
    lines.push(Line::from(vec![
        Span::styled("Session: ", Style::default().fg(Color::Yellow)),
        Span::styled(&event.session_id, Style::default().fg(Color::White)),
    ]));
    lines.push(Line::from(vec![
        Span::styled("Time: ", Style::default().fg(Color::Yellow)),
        Span::styled(&time_str, Style::default().fg(Color::White)),
    ]));
    lines.push(Line::from(""));

    // Full details if available
    if let Some(ref full_details) = event.full_details {
        lines.push(Line::from(vec![Span::styled(
            "Details:",
            Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::BOLD),
        )]));
        for line in full_details.lines() {
            lines.push(Line::from(vec![
                Span::styled("  ", Style::default()),
                Span::styled(line, Style::default().fg(Color::White)),
            ]));
        }
        lines.push(Line::from(""));
    }

    // Code execution details
    if let Some(ref code_exec) = event.code_execution {
        lines.push(Line::from(vec![Span::styled(
            "━━━ Code Execution ━━━",
            Style::default()
                .fg(Color::Green)
                .add_modifier(Modifier::BOLD),
        )]));
        lines.push(Line::from(""));

        // Language
        lines.push(Line::from(vec![
            Span::styled("Language: ", Style::default().fg(Color::Yellow)),
            Span::styled(&code_exec.language, Style::default().fg(Color::Cyan)),
        ]));

        // Duration
        if let Some(ms) = code_exec.duration_ms {
            lines.push(Line::from(vec![
                Span::styled("Duration: ", Style::default().fg(Color::Yellow)),
                Span::styled(format!("{}ms", ms), Style::default().fg(Color::White)),
            ]));
        }

        // Exit code
        if let Some(code) = code_exec.exit_code {
            let color = if code == 0 { Color::Green } else { Color::Red };
            lines.push(Line::from(vec![
                Span::styled("Exit Code: ", Style::default().fg(Color::Yellow)),
                Span::styled(code.to_string(), Style::default().fg(color)),
            ]));
        }
        lines.push(Line::from(""));

        // Code
        if !code_exec.code.is_empty() {
            lines.push(Line::from(vec![Span::styled(
                "Code:",
                Style::default()
                    .fg(Color::Magenta)
                    .add_modifier(Modifier::BOLD),
            )]));
            for line in code_exec.code.lines().take(20) {
                lines.push(Line::from(vec![
                    Span::styled("  │ ", Style::default().fg(Color::DarkGray)),
                    Span::styled(line, Style::default().fg(Color::White)),
                ]));
            }
            if code_exec.code.lines().count() > 20 {
                lines.push(Line::from(vec![Span::styled(
                    "  ... (truncated)",
                    Style::default().fg(Color::DarkGray),
                )]));
            }
            lines.push(Line::from(""));
        }

        // Stdout
        if !code_exec.stdout.is_empty() {
            lines.push(Line::from(vec![Span::styled(
                "Stdout:",
                Style::default()
                    .fg(Color::Green)
                    .add_modifier(Modifier::BOLD),
            )]));
            for line in code_exec.stdout.lines().take(10) {
                lines.push(Line::from(vec![
                    Span::styled("  │ ", Style::default().fg(Color::DarkGray)),
                    Span::styled(line, Style::default().fg(Color::Green)),
                ]));
            }
            if code_exec.stdout.lines().count() > 10 {
                lines.push(Line::from(vec![Span::styled(
                    "  ... (truncated)",
                    Style::default().fg(Color::DarkGray),
                )]));
            }
            lines.push(Line::from(""));
        }

        // Stderr
        if !code_exec.stderr.is_empty() {
            lines.push(Line::from(vec![Span::styled(
                "Stderr:",
                Style::default().fg(Color::Red).add_modifier(Modifier::BOLD),
            )]));
            for line in code_exec.stderr.lines().take(10) {
                lines.push(Line::from(vec![
                    Span::styled("  │ ", Style::default().fg(Color::DarkGray)),
                    Span::styled(line, Style::default().fg(Color::Red)),
                ]));
            }
            if code_exec.stderr.lines().count() > 10 {
                lines.push(Line::from(vec![Span::styled(
                    "  ... (truncated)",
                    Style::default().fg(Color::DarkGray),
                )]));
            }
            lines.push(Line::from(""));
        }
    }

    if let Some(ref metadata) = event.metadata {
        if let Some(program) = extract_run_create_program_details(metadata) {
            lines.push(Line::from(vec![Span::styled(
                "━━━ Programmed Run ━━━",
                Style::default()
                    .fg(Color::Cyan)
                    .add_modifier(Modifier::BOLD),
            )]));
            lines.push(Line::from(vec![
                Span::styled("Goal: ", Style::default().fg(Color::Yellow)),
                Span::styled(program.goal, Style::default().fg(Color::White)),
            ]));
            if let Some(session_id) = program.target_session_id {
                lines.push(Line::from(vec![
                    Span::styled("Target Session: ", Style::default().fg(Color::Yellow)),
                    Span::styled(session_id, Style::default().fg(Color::White)),
                ]));
            }
            lines.push(Line::from(vec![
                Span::styled("Schedule: ", Style::default().fg(Color::Yellow)),
                Span::styled(
                    program
                        .schedule
                        .unwrap_or_else(|| "none".to_string()),
                    Style::default().fg(Color::White),
                ),
            ]));
            lines.push(Line::from(vec![
                Span::styled("Next Run At: ", Style::default().fg(Color::Yellow)),
                Span::styled(
                    program
                        .next_run_at
                        .unwrap_or_else(|| "none".to_string()),
                    Style::default().fg(Color::White),
                ),
            ]));
            lines.push(Line::from(vec![
                Span::styled("Max Runs: ", Style::default().fg(Color::Yellow)),
                Span::styled(
                    program.max_run.unwrap_or_else(|| "none".to_string()),
                    Style::default().fg(Color::White),
                ),
            ]));
            lines.push(Line::from(vec![
                Span::styled("Budget: ", Style::default().fg(Color::Yellow)),
                Span::styled(
                    program.budget.unwrap_or_else(|| "none".to_string()),
                    Style::default().fg(Color::White),
                ),
            ]));
            lines.push(Line::from(vec![
                Span::styled("Execution Mode: ", Style::default().fg(Color::Yellow)),
                Span::styled(
                    program
                        .execution_mode
                        .unwrap_or_else(|| "llm_agent".to_string()),
                    Style::default().fg(Color::White),
                ),
            ]));
            lines.push(Line::from(vec![
                Span::styled("Trigger Capability: ", Style::default().fg(Color::Yellow)),
                Span::styled(
                    program
                        .trigger_capability_id
                        .unwrap_or_else(|| "none".to_string()),
                    Style::default().fg(Color::White),
                ),
            ]));
            if let Some(trigger_inputs) = program.trigger_inputs {
                lines.push(Line::from(vec![
                    Span::styled("Trigger Inputs: ", Style::default().fg(Color::Yellow)),
                    Span::styled(trigger_inputs, Style::default().fg(Color::White)),
                ]));
            }
            if let Some(parent_run_id) = program.parent_run_id {
                lines.push(Line::from(vec![
                    Span::styled("Parent Run: ", Style::default().fg(Color::Yellow)),
                    Span::styled(parent_run_id, Style::default().fg(Color::White)),
                ]));
            }
            lines.push(Line::from(""));
        }

        if let Some(memory) = extract_memory_operation_details(metadata) {
            lines.push(Line::from(vec![Span::styled(
                "━━━ Memory Operation ━━━",
                Style::default()
                    .fg(Color::Cyan)
                    .add_modifier(Modifier::BOLD),
            )]));
            lines.push(Line::from(vec![
                Span::styled("Type: ", Style::default().fg(Color::Yellow)),
                Span::styled(memory.operation.clone(), Style::default().fg(Color::White)),
            ]));
            lines.push(Line::from(vec![
                Span::styled("Key: ", Style::default().fg(Color::Yellow)),
                Span::styled(
                    memory.key.unwrap_or_else(|| "none".to_string()),
                    Style::default().fg(Color::White),
                ),
            ]));
            if memory.operation == "store" {
                lines.push(Line::from(vec![
                    Span::styled("Value Stored: ", Style::default().fg(Color::Yellow)),
                    Span::styled(
                        memory
                            .store_value
                            .unwrap_or_else(|| "none".to_string()),
                        Style::default().fg(Color::White),
                    ),
                ]));
                lines.push(Line::from(vec![
                    Span::styled("Entry ID: ", Style::default().fg(Color::Yellow)),
                    Span::styled(
                        memory
                            .store_entry_id
                            .unwrap_or_else(|| "none".to_string()),
                        Style::default().fg(Color::White),
                    ),
                ]));
                lines.push(Line::from(vec![
                    Span::styled("Store Success: ", Style::default().fg(Color::Yellow)),
                    Span::styled(
                        memory
                            .store_success
                            .map(|v| v.to_string())
                            .unwrap_or_else(|| "none".to_string()),
                        Style::default().fg(Color::White),
                    ),
                ]));
            } else {
                lines.push(Line::from(vec![
                    Span::styled("Found: ", Style::default().fg(Color::Yellow)),
                    Span::styled(
                        memory
                            .get_found
                            .map(|v| v.to_string())
                            .unwrap_or_else(|| "none".to_string()),
                        Style::default().fg(Color::White),
                    ),
                ]));
                lines.push(Line::from(vec![
                    Span::styled("Expired: ", Style::default().fg(Color::Yellow)),
                    Span::styled(
                        memory
                            .get_expired
                            .map(|v| v.to_string())
                            .unwrap_or_else(|| "none".to_string()),
                        Style::default().fg(Color::White),
                    ),
                ]));
                lines.push(Line::from(vec![
                    Span::styled("Value Retrieved: ", Style::default().fg(Color::Yellow)),
                    Span::styled(
                        memory.get_value.unwrap_or_else(|| "none".to_string()),
                        Style::default().fg(Color::White),
                    ),
                ]));
            }
            lines.push(Line::from(""));
        }
    }

    // Metadata if available
    if let Some(ref metadata) = event.metadata {
        // Result payload (if available) - show this first for quick inspection
        if let Some(result) = metadata.get("result") {
            lines.push(Line::from(vec![Span::styled(
                "━━━ Result ━━━",
                Style::default()
                    .fg(Color::LightGreen)
                    .add_modifier(Modifier::BOLD),
            )]));
            if let Ok(result_str) = serde_json::to_string_pretty(result) {
                let result_lines: Vec<&str> = result_str.lines().take(20).collect();
                let total_lines = result_str.lines().count();
                for line in result_lines {
                    lines.push(Line::from(vec![
                        Span::styled("  ", Style::default()),
                        Span::styled(line.to_string(), Style::default().fg(Color::LightGreen)),
                    ]));
                }
                if total_lines > 20 {
                    lines.push(Line::from(vec![Span::styled(
                        "  ... (truncated)",
                        Style::default().fg(Color::DarkGray),
                    )]));
                }
            } else {
                lines.push(Line::from(vec![
                    Span::styled("  ", Style::default()),
                    Span::styled("<unrenderable result>", Style::default().fg(Color::DarkGray)),
                ]));
            }
            lines.push(Line::from(""));
        }

        lines.push(Line::from(vec![Span::styled(
            "━━━ Metadata ━━━",
            Style::default()
                .fg(Color::Blue)
                .add_modifier(Modifier::BOLD),
        )]));
        if let Ok(json_str) = serde_json::to_string_pretty(metadata) {
            let json_lines: Vec<&str> = json_str.lines().take(15).collect();
            let total_lines = json_str.lines().count();
            for line in json_lines {
                lines.push(Line::from(vec![
                    Span::styled("  ", Style::default()),
                    Span::styled(line.to_string(), Style::default().fg(Color::Blue)),
                ]));
            }
            if total_lines > 15 {
                lines.push(Line::from(vec![Span::styled(
                    "  ... (truncated)",
                    Style::default().fg(Color::DarkGray),
                )]));
            }
        }
    }

    // Footer
    lines.push(Line::from(""));
    lines.push(Line::from(vec![
        Span::styled("Press ", Style::default().fg(Color::DarkGray)),
        Span::styled(
            "Esc",
            Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled(" to close  ", Style::default().fg(Color::DarkGray)),
        Span::styled(
            "↑↓/PgUp/PgDn",
            Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled(" to scroll  ", Style::default().fg(Color::DarkGray)),
        Span::styled(
            "y",
            Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled(" to copy to /tmp/ccos-monitor-copy.txt", Style::default().fg(Color::DarkGray)),
    ]));

    let paragraph = Paragraph::new(lines)
        .block(
            Block::default()
                .title(" Event Detail ")
                .title_style(
                    Style::default()
                        .fg(Color::Cyan)
                        .add_modifier(Modifier::BOLD),
                )
                .borders(Borders::ALL)
                .border_style(Style::default().fg(Color::Cyan)),
        )
        .wrap(Wrap { trim: false })
        .scroll((scroll, 0));

    f.render_widget(paragraph, popup_area);
}

/// Helper function to create a centered rectangle
fn centered_rect(percent_x: u16, percent_y: u16, r: Rect) -> Rect {
    let popup_layout = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Percentage((100 - percent_y) / 2),
            Constraint::Percentage(percent_y),
            Constraint::Percentage((100 - percent_y) / 2),
        ])
        .split(r);

    Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage((100 - percent_x) / 2),
            Constraint::Percentage(percent_x),
            Constraint::Percentage((100 - percent_x) / 2),
        ])
        .split(popup_layout[1])[1]
}

fn draw_llm_tab(f: &mut Frame, area: Rect, state: &MonitorState) {
    let filtered_consultations = state.get_filtered_llm_consultations();

    if filtered_consultations.is_empty() {
        let message = if state.llm_consultations.is_empty() {
            "No LLM consultations yet.\n\nAgent LLM consultations will appear here when the agent is running in autonomous mode."
        } else {
            "No LLM consultations for selected agent.\n\nSelect an agent in the Agents tab to filter consultations."
        };
        let empty = Paragraph::new(message)
            .style(Style::default().fg(Color::DarkGray))
            .block(
                Block::default()
                    .title("LLM Consultations")
                    .borders(Borders::ALL),
            )
            .wrap(Wrap { trim: true });
        f.render_widget(empty, area);
        return;
    }

    let text: Vec<Line> = filtered_consultations
        .iter()
        .rev()
        .flat_map(|(session_id, consultation)| {
            let complete_icon = if consultation.task_complete {
                "✓"
            } else {
                "→"
            };
            let complete_color = if consultation.task_complete {
                Color::Green
            } else {
                Color::Yellow
            };
            let init_str = if consultation.is_initial {
                "initial"
            } else {
                "follow-up"
            };
            let model_str = consultation.model.as_deref().unwrap_or("unknown");
            let caps_str = if consultation.planned_capabilities.is_empty() {
                "none".to_string()
            } else {
                consultation.planned_capabilities.join(", ")
            };

            // Truncate understanding and reasoning if too long
            let understanding: String = if consultation.understanding.len() > 100 {
                format!("{}...", &consultation.understanding[..97])
            } else {
                consultation.understanding.clone()
            };
            let reasoning: String = if consultation.reasoning.len() > 100 {
                format!("{}...", &consultation.reasoning[..97])
            } else {
                consultation.reasoning.clone()
            };
            let prompt_preview = consultation.prompt.as_ref().map(|p| {
                if p.len() > 100 {
                    format!("{}...", &p[..97])
                } else {
                    p.clone()
                }
            });
            let response_preview = consultation.response.as_ref().map(|r| {
                if r.len() > 100 {
                    format!("{}...", &r[..97])
                } else {
                    r.clone()
                }
            });

            let mut lines = vec![
                Line::from(vec![
                    Span::styled(
                        format!("[Iter {}] ", consultation.iteration),
                        Style::default().fg(Color::Cyan),
                    ),
                    Span::styled(
                        format!("[{}] ", complete_icon),
                        Style::default().fg(complete_color),
                    ),
                    Span::styled(
                        format!("[{}] ", init_str),
                        Style::default().fg(Color::DarkGray),
                    ),
                    Span::styled(session_id.clone(), Style::default().fg(Color::White)),
                ]),
                Line::from(vec![
                    Span::styled("  Model: ", Style::default().fg(Color::DarkGray)),
                    Span::styled(model_str.to_string(), Style::default().fg(Color::White)),
                ]),
                Line::from(vec![
                    Span::styled("  Understanding: ", Style::default().fg(Color::DarkGray)),
                    Span::styled(understanding, Style::default().fg(Color::White)),
                ]),
                Line::from(vec![
                    Span::styled("  Reasoning: ", Style::default().fg(Color::DarkGray)),
                    Span::styled(reasoning, Style::default().fg(Color::White)),
                ]),
                Line::from(vec![
                    Span::styled("  Planned: ", Style::default().fg(Color::DarkGray)),
                    Span::styled(caps_str, Style::default().fg(Color::Cyan)),
                ]),
            ];
            if let Some(ref p) = prompt_preview {
                lines.push(Line::from(vec![
                    Span::styled("  Prompt: ", Style::default().fg(Color::DarkGray)),
                    Span::styled(p.clone(), Style::default().fg(Color::White)),
                ]));
            }
            if let Some(ref r) = response_preview {
                lines.push(Line::from(vec![
                    Span::styled("  Response: ", Style::default().fg(Color::DarkGray)),
                    Span::styled(r.clone(), Style::default().fg(Color::White)),
                ]));
            }
            if let Some(ref usage) = consultation.token_usage {
                lines.push(Line::from(vec![
                    Span::styled("  Tokens: ", Style::default().fg(Color::DarkGray)),
                    Span::styled(
                        format!(
                            "prompt={} completion={} total={}",
                            usage.prompt_tokens, usage.completion_tokens, usage.total_tokens
                        ),
                        Style::default().fg(Color::White),
                    ),
                ]));
            }
            lines.push(Line::from(""));
            lines
        })
        .collect();

    let title = if state.get_selected_session_id().is_some() {
        format!(
            "LLM Consultations (filtered: {}/{})",
            filtered_consultations.len(),
            state.llm_consultations.len()
        )
    } else {
        format!("LLM Consultations ({})", state.llm_consultations.len())
    };

    let paragraph = Paragraph::new(text)
        .block(Block::default().title(title).borders(Borders::ALL))
        .wrap(Wrap { trim: true });

    f.render_widget(paragraph, area);
}
