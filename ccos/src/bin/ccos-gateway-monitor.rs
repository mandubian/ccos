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
    text::{Line, Span, Text},
    widgets::{Block, Borders, Cell, Paragraph, Row, Table, Wrap},
    Frame, Terminal,
};
use reqwest::Client;
use serde::{Deserialize, Serialize};
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
    created_at: String,
    last_activity: String,
}

/// Agent information
#[derive(Debug, Clone)]
struct AgentInfo {
    pid: u32,
    session_id: String,
    current_step: u32,
    memory_mb: Option<u64>,
    is_healthy: bool,
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

/// System event
#[derive(Debug, Clone)]
struct SystemEvent {
    timestamp: Instant,
    event_type: String,
    session_id: String,
    details: String,
}

#[derive(Debug, Clone)]
enum MonitorEvent {
    Input(Event),
    SessionUpdate(Vec<SessionInfo>),
    AgentHeartbeat(String, AgentInfo),
    AgentCrashed(String, u32),
    ActionEvent(String, String), // session_id, action_details
    Tick,
}

struct MonitorState {
    sessions: HashMap<String, SessionInfo>,
    agents: HashMap<String, AgentInfo>,
    events: Vec<SystemEvent>,
    selected_tab: usize,
    last_refresh: Instant,
    should_quit: bool,
    status_message: String,
}

impl MonitorState {
    fn new() -> Self {
        Self {
            sessions: HashMap::new(),
            agents: HashMap::new(),
            events: Vec::new(),
            selected_tab: 0,
            last_refresh: Instant::now(),
            should_quit: false,
            status_message: "Connected to gateway".to_string(),
        }
    }

    fn add_event(&mut self, event_type: &str, session_id: &str, details: &str) {
        self.events.push(SystemEvent {
            timestamp: Instant::now(),
            event_type: event_type.to_string(),
            session_id: session_id.to_string(),
            details: details.to_string(),
        });

        // Keep only last 100 events
        if self.events.len() > 100 {
            self.events.remove(0);
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
                        match key.code {
                            KeyCode::Char('q') => state.should_quit = true,
                            KeyCode::Tab => {
                                state.selected_tab = (state.selected_tab + 1) % 3;
                            }
                            KeyCode::BackTab => {
                                state.selected_tab = if state.selected_tab == 0 {
                                    2
                                } else {
                                    state.selected_tab - 1
                                };
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
                    for session in &sessions {
                        state
                            .sessions
                            .insert(session.session_id.clone(), session.clone());

                        // Populate agents from session info
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
                    state.status_message = format!("Updated {} sessions", state.sessions.len());
                }

                MonitorEvent::AgentHeartbeat(session_id, agent_info) => {
                    state.agents.insert(session_id, agent_info);
                }
                MonitorEvent::AgentCrashed(session_id, pid) => {
                    state.add_event("CRASH", &session_id, &format!("Agent PID {} crashed", pid));
                    state.status_message = format!("⚠️  Agent crashed: {}", session_id);
                }
                MonitorEvent::ActionEvent(session_id, action) => {
                    state.add_event("ACTION", &session_id, &action);
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
                                            let _ = tx
                                                .send(MonitorEvent::ActionEvent(
                                                    session_id.to_string(),
                                                    action_type.to_string(),
                                                ))
                                                .await;
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
    let tab_titles = vec!["Sessions", "Agents", "Events"];
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

    let rows: Vec<Row> = state
        .agents
        .values()
        .map(|agent| {
            let health_icon = if agent.is_healthy {
                "🟢 Healthy"
            } else {
                "🔴 Unhealthy"
            };
            let health_color = if agent.is_healthy {
                Color::Green
            } else {
                Color::Red
            };

            Row::new(vec![
                Cell::from(agent.session_id.clone()),
                Cell::from(agent.pid.to_string()),
                Cell::from(agent.current_step.to_string()),
                Cell::from(
                    agent
                        .memory_mb
                        .map(|m| format!("{} MB", m))
                        .unwrap_or_default(),
                ),
                Cell::from(health_icon).style(Style::default().fg(health_color)),
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
        .block(Block::default().title("Agents").borders(Borders::ALL));

    f.render_widget(table, area);
}

fn draw_events_tab(f: &mut Frame, area: Rect, state: &MonitorState) {
    let text = state
        .events
        .iter()
        .rev()
        .map(|event| {
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
                _ => Color::Gray,
            };

            Line::from(vec![
                Span::styled(
                    format!("[{}] ", time_str),
                    Style::default().fg(Color::DarkGray),
                ),
                Span::styled(
                    format!("[{}] ", event.event_type),
                    Style::default().fg(color),
                ),
                Span::raw(format!("{}: {}", event.session_id, event.details)),
            ])
        })
        .collect::<Vec<_>>();

    let paragraph = Paragraph::new(text)
        .block(
            Block::default()
                .title("Recent Events")
                .borders(Borders::ALL),
        )
        .wrap(Wrap { trim: true });

    f.render_widget(paragraph, area);
}
