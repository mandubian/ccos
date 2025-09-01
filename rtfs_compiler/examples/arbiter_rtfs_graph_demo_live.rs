//! Live Arbiter RTFS Graph Generation TUI Demo (LLM-backed)
//!
//! This example is a safe copy of `arbiter_rtfs_graph_demo.rs` with added
//! minimal bindings to the LLM-backed DelegatingArbiter so the user can
//! Generate Graphs from a goal (key: g), then later generate plans (p) and
//! execute them (e). For now this first iteration implements Generate Graph.

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
use serde_json;

use rtfs_compiler::ccos::{CCOS, runtime_service, types::{IntentId, IntentStatus, Plan, PlanBody}};
use rtfs_compiler::ccos::arbiter::arbiter_engine::ArbiterEngine;

#[derive(Parser)]
struct Args {
    #[arg(long, help = "Initial goal to load (optional)")]
    goal: Option<String>,
    #[arg(long, help = "Run in headless mode: emit JSON messages to stdout and exit")]
    headless: bool,
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
    // Visible display order for the graph list (top-to-bottom)
    display_order: Vec<IntentId>,
    view_mode: ViewMode,
    selected_intent_index: usize,
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

impl Default for Tab { fn default() -> Self { Tab::Graph } }

#[derive(Clone, Copy, PartialEq)]
enum ViewMode { Summary, Detailed }
impl Default for ViewMode { fn default() -> Self { ViewMode::Summary } }

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
enum NavDirection { Up, Down }

fn navigate_graph(app: &mut AppState, direction: NavDirection) {
    if app.display_order.is_empty() { return; }
    match direction {
        NavDirection::Up => { if app.selected_intent_index > 0 { app.selected_intent_index -= 1; } }
        NavDirection::Down => { if app.selected_intent_index < app.display_order.len() - 1 { app.selected_intent_index += 1; } }
    }
}

fn select_current_intent(app: &mut AppState) {
    if app.selected_intent_index < app.display_order.len() {
        if let Some(intent_id) = app.display_order.get(app.selected_intent_index) {
            app.selected_intent = Some(intent_id.clone());
        }
    }
}

fn toggle_expand_current(app: &mut AppState) {
    if app.selected_intent_index < app.display_order.len() {
        if let Some(intent_id) = app.display_order.get(app.selected_intent_index) {
            let intent_id = intent_id.clone();
            if app.expanded_nodes.contains(&intent_id) { app.expanded_nodes.remove(&intent_id); } else { app.expanded_nodes.insert(intent_id); }
        }
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
        std::fs::create_dir_all("logs").unwrap_or_else(|e| { eprintln!("Warning: Failed to create logs directory: {}", e); });

    // Create a channel for debug messages (we now send compact JSON strings)
    let (debug_tx, mut debug_rx) = mpsc::channel::<String>(100);

        // Initialize CCOS + runtime service
    let debug_callback = Arc::new(move |msg: String| {
            // log with timestamp and forward the raw JSON message into channel
            let timestamp = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap_or_default().as_secs();
            let log_msg = format!("[{}] {}", timestamp, msg);
            if let Ok(mut file) = std::fs::OpenOptions::new().create(true).append(true).open("logs/arbiter_demo_live.log") {
                let _ = writeln!(file, "{}", log_msg);
            }
            let _ = debug_tx.try_send(msg);
        });
    let debug_callback_for_ccos = debug_callback.clone();
    let ccos = Arc::new(CCOS::new_with_debug_callback(Some(debug_callback_for_ccos)).await.expect("init CCOS"));
        let handle = runtime_service::start_service(Arc::clone(&ccos)).await;
        let mut evt_rx = handle.subscribe();
        let cmd_tx = handle.commands();

        let mut app = AppState::default();
        let auto_start = args.goal.is_some();
        if let Some(goal) = args.goal { app.goal_input = goal; } else { app.goal_input = "Create a financial budget for a small business including expense categories, revenue projections, and a monthly cash flow forecast".to_string(); }

        // If headless flag is set, run a short non-interactive demo and exit
    if args.headless {
            let goal = app.goal_input.clone();
            app.log_lines.push(format!("🔬 Headless run: {}", goal));
            // Try to get a delegating arbiter
            if let Some(arb) = ccos.get_delegating_arbiter() {
                // Request graph
                match arb.natural_language_to_graph(&goal).await {
                    Ok(root_id) => {
                        // Emit GRAPH_ROOT JSON to stdout
                        let msg = serde_json::json!({"type":"GRAPH_ROOT","intent_id": root_id});
                        println!("{}", msg.to_string());

                        // Read stored intents and pick one to generate a plan for
                        if let Ok(graph_lock) = ccos.get_intent_graph().lock() {
                            let all = graph_lock.storage.get_all_intents_sync();
                            if let Some(st) = all.get(0) {
                                match arb.generate_plan_for_intent(st).await {
                                    Ok(res) => {
                                        let body = match res.plan.body {
                                            rtfs_compiler::ccos::types::PlanBody::Rtfs(ref s) => s.clone(),
                                            _ => "".to_string(),
                                        };
                                        let msg = serde_json::json!({"type":"PLAN_GEN","intent_id": st.intent_id, "plan_id": res.plan.plan_id, "body": body});
                                        println!("{}", msg.to_string());
                                    }
                                    Err(e) => {
                                        // Fallback: try intent_to_plan which returns a Plan (if implemented)
                                        // Fallback: construct a minimal Intent from the stored intent and call intent_to_plan
                                        let intent_obj = rtfs_compiler::ccos::types::Intent::new(st.goal.clone());
                                        match arb.intent_to_plan(&intent_obj).await {
                                            Ok(plan) => {
                                                let body = match plan.body {
                                                    rtfs_compiler::ccos::types::PlanBody::Rtfs(s) => s,
                                                    _ => "".to_string(),
                                                };
                                                let plan_id = plan.plan_id.clone();
                                                let msg = serde_json::json!({"type":"PLAN_GEN","intent_id": st.intent_id, "plan_id": plan_id, "body": body});
                                                println!("{}", msg.to_string());
                                            }
                                            Err(e2) => {
                                                let msg = serde_json::json!({"type":"PLAN_GEN_ERR","intent_id": st.intent_id, "error": format!("{} / fallback: {}", e, e2)});
                                                println!("{}", msg.to_string());
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    }
                    Err(e) => {
                        let msg = serde_json::json!({"type":"GRAPH_ROOT_ERR","error": format!("{}", e)});
                        println!("{}", msg.to_string());
                    }
                }
            } else {
                eprintln!("No delegating arbiter available (LLM not enabled in config)");
            }
            return Ok(());
        }

        // Auto-start if goal was provided via command line
        if auto_start {
            let ctx = runtime_service::default_controlled_context();
            let goal = app.goal_input.clone();
            if cmd_tx.try_send(runtime_service::RuntimeCommand::Start { goal: goal.clone(), context: ctx }).is_ok() {
                app.running = true;
                app.status_lines.push(format!("🚀 Auto-starting: {}", goal));
                app.intent_graph.clear();
                app.plans_by_intent.clear();
                app.root_intent_id = None;
                app.selected_intent = None;
            } else {
                app.log_lines.push("❌ Queue full: cannot start".into());
            }
        }

        let mut reported_capability_calls = std::collections::HashSet::new();
        let frame_sleep = std::time::Duration::from_millis(16);

        let res = loop {
            // Drain runtime events
            loop { match evt_rx.try_recv() { Ok(evt) => on_event(&mut app, evt), Err(broadcast::error::TryRecvError::Empty) => break, Err(broadcast::error::TryRecvError::Closed) => break, Err(broadcast::error::TryRecvError::Lagged(_)) => break, } }

                // Drain debug messages and handle special structured messages coming from background tasks
                loop {
                    match debug_rx.try_recv() {
                        Ok(msg) => {
                            // Keep raw debug log
                            app.debug_lines.push(format!("⚙️  {}", msg));
                            if app.debug_lines.len() > 1000 { app.debug_lines.drain(0..app.debug_lines.len()-1000); }

                            // Messages are compact JSON objects; try to parse
                            if let Ok(v) = serde_json::from_str::<serde_json::Value>(&msg) {
                                if let Some(t) = v.get("type").and_then(|x| x.as_str()) {
                                    match t {
                                        "GRAPH_ROOT" => {
                                            if let Some(root_id) = v.get("intent_id").and_then(|x| x.as_str()) {
                                                let root_id = root_id.to_string();
                                // Populate intent_graph from CCOS's stored intents
                                                if let Ok(graph_lock) = ccos.get_intent_graph().lock() {
                                                    let all = graph_lock.storage.get_all_intents_sync();
                                                    app.intent_graph.clear();
                                                    for st in all {
                                                        // Query authoritative children for this intent from the graph API
                                                        let child_sts = graph_lock.get_child_intents(&st.intent_id);
                                                        let child_ids: Vec<IntentId> = child_sts.into_iter().map(|c| c.intent_id).collect();
                                                        let node = IntentNode {
                                                            intent_id: st.intent_id.clone(),
                                                            name: st.name.clone().unwrap_or_else(|| "<unnamed>".to_string()),
                                                            goal: st.goal.clone(),
                                                            status: st.status.clone(),
                                                            children: child_ids,
                                                            parent: st.parent_intent.clone(),
                                                            created_at: st.created_at,
                                                            metadata: st.metadata.clone(),
                                                        };
                                                        app.intent_graph.insert(st.intent_id.clone(), node);
                                                    }
                                                    // After populating the in-memory intent graph, emit a compact debug JSON
                                                    // so we can inspect shapes from the TUI when nodes are missing.
                                                    // Build a sample of keys (truncate to first 20) to avoid huge logs.
                                                    let node_count = app.intent_graph.len();
                                                    let node_keys: Vec<String> = app.intent_graph.keys().cloned().take(20).collect();
                                                    let root_children_sample: Vec<IntentId> = match app.intent_graph.get(&root_id) {
                                                        Some(root_node) => root_node.children.clone(),
                                                        None => Vec::new(),
                                                    };
                                                    // Also query the authoritative graph API for children to detect
                                                    // whether edges exist even when storable.child_intents is empty.
                                                    // Use the existing `graph_lock` to avoid attempting to lock the
                                                    // same mutex twice which can deadlock in the current-thread runtime.
                                                    let mut root_children_via_api: Vec<IntentId> = Vec::new();
                                                    let children = graph_lock.get_child_intents(&root_id);
                                                    root_children_via_api = children.into_iter().map(|c| c.intent_id).collect();
                                                    // Set root and selection for display
                                                    app.root_intent_id = Some(root_id.clone());
                                                    app.selected_intent = app.root_intent_id.clone();
                                                    if let Some(r) = &app.root_intent_id { app.expanded_nodes.insert(r.clone()); }
                                                    app.log_lines.push(format!("🧭 Graph populated: {} nodes", node_count));

                                                    // Emit structured debug JSON via the debug callback so the background
                                                    // log file `logs/arbiter_demo_live.log` will contain this message.
                                                    if let Some(dbg_cb) = Some(debug_callback.clone()) {
                                                        let dbg_msg = serde_json::json!({
                                                            "type": "GRAPH_ROOT_POPULATED",
                                                            "root_id": root_id,
                                                            "node_count": node_count,
                                                            "keys_sample": node_keys,
                                                            "root_children": root_children_sample,
                                                            "root_children_via_api": root_children_via_api
                                                        });
                                                        let _ = (dbg_cb)(dbg_msg.to_string());
                                                    }
                                                }
                                            }
                                        }
                                        "PLAN_GEN" => {
                                            if let Some(intent_id) = v.get("intent_id").and_then(|x| x.as_str()) {
                                                let plan_id = v.get("plan_id").and_then(|x| x.as_str()).unwrap_or("<unknown>").to_string();
                                                let body_text = v.get("body").and_then(|x| x.as_str()).unwrap_or("").to_string();
                                                // Unescape any escaped newlines that were encoded by the background task
                                                let body_unescaped = body_text.replace("\\n", "\n");
                                                let plan_info = PlanInfo {
                                                    plan_id: plan_id.clone(),
                                                    name: None,
                                                    body: body_unescaped.clone(),
                                                    status: "Generated".to_string(),
                                                    capabilities_required: vec![],
                                                    execution_steps: vec![],
                                                };
                                                app.plans_by_intent.insert(intent_id.to_string(), plan_info);
                                                app.log_lines.push(format!("📋 Plan generated for {}: {}", intent_id, plan_id));
                                            }
                                        }
                                        "PLAN_GEN_ERR" => {
                                            if let Some(intent_id) = v.get("intent_id").and_then(|x| x.as_str()) {
                                                let err = v.get("error").and_then(|x| x.as_str()).unwrap_or("<err>");
                                                app.log_lines.push(format!("❌ Plan generation error for {}: {}", intent_id, err));
                                            }
                                        }
                                        "EXEC_RESULT" => {
                                            if let Some(intent_id) = v.get("intent_id").and_then(|x| x.as_str()) {
                                                let success = v.get("success").and_then(|x| x.as_bool()).unwrap_or(false);
                                                let value = v.get("value").map(|x| x.to_string()).unwrap_or_else(|| "null".to_string());
                                                app.last_result = Some(format!("success={} value={}", success, value));
                                                app.log_lines.push(format!("🏁 Exec result for {}: success={}", intent_id, success));
                                            }
                                        }
                                        _ => {}
                                    }
                                }
                            } else {
                                // Could not parse JSON: keep raw line in debug
                            }
                        }
                        Err(mpsc::error::TryRecvError::Empty) => break,
                        Err(mpsc::error::TryRecvError::Disconnected) => break,
                    }
                }

            // Check causal chain for capability calls
            if let Ok(chain) = ccos.get_causal_chain().lock() {
                let actions = chain.get_all_actions();
                for action in actions {
                    if let rtfs_compiler::ccos::types::ActionType::CapabilityCall = action.action_type {
                        let call_key = format!("{}-{}", action.action_id, action.function_name.as_deref().unwrap_or("unknown"));
                        if !reported_capability_calls.contains(&call_key) {
                            reported_capability_calls.insert(call_key);
                            let args_str = if let Some(args) = &action.arguments { format!("{:?}", args) } else { "no args".to_string() };
                            let call = CapabilityCall { timestamp: std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_secs(), capability_id: action.function_name.clone().unwrap_or_else(|| "unknown".to_string()), args: args_str.clone(), result: action.result.as_ref().map(|r| format!("{:?}", r.value)), success: action.result.as_ref().map(|r| r.success).unwrap_or(false), };
                            app.capability_calls.push(call);
                            app.log_lines.push(format!("⚙️ Capability call: {}({})", action.function_name.as_deref().unwrap_or("unknown"), args_str));
                            if app.log_lines.len() > 500 { app.log_lines.drain(0..app.log_lines.len()-500); }
                        }
                    }
                }
            }

            // Draw UI
            terminal.draw(|f| ui(f, &mut app))?;

            // Handle input
            if crossterm::event::poll(std::time::Duration::from_millis(0))? {
                if let CEvent::Key(key) = event::read()? {
                    match (key.code, key.modifiers) {
                        (KeyCode::Char('q'), _) => { let _ = cmd_tx.try_send(runtime_service::RuntimeCommand::Shutdown); break Ok(()); }
                        (KeyCode::Char('g'), _) => {
                            // Generate graph via DelegatingArbiter (LLM)
                            if let Some(_arb) = ccos.get_delegating_arbiter() {
                                let goal = app.goal_input.clone();
                                // spawn_local to avoid blocking; clone debug callback for the closure
                                let dbg = debug_callback.clone();
                                let ccos_clone = Arc::clone(&ccos);
                                tokio::task::spawn_local(async move {
                                    if let Some(arb) = ccos_clone.get_delegating_arbiter() {
                                        match arb.natural_language_to_graph(&goal).await {
                                        Ok(root_id) => {
                                            let msg = serde_json::json!({"type":"GRAPH_ROOT","intent_id": root_id});
                                            let _ = (dbg)(msg.to_string());
                                        }
                                        Err(e) => {
                                            let msg = serde_json::json!({"type":"GRAPH_ROOT_ERR","error": format!("{}", e)});
                                            let _ = (dbg)(msg.to_string());
                                        }
                                        }
                                    }
                                });
                                app.log_lines.push("🧭 Graph generation requested (LLM)".into());
                            } else {
                                app.log_lines.push("⚠️  No delegating arbiter available (LLM not enabled in config)".into());
                            }
                        }
                        (KeyCode::Char('s'), _) => { let ctx = runtime_service::default_controlled_context(); let goal = app.goal_input.clone(); if cmd_tx.try_send(runtime_service::RuntimeCommand::Start { goal: goal.clone(), context: ctx }).is_ok() { app.running = true; app.status_lines.push(format!("🚀 Starting: {}", goal)); app.intent_graph.clear(); app.plans_by_intent.clear(); app.root_intent_id = None; app.selected_intent = None; } else { app.log_lines.push("❌ Queue full: cannot start".into()); } }
                        (KeyCode::Char('p'), _) => {
                            // Generate plan for selected intent via LLM-backed delegating arbiter
                            if let Some(_arb) = ccos.get_delegating_arbiter() {
                                if let Some(selected) = app.selected_intent.clone() {
                                    let maybe_intent = {
                                        if let Ok(graph_lock) = ccos.get_intent_graph().lock() {
                                            graph_lock.get_intent(&selected)
                                        } else { None }
                                    };
                                    if let Some(storable) = maybe_intent {
                                        // spawn_local to call async non-Send method
                                        let dbg = debug_callback.clone();
                                        let arb_clone = _arb.clone();
                                        tokio::task::spawn_local(async move {
                                                            match arb_clone.generate_plan_for_intent(&storable).await {
                                                    Ok(result) => {
                                                        // Send a structured debug message including plan body (escape newlines)
                                                        let body = match &result.plan.body {
                                                            PlanBody::Rtfs(txt) => txt.clone(),
                                                            _ => "<non-RTFS plan>".to_string(),
                                                        };
                                                        let msg = serde_json::json!({"type":"PLAN_GEN","intent_id": storable.intent_id, "plan_id": result.plan.plan_id, "body": body.replace('\n', "\\n")});
                                                        let _ = (dbg)(msg.to_string());
                                                    }
                                                    Err(e) => {
                                                        // Fallback to intent_to_plan if available
                                                        // Fallback: build a minimal Intent and call intent_to_plan
                                                        let intent_obj = rtfs_compiler::ccos::types::Intent::new(storable.goal.clone());
                                                        match arb_clone.intent_to_plan(&intent_obj).await {
                                                            Ok(plan) => {
                                                                let body = match plan.body {
                                                                    rtfs_compiler::ccos::types::PlanBody::Rtfs(s) => s,
                                                                    _ => "".to_string(),
                                                                };
                                                                let msg = serde_json::json!({"type":"PLAN_GEN","intent_id": storable.intent_id, "plan_id": plan.plan_id, "body": body.replace('\n', "\\n")});
                                                                let _ = (dbg)(msg.to_string());
                                                            }
                                                            Err(e2) => {
                                                                let msg = serde_json::json!({"type":"PLAN_GEN_ERR","intent_id": storable.intent_id, "error": format!("{} / fallback: {}", e, e2)});
                                                                let _ = (dbg)(msg.to_string());
                                                            }
                                                        }
                                                    }
                                                }
                                        });
                                        app.log_lines.push(format!("📡 Plan generation requested for {}", selected));
                                    } else { app.log_lines.push("⚠️  Selected intent not found in graph".into()); }
                                } else { app.log_lines.push("ℹ️  No intent selected".into()); }
                            } else {
                                app.log_lines.push("⚠️  No delegating arbiter available (LLM not enabled in config)".into());
                            }
                        }
                        (KeyCode::Char('e'), _) => {
                            // Execute selected plan (if any) via delegating arbiter execute_plan
                            if let Some(_arb) = ccos.get_delegating_arbiter() {
                                if let Some(selected) = app.selected_intent.clone() {
                                        if let Some(plan_info) = app.plans_by_intent.get(&selected) {
                                            // Reconstruct a Plan object minimally for execution
                                            let plan = Plan::new_rtfs(plan_info.body.clone(), vec![selected.clone()]);
                                            let dbg = debug_callback.clone();
                                            let ccos_clone = Arc::clone(&ccos);
                                            tokio::task::spawn_local(async move {
                                                // Build a controlled runtime context for execution
                                                let ctx = runtime_service::default_controlled_context();
                                                match ccos_clone.validate_and_execute_plan(plan, &ctx).await {
                                                    Ok(exec) => {
                                                        let msg = serde_json::json!({"type":"EXEC_RESULT","intent_id": selected, "success": exec.success, "value": format!("{:?}", exec.value)});
                                                        let _ = (dbg)(msg.to_string());
                                                    }
                                                    Err(e) => {
                                                        let msg = serde_json::json!({"type":"EXEC_RESULT","intent_id": selected, "success": false, "error": format!("{}", e)});
                                                        let _ = (dbg)(msg.to_string());
                                                    }
                                                }
                                            });
                                            app.log_lines.push(format!("▶️ Execution requested for plan {}", plan_info.plan_id));
                                        } else { app.log_lines.push("ℹ️  No plan available for selected intent".into()); }
                                } else { app.log_lines.push("ℹ️  No intent selected".into()); }
                            } else {
                                app.log_lines.push("⚠️  No delegating arbiter available (LLM not enabled in config)".into());
                            }
                        }
                        (KeyCode::Char('c'), _) => { if let Some(id) = app.current_intent.clone() { let _ = cmd_tx.try_send(runtime_service::RuntimeCommand::Cancel { intent_id: id }); app.log_lines.push("🛑 Cancel requested".into()); } else { app.log_lines.push("ℹ️  No intent to cancel".into()); } }
                        (KeyCode::Char('r'), _) => { app.intent_graph.clear(); app.plans_by_intent.clear(); app.root_intent_id = None; app.selected_intent = None; app.current_intent = None; app.running = false; app.last_result = None; app.status_lines.clear(); app.log_lines.clear(); app.log_lines.push("🔄 Reset complete".into()); }
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
            let root_node = IntentNode { intent_id: intent_id.clone(), name: "Root Goal".to_string(), goal, status: IntentStatus::Active, children: vec![], parent: None, created_at: std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_secs(), metadata: HashMap::new(), };
            app.intent_graph.insert(intent_id.clone(), root_node);
            app.root_intent_id = Some(intent_id);
        }
        E::Status { intent_id, status } => { app.status_lines.push(status.clone()); if app.status_lines.len() > 200 { app.status_lines.drain(0..app.status_lines.len()-200); } if let Some(node) = app.intent_graph.get_mut(&intent_id) { if status.contains("Executing") { node.status = IntentStatus::Executing; } else if status.contains("Completed") { node.status = IntentStatus::Completed; } else if status.contains("Failed") { node.status = IntentStatus::Failed; } } }
        E::Step { intent_id, desc } => {
            // Append step description to plan info if present and log
            app.log_lines.push(format!("⚙️  {}", desc));
            if let Some(plan_info) = app.plans_by_intent.get_mut(&intent_id) {
                plan_info.execution_steps.push(desc.clone());
                plan_info.status = "Executing".to_string();
            }
            if app.log_lines.len() > 500 { app.log_lines.drain(0..app.log_lines.len()-500); }
        }
        E::Result { intent_id, result } => {
            app.running = false;
            app.last_result = Some(format!("✅ success={} value={:?}", result.success, result.value));
            app.log_lines.push("🏁 Execution completed".into());
            if let Some(node) = app.intent_graph.get_mut(&intent_id) { node.status = if result.success { IntentStatus::Completed } else { IntentStatus::Failed }; }
            if let Some(plan_info) = app.plans_by_intent.get_mut(&intent_id) {
                plan_info.status = if result.success { "Completed".to_string() } else { "Failed".to_string() };
            }
        }
        E::Error { message } => { app.running = false; app.log_lines.push(format!("❌ Error: {}", message)); }
        E::Heartbeat => {}
        E::Stopped => { app.running = false; app.log_lines.push("⏹️  Stopped".into()); }
    }
}

fn ui(f: &mut ratatui::Frame<'_>, app: &mut AppState) {
    let size = f.size();
    let tabs = Layout::default().direction(Direction::Vertical).constraints([Constraint::Length(1), Constraint::Length(3), Constraint::Min(5), Constraint::Length(1)]).split(size);
    let tab_titles = vec!["1:Graph", "2:Status", "3:Logs", "4:Debug", "5:Plans", "6:Capabilities"]; let tab_block = Block::default().borders(Borders::TOP | Borders::LEFT | Borders::RIGHT).title("Tabs • Ctrl+D:Toggle Debug • ?:Help"); let tab_items: Vec<ListItem> = tab_titles.iter().enumerate().map(|(i, &title)| { let style = match (app.current_tab, i) { (Tab::Graph, 0) | (Tab::Status, 1) | (Tab::Logs, 2) | (Tab::Debug, 3) | (Tab::Plans, 4) | (Tab::Capabilities, 5) => { Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD) } _ => Style::default().fg(Color::White), }; ListItem::new(title).style(style) }).collect(); let tab_list = List::new(tab_items).block(tab_block); f.render_widget(tab_list, tabs[0]);

    let input_title = match app.current_tab { Tab::Graph => "🎯 Goal Input (type) • s=Start c=Cancel r=Reset q=Quit • g=GenerateGraph", Tab::Status => "📊 Status View", Tab::Logs => "📝 Application Logs", Tab::Debug => "🔧 Debug Logs", Tab::Plans => "📋 Plan Details", Tab::Capabilities => "⚙️ Capability Calls", };
    let input = Paragraph::new(if matches!(app.current_tab, Tab::Graph) { app.goal_input.as_str() } else { "" }).block(Block::default().title(input_title).borders(Borders::ALL)).wrap(Wrap { trim: true }); f.render_widget(input, tabs[1]);

    match app.current_tab { Tab::Graph => render_graph_tab(f, app, tabs[2]), Tab::Status => render_status_tab(f, app, tabs[2]), Tab::Logs => render_logs_tab(f, app, tabs[2]), Tab::Debug => render_debug_tab(f, app, tabs[2]), Tab::Plans => render_plans_tab(f, app, tabs[2]), Tab::Capabilities => render_capabilities_tab(f, app, tabs[2]), }

    let status_text = format!("Intent: {} | Status: {} | Debug: {} | Tab: {}", app.current_intent.as_deref().unwrap_or("None"), if app.running { "Running" } else { "Idle" }, if app.show_debug { "Visible" } else { "Hidden" }, match app.current_tab { Tab::Graph => "Graph", Tab::Status => "Status", Tab::Logs => "Logs", Tab::Debug => "Debug", Tab::Plans => "Plans", Tab::Capabilities => "Capabilities", }); let status_bar = Paragraph::new(status_text).style(Style::default().fg(Color::Cyan)).block(Block::default().borders(Borders::TOP)); f.render_widget(status_bar, tabs[3]); if app.help_visible { render_help_overlay(f, size); }
}

fn render_graph_tab(f: &mut ratatui::Frame<'_>, app: &mut AppState, area: Rect) {
    let chunks = Layout::default().direction(Direction::Horizontal).constraints([Constraint::Percentage(60), Constraint::Percentage(40)]).split(area);
    // Rebuild visible display order each render
    app.display_order.clear();
    let mut graph_items: Vec<ListItem> = Vec::new(); let mut item_index = 0;
    if let Some(root_id) = &app.root_intent_id {
        if let Some(_root) = app.intent_graph.get(root_id) {
            build_graph_display_with_selection(&app.intent_graph, root_id, &mut graph_items, &mut item_index, 0, &app.selected_intent, &app.expanded_nodes, &mut app.display_order, app.selected_intent_index);
        } else { graph_items.push(ListItem::new("No graph data available".to_string())); }
    } else { graph_items.push(ListItem::new("No root intent yet".to_string())); }
    // Clamp cursor index to visible list bounds
    if !app.display_order.is_empty() && app.selected_intent_index >= app.display_order.len() {
        app.selected_intent_index = app.display_order.len() - 1;
    }
    let graph = List::new(graph_items).block(Block::default().title("🗺️  Intent Graph • ↑↓:Navigate • Enter:Select • Space:Expand • g:GenerateGraph").borders(Borders::ALL)).highlight_style(Style::default().add_modifier(Modifier::BOLD).fg(Color::Cyan)); f.render_widget(graph, chunks[0]);
    let detail_text = if let Some(selected_id) = &app.selected_intent { if let Some(node) = app.intent_graph.get(selected_id) { let plan_info = app.plans_by_intent.get(selected_id); format!("🎯 Intent Details:\nID: {}\nName: {}\nGoal: {}\nStatus: {:?}\nCreated: {}\n\n📋 Plan Info:\n{}", node.intent_id, node.name, node.goal, node.status, node.created_at, plan_info.map(|p| format!("Capabilities: {}\nStatus: {}\nSteps: {}", p.capabilities_required.join(", "), p.status, p.execution_steps.len())).unwrap_or("No plan information".to_string()) ) } else { "Selected intent not found".to_string() } } else { "Select an intent to view details\n\nUse ↑↓ to navigate\nEnter to select\nSpace to expand/collapse".to_string() };
    let details = Paragraph::new(detail_text).style(Style::default().fg(Color::White)).block(Block::default().title("📋 Intent Details").borders(Borders::ALL)).wrap(Wrap { trim: true }); f.render_widget(details, chunks[1]);
}

fn render_status_tab(f: &mut ratatui::Frame<'_>, app: &AppState, area: Rect) { let status_items: Vec<ListItem> = app.status_lines.iter().rev().take(100).map(|s| ListItem::new(s.clone())).collect(); let status = List::new(status_items).block(Block::default().title("📊 Status Updates").borders(Borders::ALL)); f.render_widget(status, area); }
fn render_logs_tab(f: &mut ratatui::Frame<'_>, app: &AppState, area: Rect) { let log_items: Vec<ListItem> = app.log_lines.iter().rev().take(200).map(|s| ListItem::new(s.clone())).collect(); let log = List::new(log_items).block(Block::default().title("📝 Application Logs").borders(Borders::ALL)); f.render_widget(log, area); }
fn render_debug_tab(f: &mut ratatui::Frame<'_>, app: &AppState, area: Rect) { let debug_items: Vec<ListItem> = app.debug_lines.iter().rev().take(200).map(|s| ListItem::new(s.clone())).collect(); let debug = List::new(debug_items).block(Block::default().title("🔧 Debug Logs").borders(Borders::ALL)); f.render_widget(debug, area); }
fn render_plans_tab(f: &mut ratatui::Frame<'_>, app: &AppState, area: Rect) { let plan_items: Vec<ListItem> = if let Some(selected_id) = &app.selected_intent { if let Some(plan_info) = app.plans_by_intent.get(selected_id) { vec![ ListItem::new(format!("📋 Plan ID: {}", plan_info.plan_id)), ListItem::new(format!("📝 Name: {}", plan_info.name.as_deref().unwrap_or("<unnamed>"))), ListItem::new(format!("📊 Status: {}", plan_info.status)), ListItem::new(format!("⚙️ Capabilities: {}", plan_info.capabilities_required.join(", "))), ListItem::new("📄 Plan Body:".to_string()), ].into_iter().chain( plan_info.body.lines().map(|line| ListItem::new(format!("  {}", line))) ).chain( plan_info.execution_steps.iter().map(|step| ListItem::new(format!("▶️ {}", step))) ).collect() } else { vec![ListItem::new("No plan selected or available".to_string())] } } else { vec![ListItem::new("Select an intent to view its plan".to_string())] }; let plans = List::new(plan_items).block(Block::default().title("📋 Plan Details").borders(Borders::ALL)); f.render_widget(plans, area); }
fn render_capabilities_tab(f: &mut ratatui::Frame<'_>, app: &AppState, area: Rect) { let cap_items: Vec<ListItem> = if app.capability_calls.is_empty() { vec![ListItem::new("No capability calls recorded yet".to_string())] } else { app.capability_calls.iter().rev().take(50).map(|call| { let status = if call.success { "✅" } else { "❌" }; let result = call.result.as_deref().unwrap_or("pending"); ListItem::new(format!("{} {}({}) → {}", status, call.capability_id, call.args, result)) }).collect() }; let capabilities = List::new(cap_items).block(Block::default().title("⚙️ Capability Calls").borders(Borders::ALL)); f.render_widget(capabilities, area); }

fn render_help_overlay(f: &mut ratatui::Frame<'_>, size: Rect) {
    let help_text = "\n🚀 Arbiter TUI Demo - Help\n\nNavigation:\n  1-4     Switch between tabs (Graph/Status/Logs/Debug)\n  Tab     Cycle through tabs\n  Ctrl+D  Toggle debug log visibility\n  ?/F1    Show/hide this help\n\nActions:\n  s       Start execution with current goal\n  c       Cancel current execution\n  r       Reset everything\n  q       Quit application\n  g       Generate Graph (LLM)\n  p       Generate Plan for selected intent (LLM)\n  e       Execute selected plan (LLM/runtime)\n\nInput:\n  Type    Edit goal text\n  Backspace Delete character\n\nTabs:\n  Graph   Intent graph visualization and results\n  Status  Real-time execution status updates\n  Logs    Application logs (non-debug)\n  Debug   Debug logs and detailed traces\n\nPress ? or F1 to close this help.\n";
    let help = Paragraph::new(help_text).style(Style::default().fg(Color::White).bg(Color::Black)).block(Block::default().title("❓ Help").borders(Borders::ALL)).wrap(Wrap { trim: true }); let help_area = centered_rect(60, 80, size); f.render_widget(Clear, help_area); f.render_widget(help, help_area);
}

fn centered_rect(percent_x: u16, percent_y: u16, r: Rect) -> Rect { let popup_layout = Layout::default().direction(Direction::Vertical).constraints([Constraint::Percentage((100 - percent_y) / 2), Constraint::Percentage(percent_y), Constraint::Percentage((100 - percent_y) / 2), ]).split(r); Layout::default().direction(Direction::Horizontal).constraints([Constraint::Percentage((100 - percent_x) / 2), Constraint::Percentage(percent_x), Constraint::Percentage((100 - percent_x) / 2), ]).split(popup_layout[1])[1] }

fn build_graph_display_with_selection(
    graph: &HashMap<IntentId, IntentNode>, 
    current_id: &IntentId, 
    items: &mut Vec<ListItem>, 
    item_index: &mut usize,
    depth: usize,
    selected_id: &Option<IntentId>,
    expanded_nodes: &HashSet<IntentId>,
    display_order: &mut Vec<IntentId>,
    selected_row_index: usize
) {
    if let Some(node) = graph.get(current_id) {
        let indent = "  ".repeat(depth);
        // Cursor highlight follows the current keyboard row (selected_intent_index)
        let is_cursor_row = *item_index == selected_row_index;
        let is_selected = selected_id.as_ref() == Some(current_id);
    let is_expanded = expanded_nodes.contains(current_id);
        let status_emoji = match node.status { IntentStatus::Active => "🟡", IntentStatus::Executing => "🔵", IntentStatus::Completed => "✅", IntentStatus::Failed => "❌", IntentStatus::Archived => "📦", IntentStatus::Suspended => "⏸️", };
        let expand_indicator = if !node.children.is_empty() { if is_expanded { "▼" } else { "▶" } } else { "  " };
        let display_name = if node.name.is_empty() { "<unnamed>".to_string() } else { node.name.clone() };
        let goal_preview = if node.goal.len() > 30 { format!("{}...", &node.goal[..27]) } else { node.goal.clone() };
        let mut style = Style::default();
        if is_cursor_row {
            style = style.fg(Color::Cyan).add_modifier(Modifier::BOLD);
        } else if is_selected {
            // Keep a subtle hint for the last explicitly selected intent
            style = style.fg(Color::LightBlue);
        }
    items.push(ListItem::new(format!("{}{}{}[{:?}] {} — {}", indent, expand_indicator, status_emoji, node.status, display_name, goal_preview)).style(style));
    // Record display order (this index maps to the list shown to the user)
    display_order.push(current_id.clone());
    *item_index += 1;
    if is_expanded { for child_id in &node.children { build_graph_display_with_selection(graph, child_id, items, item_index, depth + 1, selected_id, expanded_nodes, display_order, selected_row_index); } }
    }
}
