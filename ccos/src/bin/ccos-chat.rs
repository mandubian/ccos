//! CCOS Chat CLI Tool
//!
//! Rich Interactive TUI to talk to CCOS Chat Gateway.

use clap::Parser;
use crossterm::{
    event::{self, Event, KeyCode, KeyModifiers},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::{
    backend::CrosstermBackend,
    layout::{Constraint, Direction, Layout},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph},
    Frame, Terminal,
};
use reqwest::Client;
use serde_json::json;
use std::collections::{HashMap, HashSet};
use std::io;
use std::path::PathBuf;
use std::time::{Duration, Instant};
use tokio::sync::mpsc;
use tracing::{info, warn};

#[derive(Parser, Debug)]
#[command(name = "ccos-chat")]
struct Args {
    #[arg(long, default_value = "http://127.0.0.1:8833")]
    connector_url: String,

    #[arg(long, default_value = "http://127.0.0.1:8822")]
    gateway_url: String,

    /// Optional external status endpoint to poll for posts (e.g., http://localhost:8765)
    #[arg(long)]
    status_url: Option<String>,

    #[arg(long, default_value = "demo-secret")]
    secret: String,

    #[arg(long, default_value = "user1")]
    user_id: String,

    #[arg(long, default_value = "general")]
    channel_id: String,

    /// Path to agent configuration file used for /model autocompletion
    #[arg(long, default_value = "config/agent_config.toml")]
    config_path: String,
}

#[derive(Debug, Clone)]
struct ModelCompletionState {
    prefix: String,
    matches: Vec<String>,
    index: usize,
}

#[derive(Debug, Clone)]
enum MessageSource {
    User,
    Agent,
    System,
    Direct,
    Audit,
}

#[derive(Debug, Clone)]
struct ChatMessage {
    source: MessageSource,
    sender: String,
    content: String,
    timestamp: chrono::DateTime<chrono::Local>,
    #[allow(dead_code)]
    metadata: Option<HashMap<String, serde_json::Value>>,
}

enum AppEvent {
    Input(Event),
    Message(ChatMessage),
    Error(String),
    Status(String),
    Tick,
    AuditUpdate(String, ChatMessage),
}

const MAX_HISTORY_ENTRIES: usize = 500;

struct AppState {
    messages: Vec<ChatMessage>,
    input: String,
    input_cursor: usize,
    input_history: Vec<String>,
    history_index: Option<usize>,
    history_draft: Option<String>,
    scroll: usize,
    status: String,
    last_tick: Instant,
    should_quit: bool,
    is_waiting: bool,
    spinner_frame: usize,
    seen_audit_events: HashSet<String>,
    available_llm_profiles: Vec<String>,
    model_completion: Option<ModelCompletionState>,
}

impl AppState {
    fn new(available_llm_profiles: Vec<String>) -> Self {
        Self {
            messages: vec![ChatMessage {
                source: MessageSource::System,
                sender: "System".to_string(),
                content: "Welcome to CCOS Chat! Type your message below. @agent mention is added automatically if needed.".to_string(),
                timestamp: chrono::Local::now(),
                metadata: None,
            }],
            input: String::new(),
            input_cursor: 0,
            input_history: Vec::new(),
            history_index: None,
            history_draft: None,
            scroll: 0,
            status: "Connecting...".to_string(),
            last_tick: Instant::now(),
            should_quit: false,
            is_waiting: false,
            spinner_frame: 0,
            seen_audit_events: HashSet::new(),
            available_llm_profiles,
            model_completion: None,
        }
    }

    fn clear_completion(&mut self) {
        self.model_completion = None;
    }

    fn push_input_history(&mut self, input: &str) {
        if input.is_empty() {
            return;
        }
        if self.input_history.last().map(|s| s.as_str()) == Some(input) {
            return;
        }
        self.input_history.push(input.to_string());
        if self.input_history.len() > MAX_HISTORY_ENTRIES {
            let excess = self.input_history.len().saturating_sub(MAX_HISTORY_ENTRIES);
            self.input_history.drain(0..excess);
        }
    }

    fn reset_history_nav(&mut self) {
        self.history_index = None;
        self.history_draft = None;
    }

    fn history_prev(&mut self) -> bool {
        if self.input_history.is_empty() {
            return false;
        }

        match self.history_index {
            None => {
                self.history_draft = Some(self.input.clone());
                self.history_index = Some(self.input_history.len() - 1);
            }
            Some(idx) if idx > 0 => {
                self.history_index = Some(idx - 1);
            }
            Some(_) => {}
        }

        if let Some(idx) = self.history_index {
            self.input = self.input_history[idx].clone();
            self.input_cursor = self.input.len();
            return true;
        }
        false
    }

    fn history_next(&mut self) -> bool {
        let Some(idx) = self.history_index else {
            return false;
        };

        if idx + 1 < self.input_history.len() {
            self.history_index = Some(idx + 1);
            if let Some(next_idx) = self.history_index {
                self.input = self.input_history[next_idx].clone();
                self.input_cursor = self.input.len();
                return true;
            }
            return false;
        }

        self.history_index = None;
        self.input = self.history_draft.take().unwrap_or_default();
        self.input_cursor = self.input.len();
        true
    }
}

fn input_history_path() -> PathBuf {
    ccos::utils::fs::get_workspace_root()
        .join(".ccos")
        .join("ccos-chat-history.json")
}

fn load_input_history_from_disk() -> Vec<String> {
    let path = input_history_path();
    let content = match std::fs::read_to_string(&path) {
        Ok(content) => content,
        Err(_) => return Vec::new(),
    };
    let mut entries = match serde_json::from_str::<Vec<String>>(&content) {
        Ok(entries) => entries,
        Err(_) => return Vec::new(),
    };
    entries.retain(|e| !e.trim().is_empty());
    if entries.len() > MAX_HISTORY_ENTRIES {
        let excess = entries.len() - MAX_HISTORY_ENTRIES;
        entries.drain(0..excess);
    }
    entries
}

fn save_input_history_to_disk(history: &[String]) -> anyhow::Result<()> {
    let path = input_history_path();
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let content = serde_json::to_string_pretty(history)?;
    std::fs::write(path, content)?;
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
        Err(_) => return profiles,
    };

    let normalized = if content.starts_with("# RTFS") {
        content.lines().skip(1).collect::<Vec<_>>().join("\n")
    } else {
        content
    };

    let config = match toml::from_str::<ccos::config::types::AgentConfig>(&normalized) {
        Ok(config) => config,
        Err(_) => return profiles,
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

fn prev_char_boundary(text: &str, cursor: usize) -> usize {
    if cursor == 0 {
        return 0;
    }
    text[..cursor]
        .char_indices()
        .last()
        .map(|(idx, _)| idx)
        .unwrap_or(0)
}

fn next_char_boundary(text: &str, cursor: usize) -> usize {
    if cursor >= text.len() {
        return text.len();
    }
    cursor
        + text[cursor..]
            .chars()
            .next()
            .map(|c| c.len_utf8())
            .unwrap_or(0)
}

fn char_count(text: &str) -> usize {
    text.chars().count()
}

fn byte_index_at_char(text: &str, char_idx: usize) -> usize {
    if char_idx == 0 {
        return 0;
    }
    text.char_indices()
        .nth(char_idx)
        .map(|(idx, _)| idx)
        .unwrap_or(text.len())
}

fn move_cursor_vertically(text: &str, cursor: usize, direction: i32) -> usize {
    let before_cursor = &text[..cursor];
    let current_line_idx = before_cursor.matches('\n').count();
    let current_line_start = before_cursor.rfind('\n').map(|p| p + 1).unwrap_or(0);
    let current_col = char_count(&before_cursor[current_line_start..]);

    let lines: Vec<&str> = text.split('\n').collect();
    let target_line_idx = if direction < 0 {
        current_line_idx.saturating_sub(1)
    } else {
        (current_line_idx + 1).min(lines.len().saturating_sub(1))
    };

    if target_line_idx == current_line_idx {
        return cursor;
    }

    let mut target_line_start = 0usize;
    for line in lines.iter().take(target_line_idx) {
        target_line_start += line.len() + 1;
    }
    let target_col = current_col.min(char_count(lines[target_line_idx]));
    target_line_start + byte_index_at_char(lines[target_line_idx], target_col)
}

fn apply_model_completion(state: &mut AppState) {
    let input = state.input.as_str();
    if !input.starts_with("/model") || input.contains('\n') {
        state.clear_completion();
        return;
    }

    let prefix = input
        .strip_prefix("/model")
        .map(|s| s.trim_start())
        .unwrap_or("")
        .to_string();

    let matches = if prefix.is_empty() {
        state.available_llm_profiles.clone()
    } else {
        let lower = prefix.to_ascii_lowercase();
        state
            .available_llm_profiles
            .iter()
            .filter(|p| p.to_ascii_lowercase().starts_with(&lower))
            .cloned()
            .collect::<Vec<_>>()
    };

    if matches.is_empty() {
        state.clear_completion();
        return;
    }

    let next_index = if let Some(comp) = &state.model_completion {
        if comp.prefix == prefix && comp.matches == matches {
            (comp.index + 1) % matches.len()
        } else {
            0
        }
    } else {
        0
    };

    let selected = matches[next_index].clone();
    state.input = format!("/model {}", selected);
    state.input_cursor = state.input.len();
    state.model_completion = Some(ModelCompletionState {
        prefix,
        matches,
        index: next_index,
    });
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let args = Args::parse();
    let available_profiles = load_available_llm_profiles(&args.config_path);

    // Setup terminal
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    // Setup app state and channels
    let mut state = AppState::new(available_profiles);
    state.input_history = load_input_history_from_disk();
    let (tx, mut rx) = mpsc::unbounded_channel();

    // Spawn event handlers
    let tx_input = tx.clone();
    tokio::spawn(async move {
        let tick_rate = Duration::from_millis(100);
        let mut last_tick = Instant::now();
        loop {
            let timeout = tick_rate
                .checked_sub(last_tick.elapsed())
                .unwrap_or(Duration::from_secs(0));
            if event::poll(timeout).expect("failed to poll event") {
                if let Ok(ev) = event::read() {
                    let _ = tx_input.send(AppEvent::Input(ev));
                }
            }
            if last_tick.elapsed() >= tick_rate {
                if let Ok(_) = tx_input.send(AppEvent::Tick) {
                    last_tick = Instant::now();
                }
            }
        }
    });

    let client = Client::new();

    // Spawn external status poller (only if status_url is provided)
    if let Some(status_url) = args.status_url.clone() {
        let poller_client = client.clone();
        let tx_msg = tx.clone();
        tokio::spawn(async move {
            let mut poller_last_post_id: Option<String> = None;

            // First, sync with existing posts
            if let Ok(status) = fetch_status(&poller_client, &status_url).await {
                if let Some(posts) = status.get("posts").and_then(|p| p.as_array()) {
                    if let Some(last) = posts.last() {
                        poller_last_post_id = last
                            .get("id")
                            .and_then(|id| id.as_str().map(|s| s.to_string()));
                    }
                }
            }

            loop {
                tokio::time::sleep(Duration::from_secs(2)).await;
                if let Ok(status) = fetch_status(&poller_client, &status_url).await {
                    if let Some(posts) = status.get("posts").and_then(|p| p.as_array()) {
                        let mut new_posts = Vec::new();
                        let mut found_last = poller_last_post_id.is_none();

                        for post in posts {
                            let id = post
                                .get("id")
                                .and_then(|id| id.as_str())
                                .unwrap_or_default();
                            if !found_last {
                                if Some(id.to_string()) == poller_last_post_id {
                                    found_last = true;
                                }
                                continue;
                            }
                            new_posts.push(post);
                        }

                        for post in new_posts {
                            let content = post
                                .get("content")
                                .and_then(|c| c.as_str())
                                .unwrap_or_default();
                            let agent_id = post
                                .get("agent_id")
                                .and_then(|a| a.as_str())
                                .unwrap_or("agent");
                            let id = post
                                .get("id")
                                .and_then(|id| id.as_str())
                                .unwrap_or_default();

                            let _ = tx_msg.send(AppEvent::Message(ChatMessage {
                                source: MessageSource::Agent,
                                sender: agent_id.to_string(),
                                content: content.to_string(),
                                timestamp: chrono::Local::now(),
                                metadata: None,
                            }));

                            poller_last_post_id = Some(id.to_string());
                        }
                    }
                }
            }
        });
    }

    // Spawn direct message poller
    let direct_client = client.clone();
    let direct_url = args.connector_url.clone();
    let direct_secret = args.secret.clone();
    let direct_channel = args.channel_id.clone();
    let tx_direct = tx.clone();

    tokio::spawn(async move {
        let mut first_check = true;
        loop {
            match direct_client
                .get(format!("{}/connector/loopback/outbound", direct_url))
                .header("x-ccos-connector-secret", &direct_secret)
                .query(&[("channel_id", &direct_channel)])
                .send()
                .await
            {
                Ok(resp) => {
                    if first_check {
                        let _ = tx_direct.send(AppEvent::Status("Connected".to_string()));
                        first_check = false;
                    }
                    if let Ok(messages) = resp.json::<Vec<OutboundRequest>>().await {
                        for msg in messages {
                            let _ = tx_direct.send(AppEvent::Message(ChatMessage {
                                source: MessageSource::Direct,
                                sender: "agent".to_string(),
                                content: msg.content,
                                timestamp: chrono::Local::now(),
                                metadata: None,
                            }));
                        }
                    }
                }
                Err(e) => {
                    let _ = tx_direct.send(AppEvent::Status(format!("Offline: {}", e)));
                    first_check = true;
                }
            }
            tokio::time::sleep(Duration::from_secs(1)).await;
        }
    });

    // Check session status before connecting
    let ws_session = format!("chat:{}:{}", args.channel_id, args.user_id);
    let session_client = Client::new();
    let session_status =
        check_session_status(&session_client, &args.gateway_url, &ws_session).await;

    match session_status {
        SessionStatus::New => {
            let _ = tx.send(AppEvent::Message(ChatMessage {
                source: MessageSource::System,
                sender: "System".to_string(),
                content: "🆕 New session created. Agent will spawn on first message.".to_string(),
                timestamp: chrono::Local::now(),
                metadata: None,
            }));
        }
        SessionStatus::Reconnecting { agent_running } => {
            let msg = if agent_running {
                "🔄 Reconnected to existing session. Agent is running.".to_string()
            } else {
                "🔄 Reconnected to existing session. Starting agent...".to_string()
            };
            let _ = tx.send(AppEvent::Message(ChatMessage {
                source: MessageSource::System,
                sender: "System".to_string(),
                content: msg,
                timestamp: chrono::Local::now(),
                metadata: None,
            }));
        }
    }

    // Spawn WebSocket event stream for real-time updates
    let ws_url = args.gateway_url.clone();
    let tx_ws = tx.clone();
    let ws_token = args.secret.clone();

    tokio::spawn(async move {
        use futures::{SinkExt, StreamExt};
        use tokio_tungstenite::{connect_async, tungstenite::protocol::Message};

        let ws_endpoint = ws_url
            .replace("http://", "ws://")
            .replace("https://", "wss://");
        let url = format!(
            "{}/chat/stream/{}?token={}",
            ws_endpoint, ws_session, ws_token
        );

        loop {
            match connect_async(&url).await {
                Ok((ws_stream, _)) => {
                    let (mut write, mut read) = ws_stream.split();

                    // Send initial connection message
                    info!("WebSocket connected to {}", url);

                    // Process incoming messages
                    while let Some(msg) = read.next().await {
                        match msg {
                            Ok(Message::Text(text)) => {
                                // Parse the event
                                if let Ok(event) = serde_json::from_str::<serde_json::Value>(&text)
                                {
                                    let event_type = event
                                        .get("event_type")
                                        .and_then(|v| v.as_str())
                                        .unwrap_or("unknown");

                                    match event_type {
                                        "action" | "historical" => {
                                            // Handle action events
                                            if let Some(action) = event.get("action") {
                                                let action_type = action
                                                    .get("action_type")
                                                    .and_then(|v| v.as_str())
                                                    .unwrap_or("unknown");
                                                let function_name = action
                                                    .get("function_name")
                                                    .and_then(|v| v.as_str())
                                                    .unwrap_or("unknown");

                                                let (sender, content) = match action_type {
                                                    "CapabilityCall" => (
                                                        "Action".to_string(),
                                                        format!("⚡ {}", function_name),
                                                    ),
                                                    "CapabilityResult" => (
                                                        "Result".to_string(),
                                                        format!("✅ {}", function_name),
                                                    ),
                                                    _ => continue,
                                                };

                                                let event_id = format!(
                                                    "{}-{}",
                                                    action
                                                        .get("timestamp")
                                                        .and_then(|v| v.as_u64())
                                                        .unwrap_or(0),
                                                    function_name
                                                );

                                                let message = ChatMessage {
                                                    source: MessageSource::Audit,
                                                    sender,
                                                    content,
                                                    timestamp: chrono::Local::now(),
                                                    metadata: None,
                                                };

                                                let _ = tx_ws
                                                    .send(AppEvent::AuditUpdate(event_id, message));
                                            }
                                        }
                                        "state_update" => {
                                            // Handle state updates (heartbeats)
                                            if let Some(state) = event.get("state") {
                                                let is_healthy = state
                                                    .get("is_healthy")
                                                    .and_then(|v| v.as_bool())
                                                    .unwrap_or(false);
                                                let current_step = state
                                                    .get("current_step")
                                                    .and_then(|v| v.as_u64())
                                                    .unwrap_or(0);

                                                let health_icon =
                                                    if is_healthy { "🟢" } else { "🔴" };
                                                let message = ChatMessage {
                                                    source: MessageSource::Audit,
                                                    sender: "Status".to_string(),
                                                    content: format!(
                                                        "{} Agent step: {}",
                                                        health_icon, current_step
                                                    ),
                                                    timestamp: chrono::Local::now(),
                                                    metadata: None,
                                                };

                                                let _ = tx_ws.send(AppEvent::AuditUpdate(
                                                    format!(
                                                        "state-{}",
                                                        chrono::Local::now().timestamp()
                                                    ),
                                                    message,
                                                ));
                                            }
                                        }
                                        "agent_crashed" => {
                                            // Handle crash events
                                            let message = ChatMessage {
                                                source: MessageSource::Audit,
                                                sender: "Alert".to_string(),
                                                content: "💥 Agent crashed!".to_string(),
                                                timestamp: chrono::Local::now(),
                                                metadata: None,
                                            };

                                            let _ = tx_ws.send(AppEvent::AuditUpdate(
                                                format!(
                                                    "crash-{}",
                                                    chrono::Local::now().timestamp()
                                                ),
                                                message,
                                            ));
                                        }
                                        "ping" => {
                                            // Respond to ping with pong
                                            let _ = write.send(Message::Pong(vec![])).await;
                                        }
                                        _ => {}
                                    }
                                }
                            }
                            Ok(Message::Close(_)) => {
                                info!("WebSocket closed, reconnecting...");
                                break;
                            }
                            Err(e) => {
                                warn!("WebSocket error: {}, reconnecting...", e);
                                break;
                            }
                            _ => {}
                        }
                    }
                }
                Err(e) => {
                    warn!("WebSocket connection failed: {}, retrying in 5s...", e);
                }
            }

            // Wait before reconnecting
            tokio::time::sleep(Duration::from_secs(5)).await;
        }
    });

    // Main loop
    while !state.should_quit {
        terminal.draw(|f| render(f, &state))?;

        if let Some(event) = rx.recv().await {
            match event {
                AppEvent::Input(ev) => {
                    if let Event::Key(key) = ev {
                        match key.code {
                            KeyCode::Char(c) => {
                                state.reset_history_nav();
                                state.input.insert(state.input_cursor, c);
                                state.input_cursor += c.len_utf8();
                                state.clear_completion();
                            }
                            KeyCode::Backspace => {
                                if state.input_cursor > 0 {
                                    state.reset_history_nav();
                                    let prev = prev_char_boundary(&state.input, state.input_cursor);
                                    state.input.replace_range(prev..state.input_cursor, "");
                                    state.input_cursor = prev;
                                    state.clear_completion();
                                }
                            }
                            KeyCode::Delete => {
                                if state.input_cursor < state.input.len() {
                                    state.reset_history_nav();
                                    let next = next_char_boundary(&state.input, state.input_cursor);
                                    state.input.replace_range(state.input_cursor..next, "");
                                    state.clear_completion();
                                }
                            }
                            KeyCode::Enter => {
                                if key.modifiers.contains(KeyModifiers::SHIFT)
                                    || key.modifiers.contains(KeyModifiers::CONTROL)
                                {
                                    state.reset_history_nav();
                                    state.input.insert(state.input_cursor, '\n');
                                    state.input_cursor += 1;
                                    state.clear_completion();
                                } else if !state.input.trim().is_empty() {
                                    let mut text = state.input.trim().to_string();
                                    state.push_input_history(&text);
                                    if let Err(e) = save_input_history_to_disk(&state.input_history)
                                    {
                                        warn!("Failed to persist chat input history: {}", e);
                                    }

                                    // Smart Mentions: Add @agent if no mention is present
                                    // BUT: Don't add @agent to slash commands (they're handled locally)
                                    if !text.starts_with('@') && !text.starts_with('/') {
                                        text = format!("@agent {}", text);
                                    }

                                    let message = ChatMessage {
                                        source: MessageSource::User,
                                        sender: args.user_id.clone(),
                                        content: text.clone(),
                                        timestamp: chrono::Local::now(),
                                        metadata: None,
                                    };
                                    state.messages.push(message);
                                    state.is_waiting = true;

                                    // Feedback to user
                                    state.messages.push(ChatMessage {
                                        source: MessageSource::System,
                                        sender: "System".to_string(),
                                        content: format!(
                                            "Sending to gateway (Channel: {})...",
                                            args.channel_id
                                        ),
                                        timestamp: chrono::Local::now(),
                                        metadata: None,
                                    });

                                    // Send to gateway
                                    let payload = json!({
                                        "channel_id": args.channel_id,
                                        "sender_id": args.user_id,
                                        "text": text,
                                        "timestamp": chrono::Utc::now().to_rfc3339()
                                    });

                                    let client = client.clone();
                                    let url = format!(
                                        "{}/connector/loopback/inbound",
                                        args.connector_url
                                    );
                                    let secret = args.secret.clone();
                                    let tx_fb = tx.clone();

                                    tokio::spawn(async move {
                                        match client
                                            .post(url)
                                            .header("x-ccos-connector-secret", &secret)
                                            .json(&payload)
                                            .send()
                                            .await
                                        {
                                            Ok(resp) => {
                                                if resp.status().is_success() {
                                                    if let Ok(body) =
                                                        resp.json::<serde_json::Value>().await
                                                    {
                                                        if body
                                                            .get("accepted")
                                                            .and_then(|a| a.as_bool())
                                                            .unwrap_or(false)
                                                        {
                                                            let _ = tx_fb.send(AppEvent::Status(
                                                                "Message accepted".to_string(),
                                                            ));
                                                        } else {
                                                            let err = body.get("error").and_then(|e| e.as_str()).unwrap_or("Gateway rejected message (check channel/sender allowlist or mentions)");
                                                            let _ = tx_fb.send(AppEvent::Error(
                                                                format!("Rejected: {}", err),
                                                            ));
                                                            let _ = tx_fb.send(AppEvent::Status(
                                                                "Rejected".to_string(),
                                                            ));
                                                        }
                                                    } else {
                                                        let _ = tx_fb.send(AppEvent::Status(
                                                            "Message sent".to_string(),
                                                        ));
                                                    }
                                                } else {
                                                    let _ = tx_fb.send(AppEvent::Error(format!(
                                                        "Failed to send: HTTP {}",
                                                        resp.status()
                                                    )));
                                                }
                                            }
                                            Err(e) => {
                                                let _ = tx_fb.send(AppEvent::Error(format!(
                                                    "Connection error: {}",
                                                    e
                                                )));
                                            }
                                        }
                                    });

                                    state.input.clear();
                                    state.input_cursor = 0;
                                    state.reset_history_nav();
                                    state.clear_completion();
                                    state.scroll = 0; // Auto-scroll to bottom
                                }
                            }
                            KeyCode::Tab => {
                                apply_model_completion(&mut state);
                            }
                            KeyCode::Esc => {
                                state.should_quit = true;
                            }
                            KeyCode::PageUp => {
                                state.scroll += 5;
                            }
                            KeyCode::PageDown => {
                                state.scroll = state.scroll.saturating_sub(5);
                            }
                            KeyCode::Up => {
                                if key.modifiers.contains(KeyModifiers::ALT) {
                                    state.scroll += 1;
                                } else if key.modifiers.contains(KeyModifiers::CONTROL) {
                                    state.input_cursor =
                                        move_cursor_vertically(&state.input, state.input_cursor, -1);
                                } else {
                                    let changed = state.history_prev();
                                    if changed {
                                        state.clear_completion();
                                    }
                                }
                            }
                            KeyCode::Down => {
                                if key.modifiers.contains(KeyModifiers::ALT) {
                                    state.scroll = state.scroll.saturating_sub(1);
                                } else if key.modifiers.contains(KeyModifiers::CONTROL) {
                                    state.input_cursor =
                                        move_cursor_vertically(&state.input, state.input_cursor, 1);
                                } else {
                                    let changed = state.history_next();
                                    if changed {
                                        state.clear_completion();
                                    }
                                }
                            }
                            KeyCode::Left => {
                                state.input_cursor = prev_char_boundary(&state.input, state.input_cursor);
                            }
                            KeyCode::Right => {
                                state.input_cursor = next_char_boundary(&state.input, state.input_cursor);
                            }
                            KeyCode::Home => {
                                state.input_cursor = 0;
                            }
                            KeyCode::End => {
                                state.input_cursor = state.input.len();
                            }
                            _ => {}
                        }
                    }
                }
                AppEvent::Message(msg) => {
                    if matches!(msg.source, MessageSource::Agent | MessageSource::Direct) {
                        state.is_waiting = false;
                    }
                    state.messages.push(msg);
                }
                AppEvent::Error(err) => {
                    state.is_waiting = false;
                    state.messages.push(ChatMessage {
                        source: MessageSource::System,
                        sender: "Error".to_string(),
                        content: err,
                        timestamp: chrono::Local::now(),
                        metadata: None,
                    });
                }
                AppEvent::Status(s) => {
                    state.status = s;
                }
                AppEvent::Tick => {
                    state.last_tick = Instant::now();
                    state.spinner_frame = state.spinner_frame.wrapping_add(1);
                }
                AppEvent::AuditUpdate(id, msg) => {
                    if !state.seen_audit_events.contains(&id) {
                        state.seen_audit_events.insert(id);
                        state.messages.push(msg);
                    }
                }
            }
        }
    }

    // Cleanup terminal
    disable_raw_mode()?;
    execute!(terminal.backend_mut(), LeaveAlternateScreen,)?;
    terminal.show_cursor()?;

    Ok(())
}

fn render(f: &mut Frame, state: &AppState) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3), // Header
            Constraint::Min(1),    // Messages
            Constraint::Length(6), // Input
        ])
        .split(f.size());

    // Header
    let spinner = if state.is_waiting {
        let frames = ["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"];
        frames[state.spinner_frame % frames.len()]
    } else {
        ""
    };

    let header = Paragraph::new(Line::from(vec![
        Span::styled(
            " CCOS CHAT ",
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled(format!(" {} ", spinner), Style::default().fg(Color::Yellow)),
        Span::raw("│ Status: "),
        Span::styled(
            &state.status,
            Style::default().fg(if state.status == "Connected" {
                Color::Green
            } else {
                Color::Yellow
            }),
        ),
        Span::raw(" │ ESC: quit │ ↑/↓: history │ Ctrl+↑/↓: cursor line │ Alt+↑/↓: fine scroll"),
    ]))
    .block(Block::default().borders(Borders::ALL));
    f.render_widget(header, chunks[0]);

    // Messages area
    let message_area = chunks[1];
    let wrap_width = message_area.width.saturating_sub(4) as usize;

    // Build a Paragraph from all messages with manual wrapping for accurate height
    let mut message_lines = Vec::new();
    for m in &state.messages {
        let color = match m.source {
            MessageSource::User => Color::Yellow,
            MessageSource::Agent => Color::Cyan,
            MessageSource::System => Color::Red,
            MessageSource::Direct => Color::Magenta,
            MessageSource::Audit => Color::Blue,
        };

        let prefix = match m.source {
            MessageSource::User => format!(" {} [{}]:", m.sender, m.timestamp.format("%H:%M:%S")),
            MessageSource::Agent => format!(" {} [{}]:", m.sender, m.timestamp.format("%H:%M:%S")),
            MessageSource::System => format!(" {} [SYST]:", m.sender),
            MessageSource::Direct => format!(" {} [{}]:", m.sender, m.timestamp.format("%H:%M:%S")),
            MessageSource::Audit => format!(" {} [AUDT]:", m.sender),
        };

        message_lines.push(Line::from(vec![Span::styled(
            prefix,
            Style::default().fg(color).add_modifier(Modifier::BOLD),
        )]));

        for content_line in m.content.lines() {
            for wrapped in textwrap::wrap(content_line, wrap_width) {
                message_lines.push(Line::from(vec![
                    Span::raw("  "),
                    Span::raw(wrapped.into_owned()),
                ]));
            }
        }
        message_lines.push(Line::from(""));
    }

    // Add one extra line of padding at the bottom to ensure the last message is fully visible
    message_lines.push(Line::from(""));

    let total_height = message_lines.len();
    let container_height = message_area.height.saturating_sub(2) as usize;

    // Auto-scroll logic: if scroll is 0, show newest at bottom.
    // positive scroll means looking UP into history
    let scroll_offset = total_height
        .saturating_sub(container_height)
        .saturating_sub(state.scroll);

    let paragraph = Paragraph::new(message_lines)
        .block(Block::default().borders(Borders::ALL).title(format!(
            " Messages ({}/{}) ",
            total_height, container_height
        )))
        .style(Style::default().fg(Color::White))
        .scroll((scroll_offset as u16, 0));
    f.render_widget(paragraph, message_area);

    // Input
    let input = Paragraph::new(state.input.as_str())
        .style(Style::default().fg(Color::Yellow))
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title(" Type your message (Enter=send, Shift/Ctrl+Enter=newline, Tab=/model autocomplete) "),
        )
        .wrap(ratatui::widgets::Wrap { trim: false });
    f.render_widget(input, chunks[2]);

    let before_cursor = &state.input[..state.input_cursor.min(state.input.len())];
    let cursor_row = before_cursor.matches('\n').count() as u16;
    let cursor_col = before_cursor
        .rsplit('\n')
        .next()
        .map(|s| s.chars().count() as u16)
        .unwrap_or(0);
    let max_row = chunks[2].height.saturating_sub(3);
    f.set_cursor(
        chunks[2].x + 1 + cursor_col,
        chunks[2].y + 1 + cursor_row.min(max_row),
    );
}

async fn fetch_status(client: &Client, url: &str) -> anyhow::Result<serde_json::Value> {
    let resp = client.get(format!("{}/status", url)).send().await?;
    let json = resp.json().await?;
    Ok(json)
}

#[derive(Debug, serde::Deserialize)]
struct OutboundRequest {
    content: String,
    #[allow(dead_code)]
    channel_id: String,
}

#[allow(dead_code)]
#[derive(Debug, serde::Deserialize)]
struct ChatAuditEntryResponse {
    timestamp: u64,
    event_type: String,
    function_name: Option<String>,
    session_id: Option<String>,
}

#[allow(dead_code)]
#[derive(Debug, serde::Deserialize)]
struct ChatAuditResponse {
    events: Vec<ChatAuditEntryResponse>,
}

/// Session status for reconnect logic
#[derive(Debug)]
enum SessionStatus {
    New,
    Reconnecting { agent_running: bool },
}

/// Check if session exists and get its status
async fn check_session_status(
    client: &Client,
    gateway_url: &str,
    session_id: &str,
) -> SessionStatus {
    let url = format!("{}/chat/session/{}", gateway_url, session_id);

    match client.get(&url).send().await {
        Ok(response) if response.status().is_success() => {
            if let Ok(info) = response.json::<serde_json::Value>().await {
                let agent_running = info
                    .get("agent_running")
                    .and_then(|v| v.as_bool())
                    .unwrap_or(false);
                SessionStatus::Reconnecting { agent_running }
            } else {
                SessionStatus::New
            }
        }
        _ => SessionStatus::New,
    }
}
