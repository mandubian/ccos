//! CCOS Gateway Monitor
//!
//! Real-time monitoring TUI for the CCOS Gateway.
//! Shows connected sessions, spawned agents, and system events.

use clap::Parser;
use crossterm::{
    event::{
        self, DisableMouseCapture, EnableMouseCapture, Event, KeyCode, MouseButton, MouseEventKind,
    },
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

#[derive(Debug, Deserialize)]
struct GatewayPauseRunResponse {
    run_id: String,
    paused: bool,
    previous_state: String,
}

/// A row in the Runs tab tree view.
#[derive(Debug, Clone)]
enum RunDisplayItem {
    /// Session header row (selectable; pressing Enter toggles collapse).
    SessionHeader {
        session_id: String,
        run_count: usize,
        collapsed: bool,
    },
    /// Header row for a group of runs sharing the same recurring schedule.
    ScheduleGroup {
        group_id: String,
        goal: String,
        schedule: String,
        instance_count: usize,
        next_run_at: Option<String>,
        /// Run ID of the currently-`Scheduled` or `PausedSchedule` run in this group.
        /// Cancelling the group cancels this run, stopping future firings.
        pending_run_id: Option<String>,
        /// True when the pending run is in `PausedSchedule` state (rather than `Scheduled`).
        is_schedule_paused: bool,
        /// True when the children are hidden (collapsed).
        collapsed: bool,
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
            RunDisplayItem::SessionHeader { .. } => None,
        }
    }

    /// Run ID to pause when the user presses `z` on this item.
    /// Works on a `ScheduleGroup` header (non-paused) and on a `RunInstance` that is
    /// currently in `Scheduled` state, so the key works regardless of which row is selected.
    fn pause_run_id(&self) -> Option<String> {
        match self {
            RunDisplayItem::ScheduleGroup {
                pending_run_id,
                is_schedule_paused,
                ..
            } if !is_schedule_paused => pending_run_id.clone(),
            RunDisplayItem::RunInstance { run, .. }
                if run.state.contains("Scheduled") && !run.state.contains("PausedSchedule") =>
            {
                Some(run.run_id.clone())
            }
            RunDisplayItem::SessionHeader { .. } => None,
            _ => None,
        }
    }

    /// Run ID to unpause (resume) when the user presses `u` on this item.
    /// Works on a `ScheduleGroup` header (paused) and on a `RunInstance` that is in
    /// `PausedSchedule` state.
    fn unpause_run_id(&self) -> Option<String> {
        match self {
            RunDisplayItem::ScheduleGroup {
                pending_run_id,
                is_schedule_paused,
                ..
            } if *is_schedule_paused => pending_run_id.clone(),
            RunDisplayItem::RunInstance { run, .. } if run.state.contains("PausedSchedule") => {
                Some(run.run_id.clone())
            }
            RunDisplayItem::SessionHeader { .. } => None,
            _ => None,
        }
    }
}

/// A row in the Events tab session-grouped tree view.
#[derive(Debug, Clone)]
enum EventDisplayItem {
    /// Session header row (selectable; pressing Enter toggles collapse).
    SessionHeader {
        session_id: String,
        event_count: usize,
        collapsed: bool,
    },
    /// An individual event entry (selectable).
    EventEntry { event_idx: usize },
}

/// A row in the LLM tab session-grouped tree view.
#[derive(Debug, Clone)]
enum LlmDisplayItem {
    /// Session header (selectable; Enter toggles collapse).
    SessionHeader {
        session_id: String,
        count: usize,
        collapsed: bool,
    },
    /// A single LLM consultation entry (one row per call).
    ConsultationEntry { consultation_idx: usize },
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
    /// Session-grouped flat list built from events for the Events tab.
    event_display_items: Vec<EventDisplayItem>,
    /// LLM Consultations per session
    llm_consultations: HashMap<String, Vec<LlmConsultation>>,
    selected_tab: usize,
    selected_agent_index: usize,
    /// All known session IDs (sorted) — used for session navigation independent of active agents
    ordered_session_ids: Vec<String>,
    /// Runs loaded mapped by session
    runs_by_session: HashMap<String, Vec<GatewayRunSummary>>,
    /// Flat tree-view items built from runs_by_session (session headers + groups + instances)
    display_items: Vec<RunDisplayItem>,
    selected_run_index: usize,
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
    /// Collapsed state per schedule group_id (true = collapsed).
    /// Set to default on first encounter; preserved after manual toggle.
    run_group_collapsed: HashMap<String, bool>,
    /// Collapsed state per event session_id (true = collapsed).
    event_session_collapsed: HashMap<String, bool>,
    /// Collapsed state per run session_id (true = collapsed).
    run_session_collapsed: HashMap<String, bool>,
    /// Flat list of LLM display items (session headers + consultation entries).
    llm_display_items: Vec<LlmDisplayItem>,
    /// Currently selected row in the LLM tab.
    selected_llm_index: usize,
    /// Collapsed state per LLM session_id (true = collapsed).
    llm_session_collapsed: HashMap<String, bool>,
    /// Whether the LLM detail popup is shown.
    show_llm_detail: bool,
    /// Scroll offset for the LLM detail popup.
    llm_detail_scroll: u16,
}

impl MonitorState {
    fn new(available_llm_profiles: Vec<String>, active_spawn_llm_profile: Option<String>) -> Self {
        Self {
            sessions: HashMap::new(),
            agents: HashMap::new(),
            agent_session_ids: Vec::new(),
            ordered_session_ids: Vec::new(),
            events: Vec::new(),
            event_display_items: Vec::new(),
            llm_consultations: HashMap::new(),
            selected_tab: 0,
            selected_agent_index: 0,
            runs_by_session: HashMap::new(),
            display_items: Vec::new(),
            selected_run_index: 0,
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
            run_group_collapsed: HashMap::new(),
            event_session_collapsed: HashMap::new(),
            run_session_collapsed: HashMap::new(),
            llm_display_items: Vec::new(),
            selected_llm_index: 0,
            llm_session_collapsed: HashMap::new(),
            show_llm_detail: false,
            llm_detail_scroll: 0,
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

    /// Rebuild the LLM display items from llm_consultations, grouped by session.
    fn rebuild_llm_display_items(&mut self) {
        self.llm_display_items.clear();

        // ordered_session_ids contains all sessions we have seen
        for session_id in &self.ordered_session_ids {
            if let Some(consultations) = self.llm_consultations.get(session_id) {
                if consultations.is_empty() {
                    continue;
                }

                let is_active = self.agents.contains_key(session_id);
                // Default to collapsed for inactive sessions
                let collapsed = *self
                    .llm_session_collapsed
                    .entry(session_id.clone())
                    .or_insert(!is_active);

                self.llm_display_items.push(LlmDisplayItem::SessionHeader {
                    session_id: session_id.clone(),
                    count: consultations.len(),
                    collapsed,
                });
                if !collapsed {
                    // Iterate backwards so newest consultations show first
                    for (idx, _) in consultations.iter().enumerate().rev() {
                        self.llm_display_items
                            .push(LlmDisplayItem::ConsultationEntry {
                                consultation_idx: idx,
                            });
                    }
                }
            }
        }
    }

    fn navigate_llm_selection(&mut self, delta: i32) {
        let len = self.llm_display_items.len();
        if len == 0 {
            return;
        }
        let len_i = len as i32;
        let dir = if delta >= 0 { 1i32 } else { -1i32 };
        let mut remaining = delta.abs();
        let mut idx = self.selected_llm_index as i32;
        while remaining > 0 {
            idx = (idx + dir).rem_euclid(len_i);
            remaining -= 1;
        }
        self.selected_llm_index = idx as usize;
    }

    fn sync_llm_selection(&mut self) {
        let len = self.llm_display_items.len();
        if self.selected_llm_index >= len {
            self.selected_llm_index = len.saturating_sub(1);
        }
        // Close the detail popup if the selected entry disappeared (e.g. session collapsed).
        if self.show_llm_detail {
            if !matches!(
                self.llm_display_items.get(self.selected_llm_index),
                Some(LlmDisplayItem::ConsultationEntry { .. })
            ) {
                self.show_llm_detail = false;
            }
        }
    }

    /// Toggle collapse for the currently selected LLM session header.
    fn toggle_selected_llm_session_collapse(&mut self) {
        if let Some(LlmDisplayItem::SessionHeader {
            session_id,
            collapsed,
            ..
        }) = self.llm_display_items.get(self.selected_llm_index)
        {
            let new_collapsed = !collapsed;
            let sid = session_id.clone();
            self.llm_session_collapsed.insert(sid, new_collapsed);
            self.rebuild_llm_display_items();
            self.sync_llm_selection();
        }
    }

    /// Toggle collapse for the currently selected run group (if a ScheduleGroup is selected).
    fn toggle_selected_run_group_collapse(&mut self) {
        if let Some(RunDisplayItem::ScheduleGroup {
            group_id,
            collapsed,
            ..
        }) = self.display_items.get(self.selected_run_index)
        {
            let new_collapsed = !collapsed;
            let gid = group_id.clone();
            self.run_group_collapsed.insert(gid, new_collapsed);
            self.rebuild_display_items();
            self.sync_run_selection();
        }
    }

    /// Toggle collapse for the currently selected event session (if a SessionHeader is selected).
    fn toggle_selected_event_session_collapse(&mut self) {
        if let Some(EventDisplayItem::SessionHeader {
            session_id,
            collapsed,
            ..
        }) = self.event_display_items.get(self.selected_event_index)
        {
            let new_collapsed = !collapsed;
            let sid = session_id.clone();
            self.event_session_collapsed.insert(sid, new_collapsed);
            self.rebuild_event_display_items();
            self.sync_event_selection();
        }
    }

    /// Toggle collapse for the currently selected run session header (if a SessionHeader is selected).
    fn toggle_selected_run_session_collapse(&mut self) {
        if let Some(RunDisplayItem::SessionHeader {
            session_id,
            collapsed,
            ..
        }) = self.display_items.get(self.selected_run_index)
        {
            let new_collapsed = !collapsed;
            let sid = session_id.clone();
            self.run_session_collapsed.insert(sid, new_collapsed);
            self.rebuild_display_items();
            self.sync_run_selection();
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

    /// Rebuild the tree-view display items from the raw `runs_by_session` map.
    /// Groups runs by session, and then runs that share a `schedule_group_id` under a common header row.
    fn rebuild_display_items(&mut self) {
        use std::collections::HashMap as Map;
        self.display_items.clear();

        for session_id in &self.ordered_session_ids {
            if let Some(session_runs) = self.runs_by_session.get(session_id) {
                if session_runs.is_empty() {
                    continue;
                }

                let is_active = self.agents.contains_key(session_id);
                // Default to collapsed for inactive sessions
                let session_collapsed = *self
                    .run_session_collapsed
                    .entry(session_id.clone())
                    .or_insert(!is_active);

                self.display_items.push(RunDisplayItem::SessionHeader {
                    session_id: session_id.clone(),
                    run_count: session_runs.len(),
                    collapsed: session_collapsed,
                });

                if session_collapsed {
                    continue;
                }

                // Partition into grouped (has schedule_group_id) and singletons.
                let mut group_map: Map<String, Vec<GatewayRunSummary>> = Map::new();
                let mut singletons: Vec<GatewayRunSummary> = Vec::new();

                for run in session_runs {
                    if let Some(ref gid) = run.schedule_group_id {
                        group_map.entry(gid.clone()).or_default().push(run.clone());
                    } else {
                        singletons.push(run.clone());
                    }
                }

                // Build group header + child rows, ordered by most-recent activity in the group.
                let mut groups: Vec<(String, Vec<GatewayRunSummary>)> =
                    group_map.into_iter().collect();
                // Sort groups so the most recently active one comes first.
                groups.sort_by(|(_, a), (_, b)| {
                    let ta = a.iter().map(|r| r.created_at.as_str()).max().unwrap_or("");
                    let tb = b.iter().map(|r| r.created_at.as_str()).max().unwrap_or("");
                    tb.cmp(ta)
                });

                for (group_id, mut instances) in groups {
                    let goal = instances
                        .first()
                        .map(|r| r.goal.clone())
                        .unwrap_or_default();
                    let schedule = instances
                        .first()
                        .and_then(|r| r.schedule.as_deref())
                        .unwrap_or("?")
                        .to_string();

                    // Find the run that is either waiting to fire (Scheduled) or manually paused (PausedSchedule).
                    let pending = instances.iter().find(|r| {
                        r.state.contains("Scheduled") || r.state.contains("PausedSchedule")
                    });
                    let pending_run_id = pending.map(|r| r.run_id.clone());
                    let is_schedule_paused = pending
                        .map(|r| r.state.contains("PausedSchedule"))
                        .unwrap_or(false);
                    let next_run_at = pending.and_then(|r| r.next_run_at.clone());

                    // Default: expand if any run is Active or paused-while-running; collapse if all are terminal/scheduled.
                    let has_active = instances.iter().any(|r| {
                        r.state.contains("Active")
                            || (r.state.contains("Paused") && !r.state.contains("PausedSchedule"))
                    });
                    let collapsed = *self
                        .run_group_collapsed
                        .entry(group_id.clone())
                        .or_insert(!has_active);

                    self.display_items.push(RunDisplayItem::ScheduleGroup {
                        group_id,
                        goal,
                        schedule,
                        instance_count: instances.len(),
                        next_run_at,
                        pending_run_id,
                        is_schedule_paused,
                        collapsed,
                    });

                    if !collapsed {
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
        }
    }

    /// Rebuild the session-grouped display list from `self.events`.
    /// Sessions are ordered by their most-recent event (newest first).
    /// Events within each session are newest-first.
    /// Respects the `show_internal_steps` flag.
    fn rebuild_event_display_items(&mut self) {
        self.event_display_items.clear();
        let show_internal = self.show_internal_steps;

        // Determine session display order + collect per-session event indices.
        // Iterate events in reverse (newest first) to get newest-session-first ordering.
        let mut session_order: Vec<String> = self.ordered_session_ids.clone();
        let mut session_map: std::collections::HashMap<String, Vec<usize>> =
            std::collections::HashMap::new();

        for (idx, event) in self.events.iter().enumerate().rev() {
            if !show_internal && is_internal_step_event(event) {
                continue;
            }
            let entry = session_map
                .entry(event.session_id.clone())
                .or_insert_with(Vec::new);
            entry.push(idx); // already in newest-first order (reversed iteration)
            if !session_order.contains(&event.session_id) {
                session_order.push(event.session_id.clone());
            }
        }

        for session_id in &session_order {
            if let Some(indices) = session_map.get(session_id) {
                // Default: expand if session has an active agent, collapse otherwise.
                let is_active = self.agents.contains_key(session_id);
                let collapsed = *self
                    .event_session_collapsed
                    .entry(session_id.clone())
                    .or_insert(!is_active);

                self.event_display_items
                    .push(EventDisplayItem::SessionHeader {
                        session_id: session_id.clone(),
                        event_count: indices.len(),
                        collapsed,
                    });
                if !collapsed {
                    for &idx in indices {
                        self.event_display_items
                            .push(EventDisplayItem::EventEntry { event_idx: idx });
                    }
                }
            }
        }
    }

    /// Count visible EventEntry items in event_display_items.
    fn event_entry_count(&self) -> usize {
        self.event_display_items
            .iter()
            .filter(|i| matches!(i, EventDisplayItem::EventEntry { .. }))
            .count()
    }

    /// Navigate event selection by `delta` steps.
    /// SessionHeader rows are selectable (pressing Enter on one toggles collapse).
    fn navigate_event_selection(&mut self, delta: i32) {
        let len = self.event_display_items.len();
        if len == 0 {
            return;
        }
        let len_i = len as i32;
        let dir = if delta >= 0 { 1i32 } else { -1i32 };
        let mut remaining = delta.abs();
        let mut idx = self.selected_event_index as i32;

        while remaining > 0 {
            idx = (idx + dir).rem_euclid(len_i);
            remaining -= 1;
        }
        self.selected_event_index = idx as usize;
    }

    /// Get filtered event indices for selection navigation
    fn get_filtered_event_indices(&self) -> Vec<usize> {
        let event_visible =
            |event: &SystemEvent| self.show_internal_steps || !is_internal_step_event(event);

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
        self.rebuild_event_display_items();
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
        match self.event_display_items.get(self.selected_event_index) {
            Some(EventDisplayItem::EventEntry { event_idx }) => self.events.get(*event_idx),
            _ => None,
        }
    }

    fn event_id_for_display_index(&self, display_idx: usize) -> Option<u64> {
        match self.event_display_items.get(display_idx) {
            Some(EventDisplayItem::EventEntry { event_idx }) => {
                self.events.get(*event_idx).map(|e| e.id)
            }
            _ => None,
        }
    }

    fn sync_event_selection(&mut self) {
        if self.event_display_items.is_empty() {
            self.selected_event_index = 0;
            self.selected_event_id = None;
            self.show_event_detail = false;
            return;
        }

        // Keep popup content stable by anchoring on event ID.
        if self.show_event_detail {
            if let Some(selected_id) = self.selected_event_id {
                if let Some(idx) = self.event_display_items.iter().position(|item| {
                    if let EventDisplayItem::EventEntry { event_idx } = item {
                        self.events.get(*event_idx).map(|e| e.id) == Some(selected_id)
                    } else {
                        false
                    }
                }) {
                    self.selected_event_index = idx;
                    return;
                }
                // Selected event was rotated out of the buffer.
                self.selected_event_id = None;
                self.show_event_detail = false;
            }
        }

        // Clamp selected_event_index to valid range.
        let len = self.event_display_items.len();
        if self.selected_event_index >= len {
            self.selected_event_index = len.saturating_sub(1);
        }
        // Update the stable event ID from the (possibly new) index position.
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
        metadata_map.insert(
            "iteration".to_string(),
            serde_json::Value::from(consultation.iteration),
        );
        metadata_map.insert(
            "is_initial".to_string(),
            serde_json::Value::from(consultation.is_initial),
        );
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

        // Store full consultation for LLM tab per session
        let session_consultations = self
            .llm_consultations
            .entry(session_id.clone())
            .or_insert_with(Vec::new);

        session_consultations.push(consultation);

        // Keep only last 50 consultations per session
        if session_consultations.len() > 50 {
            session_consultations.remove(0);
        }

        self.rebuild_llm_display_items();
        self.sync_llm_selection();
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
    let active_spawn_llm_profile = match fetch_gateway_llm_profile_with_retry(
        &client,
        &args.gateway_url,
        &args.token,
    )
    .await
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
                                                state.status_message =
                                                    format!("Failed to set spawn profile: {}", e);
                                            }
                                        }
                                        state.show_profile_selector = false;
                                    }
                                    _ => {}
                                }
                                continue;
                            }

                            // Handle keys while LLM detail popup is open
                            if state.show_llm_detail {
                                match key.code {
                                    KeyCode::Esc | KeyCode::Enter | KeyCode::Char(' ') => {
                                        state.show_llm_detail = false;
                                        state.llm_detail_scroll = 0;
                                    }
                                    KeyCode::Up => {
                                        state.llm_detail_scroll =
                                            state.llm_detail_scroll.saturating_sub(1);
                                    }
                                    KeyCode::Down => {
                                        state.llm_detail_scroll =
                                            state.llm_detail_scroll.saturating_add(1);
                                    }
                                    KeyCode::PageUp => {
                                        state.llm_detail_scroll =
                                            state.llm_detail_scroll.saturating_sub(10);
                                    }
                                    KeyCode::PageDown => {
                                        state.llm_detail_scroll =
                                            state.llm_detail_scroll.saturating_add(10);
                                    }
                                    _ => {}
                                }
                                continue;
                            }

                            // Handle keys while event detail popup is open
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
                                        state.detail_scroll =
                                            state.detail_scroll.saturating_sub(10);
                                    }
                                    KeyCode::PageDown => {
                                        state.detail_scroll =
                                            state.detail_scroll.saturating_add(10);
                                    }
                                    KeyCode::Char('y') | KeyCode::Char('c') => {
                                        if let Some(event) = state.get_selected_event() {
                                            let content = build_copy_text(event);
                                            match std::fs::write(
                                                "/tmp/ccos-monitor-copy.txt",
                                                &content,
                                            ) {
                                                Ok(_) => {
                                                    state.status_message =
                                                        "Copied to /tmp/ccos-monitor-copy.txt"
                                                            .to_string()
                                                }
                                                Err(e) => {
                                                    state.status_message =
                                                        format!("Copy failed: {}", e)
                                                }
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
                                        state.rebuild_event_display_items();
                                        state.sync_event_selection();
                                    }
                                }
                                // Removed 's' keybind since sessions are grouped natively
                                KeyCode::Char('r') => {
                                    if state.selected_tab == 4 {
                                        let session_ids = state.ordered_session_ids.clone();
                                        match refresh_runs_for_selected_session(
                                            &mut state,
                                            &client,
                                            &args.gateway_url,
                                            &args.token,
                                            &session_ids,
                                        )
                                        .await
                                        {
                                            Ok(_) => {
                                                state.status_message = format!(
                                                    "Loaded runs for {} session(s) ({} display rows)",
                                                    state.ordered_session_ids.len(),
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
                                        if let Some(run_id_to_cancel) = state
                                            .selected_display_item()
                                            .and_then(|item| item.cancel_run_id())
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
                                                    let session_ids =
                                                        state.ordered_session_ids.clone();
                                                    if let Err(e) =
                                                        refresh_runs_for_selected_session(
                                                            &mut state,
                                                            &client,
                                                            &args.gateway_url,
                                                            &args.token,
                                                            &session_ids,
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
                                KeyCode::Char('z') => {
                                    if state.selected_tab == 4 {
                                        if let Some(run_id_to_pause) = state
                                            .selected_display_item()
                                            .and_then(|item| item.pause_run_id())
                                        {
                                            match pause_scheduled_run(
                                                &client,
                                                &args.gateway_url,
                                                &args.token,
                                                &run_id_to_pause,
                                            )
                                            .await
                                            {
                                                Ok(result) => {
                                                    state.status_message = format!(
                                                        "Run {} paused={} (was {})",
                                                        result.run_id,
                                                        result.paused,
                                                        result.previous_state
                                                    );
                                                    let session_ids =
                                                        state.ordered_session_ids.clone();
                                                    if let Err(e) =
                                                        refresh_runs_for_selected_session(
                                                            &mut state,
                                                            &client,
                                                            &args.gateway_url,
                                                            &args.token,
                                                            &session_ids,
                                                        )
                                                        .await
                                                    {
                                                        state.status_message = format!(
                                                            "Run paused, refresh failed: {}",
                                                            e
                                                        );
                                                    }
                                                }
                                                Err(e) => {
                                                    state.status_message =
                                                        format!("Failed to pause run: {}", e);
                                                }
                                            }
                                        } else {
                                            state.status_message =
                                                "No active scheduled run selected to pause"
                                                    .to_string();
                                        }
                                    }
                                }
                                KeyCode::Char('u') => {
                                    if state.selected_tab == 4 {
                                        if let Some(run_id_to_unpause) = state
                                            .selected_display_item()
                                            .and_then(|item| item.unpause_run_id())
                                        {
                                            match resume_scheduled_run(
                                                &client,
                                                &args.gateway_url,
                                                &args.token,
                                                &run_id_to_unpause,
                                            )
                                            .await
                                            {
                                                Ok(()) => {
                                                    state.status_message = format!(
                                                        "Scheduled run {} unpaused",
                                                        run_id_to_unpause
                                                    );
                                                    let session_ids =
                                                        state.ordered_session_ids.clone();
                                                    if let Err(e) =
                                                        refresh_runs_for_selected_session(
                                                            &mut state,
                                                            &client,
                                                            &args.gateway_url,
                                                            &args.token,
                                                            &session_ids,
                                                        )
                                                        .await
                                                    {
                                                        state.status_message = format!(
                                                            "Run unpaused, refresh failed: {}",
                                                            e
                                                        );
                                                    }
                                                }
                                                Err(e) => {
                                                    state.status_message =
                                                        format!("Failed to unpause run: {}", e);
                                                }
                                            }
                                        } else {
                                            state.status_message =
                                                "No paused scheduled run selected to unpause"
                                                    .to_string();
                                        }
                                    }
                                }
                                KeyCode::Tab => {
                                    state.selected_tab = (state.selected_tab + 1) % 5;
                                    if state.selected_tab == 4 {
                                        let session_ids = state.ordered_session_ids.clone();
                                        if let Err(e) = refresh_runs_for_selected_session(
                                            &mut state,
                                            &client,
                                            &args.gateway_url,
                                            &args.token,
                                            &session_ids,
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
                                        let session_ids = state.ordered_session_ids.clone();
                                        if let Err(e) = refresh_runs_for_selected_session(
                                            &mut state,
                                            &client,
                                            &args.gateway_url,
                                            &args.token,
                                            &session_ids,
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
                                    if state.selected_tab == 1
                                        && !state.agent_session_ids.is_empty()
                                    {
                                        if state.selected_agent_index > 0 {
                                            state.selected_agent_index -= 1;
                                        } else {
                                            state.selected_agent_index =
                                                state.agent_session_ids.len() - 1;
                                        }
                                    } else if state.selected_tab == 2 {
                                        state.navigate_event_selection(-1);
                                    } else if state.selected_tab == 3 {
                                        state.navigate_llm_selection(-1);
                                    } else if state.selected_tab == 4 {
                                        if !state.display_items.is_empty() {
                                            if state.selected_run_index > 0 {
                                                state.selected_run_index -= 1;
                                            } else {
                                                state.selected_run_index =
                                                    state.display_items.len() - 1;
                                            }
                                        }
                                    }
                                }
                                KeyCode::Down => {
                                    // Navigate agents in Agents tab or events in Events tab
                                    if state.selected_tab == 1
                                        && !state.agent_session_ids.is_empty()
                                    {
                                        state.selected_agent_index = (state.selected_agent_index
                                            + 1)
                                            % state.agent_session_ids.len();
                                    } else if state.selected_tab == 2 {
                                        state.navigate_event_selection(1);
                                    } else if state.selected_tab == 3 {
                                        state.navigate_llm_selection(1);
                                    } else if state.selected_tab == 4 {
                                        if !state.display_items.is_empty() {
                                            state.selected_run_index = (state.selected_run_index
                                                + 1)
                                                % state.display_items.len();
                                        }
                                    }
                                }
                                KeyCode::PageUp => {
                                    if state.selected_tab == 2 {
                                        let events_area = get_events_tab_area(terminal.size()?);
                                        let page_size =
                                            events_area.height.saturating_sub(2).max(1) as i32;
                                        state.navigate_event_selection(-page_size);
                                    } else if state.selected_tab == 3 {
                                        state.navigate_llm_selection(-10);
                                    }
                                }
                                KeyCode::PageDown => {
                                    if state.selected_tab == 2 {
                                        let events_area = get_events_tab_area(terminal.size()?);
                                        let page_size =
                                            events_area.height.saturating_sub(2).max(1) as i32;
                                        state.navigate_event_selection(page_size);
                                    } else if state.selected_tab == 3 {
                                        state.navigate_llm_selection(10);
                                    }
                                }
                                KeyCode::Home => {
                                    if state.selected_tab == 2 {
                                        // Jump to first EventEntry.
                                        if let Some((idx, _)) =
                                            state.event_display_items.iter().enumerate().find(
                                                |(_, i)| {
                                                    matches!(i, EventDisplayItem::EventEntry { .. })
                                                },
                                            )
                                        {
                                            state.selected_event_index = idx;
                                        }
                                    } else if state.selected_tab == 3 {
                                        state.selected_llm_index = 0;
                                    }
                                }
                                KeyCode::End => {
                                    if state.selected_tab == 2 {
                                        // Jump to last EventEntry.
                                        if let Some((idx, _)) =
                                            state.event_display_items.iter().enumerate().rev().find(
                                                |(_, i)| {
                                                    matches!(i, EventDisplayItem::EventEntry { .. })
                                                },
                                            )
                                        {
                                            state.selected_event_index = idx;
                                        }
                                    } else if state.selected_tab == 3 {
                                        let last = state.llm_display_items.len().saturating_sub(1);
                                        state.selected_llm_index = last;
                                    }
                                }
                                KeyCode::Enter | KeyCode::Char(' ') => {
                                    if state.selected_tab == 4 {
                                        // Toggle collapse on a ScheduleGroup header or SessionHeader.
                                        match state.display_items.get(state.selected_run_index) {
                                            Some(RunDisplayItem::SessionHeader { .. }) => {
                                                state.toggle_selected_run_session_collapse();
                                            }
                                            Some(RunDisplayItem::ScheduleGroup { .. }) => {
                                                state.toggle_selected_run_group_collapse();
                                            }
                                            _ => {}
                                        }
                                    } else if state.selected_tab == 2 {
                                        // Toggle collapse on a SessionHeader; show detail on EventEntry.
                                        if matches!(
                                            state
                                                .event_display_items
                                                .get(state.selected_event_index),
                                            Some(EventDisplayItem::SessionHeader { .. })
                                        ) {
                                            state.toggle_selected_event_session_collapse();
                                        } else if state.get_selected_event().is_some() {
                                            state.selected_event_id = state
                                                .event_id_for_display_index(
                                                    state.selected_event_index,
                                                );
                                            state.show_event_detail = true;
                                            state.detail_scroll = 0;
                                        }
                                    } else if state.selected_tab == 3 {
                                        match state.llm_display_items.get(state.selected_llm_index)
                                        {
                                            Some(LlmDisplayItem::SessionHeader { .. }) => {
                                                state.toggle_selected_llm_session_collapse();
                                            }
                                            Some(LlmDisplayItem::ConsultationEntry { .. }) => {
                                                state.show_llm_detail = !state.show_llm_detail;
                                                state.llm_detail_scroll = 0;
                                            }
                                            _ => {}
                                        }
                                    }
                                }
                                KeyCode::Esc => {
                                    if state.show_llm_detail {
                                        state.show_llm_detail = false;
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
                                    let display_count = state.event_display_items.len();
                                    if display_count > 0 {
                                        match mouse.kind {
                                            MouseEventKind::ScrollUp => {
                                                state.navigate_event_selection(-1);
                                            }
                                            MouseEventKind::ScrollDown => {
                                                state.navigate_event_selection(1);
                                            }
                                            MouseEventKind::Down(MouseButton::Left) => {
                                                // Select clicked row inside the list viewport
                                                if events_area.height > 2
                                                    && mouse.row > events_area.y
                                                {
                                                    let viewport_height =
                                                        events_area.height.saturating_sub(2)
                                                            as usize;
                                                    let inner_row =
                                                        mouse.row.saturating_sub(events_area.y + 1)
                                                            as usize;
                                                    if inner_row < viewport_height {
                                                        let scroll_offset =
                                                            state
                                                                .selected_event_index
                                                                .saturating_sub(viewport_height / 2)
                                                                .min(display_count.saturating_sub(
                                                                    viewport_height,
                                                                ));
                                                        let clicked_idx = scroll_offset + inner_row;
                                                        if clicked_idx < display_count {
                                                            match state.event_display_items.get(clicked_idx) {
                                                                Some(EventDisplayItem::SessionHeader { .. }) => {
                                                                    // Click on header: select it, second click toggles collapse.
                                                                    if clicked_idx == state.selected_event_index {
                                                                        state.toggle_selected_event_session_collapse();
                                                                    } else {
                                                                        state.selected_event_index = clicked_idx;
                                                                    }
                                                                }
                                                                Some(EventDisplayItem::EventEntry { .. }) => {
                                                                    let clicked_event_id = state.event_id_for_display_index(clicked_idx);
                                                                    if state.show_event_detail
                                                                        && state.selected_event_id == clicked_event_id
                                                                        && clicked_event_id.is_some()
                                                                    {
                                                                        state.show_event_detail = false;
                                                                    } else {
                                                                        state.selected_event_index = clicked_idx;
                                                                        state.selected_event_id = clicked_event_id;
                                                                        state.show_event_detail = true;
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
                                }
                            } else if state.selected_tab == 3 {
                                let llm_area = get_llm_tab_area(terminal.size()?);
                                let in_llm_area = mouse.column >= llm_area.x
                                    && mouse.column < llm_area.x + llm_area.width
                                    && mouse.row >= llm_area.y
                                    && mouse.row < llm_area.y + llm_area.height;

                                if in_llm_area {
                                    let items_count = state.llm_display_items.len();
                                    if items_count > 0 {
                                        match mouse.kind {
                                            MouseEventKind::ScrollUp => {
                                                state.navigate_llm_selection(-1);
                                            }
                                            MouseEventKind::ScrollDown => {
                                                state.navigate_llm_selection(1);
                                            }
                                            MouseEventKind::Down(MouseButton::Left) => {
                                                if llm_area.height > 2 && mouse.row > llm_area.y {
                                                    let viewport_height =
                                                        llm_area.height.saturating_sub(2) as usize;
                                                    let inner_row =
                                                        mouse.row.saturating_sub(llm_area.y + 1)
                                                            as usize;
                                                    if inner_row < viewport_height {
                                                        let scroll_offset =
                                                            state
                                                                .selected_llm_index
                                                                .saturating_sub(viewport_height / 2)
                                                                .min(items_count.saturating_sub(
                                                                    viewport_height,
                                                                ));
                                                        let clicked_idx = scroll_offset + inner_row;
                                                        if clicked_idx < items_count {
                                                            match state.llm_display_items.get(clicked_idx) {
                                                                Some(LlmDisplayItem::SessionHeader { .. }) => {
                                                                    if clicked_idx == state.selected_llm_index {
                                                                        state.toggle_selected_llm_session_collapse();
                                                                    } else {
                                                                        state.selected_llm_index = clicked_idx;
                                                                    }
                                                                }
                                                                Some(LlmDisplayItem::ConsultationEntry { .. }) => {
                                                                    if clicked_idx == state.selected_llm_index {
                                                                        state.show_llm_detail = !state.show_llm_detail;
                                                                        state.llm_detail_scroll = 0;
                                                                    } else {
                                                                        state.selected_llm_index = clicked_idx;
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
                                                        mouse.row.saturating_sub(runs_area.y + 1)
                                                            as usize;

                                                    if inner_row < viewport_height {
                                                        let max_offset = items_count
                                                            .saturating_sub(viewport_height);
                                                        let scroll_offset = state
                                                            .selected_run_index
                                                            .saturating_sub(viewport_height / 2)
                                                            .min(max_offset);
                                                        let clicked_index =
                                                            scroll_offset + inner_row;

                                                        if clicked_index < items_count {
                                                            // If clicking an already-selected ScheduleGroup, toggle collapse.
                                                            // Otherwise just select it.
                                                            if clicked_index == state.selected_run_index
                                                                && matches!(
                                                                    state.display_items.get(clicked_index),
                                                                    Some(RunDisplayItem::ScheduleGroup { .. })
                                                                )
                                                            {
                                                                state.toggle_selected_run_group_collapse();
                                                            } else {
                                                                state.selected_run_index = clicked_index;
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

                    // Rebuild ordered list of all session IDs for Runs tab navigation.
                    let mut all_ids: Vec<String> =
                        sessions.iter().map(|s| s.session_id.clone()).collect();
                    all_ids.sort();

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
                        &all_ids, // Pass all_ids to refresh all sessions
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
                        let program_summary =
                            single_line(&format_run_create_program_summary(program), 120);
                        if result_summary.is_empty() {
                            result_summary = format!("program: {}", program_summary);
                        } else {
                            result_summary =
                                format!("{} | program: {}", result_summary, program_summary);
                        }
                    }

                    if let Some(memory) = memory_operation.as_ref() {
                        let memory_summary =
                            single_line(&format_memory_operation_summary(memory), 120);
                        if result_summary.is_empty() {
                            result_summary = format!("memory: {}", memory_summary);
                        } else {
                            result_summary =
                                format!("{} | memory: {}", result_summary, memory_summary);
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
                            full_lines
                                .push(format!("  Trigger Capability: {}", trigger_capability_id));
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
    let resp = client
        .get(&url)
        .header("X-Admin-Token", token)
        .send()
        .await?;

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

async fn pause_scheduled_run(
    client: &Client,
    gateway_url: &str,
    token: &str,
    run_id: &str,
) -> anyhow::Result<GatewayPauseRunResponse> {
    let url = format!("{}/chat/run/{}/pause", gateway_url, run_id);
    let resp = client
        .post(&url)
        .header("X-Agent-Token", token)
        .send()
        .await?;

    if !resp.status().is_success() {
        return Err(anyhow::anyhow!(
            "Failed to pause run {}: HTTP {}",
            run_id,
            resp.status()
        ));
    }

    let body = resp.json::<GatewayPauseRunResponse>().await?;
    Ok(body)
}

async fn resume_scheduled_run(
    client: &Client,
    gateway_url: &str,
    token: &str,
    run_id: &str,
) -> anyhow::Result<()> {
    let url = format!("{}/chat/run/{}/resume", gateway_url, run_id);
    let resp = client
        .post(&url)
        .header("X-Agent-Token", token)
        .send()
        .await?;

    if !resp.status().is_success() {
        return Err(anyhow::anyhow!(
            "Failed to resume scheduled run {}: HTTP {}",
            run_id,
            resp.status()
        ));
    }

    Ok(())
}

async fn refresh_runs_for_selected_session(
    state: &mut MonitorState,
    client: &Client,
    gateway_url: &str,
    token: &str,
    session_ids: &[String], // Now takes a slice of all session IDs
) -> anyhow::Result<()> {
    state.runs_by_session.clear();

    // Fetch runs for all known sessions
    for session_id in session_ids {
        match fetch_session_runs(client, gateway_url, token, session_id).await {
            Ok(response) => {
                state
                    .runs_by_session
                    .insert(session_id.clone(), response.runs);
            }
            Err(e) => {
                // Log/ignore errors for individual sessions so we don't fail the whole refresh
                info!("Failed to fetch runs for session {}: {}", session_id, e);
            }
        }
    }

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

fn get_llm_tab_area(screen: Rect) -> Rect {
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

fn single_line(text: &str, max_chars: usize) -> String {
    let normalized = text.split_whitespace().collect::<Vec<_>>().join(" ");
    if normalized.chars().count() > max_chars {
        let truncated: String = normalized
            .chars()
            .take(max_chars.saturating_sub(3))
            .collect();
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
    let get_found = metadata.get("memory_get_found").and_then(|v| v.as_bool());
    let get_expired = metadata.get("memory_get_expired").and_then(|v| v.as_bool());
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

    let options =
        std::iter::once("<unset>").chain(state.available_llm_profiles.iter().map(|p| p.as_str()));
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
    let display_items = &state.display_items;

    // The runs are now grouped by session, no specific session is "selected".
    let title = format!(
        " Runs ({} items) · ↑↓ navigate  ↵/⎵ map/expand  [c]ancel [z]pause [u]uresume ",
        display_items.len(),
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
            RunDisplayItem::SessionHeader {
                session_id,
                run_count,
                collapsed,
            } => {
                let is_selected = false; // We don't have selected_idx here easily, but we can get it from state if needed.
                                         // Or just use a default style if it's the current selected item in List.
                                         // The selection styling is actually handled by the List widget's highlight_style,
                                         // so we don't strictly need to manually color the background here unless we want specific colors.

                let mut hdr_style = Style::default().fg(Color::Cyan);
                hdr_style = hdr_style.add_modifier(Modifier::BOLD);

                let icon = if *collapsed { "▶─" } else { "▼─" };
                let line = Line::from(vec![
                    Span::styled(format!("{} {} ", icon, session_id), hdr_style),
                    Span::styled(
                        format!("({} runs)", run_count),
                        Style::default().fg(Color::DarkGray),
                    ),
                ]);
                ListItem::new(line)
            }
            RunDisplayItem::ScheduleGroup {
                goal,
                schedule,
                instance_count,
                next_run_at,
                pending_run_id,
                is_schedule_paused,
                collapsed,
                ..
            } => {
                let next_str = next_run_at
                    .as_deref()
                    .map(|s| {
                        // Trim to short form: keep only time part of RFC3339 for compactness
                        s.get(11..19).unwrap_or(s)
                    })
                    .unwrap_or("-");
                let action_hint = if pending_run_id.is_some() {
                    if *is_schedule_paused {
                        " [u=unpause c=cancel]"
                    } else {
                        " [z=pause c=cancel]"
                    }
                } else {
                    " [completed]"
                };
                let (sched_icon, schedule_color, next_label) = if *is_schedule_paused {
                    ("⏸ ", Color::Yellow, "paused")
                } else {
                    ("📅 ", Color::Cyan, "next")
                };
                let collapse_icon = if *collapsed { "▶ " } else { "▼ " };
                let line = Line::from(vec![
                    Span::styled(collapse_icon, Style::default().fg(Color::DarkGray)),
                    Span::styled(sched_icon, Style::default().fg(schedule_color)),
                    Span::styled(
                        single_line(goal, 40),
                        Style::default()
                            .fg(Color::White)
                            .add_modifier(Modifier::BOLD),
                    ),
                    Span::styled(
                        format!("  {}  ", schedule),
                        Style::default().fg(schedule_color),
                    ),
                    Span::styled(
                        format!("[{} runs]", instance_count),
                        Style::default().fg(Color::DarkGray),
                    ),
                    Span::styled(
                        format!("  {}: {}", next_label, next_str),
                        Style::default().fg(if *is_schedule_paused {
                            Color::Yellow
                        } else {
                            Color::Green
                        }),
                    ),
                    Span::styled(
                        action_hint.to_string(),
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
                } else if run.state.contains("PausedSchedule") {
                    Color::Yellow
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
                    Span::styled(short_id.to_string(), Style::default().fg(Color::White)),
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
    let display_items = &state.event_display_items;
    let total_count = display_items.len();
    let entry_count = state.event_entry_count();
    let session_count = display_items
        .iter()
        .filter(|i| matches!(i, EventDisplayItem::SessionHeader { .. }))
        .count();

    let viewport_height = area.height.saturating_sub(2) as usize;

    // Keep selected item in the visible window (centred).
    let scroll_offset = if total_count == 0 {
        0
    } else {
        state
            .selected_event_index
            .min(total_count.saturating_sub(1))
            .saturating_sub(viewport_height / 2)
            .min(total_count.saturating_sub(viewport_height))
    };

    let visible_items: Vec<Line> = display_items
        .iter()
        .enumerate()
        .skip(scroll_offset)
        .take(viewport_height + 1)
        .map(|(display_idx, item)| {
            let is_selected = display_idx == state.selected_event_index;
            match item {
                EventDisplayItem::SessionHeader {
                    session_id,
                    event_count,
                    collapsed,
                } => {
                    let hdr_style = if is_selected {
                        Style::default()
                            .fg(Color::Black)
                            .bg(Color::Yellow)
                            .add_modifier(Modifier::BOLD)
                    } else {
                        Style::default()
                            .fg(Color::Yellow)
                            .add_modifier(Modifier::BOLD)
                    };
                    let collapse_icon = if *collapsed { "▶─ " } else { "▼─ " };
                    Line::from(vec![
                        Span::styled(collapse_icon, hdr_style),
                        Span::styled(session_id.clone(), hdr_style),
                        Span::styled(
                            format!("  {} event(s)", event_count),
                            if is_selected {
                                hdr_style
                            } else {
                                Style::default().fg(Color::DarkGray)
                            },
                        ),
                        Span::styled(
                            if *collapsed {
                                "  [↵ expand]"
                            } else {
                                "  [↵ collapse]"
                            },
                            if is_selected {
                                hdr_style
                            } else {
                                Style::default().fg(Color::DarkGray)
                            },
                        ),
                    ])
                }
                EventDisplayItem::EventEntry { event_idx } => {
                    let event = &state.events[*event_idx];
                    let elapsed = event.timestamp.elapsed().as_secs();
                    let time_str = if elapsed < 60 {
                        format!("{:>3}s", elapsed)
                    } else {
                        format!("{:>3}m", elapsed / 60)
                    };
                    let color = match event.event_type.as_str() {
                        "CRASH" => Color::Red,
                        "ACTION" => Color::Cyan,
                        "STATE" => Color::Green,
                        "LLM" => Color::Magenta,
                        _ => Color::Gray,
                    };
                    let base_style = if is_selected {
                        Style::default()
                            .fg(Color::Black)
                            .bg(Color::Cyan)
                            .add_modifier(Modifier::BOLD)
                    } else {
                        Style::default()
                    };
                    let max_detail = area.width.saturating_sub(20) as usize;
                    let detail_text = single_line(&event.details, max_detail.max(20));
                    Line::from(vec![
                        Span::styled(
                            "│ ",
                            if is_selected {
                                base_style
                            } else {
                                Style::default().fg(Color::DarkGray)
                            },
                        ),
                        Span::styled(
                            format!("[{}] ", time_str),
                            if is_selected {
                                base_style
                            } else {
                                Style::default().fg(Color::DarkGray)
                            },
                        ),
                        Span::styled(
                            format!("[{:<6}] ", &event.event_type),
                            if is_selected {
                                base_style
                            } else {
                                Style::default().fg(color)
                            },
                        ),
                        Span::styled(detail_text, base_style),
                    ])
                }
            }
        })
        .collect();

    let title = format!(
        "Events ({} entries, {} sessions) · ↑↓ navigate  ↵/⎵ expand/detail | i: {}",
        entry_count,
        session_count,
        if state.show_internal_steps {
            "hide internal"
        } else {
            "show internal"
        }
    );

    let paragraph =
        Paragraph::new(visible_items).block(Block::default().title(title).borders(Borders::ALL));
    f.render_widget(paragraph, area);

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
        if let Some(ec) = ce.exit_code {
            out.push_str(&format!("Exit Code: {}\n", ec));
        }
        if let Some(ms) = ce.duration_ms {
            out.push_str(&format!("Duration: {}ms\n", ms));
        }
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
                    program.schedule.unwrap_or_else(|| "none".to_string()),
                    Style::default().fg(Color::White),
                ),
            ]));
            lines.push(Line::from(vec![
                Span::styled("Next Run At: ", Style::default().fg(Color::Yellow)),
                Span::styled(
                    program.next_run_at.unwrap_or_else(|| "none".to_string()),
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
                        memory.store_value.unwrap_or_else(|| "none".to_string()),
                        Style::default().fg(Color::White),
                    ),
                ]));
                lines.push(Line::from(vec![
                    Span::styled("Entry ID: ", Style::default().fg(Color::Yellow)),
                    Span::styled(
                        memory.store_entry_id.unwrap_or_else(|| "none".to_string()),
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
                    Span::styled(
                        "<unrenderable result>",
                        Style::default().fg(Color::DarkGray),
                    ),
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
        Span::styled(
            " to copy to /tmp/ccos-monitor-copy.txt",
            Style::default().fg(Color::DarkGray),
        ),
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
    let display_items = &state.llm_display_items;

    if display_items.is_empty() {
        let message = if state.llm_consultations.is_empty() {
            "No LLM consultations yet.\n\nAgent LLM consultations will appear here when the agent is running in autonomous mode."
        } else {
            "No LLM consultations to display."
        };
        let empty = Paragraph::new(message)
            .style(Style::default().fg(Color::DarkGray))
            .block(
                Block::default()
                    .title("LLM Consultations · ↑↓ navigate  ↵/⎵ expand/detail")
                    .borders(Borders::ALL),
            )
            .wrap(Wrap { trim: true });
        f.render_widget(empty, area);
        return;
    }

    let total_count = display_items.len();
    let session_count = display_items
        .iter()
        .filter(|i| matches!(i, LlmDisplayItem::SessionHeader { .. }))
        .count();
    let entry_count = display_items
        .iter()
        .filter(|i| matches!(i, LlmDisplayItem::ConsultationEntry { .. }))
        .count();

    let viewport_height = area.height.saturating_sub(2) as usize;
    let scroll_offset = state
        .selected_llm_index
        .min(total_count.saturating_sub(1))
        .saturating_sub(viewport_height / 2)
        .min(total_count.saturating_sub(viewport_height));

    let visible_items: Vec<Line> = display_items
        .iter()
        .enumerate()
        .skip(scroll_offset)
        .take(viewport_height + 1)
        .map(|(display_idx, item)| {
            let is_selected = display_idx == state.selected_llm_index;
            match item {
                LlmDisplayItem::SessionHeader {
                    session_id,
                    count,
                    collapsed,
                } => {
                    let hdr_style = if is_selected {
                        Style::default()
                            .fg(Color::Black)
                            .bg(Color::Magenta)
                            .add_modifier(Modifier::BOLD)
                    } else {
                        Style::default()
                            .fg(Color::Magenta)
                            .add_modifier(Modifier::BOLD)
                    };
                    let collapse_icon = if *collapsed { "▶─ " } else { "▼─ " };
                    let action = if *collapsed {
                        "  [↵ expand]"
                    } else {
                        "  [↵ collapse]"
                    };
                    Line::from(vec![
                        Span::styled(collapse_icon, hdr_style),
                        Span::styled(session_id.clone(), hdr_style),
                        Span::styled(
                            format!("  {} call(s)", count),
                            if is_selected {
                                hdr_style
                            } else {
                                Style::default().fg(Color::DarkGray)
                            },
                        ),
                        Span::styled(
                            action,
                            if is_selected {
                                hdr_style
                            } else {
                                Style::default().fg(Color::DarkGray)
                            },
                        ),
                    ])
                }
                LlmDisplayItem::ConsultationEntry { consultation_idx } => {
                    // Backwards compatible way to find the consultation and session from the display item list
                    // Find the nearest SessionHeader before this entry
                    let mut current_session_id = String::new();
                    for i in (0..=display_idx).rev() {
                        if let Some(LlmDisplayItem::SessionHeader { session_id, .. }) =
                            state.llm_display_items.get(i)
                        {
                            current_session_id = session_id.clone();
                            break;
                        }
                    }

                    if let Some(consultations) = state.llm_consultations.get(&current_session_id) {
                        if let Some(consultation) = consultations.get(*consultation_idx) {
                            let session_id = &current_session_id;
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
                                "init"
                            } else {
                                "fup "
                            };
                            let model_str = consultation
                                .model
                                .as_deref()
                                .unwrap_or("?")
                                .split('/')
                                .last()
                                .unwrap_or("?");
                            let caps_str = if consultation.planned_capabilities.is_empty() {
                                "-".to_string()
                            } else {
                                consultation.planned_capabilities.join(", ")
                            };
                            let understanding_preview =
                                single_line(&consultation.understanding, 35);
                            let base = if is_selected {
                                Style::default()
                                    .fg(Color::Black)
                                    .bg(Color::Cyan)
                                    .add_modifier(Modifier::BOLD)
                            } else {
                                Style::default()
                            };
                            let _ = session_id; // grouped under header; not repeated per row
                            Line::from(vec![
                                Span::styled(
                                    "│ ",
                                    if is_selected {
                                        base
                                    } else {
                                        Style::default().fg(Color::DarkGray)
                                    },
                                ),
                                Span::styled(
                                    format!("[{:>3}] ", consultation.iteration),
                                    if is_selected {
                                        base
                                    } else {
                                        Style::default().fg(Color::Cyan)
                                    },
                                ),
                                Span::styled(
                                    format!("[{}] ", complete_icon),
                                    if is_selected {
                                        base
                                    } else {
                                        Style::default().fg(complete_color)
                                    },
                                ),
                                Span::styled(
                                    format!("[{}] ", init_str),
                                    if is_selected {
                                        base
                                    } else {
                                        Style::default().fg(Color::DarkGray)
                                    },
                                ),
                                Span::styled(
                                    format!("{:<16} ", model_str),
                                    if is_selected {
                                        base
                                    } else {
                                        Style::default().fg(Color::White)
                                    },
                                ),
                                Span::styled(
                                    understanding_preview,
                                    if is_selected {
                                        base
                                    } else {
                                        Style::default().fg(Color::White)
                                    },
                                ),
                                Span::styled(
                                    format!("  caps: {}", single_line(&caps_str, 25)),
                                    if is_selected {
                                        base
                                    } else {
                                        Style::default().fg(Color::DarkGray)
                                    },
                                ),
                            ])
                        } else {
                            Line::from("")
                        }
                    } else {
                        Line::from("")
                    }
                }
            }
        })
        .collect();

    let title = format!(
        "LLM Consultations ({} calls, {} sessions) · ↑↓ navigate  ↵/⎵ expand/detail",
        entry_count, session_count
    );

    let paragraph =
        Paragraph::new(visible_items).block(Block::default().title(title).borders(Borders::ALL));
    f.render_widget(paragraph, area);

    // Detail popup for the selected consultation
    if state.show_llm_detail {
        if let Some(LlmDisplayItem::ConsultationEntry { consultation_idx }) =
            state.llm_display_items.get(state.selected_llm_index)
        {
            let mut current_session_id = String::new();
            for i in (0..=state.selected_llm_index).rev() {
                if let Some(LlmDisplayItem::SessionHeader { session_id, .. }) =
                    state.llm_display_items.get(i)
                {
                    current_session_id = session_id.clone();
                    break;
                }
            }
            if let Some(consultations) = state.llm_consultations.get(&current_session_id) {
                if let Some(consultation) = consultations.get(*consultation_idx) {
                    draw_llm_detail_popup(
                        f,
                        area,
                        &current_session_id,
                        consultation,
                        state.llm_detail_scroll,
                    );
                }
            }
        }
    }
}

fn draw_llm_detail_popup(
    f: &mut Frame,
    area: Rect,
    session_id: &str,
    consultation: &LlmConsultation,
    scroll: u16,
) {
    let popup_area = centered_rect(85, 85, area);
    f.render_widget(ratatui::widgets::Clear, popup_area);

    let mut lines: Vec<Line> = Vec::new();

    let label_style = Style::default().fg(Color::Yellow);
    let value_style = Style::default().fg(Color::White);
    let dim_style = Style::default().fg(Color::DarkGray);

    lines.push(Line::from(vec![Span::styled(
        "LLM Consultation Detail",
        Style::default()
            .fg(Color::Magenta)
            .add_modifier(Modifier::BOLD),
    )]));
    lines.push(Line::from(""));

    lines.push(Line::from(vec![
        Span::styled("Session:    ", label_style),
        Span::styled(session_id, value_style),
    ]));
    lines.push(Line::from(vec![
        Span::styled("Iteration:  ", label_style),
        Span::styled(format!("{}", consultation.iteration), value_style),
    ]));
    lines.push(Line::from(vec![
        Span::styled("Type:       ", label_style),
        Span::styled(
            if consultation.is_initial {
                "initial"
            } else {
                "follow-up"
            },
            value_style,
        ),
    ]));
    lines.push(Line::from(vec![
        Span::styled("Complete:   ", label_style),
        Span::styled(
            if consultation.task_complete {
                "yes ✓"
            } else {
                "no →"
            },
            if consultation.task_complete {
                Style::default().fg(Color::Green)
            } else {
                Style::default().fg(Color::Yellow)
            },
        ),
    ]));
    if let Some(ref model) = consultation.model {
        lines.push(Line::from(vec![
            Span::styled("Model:      ", label_style),
            Span::styled(model.as_str(), value_style),
        ]));
    }
    if let Some(ref usage) = consultation.token_usage {
        lines.push(Line::from(vec![
            Span::styled("Tokens:     ", label_style),
            Span::styled(
                format!(
                    "prompt={} completion={} total={}",
                    usage.prompt_tokens, usage.completion_tokens, usage.total_tokens
                ),
                value_style,
            ),
        ]));
    }
    lines.push(Line::from(""));

    // Planned capabilities
    if !consultation.planned_capabilities.is_empty() {
        lines.push(Line::from(vec![Span::styled(
            "Planned capabilities:",
            label_style,
        )]));
        for cap in &consultation.planned_capabilities {
            lines.push(Line::from(vec![
                Span::styled("  • ", dim_style),
                Span::styled(cap.as_str(), Style::default().fg(Color::Cyan)),
            ]));
        }
        lines.push(Line::from(""));
    }

    // Understanding
    lines.push(Line::from(vec![Span::styled(
        "Understanding:",
        label_style,
    )]));
    for line in consultation.understanding.lines() {
        lines.push(Line::from(vec![
            Span::styled("  ", dim_style),
            Span::styled(line, value_style),
        ]));
    }
    lines.push(Line::from(""));

    // Reasoning
    lines.push(Line::from(vec![Span::styled("Reasoning:", label_style)]));
    for line in consultation.reasoning.lines() {
        lines.push(Line::from(vec![
            Span::styled("  ", dim_style),
            Span::styled(line, value_style),
        ]));
    }
    lines.push(Line::from(""));

    // Prompt (full)
    if let Some(ref prompt) = consultation.prompt {
        lines.push(Line::from(vec![Span::styled("Prompt:", label_style)]));
        for line in prompt.lines() {
            lines.push(Line::from(vec![
                Span::styled("  ", dim_style),
                Span::styled(line, dim_style),
            ]));
        }
        lines.push(Line::from(""));
    }

    // Response (full)
    if let Some(ref response) = consultation.response {
        lines.push(Line::from(vec![Span::styled("Response:", label_style)]));
        for line in response.lines() {
            lines.push(Line::from(vec![
                Span::styled("  ", dim_style),
                Span::styled(line, value_style),
            ]));
        }
        lines.push(Line::from(""));
    }

    lines.push(Line::from(vec![Span::styled(
        "─── Esc / Enter / Space to close ───",
        dim_style,
    )]));

    let paragraph = Paragraph::new(lines)
        .block(
            Block::default()
                .title("LLM Detail  [↑↓/PgUp/PgDn to scroll]")
                .borders(Borders::ALL)
                .border_style(Style::default().fg(Color::Magenta)),
        )
        .scroll((scroll, 0))
        .wrap(Wrap { trim: false });

    f.render_widget(paragraph, popup_area);
}
