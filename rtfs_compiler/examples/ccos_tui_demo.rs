// TUI demo for the CCOS runtime service (non-blocking orchestration)
// Run: cargo run --example ccos_tui_demo --manifest-path rtfs_compiler/Cargo.toml

use std::io::{self};
use std::sync::Arc;

use crossterm::event::{self, Event as CEvent, KeyCode, KeyModifiers};
use crossterm::terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen};
use crossterm::execute;
use ratatui::{
    backend::CrosstermBackend,
    widgets::{Block, Borders, Paragraph, Wrap, List, ListItem},
    layout::{Layout, Constraint, Direction},
    style::{Style, Color},
    Terminal,
};
use tokio::sync::broadcast;

use rtfs_compiler::ccos::{CCOS, runtime_service};

#[derive(Default)]
struct AppState {
    goal_input: String,
    current_intent: Option<String>,
    status_lines: Vec<String>,
    log_lines: Vec<String>,
    last_result: Option<String>,
    running: bool,
}

fn main() -> io::Result<()> {
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

        // Initialize CCOS + runtime service
        let ccos = Arc::new(CCOS::new().await.expect("init CCOS"));
        let handle = runtime_service::start_service(Arc::clone(&ccos)).await;
        let mut evt_rx = handle.subscribe();
        let cmd_tx = handle.commands();

        let mut app = AppState::default();
    app.goal_input = "Add 2 and 3 using math.add".to_string();

    let frame_sleep = std::time::Duration::from_millis(16);

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
                                app.status_lines.push(format!("Start: {}", goal));
                            } else {
                                app.log_lines.push("Queue full: cannot start".into());
                            }
                        }
                        (KeyCode::Char('c'), _) => {
                            if let Some(id) = app.current_intent.clone() {
                                let _ = cmd_tx.try_send(runtime_service::RuntimeCommand::Cancel { intent_id: id });
                                app.log_lines.push("Cancel requested".into());
                            } else {
                                app.log_lines.push("No intent to cancel".into());
                            }
                        }
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
            app.log_lines.push(format!("Started: {}", goal));
        }
        E::Status { intent_id: _, status } => {
            app.status_lines.push(status);
            if app.status_lines.len() > 200 { app.status_lines.drain(0..app.status_lines.len()-200); }
        }
        E::Step { intent_id: _, desc } => {
            app.log_lines.push(desc);
            if app.log_lines.len() > 500 { app.log_lines.drain(0..app.log_lines.len()-500); }
        }
        E::Result { intent_id: _, result } => {
            app.running = false;
            app.last_result = Some(format!("Result: {}", result));
            app.log_lines.push("Result received".into());
        }
        E::Error { message } => {
            app.running = false;
            app.log_lines.push(format!("Error: {}", message));
        }
        E::Heartbeat => {}
        E::Stopped => { app.running = false; }
        runtime_service::RuntimeEvent::GraphGenerated { root_id, nodes: _, edges: _ } => {
            app.log_lines.push(format!("GraphGenerated: root_id={}", root_id));
        }
        runtime_service::RuntimeEvent::PlanGenerated { intent_id, plan_id, rtfs_code } => {
            app.log_lines.push(format!("PlanGenerated: intent={} plan={}", intent_id, plan_id));
        }
        runtime_service::RuntimeEvent::StepLog { step, status, message, details } => {
            app.log_lines.push(format!("StepLog: {} [{}] {} {:?}", step, status, message, details));
        }
        runtime_service::RuntimeEvent::ReadyForNext { next_step } => {
            app.log_lines.push(format!("ReadyForNext: {}", next_step));
        }
    }
}

fn ui(f: &mut ratatui::Frame<'_>, app: &AppState) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3), // input
            Constraint::Min(5),    // status/log
            Constraint::Length(3), // result + help
        ])
        .split(f.size());

    let input = Paragraph::new(app.goal_input.as_str())
        .block(Block::default().title("Goal (type) • s=Start c=Cancel q=Quit").borders(Borders::ALL))
        .wrap(Wrap { trim: true });
    f.render_widget(input, chunks[0]);

    let cols = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(50), Constraint::Percentage(50)])
        .split(chunks[1]);

    let status_items: Vec<ListItem> = app.status_lines.iter().rev().take(100).map(|s| ListItem::new(s.clone())).collect();
    let status = List::new(status_items)
        .block(Block::default().title("Status").borders(Borders::ALL));
    f.render_widget(status, cols[0]);

    let log_items: Vec<ListItem> = app.log_lines.iter().rev().take(200).map(|s| ListItem::new(s.clone())).collect();
    let log = List::new(log_items)
        .block(Block::default().title("Log").borders(Borders::ALL));
    f.render_widget(log, cols[1]);

    let result_text = app.last_result.as_deref().unwrap_or(if app.running { "Running..." } else { "Idle" });
    let result = Paragraph::new(result_text)
        .style(Style::default().fg(if app.running { Color::Yellow } else { Color::Cyan }))
        .block(Block::default().title("Result").borders(Borders::ALL));
    f.render_widget(result, chunks[2]);
}
