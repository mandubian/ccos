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
    widgets::{Block, Borders, Cell, Paragraph, Row, Table, Wrap},
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
    /// Selected event index in Events tab
    selected_event_index: usize,
    /// Stable selected event ID (prevents selection drift when new events arrive)
    selected_event_id: Option<u64>,
    /// Monotonic event ID counter
    next_event_id: u64,
    /// Whether event detail popup is shown
    show_event_detail: bool,
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
            events: Vec::new(),
            llm_consultations: Vec::new(),
            selected_tab: 0,
            selected_agent_index: 0,
            selected_event_index: 0,
            selected_event_id: None,
            next_event_id: 1,
            show_event_detail: false,
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
        match fetch_gateway_llm_profile(&client, &args.gateway_url, &args.token).await {
            Ok(profile) => profile,
            Err(e) => {
                warn!(
                    "Failed to fetch current gateway spawn profile (continuing): {}",
                    e
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

                            // Handle Escape to close detail popup first
                            if state.show_event_detail {
                                if key.code == KeyCode::Esc {
                                    state.show_event_detail = false;
                                }
                                // Consume all other keys while popup is open
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
                                KeyCode::Tab => {
                                    state.selected_tab = (state.selected_tab + 1) % 4;
                                }
                                KeyCode::BackTab => {
                                    state.selected_tab = if state.selected_tab == 0 {
                                        3
                                    } else {
                                        state.selected_tab - 1
                                    };
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

                    // Ensure selected_event_index remains valid for current Events filter
                    let filtered_count = state.get_filtered_event_indices().len();
                    if filtered_count == 0 {
                        state.selected_event_index = 0;
                    } else if state.selected_event_index >= filtered_count {
                        state.selected_event_index = filtered_count - 1;
                    }

                    state.status_message = format!("Updated {} sessions", state.sessions.len());
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
    let tab_titles = vec!["Sessions", "Agents", "Events", "LLM"];
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
            draw_event_detail_popup(f, area, selected_event);
        }
    }
}

/// Draw a popup showing detailed event information
fn draw_event_detail_popup(f: &mut Frame, area: Rect, event: &SystemEvent) {
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
        Span::styled(" to close", Style::default().fg(Color::DarkGray)),
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
        .wrap(Wrap { trim: false });

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
