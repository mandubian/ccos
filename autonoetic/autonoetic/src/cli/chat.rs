//! TUI Chat interface using ratatui + crossterm.

use std::collections::{HashMap, HashSet};
use std::path::Path;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

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

use super::agent::format_llm_usage_for_cli;
use super::common::{
    default_terminal_channel_id, default_terminal_sender_id, terminal_channel_envelope,
};
use autonoetic_types::agent::LlmExchangeUsage;
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

#[derive(Debug, Clone)]
struct SignalResumeRef {
    signal_session_id: String,
    request_id: String,
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
    // Mouse selection - stored as CONTENT positions (row, col), not screen positions
    selecting: bool,
    sel_start: Option<(usize, usize)>,  // (content_row, content_col)
    sel_end: Option<(usize, usize)>,     // (content_row, content_col)
    signal_resume_by_internal_id: HashMap<u64, SignalResumeRef>,
    signal_resume_inflight: HashSet<String>,
    awaiting_approvals: HashSet<String>,
    announced_pending_approvals: HashSet<String>,
    seen_workflow_event_ids: HashSet<String>,
    workflow_events_bootstrapped: bool,
    workflow_status_line: String,
    // Persistent clipboard — must stay alive so arboard's background ownership
    // thread keeps running and clipboard managers have time to capture the content.
    clipboard: Option<arboard::Clipboard>,
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
            signal_resume_by_internal_id: HashMap::new(),
            signal_resume_inflight: HashSet::new(),
            awaiting_approvals: HashSet::new(),
            announced_pending_approvals: HashSet::new(),
            seen_workflow_event_ids: HashSet::new(),
            workflow_events_bootstrapped: false,
            workflow_status_line: "workflow: n/a".to_string(),
            // Safe clipboard initialization - arboard can panic on headless/SSH systems
            clipboard: std::panic::catch_unwind(|| arboard::Clipboard::new().ok()).unwrap_or(None),
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

    fn add_awaiting_approval(&mut self, request_id: String) {
        self.awaiting_approvals.insert(request_id);
    }



    fn awaiting_approval_preview(&self) -> String {
        if self.awaiting_approvals.is_empty() {
            return String::new();
        }
        let mut ids: Vec<&str> = self.awaiting_approvals.iter().map(|s| s.as_str()).collect();
        ids.sort_unstable();
        let shown: Vec<&str> = ids.iter().take(2).copied().collect();
        if ids.len() > shown.len() {
            format!("{}, +{}", shown.join(", "), ids.len() - shown.len())
        } else {
            shown.join(", ")
        }
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

fn hydrate_session_history(
    app: &mut App,
    config: &autonoetic_types::config::GatewayConfig,
    session_id: &str,
) -> anyhow::Result<usize> {
    let gateway_dir = config.agents_dir.join(".gateway");
    let store = autonoetic_gateway::runtime::content_store::ContentStore::new(&gateway_dir)?;
    let handle = match store.resolve_name_with_root(session_id, "session_history") {
        Ok(handle) => handle,
        Err(_) => return Ok(0),
    };

    let history_json = store.read_string(&handle)?;
    let history: Vec<autonoetic_gateway::llm::Message> = serde_json::from_str(&history_json)
        .map_err(|e| anyhow::anyhow!("Invalid session_history payload for {}: {}", session_id, e))?;

    let mut restored = 0usize;
    for msg in history {
        match msg.role {
            autonoetic_gateway::llm::Role::User => {
                if !msg.content.trim().is_empty() {
                    app.add_message(MessageRole::User, msg.content);
                    restored += 1;
                }
            }
            autonoetic_gateway::llm::Role::Assistant => {
                if !msg.content.trim().is_empty() {
                    app.add_message(MessageRole::Assistant, msg.content);
                    restored += 1;
                }
            }
            autonoetic_gateway::llm::Role::System => {
                if !msg.content.trim().is_empty() {
                    app.add_message(MessageRole::System, msg.content);
                    restored += 1;
                }
            }
            autonoetic_gateway::llm::Role::Tool => {}
        }
    }

    Ok(restored)
}

fn signal_resume_key(signal_session_id: &str, request_id: &str) -> String {
    format!("{}::{}", signal_session_id, request_id)
}



fn format_workflow_event_card(event: &autonoetic_types::workflow::WorkflowEventRecord) -> Option<String> {
    let ts_short: String = event.occurred_at.chars().take(19).collect();
    let task = event.task_id.as_deref().unwrap_or("-");
    let status = event
        .payload
        .get("status")
        .and_then(|v| v.as_str())
        .unwrap_or("");

    let text = match event.event_type.as_str() {
        "workflow.started" => Some(format!("📋 [{}] Workflow started", ts_short)),
        "task.spawned" => Some(format!("🚀 [{}] Task spawned: {}", ts_short, task)),
        "task.awaiting_approval" => Some(format!("⏸ [{}] Task awaiting approval: {}", ts_short, task)),
        "task.started" => Some(format!("▶ [{}] Task started: {}", ts_short, task)),
        "task.completed" => Some(format!("✅ [{}] Task completed: {}", ts_short, task)),
        "task.failed" => Some(format!("❌ [{}] Task failed: {}", ts_short, task)),
        "workflow.join.satisfied" => Some(format!("✅ [{}] Workflow join satisfied", ts_short)),
        "task.updated" if status == "runnable" => {
            Some(format!("🔁 [{}] Task resumed after approval: {}", ts_short, task))
        }
        _ => None,
    };

    text
}

fn refresh_workflow_status_line(
    app: &mut App,
    config: &autonoetic_types::config::GatewayConfig,
    root_session_id: &str,
) {
    tracing::debug!(
        target: "chat",
        agents_dir = %config.agents_dir.display(),
        root_session_id = %root_session_id,
        "refresh_workflow_status_line: resolving workflow"
    );
    let resolved = autonoetic_gateway::scheduler::resolve_workflow_id_for_root_session(
        config,
        root_session_id,
    );
    match &resolved {
        Ok(Some(wf_id)) => {
            tracing::debug!(target: "chat", workflow_id = %wf_id, "refresh_workflow_status_line: found workflow");
        }
        Ok(None) => {
            tracing::debug!(target: "chat", "refresh_workflow_status_line: no workflow found");
            // Show helpful hint when no workflow found
            if app.workflow_status_line.starts_with("workflow: n/a") {
                app.workflow_status_line = format!(
                    "workflow: n/a (session: {})",
                    if root_session_id.len() > 16 {
                        format!("{}...", &root_session_id[..16])
                    } else {
                        root_session_id.to_string()
                    }
                );
            }
            return;
        }
        Err(e) => {
            tracing::warn!(target: "chat", error = %e, "refresh_workflow_status_line: resolution failed");
            app.workflow_status_line = format!("workflow: error ({})", e);
            return;
        }
    }
    let Some(workflow_id) = resolved.ok().flatten() else {
        return;
    };

    let status = autonoetic_gateway::scheduler::load_workflow_run(config, None, &workflow_id)
        .ok()
        .flatten()
        .map(|run| format!("{:?}", run.status).to_lowercase())
        .unwrap_or_else(|| "unknown".to_string());

    let mut running = 0usize;
    let mut queued = 0usize;
    let mut awaiting = 0usize;
    let mut done = 0usize;

    if let Ok(tasks) = autonoetic_gateway::scheduler::list_task_runs_for_workflow(config, None, &workflow_id) {
        for t in tasks {
            match t.status {
                autonoetic_types::workflow::TaskRunStatus::Pending => queued += 1,
                autonoetic_types::workflow::TaskRunStatus::Runnable
                | autonoetic_types::workflow::TaskRunStatus::Running => running += 1,
                autonoetic_types::workflow::TaskRunStatus::AwaitingApproval => awaiting += 1,
                autonoetic_types::workflow::TaskRunStatus::Succeeded
                | autonoetic_types::workflow::TaskRunStatus::Failed
                | autonoetic_types::workflow::TaskRunStatus::Cancelled => done += 1,
                autonoetic_types::workflow::TaskRunStatus::Paused => {}
            }
        }
    }

    app.workflow_status_line = format!(
        "wf:{} {} | run:{} queue:{} wait:{} done:{}",
        workflow_id,
        status,
        running,
        queued,
        awaiting,
        done
    );
}

// ============================================================================
// Approval request id extraction (apr-* and UUID fallback)
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
            if let Some(request_id) = extract_request_id(after) {
                return Some(request_id);
            }
        }
    }
    extract_request_id(text)
}

fn extract_request_id(text: &str) -> Option<String> {
    extract_short_approval_id(text).or_else(|| extract_uuid(text))
}

fn extract_short_approval_id(text: &str) -> Option<String> {
    let chars: Vec<char> = text.chars().collect();
    let mut i = 0;
    while i + 4 <= chars.len() {
        let is_prefix = chars[i].eq_ignore_ascii_case(&'a')
            && chars[i + 1].eq_ignore_ascii_case(&'p')
            && chars[i + 2].eq_ignore_ascii_case(&'r')
            && chars[i + 3] == '-';
        if !is_prefix {
            i += 1;
            continue;
        }

        let mut j = i + 4;
        while j < chars.len() && chars[j].is_ascii_hexdigit() {
            j += 1;
        }

        // Current approval IDs are short ids like apr-1234abcd.
        if j >= i + 12 {
            let before_ok = i == 0 || !chars[i - 1].is_ascii_alphanumeric();
            let after_ok = j == chars.len() || !chars[j].is_ascii_alphanumeric();
            if before_ok && after_ok {
                return Some(chars[i..j].iter().collect());
            }
        }

        i += 1;
    }
    None
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

#[derive(Debug, Clone)]
struct StructuredApprovalView {
    request_id: Option<String>,
    card: String,
}

fn json_array_to_csv(value: Option<&serde_json::Value>) -> Option<String> {
    let Some(serde_json::Value::Array(values)) = value else {
        return None;
    };
    let parts: Vec<String> = values
        .iter()
        .filter_map(|v| v.as_str().map(ToOwned::to_owned))
        .collect();
    if parts.is_empty() {
        None
    } else {
        Some(parts.join(", "))
    }
}

fn extract_structured_approval(text: &str) -> Option<StructuredApprovalView> {
    let parsed: serde_json::Value = serde_json::from_str(text).ok()?;
    let approval = parsed.get("approval")?;
    let kind = approval
        .get("kind")
        .and_then(|v| v.as_str())
        .unwrap_or("unknown");
    let summary = approval
        .get("summary")
        .and_then(|v| v.as_str())
        .unwrap_or("Approval required");
    let reason = approval
        .get("reason")
        .and_then(|v| v.as_str())
        .unwrap_or("Operator approval required");
    let retry_field = approval
        .get("retry_field")
        .and_then(|v| v.as_str())
        .unwrap_or("approval_ref");
    let request_id = parsed
        .get("request_id")
        .and_then(|v| v.as_str())
        .map(ToOwned::to_owned);

    let subject = approval.get("subject").cloned().unwrap_or_default();
    let mut details = Vec::new();
    match kind {
        "sandbox_exec" => {
            if let Some(command) = subject.get("command").and_then(|v| v.as_str()) {
                details.push(format!("command: {}", command));
            }
            if let Some(hosts) = json_array_to_csv(subject.get("hosts")) {
                details.push(format!("hosts: {}", hosts));
            }
            if let Some(deps) = subject.get("dependencies") {
                let runtime = deps.get("runtime").and_then(|v| v.as_str()).unwrap_or("-");
                let packages = json_array_to_csv(deps.get("packages")).unwrap_or_default();
                if !packages.is_empty() {
                    details.push(format!("deps: {} ({})", runtime, packages));
                } else {
                    details.push(format!("deps: {}", runtime));
                }
            }
        }
        "agent_install" => {
            if let Some(agent_id) = subject.get("agent_id").and_then(|v| v.as_str()) {
                details.push(format!("agent: {}", agent_id));
            }
            if let Some(artifact_id) = subject.get("artifact_id").and_then(|v| v.as_str()) {
                details.push(format!("artifact: {}", artifact_id));
            }
            if let Some(risk_factors) = json_array_to_csv(subject.get("risk_factors")) {
                details.push(format!("risk: {}", risk_factors));
            }
            if let Some(capabilities) = json_array_to_csv(subject.get("capabilities")) {
                details.push(format!("capabilities: {}", capabilities));
            }
        }
        _ => {}
    }

    let mut lines = Vec::new();
    lines.push(format!(
        "Approval required{}",
        request_id
            .as_ref()
            .map(|id| format!(": {}", id))
            .unwrap_or_default()
    ));
    lines.push(format!("kind: {}", kind));
    lines.push(format!("summary: {}", summary));
    lines.push(format!("reason: {}", reason));
    if !details.is_empty() {
        lines.push(format!("subject: {}", details.join(" | ")));
    }
    lines.push(format!("retry field: {}", retry_field));

    Some(StructuredApprovalView {
        request_id,
        card: lines.join("\n"),
    })
}

// ============================================================================
// Drawing
// ============================================================================

fn draw(f: &mut Frame, app: &App) {
    let area = f.area();
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1), // Status
            Constraint::Length(1), // Separator
            Constraint::Min(5),    // Messages
            Constraint::Length(3), // Input
        ])
        .split(area);

    // Status
    draw_status(f, app, chunks[0]);

    // Separator
    let sep = Paragraph::new(Line::from(Span::styled(
        "─".repeat(chunks[1].width as usize),
        Style::default().fg(Color::DarkGray),
    )));
    f.render_widget(sep, chunks[1]);

    // Messages
    draw_messages(f, app, chunks[2]);

    // Input
    draw_input(f, app, chunks[3]);

    // Pin the terminal cursor inside the input box so it never wanders to the
    // last mouse position during a drag-selection.
    // Layout: top border = +1 row, "> " prefix = +2 cols, cursor_pos = byte offset.
    let before_cursor_display_width = app.input[..app.cursor_pos].chars().count() as u16;
    let cursor_x = (chunks[3].x + 2 + before_cursor_display_width)
        .min(chunks[3].x + chunks[3].width.saturating_sub(1));
    let cursor_y = chunks[3].y + 1;
    f.set_cursor_position((cursor_x, cursor_y));
}

fn draw_messages(f: &mut Frame, app: &App, area: Rect) {
    let mut lines: Vec<Line> = Vec::new();
    // `row` is the absolute content-line index (0 = very first line of all messages).
    let mut row: usize = 0;

    // Selection bounds are stored as CONTENT coordinates (content_row, content_col).
    let (content_sel_top, content_sel_bot, sel_col_start_override, sel_col_end_override) = 
        match (app.sel_start, app.sel_end) {
        (Some((r1, c1)), Some((r2, c2))) => {
            let lo_row = r1.min(r2);
            let hi_row = r1.max(r2);
            let lo_col = c1.min(c2);
            let hi_col = c1.max(c2);
            (lo_row, hi_row, lo_col, hi_col)
        }
        _ => (usize::MAX, usize::MAX, 0, 0),
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

            // Compare content row against selection bounds.
            let is_selected =
                row >= content_sel_top && row <= content_sel_bot && content_sel_top != usize::MAX;

            if is_selected {
                // For selected lines, render with highlight.
                // Column bounds only apply at the first and last selected lines.
                let sel_col_start = if row == content_sel_top {
                    sel_col_start_override
                } else {
                    0
                };
                let sel_col_end = if row == content_sel_bot {
                    sel_col_end_override
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
                    spans.push(Span::styled(in_sel.to_string(), style.bg(Color::DarkGray)));
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

            row = row.saturating_add(1);
        }
        lines.push(Line::raw(""));
        row = row.saturating_add(1);
    }

    // Pending indicator
    if !app.pending.is_empty() {
        lines.push(Line::from(vec![Span::styled(
            format!(
                "{} Working... ({} pending, {}s)",
                app.spinner(),
                app.pending.len(),
                app.oldest_secs()
            ),
            Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::ITALIC),
        )]));
    }
    if !app.awaiting_approvals.is_empty() {
        lines.push(Line::from(vec![Span::styled(
            format!(
                "⏸ Waiting operator approval ({}): {}",
                app.awaiting_approvals.len(),
                app.awaiting_approval_preview()
            ),
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::ITALIC),
        )]));
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
    let workflow = &app.workflow_status_line;
    let text = if !app.pending.is_empty() {
        let waiting = if app.awaiting_approvals.is_empty() {
            String::new()
        } else {
            format!(" | waiting approvals: {}", app.awaiting_approvals.len())
        };
        format!(
            "{} {} pending{} | {} | Enter: send | Scroll: Shift+↑↓ | Quit: Ctrl+C",
            app.spinner(),
            app.pending.len(),
            waiting,
            workflow,
        )
    } else if !app.awaiting_approvals.is_empty() {
        format!(
            "Waiting approvals: {} ({}) | {} | Enter: send | Scroll: Shift+↑↓ | Quit: Ctrl+C",
            app.awaiting_approvals.len(),
            app.awaiting_approval_preview(),
            workflow,
        )
    } else {
        format!(
            "Session: {} | Target: {} | {} | Enter: send | Scroll: Shift+↑↓ | Quit: Ctrl+C",
            &app.session_id[..20.min(app.session_id.len())],
            app.target_hint,
            workflow,
        )
    };

    let p = Paragraph::new(Span::styled(text, Style::default().fg(Color::DarkGray)));
    f.render_widget(p, area);
}

fn draw_input(f: &mut Frame, app: &App, area: Rect) {
    let mut spans = vec![Span::styled("> ", Style::default().fg(Color::Green))];

    if app.input.is_empty() {
        spans.push(Span::styled(" ", Style::default().bg(Color::White)));
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

    let p = Paragraph::new(Line::from(spans)).block(
        Block::default()
            .borders(Borders::TOP)
            .border_style(Style::default().fg(Color::DarkGray)),
    );
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
    let sender_id = args
        .sender_id
        .clone()
        .unwrap_or_else(default_terminal_sender_id);
    let channel_id = args
        .channel_id
        .clone()
        .unwrap_or_else(|| default_terminal_channel_id(&sender_id, target_hint));
    let gateway_addr = format!("127.0.0.1:{}", config.port);

    // Connect handling is mostly inside the loop.
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
    if let Ok(restored) = hydrate_session_history(&mut app, config.as_ref(), &session_id) {
        if restored > 0 {
            app.add_message(
                MessageRole::System,
                format!("Restored {} message(s) from previous session history", restored),
            );
        }
    }
    
    // Show session info and workflow hint
    let root_session = autonoetic_gateway::runtime::content_store::root_session_id(&session_id);
    app.add_message(
        MessageRole::System,
        format!("Session: {} (root: {})", session_id, root_session),
    );
    
    // Check if workflow exists for this session
    if let Ok(Some(wf_id)) = autonoetic_gateway::scheduler::resolve_workflow_id_for_root_session(&config, root_session) {
        app.add_message(
            MessageRole::System,
            format!("🔗 Connected to workflow: {}", wf_id),
        );
    } else {
        app.add_message(
            MessageRole::System,
            format!("ℹ No workflow found for root session '{}'. Use --session-id to connect to an existing workflow.", root_session),
        );
    }
    
    app.add_message(
        MessageRole::System,
        format!("Connecting to {}...", gateway_addr),
    );

    // Channel for sending messages from TUI to gateway
    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<(u64, String)>();

    // Map gateway request IDs to internal IDs
    let mut pending_map: std::collections::HashMap<String, u64> = std::collections::HashMap::new();

    // Signal check interval
    let mut signal_interval = tokio::time::interval(Duration::from_secs(1));
    signal_interval.tick().await;

    // Open gateway store for approvals and signals (same path as gateway daemon)
    let gateway_dir = autonoetic_gateway::execution::gateway_root_dir(config.as_ref());
    let gateway_store = match autonoetic_gateway::scheduler::gateway_store::GatewayStore::open(&gateway_dir) {
        Ok(store) => {
            app.add_message(
                MessageRole::System,
                format!("✓ Gateway store connected: {}", gateway_dir.display()),
            );
            Some(store)
        }
        Err(e) => {
            app.add_message(
                MessageRole::System,
                format!("⚠ Gateway store unavailable: {} (approvals may not be visible)", e),
            );
            None
        }
    };

    // Main loop
    loop {
        // Connect
        let stream = match TcpStream::connect(&gateway_addr).await {
            Ok(s) => s,
            Err(e) => {
                app.add_message(MessageRole::System, format!("Gateway connection failed (reconnecting in 3s): {}", e));
                terminal.draw(|f| draw(f, &app))?;
                tokio::time::sleep(Duration::from_secs(3)).await;
                continue;
            }
        };
        let (read_half, write_half) = stream.into_split();
        let mut gateway_lines = BufReader::new(read_half).lines();

        let disconnected = run_loop(
            &mut terminal,
            &mut app,
            write_half,
            &mut gateway_lines,
            &config,
            gateway_store.as_ref(),
            &session_id,
            &envelope,
            &tx,
            &mut rx,
            &mut pending_map,
            &mut signal_interval,
        )
        .await?;

        if !disconnected {
            break; // User quit explicitly
        }

        app.add_message(MessageRole::System, "Gateway disconnected, reconnecting in 3s...".to_string());
        terminal.draw(|f| draw(f, &app))?;
        tokio::time::sleep(Duration::from_secs(3)).await;
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

#[allow(clippy::too_many_arguments)]
async fn run_loop<B: ratatui::backend::Backend>(
    terminal: &mut Terminal<B>,
    app: &mut App,
    mut write_half: tokio::net::tcp::OwnedWriteHalf,
    gateway_lines: &mut tokio::io::Lines<tokio::io::BufReader<tokio::net::tcp::OwnedReadHalf>>,
    config: &autonoetic_types::config::GatewayConfig,
    gateway_store: Option<&autonoetic_gateway::scheduler::gateway_store::GatewayStore>,
    session_id: &str,
    envelope: &serde_json::Value,
    tx: &tokio::sync::mpsc::UnboundedSender<(u64, String)>,
    rx: &mut tokio::sync::mpsc::UnboundedReceiver<(u64, String)>,
    pending_map: &mut std::collections::HashMap<String, u64>,
    signal_interval: &mut tokio::time::Interval,
) -> anyhow::Result<bool> {
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
            biased;

            // Signal check always gets priority to avoid starvation
            _ = signal_interval.tick() => {
                if check_signals(app, config, gateway_store, session_id, tx).await {
                    needs_redraw = true;
                }
            }

            // Gateway response
            result = gateway_lines.next_line() => {
                match result {
                    Ok(Some(line)) => {
                        if let Ok(resp) = serde_json::from_str::<GatewayJsonRpcResponse>(&line) {
                            if let Some(internal_id) = pending_map.remove(&resp.id) {
                                app.remove_pending(internal_id);
                                let signal_resume_ref =
                                    app.signal_resume_by_internal_id.remove(&internal_id);
                                if let Some(resume_ref) = &signal_resume_ref {
                                    app.signal_resume_inflight.remove(&signal_resume_key(
                                        &resume_ref.signal_session_id,
                                        &resume_ref.request_id,
                                    ));
                                }

                                if let Some(error) = resp.error {
                                    app.add_message(MessageRole::System, format!("Error: {}", error.message));
                                } else {
                                    let result_json = resp.result.as_ref();
                                    let reply = result_json
                                        .and_then(|v| v.get("assistant_reply").and_then(|r| r.as_str().map(ToOwned::to_owned)))
                                        .unwrap_or_else(|| "[No response]".to_string());

                                    if let Some(structured) = extract_structured_approval(&reply) {
                                        if let Some(req_id) = structured.request_id.clone() {
                                            app.add_awaiting_approval(req_id);
                                        }
                                        app.add_message(MessageRole::Signal, structured.card);
                                    } else if let Some(req_id) = extract_approval_request_id(&reply) {
                                        app.add_awaiting_approval(req_id.clone());
                                        app.add_message(
                                            MessageRole::Signal,
                                            format!("Approval required: {}", req_id),
                                        );
                                    }

                                    app.add_message(MessageRole::Assistant, reply);

                                    if let Some(arr) =
                                        result_json.and_then(|v| v.get("llm_usage"))
                                    {
                                        if let Ok(usages) =
                                            serde_json::from_value::<Vec<LlmExchangeUsage>>(arr.clone())
                                        {
                                            if let Some(text) = format_llm_usage_for_cli(&usages) {
                                                app.add_message(MessageRole::System, text);
                                            }
                                        }
                                    }


                                }
                                needs_redraw = true;
                            }
                        }
                    }
                    Ok(None) => {
                        return Ok(true); // Disconnected
                    }
                    Err(e) => {
                        app.add_message(MessageRole::System, format!("Gateway error: {}", e));
                        return Ok(true); // Disconnected
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

            // TUI input - poll with short timeout for responsive UI
            _ = tokio::time::sleep(Duration::from_millis(16)) => {  // ~60fps
                // Drain all pending crossterm events
                while event::poll(Duration::ZERO)? {
                    match event::read()? {
                        Event::Key(key) => {
                            if !handle_key(key, app, tx)? {
                                return Ok(false); // Clean Quit
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
    // Loop only exits via returns
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
                // Only start selection if clicking in messages area (row >= 2)
                if mouse.row >= 2 {
                    // Convert screen coordinates to content coordinates
                    // Layout: status (1 row) + separator (1 row) = messages start at row 2
                    // Messages widget has left border (1 col) + prefix (2 cols) = text at col 3
                    let content_row = (mouse.row as usize - 2) + app.scroll_offset;
                    let content_col = (mouse.column as usize).saturating_sub(3);
                    app.selecting = true;
                    app.sel_start = Some((content_row, content_col));
                    app.sel_end = Some((content_row, content_col));
                    true
                } else {
                    // Clicked on status or separator - clear any existing selection
                    if app.sel_start.is_some() || app.sel_end.is_some() {
                        app.sel_start = None;
                        app.sel_end = None;
                        true
                    } else {
                        false
                    }
                }
            } else {
                false
            }
        }
        crossterm::event::MouseEventKind::Up(btn) => {
            if btn == crossterm::event::MouseButton::Left && app.selecting {
                // Only complete selection if mouse is in messages area
                if mouse.row >= 2 {
                    let content_row = (mouse.row as usize - 2) + app.scroll_offset;
                    let content_col = (mouse.column as usize).saturating_sub(3);
                    app.sel_end = Some((content_row, content_col));
                    app.selecting = false;
                    copy_selection_to_clipboard(app);
                } else {
                    // Mouse released outside messages area - cancel selection
                    app.selecting = false;
                    app.sel_start = None;
                    app.sel_end = None;
                }
                true
            } else {
                false
            }
        }
        crossterm::event::MouseEventKind::Drag(btn) => {
            if btn == crossterm::event::MouseButton::Left && app.selecting {
                // Only update if in messages area
                if mouse.row >= 2 {
                    let content_row = (mouse.row as usize - 2) + app.scroll_offset;
                    let content_col = (mouse.column as usize).saturating_sub(3);
                    app.sel_end = Some((content_row, content_col));
                }
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
        KeyCode::Up
            if key.modifiers.contains(KeyModifiers::SHIFT)
                || key.modifiers.contains(KeyModifiers::CONTROL) =>
        {
            app.scroll_offset += 3;
        }
        KeyCode::Down
            if key.modifiers.contains(KeyModifiers::SHIFT)
                || key.modifiers.contains(KeyModifiers::CONTROL) =>
        {
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
    store: Option<&autonoetic_gateway::scheduler::gateway_store::GatewayStore>,
    session_id: &str,
    _tx: &tokio::sync::mpsc::UnboundedSender<(u64, String)>,
) -> bool {
    let root_session_id = autonoetic_gateway::runtime::content_store::root_session_id(session_id);
    let mut processed_any = false;

    let previous_workflow_status = app.workflow_status_line.clone();
    refresh_workflow_status_line(app, config, &root_session_id);
    if app.workflow_status_line != previous_workflow_status {
        processed_any = true;
        // Show notification when workflow becomes active
        if app.workflow_status_line.starts_with("wf:") && previous_workflow_status.starts_with("workflow: n/a") {
            app.add_message(MessageRole::System, format!("🔗 Workflow connected: {}", app.workflow_status_line));
            processed_any = true;
        }
    }

    match autonoetic_gateway::scheduler::resolve_workflow_id_for_root_session(
        config,
        &root_session_id,
    ) {
        Ok(Some(workflow_id)) => {
            if let Ok(events) = autonoetic_gateway::scheduler::load_workflow_events(config, store, &workflow_id) {
                if !app.workflow_events_bootstrapped {
                    let recap_count = events.len().min(20);
                    if recap_count > 0 {
                        app.add_message(MessageRole::System, "── workflow recap ──".to_string());
                        let start_idx = events.len().saturating_sub(recap_count);
                        for event in &events[start_idx..] {
                            if let Some(card) = format_workflow_event_card(event) {
                                app.add_message(MessageRole::Signal, card);
                            }
                            app.seen_workflow_event_ids.insert(event.event_id.clone());
                        }
                        app.add_message(MessageRole::System, "── live updates ──".to_string());
                    }
                    app.workflow_events_bootstrapped = true;
                } else {
                    for event in events {
                        if !app.seen_workflow_event_ids.insert(event.event_id.clone()) {
                            continue;
                        }
                        if let Some(card) = format_workflow_event_card(&event) {
                            app.add_message(MessageRole::Signal, card);
                            processed_any = true;
                        }
                    }
                }
            }
        }
        Ok(None) => {
            // No workflow found - this is normal if session is not connected to a workflow
        }
        Err(e) => {
            tracing::warn!(target: "chat", error = %e, "Failed to resolve workflow");
        }
    }

    if let Ok(pending_approvals) =
        autonoetic_gateway::scheduler::approval::pending_approval_requests_for_root(
            config,
            store,
            &root_session_id,
        )
    {
        let mut still_pending: HashSet<String> = HashSet::new();
        for request in pending_approvals {
            still_pending.insert(request.request_id.clone());
            app.add_awaiting_approval(request.request_id.clone());
            if app
                .announced_pending_approvals
                .insert(request.request_id.clone())
            {
                let mut detail = format!(
                    "⏸ Approval required: {} ({} by {})",
                    request.request_id,
                    request.action.kind(),
                    request.agent_id
                );
                if let Some(reason) = request.reason.as_ref().filter(|r| !r.trim().is_empty()) {
                    detail.push_str(&format!(" - {}", reason));
                }
                app.add_message(MessageRole::Signal, detail);
                processed_any = true;
            }
        }
        app.awaiting_approvals.retain(|id| still_pending.contains(id));
        app.announced_pending_approvals
            .retain(|id| still_pending.contains(id));
    }


    processed_any
}

/// Copy the selected text region to clipboard.
///
/// Uses the persistent `App::clipboard` instance so arboard's background ownership
/// thread stays alive after the write — clipboard managers have time to see the
/// content before it is released.
fn copy_selection_to_clipboard(app: &mut App) {
    let (Some((start_row, start_col)), Some((end_row, end_col))) = (app.sel_start, app.sel_end) else {
        return;
    };

    // Normalize selection direction.
    let (top_row, top_col, bot_row, bot_col) = if start_row <= end_row {
        (start_row, start_col, end_row, end_col)
    } else {
        (end_row, end_col, start_row, start_col)
    };

    // Build a flat list of all content lines (without prefix for clipboard).
    let mut lines: Vec<String> = Vec::new();
    for msg in &app.messages {
        for line in msg.content.lines() {
            lines.push(line.to_string());
        }
        lines.push(String::new()); // blank separator between messages
    }
    if !app.pending.is_empty() {
        lines.push(format!("{} Working...", app.spinner()));
    }

    let mut selected: Vec<String> = Vec::new();

    for row in top_row..=bot_row {
        if row >= lines.len() {
            break;
        }
        let line = &lines[row];

        if row == top_row && row == bot_row {
            // Single line selection
            let col_s = top_col.min(line.len());
            let col_e = bot_col.min(line.len());
            if col_e > col_s {
                selected.push(line[col_s..col_e].to_string());
            }
        } else if row == top_row {
            // First line of multi-line selection
            let col_s = top_col.min(line.len());
            selected.push(line[col_s..].to_string());
        } else if row == bot_row {
            // Last line of multi-line selection
            let col_e = bot_col.min(line.len());
            selected.push(line[..col_e].to_string());
        } else {
            // Middle line
            selected.push(line.clone());
        }
    }

    let selected_text = selected.join("\n");
    if selected_text.is_empty() {
        return;
    }

    // Safe clipboard copy - catch panics from arboard
    // arboard can panic on systems without a clipboard manager (headless, SSH, etc.)
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        // Reuse the persistent clipboard object; fall back to a fresh one if it was
        // never initialised (e.g. running in a headless environment).
        if let Some(cb) = app.clipboard.as_mut() {
            if cb.set_text(&selected_text).is_ok() {
                return true;
            }
        }
        // Last-resort: try allocating a new clipboard
        if let Ok(mut cb) = arboard::Clipboard::new() {
            if cb.set_text(&selected_text).is_ok() {
                app.clipboard = Some(cb);
                return true;
            }
        }
        false
    }));

    if result.is_err() {
        // Clipboard operation panicked - silently ignore to avoid terminal corruption
        tracing::warn!("Clipboard operation panicked, ignoring");
    }
}

#[cfg(test)]
mod tests {
    use super::{extract_approval_request_id, extract_structured_approval};

    #[test]
    fn test_extract_approval_request_id_short_form() {
        let text = "Install requires approval. request_id: apr-1234abcd";
        assert_eq!(
            extract_approval_request_id(text).as_deref(),
            Some("apr-1234abcd")
        );
    }

    #[test]
    fn test_extract_approval_request_id_uuid_fallback() {
        let text = "Approval required for request id: c19a8a50-d6c8-4c5f-aa3c-6ba119751b11";
        assert_eq!(
            extract_approval_request_id(text).as_deref(),
            Some("c19a8a50-d6c8-4c5f-aa3c-6ba119751b11")
        );
    }

    #[test]
    fn test_extract_structured_approval_sandbox_exec() {
        let payload = serde_json::json!({
            "ok": false,
            "approval_required": true,
            "request_id": "apr-1234abcd",
            "approval": {
                "kind": "sandbox_exec",
                "reason": "Remote access detected",
                "summary": "Sandbox exec: curl https://api.example.com",
                "retry_field": "approval_ref",
                "subject": {
                    "command": "curl https://api.example.com",
                    "hosts": ["api.example.com"]
                }
            }
        })
        .to_string();

        let parsed = extract_structured_approval(&payload).expect("structured approval expected");
        assert_eq!(parsed.request_id.as_deref(), Some("apr-1234abcd"));
        assert!(parsed.card.contains("kind: sandbox_exec"));
        assert!(parsed.card.contains("retry field: approval_ref"));
        assert!(parsed.card.contains("hosts: api.example.com"));
    }

    #[test]
    fn test_extract_structured_approval_agent_install() {
        let payload = serde_json::json!({
            "ok": false,
            "approval_required": true,
            "request_id": "apr-89abcdef",
            "approval": {
                "kind": "agent_install",
                "reason": "High-risk install requires approval",
                "summary": "weather.fetcher with NetworkAccess",
                "retry_field": "promotion_gate.install_approval_ref",
                "subject": {
                    "agent_id": "weather.fetcher",
                    "artifact_id": "art_123",
                    "risk_factors": ["network_access", "scheduled_action"],
                    "capabilities": ["NetworkAccess"]
                }
            }
        })
        .to_string();

        let parsed = extract_structured_approval(&payload).expect("structured approval expected");
        assert_eq!(parsed.request_id.as_deref(), Some("apr-89abcdef"));
        assert!(parsed.card.contains("kind: agent_install"));
        assert!(parsed.card.contains("agent: weather.fetcher"));
        assert!(parsed.card.contains("retry field: promotion_gate.install_approval_ref"));
    }
}
