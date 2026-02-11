//! CCOS Gateway Monitor
//!
//! Real-time monitoring TUI for the CCOS Gateway.
//! Shows connected sessions, spawned agents, and system events.

use clap::Parser;
use crossterm::{
    event::{self, DisableMouseCapture, EnableMouseCapture, Event, KeyCode},
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
use std::collections::HashMap;
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
struct LlmConsultation {
    iteration: u32,
    is_initial: bool,
    understanding: String,
    reasoning: String,
    task_complete: bool,
    planned_capabilities: Vec<String>,
    model: Option<String>,
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
    /// Whether event detail popup is shown
    show_event_detail: bool,
    #[allow(dead_code)]
    last_refresh: Instant,
    should_quit: bool,
    status_message: String,
}

impl MonitorState {
    fn new() -> Self {
        Self {
            sessions: HashMap::new(),
            agents: HashMap::new(),
            agent_session_ids: Vec::new(),
            events: Vec::new(),
            llm_consultations: Vec::new(),
            selected_tab: 0,
            selected_agent_index: 0,
            selected_event_index: 0,
            show_event_detail: false,
            last_refresh: Instant::now(),
            should_quit: false,
            status_message: "Connected to gateway".to_string(),
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
        if let Some(selected_id) = self.get_selected_session_id() {
            self.events
                .iter()
                .enumerate()
                .filter(|(_, e)| e.session_id == selected_id)
                .map(|(i, _)| i)
                .collect()
        } else {
            self.events.iter().enumerate().map(|(i, _)| i).collect()
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
        self.events.push(SystemEvent {
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

    /// Get the currently selected event (if any)
    fn get_selected_event(&self) -> Option<&SystemEvent> {
        let filtered_indices = self.get_filtered_event_indices();
        filtered_indices
            .get(self.selected_event_index)
            .and_then(|&idx| self.events.get(idx))
    }

    fn add_llm_consultation(&mut self, session_id: String, consultation: LlmConsultation) {
        // Also add to events for the Events tab
        let caps = if consultation.planned_capabilities.is_empty() {
            "none".to_string()
        } else {
            consultation.planned_capabilities.join(", ")
        };
        let details = format!(
            "iter={} | {} | complete={} | caps=[{}]",
            consultation.iteration,
            if consultation.is_initial {
                "initial"
            } else {
                "follow-up"
            },
            consultation.task_complete,
            caps
        );
        self.add_event("LLM", &session_id, &details);

        // Store full consultation for LLM tab
        self.llm_consultations.push((session_id, consultation));

        // Keep only last 50 consultations
        if self.llm_consultations.len() > 50 {
            self.llm_consultations.remove(0);
        }
    }
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
                if let Event::Key(key) = event::read().expect("read failed") {
                    if tx_input
                        .send(MonitorEvent::Input(Event::Key(key)))
                        .await
                        .is_err()
                    {
                        break;
                    }
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
    let mut state = MonitorState::new();

    loop {
        // Draw UI
        terminal.draw(|f| draw_ui(f, &state))?;

        // Handle events
        if let Some(event) = rx.recv().await {
            match event {
                MonitorEvent::Input(event) => {
                    if let Event::Key(key) = event {
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
                                    // Navigate events in Events tab
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
                                    // Navigate events in Events tab
                                    let filtered_count = state.get_filtered_event_indices().len();
                                    if filtered_count > 0 {
                                        state.selected_event_index = (state.selected_event_index + 1)
                                            % filtered_count;
                                    }
                                }
                            }
                            KeyCode::Enter => {
                                // Show event detail in Events tab
                                if state.selected_tab == 2 {
                                    let filtered_indices = state.get_filtered_event_indices();
                                    if !filtered_indices.is_empty() {
                                        state.show_event_detail = true;
                                    }
                                }
                            }
                            _ => {}
                        }
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
                    // Build summary string for the event list
                    let mut details = action_details.action_type.clone();
                    if !action_details.function_name.is_empty() {
                        details = format!("{}: {}", details, action_details.function_name);
                    }
                    if let Some(ms) = action_details.duration_ms {
                        details = format!("{} ({}ms)", details, ms);
                    }
                    if let Some(s) = action_details.success {
                        let status = if s { "✓" } else { "✗" };
                        details = format!("{} {}", details, status);
                    }
                    if !action_details.summary.is_empty() && action_details.summary != action_details.action_type {
                        details = format!("{} - {}", details, action_details.summary);
                    }

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
                    let full_details = full_lines.join("\n");

                    state.add_event_full(
                        "ACTION",
                        &session_id,
                        &details,
                        Some(full_details),
                        action_details.code_execution.clone(),
                        action_details.metadata.clone(),
                    );
                }
                MonitorEvent::LlmConsultation(session_id, consultation) => {
                    state.add_llm_consultation(session_id, consultation);
                }
                MonitorEvent::Tick => {}
            }
        }

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
                                                        language: language.unwrap_or_else(|| "unknown".to_string()),
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

    // Status bar
    let status = Paragraph::new(format!(
        " [{}] Sessions: {} | Agents: {} | {}",
        chrono::Local::now().format("%H:%M:%S"),
        state.sessions.len(),
        state.agents.len(),
        state.status_message
    ))
    .style(Style::default().fg(Color::Yellow))
    .block(Block::default().borders(Borders::ALL));
    f.render_widget(status, chunks[2]);
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
            let is_selected = display_idx == (filtered_count - 1 - state.selected_event_index);

            let base_style = if is_selected {
                Style::default()
                    .fg(Color::Black)
                    .bg(Color::Cyan)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default()
            };

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
                Span::styled(
                    format!("{}: {}", event.session_id, event.details),
                    base_style,
                ),
            ])
        })
        .collect();

    let title = if state.get_selected_session_id().is_some() {
        format!(
            "Recent Events (filtered: {}) - ↑↓ to select, Enter for details",
            filtered_count
        )
    } else {
        format!(
            "Recent Events ({}) - ↑↓ to select, Enter for details",
            filtered_count
        )
    };

    let paragraph = Paragraph::new(text)
        .block(Block::default().title(title).borders(Borders::ALL))
        .wrap(Wrap { trim: true });

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
    lines.push(Line::from(vec![
        Span::styled("Event Details", Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)),
    ]));
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
        lines.push(Line::from(vec![
            Span::styled("Details:", Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)),
        ]));
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
        lines.push(Line::from(vec![
            Span::styled("━━━ Code Execution ━━━", Style::default().fg(Color::Green).add_modifier(Modifier::BOLD)),
        ]));
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
            lines.push(Line::from(vec![
                Span::styled("Code:", Style::default().fg(Color::Magenta).add_modifier(Modifier::BOLD)),
            ]));
            for line in code_exec.code.lines().take(20) {
                lines.push(Line::from(vec![
                    Span::styled("  │ ", Style::default().fg(Color::DarkGray)),
                    Span::styled(line, Style::default().fg(Color::White)),
                ]));
            }
            if code_exec.code.lines().count() > 20 {
                lines.push(Line::from(vec![
                    Span::styled("  ... (truncated)", Style::default().fg(Color::DarkGray)),
                ]));
            }
            lines.push(Line::from(""));
        }

        // Stdout
        if !code_exec.stdout.is_empty() {
            lines.push(Line::from(vec![
                Span::styled("Stdout:", Style::default().fg(Color::Green).add_modifier(Modifier::BOLD)),
            ]));
            for line in code_exec.stdout.lines().take(10) {
                lines.push(Line::from(vec![
                    Span::styled("  │ ", Style::default().fg(Color::DarkGray)),
                    Span::styled(line, Style::default().fg(Color::Green)),
                ]));
            }
            if code_exec.stdout.lines().count() > 10 {
                lines.push(Line::from(vec![
                    Span::styled("  ... (truncated)", Style::default().fg(Color::DarkGray)),
                ]));
            }
            lines.push(Line::from(""));
        }

        // Stderr
        if !code_exec.stderr.is_empty() {
            lines.push(Line::from(vec![
                Span::styled("Stderr:", Style::default().fg(Color::Red).add_modifier(Modifier::BOLD)),
            ]));
            for line in code_exec.stderr.lines().take(10) {
                lines.push(Line::from(vec![
                    Span::styled("  │ ", Style::default().fg(Color::DarkGray)),
                    Span::styled(line, Style::default().fg(Color::Red)),
                ]));
            }
            if code_exec.stderr.lines().count() > 10 {
                lines.push(Line::from(vec![
                    Span::styled("  ... (truncated)", Style::default().fg(Color::DarkGray)),
                ]));
            }
            lines.push(Line::from(""));
        }
    }

    // Metadata if available
    if let Some(ref metadata) = event.metadata {
        lines.push(Line::from(vec![
            Span::styled("━━━ Metadata ━━━", Style::default().fg(Color::Blue).add_modifier(Modifier::BOLD)),
        ]));
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
                lines.push(Line::from(vec![
                    Span::styled("  ... (truncated)", Style::default().fg(Color::DarkGray)),
                ]));
            }
        }
    }

    // Footer
    lines.push(Line::from(""));
    lines.push(Line::from(vec![
        Span::styled("Press ", Style::default().fg(Color::DarkGray)),
        Span::styled("Esc", Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)),
        Span::styled(" to close", Style::default().fg(Color::DarkGray)),
    ]));

    let paragraph = Paragraph::new(lines)
        .block(
            Block::default()
                .title(" Event Detail ")
                .title_style(Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD))
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

            vec![
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
                Line::from(""), // Empty line between consultations
            ]
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
