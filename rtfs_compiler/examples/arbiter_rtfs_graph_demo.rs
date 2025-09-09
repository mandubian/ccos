//! Arbiter RTFS Graph Generation TUI Demo
//!
//! Interactive TUI demo showcasing the Arbiter's ability to generate and execute
//! intent graphs from natural language goals using the CCOS runtime service.
//!
//! Features:
//! - Real-time intent graph visualization
//! - Live orchestration status updates
//! - Interactive goal input and execution control
//! - Plan visualization with step breakdown

use std::collections::{HashMap, HashSet};
use std::io::{self, Write};
use std::sync::Arc;

use clap::Parser;
use crossterm::event::{self, Event as CEvent, KeyCode, KeyModifiers};
use crossterm::terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen};
use crossterm::execute;
use ratatui::{
    backend::CrosstermBackend,
    widgets::{Block, Borders, Paragraph, Wrap, List, ListItem, Clear},
    layout::{Layout, Constraint, Direction, Rect},
    style::{Style, Color, Modifier},
    Terminal,
};
use tokio::sync::{broadcast, mpsc};

use rtfs_compiler::ccos::{CCOS, runtime_service, types::{IntentId, IntentStatus}};

#[derive(Parser)]
struct Args {
    #[arg(long, help = "Initial goal to load (optional)")]
    goal: Option<String>,
}

#[derive(Default)]
struct AppState {
    goal_input: String,
    current_intent: Option<String>,
    status_lines: Vec<String>,
    log_lines: Vec<String>,
    debug_lines: Vec<String>,
    last_result: Option<String>,
    running: bool,
    intent_graph: HashMap<IntentId, IntentNode>,
    plans_by_intent: HashMap<IntentId, PlanInfo>,
    selected_intent: Option<IntentId>,
    root_intent_id: Option<IntentId>,
    show_debug: bool,
    current_tab: Tab,
    help_visible: bool,
    capability_calls: Vec<CapabilityCall>,
    expanded_nodes: HashSet<IntentId>,
    view_mode: ViewMode,
    // Cursor over the VISIBLE, flattened list of nodes in render order
    cursor_index: usize,
    // Flattened display order of visible nodes and their indent depth (rebuilt each frame)
    display_tree: Vec<(IntentId, usize)>,
}

#[derive(Clone, Copy, PartialEq)]
enum Tab {
    Graph,
    Status,
    Logs,
    Debug,
    Plans,
    Capabilities,
}

impl Default for Tab {
    fn default() -> Self {
        Tab::Graph
    }
}

#[derive(Clone, Copy, PartialEq)]
enum ViewMode {
    Summary,
    Detailed,
}

impl Default for ViewMode {
    fn default() -> Self {
        ViewMode::Summary
    }
}

#[derive(Clone)]
struct IntentNode {
    intent_id: IntentId,
    name: String,
    goal: String,
    status: IntentStatus,
    children: Vec<IntentId>,
    parent: Option<IntentId>,
    created_at: u64,
    metadata: HashMap<String, String>,
}

#[derive(Clone)]
struct PlanInfo {
    plan_id: String,
    name: Option<String>,
    body: String,
    status: String,
    capabilities_required: Vec<String>,
    execution_steps: Vec<String>,
}

#[derive(Clone)]
struct CapabilityCall {
    timestamp: u64,
    capability_id: String,
    args: String,
    result: Option<String>,
    success: bool,
}

#[derive(Clone, Copy)]
enum NavDirection {
    Up,
    Down,
}

fn navigate_graph(app: &mut AppState, direction: NavDirection) {
    if app.display_tree.is_empty() {
        return;
    }

    match direction {
        NavDirection::Up => {
            if app.cursor_index > 0 {
                app.cursor_index -= 1;
            }
        }
        NavDirection::Down => {
            if app.cursor_index + 1 < app.display_tree.len() {
                app.cursor_index += 1;
            }
        }
    }
}

fn select_current_intent(app: &mut AppState) {
    if app.cursor_index < app.display_tree.len() {
        let (id, _) = &app.display_tree[app.cursor_index];
        app.selected_intent = Some(id.clone());
    }
}

fn toggle_expand_current(app: &mut AppState) {
    if app.cursor_index < app.display_tree.len() {
        let (id, _) = &app.display_tree[app.cursor_index];
        if app.expanded_nodes.contains(id) {
            app.expanded_nodes.remove(id);
        } else {
            app.expanded_nodes.insert(id.clone());
        }
        // After toggling, the display tree will be rebuilt before next draw
    }
}

fn main() -> Result<(), Box<dyn std::error::Error>> {    let args = Args::parse();

    // Use a current-thread runtime with LocalSet so we can keep non-Send parts local
    let rt = tokio::runtime::Builder::new_current_thread().enable_all().build().expect("runtime");
    let local = tokio::task::LocalSet::new();

    local.block_on(&rt, async move {

        // Terminal setup
        enable_raw_mode()?;
        let mut stdout = io::stdout();
        execute!(stdout, EnterAlternateScreen)?;
        let backend = CrosstermBackend::new(stdout);
        let mut terminal = Terminal::new(backend)?;

        // Create logs directory if it doesn't exist
        std::fs::create_dir_all("logs").unwrap_or_else(|e| {
            eprintln!("Warning: Failed to create logs directory: {}", e);
        });

        // Create a channel for debug messages
        let (debug_tx, mut debug_rx) = mpsc::channel::<String>(100);

        // Initialize CCOS + runtime service
        let debug_callback = Arc::new(move |msg: String| {
            // Get current timestamp
            let timestamp = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs();

            // Format log message with timestamp
            let log_msg = format!("[{}] {}", timestamp, msg);

            // Write to log file
            if let Ok(mut file) = std::fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open("logs/arbiter_demo.log")
            {
                let _ = writeln!(file, "{}", log_msg);
            }

            // Send debug message through the channel for TUI display
            let _ = debug_tx.try_send(msg);
        });
        println!("Initializing CCOS...");
        let ccos = Arc::new(CCOS::new_with_debug_callback(Some(debug_callback)).await.expect("init CCOS"));
        println!("CCOS initialized successfully");
        let handle = runtime_service::start_service(Arc::clone(&ccos)).await;
        println!("Runtime service started");
        let mut evt_rx = handle.subscribe();
        let cmd_tx = handle.commands();

        println!("Starting auto-start logic...");
        let mut app = AppState::default();
        let auto_start = args.goal.is_some();
        if let Some(goal) = args.goal {
            app.goal_input = goal;
        } else {
            app.goal_input = "Create a financial budget for a small business including expense categories, revenue projections, and a monthly cash flow forecast".to_string();
        }

        // Log initial configuration
        if let Ok(mut file) = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open("logs/arbiter_demo.log")
        {
            let _ = writeln!(file, "=== Arbiter RTFS Graph Demo Started ===");
            let _ = writeln!(file, "Auto-start: {}", auto_start);
            if auto_start {
                let _ = writeln!(file, "Goal: {}", app.goal_input);
            }
            let _ = writeln!(file, "Timestamp: {}", std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs());
            let _ = writeln!(file, "=====================================");
        }

        // Auto-start if goal was provided via command line
        if auto_start {
            println!("Auto-starting with goal: {}", app.goal_input);
            let ctx = runtime_service::default_controlled_context();
            let goal = app.goal_input.clone();
            if cmd_tx.try_send(runtime_service::RuntimeCommand::Start { goal: goal.clone(), context: ctx }).is_ok() {
                app.running = true;
                app.status_lines.push(format!("🚀 Auto-starting: {}", goal));
                app.intent_graph.clear();
                app.plans_by_intent.clear();
                app.root_intent_id = None;
                app.selected_intent = None;
                println!("Start command sent successfully");
            } else {
                println!("Failed to send start command");
                app.log_lines.push("❌ Queue full: cannot start".into());
            }
        }

        // Track capability calls we've already reported
        let mut reported_capability_calls = std::collections::HashSet::new();

    // Frame rate control for smooth UI updates
        let frame_sleep = std::time::Duration::from_millis(16);

        println!("Entering main event loop...");
        let res = loop {
            // 1) Drain runtime events without blocking UI
            loop {
                match evt_rx.try_recv() {
                    Ok(evt) => on_event(&mut app, evt),
                    Err(broadcast::error::TryRecvError::Empty) => break,
                    Err(broadcast::error::TryRecvError::Closed) => break,
                    Err(broadcast::error::TryRecvError::Lagged(_)) => break,
                }
            }

            // 1.5) Drain debug messages without blocking UI
            loop {
                match debug_rx.try_recv() {
                    Ok(msg) => {
                        // Handle debug message - route it to debug_lines
                        app.debug_lines.push(format!("⚙️  {}", msg));
                        if app.debug_lines.len() > 1000 { app.debug_lines.drain(0..app.debug_lines.len()-1000); }
                    }
                    Err(mpsc::error::TryRecvError::Empty) => break,
                    Err(mpsc::error::TryRecvError::Disconnected) => break,
                }
            }

            // 1.6) Check for new capability calls in the causal chain
            if let Ok(chain) = ccos.get_causal_chain().lock() {
                let actions = chain.get_all_actions();
                for action in actions {
                    if let rtfs_compiler::ccos::types::ActionType::CapabilityCall = action.action_type {
                        let call_key = format!("{}-{}", action.action_id, action.function_name.as_deref().unwrap_or("unknown"));
                        if !reported_capability_calls.contains(&call_key) {
                            reported_capability_calls.insert(call_key);
                            
                            // Extract capability arguments
                            let args_str = if let Some(args) = &action.arguments {
                                format!("{:?}", args)
                            } else {
                                "no args".to_string()
                            };
                            
                            // Store capability call information
                            let call = CapabilityCall {
                                timestamp: std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_secs(),
                                capability_id: action.function_name.clone().unwrap_or_else(|| "unknown".to_string()),
                                args: args_str.clone(),
                                result: action.result.as_ref().map(|r| format!("{:?}", r.value)),
                                success: action.result.as_ref().map(|r| r.success).unwrap_or(false),
                            };
                            app.capability_calls.push(call);
                            
                            app.log_lines.push(format!("⚙️ Capability call: {}({})", 
                                action.function_name.as_deref().unwrap_or("unknown"), args_str));
                            if app.log_lines.len() > 500 { app.log_lines.drain(0..app.log_lines.len()-500); }
                        }
                    }
                }
            }

            // 1.7) Rebuild display tree according to current expansion state
            rebuild_display_tree(&mut app);

            // Clamp cursor if tree shrank
            if !app.display_tree.is_empty() && app.cursor_index >= app.display_tree.len() {
                app.cursor_index = app.display_tree.len() - 1;
            }

            // 2) Draw UI
            terminal.draw(|f| ui(f, &app))?;

            // 3) Handle input without blocking the async scheduler
            if crossterm::event::poll(std::time::Duration::from_millis(0))? {
                if let CEvent::Key(key) = event::read()? {
                    match (key.code, key.modifiers) {
                        (KeyCode::Char('q'), _) => {
                            // Send shutdown best-effort and exit
                            let _ = cmd_tx.try_send(runtime_service::RuntimeCommand::Shutdown);
                            break Ok(());
                        }
                        (KeyCode::Char('s'), _) => {
                            // Start with current goal
                            let ctx = runtime_service::default_controlled_context();
                            let goal = app.goal_input.clone();
                            if cmd_tx.try_send(runtime_service::RuntimeCommand::Start { goal: goal.clone(), context: ctx }).is_ok() {
                                app.running = true;
                                app.status_lines.push(format!("🚀 Starting: {}", goal));
                                app.intent_graph.clear();
                                app.plans_by_intent.clear();
                                app.root_intent_id = None;
                                app.selected_intent = None;
                            } else {
                                app.log_lines.push("❌ Queue full: cannot start".into());
                            }
                        }
                        (KeyCode::Char('c'), _) => {
                            if let Some(id) = app.current_intent.clone() {
                                let _ = cmd_tx.try_send(runtime_service::RuntimeCommand::Cancel { intent_id: id });
                                app.log_lines.push("🛑 Cancel requested".into());
                            } else {
                                app.log_lines.push("ℹ️  No intent to cancel".into());
                            }
                        }
                        (KeyCode::Char('r'), _) => {
                            // Reset/clear everything
                            app.intent_graph.clear();
                            app.plans_by_intent.clear();
                            app.root_intent_id = None;
                            app.selected_intent = None;
                            app.current_intent = None;
                            app.running = false;
                            app.last_result = None;
                            app.status_lines.clear();
                            app.log_lines.clear();
                            app.log_lines.push("🔄 Reset complete".into());
                        }
                        (KeyCode::Char('1'), _) => { app.current_tab = Tab::Graph; }
                        (KeyCode::Char('2'), _) => { app.current_tab = Tab::Status; }
                        (KeyCode::Char('3'), _) => { app.current_tab = Tab::Logs; }
                        (KeyCode::Char('4'), _) => { app.current_tab = Tab::Debug; }
                        (KeyCode::Char('5'), _) => { app.current_tab = Tab::Plans; }
                        (KeyCode::Char('6'), _) => { app.current_tab = Tab::Capabilities; }
                        (KeyCode::Char('d'), KeyModifiers::CONTROL) => { app.show_debug = !app.show_debug; }
                        (KeyCode::F(1), _) | (KeyCode::Char('?'), _) => { app.help_visible = !app.help_visible; }
                        (KeyCode::Up, _) => { navigate_graph(&mut app, NavDirection::Up); }
                        (KeyCode::Down, _) => { navigate_graph(&mut app, NavDirection::Down); }
                        (KeyCode::Enter, _) => { select_current_intent(&mut app); }
                        (KeyCode::Char(' '), _) => { toggle_expand_current(&mut app); }
                        (KeyCode::Backspace, _) => { app.goal_input.pop(); }
                        (KeyCode::Char(ch), KeyModifiers::NONE) => { app.goal_input.push(ch); }
                        (KeyCode::Char(ch), KeyModifiers::SHIFT) => { app.goal_input.push(ch); }
                        _ => {}
                    }
                }
            }

            // Yield to Tokio so spawn_local tasks can progress (important on current-thread runtime)
            tokio::time::sleep(frame_sleep).await;
        };

        // Cleanup
        disable_raw_mode()?;
        execute!(terminal.backend_mut(), LeaveAlternateScreen)?;
        terminal.show_cursor()?;

        res
    })
}

fn on_event(app: &mut AppState, evt: runtime_service::RuntimeEvent) {
    use runtime_service::RuntimeEvent as E;
    match evt {
        E::Started { intent_id, goal } => {
            app.current_intent = Some(intent_id.clone());
            app.running = true;
            app.log_lines.push(format!("🎯 Started: {}", goal));

            // Create root intent node
            let root_node = IntentNode {
                intent_id: intent_id.clone(),
                name: "Root Goal".to_string(),
                goal,
                status: IntentStatus::Active,
                children: vec![],
                parent: None,
                created_at: std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_secs(),
                metadata: HashMap::new(),
            };
                let root_id = intent_id.clone();
                app.intent_graph.insert(root_id.clone(), root_node);
                app.root_intent_id = Some(root_id.clone());
                // Expand root initially so children are visible; user can collapse with Space
                app.expanded_nodes.insert(root_id);
        }
        E::Status { intent_id, status } => {
            app.status_lines.push(status.clone());
            if app.status_lines.len() > 200 { app.status_lines.drain(0..app.status_lines.len()-200); }

            // Update intent status in graph
            if let Some(node) = app.intent_graph.get_mut(&intent_id) {
                // Parse status to update IntentStatus
                if status.contains("Executing") {
                    node.status = IntentStatus::Executing;
                } else if status.contains("Completed") {
                    node.status = IntentStatus::Completed;
                } else if status.contains("Failed") {
                    node.status = IntentStatus::Failed;
                }
            }
        }
        E::Step { intent_id: _, desc } => {
            app.log_lines.push(format!("⚙️  {}", desc));
            if app.log_lines.len() > 500 { app.log_lines.drain(0..app.log_lines.len()-500); }
        }
        E::Result { intent_id, result } => {
            app.running = false;
            app.last_result = Some(format!("✅ Result: {:?}", result));
            app.log_lines.push("🏁 Execution completed".into());

            // Update final status
            if let Some(node) = app.intent_graph.get_mut(&intent_id) {
                // Parse result to determine success - if it starts with "Error:", it's failed
                let success = !result.starts_with("Error:");
                node.status = if success { IntentStatus::Completed } else { IntentStatus::Failed };
            }
        }
        E::Error { message } => {
            app.running = false;
            app.log_lines.push(format!("❌ Error: {}", message));
        }
        E::Heartbeat => {}
        E::Stopped => {
            app.running = false;
            app.log_lines.push("⏹️  Stopped".into());
        }
    }
}

fn ui(f: &mut ratatui::Frame<'_>, app: &AppState) {
    let size = f.size();

    // Create tabs at the top
    let tabs = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1), // tabs
            Constraint::Length(3), // input
            Constraint::Min(5),    // main content
            Constraint::Length(1), // status bar
        ])
        .split(size);

    // Tab bar
    let tab_titles = vec!["1:Graph", "2:Status", "3:Logs", "4:Debug", "5:Plans", "6:Capabilities"];
    let tab_block = Block::default()
        .borders(Borders::TOP | Borders::LEFT | Borders::RIGHT)
        .title("Tabs • Ctrl+D:Toggle Debug • ?:Help");

    let tab_items: Vec<ListItem> = tab_titles
        .iter()
        .enumerate()
        .map(|(i, &title)| {
            let style = match (app.current_tab, i) {
                (Tab::Graph, 0) | (Tab::Status, 1) | (Tab::Logs, 2) | (Tab::Debug, 3) | (Tab::Plans, 4) | (Tab::Capabilities, 5) => {
                    Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)
                }
                _ => Style::default().fg(Color::White),
            };
            ListItem::new(title).style(style)
        })
        .collect();

    let tab_list = List::new(tab_items).block(tab_block);
    f.render_widget(tab_list, tabs[0]);

    // Goal input
    let input_title = match app.current_tab {
        Tab::Graph => "🎯 Goal Input (type) • s=Start c=Cancel r=Reset q=Quit",
        Tab::Status => "📊 Status View",
        Tab::Logs => "📝 Application Logs",
        Tab::Debug => "🔧 Debug Logs",
        Tab::Plans => "📋 Plan Details",
        Tab::Capabilities => "⚙️ Capability Calls",
    };

    let input = Paragraph::new(if matches!(app.current_tab, Tab::Graph) { app.goal_input.as_str() } else { "" })
        .block(Block::default().title(input_title).borders(Borders::ALL))
        .wrap(Wrap { trim: true });
    f.render_widget(input, tabs[1]);

    // Main content based on current tab
    match app.current_tab {
    Tab::Graph => render_graph_tab(f, app, tabs[2]),
        Tab::Status => render_status_tab(f, app, tabs[2]),
        Tab::Logs => render_logs_tab(f, app, tabs[2]),
        Tab::Debug => render_debug_tab(f, app, tabs[2]),
        Tab::Plans => render_plans_tab(f, app, tabs[2]),
        Tab::Capabilities => render_capabilities_tab(f, app, tabs[2]),
    }

    // Status bar
    let status_text = format!(
        "Intent: {} | Status: {} | Debug: {} | Tab: {}",
        app.current_intent.as_deref().unwrap_or("None"),
        if app.running { "Running" } else { "Idle" },
        if app.show_debug { "Visible" } else { "Hidden" },
        match app.current_tab {
            Tab::Graph => "Graph",
            Tab::Status => "Status",
            Tab::Logs => "Logs",
            Tab::Debug => "Debug",
            Tab::Plans => "Plans",
            Tab::Capabilities => "Capabilities",
        }
    );
    let status_bar = Paragraph::new(status_text)
        .style(Style::default().fg(Color::Cyan))
        .block(Block::default().borders(Borders::TOP));
    f.render_widget(status_bar, tabs[3]);

        // Help overlay
    if app.help_visible {
        render_help_overlay(f, size);
    }
}

fn render_graph_tab(f: &mut ratatui::Frame<'_>, app: &AppState, area: Rect) {
    let chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(60), Constraint::Percentage(40)])
        .split(area);

    // Intent graph visualization with selection (based on flattened display_tree)
    let mut graph_items: Vec<ListItem> = Vec::new();
    if app.display_tree.is_empty() {
        graph_items.push(ListItem::new("No root intent yet".to_string()));
    } else {
        for (idx, (id, depth)) in app.display_tree.iter().enumerate() {
            if let Some(node) = app.intent_graph.get(id) {
                let indent = "  ".repeat(*depth);
                let is_expanded = app.expanded_nodes.contains(id) || *depth == 0;
                let status_emoji = match node.status {
                    IntentStatus::Active => "🟡",
                    IntentStatus::Executing => "🔵",
                    IntentStatus::Completed => "✅",
                    IntentStatus::Failed => "❌",
                    IntentStatus::Archived => "📦",
                    IntentStatus::Suspended => "⏸️",
                };
        
                let expand_indicator = if !node.children.is_empty() {
                    if is_expanded { "▼" } else { "▶" }
                } else { "  " };

                let display_name = if node.name.is_empty() { "<unnamed>".to_string() } else { node.name.clone() };
                let goal_preview = if node.goal.len() > 30 {
                    format!("{}...", &node.goal[..27])
                } else {
                    node.goal.clone()
                };

                let mut style = Style::default();
                if idx == app.cursor_index { style = style.fg(Color::Cyan).add_modifier(Modifier::BOLD); }
                graph_items.push(ListItem::new(format!("{}{}{}[{:?}] {} — {}", indent, expand_indicator, status_emoji, node.status, display_name, goal_preview)).style(style));
            }
        }
    }

    let graph = List::new(graph_items)
        .block(Block::default().title("🗺️  Intent Graph • ↑↓:Navigate • Enter:Select • Space:Expand").borders(Borders::ALL))
        .highlight_style(Style::default().add_modifier(Modifier::BOLD).fg(Color::Cyan));
    f.render_widget(graph, chunks[0]);

    // Detailed intent information
    let detail_text = if let Some(selected_id) = &app.selected_intent {
        if let Some(node) = app.intent_graph.get(selected_id) {
            let plan_info = app.plans_by_intent.get(selected_id);
            format!("🎯 Intent Details:\nID: {}\nName: {}\nGoal: {}\nStatus: {:?}\nCreated: {}\n\n📋 Plan Info:\n{}",
                node.intent_id,
                node.name,
                node.goal,
                node.status,
                node.created_at,
                plan_info.map(|p| format!("Capabilities: {}\nStatus: {}\nSteps: {}", 
                    p.capabilities_required.join(", "), p.status, p.execution_steps.len()))
                    .unwrap_or("No plan information".to_string())
            )
        } else {
            "Selected intent not found".to_string()
        }
    } else {
    "Select an intent to view details\n\nUse ↑↓ to navigate\nEnter to select\nSpace to expand/collapse".to_string()
    };

    let details = Paragraph::new(detail_text)
        .style(Style::default().fg(Color::White))
        .block(Block::default().title("📋 Intent Details").borders(Borders::ALL))
        .wrap(Wrap { trim: true });
    f.render_widget(details, chunks[1]);
}

fn render_status_tab(f: &mut ratatui::Frame<'_>, app: &AppState, area: Rect) {
    let status_items: Vec<ListItem> = app.status_lines.iter().rev().take(100).map(|s| ListItem::new(s.clone())).collect();
    let status = List::new(status_items)
        .block(Block::default().title("📊 Status Updates").borders(Borders::ALL));
    f.render_widget(status, area);
}

fn render_logs_tab(f: &mut ratatui::Frame<'_>, app: &AppState, area: Rect) {
    let log_items: Vec<ListItem> = app.log_lines.iter().rev().take(200).map(|s| ListItem::new(s.clone())).collect();
    let log = List::new(log_items)
        .block(Block::default().title("📝 Application Logs").borders(Borders::ALL));
    f.render_widget(log, area);
}

fn render_debug_tab(f: &mut ratatui::Frame<'_>, app: &AppState, area: Rect) {
    let debug_items: Vec<ListItem> = app.debug_lines.iter().rev().take(200).map(|s| ListItem::new(s.clone())).collect();
    let debug = List::new(debug_items)
        .block(Block::default().title("🔧 Debug Logs").borders(Borders::ALL));
    f.render_widget(debug, area);
}

fn render_plans_tab(f: &mut ratatui::Frame<'_>, app: &AppState, area: Rect) {
    let plan_items: Vec<ListItem> = if let Some(selected_id) = &app.selected_intent {
        if let Some(plan_info) = app.plans_by_intent.get(selected_id) {
            vec![
                ListItem::new(format!("📋 Plan ID: {}", plan_info.plan_id)),
                ListItem::new(format!("📝 Name: {}", plan_info.name.as_deref().unwrap_or("<unnamed>"))),
                ListItem::new(format!("📊 Status: {}", plan_info.status)),
                ListItem::new(format!("⚙️ Capabilities: {}", plan_info.capabilities_required.join(", "))),
                ListItem::new("📄 Plan Body:".to_string()),
            ].into_iter().chain(
                plan_info.body.lines().map(|line| ListItem::new(format!("  {}", line)))
            ).chain(
                plan_info.execution_steps.iter().map(|step| ListItem::new(format!("▶️ {}", step)))
            ).collect()
        } else {
            vec![ListItem::new("No plan selected or available".to_string())]
        }
    } else {
        vec![ListItem::new("Select an intent to view its plan".to_string())]
    };

    let plans = List::new(plan_items)
        .block(Block::default().title("📋 Plan Details").borders(Borders::ALL));
    f.render_widget(plans, area);
}

fn render_capabilities_tab(f: &mut ratatui::Frame<'_>, app: &AppState, area: Rect) {
    let cap_items: Vec<ListItem> = if app.capability_calls.is_empty() {
        vec![ListItem::new("No capability calls recorded yet".to_string())]
    } else {
        app.capability_calls.iter().rev().take(50).map(|call| {
            let status = if call.success { "✅" } else { "❌" };
            let result = call.result.as_deref().unwrap_or("pending");
            ListItem::new(format!("{} {}({}) → {}", status, call.capability_id, call.args, result))
        }).collect()
    };

    let capabilities = List::new(cap_items)
        .block(Block::default().title("⚙️ Capability Calls").borders(Borders::ALL));
    f.render_widget(capabilities, area);
}

fn render_help_overlay(f: &mut ratatui::Frame<'_>, size: Rect) {
    let help_text = "
🚀 Arbiter TUI Demo - Help

Navigation:
  1-4     Switch between tabs (Graph/Status/Logs/Debug)
  Tab     Cycle through tabs
  Ctrl+D  Toggle debug log visibility
  ?/F1    Show/hide this help

Actions:
  s       Start execution with current goal
  c       Cancel current execution
  r       Reset everything
  q       Quit application

Input:
  Type    Edit goal text
  Backspace Delete character

Tabs:
  Graph   Intent graph visualization and results
  Status  Real-time execution status updates
  Logs    Application logs (non-debug)
  Debug   Debug logs and detailed traces

Press ? or F1 to close this help.
";

    let help = Paragraph::new(help_text)
        .style(Style::default().fg(Color::White).bg(Color::Black))
        .block(Block::default().title("❓ Help").borders(Borders::ALL))
        .wrap(Wrap { trim: true });

    let help_area = centered_rect(60, 80, size);
    f.render_widget(Clear, help_area);
    f.render_widget(help, help_area);
}

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

// Rebuild the visible, flattened display tree and keep it on app
fn rebuild_display_tree(app: &mut AppState) {
    app.display_tree.clear();
    if let Some(root_id) = &app.root_intent_id {
        flatten_visible(&app.intent_graph, root_id, 0, &app.expanded_nodes, &mut app.display_tree);
    }
}

fn flatten_visible(
    graph: &HashMap<IntentId, IntentNode>,
    current_id: &IntentId,
    depth: usize,
    expanded: &HashSet<IntentId>,
    out: &mut Vec<(IntentId, usize)>,
) {
    if let Some(node) = graph.get(current_id) {
        out.push((current_id.clone(), depth));
    let is_expanded = expanded.contains(current_id);
        if is_expanded {
            for child in &node.children {
                flatten_visible(graph, child, depth + 1, expanded, out);
            }
        }
    }
}