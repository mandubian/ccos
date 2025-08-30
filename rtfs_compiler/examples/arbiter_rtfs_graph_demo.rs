//! Arbiter RTFS Graph Generation Demo
//!
//! This example showcases the Arbiter's ability to generate a full intent graph
//! from a single high-level goal. The graph itself is generated as an RTFS
//! expression, which is then interpreted to build the structure in the IntentGraph.
//!
//! Try:
//!   cargo run --example arbiter_rtfs_graph_demo -- --goal "Review the latest security advisory for the 'log4j' package on GitHub. If a critical vulnerability exists for version 2.15.0, open a new issue in the 'our-company/security-audits' repository and assign it to the 'security-triage' team."
//!
//! To use a real LLM:
//!   export OPENROUTER_API_KEY=... && cargo run --example arbiter_rtfs_graph_demo -- --goal "..."
//!

use clap::Parser;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use tokio::sync::RwLock;
use std::time::{Duration, Instant};

use rtfs_compiler::ccos::types::{IntentId, IntentStatus, EdgeType};
use rtfs_compiler::ccos::intent_graph::IntentGraph;
use rtfs_compiler::ccos::causal_chain::CausalChain;
use rtfs_compiler::ccos::orchestrator::Orchestrator;
use rtfs_compiler::runtime::capability_marketplace::CapabilityMarketplace;
use rtfs_compiler::runtime::capabilities::registry::CapabilityRegistry;
use rtfs_compiler::runtime::security::RuntimeContext;
use rtfs_compiler::ccos::arbiter::{
    arbiter_factory::ArbiterFactory,
    arbiter_config::{ArbiterConfig, ArbiterEngineType, LlmConfig},
};
use rtfs_compiler::ccos::event_sink::CausalChainIntentEventSink;

// TUI imports
use crossterm::event::{self, Event as CEvent, KeyCode};
use crossterm::terminal::{disable_raw_mode, enable_raw_mode};
use ratatui::backend::CrosstermBackend;
use ratatui::Terminal;
use ratatui::widgets::{Block, Borders, List, ListItem, Paragraph};
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::text::{Line, Span};
use ratatui::style::{Color, Style};

#[derive(Parser, Debug)]
#[command(name = "arbiter_rtfs_graph_demo")]
#[command(about = "Generate and execute a full intent graph from a single goal using RTFS.")]
struct Args {
    /// Natural language goal for the Arbiter
    #[arg(long)]
    goal: Option<String>,

    /// Force stub (ignore API keys)
    #[arg(long, default_value_t = false)]
    stub: bool,

    /// Verbose output
    #[arg(long, default_value_t = false)]
    verbose: bool,

    /// Debug: print intent prompt, raw LLM responses, and parsed intent
    #[arg(long, default_value_t = false)]
    debug: bool,

    /// Enable TUI visualization
    #[arg(long, default_value_t = false)]
    tui: bool,
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args = Args::parse();

    let goal = args.goal.unwrap_or_else(|| {
        "Fetch a user's profile from GitHub and display their number of followers.".to_string()
    });

    println!("🚀 Arbiter RTFS Graph Demo\n===========================\n");
    println!("🎯 Goal: \"{}\"\n", goal);

    if args.debug {
        std::env::set_var("RTFS_ARBITER_DEBUG", "1");
        std::env::set_var("RTFS_SHOW_PROMPTS", "1");
        println!("🔧 Debug mode enabled (prompts and raw responses will be shown)");
    }

    // --- CCOS subsystems ---
    let causal_chain = Arc::new(Mutex::new(CausalChain::new()?));
    let event_sink = Arc::new(CausalChainIntentEventSink::new(Arc::clone(&causal_chain)));
    let intent_graph = Arc::new(Mutex::new(IntentGraph::with_event_sink(event_sink)?));

    let capability_registry = Arc::new(RwLock::new(CapabilityRegistry::new()));
    let marketplace = CapabilityMarketplace::with_causal_chain(
        Arc::clone(&capability_registry),
        Some(Arc::clone(&causal_chain)),
    );
    marketplace.bootstrap().await?;
    let marketplace = Arc::new(marketplace);

    let orchestrator = Arc::new(Orchestrator::new(
        Arc::clone(&causal_chain),
        Arc::clone(&intent_graph),
        Arc::clone(&marketplace),
    ));

    // --- Arbiter Configuration ---
    let use_stub = args.stub || std::env::var("OPENROUTER_API_KEY").is_err() && std::env::var("OPENAI_API_KEY").is_err();

    let arbiter_config = if use_stub {
        println!("⚠️  No API key detected or --stub set. Using stub Arbiter.\n");
    ArbiterConfig { engine_type: ArbiterEngineType::Dummy, ..Default::default() }
    } else {
        let (api_key, base_url, model) = if let Ok(key) = std::env::var("OPENROUTER_API_KEY") {
            let model = std::env::var("LLM_MODEL").unwrap_or_else(|_| "moonshotai/kimi-k2:free".to_string());
            (Some(key), Some("https://openrouter.ai/api/v1".to_string()), model)
        } else {
            let key = std::env::var("OPENAI_API_KEY").ok();
            let model = std::env::var("LLM_MODEL").unwrap_or_else(|_| "gpt-4o-mini".to_string());
            (key, None, model)
        };

        ArbiterConfig {
            engine_type: ArbiterEngineType::Llm,
            llm_config: Some(LlmConfig {
                provider_type: rtfs_compiler::ccos::arbiter::arbiter_config::LlmProviderType::OpenAI,
                model,
                api_key,
                base_url,
                max_tokens: Some(1024),
                temperature: Some(0.0), // Low temp for predictable structure
                timeout_seconds: Some(60),
                prompts: None,
            }),
            ..Default::default()
        }
    };

    let arbiter = ArbiterFactory::create_arbiter(arbiter_config, Arc::clone(&intent_graph), Some(Arc::clone(&marketplace)))
        .await
        .map_err(|e| format!("Failed to create arbiter: {}", e))?;

    // --- Graph Generation ---
    println!("🧠 Asking Arbiter to generate an intent graph from the goal...");

    // This is the new, core function we need to implement.
    // It will return the ID of the root intent of the generated graph.
    let root_intent_id = arbiter
        .natural_language_to_graph(&goal)
        .await
        .map_err(|e| format!("Failed to generate graph: {}", e))?;

    println!("✅ Arbiter generated graph. Root Intent ID: {}", root_intent_id);

    if args.verbose { print_graph_overview(&intent_graph, &root_intent_id); }

    // --- Orchestration Loop ---
    println!("\n🚀 Starting orchestration loop...");
    let ctx = RuntimeContext::full();
    let mut loop_count = 0;
    const MAX_LOOPS: u32 = 10; // Safety break

    // NEW: Collect plans by intent for final pretty output
    let mut plans_by_intent: HashMap<IntentId, String> = HashMap::new();

    // Optional TUI setup
    let use_tui = args.tui && atty::is(atty::Stream::Stdout);
    let mut terminal_opt: Option<Terminal<CrosstermBackend<std::io::Stdout>>> = None;
    if use_tui {
        enable_raw_mode().ok();
        let stdout = std::io::stdout();
        let backend = CrosstermBackend::new(stdout);
        let mut terminal = Terminal::new(backend).expect("create terminal");
        terminal.clear().ok();
        terminal_opt = Some(terminal);
    }

    let mut event_log: Vec<String> = Vec::with_capacity(128);
    // UI state
    let mut collapsed: std::collections::HashSet<IntentId> = std::collections::HashSet::new();
    let mut flat_rows: Vec<(usize, IntentId)> = Vec::new();
    let mut selected_row: usize = 0;
    let mut show_plan_steps: bool = true;
    let mut current_executing: Option<IntentId> = None;
    let mut last_draw = Instant::now();

    loop {
        if loop_count > MAX_LOOPS {
            println!("⚠️ Max loop count reached. Exiting.");
            break;
        }
    let ready_intents = {
            // Apply dependency semantics:
            // - An intent with DependsOn edges is ready only if all prerequisites are Completed.
            // - A parent intent (with incoming IsSubgoalOf edges) is ready only when all its subgoals are Completed.
            let g = intent_graph.lock().unwrap();
            let mut all = g.get_ready_intents();

            all.retain(|intent| {
                // Check DependsOn prerequisites
                let edges = g.get_edges_for_intent(&intent.intent_id);
                for e in &edges {
                    if e.edge_type == EdgeType::DependsOn && e.to == intent.intent_id {
                        if let Some(dep) = g.get_intent(&e.from) {
                            if dep.status != IntentStatus::Completed {
                                return false;
                            }
                        } else {
                            // Missing dependency -> not ready
                            return false;
                        }
                    }
                }

                // If this intent is a parent (has subgoals pointing to it), ensure all subgoals are Completed
                for e in &edges {
                    if e.edge_type == EdgeType::IsSubgoalOf && e.to == intent.intent_id {
                        if let Some(child) = g.get_intent(&e.from) {
                            if child.status != IntentStatus::Completed {
                                return false;
                            }
                        } else {
                            // Missing child -> be conservative and not ready
                            return false;
                        }
                    }
                }

                true
            });

            all
        };

        if ready_intents.is_empty() {
            let g = intent_graph.lock().unwrap();
            let root_status = g.get_intent(&root_intent_id).map(|i| i.status).unwrap_or(IntentStatus::Failed);
            if root_status == IntentStatus::Completed || root_status == IntentStatus::Failed {
                println!("🏁 Orchestration complete. Root intent status: {:?}", root_status);
                break;
            } else {
                 println!("⏳ No ready intents, but root is not finished. Waiting...");
                 // In a real system, this might wait or check for external events.
                 // For the demo, we'll just break if the graph seems stuck.
                 if loop_count > 0 && g.get_active_intents().is_empty() {
                    println!("🛑 No active or ready intents. Halting.");
                    break;
                 }
            }
        }

    for intent in ready_intents {
            println!("\n  - Found ready intent: {} (Goal: \"{}\")", intent.intent_id, intent.goal);
            // 1. Generate a plan for this specific intent
            println!("    - Generating plan...");
            event_log.push(format!("Plan: {}", intent.goal));
            let plan_result = arbiter.generate_plan_for_intent(&intent).await?;
            if let rtfs_compiler::ccos::types::PlanBody::Rtfs(code) = &plan_result.plan.body {
                plans_by_intent.insert(intent.intent_id.clone(), code.clone());
            }
            if args.verbose {
                if let rtfs_compiler::ccos::types::PlanBody::Rtfs(code) = &plan_result.plan.body {
                    println!("      - Plan Body: {}", code);
                }
            }

            // 2. Execute the plan
            println!("    - Executing plan...");
            event_log.push(format!("Exec: {}", intent.goal));
            // Track currently executing for TUI highlight
            current_executing = Some(intent.intent_id.clone());
            // Touch the value so the assignment is considered used even if UI draw happens later
            let _ = current_executing.is_some();
            let _ = orchestrator.execute_plan(&plan_result.plan, &ctx).await;
            current_executing = None;
        }
        loop_count += 1;

        // TUI draw cycle (throttle to ~30fps)
        if let Some(terminal) = terminal_opt.as_mut() {
            if last_draw.elapsed() > Duration::from_millis(33) {
                let g = intent_graph.lock().unwrap();
                terminal
                    .draw(|f| {
                        let root_chunks = Layout::default()
                            .direction(Direction::Vertical)
                            .constraints([
                                Constraint::Min(10),
                                Constraint::Length(7),
                            ])
                            .split(f.size());

                        let top_chunks = Layout::default()
                            .direction(Direction::Horizontal)
                            .constraints([
                                Constraint::Percentage(60),
                                Constraint::Percentage(40),
                            ])
                            .split(root_chunks[0]);

                        // Rebuild flat rows based on collapsed set
                        flat_rows.clear();
                        build_flat_tree(&g, &root_intent_id, 0, &mut flat_rows, &mut std::collections::HashSet::new(), &collapsed);
                        if selected_row >= flat_rows.len() && !flat_rows.is_empty() { selected_row = flat_rows.len() - 1; }

                        // Left/top: Intent tree (flat list with indentation)
                        let mut items: Vec<ListItem> = Vec::with_capacity(flat_rows.len());
                        for (idx, (depth, id)) in flat_rows.iter().enumerate() {
                            if let Some(intent) = g.get_intent(id) {
                                let (color, status_str) = status_color_str(&intent.status);
                                let bullet = "• ";
                                // Expansion indicator if this node has children
                                let has_children = !g.get_child_intents(&intent.intent_id).is_empty();
                                let collapsed_here = collapsed.contains(&intent.intent_id);
                                let indicator = if has_children { if collapsed_here { "▶ " } else { "▼ " } } else { "  " };
                                let indent = "  ".repeat(*depth);
                                let mut spans_vec = vec![Span::raw(indent), Span::raw(indicator), Span::raw(bullet), Span::styled(intent.name.clone().unwrap_or_else(|| "<unnamed>".to_string()), Style::default().fg(color)), Span::raw(format!(" [{}]", status_str))];
                                if let Some(exec_id) = &current_executing { if exec_id == id { spans_vec.push(Span::raw(" ⏳")); } }
                                let line = Line::from(spans_vec);
                                let item = ListItem::new(line);
                                if idx == selected_row { /* styling handled by color; optional underline could be added */ }
                                items.push(item);
                            }
                        }
                        let tree = List::new(items)
                            .block(Block::default().borders(Borders::ALL).title("Intents (↑/↓ select, ←/→ collapse/expand, Enter toggle, a=expand all, z=collapse all, p=steps, q=quit)"));
                        f.render_widget(tree, top_chunks[0]);

                        // Right/top: Details / Steps for selected intent
                        let detail_block = Block::default().borders(Borders::ALL).title("Details");
                        let detail_area = top_chunks[1];
                        if show_plan_steps && !flat_rows.is_empty() {
                            let (_, selected_id) = &flat_rows[selected_row];
                            let mut lines: Vec<Line> = Vec::new();
                            if let Some(intent) = g.get_intent(selected_id) {
                                lines.push(Line::from(vec![Span::styled("Intent:", Style::default().fg(Color::Cyan)), Span::raw(format!(" {}", intent.name.clone().unwrap_or_else(|| "<unnamed>".to_string())))]));
                                lines.push(Line::raw(format!("Goal: {}", intent.goal)));
                            }
                            if let Some(plan) = plans_by_intent.get(selected_id) {
                                let steps = extract_steps_from_plan(plan);
                                lines.push(Line::from(Span::styled("Steps:", Style::default().fg(Color::Cyan))));
                                if steps.is_empty() {
                                    lines.push(Line::raw("(no steps found)"));
                                } else {
                                    for (i, s) in steps.iter().enumerate() {
                                        lines.push(Line::raw(format!("{}. {}", i + 1, s)));
                                    }
                                }
                            } else {
                                lines.push(Line::raw("No plan generated yet."));
                            }
                            let para = Paragraph::new(lines).block(detail_block);
                            f.render_widget(para, detail_area);
                        } else {
                            let para = Paragraph::new(Line::raw("(steps hidden, press 'p' to toggle)")).block(detail_block);
                            f.render_widget(para, detail_area);
                        }

                        // Bottom: Event log
                        let tail: Vec<ListItem> = event_log.iter().rev().take(5).rev().map(|s| ListItem::new(s.as_str())).collect();
                        let logw = List::new(tail)
                            .block(Block::default().borders(Borders::ALL).title("Events"));
                        f.render_widget(logw, root_chunks[1]);
                    })
                    .ok();
                last_draw = Instant::now();
            }

            // Non-blocking key handling: press q to quit early
            while event::poll(Duration::from_millis(1)).unwrap_or(false) {
                if let Ok(CEvent::Key(k)) = event::read() {
                    match k.code {
                        KeyCode::Char('q') => {
                            disable_raw_mode().ok();
                            terminal_opt.take();
                            println!("\n👋 Quit by user (q)");
                            break;
                        }
                        KeyCode::Up => { if selected_row > 0 { selected_row -= 1; } }
                        KeyCode::Down => { if selected_row + 1 < flat_rows.len() { selected_row += 1; } }
                        KeyCode::Left => { if let Some((_, id)) = flat_rows.get(selected_row) { collapsed.insert(id.clone()); } }
                        KeyCode::Right => { if let Some((_, id)) = flat_rows.get(selected_row) { collapsed.remove(id); } }
                        KeyCode::Enter => { if let Some((_, id)) = flat_rows.get(selected_row) { if !collapsed.insert(id.clone()) { collapsed.remove(id); } } }
                        KeyCode::Char('z') => { // collapse all under root
                            collapsed.clear();
                            // collapse everything by default: add all nodes with children
                            {
                                let g = intent_graph.lock().unwrap();
                                add_all_nodes_with_children(&g, &root_intent_id, &mut collapsed);
                            }
                        }
                        KeyCode::Char('a') => { // expand all
                            collapsed.clear();
                        }
                        KeyCode::Char('p') => { show_plan_steps = !show_plan_steps; }
                        _ => {}
                    }
                }
            }
        }
    }

    println!("\n📊 Final Graph State:");
    print_graph_overview(&intent_graph, &root_intent_id);

    // Also print a detailed view with the RTFS plan body associated to each intent
    println!("\n🗺️  Graph with plans (detailed):");
    print_graph_with_plans(&intent_graph, &root_intent_id, &plans_by_intent);

    if let Some(mut terminal) = terminal_opt {
        disable_raw_mode().ok();
        terminal.show_cursor().ok();
    }

    Ok(())
}

// Simple overview: print parents and children reachable from a root
fn print_graph_overview(intent_graph: &Arc<Mutex<IntentGraph>>, root_id: &IntentId) {
    let g = intent_graph.lock().unwrap();
    println!("\n🧭 Graph overview from root {}:", root_id);
    // List children (direct)
    let children = g.get_child_intents(root_id);
    for c in &children {
        println!("  • child {} [{:?}]", c.intent_id, c.status);
    }

    // For each child, list its DependsOn parents
    for c in &children {
        let edges = g.get_edges_for_intent(&c.intent_id);
        let deps: Vec<_> = edges
            .into_iter()
            .filter(|e| e.edge_type == EdgeType::DependsOn && e.from == c.intent_id)
            .map(|e| e.to)
            .collect();
        if !deps.is_empty() {
            for d in deps {
                if let Some(pi) = g.get_intent(&d) {
                    println!("     ↳ depends on {} [{:?}]", pi.intent_id, pi.status);
                } else {
                    println!("     ↳ depends on {} [missing]", d);
                }
            }
        }
    }
}

// Detailed view: recursively print the tree from root with per-intent plan bodies (if any)
fn print_graph_with_plans(
    intent_graph: &Arc<Mutex<IntentGraph>>,
    root_id: &IntentId,
    plans_by_intent: &HashMap<IntentId, String>,
) {
    let g = intent_graph.lock().unwrap();

    fn recurse(
        g: &IntentGraph,
        current: &IntentId,
        plans: &HashMap<IntentId, String>,
        indent: usize,
        seen: &mut std::collections::HashSet<IntentId>,
    ) {
        if seen.contains(current) { return; }
        seen.insert(current.clone());

        let pad = "  ".repeat(indent);
        if let Some(intent) = g.get_intent(current) {
            let name = intent.name.as_deref().unwrap_or("<unnamed>");
            println!("{}• {} | name={} | status={:?}", pad, intent.intent_id, name, intent.status);
            println!("{}  goal: {}", pad, intent.goal);
            if let Some(plan) = plans.get(&intent.intent_id) {
                println!("{}  plan:", pad);
                for line in plan.lines() {
                    println!("{}    {}", pad, line);
                }
            } else {
                println!("{}  plan: <none>", pad);
            }
            // Recurse into children
            let children = g.get_child_intents(&intent.intent_id);
            for child in children {
                recurse(g, &child.intent_id, plans, indent + 1, seen);
            }
        } else {
            println!("{}• {} [missing intent]", pad, current);
        }
    }

    let mut seen = std::collections::HashSet::new();
    recurse(&g, root_id, plans_by_intent, 0, &mut seen);
}

// Build list items recursively with colored status
fn build_tree_items(
    g: &IntentGraph,
    current: &IntentId,
    depth: usize,
    out: &mut Vec<ratatui::widgets::ListItem<'static>>,
    seen: &mut std::collections::HashSet<IntentId>,
) {
    if seen.contains(current) { return; }
    seen.insert(current.clone());
    if let Some(intent) = g.get_intent(current) {
        let name = intent.name.clone().unwrap_or_else(|| "<unnamed>".to_string());
        let (color, status_str) = match intent.status {
            IntentStatus::Active => (Color::Yellow, "Active"),
            IntentStatus::Executing => (Color::Blue, "Executing"),
            IntentStatus::Completed => (Color::Green, "Completed"),
            IntentStatus::Failed => (Color::Red, "Failed"),
            IntentStatus::Archived => (Color::Gray, "Archived"),
            IntentStatus::Suspended => (Color::Magenta, "Suspended"),
        };
        let indent = "  ".repeat(depth);
        let spans_vec = vec![
            Span::raw(indent),
            Span::raw("• "),
            Span::styled(name, Style::default().fg(color)),
            Span::raw(format!(" [{}] — {}", status_str, intent.goal)),
        ];
        out.push(ListItem::new(Line::from(spans_vec)));

        for child in g.get_child_intents(&intent.intent_id) {
            build_tree_items(g, &child.intent_id, depth + 1, out, seen);
        }
    }
}

// Build a flat list of (depth, intent_id), honoring collapsed nodes
fn build_flat_tree(
    g: &IntentGraph,
    current: &IntentId,
    depth: usize,
    out: &mut Vec<(usize, IntentId)>,
    seen: &mut std::collections::HashSet<IntentId>,
    collapsed: &std::collections::HashSet<IntentId>,
) {
    if seen.contains(current) { return; }
    seen.insert(current.clone());
    out.push((depth, current.clone()));
    if collapsed.contains(current) { return; }
    for child in g.get_child_intents(current) {
        build_flat_tree(g, &child.intent_id, depth + 1, out, seen, collapsed);
    }
}

fn status_color_str(status: &IntentStatus) -> (Color, &'static str) {
    match status {
        IntentStatus::Active => (Color::Yellow, "Active"),
        IntentStatus::Executing => (Color::Blue, "Executing"),
        IntentStatus::Completed => (Color::Green, "Completed"),
        IntentStatus::Failed => (Color::Red, "Failed"),
        IntentStatus::Archived => (Color::Gray, "Archived"),
        IntentStatus::Suspended => (Color::Magenta, "Suspended"),
    }
}

fn add_all_nodes_with_children(g: &IntentGraph, current: &IntentId, collapsed: &mut std::collections::HashSet<IntentId>) {
    let children = g.get_child_intents(current);
    if !children.is_empty() {
        collapsed.insert(current.clone());
        for c in children { add_all_nodes_with_children(g, &c.intent_id, collapsed); }
    }
}

// Extract step names from a plan body string by scanning for (step "...")
fn extract_steps_from_plan(plan: &str) -> Vec<String> {
    let mut steps = Vec::new();
    let mut i = 0;
    while let Some(pos) = plan[i..].find("(step \"") {
        let start = i + pos + 7; // after (step "
        if let Some(end_rel) = plan[start..].find('"') {
            let end = start + end_rel;
            steps.push(plan[start..end].to_string());
            i = end + 1;
        } else { break; }
    }
    steps
}
