//! TUI Chat interface using ratatui + crossterm.

use std::path::Path;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

use arboard::Clipboard;
use tokio::net::TcpStream;

use crossterm::{
    event::{self, DisableMouseCapture, EnableMouseCapture, Event, KeyCode, KeyModifiers},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span, Text},
    widgets::{Block, Borders, Paragraph},
    Frame, Terminal,
};

use super::common::{
    default_terminal_channel_id, default_terminal_sender_id, terminal_channel_envelope,
};
use autonoetic_gateway::router::{
    JsonRpcRequest as GatewayJsonRpcRequest, JsonRpcResponse as GatewayJsonRpcResponse,
};

// ============================================================================
// Constants
// ============================================================================

const SPINNER_FRAMES: &[&str] = &["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"];

// ============================================================================
// App State
// ============================================================================

#[derive(Debug, Clone)]
enum MessageRole {
    User,
    Assistant,
    System,
    Signal,
}

#[derive(Debug, Clone)]
struct ChatMessage {
    role: MessageRole,
    content: String,
}

struct PendingRequest {
    id: u64,
    sent_at: Instant,
}

struct App {
    messages: Vec<ChatMessage>,
    input: String,
    cursor_pos: usize,
    pending: Vec<PendingRequest>,
    next_id: u64,
    spinner_frame: usize,
    scroll_offset: usize,
    session_id: String,
    target_hint: String,
    // Mouse selection
    selecting: bool,
    sel_start: Option<(u16, u16)>,
    sel_end: Option<(u16, u16)>,
}

impl App {
    fn new(session_id: String, target_hint: String) -> Self {
        Self {
            messages: Vec::new(),
            input: String::new(),
            cursor_pos: 0,
            pending: Vec::new(),
            next_id: 1,
            spinner_frame: 0,
            scroll_offset: 0,
            session_id,
            target_hint,
            selecting: false,
            sel_start: None,
            sel_end: None,
        }
    }

    fn add_message(&mut self, role: MessageRole, content: String) {
        self.messages.push(ChatMessage { role, content });
        // Auto-scroll to bottom
        self.scroll_offset = 0;
    }

    fn next_id(&mut self) -> u64 {
        let id = self.next_id;
        self.next_id += 1;
        id
    }

    fn add_pending(&mut self, id: u64) {
        self.pending.push(PendingRequest {
            id,
            sent_at: Instant::now(),
        });
    }

    fn remove_pending(&mut self, id: u64) {
        self.pending.retain(|r| r.id != id);
    }

    fn oldest_secs(&self) -> u64 {
        self.pending
            .iter()
            .map(|r| r.sent_at.elapsed().as_secs())
            .max()
            .unwrap_or(0)
    }

    fn tick_spinner(&mut self) {
        self.spinner_frame = (self.spinner_frame + 1) % SPINNER_FRAMES.len();
    }

    fn spinner(&self) -> &'static str {
        SPINNER_FRAMES[self.spinner_frame]
    }

    fn insert_char(&mut self, c: char) {
        self.input.insert(self.cursor_pos, c);
        self.cursor_pos += c.len_utf8();
    }

    fn delete_char(&mut self) {
        if self.cursor_pos > 0 {
            let prev = self.input[..self.cursor_pos].chars().last().unwrap();
            let len = prev.len_utf8();
            self.cursor_pos -= len;
            self.input.remove(self.cursor_pos);
        }
    }

    fn cursor_left(&mut self) {
        if self.cursor_pos > 0 {
            let prev = self.input[..self.cursor_pos].chars().last().unwrap();
            self.cursor_pos -= prev.len_utf8();
        }
    }

    fn cursor_right(&mut self) {
        if self.cursor_pos < self.input.len() {
            let next = self.input[self.cursor_pos..].chars().next().unwrap();
            self.cursor_pos += next.len_utf8();
        }
    }
}

// ============================================================================
// UUID extraction (for approval notifications)
// ============================================================================

fn extract_approval_request_id(text: &str) -> Option<String> {
    let lower = text.to_lowercase();
    if !lower.contains("approval") && !lower.contains("approve") {
        return None;
    }
    let prefixes = ["request_id:", "request id:", "request_id :", "request id :"];
    for prefix in &prefixes {
        if let Some(start) = lower.find(prefix) {
            let after = &text[start + prefix.len()..].trim();
            if let Some(uuid) = extract_uuid(after) {
                return Some(uuid);
            }
        }
    }
    extract_uuid(text)
}

fn extract_uuid(text: &str) -> Option<String> {
    let chars: Vec<char> = text.chars().collect();
    let mut i = 0;
    while i < chars.len() {
        if i + 8 <= chars.len() && chars[i..i + 8].iter().all(|c| c.is_ascii_hexdigit()) {
            let mut pos = i + 8;
            let segs = [4, 4, 12];
            let mut ok = true;
            for &len in &segs {
                if pos + 1 + len > chars.len() || chars[pos] != '-' {
                    ok = false;
                    break;
                }
                pos += 1;
                if !chars[pos..pos + len].iter().all(|c| c.is_ascii_hexdigit()) {
                    ok = false;
                    break;
                }
                pos += len;
            }
            if ok {
                return Some(chars[i..pos].iter().collect());
            }
        }
        i += 1;
    }
    None
}

// ============================================================================
// Drawing
// ============================================================================

fn draw(f: &mut Frame, app: &App) {
    let area = f.area();
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Min(5),    // Messages
            Constraint::Length(1), // Status
            Constraint::Length(1), // Separator
            Constraint::Length(3), // Input
        ])
        .split(area);

    // Messages
    draw_messages(f, app, chunks[0]);

    // Status
    draw_status(f, app, chunks[1]);

    // Separator
    let sep = Paragraph::new(Line::from(Span::styled(
        "─".repeat(chunks[2].width as usize),
        Style::default().fg(Color::DarkGray),
    )));
    f.render_widget(sep, chunks[2]);

    // Input
    draw_input(f, app, chunks[3]);
}

fn draw_messages(f: &mut Frame, app: &App, area: Rect) {
    let mut lines: Vec<Line> = Vec::new();
    let mut row: u16 = 0;

    // Get selection bounds (if any)
    let sel_start = app.sel_start.map(|(_, r)| r);
    let sel_end = app.sel_end.map(|(_, r)| r);
    let (sel_top, sel_bot) = match (sel_start, sel_end) {
        (Some(a), Some(b)) => (a.min(b), a.max(b)),
        _ => (u16::MAX, u16::MAX),
    };

    for msg in &app.messages {
        let (icon, style) = match msg.role {
            MessageRole::User => ("> ", Style::default().fg(Color::Green)),
            MessageRole::Assistant => ("🤖 ", Style::default().fg(Color::Blue)),
            MessageRole::System => ("ℹ ", Style::default().fg(Color::Yellow)),
            MessageRole::Signal => ("🔔 ", Style::default().fg(Color::Cyan)),
        };

        for (i, text_line) in msg.content.lines().enumerate() {
            let prefix = if i == 0 { icon } else { "  " };
            let prefix_len = prefix.len();

            // Check if this line is in selection range
            let is_selected = row >= sel_top && row <= sel_bot && sel_top != u16::MAX;

            if is_selected {
                // For selected lines, render with highlight
                let sel_col_start = if row == sel_top {
                    app.sel_start.map(|(c, _)| c).unwrap_or(0) as usize
                } else {
                    0
                };
                let sel_col_end = if row == sel_bot {
                    app.sel_end.map(|(c, _)| c).unwrap_or(text_line.len() as u16) as usize
                } else {
                    text_line.len()
                };

                // Normalize selection order (handle backwards selection)
                let (sel_start, sel_end) = if sel_col_start <= sel_col_end {
                    (sel_col_start, sel_col_end)
                } else {
                    (sel_col_end, sel_col_start)
                };

                let mut spans: Vec<Span> = Vec::new();
                spans.push(Span::raw(prefix));

                let sel_start_clamped = sel_start.min(text_line.len());
                let sel_end_clamped = sel_end.min(text_line.len());

                let before_sel = &text_line[..sel_start_clamped];
                let in_sel = &text_line[sel_start_clamped..sel_end_clamped];
                let after_sel = &text_line[sel_end_clamped..];

                if !before_sel.is_empty() {
                    spans.push(Span::styled(before_sel.to_string(), style));
                }
                if !in_sel.is_empty() {
                    spans.push(Span::styled(
                        in_sel.to_string(),
                        style.bg(Color::DarkGray),
                    ));
                }
                if !after_sel.is_empty() {
                    spans.push(Span::styled(after_sel.to_string(), style));
                }

                lines.push(Line::from(spans));
            } else {
                lines.push(Line::from(vec![
                    Span::raw(prefix),
                    Span::styled(text_line.to_string(), style),
                ]));
            }

            row += 1;
        }
        lines.push(Line::raw(""));
        row += 1;
    }

    // Pending indicator
    if !app.pending.is_empty() {
        lines.push(Line::from(vec![
            Span::styled(
                format!(
                    "{} Working... ({} pending, {}s)",
                    app.spinner(),
                    app.pending.len(),
                    app.oldest_secs()
                ),
                Style::default().fg(Color::Yellow).add_modifier(Modifier::ITALIC),
            ),
        ]));
    }

    let p = Paragraph::new(Text::from(lines))
        .scroll((app.scroll_offset as u16, 0))
        .block(
            Block::default()
                .borders(Borders::LEFT)
                .border_style(Style::default().fg(Color::DarkGray)),
        );
    f.render_widget(p, area);
}

fn draw_status(f: &mut Frame, app: &App, area: Rect) {
    let text = if !app.pending.is_empty() {
        format!(
            "{} {} pending | Enter: send | Scroll: Shift+↑↓ | Quit: Ctrl+C",
            app.spinner(),
            app.pending.len()
        )
    } else {
        format!(
            "Session: {} | Target: {} | Enter: send | Scroll: Shift+↑↓ | Quit: Ctrl+C",
            &app.session_id[..20.min(app.session_id.len())],
            app.target_hint
        )
    };

    let p = Paragraph::new(Span::styled(text, Style::default().fg(Color::DarkGray)));
    f.render_widget(p, area);
}

fn draw_input(f: &mut Frame, app: &App, area: Rect) {
    let mut spans = vec![Span::styled("> ", Style::default().fg(Color::Green))];

    if app.input.is_empty() {
        spans.push(Span::styled(
            " ",
            Style::default().bg(Color::White),
        ));
    } else {
        let before = &app.input[..app.cursor_pos];
        let after = &app.input[app.cursor_pos..];

        if !before.is_empty() {
            spans.push(Span::raw(before.to_string()));
        }
        spans.push(Span::styled(" ", Style::default().bg(Color::White)));
        if !after.is_empty() {
            spans.push(Span::raw(after.to_string()));
        }
    }

    let p = Paragraph::new(Line::from(spans))
        .block(Block::default().borders(Borders::TOP).border_style(Style::default().fg(Color::DarkGray)));
    f.render_widget(p, area);
}

// ============================================================================
// Main Entry Point
// ============================================================================

pub async fn handle_chat(config_path: &Path, args: &super::common::ChatArgs) -> anyhow::Result<()> {
    let config = autonoetic_gateway::config::load_config(config_path)?;
    let target_hint = args.agent_id.as_deref().unwrap_or("default-lead");
    let session_id = args
        .session_id
        .clone()
        .unwrap_or_else(|| format!("session-{}", &uuid::Uuid::new_v4().to_string()[..8]));
    let sender_id = args.sender_id.clone().unwrap_or_else(default_terminal_sender_id);
    let channel_id = args
        .channel_id
        .clone()
        .unwrap_or_else(|| default_terminal_channel_id(&sender_id, target_hint));
    let gateway_addr = format!("127.0.0.1:{}", config.port);

    // Connect
    let stream = TcpStream::connect(&gateway_addr).await.map_err(|e| {
        anyhow::anyhow!("Failed to connect to {}: {}", gateway_addr, e)
    })?;
    let (read_half, write_half) = stream.into_split();
    let mut gateway_lines = BufReader::new(read_half).lines();
    let envelope = terminal_channel_envelope(&channel_id, &sender_id, &session_id);
    let config = Arc::new(config);

    // Setup terminal
    enable_raw_mode()?;
    let mut stdout = std::io::stdout();
    execute!(stdout, EnterAlternateScreen, EnableMouseCapture)?;
    let backend = ratatui::backend::CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;
    terminal.clear()?;

    let mut app = App::new(session_id.clone(), target_hint.to_string());
    app.add_message(MessageRole::System, format!("Connected to {}", gateway_addr));

    // Channel for sending messages from TUI to gateway
    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<(u64, String)>();

    // Map gateway request IDs to internal IDs
    let mut pending_map: std::collections::HashMap<String, u64> = std::collections::HashMap::new();

    // Signal check interval
    let mut signal_interval = tokio::time::interval(Duration::from_secs(3));
    signal_interval.tick().await;

    // Main loop
    let result = run_loop(
        &mut terminal,
        &mut app,
        write_half,
        &mut gateway_lines,
        &config,
        &session_id,
        &envelope,
        &tx,
        &mut rx,
        &mut pending_map,
        &mut signal_interval,
    )
    .await;

    // Cleanup
    disable_raw_mode()?;
    execute!(terminal.backend_mut(), LeaveAlternateScreen, DisableMouseCapture)?;
    terminal.show_cursor()?;

    result
}

#[allow(clippy::too_many_arguments)]
async fn run_loop<B: ratatui::backend::Backend>(
    terminal: &mut Terminal<B>,
    app: &mut App,
    mut write_half: tokio::net::tcp::OwnedWriteHalf,
    gateway_lines: &mut tokio::io::Lines<tokio::io::BufReader<tokio::net::tcp::OwnedReadHalf>>,
    config: &autonoetic_types::config::GatewayConfig,
    session_id: &str,
    envelope: &serde_json::Value,
    tx: &tokio::sync::mpsc::UnboundedSender<(u64, String)>,
    rx: &mut tokio::sync::mpsc::UnboundedReceiver<(u64, String)>,
    pending_map: &mut std::collections::HashMap<String, u64>,
    signal_interval: &mut tokio::time::Interval,
) -> anyhow::Result<()> {
    let mut needs_redraw = true;
    let mut last_spinner_tick = Instant::now();

    loop {
        // Tick spinner every 100ms (only when needed for redraw)
        if last_spinner_tick.elapsed() > Duration::from_millis(100) {
            app.tick_spinner();
            last_spinner_tick = Instant::now();
            needs_redraw = true;
        }

        // Only draw when something changed
        if needs_redraw {
            terminal.draw(|f| draw(f, app))?;
            needs_redraw = false;
        }

        // Use tokio::select to handle async events
        tokio::select! {
            // Gateway response (highest priority)
            result = gateway_lines.next_line() => {
                match result {
                    Ok(Some(line)) => {
                        if let Ok(resp) = serde_json::from_str::<GatewayJsonRpcResponse>(&line) {
                            if let Some(internal_id) = pending_map.remove(&resp.id) {
                                app.remove_pending(internal_id);

                                if let Some(error) = resp.error {
                                    app.add_message(MessageRole::System, format!("Error: {}", error.message));
                                } else {
                                    let reply = resp
                                        .result
                                        .and_then(|v| v.get("assistant_reply").and_then(|r| r.as_str().map(ToOwned::to_owned)))
                                        .unwrap_or_else(|| "[No response]".to_string());

                                    if let Some(req_id) = extract_approval_request_id(&reply) {
                                        app.add_message(MessageRole::Signal, format!("Approval required: {}", req_id));
                                    }

                                    app.add_message(MessageRole::Assistant, reply);
                                }
                                needs_redraw = true;
                            }
                        }
                    }
                    Ok(None) => {
                        app.add_message(MessageRole::System, "Gateway disconnected".to_string());
                        needs_redraw = true;
                        break;
                    }
                    Err(e) => {
                        app.add_message(MessageRole::System, format!("Gateway error: {}", e));
                        needs_redraw = true;
                        break;
                    }
                }
            }

            // User message to send
            msg = rx.recv() => {
                if let Some((id, message)) = msg {
                    let req_id = format!("tui-{}", id);
                    pending_map.insert(req_id.clone(), id);

                    let params = serde_json::json!({
                        "event_type": "chat",
                        "message": message,
                        "session_id": session_id,
                        "metadata": envelope,
                    });

                    let request = GatewayJsonRpcRequest {
                        jsonrpc: "2.0".to_string(),
                        id: req_id,
                        method: "event.ingest".to_string(),
                        params,
                    };

                    let encoded = serde_json::to_string(&request)?;
                    write_half.write_all(encoded.as_bytes()).await?;
                    write_half.write_all(b"\n").await?;
                    write_half.flush().await?;
                    needs_redraw = true;
                }
            }

            // Signal check
            _ = signal_interval.tick() => {
                if check_signals(app, config, session_id).await {
                    needs_redraw = true;
                }
            }

            // TUI input - poll with short timeout for responsive UI
            _ = tokio::time::sleep(Duration::from_millis(16)) => {  // ~60fps
                // Drain all pending crossterm events
                while event::poll(Duration::ZERO)? {
                    match event::read()? {
                        Event::Key(key) => {
                            if !handle_key(key, app, tx)? {
                                return Ok(()); // Quit
                            }
                            needs_redraw = true;
                        }
                        Event::Mouse(mouse) => {
                            let redraw = handle_mouse(mouse, app);
                            needs_redraw = needs_redraw || redraw;
                        }
                        Event::Resize(_, _) => {
                            needs_redraw = true;
                        }
                        _ => {}
                    }
                }
            }
        }
    }

    Ok(())
}

fn handle_mouse(mouse: crossterm::event::MouseEvent, app: &mut App) -> bool {
    match mouse.kind {
        crossterm::event::MouseEventKind::ScrollUp => {
            app.scroll_offset += 3;
            true
        }
        crossterm::event::MouseEventKind::ScrollDown => {
            app.scroll_offset = app.scroll_offset.saturating_sub(3);
            true
        }
        crossterm::event::MouseEventKind::Down(btn) => {
            if btn == crossterm::event::MouseButton::Left {
                app.selecting = true;
                app.sel_start = Some((mouse.column, mouse.row));
                app.sel_end = Some((mouse.column, mouse.row));
                true
            } else {
                false
            }
        }
        crossterm::event::MouseEventKind::Up(btn) => {
            if btn == crossterm::event::MouseButton::Left && app.selecting {
                app.sel_end = Some((mouse.column, mouse.row));
                app.selecting = false;
                copy_selection_to_clipboard(app);
                true
            } else {
                false
            }
        }
        crossterm::event::MouseEventKind::Drag(btn) => {
            if btn == crossterm::event::MouseButton::Left && app.selecting {
                app.sel_end = Some((mouse.column, mouse.row));
                true // Need redraw to show selection highlight
            } else {
                false
            }
        }
        _ => false,
    }
}

fn handle_key(
    key: crossterm::event::KeyEvent,
    app: &mut App,
    tx: &tokio::sync::mpsc::UnboundedSender<(u64, String)>,
) -> anyhow::Result<bool> {
    match key.code {
        // Quit
        KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => return Ok(false),

        // Send
        KeyCode::Enter => {
            if !app.input.is_empty() {
                let msg = std::mem::take(&mut app.input);
                app.cursor_pos = 0;
                let id = app.next_id();
                app.add_pending(id);
                app.add_message(MessageRole::User, msg.clone());
                let _ = tx.send((id, msg));
            }
        }

        // Cursor
        KeyCode::Left => app.cursor_left(),
        KeyCode::Right => app.cursor_right(),
        KeyCode::Home => app.cursor_pos = 0,
        KeyCode::End => app.cursor_pos = app.input.len(),

        // Delete
        KeyCode::Backspace => app.delete_char(),
        KeyCode::Delete => {
            if app.cursor_pos < app.input.len() {
                app.input.remove(app.cursor_pos);
            }
        }

        // Type
        KeyCode::Char(c) if !key.modifiers.contains(KeyModifiers::CONTROL) => {
            app.insert_char(c);
        }

        // Scroll (Shift or Ctrl)
        KeyCode::Up if key.modifiers.contains(KeyModifiers::SHIFT) || key.modifiers.contains(KeyModifiers::CONTROL) => {
            app.scroll_offset += 3;
        }
        KeyCode::Down if key.modifiers.contains(KeyModifiers::SHIFT) || key.modifiers.contains(KeyModifiers::CONTROL) => {
            app.scroll_offset = app.scroll_offset.saturating_sub(3);
        }

        _ => {}
    }

    Ok(true)
}

/// Check for signals and inject into app. Returns true if signals were processed.
async fn check_signals(
    app: &mut App,
    config: &autonoetic_types::config::GatewayConfig,
    session_id: &str,
) -> bool {
    let gateway_dir = config.agents_dir.join(".gateway");
    let Ok(signals) = autonoetic_gateway::scheduler::signal::check_pending_signals(&gateway_dir, session_id) else {
        return false;
    };
    
    if signals.is_empty() {
        return false;
    }
    
    for pending in signals {
        match &pending.signal {
            autonoetic_gateway::scheduler::signal::Signal::ApprovalResolved { request_id, agent_id, status, .. } => {
                let icon = if status == "approved" { "✅" } else { "❌" };
                app.add_message(
                    MessageRole::Signal,
                    format!("{} Approval {} for {}", icon, status, agent_id),
                );
                let _ = autonoetic_gateway::scheduler::signal::consume_signal(&gateway_dir, session_id, request_id);
            }
        }
    }
    true
}

/// Copy the selected text region to clipboard.
/// Mouse coordinates are absolute to terminal - we need to map to content.
fn copy_selection_to_clipboard(app: &App) {
    let (Some(start), Some(end)) = (app.sel_start, app.sel_end) else {
        return;
    };

    // Determine selection bounds (handle both directions)
    let (top, bottom) = if start.1 <= end.1 {
        (start, end)
    } else {
        (end, start)
    };

    // Build list of ALL rendered lines (including scrollable content)
    let mut lines: Vec<String> = Vec::new();
    for msg in &app.messages {
        let icon = match msg.role {
            MessageRole::User => "> ",
            MessageRole::Assistant => "  ",
            MessageRole::System => "  ",
            MessageRole::Signal => "  ",
        };
        for (i, line) in msg.content.lines().enumerate() {
            let prefix = if i == 0 { icon } else { "  " };
            lines.push(format!("{}{}", prefix, line));
        }
        lines.push(String::new()); // Empty line between messages
    }
    if !app.pending.is_empty() {
        lines.push(format!("{} Working...", app.spinner()));
    }

    // The messages area starts at row 0 (first chunk after layout).
    // Account for scroll offset to map screen rows to content rows.
    let screen_start_row = top.1 as usize;
    let screen_end_row = bottom.1 as usize;
    let start_col = top.0 as usize;
    let end_col = bottom.0 as usize;

        // Row 0 on screen = row `scroll_offset` in content
        let content_start = app.scroll_offset;

    let mut selected = Vec::new();

    for screen_row in screen_start_row..=screen_end_row {
        let content_row = content_start + screen_row;
        if content_row >= lines.len() {
            break;
        }
        let line = &lines[content_row];

        if screen_row == screen_start_row && screen_row == screen_end_row {
            // Single line selection
            let col_start = start_col.min(line.len());
            let col_end = end_col.min(line.len());
            if col_end > col_start {
                selected.push(line[col_start..col_end].to_string());
            }
        } else if screen_row == screen_start_row {
            // First line
            let col_start = start_col.min(line.len());
            selected.push(line[col_start..].to_string());
        } else if screen_row == screen_end_row {
            // Last line
            let col_end = end_col.min(line.len());
            selected.push(line[..col_end].to_string());
        } else {
            // Middle lines
            selected.push(line.clone());
        }
    }

    let selected_text = selected.join("\n");
    if !selected_text.is_empty() {
        if let Ok(mut clipboard) = Clipboard::new() {
            let _ = clipboard.set_text(&selected_text);
        }
    }
}
