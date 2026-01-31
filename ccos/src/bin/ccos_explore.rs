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
        self, Event, KeyCode, KeyModifiers, MouseEventKind,
        KeyEventKind,
    },
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::{backend::CrosstermBackend, Terminal};
use tokio::sync::mpsc;
use tokio::task::LocalSet;
use reqwest::Client;

use ccos::ccos_eprintln;
use ccos::examples_common::builder::ModularPlannerBuilder;
use ccos::planner::modular_planner::decomposition::llm_adapter::LlmInteractionCapture;
use ccos::planner::modular_planner::orchestrator::TraceEvent;
use ccos::synthesis::core::schema_serializer::{type_expr_to_rtfs_pretty,};

use ccos::tui::{
    panels,
    state::{
        ActivePanel, AppState, ApprovalsTab, ApprovedServerEntry, AuthStatus, AuthTokenPopup,
        BudgetApprovalEntry, CapabilityCategory, CapabilityResolution, CapabilitySource, DecompNode,
        DiscoverPopup, DiscoveredCapability, DiscoveryEntry, ExecutionMode, LlmInteraction,
        NodeStatus, PendingServerEntry, ServerInfo, ServerStatus, TraceEventType, View,
        ChatAuditEntry,
    },
};
use ccos::ops::introspection_service::IntrospectionService;
use ccos::ops::browser_discovery::BrowserDiscoveryService;
use ccos::discovery::registry_search::{RegistrySearchResult, DiscoveryCategory};
use ccos::discovery::RegistrySearcher;

#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize)]
struct HiddenServersConfig {
    #[serde(default)]
    names: Vec<String>,
    #[serde(default)]
    endpoints: Vec<String>,
}

fn hidden_servers_path() -> std::path::PathBuf {
    get_capabilities_base_path().join("servers/hidden_servers.json")
}

fn load_hidden_servers_config() -> HiddenServersConfig {
    let path = hidden_servers_path();
    let Ok(contents) = std::fs::read_to_string(&path) else {
        return HiddenServersConfig::default();
    };
    serde_json::from_str(&contents).unwrap_or_default()
}

fn save_hidden_servers_config(cfg: &HiddenServersConfig) -> std::io::Result<()> {
    let path = hidden_servers_path();
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let json = serde_json::to_string_pretty(cfg)
        .unwrap_or_else(|_| "{\n  \"names\": [],\n  \"endpoints\": []\n}".to_string());
    std::fs::write(path, json)
}

fn hide_known_server_async(
    event_tx: mpsc::UnboundedSender<TuiEvent>,
    server_name: String,
    endpoint: String,
) {
    tokio::task::spawn_local(async move {
        let mut cfg = load_hidden_servers_config();
        if !cfg.names.iter().any(|n| n == &server_name) {
            cfg.names.push(server_name.clone());
        }
        if !cfg.endpoints.iter().any(|e| e == &endpoint) {
            cfg.endpoints.push(endpoint.clone());
        }

        if let Err(e) = save_hidden_servers_config(&cfg) {
            let _ = event_tx.send(TuiEvent::ApprovalsError(format!(
                "Failed to hide server '{}': {}",
                server_name, e
            )));
            return;
        }

        let _ = event_tx.send(TuiEvent::Trace(
            TraceEventType::Info,
            format!("Hidden known server: {}", server_name),
            Some(format!("Endpoint: {}", endpoint)),
        ));

        load_servers_async(event_tx);
    });
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
    DiscoverySearchComplete(Vec<RegistrySearchResult>),
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
    /// Drill down completed (results, breadcrumb)
    DiscoveryDrillDownComplete(Vec<RegistrySearchResult>, String),
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

    /// Chat audit loading
    ChatAuditLoading,
    /// Chat audit loaded
    ChatAuditLoaded(Vec<ChatAuditEntry>),
    /// Chat audit error
    ChatAuditError(String),

    // =========================================
    // Approvals Events
    // =========================================
    /// Approvals loading started
    ApprovalsLoading,
    /// Pending servers loaded
    PendingServersLoaded(Vec<PendingServerEntry>),
    /// Approved servers loaded
    ApprovedServersLoaded(Vec<ApprovedServerEntry>),
    /// Budget approvals loaded
    BudgetApprovalsLoaded(Vec<BudgetApprovalEntry>),
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

    // Terminal setup
    enable_raw_mode()?;
    let mut stdout = stdout();
    execute!(stdout, EnterAlternateScreen)?;
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

    // Preload servers at startup without introspecting capabilities
    // This makes the server list available immediately in the Servers view
    load_servers_async(event_tx.clone());

    // Preload capabilities for the Discover view (the default view)
    // This also loads server placeholders without introspecting them
    load_local_capabilities_async(event_tx.clone());

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
        TuiEvent::DiscoverySearchComplete(results) => {
            state.discover_loading = false;
            // Removed empty check, RegistrySearch handles it via empty list
            // But we can show error popup if we want. Let's show results popup even if empty so prompt shows "0 found"
            
            state.discover_popup = DiscoverPopup::SearchResults {
                servers: results,
                selected: 0,
                stack: Vec::new(),
                breadcrumbs: Vec::new(),
                current_category: None,
            };
            state.add_trace(
                TraceEventType::ToolDiscovery,
                "Discovery search complete - popup opened".to_string(),
                None,
            );
        }
        TuiEvent::DiscoveryDrillDownComplete(results, breadcrumb) => {
            state.discover_loading = false;
            
            // Check if results are empty and show appropriate feedback
            if results.is_empty() {
                // Try to get previous results from Introspecting popup or use a fallback
                if let DiscoverPopup::Introspecting { return_to_results: Some((prev_results, prev_breadcrumbs)), .. } = &state.discover_popup {
                    // Restore to previous results with an error message
                    state.discover_popup = DiscoverPopup::SearchResults {
                        servers: prev_results.clone(),
                        selected: 0,
                        stack: Vec::new(),
                        breadcrumbs: prev_breadcrumbs.clone(),
                        current_category: None,
                    };
                    state.add_trace(
                        TraceEventType::ToolDiscovery,
                        format!("No API endpoints found in documentation: {}", breadcrumb),
                        None,
                    );
                } else {
                    state.discover_popup = DiscoverPopup::Error {
                        title: "No Endpoints Found".to_string(),
                        message: format!("Could not extract API endpoints from: {}", breadcrumb),
                    };
                }
                return;
            }
            
            // Get navigation context from Introspecting popup if present
            let (prev_results, prev_breadcrumbs) = if let DiscoverPopup::Introspecting { return_to_results: Some((results, breadcrumbs)), .. } = &state.discover_popup {
                (Some(results.clone()), Some(breadcrumbs.clone()))
            } else if let DiscoverPopup::SearchResults { servers, breadcrumbs, .. } = &state.discover_popup {
                (Some(servers.clone()), Some(breadcrumbs.clone()))
            } else if let DiscoverPopup::ServerSuggestions { results: old_results, breadcrumbs, .. } = &state.discover_popup {
                (Some(old_results.clone()), Some(breadcrumbs.clone()))
            } else {
                (None, None)
            };

            // Convert results to DiscoveredCapability for the tool selection UI
            // This allows immediate selection and saving instead of another SearchResults layer
            let tools: Vec<DiscoveredCapability> = results.iter().map(|result| {
                let cap_category = match result.category {
                    DiscoveryCategory::OpenApiTool => CapabilityCategory::OpenApiTool,
                    DiscoveryCategory::BrowserApiTool => CapabilityCategory::BrowserApiTool,
                    _ => CapabilityCategory::McpTool,
                };
                
                DiscoveredCapability {
                    id: result.server_info.name.clone(),
                    name: result.server_info.name.clone(),
                    description: result.server_info.description.clone().unwrap_or_default(),
                    source: breadcrumb.clone(),
                    category: cap_category,
                    version: None,
                    input_schema: None,
                    output_schema: None,
                    permissions: Vec::new(),
                    effects: Vec::new(),
                    metadata: std::collections::HashMap::new(),
                }
            }).collect();
            
            // Build return_to_results context for back navigation
            let return_to_results = if let (Some(prev_res), Some(prev_bc)) = (prev_results, prev_breadcrumbs) {
                Some((prev_res, prev_bc))
            } else {
                None
            };

            // Open IntrospectionResults directly for immediate tool selection
            state.discover_popup = DiscoverPopup::IntrospectionResults {
                server_name: breadcrumb.clone(),
                endpoint: results.first().map(|r| r.server_info.endpoint.clone()).unwrap_or_default(),
                tools,
                selected: 0,
                selected_tools: std::collections::HashSet::new(),
                added_success: false,
                pended_success: false,
                editing_name: false,
                return_to_results,
            };
        }
        TuiEvent::IntrospectionComplete {
            server_name,
            endpoint,
            tools,
        } => {
            state.discover_loading = false;
            // Update popup to show results
            let return_to_results = if let DiscoverPopup::Introspecting { return_to_results, .. } = &state.discover_popup {
                return_to_results.clone()
            } else {
                None
            };

            state.discover_popup = DiscoverPopup::IntrospectionResults {
                server_name,
                endpoint,
                tools,
                selected: 0,
                selected_tools: std::collections::HashSet::new(),
                added_success: false,
                pended_success: false,
                editing_name: false,
                return_to_results,
            };
        }
        TuiEvent::IntrospectionFailed { server_name, error } => {
            state.discover_loading = false;
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
            state.discover_loading = false;
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
            // Also refresh the main servers list to stay in sync
            load_servers_async(event_tx.clone());
        }
        TuiEvent::ApprovedServersLoaded(servers) => {
            state.approved_servers = servers;
            state.approvals_loading = false;
            state.approved_selected = 0;
            // Also refresh the main servers list to stay in sync
            load_servers_async(event_tx.clone());
        }
        TuiEvent::BudgetApprovalsLoaded(approvals) => {
            state.budget_approvals = approvals;
            state.approvals_loading = false;
            state.budget_selected = 0;
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
            // Refresh approvals queue AND servers list
            load_approvals_async(event_tx.clone());
            load_servers_async(event_tx.clone());
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
            // Refresh approvals queue AND servers list
            load_approvals_async(event_tx.clone());
            load_servers_async(event_tx.clone());
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
            // Refresh approvals queue AND servers list
            load_approvals_async(event_tx.clone());
            load_servers_async(event_tx.clone());
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
        TuiEvent::ChatAuditLoading => {
            state.chat_audit_loading = true;
        }
        TuiEvent::ChatAuditLoaded(entries) => {
            state.chat_audit_loading = false;
            state.chat_audit_entries = entries.into_iter().collect();
            state.chat_audit_selected = 0;
            state.chat_audit_last_refresh = Some(Instant::now());
        }
        TuiEvent::ChatAuditError(error) => {
            state.chat_audit_loading = false;
            state.add_trace(
                TraceEventType::Error,
                format!("Chat audit error: {}", error),
                None,
            );
        }
    }
}

/// Handle keyboard events
async fn handle_key_event(
    state: &mut AppState,
    key: event::KeyEvent,
    event_tx: mpsc::UnboundedSender<TuiEvent>,
) {
    // Terminals differ on whether they emit KeyEventKind::Press vs ::Release.
    // - When we get Press/Repeat: handle it and remember it.
    // - When we get Release: ignore it only if it matches a very recent Press;
    //   otherwise treat it as an actionable event (some terminals send Release-only).
    let now = Instant::now();
    let sig = format!("{:?}:{:?}", key.code, key.modifiers);
    match key.kind {
        KeyEventKind::Press | KeyEventKind::Repeat => {
            state.last_key_press_sig = Some(sig);
            state.last_key_press_at = Some(now);
        }
        KeyEventKind::Release => {
            let should_ignore_release = state
                .last_key_press_sig
                .as_deref()
                .is_some_and(|s| s == sig.as_str())
                && state
                    .last_key_press_at
                    .is_some_and(|t| now.duration_since(t) < Duration::from_millis(250));

            if should_ignore_release {
                return;
            }
        }
    }

    fn default_panel_for_view(view: View) -> ActivePanel {
        match view {
            View::Goals => ActivePanel::GoalInput,
            View::Discover => ActivePanel::DiscoverList,
            View::Servers => ActivePanel::ServersList,
            View::Approvals => ActivePanel::ApprovalsPendingList,
            View::ChatAudit => ActivePanel::ChatAuditList,
            View::Plans | View::Execute | View::Config => ActivePanel::GoalInput,
        }
    }

    fn is_panel_in_view(panel: ActivePanel, view: View) -> bool {
        match view {
            View::Goals => matches!(
                panel,
                ActivePanel::GoalInput
                    | ActivePanel::RtfsPlan
                    | ActivePanel::DecompositionTree
                    | ActivePanel::CapabilityResolution
                    | ActivePanel::TraceTimeline
                    | ActivePanel::LlmInspector
            ),
            View::Discover => matches!(
                panel,
                ActivePanel::DiscoverInput | ActivePanel::DiscoverList | ActivePanel::DiscoverDetails
            ),
            View::Servers => matches!(panel, ActivePanel::ServersList | ActivePanel::ServerDetails),
            View::Approvals => matches!(
                panel,
                ActivePanel::ApprovalsPendingList
                    | ActivePanel::ApprovalsApprovedList
                    | ActivePanel::ApprovalsDetails
            ),
            View::ChatAudit => matches!(panel, ActivePanel::ChatAuditList),
            View::Plans | View::Execute | View::Config => true,
        }
    }

    fn next_panel_in_view(panel: ActivePanel, view: View) -> ActivePanel {
        let panel = if is_panel_in_view(panel, view) {
            panel
        } else {
            default_panel_for_view(view)
        };

        match view {
            View::Goals => match panel {
                ActivePanel::GoalInput => ActivePanel::RtfsPlan,
                ActivePanel::RtfsPlan => ActivePanel::DecompositionTree,
                ActivePanel::DecompositionTree => ActivePanel::CapabilityResolution,
                ActivePanel::CapabilityResolution => ActivePanel::TraceTimeline,
                ActivePanel::TraceTimeline => ActivePanel::LlmInspector,
                ActivePanel::LlmInspector => ActivePanel::GoalInput,
                _ => ActivePanel::GoalInput,
            },
            View::Discover => match panel {
                ActivePanel::DiscoverInput => ActivePanel::DiscoverList,
                ActivePanel::DiscoverList => ActivePanel::DiscoverDetails,
                ActivePanel::DiscoverDetails => ActivePanel::DiscoverInput,
                _ => ActivePanel::DiscoverList,
            },
            View::Servers => match panel {
                ActivePanel::ServersList => ActivePanel::ServerDetails,
                ActivePanel::ServerDetails => ActivePanel::ServersList,
                _ => ActivePanel::ServersList,
            },
            View::Approvals => match panel {
                ActivePanel::ApprovalsPendingList => ActivePanel::ApprovalsApprovedList,
                ActivePanel::ApprovalsApprovedList => ActivePanel::ApprovalsDetails,
                ActivePanel::ApprovalsDetails => ActivePanel::ApprovalsPendingList,
                _ => ActivePanel::ApprovalsPendingList,
            },
            View::ChatAudit => ActivePanel::ChatAuditList,
            View::Plans | View::Execute | View::Config => panel,
        }
    }

    fn prev_panel_in_view(panel: ActivePanel, view: View) -> ActivePanel {
        let panel = if is_panel_in_view(panel, view) {
            panel
        } else {
            default_panel_for_view(view)
        };

        match view {
            View::Goals => match panel {
                ActivePanel::GoalInput => ActivePanel::LlmInspector,
                ActivePanel::RtfsPlan => ActivePanel::GoalInput,
                ActivePanel::DecompositionTree => ActivePanel::RtfsPlan,
                ActivePanel::CapabilityResolution => ActivePanel::DecompositionTree,
                ActivePanel::TraceTimeline => ActivePanel::CapabilityResolution,
                ActivePanel::LlmInspector => ActivePanel::TraceTimeline,
                _ => ActivePanel::GoalInput,
            },
            View::Discover => match panel {
                ActivePanel::DiscoverInput => ActivePanel::DiscoverDetails,
                ActivePanel::DiscoverList => ActivePanel::DiscoverInput,
                ActivePanel::DiscoverDetails => ActivePanel::DiscoverList,
                _ => ActivePanel::DiscoverList,
            },
            View::Servers => match panel {
                ActivePanel::ServersList => ActivePanel::ServerDetails,
                ActivePanel::ServerDetails => ActivePanel::ServersList,
                _ => ActivePanel::ServersList,
            },
            View::Approvals => match panel {
                ActivePanel::ApprovalsPendingList => ActivePanel::ApprovalsDetails,
                ActivePanel::ApprovalsApprovedList => ActivePanel::ApprovalsPendingList,
                ActivePanel::ApprovalsDetails => ActivePanel::ApprovalsApprovedList,
                _ => ActivePanel::ApprovalsPendingList,
            },
            View::ChatAudit => ActivePanel::ChatAuditList,
            View::Plans | View::Execute | View::Config => panel,
        }
    }

    // Popup-specific handling should intercept keys BEFORE global shortcuts.
    // Otherwise, view-switching and other global keys can steal input from menus.
    if !matches!(state.discover_popup, DiscoverPopup::None) {
        handle_discover_popup_key(state, key, event_tx);
        return;
    }

    if state.auth_token_popup.is_some() {
        handle_auth_token_popup(state, key, event_tx);
        return;
    }

    let allow_view_switch = !matches!(
        (state.current_view, state.active_panel),
        (View::Goals, ActivePanel::GoalInput) | (View::Discover, ActivePanel::DiscoverInput)
    );

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
            } else {
                // Reset focus to the default panel for the current view.
                state.active_panel = default_panel_for_view(state.current_view);
            }
            return;
        }
        // View switching shortcuts (1-7) - disabled only when typing in Goals input
        (KeyCode::Char('1'), _) if allow_view_switch => {
            state.current_view = View::Discover;
            state.active_panel = ActivePanel::DiscoverList;
            // Trigger capability loading if not already loaded
            if state.discovered_capabilities.is_empty() && !state.discover_loading {
                load_local_capabilities_async(event_tx.clone());
            }
            return;
        }
        (KeyCode::Char('2'), _) if allow_view_switch => {
            state.current_view = View::Servers;
            state.active_panel = ActivePanel::ServersList;
            // Trigger server loading if not already loaded
            if state.servers.is_empty() && !state.servers_loading {
                load_servers_async(event_tx.clone());
            }
            return;
        }
        (KeyCode::Char('3'), _) if allow_view_switch => {
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
        (KeyCode::Char('4'), _) if allow_view_switch => {
            state.current_view = View::Goals;
            state.active_panel = ActivePanel::RtfsPlan;
            return;
        }
        (KeyCode::Char('5'), _) if allow_view_switch => {
            state.current_view = View::Plans;
            state.active_panel = ActivePanel::GoalInput; // Plans view not yet implemented
            return;
        }
        (KeyCode::Char('6'), _) if allow_view_switch => {
            state.current_view = View::Execute;
            state.active_panel = ActivePanel::GoalInput; // Execute view not yet implemented
            return;
        }
        (KeyCode::Char('7'), _) if allow_view_switch => {
            state.current_view = View::Config;
            state.active_panel = ActivePanel::GoalInput; // Config view not yet implemented
            return;
        }
        (KeyCode::Char('8'), _) if allow_view_switch => {
            state.current_view = View::ChatAudit;
            state.active_panel = ActivePanel::ChatAuditList;
            if state.chat_audit_entries.is_empty() && !state.chat_audit_loading {
                load_chat_audit_async(event_tx.clone(), state.chat_audit_endpoint.clone());
            }
            return;
        }
        (KeyCode::Tab, KeyModifiers::NONE) => {
            state.active_panel = next_panel_in_view(state.active_panel, state.current_view);
            return;
        }
        (KeyCode::BackTab, _) => {
            state.active_panel = prev_panel_in_view(state.active_panel, state.current_view);
            return;
        }
        _ => {}
    }

    // Route keys based on current view and active panel
    match state.current_view {
        View::Servers => {
            handle_servers_view(state, key, event_tx);
        }
        View::Discover => {
            match state.active_panel {
                ActivePanel::DiscoverInput => handle_discover_input(state, key, event_tx),
                ActivePanel::DiscoverDetails => handle_discover_details(state, key),
                _ => handle_discover_list(state, key, event_tx),
            }
        }
        View::Approvals => {
            handle_approvals_view(state, key, event_tx).await;
        }
        View::Goals => {
            match state.active_panel {
                ActivePanel::GoalInput => handle_goal_input(state, key, event_tx),
                ActivePanel::RtfsPlan => handle_rtfs_plan(state, key),
                ActivePanel::DecompositionTree => handle_decomp_tree(state, key),
                ActivePanel::CapabilityResolution => handle_resolution(state, key),
                ActivePanel::TraceTimeline => handle_timeline(state, key),
                ActivePanel::LlmInspector => handle_llm_inspector(state, key),
                _ => handle_goal_input(state, key, event_tx),
            }
        }
        View::ChatAudit => {
            handle_chat_audit_view(state, key, event_tx);
        }
        _ => {}
    }
}

fn handle_chat_audit_view(
    state: &mut AppState,
    key: event::KeyEvent,
    event_tx: mpsc::UnboundedSender<TuiEvent>,
) {
    match key.code {
        KeyCode::Up | KeyCode::Char('k') => {
            state.chat_audit_selected = state.chat_audit_selected.saturating_sub(1);
        }
        KeyCode::Down | KeyCode::Char('j') => {
            if !state.chat_audit_entries.is_empty() {
                state.chat_audit_selected = (state.chat_audit_selected + 1)
                    .min(state.chat_audit_entries.len().saturating_sub(1));
            }
        }
        KeyCode::Char('r') => {
            load_chat_audit_async(event_tx, state.chat_audit_endpoint.clone());
        }
        _ => {}
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
        // Find new servers (opens discovery search popup)
        KeyCode::Char('f') => {
            state.discover_popup = DiscoverPopup::ServerSearchInput {
                query: String::new(),
                cursor_position: 0,
            };
        }
        // Discover tools for selected server
        KeyCode::Char('d') => {
            if !state.servers.is_empty() && state.servers_selected < state.servers.len() {
                let server = &state.servers[state.servers_selected];
                discover_server_tools_async(
                    event_tx,
                    state.servers_selected,
                    server.name.clone(),
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
                let server_name = state.servers[state.servers_selected].name.clone();
                check_server_connection_async(event_tx, state.servers_selected, server_name, endpoint);
            }
        }
        // Enter to select/activate server (same as discover)
        KeyCode::Enter => {
            if !state.servers.is_empty() && state.servers_selected < state.servers.len() {
                let server = &state.servers[state.servers_selected];
                discover_server_tools_async(
                    event_tx,
                    state.servers_selected,
                    server.name.clone(),
                    server.endpoint.clone(),
                );
            }
        }
        // Delete server (approved only)
        KeyCode::Char('x') => {
            if !state.servers.is_empty() && state.servers_selected < state.servers.len() {
                let server = state.servers[state.servers_selected].clone();
                state.discover_popup = DiscoverPopup::DeleteConfirmation { server };
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
        KeyCode::Char('[') => {
            state.approvals_tab = ApprovalsTab::Pending;
            state.active_panel = ActivePanel::ApprovalsPendingList;
        }
        KeyCode::Char(']') => {
            state.approvals_tab = ApprovalsTab::Approved;
            state.active_panel = ActivePanel::ApprovalsApprovedList;
        }
        KeyCode::Char('b') => {
            state.approvals_tab = ApprovalsTab::Budget;
            state.active_panel = ActivePanel::ApprovalsBudgetList;
        }
        // Navigation
        KeyCode::Up | KeyCode::Char('k') => match state.approvals_tab {
            ApprovalsTab::Pending => {
                if state.pending_selected > 0 {
                    state.pending_selected -= 1;
                }
            }
            ApprovalsTab::Approved => {
                if state.approved_selected > 0 {
                    state.approved_selected -= 1;
                }
            }
            ApprovalsTab::Budget => {
                if state.budget_selected > 0 {
                    state.budget_selected -= 1;
                }
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
            ApprovalsTab::Budget => {
                if !state.budget_approvals.is_empty() {
                    state.budget_selected =
                        (state.budget_selected + 1).min(state.budget_approvals.len() - 1);
                }
            }
        },
        KeyCode::PageUp => match state.approvals_tab {
            ApprovalsTab::Pending => {
                state.pending_selected = state.pending_selected.saturating_sub(10);
            }
            ApprovalsTab::Approved => {
                state.approved_selected = state.approved_selected.saturating_sub(10);
            }
            ApprovalsTab::Budget => {
                state.budget_selected = state.budget_selected.saturating_sub(10);
            }
        },
        KeyCode::PageDown => match state.approvals_tab {
            ApprovalsTab::Pending => {
                if !state.pending_servers.is_empty() {
                    state.pending_selected = (state.pending_selected + 10).min(state.pending_servers.len() - 1);
                }
            }
            ApprovalsTab::Approved => {
                if !state.approved_servers.is_empty() {
                    state.approved_selected = (state.approved_selected + 10).min(state.approved_servers.len() - 1);
                }
            }
            ApprovalsTab::Budget => {
                if !state.budget_approvals.is_empty() {
                    state.budget_selected = (state.budget_selected + 10)
                        .min(state.budget_approvals.len() - 1);
                }
            }
        },
        KeyCode::Home => match state.approvals_tab {
            ApprovalsTab::Pending => state.pending_selected = 0,
            ApprovalsTab::Approved => state.approved_selected = 0,
            ApprovalsTab::Budget => state.budget_selected = 0,
        },
        KeyCode::End => match state.approvals_tab {
            ApprovalsTab::Pending => {
                if !state.pending_servers.is_empty() {
                    state.pending_selected = state.pending_servers.len() - 1;
                }
            }
            ApprovalsTab::Approved => {
                if !state.approved_servers.is_empty() {
                    state.approved_selected = state.approved_servers.len() - 1;
                }
            }
            ApprovalsTab::Budget => {
                if !state.budget_approvals.is_empty() {
                    state.budget_selected = state.budget_approvals.len() - 1;
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
        KeyCode::Char('a') if state.approvals_tab == ApprovalsTab::Budget => {
            if let Some(approval) = state.budget_approvals.get(state.budget_selected) {
                let approval_id = approval.id.clone();
                let dimension = approval.dimension.clone();
                approve_budget_extension_async(event_tx, approval_id, dimension);
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
        // Reject budget extension
        KeyCode::Char('r') if state.approvals_tab == ApprovalsTab::Budget => {
            if let Some(approval) = state.budget_approvals.get(state.budget_selected) {
                let approval_id = approval.id.clone();
                let dimension = approval.dimension.clone();
                reject_budget_extension_async(event_tx, approval_id, dimension);
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
                dismiss_server_async(event_tx, server_id, server_name, None);
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
            ApprovalsTab::Budget => {}
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
                            return_to_results: None,
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

                // RECONCILIATION: Check filesystem for orphan pending servers (on disk but not in queue)
                let caps_base = get_capabilities_base_path();
                let pending_root = caps_base.join("servers/pending");
                let mut found_orphans = false;

                if let Ok(entries) = std::fs::read_dir(&pending_root) {
                    for entry in entries.flatten() {
                        let path = entry.path();
                        if !path.is_dir() {
                            continue;
                        }

                        // Use our existing inference logic to find server info
                        if let Some((name, endpoint, _, auth_env_var)) = infer_server_from_dir("", &path) {
                            // Check if this endpoint/name is already in our list
                            let already_in_queue = pending_entries.iter().any(|e| {
                                e.name == name || (!e.endpoint.is_empty() && e.endpoint == endpoint)
                            });

                            if !already_in_queue {
                                // This is an orphan! Push it to the queue automatically.
                                use ccos::approval::{
                                    DiscoverySource, RiskAssessment, RiskLevel, ServerInfo,
                                };

                                let server_info = ServerInfo {
                                    name: name.clone(),
                                    endpoint: endpoint.clone(),
                                    description: Some(
                                        "Auto-recovered orphan pending server".to_string(),
                                    ),
                                    auth_env_var,
                                    capabilities_path: None,
                                    alternative_endpoints: vec![],
                                    capability_files: None,
                                };

                                let source = DiscoverySource::Manual {
                                    user: "system".to_string(),
                                };

                                let risk = RiskAssessment {
                                    level: RiskLevel::Medium,
                                    reasons: vec!["Auto-recovered orphan".to_string()],
                                };

                                if let Ok(_) = queue
                                    .add_server_discovery(source, server_info, vec![], risk, None, 24)
                                    .await
                                {
                                    found_orphans = true;
                                }
                            }
                        }
                    }
                }

                // If we found orphans, we need to reload the pending entries from the queue
                let final_pending_entries = if found_orphans {
                    match queue.list_pending_servers().await {
                        Ok(new_pending) => new_pending
                            .iter()
                            .filter_map(|r| r.to_pending_discovery())
                            .map(|p| {
                                // Check if auth token is available
                                let auth_status =
                                    if let Some(ref env_var) = p.server_info.auth_env_var {
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
                                    tool_count: None,
                                    risk_level: p
                                        .risk_assessment
                                        .as_ref()
                                        .map(|ra| format!("{:?}", ra.level).to_lowercase())
                                        .unwrap_or_else(|| "unknown".to_string()),
                                    requested_at: p.requested_at.format("%Y-%m-%d %H:%M").to_string(),
                                    requesting_goal: p.requesting_goal,
                                }
                            })
                            .collect(),
                        Err(_) => pending_entries,
                    }
                } else {
                    pending_entries
                };

                let _ = event_tx.send(TuiEvent::PendingServersLoaded(final_pending_entries));
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

        // Load pending budget extension approvals
        match queue.list_pending_budget_extensions().await {
            Ok(pending_requests) => {
                let budget_entries: Vec<BudgetApprovalEntry> = pending_requests
                    .iter()
                    .filter_map(|req| {
                        if let ccos::approval::ApprovalCategory::BudgetExtension {
                            plan_id,
                            intent_id,
                            dimension,
                            requested_additional,
                            consumed,
                            limit,
                        } = &req.category
                        {
                            Some(BudgetApprovalEntry {
                                id: req.id.clone(),
                                plan_id: plan_id.clone(),
                                intent_id: intent_id.clone(),
                                dimension: dimension.clone(),
                                requested_additional: *requested_additional,
                                consumed: *consumed,
                                limit: *limit,
                                risk_level: format!("{:?}", req.risk_assessment.level)
                                    .to_lowercase(),
                                requested_at: req.requested_at.format("%Y-%m-%d %H:%M").to_string(),
                            })
                        } else {
                            None
                        }
                    })
                    .collect();
                let _ = event_tx.send(TuiEvent::BudgetApprovalsLoaded(budget_entries));
            }
            Err(e) => {
                let _ = event_tx.send(TuiEvent::ApprovalsError(format!(
                    "Failed to load budget approvals: {}",
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

/// Approve a pending budget extension
fn approve_budget_extension_async(
    event_tx: mpsc::UnboundedSender<TuiEvent>,
    approval_id: String,
    dimension: String,
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
                &approval_id,
                ApprovalAuthority::User("tui".to_string()),
                Some("Budget extension approved via TUI".to_string()),
            )
            .await
        {
            Ok(()) => {
                let _ = event_tx.send(TuiEvent::Trace(
                    TraceEventType::Info,
                    format!("Budget extension approved: {}", dimension),
                    None,
                ));
                load_approvals_async(event_tx);
            }
            Err(e) => {
                let _ = event_tx.send(TuiEvent::ApprovalsError(format!(
                    "Failed to approve budget extension: {}",
                    e
                )));
            }
        }
    });
}

/// Reject a pending budget extension
fn reject_budget_extension_async(
    event_tx: mpsc::UnboundedSender<TuiEvent>,
    approval_id: String,
    dimension: String,
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
                &approval_id,
                ApprovalAuthority::User("tui".to_string()),
                "Budget extension rejected via TUI".to_string(),
            )
            .await
        {
            Ok(()) => {
                let _ = event_tx.send(TuiEvent::Trace(
                    TraceEventType::Info,
                    format!("Budget extension rejected: {}", dimension),
                    None,
                ));
                load_approvals_async(event_tx);
            }
            Err(e) => {
                let _ = event_tx.send(TuiEvent::ApprovalsError(format!(
                    "Failed to reject budget extension: {}",
                    e
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
    directory_path: Option<String>,
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
                // Best-effort: archive the server directory so it can be restored later.
                // Dismissing removes it from the queue, so it disappears from the UI.
                let caps_base = get_capabilities_base_path();
                let sanitized = ccos::utils::fs::sanitize_filename(&server_name);

                let mut archive_candidates: Vec<std::path::PathBuf> = Vec::new();
                if let Some(path) = directory_path {
                    archive_candidates.push(std::path::PathBuf::from(path));
                } else {
                    let approved_root = caps_base.join("servers/approved");
                    archive_candidates.push(approved_root.join(&server_name));
                    archive_candidates.push(approved_root.join(&sanitized));
                }

                for candidate in archive_candidates {
                    if candidate.exists() {
                        match archive_server_directory(&event_tx, &candidate, &server_name, "Dismissed via TUI") {
                            Ok(Some(dest)) => {
                                let _ = event_tx.send(TuiEvent::Trace(
                                    TraceEventType::Info,
                                    format!("Archived server directory: {}", server_name),
                                    Some(format!("Archived to: {}", dest.display())),
                                ));
                            }
                            Ok(None) => {}
                            Err(e) => {
                                let _ = event_tx.send(TuiEvent::Trace(
                                    TraceEventType::Info,
                                    format!("Failed to archive directory for {}: {}", server_name, e),
                                    Some(format!("Path: {}", candidate.display())),
                                ));
                            }
                        }
                        break;
                    }
                }

                let _ = event_tx.send(TuiEvent::ServerRejected {
                    _server_id: server_id.clone(),
                    server_name: server_name.clone(),
                });
                // Reload the queues
                load_approvals_async(event_tx.clone());
                load_servers_async(event_tx);
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

fn deleted_servers_root() -> std::path::PathBuf {
    get_capabilities_base_path().join("servers/deleted")
}

fn copy_dir_recursive(from: &std::path::Path, to: &std::path::Path) -> std::io::Result<()> {
    std::fs::create_dir_all(to)?;
    for entry in std::fs::read_dir(from)? {
        let entry = entry?;
        let src = entry.path();
        let dst = to.join(entry.file_name());
        let file_type = entry.file_type()?;

        if file_type.is_dir() {
            copy_dir_recursive(&src, &dst)?;
        } else if file_type.is_file() {
            if let Some(parent) = dst.parent() {
                std::fs::create_dir_all(parent)?;
            }
            let _bytes = std::fs::copy(&src, &dst)?;
        }
    }
    Ok(())
}

fn archive_server_directory(
    event_tx: &mpsc::UnboundedSender<TuiEvent>,
    source_dir: &std::path::Path,
    server_name: &str,
    reason: &str,
) -> std::io::Result<Option<std::path::PathBuf>> {
    if !source_dir.exists() {
        return Ok(None);
    }

    let deleted_root = deleted_servers_root();
    std::fs::create_dir_all(&deleted_root)?;

    let ts = match std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH) {
        Ok(d) => d.as_secs(),
        Err(_) => 0,
    };
    let sanitized = ccos::utils::fs::sanitize_filename(server_name);

    let mut dest_dir = None;
    for i in 0..1000u32 {
        let suffix = if i == 0 { String::new() } else { format!("_{}", i) };
        let candidate = deleted_root.join(format!("{}_{}{}", ts, sanitized, suffix));
        if !candidate.exists() {
            dest_dir = Some(candidate);
            break;
        }
    }
    let Some(dest_dir) = dest_dir else {
        return Err(std::io::Error::new(
            std::io::ErrorKind::AlreadyExists,
            "Could not allocate unique deleted/ directory name",
        ));
    };

    // Prefer a move; fall back to copy+delete if needed (e.g. cross-device).
    match std::fs::rename(source_dir, &dest_dir) {
        Ok(()) => {}
        Err(_) => {
            std::fs::create_dir_all(&dest_dir)?;
            copy_dir_recursive(source_dir, &dest_dir)?;
            std::fs::remove_dir_all(source_dir)?;
        }
    }

    // Best-effort metadata about the deletion.
    let metadata_path = dest_dir.join("deleted.json");
    let metadata = serde_json::json!({
        "server_name": server_name,
        "reason": reason,
        "archived_at_unix": ts,
        "original_path": source_dir.to_string_lossy().to_string(),
    });
    let json = serde_json::to_string_pretty(&metadata)
        .unwrap_or_else(|_| "{\n  \"server_name\": \"\",\n  \"reason\": \"\",\n  \"archived_at_unix\": 0,\n  \"original_path\": \"\"\n}".to_string());
    if let Err(e) = std::fs::write(&metadata_path, json) {
        let _ = event_tx.send(TuiEvent::Trace(
            TraceEventType::Info,
            format!("Archived server but failed to write metadata: {}", e),
            Some(format!("Path: {}", metadata_path.display())),
        ));
    }

    Ok(Some(dest_dir))
}

/// Remove a pending server and archive its pending directory (soft delete)
fn remove_pending_server_async(
    event_tx: mpsc::UnboundedSender<TuiEvent>,
    server_id: String,
    server_name: String,
    directory_path: Option<String>,
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

        match queue.remove_pending(&server_id).await {
            Ok(true) => {}
            Ok(false) => {
                let _ = event_tx.send(TuiEvent::Trace(
                    TraceEventType::Info,
                    format!("Pending request not found for '{}' (id={})", server_name, server_id),
                    None,
                ));
            }
            Err(e) => {
                let _ = event_tx.send(TuiEvent::Trace(
                    TraceEventType::Info,
                    format!("Failed to remove pending request for '{}' (id={}): {}", server_name, server_id, e),
                    None,
                ));
            }
        }

        let caps_base = get_capabilities_base_path();
        let server_id = ccos::utils::fs::sanitize_filename(&server_name);

        let pending_dir = if let Some(path) = directory_path {
            std::path::PathBuf::from(path)
        } else {
            let base = caps_base.join("servers/pending");
            let named = base.join(&server_name);
            if named.exists() {
                named
            } else {
                base.join(&server_id)
            }
        };

        // Helper: remove empty parents up to (but not including) stop_at
        fn cleanup_empty_parents(mut dir: std::path::PathBuf, stop_at: &std::path::Path) {
            while dir.starts_with(stop_at) && dir != stop_at {
                if let Ok(mut entries) = std::fs::read_dir(&dir) {
                    if entries.next().is_none() {
                        let _ = std::fs::remove_dir(&dir);
                    } else {
                        break;
                    }
                } else {
                    break;
                }

                if let Some(parent) = dir.parent() {
                    dir = parent.to_path_buf();
                } else {
                    break;
                }
            }
        }

        let pending_root = caps_base.join("servers/pending");
        let mut delete_candidates: Vec<std::path::PathBuf> = vec![pending_dir.clone()];
        delete_candidates.push(pending_root.join(&server_name));
        delete_candidates.push(pending_root.join(&server_id));

        for dir in delete_candidates {
            if dir.exists() {
                match archive_server_directory(&event_tx, &dir, &server_name, "Deleted pending server via TUI") {
                    Ok(Some(dest)) => {
                        let _ = event_tx.send(TuiEvent::Trace(
                            TraceEventType::Info,
                            format!("Archived pending server: {}", server_name),
                            Some(format!("Archived to: {}", dest.display())),
                        ));
                    }
                    Ok(None) => {}
                    Err(e) => {
                        let _ = event_tx.send(TuiEvent::ApprovalsError(format!(
                            "Failed to archive pending directory {}: {}",
                            dir.display(),
                            e
                        )));
                        return;
                    }
                }

                if let Some(parent) = dir.parent() {
                    cleanup_empty_parents(parent.to_path_buf(), &pending_root);
                }
            }
        }

        let _ = event_tx.send(TuiEvent::Trace(
            TraceEventType::Info,
            format!("Pending server deleted (archived): {}", server_name),
            None,
        ));
        load_approvals_async(event_tx.clone());
        load_servers_async(event_tx.clone());
    });
}

/// Remove a server directory without relying on queue entries
fn remove_server_directory_async(
    event_tx: mpsc::UnboundedSender<TuiEvent>,
    server_name: String,
    directory_path: Option<String>,
) {
    tokio::task::spawn_local(async move {
        let workspace_root = ccos::utils::fs::get_workspace_root();
        let caps_base = get_capabilities_base_path();
        let queue_base = get_approval_queue_base();
        let mut roots = vec![workspace_root.clone(), queue_base.clone()];
        if let Some(parent) = workspace_root.parent() {
            roots.push(parent.to_path_buf());
        }
        let server_id = ccos::utils::fs::sanitize_filename(&server_name);
        let mut candidates: Vec<std::path::PathBuf> = Vec::new();

        let mut push_unique = |path: std::path::PathBuf| {
            if !candidates.iter().any(|p| p == &path) {
                candidates.push(path);
            }
        };

        if let Some(path) = directory_path {
            push_unique(std::path::PathBuf::from(path));
        }

        let mut base_dirs: Vec<std::path::PathBuf> = vec![caps_base.join("servers")];
        base_dirs.push(queue_base.join("capabilities/servers"));

        for root in &roots {
            base_dirs.push(root.join("capabilities/servers"));
        }

        for base_root in base_dirs {
            for bucket in ["pending", "approved", "rejected"] {
                let base = base_root.join(bucket);
                push_unique(base.join(&server_name));
                push_unique(base.join(&server_id));

                // Also look for nested structure (e.g. bucket/namespace/server_name)
                if let Ok(entries) = std::fs::read_dir(&base) {
                    for entry in entries.flatten() {
                        let path = entry.path();
                        if !path.is_dir() {
                            continue;
                        }
                        
                        // Check if this dir matches name or id
                        let name = entry.file_name().to_string_lossy().to_string();
                        if name == server_name || name == server_id {
                            push_unique(path.clone());
                        }

                        // Check one level deeper for the actual server dir
                        if let Ok(subs) = std::fs::read_dir(&path) {
                            for sub in subs.flatten() {
                                let sub_path = sub.path();
                                if sub_path.is_dir() {
                                    let sub_name = sub.file_name().to_string_lossy().to_string();
                                    if sub_name == server_name || sub_name == server_id {
                                        // If we found the match deep, we probably want to remove 
                                        // the namespace parent too if it only contains this server.
                                        push_unique(path.clone());
                                        push_unique(sub_path);
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }

        let mut removed_any = false;
        for path in candidates {
            if path.exists() {
                match archive_server_directory(&event_tx, &path, &server_name, "Deleted server directory via TUI") {
                    Ok(Some(dest)) => {
                        let _ = event_tx.send(TuiEvent::Trace(
                            TraceEventType::Info,
                            format!("Archived server directory: {}", server_name),
                            Some(format!("Archived to: {}", dest.display())),
                        ));
                    }
                    Ok(None) => {}
                    Err(e) => {
                        let _ = event_tx.send(TuiEvent::ApprovalsError(format!(
                            "Failed to archive server directory {}: {}",
                            path.display(),
                            e
                        )));
                        return;
                    }
                }
                removed_any = true;
            }
        }

        if removed_any {
            let _ = event_tx.send(TuiEvent::Trace(
                TraceEventType::Info,
                format!("Server directory deleted (archived): {}", server_name),
                None,
            ));
            load_servers_async(event_tx);
        } else {
            let _ = event_tx.send(TuiEvent::ApprovalsError(format!(
                "No server directory found for '{}'",
                server_name
            )));
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
        use ccos::synthesis::introspection::mcp_introspector::MCPIntrospector;

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
                // Recover input schema (TypeExpr) and JSON string
                let (input_schema, input_schema_json) = if let Some(json_str) = t.metadata.get("input_schema_json") {
                     if let Ok(val) = serde_json::from_str::<serde_json::Value>(json_str) {
                         // Convert JSON schema back to RTFS TypeExpr
                         let introspector = MCPIntrospector::new();
                         let type_expr = introspector.json_schema_to_rtfs_type(&val).ok();
                         (type_expr, Some(val))
                     } else {
                         (None, None)
                     }
                } else {
                     // Fallback: try to parse input_schema if it WAS valid JSON (legacy fallback)
                     // But typically t.input_schema is now RTFS compact string.
                     (None, None)
                };

                DiscoveredMCPTool {
                    tool_name: t.name.clone(),
                    description: Some(t.description.clone()),
                    input_schema,
                    output_schema: None,
                    input_schema_json,
                }
            })
            .collect();

        // 2. Synthesize RTFS capabilities files + server.rtfs
        let mut capabilities_path = None;
        let mut capability_files = None;
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
                // Determine target directory: Approved takes precedence over Pending
                let caps_dir = get_capabilities_base_path();
                let approved_base = caps_dir.join("servers/approved");
                
                // Check if directory exists using original name (allowing nested paths like github/github-mcp)
                // This is important because sanitize_filename might flatten hierarchy that exists on disk
                let approve_server_dir_original = approved_base.join(&server_name);
                
                let sanitized_name = ccos::utils::fs::sanitize_filename(&server_name);
                let approve_server_dir_sanitized = approved_base.join(&sanitized_name);
                
                let target_base = if approve_server_dir_original.exists() {
                    approved_base
                } else if approve_server_dir_sanitized.exists() {
                    approved_base
                } else {
                    caps_dir.join("servers/pending")
                };
                
                let server_dir = target_base.join(&sanitized_name);

                if let Ok(_) = std::fs::create_dir_all(&server_dir) {
                    // export_manifests_to_rtfs_layout takes the PARENT export directory 
                    // (e.g. servers/pending) because it appends the server name itself?
                    // Let's double check signature in mcp/core.rs.
                    // Step 529 (lines 1389-1393):
                    // pub fn export_manifests_to_rtfs_layout( &self, server_config: &MCPServerConfig, manifests: &[CapabilityManifest], export_dir_override: &std::path::Path )
                    // It uses the override directly.
                    // Wait, mcp/core.rs logic usually does server_dir = export_dir.join(server_name).
                    // Let's assume export_dir passed here should be the PARENT (servers/pending or servers/approve).
                    
                    if let Ok(files) = service.export_manifests_to_rtfs_layout(
                        &server_config,
                        &manifest_results,
                        &target_base,
                    ) {
                        capability_files = Some(files.clone());
                        
                        // Use the actual directory where files were exported
                        if let Some(first_file) = files.first() {
                            let path = std::path::Path::new(first_file);
                            if let Some(parent) = path.parent() {
                                let server_rtfs = if path.file_name().is_some_and(|n| n == "server.rtfs") {
                                    path.to_path_buf()
                                } else {
                                    parent.join("server.rtfs")
                                };
                                capabilities_path = Some(server_rtfs.to_string_lossy().to_string());
                            }
                        }
                        
                        // Fallback if files was empty or parent failed
                        if capabilities_path.is_none() {
                            let server_rtfs = server_dir.join("server.rtfs");
                            capabilities_path = Some(server_rtfs.to_string_lossy().to_string());
                        }
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
            capability_files,
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

fn normalize_endpoint(endpoint: &str) -> String {
    endpoint.trim().trim_end_matches('/').to_string()
}

fn extract_quoted_value_after_key(contents: &str, key: &str) -> Option<String> {
    // Both ":name" and "name" and "\"name\"" formats supported
    let pattern = format!(r##"[:"]?{}\b["]?\s+"([^"]+)"##, key);
    let re = regex::Regex::new(&pattern).ok()?;
    re.captures(contents).map(|cap| cap[1].to_string())
}

fn extract_endpoint_from_capabilities(contents: &str) -> Option<String> {
    // (mcp-call "ENDPOINT" ...)
    let needle = "(mcp-call \"";
    let idx = contents.find(needle)?;
    let rest = &contents[idx + needle.len()..];
    let end_quote = rest.find('"')?;
    Some(rest[..end_quote].to_string())
}

/// Detailed information about a capability found on disk
#[derive(Debug, Clone)]
struct LocalCapabilityInfo {
    name: String,
    description: Option<String>,
    input_schema: Option<String>,
    output_schema: Option<String>,
}

/// Reformat a schema string to use pretty-printing.
/// Parses the RTFS type expression and re-serializes it with indentation.
fn reformat_schema_pretty(schema: Option<String>) -> Option<String> {
    schema.and_then(|s| {
        // Try to parse as TypeExpr and re-serialize with pretty printing
        match rtfs::parser::parse_type_expression(&s) {
            Ok(type_expr) => Some(type_expr_to_rtfs_pretty(&type_expr)),
            Err(_) => Some(s), // If parsing fails, return original string
        }
    })
}

fn extract_rtfs_attr(content: &str, key: &str) -> Option<String> {
    if key == "capability" {
        let needle = "capability \"";
        let idx = content.find(needle)?;
        let rest = &content[idx + needle.len()..];
        let end = rest.find('"')?;
        return Some(rest[..end].to_string());
    }

    // Look for :key followed by whitespace or quote or bracket
    let needle = format!(":{}", key);
    let idx = content.find(&needle)?;
    let rest = &content[idx + needle.len()..];
    let rest = rest.trim_start();

    if rest.starts_with('"') {
        let end = rest[1..].find('"')?;
        return Some(rest[1..end + 1].to_string());
    } else if rest.starts_with(':') {
        let end = rest
            .find(|c: char| c.is_whitespace() || c == ')' || c == '}')
            .unwrap_or(rest.len());
        return Some(rest[..end].to_string());
    } else if rest.starts_with('{') {
        let mut count = 0;
        for (i, c) in rest.chars().enumerate() {
            if c == '{' {
                count += 1;
            } else if c == '}' {
                count -= 1;
                if count == 0 {
                    return Some(rest[..i + 1].to_string());
                }
            }
        }
    } else if rest.starts_with('[') {
        let mut count = 0;
        for (i, c) in rest.chars().enumerate() {
            if c == '[' {
                count += 1;
            } else if c == ']' {
                count -= 1;
                if count == 0 {
                    return Some(rest[..i + 1].to_string());
                }
            }
        }
    } else {
        // Just till whitespace or bracket
        let end = rest
            .find(|c: char| c.is_whitespace() || c == ')' || c == '}')
            .unwrap_or(rest.len());
        let val = rest[..end].trim();
        if !val.is_empty() {
            return Some(val.to_string());
        }
    }

    None
}

fn list_tools_in_dir(dir: &std::path::Path) -> Vec<String> {
    list_tools_detailed_in_dir(dir)
        .into_iter()
        .map(|c| c.name)
        .collect()
}

fn list_tools_detailed_in_dir(dir: &std::path::Path) -> Vec<LocalCapabilityInfo> {
    let mut tools = Vec::new();
    let Ok(entries) = std::fs::read_dir(dir) else {
        return tools;
    };

    for entry in entries.flatten() {
        let p = entry.path();
        if p.is_dir() {
            tools.extend(list_tools_detailed_in_dir(&p));
            continue;
        }

        let Some(fname) = p.file_name().and_then(|n| n.to_str()) else {
            continue;
        };
        if fname == "server.rtfs" || !fname.ends_with(".rtfs") {
            continue;
        }

        let Ok(content) = std::fs::read_to_string(&p) else {
            continue;
        };

        if fname == "capabilities.rtfs" {
            // Split content by (capability
            let sections = content.split("(capability ");
            for section in sections.skip(1) {
                // skip preamble
                let section = format!("(capability {}", section);
                let name = extract_rtfs_attr(&section, "capability")
                    .unwrap_or_else(|| "unknown".to_string());
                let description = extract_rtfs_attr(&section, "description");
                let input_schema = reformat_schema_pretty(extract_rtfs_attr(&section, "input-schema"));
                let output_schema = reformat_schema_pretty(extract_rtfs_attr(&section, "output-schema"));

                tools.push(LocalCapabilityInfo {
                    name,
                    description,
                    input_schema,
                    output_schema,
                });
            }
        } else {
            let mut name = extract_rtfs_attr(&content, "capability")
                .unwrap_or_else(|| fname.trim_end_matches(".rtfs").to_string());

            // Strip common MCP tool prefixes like mcp.something.
            if name.starts_with("mcp.") {
                if let Some(last_dot) = name.rfind('.') {
                    name = name[last_dot + 1..].to_string();
                }
            }

            let description = extract_rtfs_attr(&content, "description");
            let input_schema = reformat_schema_pretty(extract_rtfs_attr(&content, "input-schema"));
            let output_schema = reformat_schema_pretty(extract_rtfs_attr(&content, "output-schema"));

            tools.push(LocalCapabilityInfo {
                name,
                description,
                input_schema,
                output_schema,
            });
        }
    }

    tools.sort_by(|a, b| a.name.cmp(&b.name));
    tools.dedup_by(|a, b| a.name == b.name);
    tools
}

fn count_rtfs_files(dir: &std::path::Path) -> usize {
    list_tools_in_dir(dir).len()
}

fn infer_server_from_dir(
    bucket_prefix: &str,
    dir: &std::path::Path,
) -> Option<(String, String, Option<usize>, Option<String>)> {
    let dir_name = dir.file_name()?.to_string_lossy().to_string();

    // Prefer server.rtfs for name+endpoint, then fall back to capabilities.rtfs for endpoint.
    // If they are not in the root dir, check one level deeper (often Happens with namespaces)
    let mut server_rtfs = dir.join("server.rtfs");
    let mut caps_rtfs = dir.join("capabilities.rtfs");

    if !server_rtfs.exists() && !caps_rtfs.exists() {
        // Look one level deeper
        if let Ok(entries) = std::fs::read_dir(dir) {
            for entry in entries.flatten() {
                let path = entry.path();
                if path.is_dir() {
                    let s = path.join("server.rtfs");
                    let c = path.join("capabilities.rtfs");
                    if s.exists() || c.exists() {
                        server_rtfs = s;
                        caps_rtfs = c;
                        break;
                    }
                }
            }
        }
    }

    let mut name = dir_name.replace('_', " ");
    let mut endpoint = String::new();
    let mut auth_env_var = None;

    if let Ok(contents) = std::fs::read_to_string(&server_rtfs) {
        if let Some(n) = extract_quoted_value_after_key(&contents, "name") {
            name = n;
        }
        if let Some(e) = extract_quoted_value_after_key(&contents, "endpoint") {
            endpoint = e;
        }
        if let Some(a) = extract_quoted_value_after_key(&contents, "auth_env_var") {
            auth_env_var = Some(a);
        }
    }

    if endpoint.is_empty() {
        if let Ok(contents) = std::fs::read_to_string(&caps_rtfs) {
            if let Some(e) = extract_endpoint_from_capabilities(&contents) {
                endpoint = e;
            }
        }
    }

    let display_name = format!("{}{}", bucket_prefix, name);
    let tool_count = Some(count_rtfs_files(dir));
    Some((display_name, endpoint, tool_count, auth_env_var))
}

/// Extract the source type from a server.rtfs file.
/// Returns "MCP", "OpenAPI", "WebSearch", or "Browser" based on the :source :type field.
fn extract_server_source_type(dir: &std::path::Path) -> Option<String> {
    let server_rtfs = dir.join("server.rtfs");
    if !server_rtfs.exists() {
        return None;
    }

    let content = std::fs::read_to_string(&server_rtfs).ok()?;

    // Look for :source {:type "..." ...} pattern
    if let Some(source_start) = content.find(":source {") {
        let rest = &content[source_start..];
        // Find :type within the source block
        if let Some(type_start) = rest.find(":type") {
            let type_rest = &rest[type_start..];
            // Extract the quoted value after :type
            if let Some(quote_start) = type_rest.find('"') {
                let after_quote = &type_rest[quote_start + 1..];
                if let Some(quote_end) = after_quote.find('"') {
                    return Some(after_quote[..quote_end].to_string());
                }
            }
        }
    }
    
    // Also check for :source {\"type\" \"...\" ...} format (escaped quotes in RTFS)
    if content.contains("\"type\" \"MCP\"") || content.contains(":type \"MCP\"") {
        return Some("MCP".to_string());
    }
    if content.contains("\"type\" \"OpenAPI\"") || content.contains(":type \"OpenAPI\"") {
        return Some("OpenAPI".to_string());
    }
    if content.contains("\"type\" \"WebSearch\"") || content.contains(":type \"WebSearch\"") {
        return Some("WebSearch".to_string());
    }
    if content.contains("\"type\" \"Browser\"") || content.contains(":type \"Browser\"") {
        return Some("Browser".to_string());
    }

    None
}

/// Get the appropriate CapabilityCategory based on server source type
fn category_for_source_type(source_type: Option<&str>) -> CapabilityCategory {
    match source_type {
        Some("MCP") => CapabilityCategory::McpTool,
        Some("OpenAPI") | Some("WebSearch") => CapabilityCategory::OpenApiTool,
        Some("Browser") => CapabilityCategory::BrowserApiTool,
        _ => CapabilityCategory::McpTool, // Default fallback
    }
}

/// Extract the source spec URL from a server.rtfs file.
/// Returns the URL from :source :entry :url or :source :spec_url for WebSearch/OpenAPI sources.
fn extract_server_spec_url(dir: &std::path::Path) -> Option<String> {
    let server_rtfs = dir.join("server.rtfs");
    if !server_rtfs.exists() {
        return None;
    }

    let content = std::fs::read_to_string(&server_rtfs).ok()?;

    // Look for :source {...} block and extract :entry :url or "url" or :spec_url
    if let Some(source_start) = content.find(":source {") {
        let rest = &content[source_start..];
        // Find matching closing brace (simple approach - find next })
        if let Some(source_end) = rest.find('}') {
            let source_block = &rest[..source_end + 1];
            
            // Look for :spec_url "..." pattern (CoinMarketCap format)
            if let Some(url_idx) = source_block.find(":spec_url") {
                let after_key = &source_block[url_idx + 9..];
                let after_key = after_key.trim_start();
                if after_key.starts_with('"') {
                    if let Some(end_quote) = after_key[1..].find('"') {
                        return Some(after_key[1..end_quote + 1].to_string());
                    }
                }
            }
            
            // Look for "url" "..." pattern (compact format)
            if let Some(url_idx) = source_block.find("\"url\"") {
                let after_key = &source_block[url_idx + 5..];
                let after_key = after_key.trim_start();
                if after_key.starts_with('"') {
                    if let Some(end_quote) = after_key[1..].find('"') {
                        return Some(after_key[1..end_quote + 1].to_string());
                    }
                }
            }
            
            // Look for :url "..." pattern
            if let Some(url_idx) = source_block.find(":url") {
                let after_key = &source_block[url_idx + 4..];
                let after_key = after_key.trim_start();
                if after_key.starts_with('"') {
                    if let Some(end_quote) = after_key[1..].find('"') {
                        return Some(after_key[1..end_quote + 1].to_string());
                    }
                }
            }
        }
    }

    None
}

/// Get source type and spec URL for a server by name
fn get_server_source_info(server_name: &str) -> (Option<String>, Option<String>) {
    let caps_base = get_capabilities_base_path();
    
    // Check approved servers
    let approved_base = caps_base.join("servers/approved");
    
    // Try direct match
    let server_dir = approved_base.join(server_name);
    if server_dir.join("server.rtfs").exists() {
        return (extract_server_source_type(&server_dir), extract_server_spec_url(&server_dir));
    }
    
    // Try sanitized name
    let sanitized = ccos::utils::fs::sanitize_filename(server_name);
    let server_dir = approved_base.join(&sanitized);
    if server_dir.join("server.rtfs").exists() {
        return (extract_server_source_type(&server_dir), extract_server_spec_url(&server_dir));
    }
    
    // Scan subdirectories for nested servers
    if let Ok(entries) = std::fs::read_dir(&approved_base) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                // Look in subdirectories
                if let Ok(sub_entries) = std::fs::read_dir(&path) {
                    for sub_entry in sub_entries.flatten() {
                        let sub_path = sub_entry.path();
                        if sub_path.is_dir() && sub_path.join("server.rtfs").exists() {
                            // Check if this matches the server name
                            if let Some(name) = sub_path.file_name().and_then(|n| n.to_str()) {
                                if name == server_name || name == sanitized {
                                    return (extract_server_source_type(&sub_path), extract_server_spec_url(&sub_path));
                                }
                            }
                        }
                    }
                }
            }
        }
    }
    
    (None, None)
}

fn load_servers_async(event_tx: mpsc::UnboundedSender<TuiEvent>) {
    // Signal loading started
    let _ = event_tx.send(TuiEvent::ServersLoading);

    tokio::task::spawn_local(async move {
        use ccos::mcp::core::MCPDiscoveryService;
        use std::collections::{HashMap, HashSet};

        // Best-effort cleanup: remove empty folders under servers/pending (e.g. leftover namespaces).
        // This keeps the filesystem tidy even if a prior delete partially succeeded.
        let pending_root = get_capabilities_base_path().join("servers/pending");
        if let Ok(entries) = std::fs::read_dir(&pending_root) {
            for entry in entries.flatten() {
                let path = entry.path();
                if !path.is_dir() {
                    continue;
                }
                if let Ok(mut child_entries) = std::fs::read_dir(&path) {
                    if child_entries.next().is_none() {
                        let _ = std::fs::remove_dir(&path);
                    }
                }
            }
        }

        // Queue is used for sync/enrichment (e.g. queue_id), but not as the proof of existence.
        let mut queue_id_by_endpoint: HashMap<String, String> = HashMap::new();
        if let Ok(queue) = create_unified_queue() {
            if let Ok(pending_requests) = queue.list_pending_servers().await {
                for r in pending_requests {
                    if let Some(p) = r.to_pending_discovery() {
                        queue_id_by_endpoint.insert(normalize_endpoint(&p.server_info.endpoint), p.id);
                    }
                }
            }
            if let Ok(approved_requests) = queue.list_approved_servers().await {
                for r in approved_requests {
                    if let Some(a) = r.to_approved_discovery() {
                        queue_id_by_endpoint.insert(normalize_endpoint(&a.server_info.endpoint), a.id);
                    }
                }
            }
        }

        let hidden = load_hidden_servers_config();

        let mut servers: Vec<ServerInfo> = Vec::new();
        let mut seen_endpoints: HashSet<String> = HashSet::new();

        // Source of truth: filesystem under capabilities/servers/*
        let caps_base = get_capabilities_base_path();
        let servers_root = caps_base.join("servers");
        // IMPORTANT: Prioritize 'approved' so that if a server exists in both pending and approved,
        // it shows correctly as approved in the view (seen_endpoints will skip later pending duplicates).
        let buckets: Vec<(&str, ServerStatus, &str)> = vec![
            ("approved", ServerStatus::Connected, ""),
            ("pending", ServerStatus::Pending, ""),
            ("timeout", ServerStatus::Timeout, ""),
            ("rejected", ServerStatus::Rejected, ""),
        ];

        for (bucket, status, prefix) in buckets {
            let bucket_dir = servers_root.join(bucket);
            let mut stack = vec![bucket_dir];

            while let Some(current_dir) = stack.pop() {
                let Ok(entries) = std::fs::read_dir(&current_dir) else {
                    continue;
                };

                for entry in entries.flatten() {
                    let path = entry.path();
                    if !path.is_dir() {
                        continue;
                    }

                    if path.join("server.rtfs").exists() {
                        let Some((display_name, endpoint_raw, tool_count, _auth_env_var)) =
                            infer_server_from_dir(prefix, &path)
                        else {
                            continue;
                        };

                        let normalized = if endpoint_raw.is_empty() {
                            String::new()
                        } else {
                            normalize_endpoint(&endpoint_raw)
                        };

                        if !normalized.is_empty() {
                            if seen_endpoints.contains(&normalized) {
                                continue;
                            }
                            seen_endpoints.insert(normalized.clone());
                        }

                        let queue_id = if !normalized.is_empty() {
                            queue_id_by_endpoint.get(&normalized).cloned()
                        } else {
                            None
                        };

                        let tools = list_tools_in_dir(&path);
                        let tool_count = if tools.is_empty() {
                            tool_count
                        } else {
                            Some(tools.len())
                        };

                        servers.push(ServerInfo {
                            name: display_name,
                            endpoint: endpoint_raw,
                            status,
                            tool_count,
                            tools,
                            last_checked: None,
                            directory_path: Some(path.to_string_lossy().to_string()),
                            queue_id,
                        });
                    } else {
                        // RECURSION: explore deeper if no server.rtfs here
                        stack.push(path);
                    }
                }
            }
        }

        // Add known servers from config (if not already present on disk)
        let service = MCPDiscoveryService::new();
        let mcp_servers = service.list_known_servers().await;
        
        {
            use std::io::Write;
            if let Ok(mut file) = std::fs::OpenOptions::new().create(true).append(true).open("/tmp/ccos_debug.log") {
                let _ = writeln!(file, "[{}] Loaded {} servers from disk, seen_endpoints: {:?}", 
                    chrono::Utc::now(), servers.len(), seen_endpoints);
                let _ = writeln!(file, "[{}] list_known_servers returned {} servers", 
                    chrono::Utc::now(), mcp_servers.len());
            }
        }

        for config in mcp_servers {
            let normalized = normalize_endpoint(&config.endpoint);
            
            {
                use std::io::Write;
                if let Ok(mut file) = std::fs::OpenOptions::new().create(true).append(true).open("/tmp/ccos_debug.log") {
                    let _ = writeln!(file, "[{}] Checking config server '{}' ({}) -> normalized: '{}'", 
                        chrono::Utc::now(), config.name, config.endpoint, normalized);
                    let _ = writeln!(file, "[{}] seen_endpoints contains it? {}", 
                        chrono::Utc::now(), seen_endpoints.contains(&normalized));
                }
            }
            
            if !normalized.is_empty() && !seen_endpoints.contains(&normalized) {
                if hidden.names.iter().any(|n| n == &config.name)
                    || hidden.endpoints.iter().any(|e| e == &config.endpoint)
                {
                    continue;
                }

                servers.push(ServerInfo {
                    name: config.name,
                    endpoint: config.endpoint,
                    status: ServerStatus::Unknown,
                    tool_count: None,
                    tools: vec![],
                    last_checked: None,
                    directory_path: None,
                    queue_id: None,
                });
                seen_endpoints.insert(normalized);
            }
        }

        let _ = event_tx.send(TuiEvent::ServersLoaded(servers));
    });
}

#[derive(Debug, serde::Deserialize)]
struct ChatAuditResponse {
    events: Vec<ChatAuditEvent>,
}

#[derive(Debug, serde::Deserialize)]
struct ChatAuditEvent {
    timestamp: u64,
    event_type: String,
    function_name: Option<String>,
    session_id: Option<String>,
    run_id: Option<String>,
    step_id: Option<String>,
    rule_id: Option<String>,
    decision: Option<String>,
    gate: Option<String>,
    message_id: Option<String>,
    payload_classification: Option<String>,
}

fn load_chat_audit_async(event_tx: mpsc::UnboundedSender<TuiEvent>, endpoint: String) {
    let _ = event_tx.send(TuiEvent::ChatAuditLoading);

    tokio::task::spawn_local(async move {
        let client = Client::new();
        let url = format!("{}?limit=200", endpoint.trim_end_matches('/'));
        let resp = client.get(url).send().await;
        let result = match resp {
            Ok(resp) => resp
                .json::<ChatAuditResponse>()
                .await
                .map_err(|e| format!("Failed to parse response: {}", e)),
            Err(e) => Err(format!("Request failed: {}", e)),
        };

        match result {
            Ok(payload) => {
                let mut entries = Vec::new();
                for event in payload.events {
                    let mut details = Vec::new();
                    if let Some(session_id) = event.session_id.clone() {
                        details.push(("session_id".to_string(), session_id));
                    }
                    if let Some(run_id) = event.run_id.clone() {
                        details.push(("run_id".to_string(), run_id));
                    }
                    if let Some(step_id) = event.step_id.clone() {
                        details.push(("step_id".to_string(), step_id));
                    }
                    if let Some(rule_id) = event.rule_id.clone() {
                        details.push(("rule_id".to_string(), rule_id));
                    }
                    if let Some(decision) = event.decision.clone() {
                        details.push(("decision".to_string(), decision));
                    }
                    if let Some(gate) = event.gate.clone() {
                        details.push(("gate".to_string(), gate));
                    }
                    if let Some(message_id) = event.message_id.clone() {
                        details.push(("message_id".to_string(), message_id));
                    }
                    if let Some(payload_classification) = event.payload_classification.clone() {
                        details.push((
                            "payload_classification".to_string(),
                            payload_classification,
                        ));
                    }
                    if let Some(function_name) = event.function_name.clone() {
                        details.push(("function_name".to_string(), function_name));
                    }

                    let summary = event
                        .decision
                        .clone()
                        .or(event.rule_id.clone())
                        .or(event.message_id.clone())
                        .unwrap_or_else(|| "event".to_string());

                    entries.push(ChatAuditEntry {
                        timestamp: event.timestamp,
                        event_type: event.event_type,
                        summary,
                        details,
                    });
                }

                let _ = event_tx.send(TuiEvent::ChatAuditLoaded(entries));
            }
            Err(err) => {
                let _ = event_tx.send(TuiEvent::ChatAuditError(err));
            }
        }
    });
}

/// Discover tools for a specific server
fn discover_server_tools_async(
    event_tx: mpsc::UnboundedSender<TuiEvent>,
    server_index: usize,
    server_name: String,
    endpoint: String,
) {
    tokio::task::spawn_local(async move {
        use ccos::mcp::core::MCPDiscoveryService;
        use ccos::mcp::types::DiscoveryOptions;
        use ccos::mcp::types::MCPServerConfig;

        let service = MCPDiscoveryService::new();

        if endpoint.trim().is_empty() {
            let _ = event_tx.send(TuiEvent::ServerConnectionChecked {
                server_index,
                status: ServerStatus::Disconnected,
            });
            return;
        }

        let desired_endpoint = endpoint.trim().trim_end_matches('/');

        // Find the server config matching this endpoint
        let server_config = service
            .list_known_servers()
            .await
            .into_iter()
            .find(|s| s.endpoint.trim().trim_end_matches('/') == desired_endpoint);

        let config = server_config.unwrap_or_else(|| MCPServerConfig {
            name: server_name,
            endpoint,
            auth_token: None,
            timeout_seconds: 30,
            protocol_version: "2024-11-05".to_string(),
        });

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
    });
}

/// Check connection to a specific server
fn check_server_connection_async(
    event_tx: mpsc::UnboundedSender<TuiEvent>,
    server_index: usize,
    server_name: String,
    endpoint: String,
) {
    tokio::task::spawn_local(async move {
        use ccos::mcp::core::MCPDiscoveryService;
        use ccos::mcp::types::DiscoveryOptions;
        use ccos::mcp::types::MCPServerConfig;

        let service = MCPDiscoveryService::new();

        if endpoint.trim().is_empty() {
            let _ = event_tx.send(TuiEvent::ServerConnectionChecked {
                server_index,
                status: ServerStatus::Disconnected,
            });
            return;
        }

        let desired_endpoint = endpoint.trim().trim_end_matches('/');

        // Find the server config matching this endpoint
        let server_config = service
            .list_known_servers()
            .await
            .into_iter()
            .find(|s| s.endpoint.trim().trim_end_matches('/') == desired_endpoint);

        let config = server_config.unwrap_or_else(|| MCPServerConfig {
            name: server_name,
            endpoint,
            auth_token: None,
            timeout_seconds: 30,
            protocol_version: "2024-11-05".to_string(),
        });

        // Try to discover tools as a connection check
        let options = DiscoveryOptions::default();
        let status = match service.discover_tools(&config, &options).await {
            Ok(_) => ServerStatus::Connected,
            Err(_) => ServerStatus::Error,
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

    tokio::spawn(async move {
        use ccos::capabilities::registry::CapabilityRegistry;
        use ccos::mcp::core::MCPDiscoveryService;
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
                let name = id.split('.').last().unwrap_or(&id).to_string();

                // Try to get the full capability to extract schemas and description
                let (description, input_schema, output_schema) = registry
                    .get_capability(&id)
                    .map(|cap| {
                        let desc = cap
                            .description
                            .clone()
                            .unwrap_or_else(|| format!("Built-in capability: {}", id));
                        let input = cap.input_schema.as_ref().map(type_expr_to_rtfs_pretty);
                        let output = cap.output_schema.as_ref().map(type_expr_to_rtfs_pretty);
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

        // --- Load MCP capabilities from filesystem (Source of Truth) ---
        let mut seen_endpoints = std::collections::HashSet::new();
        let caps_base = get_capabilities_base_path();
        let servers_root = caps_base.join("servers");

        // Scan the servers directory (approved and pending)
        let mut stack = vec![servers_root.clone()];
        while let Some(current_dir) = stack.pop() {
            if let Ok(entries) = std::fs::read_dir(&current_dir) {
                for entry in entries.flatten() {
                    let path = entry.path();
                    if !path.is_dir() {
                        continue;
                    }

                    // Skip negative result directories
                    if let Some(name) = path.file_name().and_then(|n| n.to_str()) {
                        if name == "rejected" || name == "timeout" {
                            continue;
                        }
                    }

                    if path.join("server.rtfs").exists() {
                        if let Some((name, endpoint, _, _)) = infer_server_from_dir("", &path) {
                            if !endpoint.is_empty() {
                                seen_endpoints.insert(endpoint.clone());
                            }

                            // Detect server source type for correct categorization
                            let source_type = extract_server_source_type(&path);
                            let category = category_for_source_type(source_type.as_deref());

                            // Read details from disk instead of online discovery
                            let tools = list_tools_detailed_in_dir(&path);
                            for t in tools {
                                capabilities.push(DiscoveredCapability {
                                    id: format!("mcp:{}:{}", name, t.name),
                                    name: t.name,
                                    description: t
                                        .description
                                        .unwrap_or_else(|| format!("Local capability from {}", name)),
                                    source: name.clone(),
                                    category,
                                    version: None,
                                    input_schema: t.input_schema,
                                    output_schema: t.output_schema,
                                    permissions: vec![],
                                    effects: vec![],
                                    metadata: HashMap::new(),
                                });
                            }
                        }
                    } else {
                        // Deeper exploration (namespaces)
                        stack.push(path);
                    }
                }
            }
        }

        // --- Also Load from configuration (Known Servers) but defer introspection ---
        // We don't introspect servers at startup to avoid slow loading times.
        // Instead, we add placeholder entries that indicate the server exists.
        // Users can select a server in the Discover menu to trigger introspection.
        let discovery_service = MCPDiscoveryService::new();
        let known_servers = discovery_service.list_known_servers().await;
        for server in known_servers {
            let normalized = normalize_endpoint(&server.endpoint);
            if normalized.is_empty() {
                continue;
            }
            if seen_endpoints.contains(&normalized) {
                continue;
            }
            seen_endpoints.insert(normalized);

            // Add a placeholder entry for the server (no tools introspected yet)
            // The user can select the server to trigger introspection
            capabilities.push(DiscoveredCapability {
                id: format!("mcp:{}:_server", server.name),
                name: server.name.clone(),
                description: format!("Server: {} (select to discover tools)", server.endpoint),
                source: format!("Known Server: {}", server.name),
                category: CapabilityCategory::McpTool,
                version: None,
                input_schema: None,
                output_schema: None,
                permissions: vec![],
                effects: vec![],
                metadata: HashMap::new(),
            });
        }

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
                                                .map(type_expr_to_rtfs_pretty),
                                            output_schema: manifest
                                                .output_schema
                                                .as_ref()
                                                .map(type_expr_to_rtfs_pretty),
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

fn search_discovery_async(query: String, event_tx: mpsc::UnboundedSender<TuiEvent>) {
    // use ccos::ops::server::search_servers; // Removed in favor of direct RegistrySearcher usage

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
        let _ = event_tx.send(TuiEvent::IntrospectionLog("Searching MCP Registry (Remote)...".to_string()));
        let searcher = RegistrySearcher::new();
        
        let _ = event_tx.send(TuiEvent::IntrospectionLog("Searching NPM Marketplace...".to_string()));
        // Note: RegistrySearcher::search internally calls all engines
        // We can't easily get fine-grained logs from inside it without changing its API
        // but we can definitely log when it returns.
        
        match searcher.search(&query_clone).await {
            Ok(results) => {
                let _ = event_tx.send(TuiEvent::IntrospectionLog(format!("Found {} matching servers/tools.", results.len())));
                
                // Show which ones were found
                for res in results.iter().take(5) {
                    let _ = event_tx.send(TuiEvent::IntrospectionLog(format!(" - {}", res.server_info.name)));
                }
                if results.len() > 5 {
                    let _ = event_tx.send(TuiEvent::IntrospectionLog(format!(" ... and {} more", results.len() - 5)));
                }

                // Log how many we found
                let _ = event_tx.send(TuiEvent::Trace(
                    TraceEventType::ToolDiscovery,
                    format!("RegistrySearcher returned {} items", results.len()),
                    None,
                ));

                tokio::time::sleep(tokio::time::Duration::from_millis(500)).await;
                let _ = event_tx.send(TuiEvent::DiscoverySearchComplete(results));
            }
            Err(e) => {
                let _ = event_tx.send(TuiEvent::IntrospectionLog(format!("❌ Discovery search failed: {}", e)));
                // Log the error so user knows what happened
                let _ = event_tx.send(TuiEvent::Trace(
                    TraceEventType::ToolDiscovery,
                    format!("Discovery search failed: {}", e),
                    None,
                ));
                tokio::time::sleep(tokio::time::Duration::from_millis(1500)).await;
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

    // Try MCP introspection first (no side effects)
    let _ = event_tx.send(TuiEvent::IntrospectionLog(
        "Attempting MCP introspection...".to_string(),
    ));
    
    // Explicitly log the timeout/attempt
    let _ = event_tx.send(TuiEvent::IntrospectionLog(
        "Waiting for server initialization (up to 30s)...".to_string(),
    ));

    let (mcp_error, mcp_auth_error) = match introspect_server_by_url(&endpoint, &server_name, auth_env_var).await {
        Ok(result) => {
            if !result.tools.is_empty() {
                let _ = event_tx.send(TuiEvent::IntrospectionLog(format!(
                    "Success! Found {} MCP tools.",
                    result.tools.len()
                )));

                let discovered_tools: Vec<DiscoveredCapability> = result
                    .tools
                    .iter()
                    .map(|tool| {
                        let mut metadata = std::collections::HashMap::new();
                        // Store original JSON schema for persistence reconstruction
                        if let Some(json) = &tool.input_schema_json {
                            metadata.insert("input_schema_json".to_string(), json.to_string());
                        }

                        DiscoveredCapability {
                            id: format!("mcp:{}:{}", server_name, tool.tool_name),
                            name: tool.tool_name.clone(),
                            description: tool.description.clone().unwrap_or_default(),
                            source: server_name.clone(),
                            category: CapabilityCategory::McpTool,
                            version: None,
                            // Use RTFS format for display/TUI
                            input_schema: tool.input_schema.as_ref().map(type_expr_to_rtfs_pretty),
                            output_schema: None,
                            permissions: Vec::new(),
                            effects: Vec::new(),
                            metadata,
                        }
                    })
                    .collect();

                let _ = event_tx.send(TuiEvent::IntrospectionComplete {
                    server_name: server_name.clone(),
                    endpoint: endpoint.clone(),
                    tools: discovered_tools,
                });
                return;
            }

            (Some("MCP introspection returned no tools".to_string()), false)
        }
        Err(e) => {
            let error_str = format!("{}", e);
            let auth_error = error_str.contains("MCP_AUTH_TOKEN")
                || error_str.contains("not set")
                || error_str.contains("401")
                || error_str.contains("Unauthorized")
                || error_str.contains("authentication")
                || error_str.contains("token")
                || error_str.contains("auth");
            (Some(error_str), auth_error)
        }
    };

    let _ = event_tx.send(TuiEvent::IntrospectionLog(
        "MCP introspection did not succeed; trying OpenAPI/Browser fallback...".to_string(),
    ));

    // Check if server has an OpenAPI/WebSearch source type and get spec URL
    let (source_type, spec_url) = get_server_source_info(&server_name);
    
    // For OpenAPI/WebSearch sources, use the spec URL if available
    let introspection_url = if matches!(source_type.as_deref(), Some("OpenAPI") | Some("WebSearch")) {
        if let Some(ref url) = spec_url {
            let _ = event_tx.send(TuiEvent::IntrospectionLog(format!(
                "Using OpenAPI spec URL: {}", url
            )));
            url.clone()
        } else {
            let _ = event_tx.send(TuiEvent::IntrospectionLog(
                "No spec URL found for OpenAPI source, falling back to endpoint".to_string()
            ));
            endpoint.clone()
        }
    } else {
        endpoint.clone()
    };

    let introspection_service = IntrospectionService::empty()
        .with_browser_discovery(std::sync::Arc::new(BrowserDiscoveryService::new()));

    let fallback_result = if IntrospectionService::is_openapi_url(&introspection_url) 
        || matches!(source_type.as_deref(), Some("OpenAPI") | Some("WebSearch")) {
        introspection_service
            .introspect_openapi(&introspection_url, &server_name)
            .await
    } else {
        introspection_service
            .introspect_browser(&introspection_url, &server_name)
            .await
    };

    match fallback_result {
        Ok(result) if result.success => {
            let mut discovered_tools: Vec<DiscoveredCapability> = Vec::new();

            if let Some(api_result) = &result.api_result {
                for ep in &api_result.endpoints {
                    let mut metadata = std::collections::HashMap::new();
                    metadata.insert("source_type".to_string(), "openapi".to_string());
                    metadata.insert("method".to_string(), ep.method.clone());
                    metadata.insert("path".to_string(), ep.path.clone());

                    let name = if ep.name.trim().is_empty() {
                        format!("{} {}", ep.method, ep.path)
                    } else {
                        ep.name.clone()
                    };

                    discovered_tools.push(DiscoveredCapability {
                        id: format!("openapi:{}:{}", server_name, ep.endpoint_id),
                        name,
                        description: ep.description.clone(),
                        source: server_name.clone(),
                        category: CapabilityCategory::OpenApiTool,
                        version: None,
                        input_schema: ep.input_schema.as_ref().map(type_expr_to_rtfs_pretty),
                        output_schema: ep.output_schema.as_ref().map(type_expr_to_rtfs_pretty),
                        permissions: Vec::new(),
                        effects: Vec::new(),
                        metadata,
                    });
                }
            }

            if let Some(browser_result) = &result.browser_result {
                for ep in &browser_result.discovered_endpoints {
                    let mut metadata = std::collections::HashMap::new();
                    metadata.insert("source_type".to_string(), "browser".to_string());
                    metadata.insert("method".to_string(), ep.method.clone());
                    metadata.insert("path".to_string(), ep.path.clone());

                    let name = format!("{} {}", ep.method, ep.path);
                    let id_path = ep.path.replace('/', "_");

                    discovered_tools.push(DiscoveredCapability {
                        id: format!("browser:{}:{}:{}", server_name, ep.method, id_path),
                        name,
                        description: ep.description.clone().unwrap_or_default(),
                        source: server_name.clone(),
                        category: CapabilityCategory::BrowserApiTool,
                        version: None,
                        input_schema: ep.input_schema.as_ref().map(type_expr_to_rtfs_pretty),
                        output_schema: ep.output_schema.as_ref().map(type_expr_to_rtfs_pretty),
                        permissions: Vec::new(),
                        effects: Vec::new(),
                        metadata,
                    });
                }
            }

            let _ = event_tx.send(TuiEvent::IntrospectionLog(format!(
                "Fallback discovery found {} endpoints.",
                discovered_tools.len()
            )));

            let _ = event_tx.send(TuiEvent::IntrospectionComplete {
                server_name: server_name.clone(),
                endpoint: endpoint.clone(),
                tools: discovered_tools,
            });
        }
        Ok(result) => {
            let error_msg = result
                .error
                .unwrap_or_else(|| "OpenAPI/Browser introspection failed".to_string());

            if mcp_auth_error {
                let env_var = suggest_auth_env_var(&server_name);
                let _ = event_tx.send(TuiEvent::IntrospectionAuthRequired {
                    server_name: server_name.clone(),
                    endpoint: endpoint.clone(),
                    env_var,
                });
            } else {
                let combined = if let Some(mcp_err) = mcp_error {
                    format!("MCP error: {}\nFallback error: {}", mcp_err, error_msg)
                } else {
                    error_msg
                };
                let _ = event_tx.send(TuiEvent::IntrospectionFailed {
                    server_name: server_name.clone(),
                    error: combined,
                });
            }
        }
        Err(e) => {
            let error_msg = format!("{}", e);
            if mcp_auth_error {
                let env_var = suggest_auth_env_var(&server_name);
                let _ = event_tx.send(TuiEvent::IntrospectionAuthRequired {
                    server_name: server_name.clone(),
                    endpoint: endpoint.clone(),
                    env_var,
                });
            } else {
                let combined = if let Some(mcp_err) = mcp_error {
                    format!("MCP error: {}\nFallback error: {}", mcp_err, error_msg)
                } else {
                    error_msg
                };
                let _ = event_tx.send(TuiEvent::IntrospectionFailed {
                    server_name: server_name.clone(),
                    error: combined,
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
        DiscoverPopup::ServerSearchInput {
            query,
            cursor_position,
        } => match key.code {
            KeyCode::Enter => {
                let q = query.trim().to_string();
                if !q.is_empty() {
                    // Reuse the existing discovery search pipeline.
                    state.discover_search_hint = q.clone();
                    
                    // Show loading state directly instead of closing
                    next_popup = Some(DiscoverPopup::Introspecting {
                        server_name: format!("Searching: {}", q),
                        endpoint: "In progress...".to_string(),
                        logs: vec![format!("Starting discovery search for '{}'...", q)],
                        return_to_results: None,
                    });

                    search_discovery_async(q, event_tx);
                } else {
                    next_popup = Some(DiscoverPopup::None);
                }
            }
            KeyCode::Esc => {
                next_popup = Some(DiscoverPopup::None);
            }
            KeyCode::Left => {
                *cursor_position = cursor_position.saturating_sub(1);
            }
            KeyCode::Right => {
                *cursor_position = (*cursor_position + 1).min(query.len());
            }
            KeyCode::Home => {
                *cursor_position = 0;
            }
            KeyCode::End => {
                *cursor_position = query.len();
            }
            KeyCode::Backspace => {
                if *cursor_position > 0 && *cursor_position <= query.len() {
                    let remove_at = cursor_position.saturating_sub(1);
                    query.remove(remove_at);
                    *cursor_position = remove_at;
                }
            }
            KeyCode::Delete => {
                if *cursor_position < query.len() {
                    query.remove(*cursor_position);
                }
            }
            KeyCode::Char(c) => {
                if *cursor_position <= query.len() {
                    query.insert(*cursor_position, c);
                    *cursor_position += 1;
                }
            }
            _ => {}
        },
        DiscoverPopup::ServerSuggestions {
            results,
            selected,
            breadcrumbs,
            ..
        } => match key.code {
            KeyCode::Up | KeyCode::Char('k') => {
                *selected = selected.saturating_sub(1);
            }
            KeyCode::Down | KeyCode::Char('j') => {
                if !results.is_empty() {
                    *selected = (*selected + 1).min(results.len() - 1);
                }
            }
            KeyCode::Enter => {
                if let Some(s) = results.get(*selected) {
                    let server = s.clone();
                    let event_tx_clone = event_tx.clone();
                    
                    match server.category {
                        DiscoveryCategory::WebDoc => {
                             // Show loading state
                             next_popup = Some(DiscoverPopup::Introspecting {
                                 server_name: format!("Parsing: {}", server.server_info.name),
                                 endpoint: server.server_info.endpoint.clone(),
                                 logs: vec!["Using LLM to extract API endpoints from documentation...".to_string()],
                                 return_to_results: Some((results.clone(), breadcrumbs.clone())),
                             });
                             // Drill down
                             tokio::task::spawn_local(async move {
                                 drill_down_discovery_async(server, event_tx_clone).await;
                             });
                        }
                        _ => {
                            next_popup = Some(DiscoverPopup::Introspecting {
                                server_name: server.server_info.name.clone(),
                                endpoint: server.server_info.endpoint.clone(),
                                logs: Vec::new(),
                                return_to_results: Some((results.clone(), breadcrumbs.clone())),
                            });

                            let server_name = server.server_info.name.clone();
                            let endpoint = server.server_info.endpoint.clone();
                            tokio::task::spawn_local(async move {
                                introspect_server_async(server_name, endpoint, event_tx_clone).await;
                            });
                        }
                    }
                }
            }
            KeyCode::Esc => {
                next_popup = Some(DiscoverPopup::None);
            }
            _ => {}
        },
        DiscoverPopup::SearchResults { servers, selected, breadcrumbs, .. } => {
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
                    if let Some(server) = servers.get(*selected) {
                        // Handle selection centrally
                        let server = server.clone();
                        let event_tx_clone = event_tx.clone();
                        
                        match server.category {
                            DiscoveryCategory::WebDoc => {
                                // Show loading state
                                next_popup = Some(DiscoverPopup::Introspecting {
                                    server_name: format!("Parsing: {}", server.server_info.name),
                                    endpoint: server.server_info.endpoint.clone(),
                                    logs: vec!["Using LLM to extract API endpoints from documentation...".to_string()],
                                    return_to_results: Some((servers.clone(), breadcrumbs.clone())),
                                });
                                // Drill down
                                tokio::task::spawn_local(async move {
                                    drill_down_discovery_async(server, event_tx_clone).await;
                                });
                            }
                            DiscoveryCategory::OpenApiTool | DiscoveryCategory::BrowserApiTool => {
                                // Transition to IntrospectionResults instead of ToolDetails
                                // This allows the interactive selection/creation UI
                                let desc = server.server_info.description.clone()
                                    .unwrap_or_else(|| "No description available".to_string());
                                
                                let cap_category = match server.category {
                                    DiscoveryCategory::OpenApiTool => CapabilityCategory::OpenApiTool,
                                    DiscoveryCategory::BrowserApiTool => CapabilityCategory::BrowserApiTool,
                                    _ => CapabilityCategory::McpTool,
                                };

                                let capability = DiscoveredCapability {
                                    id: server.server_info.name.clone(),
                                    name: server.server_info.name.clone(),
                                    description: desc,
                                    source: server.server_info.name.clone(),
                                    category: cap_category,
                                    version: None,
                                    input_schema: None,
                                    output_schema: None,
                                    permissions: Vec::new(),
                                    effects: Vec::new(),
                                    metadata: std::collections::HashMap::new(),
                                };

                                next_popup = Some(DiscoverPopup::IntrospectionResults {
                                    server_name: server.server_info.name.clone(),
                                    endpoint: server.server_info.endpoint.clone(),
                                    tools: vec![capability],
                                    selected: 0,
                                    selected_tools: [0].iter().cloned().collect(),
                                    added_success: false,
                                    pended_success: false,
                                    editing_name: false,
                                    return_to_results: Some((servers.clone(), breadcrumbs.clone())),
                                });
                            }
                            _ => {
                                // Introspect
                                next_popup = Some(DiscoverPopup::Introspecting {
                                    server_name: server.server_info.name.clone(),
                                    endpoint: server.server_info.endpoint.clone(),
                                    logs: Vec::new(),
                                    return_to_results: Some((servers.clone(), breadcrumbs.clone())),
                                });

                                let server_name = server.server_info.name.clone();
                                let endpoint = server.server_info.endpoint.clone();
                                tokio::task::spawn_local(async move {
                                    introspect_server_async(server_name, endpoint, event_tx_clone).await;
                                });
                            }
                        }
                    }
                }
                KeyCode::Esc => {
                    // Navigate back if stack is not empty
                    let mut handled = false;
                     if let DiscoverPopup::SearchResults { stack, breadcrumbs, .. } = &mut state.discover_popup {
                        if let Some((prev_results, _prev_context)) = stack.pop() {
                             breadcrumbs.pop();
                             // Update in place? Cannot mut borrow in match arm...
                             // We need to trigger a state update or handle it via next_popup
                             // But next_popup is replacement.
                             
                             // Let's just create the new popup state
                             next_popup = Some(DiscoverPopup::SearchResults {
                                 servers: prev_results,
                                 selected: 0,
                                 stack: stack.clone(),
                                 breadcrumbs: breadcrumbs.clone(),
                                 current_category: None,
                             });
                             handled = true;
                        }
                     }
                     
                    if !handled {
                        next_popup = Some(DiscoverPopup::None);
                    }
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
            return_to_results,
            editing_name,
            ..
        } => {
            if *editing_name {
                match key.code {
                    KeyCode::Char(c) => {
                        server_name.push(c);
                    }
                    KeyCode::Backspace => {
                        server_name.pop();
                    }
                    KeyCode::Enter | KeyCode::Esc => {
                        *editing_name = false;
                    }
                    _ => {}
                }
                return;
            }

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
                    // AND also add to pending approval queue (persisted)
                    let tools_to_add: Vec<_> = selected_tools
                        .iter()
                        .filter_map(|idx| tools.get(*idx).cloned())
                        .collect();

                    // Remove existing capabilities from the same server to avoid duplicates
                    // when refreshing/re-introspecting a server
                    let source_to_remove = server_name.clone();
                    state.discovered_capabilities.retain(|cap| cap.source != source_to_remove);

                    for tool in tools_to_add.iter().cloned() {
                        state.discovered_capabilities.push(tool);
                    }

                    // persist it!
                    let server_name_clone = server_name.clone();
                    let endpoint_clone = endpoint.clone();
                    let event_tx_clone = event_tx.clone();

                    let tools_to_save = if tools_to_add.is_empty() {
                        tools.clone()
                    } else {
                        tools_to_add
                    };

                    add_server_to_pending_async(
                        event_tx_clone,
                        server_name_clone,
                        endpoint_clone,
                        tools_to_save,
                    );

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
                KeyCode::Char('c') => {
                    // Select none
                    selected_tools.clear();
                }
                KeyCode::Char('n') => {
                    // Start editing name
                    *editing_name = true;
                }
                KeyCode::Esc => {
                    if let Some((prev_results, prev_breadcrumbs)) = return_to_results.take() {
                        next_popup = Some(DiscoverPopup::SearchResults {
                            servers: prev_results,
                            selected: 0,
                            stack: Vec::new(),
                            breadcrumbs: prev_breadcrumbs,
                            current_category: None,
                        });
                    } else {
                        next_popup = Some(DiscoverPopup::None);
                        state.discover_loading = false;
                    }
                }
                _ => {}
            }
        }
        DiscoverPopup::Introspecting { return_to_results, .. } => {
            if let KeyCode::Esc = key.code {
                if let Some((results, breadcrumbs)) = return_to_results {
                    // Go back to previous search results
                    next_popup = Some(DiscoverPopup::SearchResults {
                        servers: results.clone(),
                        selected: 0,
                        stack: Vec::new(),
                        breadcrumbs: breadcrumbs.clone(),
                        current_category: None,
                    });
                } else {
                    next_popup = Some(DiscoverPopup::None);
                }
            }
        }
        DiscoverPopup::Error { .. } => {
            if let KeyCode::Esc | KeyCode::Enter = key.code {
                next_popup = Some(DiscoverPopup::None);
            }
        }
        DiscoverPopup::Success { .. } => {
            if let KeyCode::Esc | KeyCode::Enter = key.code {
                next_popup = Some(DiscoverPopup::None);
            }
        }
        DiscoverPopup::DeleteConfirmation { server } => match key.code {
            KeyCode::Char('y') | KeyCode::Enter => {
                if let Some(server_id) = server.queue_id.clone() {
                    let server_name = server
                        .name
                        .trim_start_matches("✓ ")
                        .trim_start_matches("⏳ ")
                        .to_string();

                    match server.status {
                        ServerStatus::Pending => {
                            remove_pending_server_async(
                                event_tx.clone(),
                                server_id,
                                server_name.clone(),
                                server.directory_path.clone(),
                            );
                            next_popup = Some(DiscoverPopup::None);
                        }
                        _ => {
                            dismiss_server_async(
                                event_tx.clone(),
                                server_id,
                                server_name.clone(),
                                server.directory_path.clone(),
                            );
                            next_popup = Some(DiscoverPopup::None);
                        }
                    }
                } else if server.directory_path.is_some() {
                    let server_name = server
                        .name
                        .trim_start_matches("✓ ")
                        .trim_start_matches("⏳ ")
                        .to_string();
                    remove_server_directory_async(
                        event_tx.clone(),
                        server_name.clone(),
                        server.directory_path.clone(),
                    );
                    next_popup = Some(DiscoverPopup::None);
                } else {
                    // Known/config server: hide it from the Servers list.
                    hide_known_server_async(
                        event_tx.clone(),
                        server.name.trim_start_matches("✓ ").trim_start_matches("⏳ ").to_string(),
                        server.endpoint.clone(),
                    );
                    next_popup = Some(DiscoverPopup::None);
                }
            }
            KeyCode::Char('n') | KeyCode::Esc => {
                next_popup = Some(DiscoverPopup::None);
            }
            _ => {}
        },
        DiscoverPopup::ToolDetails {
            name,
            endpoint,
            description,
            category: disc_category,
            return_to_results,
        } => {
            match key.code {
                KeyCode::Esc => {
                    // Return to previous search results if available
                    if let Some((results, breadcrumbs)) = return_to_results.clone() {
                        next_popup = Some(DiscoverPopup::SearchResults {
                            servers: results,
                            selected: 0,
                            stack: Vec::new(),
                            breadcrumbs,
                            current_category: Some(DiscoveryCategory::OpenApiTool),
                        });
                    } else {
                        next_popup = Some(DiscoverPopup::None);
                    }
                }
                KeyCode::Enter => {
                    // Add this specific tool/endpoint to the pending approval queue
                    let event_tx_clone = event_tx.clone();
                    let name_clone = name.clone();
                    let endpoint_clone = endpoint.clone();
                    let desc_clone = description.clone();

                    let cap_category = match disc_category {
                        DiscoveryCategory::OpenApiTool => CapabilityCategory::OpenApiTool,
                        DiscoveryCategory::BrowserApiTool => CapabilityCategory::BrowserApiTool,
                        _ => CapabilityCategory::McpTool,
                    };

                    // Construct a single discovered capability for this tool
                    let capability = DiscoveredCapability {
                        id: name_clone.clone(),
                        name: name_clone.clone(),
                        description: desc_clone,
                        source: name_clone.clone(),
                        category: cap_category,
                        version: None,
                        input_schema: None,
                        output_schema: None,
                        permissions: Vec::new(),
                        effects: Vec::new(),
                        metadata: std::collections::HashMap::new(),
                    };

                    add_server_to_pending_async(
                        event_tx_clone,
                        name_clone,
                        endpoint_clone,
                        vec![capability],
                    );

                    next_popup = Some(DiscoverPopup::None);
                }
                _ => {}
            }
        }
        DiscoverPopup::None => {}
    }

    if let Some(popup) = next_popup {
        state.discover_popup = popup;
    }
}

fn handle_discover_list(
    state: &mut AppState,
    key: event::KeyEvent,
    event_tx: mpsc::UnboundedSender<TuiEvent>,
) {
    // Check for Shift+S to refresh schema for the server of the selected capability
    if key.code == KeyCode::Char('S') {
        let target_server = {
            let visible_entries = state.visible_discovery_entries();
            let visible_len = visible_entries.len();
            let selected_idx = state.discover_selected.min(visible_len.saturating_sub(1));

            if let Some(entry) = visible_entries.get(selected_idx) {
                match entry {
                    DiscoveryEntry::Capability(idx) => {
                        if let Some((_, cap)) = state.filtered_discovered_caps().get(*idx) {
                            let source = &cap.source;
                            if let Some(server) = state.servers.iter().find(|s| &s.name == source) {
                                Some((source.clone(), server.endpoint.clone()))
                            } else if source.starts_with("Known Server:") {
                                let name = source
                                    .strip_prefix("Known Server: ")
                                    .unwrap_or(source);
                                state
                                    .servers
                                    .iter()
                                    .find(|s| s.name == name)
                                    .map(|s| (name.to_string(), s.endpoint.clone()))
                            } else {
                                None
                            }
                        } else {
                            None
                        }
                    }
                    DiscoveryEntry::Header { name, .. } => state
                        .servers
                        .iter()
                        .find(|s| &s.name == name)
                        .map(|s| (name.clone(), s.endpoint.clone())),
                }
            } else {
                None
            }
        };

        if let Some((server_name, endpoint)) = target_server {
            state.discover_loading = true;
            state.discover_popup = DiscoverPopup::Introspecting {
                server_name: server_name.clone(),
                endpoint: endpoint.clone(),
                logs: Vec::new(),
                return_to_results: None,
            };
            tokio::spawn(introspect_server_async(server_name, endpoint, event_tx));
        }
        return;
    }

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

async fn drill_down_discovery_async(
    server: RegistrySearchResult,
    event_tx: mpsc::UnboundedSender<TuiEvent>,
) {
    use ccos::ops::browser_discovery::BrowserDiscoveryService;
    use std::sync::Arc;
    
    let source_url = server.server_info.endpoint.clone();
    let server_name = server.server_info.name.clone();
    
    let _ = event_tx.send(TuiEvent::Trace(
        TraceEventType::ToolDiscovery,
        format!("Drilling down into documentation: {}", source_url),
        None,
    ));

    // Create browser discovery service (headless browser for JS-rendered pages)
    let browser_ds = Arc::new(BrowserDiscoveryService::new());
    
    // Create introspection service with browser support
    let introspection_service = ccos::ops::introspection_service::IntrospectionService::empty()
        .with_browser_discovery(browser_ds);

    // Check if URL looks like an OpenAPI spec first
    let result = if ccos::ops::introspection_service::IntrospectionService::is_openapi_url(&source_url) {
        let _ = event_tx.send(TuiEvent::Trace(
            TraceEventType::ToolDiscovery,
            format!("Detected OpenAPI spec, introspecting: {}", source_url),
            None,
        ));
        introspection_service.introspect_openapi(&source_url, &server_name).await
    } else {
        // Use browser-based introspection for HTML docs and SPA pages
        let _ = event_tx.send(TuiEvent::Trace(
            TraceEventType::ToolDiscovery,
            format!("Using browser-based discovery for: {}", source_url),
            None,
        ));
        introspection_service.introspect_browser(&source_url, &server_name).await
    };
    
    match result {
        Ok(introspection_result) => {
            if introspection_result.success {
                // Convert results to RegistrySearchResult format
                let mut results: Vec<RegistrySearchResult> = Vec::new();
                
                // From OpenAPI introspection
                if let Some(api_result) = &introspection_result.api_result {
                    for ep in &api_result.endpoints {
                        results.push(RegistrySearchResult {
                            server_info: ccos::approval::queue::ServerInfo {
                                name: format!("{} {}", ep.method, ep.path),
                                endpoint: api_result.base_url.clone(),
                                description: Some(ep.description.clone()),
                                auth_env_var: None,
                                capabilities_path: None,
                                alternative_endpoints: Vec::new(),
                                capability_files: None,
                            },
                            source: server.source.clone(),
                            category: DiscoveryCategory::OpenApiTool,
                            match_score: 1.0,
                            alternative_endpoints: Vec::new(),
                        });
                    }
                }
                
                // From Browser-based discovery
                if let Some(browser_result) = &introspection_result.browser_result {
                    let base_url = browser_result.api_base_url.clone()
                        .unwrap_or_else(|| browser_result.source_url.clone());
                    
                    for ep in &browser_result.discovered_endpoints {
                        results.push(RegistrySearchResult {
                            server_info: ccos::approval::queue::ServerInfo {
                                name: format!("{} {}", ep.method, ep.path),
                                endpoint: base_url.clone(),
                                description: ep.description.clone(),
                                auth_env_var: None,
                                capabilities_path: None,
                                alternative_endpoints: Vec::new(),
                                capability_files: None,
                            },
                            source: server.source.clone(),
                            category: DiscoveryCategory::BrowserApiTool,
                            match_score: 1.0,
                            alternative_endpoints: Vec::new(),
                        });
                    }
                }
                
                let _ = event_tx.send(TuiEvent::Trace(
                    TraceEventType::ToolDiscovery,
                    format!("Discovered {} endpoints from {}", results.len(), source_url),
                    None,
                ));
                
                let _ = event_tx.send(TuiEvent::DiscoveryDrillDownComplete(results, server_name));
            } else {
                let error_msg = introspection_result.error.unwrap_or_else(|| "Unknown error".to_string());
                let _ = event_tx.send(TuiEvent::IntrospectionFailed {
                    server_name,
                    error: format!("Introspection failed: {}", error_msg),
                });
            }
        }
        Err(e) => {
            let _ = event_tx.send(TuiEvent::IntrospectionFailed {
                server_name,
                error: format!("Introspection error: {}", e),
            });
        }
    }
}
