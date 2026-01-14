//! CCOS Control Center TUI
//!
//! A rich terminal interface for introspecting:
//! - Goal decomposition process
//! - Capability resolution
//! - LLM prompts and responses
//! - Learning patterns and hints
//!
//! Run with: cargo run --bin ccos_explore

use std::io::{self, stdout};
use std::time::{Duration, Instant};

use clap::Parser;
use crossterm::{
    event::{
        self, DisableMouseCapture, EnableMouseCapture, Event, KeyCode, KeyModifiers, MouseEventKind,
    },
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::{backend::CrosstermBackend, Terminal};
use tokio::sync::mpsc;
use tokio::task::LocalSet;

use ccos::ccos_eprintln;
use ccos::examples_common::builder::ModularPlannerBuilder;
use ccos::planner::modular_planner::decomposition::llm_adapter::LlmInteractionCapture;
use ccos::planner::modular_planner::orchestrator::TraceEvent;
use ccos::tui::{
    panels,
    state::{
        ActivePanel, AppState, ApprovalsTab, ApprovedServerEntry, AuthStatus, AuthTokenPopup,
        CapabilityCategory, CapabilityResolution, CapabilitySource, DecompNode, DiscoverPopup,
        DiscoveredCapability, DiscoveryEntry, DiscoverySearchResult, ExecutionMode, LlmInteraction,
        NodeStatus, PendingServerEntry, ServerInfo, ServerStatus, TraceEventType, View,
    },
};

/// Format a JSON schema into a compact "field: type" format
fn format_schema_compact(json_value: &serde_json::Value) -> String {
    let mut lines = Vec::new();

    if let Some(props) = json_value.get("properties").and_then(|p| p.as_object()) {
        let required: std::collections::HashSet<&str> = json_value
            .get("required")
            .and_then(|r| r.as_array())
            .map(|arr| arr.iter().filter_map(|v| v.as_str()).collect())
            .unwrap_or_default();

        for (field, schema) in props {
            let type_str = extract_type_string(schema);
            let req_marker = if required.contains(field.as_str()) {
                "*"
            } else {
                "?"
            };
            lines.push(format!("  {}{}: {}", field, req_marker, type_str));
        }
    } else {
        // Not an object schema, just show the type
        let type_str = extract_type_string(json_value);
        lines.push(type_str);
    }

    lines.join("\n")
}

/// Extract a simple type string from a JSON schema
fn extract_type_string(schema: &serde_json::Value) -> String {
    // Handle anyOf (nullable types)
    if let Some(any_of) = schema.get("anyOf").and_then(|a| a.as_array()) {
        let types: Vec<String> = any_of
            .iter()
            .map(|s| extract_type_string(s))
            .filter(|t| t != "null")
            .collect();
        if types.len() == 1 {
            return types[0].clone();
        }
        return types.join(" | ");
    }

    // Handle oneOf
    if let Some(one_of) = schema.get("oneOf").and_then(|a| a.as_array()) {
        let types: Vec<String> = one_of.iter().map(|s| extract_type_string(s)).collect();
        return types.join(" | ");
    }

    // Get the type field
    match schema.get("type").and_then(|t| t.as_str()) {
        Some("string") => "string".to_string(),
        Some("integer") => "int".to_string(),
        Some("number") => "number".to_string(),
        Some("boolean") => "bool".to_string(),
        Some("null") => "null".to_string(),
        Some("array") => {
            if let Some(items) = schema.get("items") {
                format!("[{}]", extract_type_string(items))
            } else {
                "[]".to_string()
            }
        }
        Some("object") => {
            if let Some(props) = schema.get("properties").and_then(|p| p.as_object()) {
                let fields: Vec<String> = props.keys().take(3).cloned().collect();
                if fields.len() < props.len() {
                    format!("{{{},...}}", fields.join(", "))
                } else {
                    format!("{{{}}}", fields.join(", "))
                }
            } else {
                "object".to_string()
            }
        }
        _ => "any".to_string(),
    }
}

/// CCOS Control Center TUI - Explore goal decomposition and capability resolution
#[derive(Parser, Debug)]
#[command(name = "ccos_explore")]
#[command(about = "CCOS Control Center TUI for introspecting goal decomposition")]
struct Args {
    /// Goal to execute (if not provided, enter interactively)
    #[arg(short, long)]
    goal: Option<String>,

    /// Automatically start execution when goal is provided
    #[arg(short, long, default_value = "false")]
    auto_run: bool,
}

/// Event tick rate (60 FPS)
const TICK_RATE: Duration = Duration::from_millis(16);

/// Simplified intent info for TUI display
#[derive(Debug, Clone)]
struct SubIntentInfo {
    description: String,
    intent_type: String,
    params: std::collections::HashMap<String, String>,
    domain_hint: Option<String>,
}

/// Resolution info for TUI display
#[derive(Debug, Clone)]
struct ResolutionInfo {
    intent_id: String,
    intent_desc: String,
    capability_name: String,
    source_type: String,           // "Local", "Remote", "Synthesized", "BuiltIn"
    source_detail: Option<String>, // e.g. server URL for Remote
    confidence: Option<f64>,
}

/// Events sent from the background planner task to the TUI
#[derive(Debug)]
enum TuiEvent {
    Trace(TraceEventType, String, Option<String>),
    #[allow(dead_code)]
    GoalReceived(String, String, String, usize), // goal, rtfs_plan, prompt, prompt_scroll
    PlanComplete {
        root_id: String,
        intent_ids: Vec<String>,
        sub_intents: Vec<SubIntentInfo>,
        resolutions: Vec<ResolutionInfo>,
        rtfs_plan: String,
        _decomposition_prompt: Option<String>,
    },
    PlanError(String),
    EnvError(String),
    /// LLM call captured via trace callback
    LlmCalled {
        model: String,
        prompt: String,
        response: Option<String>,
        duration_ms: u64,
    },
    /// Mode change event for async state transitions
    ModeChange(ExecutionMode),
    /// Servers list loaded
    ServersLoaded(Vec<ServerInfo>),
    /// Server loading started
    ServersLoading,
    /// Server tools discovered
    ServerToolsDiscovered {
        server_index: usize,
        tool_count: usize,
        tool_names: Vec<String>,
    },
    /// Server connection check result
    ServerConnectionChecked {
        server_index: usize,
        status: ServerStatus,
    },
    /// Local capabilities loaded
    LocalCapabilitiesLoaded(Vec<DiscoveredCapability>),
    /// Discover loading state
    DiscoverLoading,
    /// Discovery search started
    DiscoverySearchStarted,
    /// Discovery search completed - populates popup with server list
    DiscoverySearchComplete(Vec<DiscoverySearchResult>),
    /// Server introspection completed - shows tools in popup
    IntrospectionComplete {
        server_name: String,
        endpoint: String,
        tools: Vec<DiscoveredCapability>,
    },
    /// Server introspection failed
    IntrospectionFailed {
        server_name: String,
        error: String,
    },
    /// Server introspection requires authentication
    IntrospectionAuthRequired {
        server_name: String,
        endpoint: String,
        env_var: String,
    },
    /// Log message during introspection
    IntrospectionLog(String),
    /// Popup closed
    #[allow(dead_code)]
    PopupClosed,

    // =========================================
    // Approvals Events
    // =========================================
    /// Approvals loading started
    ApprovalsLoading,
    /// Pending servers loaded
    PendingServersLoaded(Vec<PendingServerEntry>),
    /// Approved servers loaded
    ApprovedServersLoaded(Vec<ApprovedServerEntry>),
    /// Server approved successfully
    ServerApproved {
        _server_id: String,
        server_name: String,
    },
    /// Server rejected
    ServerRejected {
        _server_id: String,
        server_name: String,
    },
    /// Server added to pending queue
    ServerAddedToPending {
        server_name: String,
        _pending_id: String,
    },
    /// Auth token set successfully
    AuthTokenSet {
        env_var: String,
    },
    /// Approvals operation error
    ApprovalsError(String),
}

fn main() -> io::Result<()> {
    // Build a current-thread Tokio runtime
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("Failed to create Tokio runtime");

    // Run everything inside a LocalSet to allow spawn_local
    let local = LocalSet::new();
    local.block_on(&rt, async_main())
}

async fn async_main() -> io::Result<()> {
    // Parse command-line arguments before entering raw mode
    let args = Args::parse();

    // Suppress log output to avoid corrupting TUI
    // SAFETY: This is safe because we're setting env vars before any threads are spawned
    // and before accessing these variables from multiple threads
    unsafe {
        if std::env::var("RUST_LOG").is_err() {
            std::env::set_var("RUST_LOG", "off");
        }
        // Also suppress CCOS-specific debug output
        std::env::set_var("CCOS_QUIET_RESOLVER", "1");
        std::env::set_var("CCOS_QUIET", "1");
    }

    // Install panic hook to restore terminal on panic
    let original_panic_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |panic_info| {
        // Best-effort terminal restoration
        let _ = disable_raw_mode();
        let _ = execute!(
            std::io::stdout(),
            LeaveAlternateScreen,
            DisableMouseCapture
        );
        original_panic_hook(panic_info);
    }));

    // Terminal setup
    enable_raw_mode()?;
    let mut stdout = stdout();
    execute!(stdout, EnterAlternateScreen, EnableMouseCapture)?;
    // Flush to ensure escape sequences are sent immediately
    std::io::Write::flush(&mut stdout)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    // Initialize app state
    let mut state = AppState::new();

    // Set goal from CLI arg or use default placeholder
    if let Some(goal) = args.goal {
        state.goal_input = goal;
    } else {
        state.goal_input = "list issues in mandubian/ccos but ask me for the page size".to_string();
    }
    state.cursor_position = state.goal_input.len();

    // Create channel for real-time events from background planner
    let (tx, rx) = mpsc::unbounded_channel::<TuiEvent>();

    // If auto_run is set and we have a goal, trigger planning immediately
    let auto_run = args.auto_run && !state.goal_input.is_empty();

    // Run event loop
    let result = run_event_loop(&mut terminal, &mut state, tx, rx, auto_run).await;

    // Cleanup
    disable_raw_mode()?;
    execute!(
        terminal.backend_mut(),
        LeaveAlternateScreen,
        DisableMouseCapture
    )?;
    terminal.show_cursor()?;

    result
}

/// Main event loop
async fn run_event_loop<B: ratatui::backend::Backend>(
    terminal: &mut Terminal<B>,
    state: &mut AppState,
    event_tx: mpsc::UnboundedSender<TuiEvent>,
    mut event_rx: mpsc::UnboundedReceiver<TuiEvent>,
    auto_run: bool,
) -> io::Result<()> {
    let mut last_tick = Instant::now();

    // If auto_run is enabled, start the planner immediately
    if auto_run && !state.goal_input.is_empty() {
        spawn_planner_task(state, event_tx.clone());
    }

    loop {
        // Draw UI
        terminal.draw(|f| panels::render(f, state))?;

        // Poll for real-time events from the background planner (non-blocking)
        while let Ok(tui_event) = event_rx.try_recv() {
            process_tui_event(state, tui_event, event_tx.clone());
        }

        // Handle keyboard input with timeout
        let timeout = TICK_RATE
            .checked_sub(last_tick.elapsed())
            .unwrap_or(Duration::ZERO);

        if crossterm::event::poll(timeout)? {
            match event::read()? {
                Event::Key(key) => {
                    handle_key_event(state, key, event_tx.clone()).await;
                }
                Event::Mouse(mouse) => {
                    handle_mouse_event(state, mouse, terminal.size()?);
                }
                _ => {}
            }
        }

        if last_tick.elapsed() >= TICK_RATE {
            // Advance spinner animation when running
            state.tick();
            last_tick = Instant::now();
        }

        // Yield to allow spawned tasks to progress
        tokio::task::yield_now().await;

        if state.should_quit {
            break Ok(());
        }
    }
}

/// Process a TUI event from the background planner
fn process_tui_event(
    state: &mut AppState,
    event: TuiEvent,
    event_tx: mpsc::UnboundedSender<TuiEvent>,
) {
    // ... (existing handlers)
    match event {
        TuiEvent::GoalReceived(goal, rtfs_plan, prompt, prompt_scroll) => {
            state.mode = ExecutionMode::Received;
            state.goal_input = goal;
            state.cursor_position = state.goal_input.len();
            state.rtfs_plan = Some(rtfs_plan);
            state.rtfs_plan_scroll = 0;
            state.llm_prompt_scroll = prompt_scroll;
            // Add prompt to history if unique
            if state.llm_history.is_empty()
                || state
                    .llm_history
                    .back()
                    .map(|h| h.prompt != prompt)
                    .unwrap_or(true)
            {
                state.llm_history.push_back(LlmInteraction {
                    timestamp: Instant::now(),
                    model: "planner".to_string(),
                    prompt: prompt.clone(),
                    response: None,
                    tokens_prompt: prompt.len() / 4,
                    tokens_response: 0,
                    duration_ms: 0,
                });
            }
        }
        TuiEvent::PlanComplete {
            root_id,
            intent_ids,
            sub_intents,
            resolutions,
            rtfs_plan,
            _decomposition_prompt: _,
        } => {
            // Build decomposition tree
            state.decomp_root_id = Some(root_id.clone());
            state.decomp_nodes.push(DecompNode {
                id: root_id.clone(),
                description: state.goal_input.clone(),
                intent_type: "Root".to_string(),
                status: NodeStatus::Resolved {
                    capability: "plan".to_string(),
                },
                depth: 0,
                children: intent_ids.clone(),
                params: std::collections::HashMap::new(),
            });

            // Add child nodes with real intent data
            for (i, intent_id) in intent_ids.iter().enumerate() {
                let intent_info = sub_intents.get(i);
                state.decomp_nodes.push(DecompNode {
                    id: intent_id.clone(),
                    description: intent_info
                        .map(|info| info.description.clone())
                        .unwrap_or_else(|| format!("Step {}", i + 1)),
                    intent_type: intent_info
                        .map(|info| info.intent_type.clone())
                        .unwrap_or_else(|| "SubIntent".to_string()),
                    status: NodeStatus::Resolved {
                        capability: intent_info
                            .and_then(|info| info.domain_hint.clone())
                            .unwrap_or_else(|| "resolved".to_string()),
                    },
                    depth: 1,
                    children: Vec::new(),
                    params: intent_info
                        .map(|info| info.params.clone())
                        .unwrap_or_default(),
                });
            }

            // Add capability resolutions to panel
            for res in resolutions {
                let source = match res.source_type.as_str() {
                    "Remote" => CapabilitySource::McpServer(res.source_detail.unwrap_or_default()),
                    "Local" => CapabilitySource::LocalRtfs(res.source_detail.unwrap_or_default()),
                    "Synthesized" => CapabilitySource::Synthesized,
                    "BuiltIn" => CapabilitySource::Builtin,
                    _ => CapabilitySource::Unknown,
                };
                state.resolutions.push_back(CapabilityResolution {
                    intent_id: res.intent_id,
                    intent_desc: res.intent_desc,
                    capability_name: res.capability_name,
                    source,
                    embed_score: res.confidence.map(|c| c as f32),
                    heuristic_score: None,
                    timestamp: Instant::now(),
                });
            }

            // Store the RTFS plan for display
            state.rtfs_plan = Some(rtfs_plan.clone());
            state.rtfs_plan_scroll = 0;

            state.add_trace(
                TraceEventType::DecompositionComplete,
                format!("Plan generated: {} steps", intent_ids.len()),
                Some(rtfs_plan.clone()),
            );

            state.mode = ExecutionMode::Complete;
        }
        TuiEvent::PlanError(e) => {
            state.add_trace(
                TraceEventType::Error,
                format!("Planning failed: {}", e),
                None,
            );
            state.mode = ExecutionMode::Error;
        }
        TuiEvent::EnvError(e) => {
            state.add_trace(
                TraceEventType::Error,
                format!("Failed to build planner: {}", e),
                None,
            );
            state.mode = ExecutionMode::Error;
        }
        TuiEvent::LlmCalled {
            model,
            prompt,
            response,
            duration_ms,
        } => {
            // Add trace event for LLM call
            state.add_trace(
                TraceEventType::LlmCall,
                format!("LLM call to {} ({} ms)", model, duration_ms),
                response
                    .as_ref()
                    .map(|r| r.chars().take(100).collect::<String>() + "..."),
            );

            // Update LLM inspector with the actual prompt and response
            state.add_llm_interaction(LlmInteraction {
                timestamp: Instant::now(),
                model: model.clone(),
                prompt,
                response: response.clone(),
                tokens_prompt: 0, // We don't have token counts from the adapter yet
                tokens_response: 0,
                duration_ms,
            });
        }
        TuiEvent::ModeChange(new_mode) => {
            state.mode = new_mode;
        }
        TuiEvent::ServersLoading => {
            state.servers_loading = true;
        }
        TuiEvent::ServersLoaded(servers) => {
            state.servers = servers;
            state.servers_loading = false;
            state.servers_selected = 0;
        }
        TuiEvent::ServerToolsDiscovered {
            server_index,
            tool_count,
            tool_names,
        } => {
            if server_index < state.servers.len() {
                state.servers[server_index].tool_count = Some(tool_count);
                state.servers[server_index].tools = tool_names;
                state.servers[server_index].status = ServerStatus::Connected;
            }
        }
        TuiEvent::ServerConnectionChecked {
            server_index,
            status,
        } => {
            if server_index < state.servers.len() {
                state.servers[server_index].status = status;
            }
        }
        TuiEvent::DiscoverLoading => {
            state.discover_loading = true;
        }
        TuiEvent::LocalCapabilitiesLoaded(capabilities) => {
            state.discovered_capabilities = capabilities;
            state.discover_loading = false;
            state.discover_selected = 0;
        }
        TuiEvent::DiscoverySearchStarted => {
            state.discover_loading = true;
            state.add_trace(
                TraceEventType::ToolDiscovery,
                format!("Searching capabilities: '{}'", state.discover_search_hint),
                None,
            );
        }
        TuiEvent::DiscoverySearchComplete(servers) => {
            state.discover_loading = false;
            if servers.is_empty() {
                state.discover_popup = DiscoverPopup::Error {
                    title: "No Results".to_string(),
                    message: "No servers found matching your search".to_string(),
                };
            } else {
                state.discover_popup = DiscoverPopup::SearchResults {
                    servers,
                    selected: 0,
                };
            }
            state.add_trace(
                TraceEventType::ToolDiscovery,
                "Discovery search complete - popup opened".to_string(),
                None,
            );
        }
        TuiEvent::IntrospectionComplete {
            server_name,
            endpoint,
            tools,
        } => {
            // Update popup to show results
            state.discover_popup = DiscoverPopup::IntrospectionResults {
                server_name,
                endpoint,
                tools,
                selected: 0,
                selected_tools: std::collections::HashSet::new(),
            };
        }
        TuiEvent::IntrospectionFailed { server_name, error } => {
            // Check if this is an auth error and we have an auth retry pending
            // If so, update the auth popup with the error instead of showing error popup
            if let Some((retry_name, _)) = &state.discover_auth_retry {
                if retry_name == &server_name {
                    // This is an auth retry failure - show error in auth popup
                    if let Some(ref mut popup) = state.auth_token_popup {
                        popup.error_message = Some(format!(
                            "Authentication failed: {}. Please check your token and try again.",
                            error
                        ));
                    } else {
                        // No auth popup, but we're expecting one - create it
                        // We need the endpoint and env_var, but we don't have them here
                        // Fall back to showing error popup
                        state.discover_popup = DiscoverPopup::Error {
                            title: "Introspection Failed".to_string(),
                            message: error,
                        };
                    }
                } else {
                    // Different server - show error popup
                    state.discover_popup = DiscoverPopup::Error {
                        title: "Introspection Failed".to_string(),
                        message: error,
                    };
                }
            } else {
                // No auth retry pending - show error popup
                state.discover_popup = DiscoverPopup::Error {
                    title: "Introspection Failed".to_string(),
                    message: error,
                };
            }
        }
        TuiEvent::IntrospectionAuthRequired {
            server_name,
            endpoint,
            env_var,
        } => {
            // Close introspecting popup and show auth token input
            state.discover_popup = DiscoverPopup::None;
            state.discover_auth_retry = Some((server_name.clone(), endpoint.clone()));

            // Check if we already have an auth popup (retry case) and preserve token input
            let (existing_token, has_existing) = if let Some(ref existing_popup) =
                state.auth_token_popup
            {
                if existing_popup.server_name == server_name && existing_popup.env_var == env_var {
                    (existing_popup.token_input.clone(), true)
                } else {
                    (String::new(), false)
                }
            } else {
                (String::new(), false)
            };

            let token_len = existing_token.len();

            state.auth_token_popup = Some(AuthTokenPopup {
                server_name,
                env_var,
                token_input: existing_token,
                cursor_position: token_len,
                error_message: if has_existing {
                    Some(
                        "Token authentication failed. Please check your token and try again."
                            .to_string(),
                    )
                } else {
                    None
                },
                pending_id: String::new(), // Not used for introspection retry
            });
        }
        TuiEvent::IntrospectionLog(msg) => {
            // Append log to introspecting popup if active
            if let DiscoverPopup::Introspecting { logs, .. } = &mut state.discover_popup {
                logs.push(msg);
                // Keep only last 100 logs
                if logs.len() > 100 {
                    logs.remove(0);
                }
            }
        }
        TuiEvent::PopupClosed => {
            state.discover_popup = DiscoverPopup::None;
        }
        TuiEvent::Trace(event_type, msg, meta) => {
            state.add_trace(event_type, msg, meta);
        }
        // Approvals events
        TuiEvent::ApprovalsLoading => {
            state.approvals_loading = true;
        }
        TuiEvent::PendingServersLoaded(servers) => {
            state.pending_servers = servers;
            state.approvals_loading = false;
            state.pending_selected = 0;
        }
        TuiEvent::ApprovedServersLoaded(servers) => {
            state.approved_servers = servers;
            state.approvals_loading = false;
            state.approved_selected = 0;
        }
        TuiEvent::ServerApproved {
            _server_id: _,
            server_name,
        } => {
            state.add_trace(
                TraceEventType::Info,
                format!("Server approved: {}", server_name),
                None,
            );
            state.approvals_loading = false;
        }
        TuiEvent::ServerRejected {
            _server_id: _,
            server_name,
        } => {
            state.add_trace(
                TraceEventType::Info,
                format!("Server rejected: {}", server_name),
                None,
            );
            state.approvals_loading = false;
        }
        TuiEvent::ServerAddedToPending {
            server_name,
            _pending_id: _,
        } => {
            state.add_trace(
                TraceEventType::ToolDiscovery,
                format!("Server added to pending: {}", server_name),
                None,
            );
            // Refresh approvals queue
            load_approvals_async(event_tx.clone());
        }
        TuiEvent::AuthTokenSet { env_var } => {
            state.add_trace(
                TraceEventType::Info,
                format!("Auth token set for {}", env_var),
                None,
            );
            state.auth_token_popup = None;
        }
        TuiEvent::ApprovalsError(error) => {
            state.add_trace(
                TraceEventType::Error,
                format!("Approvals error: {}", error),
                None,
            );
            state.approvals_loading = false;
        }
    }
}

/// Handle keyboard events
async fn handle_key_event(
    state: &mut AppState,
    key: event::KeyEvent,
    event_tx: mpsc::UnboundedSender<TuiEvent>,
) {
    // Global shortcuts
    match (key.code, key.modifiers) {
        (KeyCode::Char('q'), _) if state.active_panel != ActivePanel::GoalInput => {
            state.should_quit = true;
            return;
        }
        (KeyCode::Char('?'), _) => {
            state.show_help = !state.show_help;
            return;
        }
        (KeyCode::Esc, _) => {
            if state.show_trace_popup {
                state.show_trace_popup = false;
            } else if state.show_intent_popup {
                state.show_intent_popup = false;
            } else if !matches!(state.discover_popup, DiscoverPopup::None) {
                // Handle popup escape - go back or close
                match &state.discover_popup {
                    DiscoverPopup::IntrospectionResults { .. } => {
                        // Go back to search results (if we had them)
                        state.discover_popup = DiscoverPopup::None;
                    }
                    _ => {
                        state.discover_popup = DiscoverPopup::None;
                    }
                }
            } else if state.show_help {
                state.show_help = false;
            } else if state.active_panel != ActivePanel::GoalInput {
                state.active_panel = ActivePanel::GoalInput;
            }
            return;
        }
        // View switching shortcuts (1-7) - blocked only in Goals view with active goal input
        // (so user can type numbers in the goal input field)
        (KeyCode::Char('1'), _) if state.active_panel != ActivePanel::GoalInput || state.current_view != View::Goals => {
            state.current_view = View::Goals;
            state.active_panel = ActivePanel::GoalInput;
            return;
        }
        (KeyCode::Char('2'), _) if state.active_panel != ActivePanel::GoalInput || state.current_view != View::Goals => {
            state.current_view = View::Plans;
            state.active_panel = ActivePanel::GoalInput; // Plans view not yet implemented
            return;
        }
        (KeyCode::Char('3'), _) if state.active_panel != ActivePanel::GoalInput || state.current_view != View::Goals => {
            state.current_view = View::Execute;
            state.active_panel = ActivePanel::GoalInput; // Execute view not yet implemented
            return;
        }
        (KeyCode::Char('4'), _) if state.active_panel != ActivePanel::GoalInput || state.current_view != View::Goals => {
            state.current_view = View::Discover;
            state.active_panel = ActivePanel::DiscoverList;
            // Trigger capability loading if not already loaded
            if state.discovered_capabilities.is_empty() && !state.discover_loading {
                load_local_capabilities_async(event_tx.clone());
            }
            return;
        }
        (KeyCode::Char('5'), _) if state.active_panel != ActivePanel::GoalInput || state.current_view != View::Goals => {
            state.current_view = View::Servers;
            state.active_panel = ActivePanel::ServersList;
            // Trigger server loading if not already loaded
            if state.servers.is_empty() && !state.servers_loading {
                load_servers_async(event_tx.clone());
            }
            return;
        }
        (KeyCode::Char('6'), _) if state.active_panel != ActivePanel::GoalInput || state.current_view != View::Goals => {
            state.current_view = View::Approvals;
            state.active_panel = ActivePanel::ApprovalsPendingList;
            // Trigger approvals loading if not already loaded
            if state.pending_servers.is_empty()
                && state.approved_servers.is_empty()
                && !state.approvals_loading
            {
                load_approvals_async(event_tx.clone());
            }
            return;
        }
        (KeyCode::Char('7'), _) if state.active_panel != ActivePanel::GoalInput || state.current_view != View::Goals => {
            state.current_view = View::Config;
            state.active_panel = ActivePanel::GoalInput; // Config view not yet implemented
            return;
        }
        (KeyCode::Tab, KeyModifiers::NONE) => {
            state.active_panel = state.active_panel.next();
            return;
        }
        (KeyCode::BackTab, _) => {
            state.active_panel = state.active_panel.prev();
            return;
        }
        _ => {}
    }

    // Discovery popup-specific handling (intercepts all keys when active)
    if !matches!(state.discover_popup, DiscoverPopup::None) {
        handle_discover_popup_key(state, key, event_tx.clone());
        return;
    }

    // Auth token popup handling (intercepts all keys when active)
    if state.auth_token_popup.is_some() {
        handle_auth_token_popup(state, key, event_tx.clone());
        return;
    }

    // View-specific handling
    match state.current_view {
        View::Servers => {
            handle_servers_view(state, key, event_tx);
            return;
        }
        View::Approvals => {
            handle_approvals_view(state, key, event_tx).await;
            return;
        }
        View::Goals => {
            // Fall through to panel-specific handling for Goals view
        }
        _ => {
            // Other views don't have specific handling yet
        }
    }

    // Panel-specific handling (for Goals view)
    // Panel-specific handling (for Goals view)
    match state.active_panel {
        ActivePanel::GoalInput => handle_goal_input(state, key, event_tx),
        ActivePanel::RtfsPlan => handle_rtfs_plan(state, key),
        ActivePanel::DecompositionTree => handle_decomp_tree(state, key),
        ActivePanel::CapabilityResolution => handle_resolution(state, key),
        ActivePanel::TraceTimeline => handle_timeline(state, key),
        ActivePanel::LlmInspector => handle_llm_inspector(state, key),
        // Servers View
        ActivePanel::ServersList | ActivePanel::ServerDetails => {
            handle_servers_view(state, key, event_tx)
        }
        // Discover View
        ActivePanel::DiscoverList => handle_discover_list(state, key),
        ActivePanel::DiscoverDetails => handle_discover_details(state, key),
        ActivePanel::DiscoverInput => handle_discover_input(state, key, event_tx),
        // Approvals View
        ActivePanel::ApprovalsPendingList
        | ActivePanel::ApprovalsApprovedList
        | ActivePanel::ApprovalsDetails => {
            handle_approvals_view(state, key, event_tx).await;
        }
    }
}

fn handle_discover_details(state: &mut AppState, key: event::KeyEvent) {
    match key.code {
        KeyCode::Up | KeyCode::Char('k') => {
            state.discover_details_scroll = state.discover_details_scroll.saturating_sub(1);
        }
        KeyCode::Down | KeyCode::Char('j') => {
            state.discover_details_scroll = state.discover_details_scroll.saturating_add(1);
        }
        KeyCode::PageUp => {
            state.discover_details_scroll = state.discover_details_scroll.saturating_sub(10);
        }
        KeyCode::PageDown => {
            state.discover_details_scroll = state.discover_details_scroll.saturating_add(10);
        }
        KeyCode::Home => {
            state.discover_details_scroll = 0;
        }
        _ => {}
    }
}

/// Handle goal input panel keys
fn handle_goal_input(
    state: &mut AppState,
    key: event::KeyEvent,
    event_tx: mpsc::UnboundedSender<TuiEvent>,
) {
    match key.code {
        KeyCode::Enter => {
            if !state.goal_input.is_empty() && !state.is_running() {
                // Immediate feedback that goal was received
                state.mode = ExecutionMode::Received;
                state.add_trace(
                    TraceEventType::Info,
                    format!("Goal received: '{}'", state.goal_input),
                    None,
                );
                spawn_planner_task(state, event_tx);
            }
        }
        KeyCode::Backspace => {
            if state.cursor_position > 0 {
                state.goal_input.remove(state.cursor_position - 1);
                state.cursor_position -= 1;
            }
        }
        KeyCode::Delete => {
            if state.cursor_position < state.goal_input.len() {
                state.goal_input.remove(state.cursor_position);
            }
        }
        KeyCode::Left => {
            state.cursor_position = state.cursor_position.saturating_sub(1);
        }
        KeyCode::Right => {
            state.cursor_position = (state.cursor_position + 1).min(state.goal_input.len());
        }
        KeyCode::Home => {
            state.cursor_position = 0;
        }
        KeyCode::End => {
            state.cursor_position = state.goal_input.len();
        }
        KeyCode::Char('q') if key.modifiers.contains(KeyModifiers::CONTROL) => {
            state.should_quit = true;
        }
        KeyCode::Char(c) => {
            state.goal_input.insert(state.cursor_position, c);
            state.cursor_position += 1;
        }
        _ => {}
    }
}

/// Handle RTFS Plan panel keys (scrolling)
fn handle_rtfs_plan(state: &mut AppState, key: event::KeyEvent) {
    match key.code {
        KeyCode::Up | KeyCode::Char('k') => {
            state.rtfs_plan_scroll = state.rtfs_plan_scroll.saturating_sub(1);
        }
        KeyCode::Down | KeyCode::Char('j') => {
            if let Some(plan) = &state.rtfs_plan {
                let max_scroll = plan.lines().count().saturating_sub(1);
                state.rtfs_plan_scroll = (state.rtfs_plan_scroll + 1).min(max_scroll);
            }
        }
        KeyCode::PageUp => {
            state.rtfs_plan_scroll = state.rtfs_plan_scroll.saturating_sub(10);
        }
        KeyCode::PageDown => {
            if let Some(plan) = &state.rtfs_plan {
                let max_scroll = plan.lines().count().saturating_sub(1);
                state.rtfs_plan_scroll = (state.rtfs_plan_scroll + 10).min(max_scroll);
            }
        }
        KeyCode::Home => {
            state.rtfs_plan_scroll = 0;
        }
        KeyCode::End => {
            if let Some(plan) = &state.rtfs_plan {
                state.rtfs_plan_scroll = plan.lines().count().saturating_sub(1);
            }
        }
        _ => {}
    }
}

/// Spawn the planner as a local background task that sends events to the TUI in real-time
fn spawn_planner_task(state: &mut AppState, event_tx: mpsc::UnboundedSender<TuiEvent>) {
    state.reset_for_new_goal();
    // Mode is already set to Received, it will transition to Planning when work starts
    state.add_trace(
        TraceEventType::Info,
        "Starting goal execution...".to_string(),
        None,
    );

    let goal = state.goal_input.clone();

    // Spawn using spawn_local (no Send required!)
    tokio::task::spawn_local(async move {
        // Artificial delay to ensure "Received" status is visible
        tokio::time::sleep(std::time::Duration::from_millis(500)).await;

        // Send loading config trace
        let _ = event_tx.send(TuiEvent::Trace(
            TraceEventType::Info,
            "Loading config: config/agent_config.toml".to_string(),
            None,
        ));

        // Create LLM trace callback that sends events to TUI
        let llm_tx = event_tx.clone();
        let llm_trace_callback = std::sync::Arc::new(move |capture: LlmInteractionCapture| {
            // Send LLM call event to TUI
            let _ = llm_tx.send(TuiEvent::LlmCalled {
                model: capture.model,
                prompt: capture.prompt,
                response: capture.response,
                duration_ms: capture.duration_ms,
            });
        });

        // Build planner environment
        match ModularPlannerBuilder::new()
            .with_config("config/agent_config.toml")
            .with_options(true, true, false, true) // embeddings, mcp, cache, pure_llm
            .with_debug_options(true, false, false)
            .with_llm_trace_callback(llm_trace_callback)
            .build()
            .await
        {
            Ok(env) => {
                // Transition from Received to Planning now that work is starting
                let _ = event_tx.send(TuiEvent::ModeChange(ExecutionMode::Planning));

                let _ = event_tx.send(TuiEvent::Trace(
                    TraceEventType::Info,
                    "Environment built successfully".to_string(),
                    None,
                ));

                let _ = event_tx.send(TuiEvent::Trace(
                    TraceEventType::DecompositionStart,
                    format!("Goal: {}", goal),
                    None,
                ));

                let mut planner = env.planner;

                match planner.plan(&goal).await {
                    Ok(result) => {
                        // Send trace events as they were collected
                        for event in &result.trace.events {
                            send_trace_event(&event_tx, event);
                        }

                        // Send plan completion with intent details
                        let sub_intents: Vec<SubIntentInfo> = result
                            .sub_intents
                            .iter()
                            .map(|si| SubIntentInfo {
                                description: si.description.clone(),
                                intent_type: format!("{:?}", si.intent_type),
                                params: si.extracted_params.clone(),
                                domain_hint: si.domain_hint.as_ref().map(|d| format!("{:?}", d)),
                            })
                            .collect();

                        // Extract resolutions for TUI
                        let resolutions: Vec<ResolutionInfo> = result
                            .resolutions
                            .iter()
                            .map(|(intent_id, resolved)| {
                                let (source_type, source_detail, confidence) = match resolved {
                                    ccos::planner::modular_planner::resolution::ResolvedCapability::Local {
                                        capability_id,
                                        confidence,
                                        ..
                                    } => ("Local".to_string(), Some(capability_id.clone()), Some(*confidence)),
                                    ccos::planner::modular_planner::resolution::ResolvedCapability::Remote {
                                        capability_id,
                                        server_url,
                                        confidence,
                                        ..
                                    } => ("Remote".to_string(), Some(format!("{} ({})", capability_id, server_url)), Some(*confidence)),
                                    ccos::planner::modular_planner::resolution::ResolvedCapability::Synthesized {
                                        capability_id,
                                        ..
                                    } => ("Synthesized".to_string(), Some(capability_id.clone()), None),
                                    ccos::planner::modular_planner::resolution::ResolvedCapability::BuiltIn {
                                        capability_id,
                                        ..
                                    } => ("BuiltIn".to_string(), Some(capability_id.clone()), None),
                                    ccos::planner::modular_planner::resolution::ResolvedCapability::NeedsReferral {
                                        reason,
                                        ..
                                    } => ("NeedsReferral".to_string(), Some(reason.clone()), None),
                                };
                                
                                let intent_desc = result.sub_intents.iter()
                                    .enumerate()
                                    .find(|(_, si)| format!("intent_{}", si.description.chars().take(20).collect::<String>()) == *intent_id || intent_id.contains(&si.description[..std::cmp::min(20, si.description.len())]))
                                    .map(|(_, si)| si.description.clone())
                                    .unwrap_or_else(|| intent_id.clone());
                                    
                                ResolutionInfo {
                                    intent_id: intent_id.clone(),
                                    intent_desc,
                                    capability_name: resolved.capability_id().unwrap_or("unknown").to_string(),
                                    source_type,
                                    source_detail,
                                    confidence,
                                }
                            })
                            .collect();

                        // No LlmCalled variant in TraceEvent, so we generate a placeholder prompt
                        let _decomposition_prompt: Option<String> = None;

                        let _ = event_tx.send(TuiEvent::PlanComplete {
                            root_id: result.root_intent_id.clone(),
                            intent_ids: result.intent_ids.clone(),
                            sub_intents,
                            resolutions,
                            rtfs_plan: result.rtfs_plan.clone(),
                            _decomposition_prompt: _decomposition_prompt,
                        });
                    }
                    Err(e) => {
                        let _ = event_tx.send(TuiEvent::PlanError(format!("{:?}", e)));
                    }
                }
            }
            Err(e) => {
                let _ = event_tx.send(TuiEvent::EnvError(format!("{}", e)));
            }
        }
    });
}

/// Send a trace event from the planner through the channel
fn send_trace_event(tx: &mpsc::UnboundedSender<TuiEvent>, event: &TraceEvent) {
    let (event_type, message, details) = match event {
        TraceEvent::DecompositionStarted { strategy } => (
            TraceEventType::DecompositionStart,
            format!("Decomposing with strategy: {}", strategy),
            None,
        ),
        TraceEvent::DecompositionCompleted {
            num_intents,
            confidence,
        } => (
            TraceEventType::DecompositionComplete,
            format!(
                "Decomposed into {} intents (confidence: {:.2})",
                num_intents, confidence
            ),
            None,
        ),
        TraceEvent::IntentCreated {
            intent_id,
            description,
        } => (
            TraceEventType::Info,
            format!("Intent created: {} - {}", intent_id, description),
            None,
        ),
        TraceEvent::EdgeCreated {
            from,
            to,
            edge_type,
        } => (
            TraceEventType::Info,
            format!("Edge: {} → {} ({})", from, to, edge_type),
            None,
        ),
        TraceEvent::ResolutionStarted { intent_id } => (
            TraceEventType::ResolutionStart,
            format!("Resolving intent: {}", intent_id),
            None,
        ),
        TraceEvent::ResolutionCompleted {
            intent_id,
            capability,
        } => (
            TraceEventType::ResolutionComplete,
            format!("Resolved {} → {}", intent_id, capability),
            None,
        ),
        TraceEvent::ResolutionFailed { intent_id, reason } => (
            TraceEventType::ResolutionFailed,
            format!("Failed to resolve {}: {}", intent_id, reason),
            None,
        ),
        TraceEvent::LlmCalled {
            model,
            prompt,
            response,
            tokens_prompt,
            tokens_response,
            duration_ms,
        } => (
            TraceEventType::LlmCall,
            format!(
                "LLM Call: {} ({} tokens → {} tokens, {}ms)",
                model, tokens_prompt, tokens_response, duration_ms
            ),
            Some(format!(
                "Prompt:\n{}\n\nResponse:\n{}",
                prompt,
                response.as_deref().unwrap_or("(pending)")
            )),
        ),
        TraceEvent::DiscoverySearchCompleted { query, num_results } => (
            TraceEventType::Info,
            format!("Discovery search for '{}': {} results", query, num_results),
            None,
        ),
    };

    let _ = tx.send(TuiEvent::Trace(event_type, message, details));
}

/// Handle decomposition tree panel keys
fn handle_decomp_tree(state: &mut AppState, key: event::KeyEvent) {
    match key.code {
        KeyCode::Up => {
            state.decomp_selected = state.decomp_selected.saturating_sub(1);
        }
        KeyCode::Down => {
            if !state.decomp_nodes.is_empty() {
                state.decomp_selected =
                    (state.decomp_selected + 1).min(state.decomp_nodes.len() - 1);
            }
        }
        KeyCode::Enter => {
            // Show intent details popup
            state.show_intent_popup = true;
        }
        KeyCode::Char(' ') => {
            // Toggle expansion of selected node
            if let Some(node) = state.decomp_nodes.get(state.decomp_selected) {
                let id = node.id.clone();
                if state.decomp_expanded.contains(&id) {
                    state.decomp_expanded.remove(&id);
                } else {
                    state.decomp_expanded.insert(id);
                }
            }
        }
        _ => {}
    }
}

/// Handle capability resolution panel keys
fn handle_resolution(state: &mut AppState, key: event::KeyEvent) {
    match key.code {
        KeyCode::Up => {
            state.resolution_selected = state.resolution_selected.saturating_sub(1);
        }
        KeyCode::Down => {
            if !state.resolutions.is_empty() {
                state.resolution_selected =
                    (state.resolution_selected + 1).min(state.resolutions.len() - 1);
            }
        }
        _ => {}
    }
}

/// Handle trace timeline panel keys
fn handle_timeline(state: &mut AppState, key: event::KeyEvent) {
    // Count filtered entries for bounds checking
    let filtered_count = state
        .trace_entries
        .iter()
        .filter(|e| state.verbose_trace || e.event_type.is_important())
        .count();

    match key.code {
        KeyCode::Char('v') | KeyCode::Char('V') => {
            state.verbose_trace = !state.verbose_trace;
            state.trace_selected = 0; // Reset selection when toggling
        }
        KeyCode::Up | KeyCode::Char('k') => {
            state.trace_selected = state.trace_selected.saturating_sub(1);
        }
        KeyCode::Down | KeyCode::Char('j') => {
            if filtered_count > 0 {
                state.trace_selected =
                    (state.trace_selected + 1).min(filtered_count.saturating_sub(1));
            }
        }
        KeyCode::PageUp => {
            state.trace_selected = state.trace_selected.saturating_sub(10);
        }
        KeyCode::PageDown => {
            if filtered_count > 0 {
                state.trace_selected =
                    (state.trace_selected + 10).min(filtered_count.saturating_sub(1));
            }
        }
        KeyCode::Home => {
            state.trace_selected = 0;
        }
        KeyCode::End => {
            if filtered_count > 0 {
                state.trace_selected = filtered_count.saturating_sub(1);
            }
        }
        KeyCode::Enter => {
            // Toggle popup to show full trace details
            state.show_trace_popup = !state.show_trace_popup;
        }
        KeyCode::Esc => {
            // Close popup
            state.show_trace_popup = false;
        }
        _ => {}
    }
}

/// Handle LLM inspector panel keys
fn handle_llm_inspector(state: &mut AppState, key: event::KeyEvent) {
    match key.code {
        KeyCode::Up => {
            state.llm_selected = state.llm_selected.saturating_sub(1);
        }
        KeyCode::Down => {
            if !state.llm_history.is_empty() {
                state.llm_selected = (state.llm_selected + 1).min(state.llm_history.len() - 1);
            }
        }
        KeyCode::Left => {
            // Scroll prompt up
            state.llm_prompt_scroll = state.llm_prompt_scroll.saturating_sub(1);
        }
        KeyCode::Right => {
            // Scroll prompt down
            state.llm_prompt_scroll += 1;
        }
        KeyCode::Char('j') => {
            // Scroll response down
            state.llm_response_scroll += 1;
        }
        KeyCode::Char('k') => {
            // Scroll response up
            state.llm_response_scroll = state.llm_response_scroll.saturating_sub(1);
        }
        _ => {}
    }
}

/// Handle mouse events for panel selection and scrolling
fn handle_mouse_event(
    state: &mut AppState,
    mouse: crossterm::event::MouseEvent,
    size: ratatui::layout::Rect,
) {
    use ccos::tui::state::ActivePanel;

    let col = mouse.column;
    let row = mouse.row;

    match state.current_view {
        View::Goals => {
            // Calculate layout regions (matching panels.rs render function)
            // Main vertical layout: [Goal (3 rows), Main (remaining), Status (1 row)]
            let goal_height = 3u16;
            let status_height = 1u16;
            let main_height = size.height.saturating_sub(goal_height + status_height);

            // Layout: 45% left (RTFS Plan), 55% right (2x2 grid)
            let left_width = (size.width * 45) / 100;

            // Determine which region was clicked
            match mouse.kind {
                MouseEventKind::Down(_) | MouseEventKind::Up(_) => {
                    if row < goal_height {
                        // Goal input panel
                        state.active_panel = ActivePanel::GoalInput;
                    } else if row < goal_height + main_height {
                        // Main content area
                        let main_row = row - goal_height;

                        if col < left_width {
                            // Left column: RTFS Plan (full height)
                            state.active_panel = ActivePanel::RtfsPlan;
                        } else {
                            // Right column: 2x2 grid
                            let right_height = main_height / 2;
                            let right_width = size.width - left_width;
                            let right_half_width = right_width / 2;
                            let right_col = col - left_width;

                            if main_row < right_height {
                                // Top row
                                if right_col < right_half_width {
                                    state.active_panel = ActivePanel::DecompositionTree;
                                } else {
                                    state.active_panel = ActivePanel::CapabilityResolution;
                                }
                            } else {
                                // Bottom row
                                if right_col < right_half_width {
                                    state.active_panel = ActivePanel::TraceTimeline;
                                } else {
                                    state.active_panel = ActivePanel::LlmInspector;
                                }
                            }
                        }
                    }
                }
                MouseEventKind::ScrollUp => match state.active_panel {
                    ActivePanel::RtfsPlan => {
                        state.rtfs_plan_scroll = state.rtfs_plan_scroll.saturating_sub(3);
                    }
                    ActivePanel::LlmInspector => {
                        state.llm_response_scroll = state.llm_response_scroll.saturating_sub(3);
                    }
                    ActivePanel::TraceTimeline => {
                        state.trace_selected = state.trace_selected.saturating_sub(3);
                    }
                    _ => {}
                },
                MouseEventKind::ScrollDown => match state.active_panel {
                    ActivePanel::RtfsPlan => {
                        if let Some(plan) = &state.rtfs_plan {
                            let max_scroll = plan.lines().count().saturating_sub(1);
                            state.rtfs_plan_scroll = (state.rtfs_plan_scroll + 3).min(max_scroll);
                        }
                    }
                    ActivePanel::LlmInspector => {
                        state.llm_response_scroll += 3;
                    }
                    ActivePanel::TraceTimeline => {
                        let filtered_count = state
                            .trace_entries
                            .iter()
                            .filter(|e| state.verbose_trace || e.event_type.is_important())
                            .count();
                        if filtered_count > 0 {
                            state.trace_selected =
                                (state.trace_selected + 3).min(filtered_count.saturating_sub(1));
                        }
                    }
                    _ => {}
                },
                _ => {}
            }
        }
        View::Discover => {
            // Layout: Discovery input at top (3 rows), List below
            let input_height = 3u16;

            match mouse.kind {
                MouseEventKind::Down(_) | MouseEventKind::Up(_) => {
                    if row < input_height {
                        state.active_panel = ActivePanel::DiscoverInput;
                    } else {
                        state.active_panel = ActivePanel::DiscoverList;
                    }
                }
                MouseEventKind::ScrollUp => {
                    if state.active_panel == ActivePanel::DiscoverList {
                        state.discover_selected = state.discover_selected.saturating_sub(3);
                        // Sync scroll
                        if state.discover_selected < state.discover_scroll {
                            state.discover_scroll = state.discover_selected;
                        }
                    } else if state.active_panel == ActivePanel::DiscoverDetails {
                        state.discover_details_scroll =
                            state.discover_details_scroll.saturating_sub(3);
                    }
                }
                MouseEventKind::ScrollDown => {
                    if state.active_panel == ActivePanel::DiscoverList {
                        let visible_len = state.visible_discovery_entries().len();
                        let visible_height = state.discover_panel_height;
                        if visible_len > 0 {
                            state.discover_selected =
                                (state.discover_selected + 3).min(visible_len - 1);
                            // Sync scroll
                            if state.discover_selected >= state.discover_scroll + visible_height {
                                state.discover_scroll =
                                    state.discover_selected.saturating_sub(visible_height - 1);
                            }
                        }
                    } else if state.active_panel == ActivePanel::DiscoverDetails {
                        state.discover_details_scroll =
                            state.discover_details_scroll.saturating_add(3);
                    }
                }
                _ => {}
            }
        }
        View::Servers => {
            // Layout: Title? Or maybe full list?
            // Assuming full list or similar to Discover
            match mouse.kind {
                MouseEventKind::Down(_) | MouseEventKind::Up(_) => {
                    // For now just activate list
                    state.active_panel = ActivePanel::ServersList;
                }
                MouseEventKind::ScrollUp => {
                    if state.active_panel == ActivePanel::ServersList {
                        state.servers_selected = state.servers_selected.saturating_sub(3);
                    }
                }
                MouseEventKind::ScrollDown => {
                    if state.active_panel == ActivePanel::ServersList && !state.servers.is_empty() {
                        state.servers_selected =
                            (state.servers_selected + 3).min(state.servers.len() - 1);
                    }
                }
                _ => {}
            }
        }
        _ => {}
    }
}

/// Load servers asynchronously in the background
/// Handle keyboard events in the Servers view
fn handle_servers_view(
    state: &mut AppState,
    key: event::KeyEvent,
    event_tx: mpsc::UnboundedSender<TuiEvent>,
) {
    match key.code {
        // Navigate up in server list
        KeyCode::Char('k') | KeyCode::Up => {
            if state.servers_selected > 0 {
                state.servers_selected -= 1;
            }
        }
        // Navigate down in server list
        KeyCode::Char('j') | KeyCode::Down => {
            if !state.servers.is_empty() && state.servers_selected < state.servers.len() - 1 {
                state.servers_selected += 1;
            }
        }
        // Refresh servers
        KeyCode::Char('r') => {
            if !state.servers_loading {
                load_servers_async(event_tx);
            }
        }
        // Discover tools for selected server
        KeyCode::Char('d') => {
            if !state.servers.is_empty() && state.servers_selected < state.servers.len() {
                let server = &state.servers[state.servers_selected];
                discover_server_tools_async(
                    event_tx,
                    state.servers_selected,
                    server.endpoint.clone(),
                );
            }
        }
        // Check connection for selected server
        KeyCode::Char('c') => {
            if !state.servers.is_empty() && state.servers_selected < state.servers.len() {
                // Clone endpoint first to avoid borrow conflict
                let endpoint = state.servers[state.servers_selected].endpoint.clone();
                // Update status to Connecting
                state.servers[state.servers_selected].status = ServerStatus::Connecting;
                check_server_connection_async(event_tx, state.servers_selected, endpoint);
            }
        }
        // Enter to select/activate server (same as discover)
        KeyCode::Enter => {
            if !state.servers.is_empty() && state.servers_selected < state.servers.len() {
                let server = &state.servers[state.servers_selected];
                discover_server_tools_async(
                    event_tx,
                    state.servers_selected,
                    server.endpoint.clone(),
                );
            }
        }
        _ => {}
    }
}

// =========================================
// Approvals View Handlers
// =========================================

/// Handle keyboard events in the Approvals view
async fn handle_approvals_view(
    state: &mut AppState,
    key: event::KeyEvent,
    event_tx: mpsc::UnboundedSender<TuiEvent>,
) {
    match key.code {
        // Tab switching
        KeyCode::Char('1') => {
            state.approvals_tab = ApprovalsTab::Pending;
            state.active_panel = ActivePanel::ApprovalsPendingList;
        }
        KeyCode::Char('2') => {
            state.approvals_tab = ApprovalsTab::Approved;
            state.active_panel = ActivePanel::ApprovalsApprovedList;
        }
        // Navigation
        KeyCode::Up | KeyCode::Char('k') => match state.approvals_tab {
            ApprovalsTab::Pending => {
                state.pending_selected = state.pending_selected.saturating_sub(1);
            }
            ApprovalsTab::Approved => {
                state.approved_selected = state.approved_selected.saturating_sub(1);
            }
        },
        KeyCode::Down | KeyCode::Char('j') => match state.approvals_tab {
            ApprovalsTab::Pending => {
                if !state.pending_servers.is_empty() {
                    state.pending_selected =
                        (state.pending_selected + 1).min(state.pending_servers.len() - 1);
                }
            }
            ApprovalsTab::Approved => {
                if !state.approved_servers.is_empty() {
                    state.approved_selected =
                        (state.approved_selected + 1).min(state.approved_servers.len() - 1);
                }
            }
        },
        // Refresh
        KeyCode::Char('R') => {
            load_approvals_async(event_tx);
        }
        // Approve pending server
        KeyCode::Char('a') if state.approvals_tab == ApprovalsTab::Pending => {
            if let Some(server) = state.pending_servers.get(state.pending_selected) {
                let server_id = server.id.clone();
                let server_name = server.name.clone();
                approve_server_async(event_tx, server_id, server_name);
            }
        }
        // Reject pending server
        KeyCode::Char('r') if state.approvals_tab == ApprovalsTab::Pending => {
            if let Some(server) = state.pending_servers.get(state.pending_selected) {
                let server_id = server.id.clone();
                let server_name = server.name.clone();
                reject_server_async(event_tx, server_id, server_name);
            }
        }
        // Set auth token
        KeyCode::Char('t') if state.approvals_tab == ApprovalsTab::Pending => {
            if let Some(server) = state.pending_servers.get(state.pending_selected) {
                if server.auth_status == AuthStatus::TokenMissing {
                    if let Some(ref env_var) = server.auth_env_var {
                        state.auth_token_popup = Some(AuthTokenPopup {
                            server_name: server.name.clone(),
                            env_var: env_var.clone(),
                            token_input: String::new(),
                            cursor_position: 0,
                            error_message: None,
                            pending_id: server.id.clone(),
                        });
                    }
                }
            }
        }
        // Dismiss approved server
        KeyCode::Char('d') if state.approvals_tab == ApprovalsTab::Approved => {
            if let Some(server) = state.approved_servers.get(state.approved_selected) {
                let server_id = server.id.clone();
                let server_name = server.name.clone();
                dismiss_server_async(event_tx, server_id, server_name);
            }
        }
        // Introspect tools
        KeyCode::Char('i') => match state.approvals_tab {
            ApprovalsTab::Pending => {
                if let Some(server) = state.pending_servers.get(state.pending_selected) {
                    let server_name = server.name.clone();
                    let endpoint = server.endpoint.clone();
                    let event_tx_clone = event_tx.clone();
                    tokio::task::spawn_local(async move {
                        introspect_server_async(server_name, endpoint, event_tx_clone).await;
                    });
                }
            }
            ApprovalsTab::Approved => {
                if let Some(server) = state.approved_servers.get(state.approved_selected) {
                    let server_name = server.name.clone();
                    let endpoint = server.endpoint.clone();
                    let event_tx_clone = event_tx.clone();
                    tokio::task::spawn_local(async move {
                        introspect_server_async(server_name, endpoint, event_tx_clone).await;
                    });
                }
            }
        },
        _ => {}
    }
}

/// Handle auth token popup input
fn handle_auth_token_popup(
    state: &mut AppState,
    key: event::KeyEvent,
    event_tx: mpsc::UnboundedSender<TuiEvent>,
) {
    if let Some(ref mut popup) = state.auth_token_popup {
        match key.code {
            KeyCode::Esc => {
                state.auth_token_popup = None;
            }
            KeyCode::Enter => {
                if !popup.token_input.is_empty() {
                    let env_var = popup.env_var.clone();
                    let token = popup.token_input.clone();
                    let pending_id = popup.pending_id.clone();

                    // Set the environment variable (for current session only)
                    // SAFETY: This is within a TUI context where we control execution
                    unsafe {
                        std::env::set_var(&env_var, &token);
                    }

                    // Update auth status for the pending server (if in Approvals view)
                    if !pending_id.is_empty() {
                        if let Some(server) = state
                            .pending_servers
                            .iter_mut()
                            .find(|s| s.id == pending_id)
                        {
                            server.auth_status = AuthStatus::TokenPresent;
                        }
                    }

                    // Check if we need to retry introspection
                    let retry_introspection = state.discover_auth_retry.take();

                    let _ = event_tx.send(TuiEvent::AuthTokenSet {
                        env_var: env_var.clone(),
                    });
                    state.auth_token_popup = None;

                    // Retry introspection if needed
                    if let Some((server_name, endpoint)) = retry_introspection {
                        // Restore introspecting popup
                        state.discover_popup = DiscoverPopup::Introspecting {
                            server_name: server_name.clone(),
                            endpoint: endpoint.clone(),
                            logs: vec!["Retrying with authentication...".to_string()],
                        };

                        let event_tx_clone = event_tx.clone();
                        tokio::task::spawn_local(async move {
                            // Small delay to ensure env var is set
                            tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;
                            introspect_server_async(server_name, endpoint, event_tx_clone).await;
                        });
                    }
                } else {
                    popup.error_message = Some("Token cannot be empty".to_string());
                }
            }
            KeyCode::Backspace => {
                if popup.cursor_position > 0 {
                    popup.token_input.remove(popup.cursor_position - 1);
                    popup.cursor_position -= 1;
                }
                popup.error_message = None;
            }
            KeyCode::Char(c) => {
                popup.token_input.insert(popup.cursor_position, c);
                popup.cursor_position += 1;
                popup.error_message = None;
            }
            _ => {}
        }
    }
}

/// Load approval queues asynchronously
fn load_approvals_async(event_tx: mpsc::UnboundedSender<TuiEvent>) {
    let _ = event_tx.send(TuiEvent::ApprovalsLoading);

    tokio::task::spawn_local(async move {
        let queue = match create_unified_queue() {
            Ok(q) => q,
            Err(e) => {
                let _ = event_tx.send(TuiEvent::ApprovalsError(format!(
                    "Failed to create queue: {}",
                    e
                )));
                return;
            }
        };

        // Load pending servers
        match queue.list_pending_servers().await {
            Ok(pending_requests) => {
                let pending_entries: Vec<PendingServerEntry> = pending_requests
                    .iter()
                    .filter_map(|r| r.to_pending_discovery())
                    .map(|p| {
                        // Check if auth token is available
                        let auth_status = if let Some(ref env_var) = p.server_info.auth_env_var {
                            if std::env::var(env_var).is_ok() {
                                AuthStatus::TokenPresent
                            } else {
                                AuthStatus::TokenMissing
                            }
                        } else {
                            AuthStatus::NotRequired
                        };

                        PendingServerEntry {
                            id: p.id,
                            name: p.server_info.name,
                            endpoint: p.server_info.endpoint,
                            description: p.server_info.description,
                            auth_env_var: p.server_info.auth_env_var,
                            auth_status,
                            tool_count: None, // Could be fetched from capabilities_path
                            risk_level: p
                                .risk_assessment
                                .as_ref()
                                .map(|ra| format!("{:?}", ra.level).to_lowercase())
                                .unwrap_or_else(|| "unknown".to_string()),
                            requested_at: p.requested_at.format("%Y-%m-%d %H:%M").to_string(),
                            requesting_goal: p.requesting_goal,
                        }
                    })
                    .collect();
                let _ = event_tx.send(TuiEvent::PendingServersLoaded(pending_entries));
            }
            Err(e) => {
                let _ = event_tx.send(TuiEvent::ApprovalsError(format!(
                    "Failed to load pending: {}",
                    e
                )));
            }
        }

        // Load approved servers
        match queue.list_approved_servers().await {
            Ok(approved_requests) => {
                let approved_entries: Vec<ApprovedServerEntry> = approved_requests
                    .iter()
                    .filter_map(|r| r.to_approved_discovery())
                    .map(|a| {
                        // Compute these before moving other fields
                        let error_rate = a.error_rate();
                        let tool_count = a.capability_files.as_ref().map(|f| f.len());
                        let approved_at = a.approved_at.format("%Y-%m-%d %H:%M").to_string();

                        ApprovedServerEntry {
                            id: a.id,
                            name: a.server_info.name,
                            endpoint: a.server_info.endpoint,
                            description: a.server_info.description,
                            auth_env_var: a.server_info.auth_env_var,
                            tool_count,
                            approved_at,
                            total_calls: a.total_calls,
                            error_rate,
                        }
                    })
                    .collect();
                let _ = event_tx.send(TuiEvent::ApprovedServersLoaded(approved_entries));
            }
            Err(e) => {
                let _ = event_tx.send(TuiEvent::ApprovalsError(format!(
                    "Failed to load approved: {}",
                    e
                )));
            }
        }
    });
}

/// Approve a pending server
fn approve_server_async(
    event_tx: mpsc::UnboundedSender<TuiEvent>,
    server_id: String,
    server_name: String,
) {
    let _ = event_tx.send(TuiEvent::ApprovalsLoading);

    tokio::task::spawn_local(async move {
        use ccos::approval::ApprovalAuthority;

        let queue = match create_unified_queue() {
            Ok(q) => q,
            Err(e) => {
                let _ = event_tx.send(TuiEvent::ApprovalsError(format!(
                    "Failed to create queue: {}",
                    e
                )));
                return;
            }
        };

        match queue
            .approve(
                &server_id,
                ApprovalAuthority::User("tui".to_string()),
                Some("Approved via TUI".to_string()),
            )
            .await
        {
            Ok(()) => {
                let _ = event_tx.send(TuiEvent::ServerApproved {
                    _server_id: server_id.clone(),
                    server_name: server_name.clone(),
                });
                // Reload the queues
                load_approvals_async(event_tx);
            }
            Err(e) => {
                let _ = event_tx.send(TuiEvent::ApprovalsError(format!(
                    "Failed to approve {}: {}",
                    server_name, e
                )));
            }
        }
    });
}

/// Reject a pending server
fn reject_server_async(
    event_tx: mpsc::UnboundedSender<TuiEvent>,
    server_id: String,
    server_name: String,
) {
    let _ = event_tx.send(TuiEvent::ApprovalsLoading);

    tokio::task::spawn_local(async move {
        use ccos::approval::ApprovalAuthority;

        let queue = match create_unified_queue() {
            Ok(q) => q,
            Err(e) => {
                let _ = event_tx.send(TuiEvent::ApprovalsError(format!(
                    "Failed to create queue: {}",
                    e
                )));
                return;
            }
        };

        match queue
            .reject(
                &server_id,
                ApprovalAuthority::User("tui".to_string()),
                "Rejected via TUI".to_string(),
            )
            .await
        {
            Ok(()) => {
                let _ = event_tx.send(TuiEvent::ServerRejected {
                    _server_id: server_id.clone(),
                    server_name: server_name.clone(),
                });
                // Reload the queues
                load_approvals_async(event_tx);
            }
            Err(e) => {
                let _ = event_tx.send(TuiEvent::ApprovalsError(format!(
                    "Failed to reject {}: {}",
                    server_name, e
                )));
            }
        }
    });
}

/// Dismiss an approved server
fn dismiss_server_async(
    event_tx: mpsc::UnboundedSender<TuiEvent>,
    server_id: String,
    server_name: String,
) {
    let _ = event_tx.send(TuiEvent::ApprovalsLoading);

    tokio::task::spawn_local(async move {
        let queue = match create_unified_queue() {
            Ok(q) => q,
            Err(e) => {
                let _ = event_tx.send(TuiEvent::ApprovalsError(format!(
                    "Failed to create queue: {}",
                    e
                )));
                return;
            }
        };

        match queue
            .dismiss_server(&server_id, "Dismissed via TUI".to_string())
            .await
        {
            Ok(()) => {
                let _ = event_tx.send(TuiEvent::ServerRejected {
                    _server_id: server_id.clone(),
                    server_name: server_name.clone(),
                });
                // Reload the queues
                load_approvals_async(event_tx);
            }
            Err(e) => {
                let _ = event_tx.send(TuiEvent::ApprovalsError(format!(
                    "Failed to dismiss {}: {}",
                    server_name, e
                )));
            }
        }
    });
}

/// Get the base directory for capability storage, respecting agent_config.toml
fn get_capabilities_base_path() -> std::path::PathBuf {
    use ccos::examples_common::builder::load_agent_config;
    use ccos::utils::fs::resolve_workspace_path;

    // Load config to get capabilities directory
    match load_agent_config("config/agent_config.toml") {
        Ok(config) => {
            // resolve_workspace_path handles relative paths from config/ directory
            resolve_workspace_path(&config.storage.capabilities_dir)
        }
        Err(_) => {
            let storage_dir = std::env::var("CCOS_CAPABILITY_STORAGE")
                .map(std::path::PathBuf::from)
                .unwrap_or_else(|_| ccos::utils::fs::get_configured_capabilities_path());
            storage_dir
        }
    }
}

/// Get the base directory for the ApprovalQueue (parent of capabilities_dir)
fn get_approval_queue_base() -> std::path::PathBuf {
    let caps_dir = get_capabilities_base_path();
    // ApprovalQueue appends "capabilities/..." so we need the parent
    caps_dir
        .parent()
        .map(|p| p.to_path_buf())
        .unwrap_or_else(|| {
            use ccos::utils::fs::get_workspace_root;
            get_workspace_root()
        })
}

/// Create a UnifiedApprovalQueue with FileApprovalStorage
fn create_unified_queue() -> Result<
    ccos::approval::UnifiedApprovalQueue<ccos::approval::storage_file::FileApprovalStorage>,
    Box<dyn std::error::Error>,
> {
    let queue_base = get_approval_queue_base();
    let storage_path =
        queue_base.join(&rtfs::config::AgentConfig::from_env().storage.approvals_dir);
    let storage = std::sync::Arc::new(ccos::approval::storage_file::FileApprovalStorage::new(
        storage_path,
    )?);
    Ok(ccos::approval::UnifiedApprovalQueue::new(storage))
}

/// Add a discovered server to the pending approval queue
fn add_server_to_pending_async(
    event_tx: mpsc::UnboundedSender<TuiEvent>,
    server_name: String,
    endpoint: String,
    tools: Vec<DiscoveredCapability>,
) {
    tokio::task::spawn_local(async move {
        use ccos::approval::{
            DiscoverySource, RiskAssessment, RiskLevel, ServerInfo as QueueServerInfo,
        };
        use ccos::mcp::core::MCPDiscoveryService;
        use ccos::mcp::types::{DiscoveredMCPTool, MCPServerConfig};

        let queue = match create_unified_queue() {
            Ok(q) => q,
            Err(e) => {
                let _ = event_tx.send(TuiEvent::ApprovalsError(format!(
                    "Failed to create queue: {}",
                    e
                )));
                return;
            }
        };

        // 1. Convert TUI DiscoveredCapability to DiscoveredMCPTool
        let mcp_tools: Vec<DiscoveredMCPTool> = tools
            .iter()
            .map(|t| {
                DiscoveredMCPTool {
                    tool_name: t.name.clone(),
                    description: Some(t.description.clone()),
                    input_schema: None, // Will use input_schema_json if available
                    output_schema: None,
                    input_schema_json: t
                        .input_schema
                        .as_ref()
                        .and_then(|s| serde_json::from_str(s).ok()),
                }
            })
            .collect();

        // 2. Synthesize RTFS capabilities file
        let mut capabilities_path = None;
        if !mcp_tools.is_empty() {
            let server_config = MCPServerConfig {
                name: server_name.clone(),
                endpoint: endpoint.clone(),
                auth_token: None,
                timeout_seconds: 30,
                protocol_version: "2024-11-05".to_string(),
            };

            let service = MCPDiscoveryService::new();
            let manifest_results: Vec<_> = mcp_tools
                .iter()
                .map(|tool| service.tool_to_manifest(tool, &server_config))
                .collect();

            if !manifest_results.is_empty() {
                // Create directory for pending server in the CONFIGURED capabilities dir
                let sanitized_name = ccos::utils::fs::sanitize_filename(&server_name);
                let caps_dir = get_capabilities_base_path();
                let server_dir = caps_dir.join("servers/pending").join(&sanitized_name);

                if let Ok(_) = std::fs::create_dir_all(&server_dir) {
                    let file_path = server_dir.join("capabilities.rtfs");

                    // Generate RTFS content
                    let mut rtfs_content = String::new();
                    for manifest in manifest_results {
                        rtfs_content.push_str(&format!(";; Capability: {}\n", manifest.id));
                        rtfs_content.push_str(&format!(
                            "(define-capability {} [args]\n  (mcp-call \"{}\" \"{}\" args))\n\n",
                            manifest.id, endpoint, manifest.name
                        ));
                    }

                    if let Ok(_) = std::fs::write(&file_path, rtfs_content) {
                        capabilities_path = Some(file_path.to_string_lossy().to_string());
                    }
                }
            }
        }

        // 3. Create server info and add via unified queue
        let tool_count = tools.len();
        let auth_env_var = suggest_auth_env_var(&server_name);
        let server_info = QueueServerInfo {
            name: server_name.clone(),
            endpoint: endpoint.clone(),
            description: Some(format!("Discovered via TUI ({} tools)", tool_count)),
            auth_env_var: Some(auth_env_var),
            capabilities_path: capabilities_path.clone(),
            alternative_endpoints: vec![],
            capability_files: capabilities_path.map(|p| vec![p]),
        };

        match queue
            .add_server_discovery(
                DiscoverySource::Manual {
                    user: "tui_user".to_string(),
                },
                server_info,
                vec!["discovered".to_string()],
                RiskAssessment {
                    level: RiskLevel::Medium,
                    reasons: vec!["Discovered via interactive search".to_string()],
                },
                None,    // requesting_goal
                24 * 30, // expires_in_hours (30 days)
            )
            .await
        {
            Ok(pending_id) => {
                let _ = event_tx.send(TuiEvent::ServerAddedToPending {
                    server_name: server_name.clone(),
                    _pending_id: pending_id,
                });
                let _ = event_tx.send(TuiEvent::Trace(
                    TraceEventType::ToolDiscovery,
                    format!("Server '{}' added to pending queue", server_name),
                    Some(format!("Endpoint: {}\nTools: {}", endpoint, tool_count)),
                ));
            }
            Err(e) => {
                let _ = event_tx.send(TuiEvent::ApprovalsError(format!(
                    "Failed to add {} to pending: {}",
                    server_name, e
                )));
            }
        }
    });
}

/// Suggest an auth env var based on server name (helper)
fn suggest_auth_env_var(server_name: &str) -> String {
    let parts: Vec<&str> = server_name
        .split(|c| c == '/' || c == '-' || c == '_')
        .collect();
    let namespace = parts.first().unwrap_or(&server_name);
    format!("{}_MCP_TOKEN", namespace.to_uppercase())
}

fn load_servers_async(event_tx: mpsc::UnboundedSender<TuiEvent>) {
    // Signal loading started
    let _ = event_tx.send(TuiEvent::ServersLoading);

    tokio::task::spawn_local(async move {
        use ccos::mcp::core::MCPDiscoveryService;
        use std::collections::HashSet;

        let mut servers: Vec<ServerInfo> = Vec::new();
        let mut seen_endpoints: HashSet<String> = HashSet::new();

        // First, load approved servers from the approval queue
        if let Ok(queue) = create_unified_queue() {
            if let Ok(approved_requests) = queue.list_approved_servers().await {
                for r in approved_requests {
                    if let Some(a) = r.to_approved_discovery() {
                        seen_endpoints.insert(a.server_info.endpoint.clone());
                        servers.push(ServerInfo {
                            name: format!("✓ {}", a.server_info.name), // Mark as approved
                            endpoint: a.server_info.endpoint,
                            status: if a.consecutive_failures > 0 {
                                ServerStatus::Error
                            } else if a.total_calls > 0 {
                                ServerStatus::Connected
                            } else {
                                ServerStatus::Unknown
                            },
                            tool_count: a.capability_files.as_ref().map(|f| f.len()),
                            tools: vec![],
                            last_checked: None, // Would need live check
                        });
                    }
                }
            }
        }

        // Then, add known servers from config (if not already in approved list)
        let service = MCPDiscoveryService::new();
        let mcp_servers = service.list_known_servers().await;

        for config in mcp_servers {
            if !seen_endpoints.contains(&config.endpoint) {
                servers.push(ServerInfo {
                    name: config.name,
                    endpoint: config.endpoint.clone(),
                    status: ServerStatus::Unknown,
                    tool_count: None,
                    tools: vec![],
                    last_checked: None,
                });
                seen_endpoints.insert(config.endpoint);
            }
        }

        let _ = event_tx.send(TuiEvent::ServersLoaded(servers));
    });
}

/// Discover tools for a specific server
fn discover_server_tools_async(
    event_tx: mpsc::UnboundedSender<TuiEvent>,
    server_index: usize,
    endpoint: String,
) {
    tokio::task::spawn_local(async move {
        use ccos::mcp::core::MCPDiscoveryService;
        use ccos::mcp::types::DiscoveryOptions;

        let service = MCPDiscoveryService::new();

        // Find the server config matching this endpoint
        let server_config = service
            .list_known_servers()
            .await
            .into_iter()
            .find(|s| s.endpoint == endpoint);

        if let Some(config) = server_config {
            let options = DiscoveryOptions::default();
            match service.discover_tools(&config, &options).await {
                Ok(tools) => {
                    let tool_names: Vec<String> =
                        tools.iter().map(|t| t.tool_name.clone()).collect();
                    let _ = event_tx.send(TuiEvent::ServerToolsDiscovered {
                        server_index,
                        tool_count: tools.len(),
                        tool_names,
                    });
                }
                Err(_) => {
                    let _ = event_tx.send(TuiEvent::ServerConnectionChecked {
                        server_index,
                        status: ServerStatus::Error,
                    });
                }
            }
        } else {
            // Server not in known configs, report as unknown
            let _ = event_tx.send(TuiEvent::ServerConnectionChecked {
                server_index,
                status: ServerStatus::Disconnected,
            });
        }
    });
}

/// Check connection to a specific server
fn check_server_connection_async(
    event_tx: mpsc::UnboundedSender<TuiEvent>,
    server_index: usize,
    endpoint: String,
) {
    tokio::task::spawn_local(async move {
        use ccos::mcp::core::MCPDiscoveryService;
        use ccos::mcp::types::DiscoveryOptions;

        let service = MCPDiscoveryService::new();

        // Find the server config matching this endpoint
        let server_config = service
            .list_known_servers()
            .await
            .into_iter()
            .find(|s| s.endpoint == endpoint);

        let status = if let Some(config) = server_config {
            // Try to discover tools as a connection check
            let options = DiscoveryOptions::default();
            match service.discover_tools(&config, &options).await {
                Ok(_) => ServerStatus::Connected,
                Err(_) => ServerStatus::Error,
            }
        } else {
            ServerStatus::Disconnected
        };

        let _ = event_tx.send(TuiEvent::ServerConnectionChecked {
            server_index,
            status,
        });
    });
}

/// Load local/builtin capabilities from the registry
fn load_local_capabilities_async(event_tx: mpsc::UnboundedSender<TuiEvent>) {
    // Signal loading started
    let _ = event_tx.send(TuiEvent::DiscoverLoading);

    tokio::task::spawn_local(async move {
        use ccos::capabilities::registry::CapabilityRegistry;
        use std::collections::HashMap;

        // Create a capability registry and get all registered capabilities
        let registry = CapabilityRegistry::new();

        // Get all capability IDs from the registry
        let capability_ids = registry.list_capabilities();

        let mut capabilities: Vec<DiscoveredCapability> = capability_ids
            .into_iter()
            .map(|id| {
                // Determine category based on capability ID prefix
                let category = if id.starts_with("ccos.") {
                    CapabilityCategory::Builtin
                } else if id.contains(".rtfs.") || id.ends_with(".rtfs") {
                    CapabilityCategory::RtfsFunction
                } else if id.starts_with("mcp.") {
                    CapabilityCategory::McpTool
                } else {
                    CapabilityCategory::Builtin
                };

                // Extract a human-readable name from the ID
                let name = id.split('.').last().unwrap_or(id).to_string();

                // Try to get the full capability to extract schemas and description
                let (description, input_schema, output_schema) = registry
                    .get_capability(id)
                    .map(|cap| {
                        let desc = cap
                            .description
                            .clone()
                            .unwrap_or_else(|| format!("Built-in capability: {}", id));
                        let input = cap.input_schema.as_ref().map(|s| s.to_string());
                        let output = cap.output_schema.as_ref().map(|s| s.to_string());
                        (desc, input, output)
                    })
                    .unwrap_or_else(|| (format!("Built-in capability: {}", id), None, None));

                DiscoveredCapability {
                    id: id.to_string(),
                    name,
                    description,
                    source: "Local Registry".to_string(),
                    category,
                    version: Some("1.0.0".to_string()),
                    input_schema,
                    output_schema,
                    permissions: vec![],
                    effects: vec![],
                    metadata: HashMap::new(),
                }
            })
            .collect();

        // Add known API endpoints when authorization is available
        capabilities.extend(load_known_api_capabilities());

        // Add core capabilities from files
        capabilities.extend(load_core_capabilities());

        // Load capabilities from approved servers (from RTFS files on disk - no network needed)
        capabilities.extend(load_approved_server_capabilities());

        // NOTE: MCP server tools are NOT loaded here to avoid blocking the TUI.
        // Users can load server tools on-demand via the Servers view (press 5)
        // or by using the search functionality (press 's' or '/').

        let _ = event_tx.send(TuiEvent::LocalCapabilitiesLoaded(capabilities));
    });
}

fn load_known_api_capabilities() -> Vec<DiscoveredCapability> {
    use ccos::synthesis::introspection::known_apis::KnownApisRegistry;
    use std::collections::HashMap;

    let registry = match KnownApisRegistry::new() {
        Ok(r) => r,
        Err(e) => {
            log::warn!("Failed to load known APIs: {}", e);
            return Vec::new();
        }
    };

    let mut caps = Vec::new();

    for api in registry.list_apis() {
        // Skip APIs that require auth if we do not have the required token
        let authorized = match &api.auth {
            Some(auth) if auth.required => match &auth.env_var {
                Some(var) => std::env::var(var).is_ok(),
                None => false,
            },
            _ => true,
        };

        if !authorized {
            continue;
        }

        for ep in &api.endpoints {
            let mut metadata = HashMap::new();
            metadata.insert("base_url".to_string(), api.api.base_url.clone());
            metadata.insert("path".to_string(), ep.path.clone());
            metadata.insert("method".to_string(), ep.method.clone());

            let name = format!("{}::{}", api.api.name, ep.id);
            let description = format!("{} [{} {}]", ep.description, ep.method, ep.path);

            caps.push(DiscoveredCapability {
                id: name.clone(),
                name,
                description,
                source: format!("Known API: {}", api.api.title),
                category: CapabilityCategory::Builtin,
                version: Some(api.api.version.clone()),
                input_schema: None,
                output_schema: None,
                permissions: Vec::new(),
                effects: Vec::new(),
                metadata,
            });
        }
    }

    caps
}

fn load_core_capabilities() -> Vec<DiscoveredCapability> {
    use ccos::capability_marketplace::mcp_discovery::MCPDiscoveryProvider;
    use ccos::capability_marketplace::mcp_discovery::MCPServerConfig;
    use ccos::examples_common::builder::load_agent_config;
    use ccos::utils::fs::resolve_workspace_path;

    let mut caps = Vec::new();

    // Load config to get capabilities directory
    let config = match load_agent_config("config/agent_config.toml") {
        Ok(c) => c,
        Err(e) => {
            ccos_eprintln!("load_core_capabilities: Failed to load config: {}", e);
            return caps;
        }
    };

    // Use capabilities_dir from config, relative to workspace root (config/ directory)
    let core_dir = resolve_workspace_path(&config.storage.capabilities_dir).join("core");
    ccos_eprintln!(
        "load_core_capabilities: Looking for core capabilities in {:?}",
        core_dir
    );

    let parser = match MCPDiscoveryProvider::new(MCPServerConfig::default()) {
        Ok(p) => p,
        Err(e) => {
            ccos_eprintln!("load_core_capabilities: Failed to create parser: {}", e);
            return caps;
        }
    };

    if core_dir.exists() && core_dir.is_dir() {
        if let Ok(entries) = std::fs::read_dir(&core_dir) {
            for entry in entries.flatten() {
                let path = entry.path();
                if path.is_file() && path.extension().map_or(false, |ext| ext == "rtfs") {
                    ccos_eprintln!("load_core_capabilities: Loading {:?}", path);
                    match parser.load_rtfs_capabilities(path.to_str().unwrap_or_default()) {
                        Ok(module) => {
                            ccos_eprintln!("load_core_capabilities: Loaded module with {} capabilities from {:?}", module.capabilities.len(), path);
                            for cap_def in module.capabilities {
                                match parser.rtfs_to_capability_manifest(&cap_def) {
                                    Ok(manifest) => {
                                        ccos_eprintln!(
                                            "load_core_capabilities: Converted manifest for {}",
                                            manifest.id
                                        );
                                        caps.push(DiscoveredCapability {
                                            id: manifest.id.clone(),
                                            name: manifest.name.clone(),
                                            description: manifest.description.clone(),
                                            source: "Core".to_string(),
                                            category: CapabilityCategory::RtfsFunction,
                                            version: Some(manifest.version.clone()),
                                            input_schema: manifest
                                                .input_schema
                                                .as_ref()
                                                .map(|s| s.to_string()),
                                            output_schema: manifest
                                                .output_schema
                                                .as_ref()
                                                .map(|s| s.to_string()),
                                            permissions: Vec::new(),
                                            effects: Vec::new(),
                                            metadata: manifest.metadata.clone(),
                                        });
                                    }
                                    Err(e) => {
                                        ccos_eprintln!("load_core_capabilities: Failed to convert manifest for cap in {:?}: {}", path, e);
                                    }
                                }
                            }
                        }
                        Err(e) => {
                            ccos_eprintln!(
                                "load_core_capabilities: Failed to load RTFS from {:?}: {}",
                                path,
                                e
                            );
                        }
                    }
                }
            }
        }
    } else {
        ccos_eprintln!(
            "load_core_capabilities: core_dir does not exist: {:?}",
            core_dir
        );
    }

    ccos_eprintln!(
        "load_core_capabilities: Loaded {} core capabilities total",
        caps.len()
    );
    caps
}

/// Load capabilities from approved server RTFS files (no network required)
fn load_approved_server_capabilities() -> Vec<DiscoveredCapability> {
    use ccos::capability_marketplace::mcp_discovery::MCPDiscoveryProvider;
    use ccos::capability_marketplace::mcp_discovery::MCPServerConfig;
    use ccos::utils::fs::resolve_workspace_path;

    let mut caps = Vec::new();

    // Look in capabilities/servers/approved/ for RTFS files
    let approved_dir = resolve_workspace_path("capabilities/servers/approved");
    ccos_eprintln!(
        "load_approved_server_capabilities: Looking for approved servers in {:?}",
        approved_dir
    );

    if !approved_dir.exists() || !approved_dir.is_dir() {
        ccos_eprintln!(
            "load_approved_server_capabilities: approved_dir does not exist: {:?}",
            approved_dir
        );
        return caps;
    }

    let parser = match MCPDiscoveryProvider::new(MCPServerConfig::default()) {
        Ok(p) => p,
        Err(e) => {
            ccos_eprintln!(
                "load_approved_server_capabilities: Failed to create parser: {}",
                e
            );
            return caps;
        }
    };

    // Iterate over subdirectories (each subdirectory is a server)
    if let Ok(entries) = std::fs::read_dir(&approved_dir) {
        for entry in entries.flatten() {
            let server_dir = entry.path();
            if !server_dir.is_dir() {
                continue;
            }

            let server_name = server_dir
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or("unknown")
                .to_string();

            // Recursively find all .rtfs capability files (not server.rtfs manifest)
            fn find_rtfs_files(dir: &std::path::Path) -> Vec<std::path::PathBuf> {
                let mut files = Vec::new();
                if let Ok(entries) = std::fs::read_dir(dir) {
                    for entry in entries.flatten() {
                        let path = entry.path();
                        if path.is_dir() {
                            files.extend(find_rtfs_files(&path));
                        } else if path.is_file() {
                            // Only include .rtfs files, skip server.rtfs manifests
                            if path.extension().map_or(false, |ext| ext == "rtfs") {
                                if let Some(file_name) = path.file_name().and_then(|n| n.to_str()) {
                                    if file_name != "server.rtfs" {
                                        files.push(path);
                                    }
                                }
                            }
                        }
                    }
                }
                files
            }

            let rtfs_files = find_rtfs_files(&server_dir);
            ccos_eprintln!(
                "load_approved_server_capabilities: Found {} RTFS files in server {}",
                rtfs_files.len(),
                server_name
            );

            for path in rtfs_files {
                match parser.load_rtfs_capabilities(path.to_str().unwrap_or_default()) {
                    Ok(module) => {
                        for cap_def in module.capabilities {
                            match parser.rtfs_to_capability_manifest(&cap_def) {
                                Ok(manifest) => {
                                    caps.push(DiscoveredCapability {
                                        id: manifest.id.clone(),
                                        name: manifest.name.clone(),
                                        description: manifest.description.clone(),
                                        source: format!("✓ {}", server_name),
                                        category: CapabilityCategory::McpTool,
                                        version: Some(manifest.version.clone()),
                                        input_schema: manifest
                                            .input_schema
                                            .as_ref()
                                            .map(|s| s.to_string()),
                                        output_schema: manifest
                                            .output_schema
                                            .as_ref()
                                            .map(|s| s.to_string()),
                                        permissions: Vec::new(),
                                        effects: Vec::new(),
                                        metadata: manifest.metadata.clone(),
                                    });
                                }
                                Err(e) => {
                                    ccos_eprintln!(
                                        "load_approved_server_capabilities: Failed to convert manifest from {:?}: {}",
                                        path,
                                        e
                                    );
                                }
                            }
                        }
                    }
                    Err(e) => {
                        ccos_eprintln!(
                            "load_approved_server_capabilities: Failed to load {:?}: {}",
                            path,
                            e
                        );
                    }
                }
            }
        }
    }

    ccos_eprintln!(
        "load_approved_server_capabilities: Loaded {} capabilities from approved servers",
        caps.len()
    );
    caps
}

fn search_discovery_async(query: String, event_tx: mpsc::UnboundedSender<TuiEvent>) {
    use ccos::ops::server::search_servers;

    let _ = event_tx.send(TuiEvent::DiscoverySearchStarted);

    let query_clone = query.clone();
    tokio::task::spawn_local(async move {
        // First add a trace showing what we're searching for
        let _ = event_tx.send(TuiEvent::Trace(
            TraceEventType::ToolDiscovery,
            format!("Calling search_servers for query: '{}'", query_clone),
            None,
        ));

        // Search registry for servers/capabilities matching query
        // Pass None for capability filter to get all matching servers first
        match search_servers(query_clone.clone(), None, false, None).await {
            Ok(server_infos) => {
                // Log how many we found
                let _ = event_tx.send(TuiEvent::Trace(
                    TraceEventType::ToolDiscovery,
                    format!("search_servers returned {} servers", server_infos.len()),
                    None,
                ));

                // Build list of servers for popup
                let discovered: Vec<DiscoverySearchResult> = server_infos
                    .iter()
                    .map(|info| DiscoverySearchResult {
                        name: info.name.clone(),
                        endpoint: info.endpoint.clone(),
                        description: info.description.clone(),
                        source: "MCP Registry".to_string(),
                    })
                    .collect();

                let _ = event_tx.send(TuiEvent::DiscoverySearchComplete(discovered));
            }
            Err(e) => {
                // Log the error so user knows what happened
                let _ = event_tx.send(TuiEvent::Trace(
                    TraceEventType::ToolDiscovery,
                    format!("Discovery search failed: {}", e),
                    None,
                ));
                let _ = event_tx.send(TuiEvent::DiscoverySearchComplete(vec![]));
            }
        }
    });
}

async fn introspect_server_async(
    server_name: String,
    endpoint: String,
    event_tx: mpsc::UnboundedSender<TuiEvent>,
) {
    use ccos::ops::server::introspect_server_by_url;

    let _ = event_tx.send(TuiEvent::IntrospectionLog(format!(
        "Initializing session with {}...",
        endpoint
    )));
    let _ = event_tx.send(TuiEvent::Trace(
        TraceEventType::ToolDiscovery,
        format!("Introspecting {} at {}", server_name, endpoint),
        None,
    ));

    // Determine the auth env var to use
    let suggested_env_var = suggest_auth_env_var(&server_name);

    // Check if token is available
    let auth_env_var = if std::env::var(&suggested_env_var).is_ok() {
        let _ = event_tx.send(TuiEvent::IntrospectionLog(format!(
            "Using auth token from {}",
            suggested_env_var
        )));
        Some(suggested_env_var.as_str())
    } else if std::env::var("MCP_AUTH_TOKEN").is_ok() {
        let _ = event_tx.send(TuiEvent::IntrospectionLog(
            "Using auth token from MCP_AUTH_TOKEN".to_string(),
        ));
        Some("MCP_AUTH_TOKEN")
    } else {
        let _ = event_tx.send(TuiEvent::IntrospectionLog(format!(
            "No auth token found (checked {} and MCP_AUTH_TOKEN)",
            suggested_env_var
        )));
        None
    };

    // Use the same introspection function as the CLI
    match introspect_server_by_url(&endpoint, &server_name, auth_env_var).await {
        Ok(result) => {
            let _ = event_tx.send(TuiEvent::IntrospectionLog(format!(
                "Found {} tools.",
                result.tools.len()
            )));

            let discovered_tools: Vec<DiscoveredCapability> = result
                .tools
                .iter()
                .map(|tool| DiscoveredCapability {
                    id: format!("mcp:{}:{}", server_name, tool.tool_name),
                    name: tool.tool_name.clone(),
                    description: tool.description.clone().unwrap_or_default(),
                    source: server_name.clone(),
                    category: CapabilityCategory::McpTool,
                    version: None,
                    input_schema: tool.input_schema_json.as_ref().map(|v| v.to_string()),
                    output_schema: None,
                    permissions: Vec::new(),
                    effects: Vec::new(),
                    metadata: std::collections::HashMap::new(),
                })
                .collect();

            let _ = event_tx.send(TuiEvent::IntrospectionComplete {
                server_name: server_name.clone(),
                endpoint: endpoint.clone(),
                tools: discovered_tools,
            });
        }
        Err(e) => {
            let error_str = format!("{}", e);

            // Log full error for debugging
            let _ = event_tx.send(TuiEvent::Trace(
                TraceEventType::Error,
                format!("Introspection failed for {}: {}", server_name, error_str),
                None,
            ));

            // Check if it's an auth error
            if error_str.contains("MCP_AUTH_TOKEN")
                || error_str.contains("not set")
                || error_str.contains("401")
                || error_str.contains("Unauthorized")
                || error_str.contains("authentication")
                || error_str.contains("token")
                || error_str.contains("auth")
            {
                // Suggest auth env var based on server name
                let env_var = suggest_auth_env_var(&server_name);

                let _ = event_tx.send(TuiEvent::IntrospectionAuthRequired {
                    server_name: server_name.clone(),
                    endpoint: endpoint.clone(),
                    env_var,
                });
            } else {
                let _ = event_tx.send(TuiEvent::IntrospectionFailed {
                    server_name,
                    error: error_str,
                });
            }
        }
    }
}

fn handle_discover_input(
    state: &mut AppState,
    key: event::KeyEvent,
    event_tx: mpsc::UnboundedSender<TuiEvent>,
) {
    match key.code {
        KeyCode::Enter => {
            // Trigger search
            if !state.discover_search_hint.is_empty() && !state.discover_loading {
                search_discovery_async(state.discover_search_hint.clone(), event_tx);
                // Move focus to list while loading
                state.active_panel = ActivePanel::DiscoverList;
                state.discover_input_active = false;
                state.discover_selected = 0;
            }
        }
        KeyCode::Esc => {
            state.discover_input_active = false;
            state.active_panel = ActivePanel::DiscoverList;
            state.discover_search_hint.clear();
            state.discover_selected = 0;
        }
        KeyCode::Backspace => {
            state.discover_search_hint.pop();
            state.discover_selected = 0;
        }
        KeyCode::Char(c) => {
            state.discover_search_hint.push(c);
            state.discover_selected = 0;
        }
        _ => {}
    }
}

/// Handle keyboard events when a discover popup is active
fn handle_discover_popup_key(
    state: &mut AppState,
    key: event::KeyEvent,
    event_tx: mpsc::UnboundedSender<TuiEvent>,
) {
    let mut next_popup = None;

    match &mut state.discover_popup {
        DiscoverPopup::SearchResults { servers, selected } => {
            match key.code {
                KeyCode::Up | KeyCode::Char('k') => {
                    *selected = selected.saturating_sub(1);
                }
                KeyCode::Down | KeyCode::Char('j') => {
                    if !servers.is_empty() {
                        *selected = (*selected + 1).min(servers.len() - 1);
                    }
                }
                KeyCode::Enter => {
                    // Start introspection
                    if let Some(server) = servers.get(*selected) {
                        next_popup = Some(DiscoverPopup::Introspecting {
                            server_name: server.name.clone(),
                            endpoint: server.endpoint.clone(),
                            logs: Vec::new(),
                        });

                        // Spawn async task for introspection
                        let server_name = server.name.clone();
                        let endpoint = server.endpoint.clone();
                        let event_tx_clone = event_tx.clone();

                        tokio::task::spawn_local(async move {
                            introspect_server_async(server_name, endpoint, event_tx_clone).await;
                        });
                    }
                }
                KeyCode::Esc => {
                    next_popup = Some(DiscoverPopup::None);
                }
                _ => {}
            }
        }
        DiscoverPopup::IntrospectionResults {
            tools,
            selected,
            server_name,
            endpoint,
            selected_tools,
        } => {
            match key.code {
                KeyCode::Up | KeyCode::Char('k') => {
                    if !tools.is_empty() {
                        *selected = selected.saturating_sub(1);
                    }
                }
                KeyCode::Down | KeyCode::Char('j') => {
                    if !tools.is_empty() {
                        *selected = (*selected + 1).min(tools.len() - 1);
                    }
                }
                KeyCode::Char(' ') => {
                    // Toggle selected tool using HashSet
                    if selected_tools.contains(selected) {
                        selected_tools.remove(selected);
                    } else {
                        selected_tools.insert(*selected);
                    }
                }
                KeyCode::Enter => {
                    // Accept selected tools - add them to discovered_capabilities (in-memory only)
                    let tools_to_add: Vec<_> = selected_tools
                        .iter()
                        .filter_map(|idx| tools.get(*idx).cloned())
                        .collect();

                    for tool in tools_to_add {
                        state.discovered_capabilities.push(DiscoveredCapability {
                            id: tool.id.clone(),
                            name: tool.name.clone(),
                            source: server_name.clone(),
                            description: tool.description.clone(),
                            category: tool.category,
                            version: tool.version.clone(),
                            input_schema: tool.input_schema.clone(),
                            output_schema: tool.output_schema.clone(),
                            permissions: tool.permissions.clone(),
                            effects: tool.effects.clone(),
                            metadata: tool.metadata.clone(),
                        });
                    }
                    next_popup = Some(DiscoverPopup::None);
                }
                KeyCode::Char('p') => {
                    // Add server to pending approval queue (persisted)
                    let server_name_clone = server_name.clone();
                    let endpoint_clone = endpoint.clone();
                    let event_tx_clone = event_tx.clone();

                    // Filter to only selected tools if any, otherwise take all
                    let tools_to_save = if selected_tools.is_empty() {
                        tools.clone()
                    } else {
                        selected_tools
                            .iter()
                            .filter_map(|idx| tools.get(*idx).cloned())
                            .collect()
                    };

                    add_server_to_pending_async(
                        event_tx_clone,
                        server_name_clone,
                        endpoint_clone,
                        tools_to_save,
                    );
                    next_popup = Some(DiscoverPopup::None);
                }
                KeyCode::Char('a') => {
                    // Select all
                    for i in 0..tools.len() {
                        selected_tools.insert(i);
                    }
                }
                KeyCode::Char('n') => {
                    // Select none
                    selected_tools.clear();
                }
                KeyCode::Esc => {
                    next_popup = Some(DiscoverPopup::None);
                }
                _ => {}
            }
        }
        DiscoverPopup::Introspecting { .. } => {
            if let KeyCode::Esc = key.code {
                next_popup = Some(DiscoverPopup::None);
            }
        }
        DiscoverPopup::Error { .. } => {
            if let KeyCode::Esc | KeyCode::Enter = key.code {
                next_popup = Some(DiscoverPopup::None);
            }
        }
        DiscoverPopup::None => {}
    }

    if let Some(popup) = next_popup {
        state.discover_popup = popup;
    }
}

fn handle_discover_list(state: &mut AppState, key: event::KeyEvent) {
    let visible_entries = state.visible_discovery_entries();
    let visible_len = visible_entries.len();
    let visible_height = state.discover_panel_height;

    match key.code {
        KeyCode::Up | KeyCode::Char('k') => {
            state.discover_selected = state.discover_selected.saturating_sub(1);
            // Scroll up if selection moves above visible area
            if state.discover_selected < state.discover_scroll {
                state.discover_scroll = state.discover_selected;
            }
        }
        KeyCode::Down | KeyCode::Char('j') => {
            if visible_len > 0 {
                state.discover_selected = (state.discover_selected + 1).min(visible_len - 1);
                // Scroll down if selection moves below visible area
                if state.discover_selected >= state.discover_scroll + visible_height {
                    state.discover_scroll =
                        state.discover_selected.saturating_sub(visible_height - 1);
                }
            }
        }
        KeyCode::PageUp => {
            state.discover_selected = state.discover_selected.saturating_sub(visible_height);
            state.discover_scroll = state.discover_scroll.saturating_sub(visible_height);
            // Ensure selection is visible
            if state.discover_selected < state.discover_scroll {
                state.discover_scroll = state.discover_selected;
            }
        }
        KeyCode::PageDown => {
            if visible_len > 0 {
                state.discover_selected =
                    (state.discover_selected + visible_height).min(visible_len - 1);
                // Scroll down to keep selection visible
                if state.discover_selected >= state.discover_scroll + visible_height {
                    state.discover_scroll =
                        state.discover_selected.saturating_sub(visible_height - 1);
                }
            }
        }
        KeyCode::Home | KeyCode::Char('g') => {
            state.discover_selected = 0;
            state.discover_scroll = 0;
        }
        KeyCode::End | KeyCode::Char('G') => {
            if visible_len > 0 {
                state.discover_selected = visible_len - 1;
                // Scroll to show end of list
                state.discover_scroll = visible_len.saturating_sub(visible_height);
            }
        }
        KeyCode::Char('/') | KeyCode::Char('s') => {
            state.active_panel = ActivePanel::DiscoverInput;
            state.discover_input_active = true;
            state.discover_selected = 0;
            state.discover_scroll = 0;
        }
        KeyCode::Char('c') | KeyCode::Char(' ') | KeyCode::Enter => {
            // Toggle collapse for the currently selected source
            if let Some(entry) =
                visible_entries.get(state.discover_selected.min(visible_len.saturating_sub(1)))
            {
                match entry {
                    DiscoveryEntry::Header { name, is_local } => {
                        if *is_local {
                            state.discover_local_collapsed = !state.discover_local_collapsed;
                            if state.discover_local_collapsed {
                                state
                                    .discover_collapsed_sources
                                    .insert("Local Capabilities".to_string());
                                state
                                    .discover_expanded_sources
                                    .remove(&"Local Capabilities".to_string());
                            } else {
                                state
                                    .discover_collapsed_sources
                                    .remove("Local Capabilities");
                                state
                                    .discover_expanded_sources
                                    .insert("Local Capabilities".to_string());
                            }
                        } else {
                            // Toggle for non-local sources with all_collapsed_by_default support
                            let is_currently_expanded =
                                state.discover_expanded_sources.contains(name);
                            let is_explicitly_collapsed =
                                state.discover_collapsed_sources.contains(name);

                            if is_explicitly_collapsed {
                                // Was explicitly collapsed, expand it
                                state.discover_collapsed_sources.remove(name);
                                state.discover_expanded_sources.insert(name.clone());
                            } else if is_currently_expanded {
                                // Was explicitly expanded, collapse it
                                state.discover_expanded_sources.remove(name);
                                // If all_collapsed_by_default is true, removing from expanded is enough
                                // Otherwise, we need to add to collapsed
                                if !state.discover_all_collapsed_by_default {
                                    state.discover_collapsed_sources.insert(name.clone());
                                }
                            } else {
                                // Default state (collapsed due to all_collapsed_by_default), expand it
                                state.discover_expanded_sources.insert(name.clone());
                            }
                        }
                    }
                    DiscoveryEntry::Capability(idx) => {
                        if let KeyCode::Char('c') = key.code {
                            if let Some((_, cap)) = state.filtered_discovered_caps().get(*idx) {
                                let source = cap.source.clone();
                                if source == "Local"
                                    || source == "Local Registry"
                                    || source == "Core"
                                {
                                    state.discover_local_collapsed =
                                        !state.discover_local_collapsed;
                                    if state.discover_local_collapsed {
                                        state
                                            .discover_collapsed_sources
                                            .insert("Local Capabilities".to_string());
                                        state
                                            .discover_expanded_sources
                                            .remove(&"Local Capabilities".to_string());
                                    } else {
                                        state
                                            .discover_collapsed_sources
                                            .remove("Local Capabilities");
                                        state
                                            .discover_expanded_sources
                                            .insert("Local Capabilities".to_string());
                                    }
                                } else {
                                    // Toggle for non-local sources
                                    let is_currently_expanded =
                                        state.discover_expanded_sources.contains(&source);
                                    let is_explicitly_collapsed =
                                        state.discover_collapsed_sources.contains(&source);

                                    if is_explicitly_collapsed {
                                        state.discover_collapsed_sources.remove(&source);
                                        state.discover_expanded_sources.insert(source);
                                    } else if is_currently_expanded {
                                        state.discover_expanded_sources.remove(&source);
                                        if !state.discover_all_collapsed_by_default {
                                            state.discover_collapsed_sources.insert(source);
                                        }
                                    } else {
                                        state.discover_expanded_sources.insert(source);
                                    }
                                }
                            }
                        }
                    }
                }
            }
            // Ensure selection remains in bounds after collapse
            let new_visible_len = state.visible_discovery_entries().len();
            if new_visible_len > 0 {
                state.discover_selected = state.discover_selected.min(new_visible_len - 1);
                // Also adjust scroll if needed
                if state.discover_scroll > state.discover_selected {
                    state.discover_scroll = state.discover_selected;
                }
            } else {
                state.discover_selected = 0;
                state.discover_scroll = 0;
            }
        }
        _ => {}
    }
}
