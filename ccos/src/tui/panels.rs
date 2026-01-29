//! TUI Panel Rendering
//!
//! Ratatui widgets for each panel in the 6-panel layout.

use ratatui::{
    layout::{Alignment, Constraint, Direction, Layout, Rect},
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, List, ListItem, ListState, Paragraph, Wrap},
    Frame,
};

use super::state::{
    ActivePanel, AppState, ApprovalsTab, AuthStatus, CapabilityCategory, DiscoverPopup,
    DiscoveryEntry, ExecutionMode, NodeStatus, ServerStatus, TraceEventType, View,
};
use super::theme;
use crate::discovery::registry_search::{DiscoveryCategory, RegistrySearchResult};
use serde_json;

/// Render the complete TUI
pub fn render(f: &mut Frame, state: &mut AppState) {
    // Main layout: header | [nav menu | content] | status bar
    let main_chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1), // Header
            Constraint::Min(10),   // Nav + Content
            Constraint::Length(1), // Status bar
        ])
        .split(f.size());

    render_header(f, state, main_chunks[0]);

    // Horizontal split: nav menu | main content
    let content_chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Length(16), // Nav menu (fixed width)
            Constraint::Min(40),    // Main content
        ])
        .split(main_chunks[1]);

    render_nav_menu(f, state, content_chunks[0]);
    render_view_content(f, state, content_chunks[1]);
    render_status_bar(f, state, main_chunks[2]);

    // Help overlay if active
    if state.show_help {
        render_help_overlay(f);
    }

    // Trace detail popup if active
    if state.show_trace_popup {
        render_trace_popup(f, state);
    } else if state.show_intent_popup {
        render_intent_popup(f, state);
    }

    // Discovery popup if active
    if !matches!(state.discover_popup, DiscoverPopup::None) {
        render_discover_popup(f, state);
    }

    // Auth token popup overlay (global, works in any view)
    if let Some(ref popup) = state.auth_token_popup {
        render_auth_token_popup(f, popup);
    }
}

/// Render the top header bar
fn render_header(f: &mut Frame, state: &mut AppState, area: Rect) {
    let mode_color = match state.mode {
        ExecutionMode::Idle => theme::SUBTEXT0,
        ExecutionMode::Received => theme::TEAL,
        ExecutionMode::Planning => theme::BLUE,
        ExecutionMode::Executing => theme::YELLOW,
        ExecutionMode::Complete => theme::GREEN,
        ExecutionMode::Error => theme::RED,
    };

    let status = if state.is_running() {
        format!("{} {}", state.spinner_icon(), state.mode)
    } else {
        format!("{}", state.mode)
    };

    // Build header spans
    let mut spans = vec![
        Span::styled(
            " CCOS ",
            Style::default()
                .fg(theme::MAUVE)
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled("Control Center ", Style::default().fg(theme::TEXT)),
        Span::styled("│ ", Style::default().fg(theme::SURFACE1)),
        Span::styled(status, Style::default().fg(mode_color)),
    ];

    // Add discover/server loading status if applicable
    if state.discover_loading {
        spans.push(Span::styled(" │ ", Style::default().fg(theme::SURFACE1)));
        spans.push(Span::styled(
            format!("{} Searching...", state.spinner_icon()),
            Style::default().fg(theme::TEAL),
        ));
    } else if state.servers_loading {
        spans.push(Span::styled(" │ ", Style::default().fg(theme::SURFACE1)));
        spans.push(Span::styled(
            format!("{} Loading Servers...", state.spinner_icon()),
            Style::default().fg(theme::BLUE),
        ));
    }

    let line = Line::from(spans);
    let paragraph = Paragraph::new(line).style(Style::default().bg(theme::SURFACE0));
    f.render_widget(paragraph, area);
}

/// Render the left navigation menu
fn render_nav_menu(f: &mut Frame, state: &mut AppState, area: Rect) {
    let views = [
        (View::Discover, "Discover", "1"),
        (View::Servers, "Servers", "2"),
        (View::Approvals, "Approvals", "3"),
        (View::Goals, "Goals", "4"),
        (View::Plans, "Plan", "5"),
        (View::Execute, "Execute", "6"),
        (View::Config, "Config", "7"),
    ];

    let items: Vec<ListItem> = views
        .iter()
        .map(|(view, name, key)| {
            let is_selected = state.current_view == *view;
            let (prefix, style) = if is_selected {
                (
                    "► ",
                    Style::default()
                        .fg(theme::MAUVE)
                        .add_modifier(Modifier::BOLD),
                )
            } else {
                ("  ", Style::default().fg(theme::SUBTEXT0))
            };
            ListItem::new(Line::from(vec![
                Span::styled(prefix, style),
                Span::styled(format!("{} ", key), Style::default().fg(theme::SURFACE2)),
                Span::styled(*name, style),
            ]))
        })
        .collect();

    let block = Block::default()
        .title("Menu")
        .borders(Borders::ALL)
        .border_style(Style::default().fg(theme::SURFACE1));

    let list = List::new(items).block(block);
    f.render_widget(list, area);
}

/// Render view-specific content in the main area
fn render_view_content(f: &mut Frame, state: &mut AppState, area: Rect) {
    match state.current_view {
        View::Goals => {
            // Goals view: goal input + decomposition tree + LLM inspector
            let chunks = Layout::default()
                .direction(Direction::Vertical)
                .constraints([
                    Constraint::Length(3), // Goal input
                    Constraint::Min(10),   // Main content
                ])
                .split(area);

            render_goal_input(f, state, chunks[0]);
            render_goals_view(f, state, chunks[1]);
        }
        View::Servers => {
            render_servers_view(f, state, area);
        }
        View::Plans => {
            render_placeholder_view(f, View::Plans, area);
        }
        View::Execute => {
            render_placeholder_view(f, View::Execute, area);
        }
        View::Discover => {
            render_discover_view(f, state, area);
        }
        View::Approvals => {
            render_approvals_view(f, state, area);
        }
        View::Config => {
            render_placeholder_view(f, View::Config, area);
        }
    }
}

/// Render the goal input panel
fn render_goal_input(f: &mut Frame, state: &mut AppState, area: Rect) {
    let is_active = state.active_panel == ActivePanel::GoalInput;

    let border_color = if is_active {
        theme::PANEL_BORDER_ACTIVE
    } else {
        theme::PANEL_BORDER
    };

    // Add spinner to title when running
    let title = if state.is_running() {
        format!("Goal Input [{} {}]", state.spinner_icon(), state.mode)
    } else {
        format!("Goal Input [{}]", state.mode)
    };
    let block = Block::default()
        .title(title)
        .borders(Borders::ALL)
        .border_style(Style::default().fg(border_color));

    let mode_style = match state.mode {
        ExecutionMode::Idle => Style::default().fg(theme::SUBTEXT0),
        ExecutionMode::Received => Style::default().fg(theme::TEAL),
        ExecutionMode::Planning => Style::default().fg(theme::BLUE),
        ExecutionMode::Executing => Style::default().fg(theme::YELLOW),
        ExecutionMode::Complete => Style::default().fg(theme::GREEN),
        ExecutionMode::Error => Style::default().fg(theme::RED),
    };

    let input_text = if state.goal_input.is_empty() && !is_active {
        Span::styled("Enter a goal...", Style::default().fg(theme::SUBTEXT0))
    } else {
        Span::styled(&state.goal_input, Style::default().fg(theme::TEXT))
    };

    let paragraph = Paragraph::new(Line::from(vec![Span::styled("> ", mode_style), input_text]))
        .block(block)
        .style(Style::default().bg(theme::BASE));

    f.render_widget(paragraph, area);
}

/// Render placeholder for views under development
fn render_placeholder_view(f: &mut Frame, view: View, area: Rect) {
    let title = format!("{:?} View (Coming Soon)", view);
    let block = Block::default()
        .title(title)
        .borders(Borders::ALL)
        .border_style(Style::default().fg(theme::PANEL_BORDER));

    let content = Paragraph::new("This view is under development...")
        .style(Style::default().fg(theme::SUBTEXT0))
        .block(block);

    f.render_widget(content, area);
}

/// Render the Servers view - MCP server management
fn render_servers_view(f: &mut Frame, state: &mut AppState, area: Rect) {
    // Two-column layout: Server list (left) | Server details (right)
    let cols = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(40), Constraint::Percentage(60)])
        .split(area);

    render_server_list(f, state, cols[0]);
    render_server_details(f, state, cols[1]);
}

/// Render the list of MCP servers
fn render_server_list(f: &mut Frame, state: &mut AppState, area: Rect) {
    let title = if state.servers_loading {
        "MCP Servers [Loading...]".to_string()
    } else {
        format!("MCP Servers [{}]", state.servers.len())
    };

    let block = Block::default()
        .title(title)
        .borders(Borders::ALL)
        .border_style(Style::default().fg(theme::PANEL_BORDER_ACTIVE));

    if state.servers.is_empty() {
        let content = if state.servers_loading {
            "Loading servers..."
        } else {
            "No servers configured.\n\nPress 'r' to refresh or add servers in ccos.toml"
        };
        let paragraph = Paragraph::new(content)
            .style(Style::default().fg(theme::SUBTEXT0))
            .block(block);
        f.render_widget(paragraph, area);
        return;
    }

    // Layout: List (top) | Help Footer (bottom)
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Min(1),    // List area
            Constraint::Length(1), // Help footer
        ])
        .split(area);

    let list_area = chunks[0];
    let help_area = chunks[1];

    let items: Vec<ListItem> = state
        .servers
        .iter()
        .enumerate()
        .map(|(i, server)| {
            let is_selected = i == state.servers_selected;
            let status_color = match server.status {
                ServerStatus::Connected => theme::STATUS_SUCCESS,
                ServerStatus::Disconnected => theme::SUBTEXT0,
                ServerStatus::Connecting => theme::STATUS_WARNING,
                ServerStatus::Error => theme::STATUS_ERROR,
                ServerStatus::Timeout => theme::STATUS_WARNING,
                ServerStatus::Unknown => theme::SUBTEXT0,
                ServerStatus::Pending => theme::STATUS_WARNING,
                ServerStatus::Rejected => theme::STATUS_ERROR,
            };

            let tools_str = server
                .tool_count
                .map(|c| format!(" [{} tools]", c))
                .unwrap_or_default();

            let line = Line::from(vec![
                Span::styled(server.status.icon(), Style::default().fg(status_color)),
                Span::raw(" "),
                Span::styled(
                    &server.name,
                    if is_selected {
                        Style::default()
                            .fg(theme::TEXT)
                            .add_modifier(Modifier::BOLD)
                    } else {
                        Style::default().fg(theme::SUBTEXT1)
                    },
                ),
                Span::styled(tools_str, Style::default().fg(theme::SUBTEXT0)),
            ]);

            let mut item = ListItem::new(line);
            if is_selected {
                item = item.style(
                    Style::default()
                        .bg(theme::SURFACE0)
                        .add_modifier(Modifier::BOLD),
                );
            }
            item
        })
        .collect();

    let list = List::new(items).block(block);
    f.render_widget(list, list_area);

    // Render help footer
    let help_text = Line::from(vec![
        Span::styled("[d]", Style::default().fg(theme::GREEN)),
        Span::raw(" Discover  "),
        Span::styled("[r]", Style::default().fg(theme::BLUE)),
        Span::raw(" Refresh  "),
        Span::styled("[f]", Style::default().fg(theme::YELLOW)),
        Span::raw(" Find  "),
        Span::styled("[x]", Style::default().fg(theme::RED)),
        Span::raw(" Delete"),
    ]);
    let help_paragraph = Paragraph::new(help_text).alignment(Alignment::Center);
    f.render_widget(help_paragraph, help_area);
}

/// Render details for the selected server
fn render_server_details(f: &mut Frame, state: &mut AppState, area: Rect) {
    let block = Block::default()
        .title("Server Details")
        .borders(Borders::ALL)
        .border_style(Style::default().fg(theme::PANEL_BORDER));

    if state.servers.is_empty() {
        let paragraph = Paragraph::new("Select a server to view details")
            .style(Style::default().fg(theme::SUBTEXT0))
            .block(block);
        f.render_widget(paragraph, area);
        return;
    }

    if state.servers_selected >= state.servers.len() {
        return;
    }

    let server = &state.servers[state.servers_selected];

    // Render the outer block
    f.render_widget(block.clone(), area);

    // Get inner area for content
    let inner_area = block.inner(area);

    // Layout:
    // Top: Info (full width)
    // Middle: Tool List (Scrollable)
    // Bottom: Actions (Right-aligned)
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(4), // Server info
            Constraint::Min(1),    // Tool list body
            Constraint::Length(8), // Actions at bottom
        ])
        .split(inner_area);

    // --- Render Server Info (Top) ---
    let status_color = match server.status {
        ServerStatus::Connected => theme::STATUS_SUCCESS,
        ServerStatus::Disconnected => theme::SUBTEXT0,
        ServerStatus::Connecting => theme::STATUS_WARNING,
        ServerStatus::Error => theme::STATUS_ERROR,
        ServerStatus::Timeout => theme::STATUS_WARNING,
        ServerStatus::Unknown => theme::SUBTEXT0,
        ServerStatus::Pending => theme::STATUS_WARNING,
        ServerStatus::Rejected => theme::STATUS_ERROR,
    };

    let status_text = match server.status {
        ServerStatus::Connected => "Connected",
        ServerStatus::Disconnected => "Disconnected",
        ServerStatus::Connecting => "Connecting...",
        ServerStatus::Error => "Error",
        ServerStatus::Timeout => "Timed out",
        ServerStatus::Unknown => "Unknown",
        ServerStatus::Pending => "Pending Approval",
        ServerStatus::Rejected => "Rejected",
    };

    let tools_text = server
        .tool_count
        .map(|c| format!("{}", c))
        .unwrap_or_else(|| "Unknown".to_string());

    let info_lines = vec![
        Line::from(vec![
            Span::styled("Name:     ", Style::default().fg(theme::SUBTEXT0)),
            Span::styled(
                &server.name,
                Style::default()
                    .fg(theme::TEXT)
                    .add_modifier(Modifier::BOLD),
            ),
        ]),
        Line::from(vec![
            Span::styled("Endpoint: ", Style::default().fg(theme::SUBTEXT0)),
            Span::styled(&server.endpoint, Style::default().fg(theme::SAPPHIRE)),
        ]),
        Line::from(vec![
            Span::styled("Status:   ", Style::default().fg(theme::SUBTEXT0)),
            Span::styled(server.status.icon(), Style::default().fg(status_color)),
            Span::raw(" "),
            Span::styled(status_text, Style::default().fg(status_color)),
        ]),
        Line::from(vec![
            Span::styled("Tools:    ", Style::default().fg(theme::SUBTEXT0)),
            Span::styled(tools_text, Style::default().fg(theme::PEACH)),
        ]),
    ];

    f.render_widget(Paragraph::new(info_lines), chunks[0]);

    // --- Render Tool List (Middle) ---
    let mut tool_lines = Vec::new();
    if !server.tools.is_empty() {
        tool_lines.push(Line::from(vec![Span::styled(
            "Tool List:",
            Style::default().fg(theme::SUBTEXT0),
        )]));
        for tool_name in &server.tools {
            tool_lines.push(Line::from(vec![
                Span::styled("  • ", Style::default().fg(theme::GREEN)),
                Span::styled(tool_name.clone(), Style::default().fg(theme::TEXT)),
            ]));
        }
    } else {
        tool_lines.push(Line::from(Span::styled(
            "No tools discovered yet.",
            Style::default().fg(theme::SUBTEXT0),
        )));
    }

    let scroll_offset = state.server_details_scroll;
    f.render_widget(
        Paragraph::new(tool_lines).scroll((scroll_offset as u16, 0)),
        chunks[1],
    );

    // --- Render Actions (Bottom Right) ---
    let actions_lines = vec![
        Line::from(vec![Span::styled(
            "Actions",
            Style::default()
                .fg(theme::SUBTEXT0)
                .add_modifier(Modifier::UNDERLINED),
        )]),
        Line::from(vec![
            Span::styled("[d] ", Style::default().fg(theme::MAUVE)),
            Span::raw("Discover tools"),
        ]),
        Line::from(vec![
            Span::styled("[c] ", Style::default().fg(theme::MAUVE)),
            Span::raw("Check connection"),
        ]),
        Line::from(vec![
            Span::styled("[r] ", Style::default().fg(theme::MAUVE)),
            Span::raw("Refresh servers"),
        ]),
        Line::from(vec![
            Span::styled("[f] ", Style::default().fg(theme::TEAL)),
            Span::raw("Find new servers"),
        ]),
        Line::from(vec![
            Span::styled("[R] ", Style::default().fg(theme::YELLOW)),
            Span::raw("Retry rejected"),
        ]),
        Line::from(vec![
            Span::styled("[S] ", Style::default().fg(theme::MAUVE)),
            Span::raw("Refresh schemas"),
        ]),
        Line::from(vec![
            Span::styled("[x] ", Style::default().fg(theme::RED)),
            Span::raw("Delete server"),
        ]),
    ];

    // Create a layout for bottom area to align actions to the right
    let bottom_chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Min(0),     // Spacer (takes remaining space)
            Constraint::Length(25), // Actions width (enough for action text)
        ])
        .split(chunks[2]);

    // Render actions aligned to the right
    f.render_widget(
        Paragraph::new(actions_lines).alignment(Alignment::Right),
        bottom_chunks[1],
    );
}

// =========================================
// Approvals View
// =========================================

/// Render the Approvals view - pending and approved servers
fn render_approvals_view(f: &mut Frame, state: &mut AppState, area: Rect) {
    // Layout: Tab bar (top) | Content (bottom)
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3), // Tab bar
            Constraint::Min(1),    // Content
        ])
        .split(area);

    render_approvals_tabs(f, state, chunks[0]);

    // Two-column layout: List (left) | Details (right)
    let cols = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(45), Constraint::Percentage(55)])
        .split(chunks[1]);

    match state.approvals_tab {
        ApprovalsTab::Pending => {
            render_pending_list(f, state, cols[0]);
            render_pending_details(f, state, cols[1]);
        }
        ApprovalsTab::Approved => {
            render_approved_list(f, state, cols[0]);
            render_approved_details(f, state, cols[1]);
        }
        ApprovalsTab::Budget => {
            render_budget_list(f, state, cols[0]);
            render_budget_details(f, state, cols[1]);
        }
    }
}

/// Render the tab bar for Approvals view
fn render_approvals_tabs(f: &mut Frame, state: &AppState, area: Rect) {
    let pending_count = state.pending_servers.len();
    let approved_count = state.approved_servers.len();
    let budget_count = state.budget_approvals.len();

    let pending_style = if state.approvals_tab == ApprovalsTab::Pending {
        Style::default()
            .fg(theme::BASE)
            .bg(theme::MAUVE)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(theme::SUBTEXT1)
    };

    let approved_style = if state.approvals_tab == ApprovalsTab::Approved {
        Style::default()
            .fg(theme::BASE)
            .bg(theme::GREEN)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(theme::SUBTEXT1)
    };

    let budget_style = if state.approvals_tab == ApprovalsTab::Budget {
        Style::default()
            .fg(theme::BASE)
            .bg(theme::PEACH)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(theme::SUBTEXT1)
    };

    let loading_indicator = if state.approvals_loading { " ⟳" } else { "" };

    let tabs = Line::from(vec![
        Span::styled(format!(" [[] Pending ({}) ", pending_count), pending_style),
        Span::raw("  "),
        Span::styled(
            format!(" []] Approved ({}) ", approved_count),
            approved_style,
        ),
        Span::raw("  "),
        Span::styled(
            format!(" {{}} Budget ({}) ", budget_count),
            budget_style,
        ),
        Span::styled(loading_indicator, Style::default().fg(theme::YELLOW)),
    ]);

    let block = Block::default()
        .title("Server Approvals")
        .borders(Borders::ALL)
        .border_style(Style::default().fg(theme::PANEL_BORDER_ACTIVE));

    let paragraph = Paragraph::new(tabs).block(block);
    f.render_widget(paragraph, area);
}

/// Render the pending servers list
fn render_pending_list(f: &mut Frame, state: &AppState, area: Rect) {
    let is_active = state.active_panel == ActivePanel::ApprovalsPendingList;
    let border_color = if is_active {
        theme::PANEL_BORDER_ACTIVE
    } else {
        theme::PANEL_BORDER
    };

    let block = Block::default()
        .title(format!("Pending [{}]", state.pending_servers.len()))
        .borders(Borders::ALL)
        .border_style(Style::default().fg(border_color));

    if state.pending_servers.is_empty() {
        let content = if state.approvals_loading {
            "Loading pending servers..."
        } else {
            "No pending servers.\n\nUse Discover view to find and add new servers."
        };
        let paragraph = Paragraph::new(content)
            .style(Style::default().fg(theme::SUBTEXT0))
            .block(block);
        f.render_widget(paragraph, area);
        return;
    }

    // Layout: List (top) | Help Footer (bottom)
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Min(1),    // List area
            Constraint::Length(1), // Help footer
        ])
        .split(area);

    let list_area = chunks[0];
    let help_area = chunks[1];

    let items: Vec<ListItem> = state
        .pending_servers
        .iter()
        .enumerate()
        .map(|(i, server)| {
            let is_selected = i == state.pending_selected;

            let auth_icon = match server.auth_status {
                AuthStatus::NotRequired => "○",
                AuthStatus::TokenPresent => "🔑",
                AuthStatus::TokenMissing => "⚠",
                AuthStatus::Unknown => "?",
            };

            let auth_color = match server.auth_status {
                AuthStatus::NotRequired => theme::GREEN,
                AuthStatus::TokenPresent => theme::GREEN,
                AuthStatus::TokenMissing => theme::YELLOW,
                AuthStatus::Unknown => theme::SUBTEXT0,
            };

            let risk_color = match server.risk_level.as_str() {
                "low" => theme::GREEN,
                "medium" => theme::YELLOW,
                "high" => theme::PEACH,
                "critical" => theme::RED,
                _ => theme::SUBTEXT0,
            };

            let line = Line::from(vec![
                Span::styled(auth_icon, Style::default().fg(auth_color)),
                Span::raw(" "),
                Span::styled(
                    truncate(&server.name, 25),
                    if is_selected {
                        Style::default()
                            .fg(theme::TEXT)
                            .add_modifier(Modifier::BOLD)
                    } else {
                        Style::default().fg(theme::SUBTEXT1)
                    },
                ),
                Span::raw(" "),
                Span::styled(
                    format!("[{}]", server.risk_level),
                    Style::default().fg(risk_color),
                ),
            ]);

            let mut item = ListItem::new(line);
            if is_selected {
                item = item.style(
                    Style::default()
                        .bg(theme::SURFACE0)
                        .add_modifier(Modifier::BOLD),
                );
            }
            item
        })
        .collect();

    let list = List::new(items).block(block);
    f.render_widget(list, list_area);

    // Render help footer
    let help_text = Line::from(vec![
        Span::styled("[a]", Style::default().fg(theme::GREEN)),
        Span::raw(" Approve  "),
        Span::styled("[r]", Style::default().fg(theme::RED)),
        Span::raw(" Reject  "),
        Span::styled("[t]", Style::default().fg(theme::YELLOW)),
        Span::raw(" Token"),
    ]);
    let help_paragraph = Paragraph::new(help_text).alignment(Alignment::Center);
    f.render_widget(help_paragraph, help_area);
}

/// Render details for the selected pending server
fn render_pending_details(f: &mut Frame, state: &AppState, area: Rect) {
    let is_active = state.active_panel == ActivePanel::ApprovalsDetails;
    let border_color = if is_active {
        theme::PANEL_BORDER_ACTIVE
    } else {
        theme::PANEL_BORDER
    };

    let block = Block::default()
        .title("Pending Server Details")
        .borders(Borders::ALL)
        .border_style(Style::default().fg(border_color));

    if state.pending_servers.is_empty() || state.pending_selected >= state.pending_servers.len() {
        let paragraph = Paragraph::new("Select a server to view details")
            .style(Style::default().fg(theme::SUBTEXT0))
            .block(block);
        f.render_widget(paragraph, area);
        return;
    }

    let server = &state.pending_servers[state.pending_selected];

    let auth_status_text = match server.auth_status {
        AuthStatus::NotRequired => ("Not Required", theme::GREEN),
        AuthStatus::TokenPresent => ("Token Present ✓", theme::GREEN),
        AuthStatus::TokenMissing => ("Token Missing ⚠", theme::YELLOW),
        AuthStatus::Unknown => ("Unknown", theme::SUBTEXT0),
    };

    let mut lines = vec![
        Line::from(vec![
            Span::styled("Name:       ", Style::default().fg(theme::SUBTEXT0)),
            Span::styled(
                &server.name,
                Style::default()
                    .fg(theme::TEXT)
                    .add_modifier(Modifier::BOLD),
            ),
        ]),
        Line::from(""),
        Line::from(vec![
            Span::styled("Endpoint:   ", Style::default().fg(theme::SUBTEXT0)),
            Span::styled(&server.endpoint, Style::default().fg(theme::SAPPHIRE)),
        ]),
        Line::from(""),
        Line::from(vec![
            Span::styled("Auth:       ", Style::default().fg(theme::SUBTEXT0)),
            Span::styled(auth_status_text.0, Style::default().fg(auth_status_text.1)),
        ]),
    ];

    if let Some(ref env_var) = server.auth_env_var {
        lines.push(Line::from(vec![
            Span::styled("  Env var:  ", Style::default().fg(theme::SUBTEXT0)),
            Span::styled(env_var, Style::default().fg(theme::PEACH)),
        ]));
    }

    lines.push(Line::from(""));
    lines.push(Line::from(vec![
        Span::styled("Risk:       ", Style::default().fg(theme::SUBTEXT0)),
        Span::styled(
            &server.risk_level,
            Style::default().fg(match server.risk_level.as_str() {
                "low" => theme::GREEN,
                "medium" => theme::YELLOW,
                "high" => theme::PEACH,
                "critical" => theme::RED,
                _ => theme::SUBTEXT0,
            }),
        ),
    ]));

    if let Some(ref desc) = server.description {
        lines.push(Line::from(""));
        lines.push(Line::from(vec![Span::styled(
            "Description:",
            Style::default().fg(theme::SUBTEXT0),
        )]));
        lines.push(Line::from(vec![Span::styled(
            format!("  {}", desc),
            Style::default().fg(theme::TEXT),
        )]));
    }

    if let Some(ref goal) = server.requesting_goal {
        lines.push(Line::from(""));
        lines.push(Line::from(vec![Span::styled(
            "Requested for goal:",
            Style::default().fg(theme::SUBTEXT0),
        )]));
        lines.push(Line::from(vec![Span::styled(
            format!("  \"{}\"", truncate(goal, 50)),
            Style::default().fg(theme::LAVENDER),
        )]));
    }

    lines.push(Line::from(""));
    lines.push(Line::from(""));
    lines.push(Line::from(vec![Span::styled(
        "Actions:",
        Style::default().fg(theme::SUBTEXT0),
    )]));
    lines.push(Line::from(vec![
        Span::styled("  [a] ", Style::default().fg(theme::GREEN)),
        Span::raw("Approve server"),
    ]));
    lines.push(Line::from(vec![
        Span::styled("  [r] ", Style::default().fg(theme::RED)),
        Span::raw("Reject server"),
    ]));
    if server.auth_status == AuthStatus::TokenMissing {
        lines.push(Line::from(vec![
            Span::styled("  [t] ", Style::default().fg(theme::YELLOW)),
            Span::raw("Set auth token"),
        ]));
    }
    lines.push(Line::from(vec![
        Span::styled("  [i] ", Style::default().fg(theme::BLUE)),
        Span::raw("Introspect tools"),
    ]));

    let paragraph = Paragraph::new(lines)
        .block(block)
        .wrap(Wrap { trim: false });
    f.render_widget(paragraph, area);
}

/// Render the approved servers list
fn render_approved_list(f: &mut Frame, state: &AppState, area: Rect) {
    let is_active = state.active_panel == ActivePanel::ApprovalsApprovedList;
    let border_color = if is_active {
        theme::PANEL_BORDER_ACTIVE
    } else {
        theme::PANEL_BORDER
    };

    let block = Block::default()
        .title(format!("Approved [{}]", state.approved_servers.len()))
        .borders(Borders::ALL)
        .border_style(Style::default().fg(border_color));

    if state.approved_servers.is_empty() {
        let content = if state.approvals_loading {
            "Loading approved servers..."
        } else {
            "No approved servers.\n\nApprove pending servers to add them here."
        };
        let paragraph = Paragraph::new(content)
            .style(Style::default().fg(theme::SUBTEXT0))
            .block(block);
        f.render_widget(paragraph, area);
        return;
    }

    let items: Vec<ListItem> = state
        .approved_servers
        .iter()
        .enumerate()
        .map(|(i, server)| {
            let is_selected = i == state.approved_selected;

            let health_icon = if server.error_rate > 0.5 {
                "⚠"
            } else if server.total_calls > 0 {
                "●"
            } else {
                "○"
            };

            let health_color = if server.error_rate > 0.5 {
                theme::RED
            } else if server.total_calls > 0 {
                theme::GREEN
            } else {
                theme::SUBTEXT0
            };

            let tools_str = server
                .tool_count
                .map(|c| format!(" [{}]", c))
                .unwrap_or_default();

            let line = Line::from(vec![
                Span::styled(health_icon, Style::default().fg(health_color)),
                Span::raw(" "),
                Span::styled(
                    truncate(&server.name, 25),
                    if is_selected {
                        Style::default()
                            .fg(theme::TEXT)
                            .add_modifier(Modifier::BOLD)
                    } else {
                        Style::default().fg(theme::SUBTEXT1)
                    },
                ),
                Span::styled(tools_str, Style::default().fg(theme::PEACH)),
            ]);

            let mut item = ListItem::new(line);
            if is_selected {
                item = item.style(
                    Style::default()
                        .bg(theme::SURFACE0)
                        .add_modifier(Modifier::BOLD),
                );
            }
            item
        })
        .collect();

    let list = List::new(items).block(block);
    f.render_widget(list, area);
}

/// Render details for the selected approved server
fn render_approved_details(f: &mut Frame, state: &AppState, area: Rect) {
    let is_active = state.active_panel == ActivePanel::ApprovalsDetails;
    let border_color = if is_active {
        theme::PANEL_BORDER_ACTIVE
    } else {
        theme::PANEL_BORDER
    };

    let block = Block::default()
        .title("Approved Server Details")
        .borders(Borders::ALL)
        .border_style(Style::default().fg(border_color));

    if state.approved_servers.is_empty() || state.approved_selected >= state.approved_servers.len()
    {
        let paragraph = Paragraph::new("Select a server to view details")
            .style(Style::default().fg(theme::SUBTEXT0))
            .block(block);
        f.render_widget(paragraph, area);
        return;
    }

    let server = &state.approved_servers[state.approved_selected];

    let health_text = if server.error_rate > 0.5 {
        ("Unhealthy", theme::RED)
    } else if server.total_calls > 0 {
        ("Healthy", theme::GREEN)
    } else {
        ("No calls yet", theme::SUBTEXT0)
    };

    let mut lines = vec![
        Line::from(vec![
            Span::styled("Name:       ", Style::default().fg(theme::SUBTEXT0)),
            Span::styled(
                &server.name,
                Style::default()
                    .fg(theme::TEXT)
                    .add_modifier(Modifier::BOLD),
            ),
        ]),
        Line::from(""),
        Line::from(vec![
            Span::styled("Endpoint:   ", Style::default().fg(theme::SUBTEXT0)),
            Span::styled(&server.endpoint, Style::default().fg(theme::SAPPHIRE)),
        ]),
        Line::from(""),
        Line::from(vec![
            Span::styled("Health:     ", Style::default().fg(theme::SUBTEXT0)),
            Span::styled(health_text.0, Style::default().fg(health_text.1)),
        ]),
        Line::from(""),
        Line::from(vec![
            Span::styled("Calls:      ", Style::default().fg(theme::SUBTEXT0)),
            Span::styled(
                format!("{}", server.total_calls),
                Style::default().fg(theme::TEXT),
            ),
        ]),
        Line::from(vec![
            Span::styled("Error rate: ", Style::default().fg(theme::SUBTEXT0)),
            Span::styled(
                format!("{:.1}%", server.error_rate * 100.0),
                Style::default().fg(if server.error_rate > 0.5 {
                    theme::RED
                } else if server.error_rate > 0.1 {
                    theme::YELLOW
                } else {
                    theme::GREEN
                }),
            ),
        ]),
        Line::from(""),
        Line::from(vec![
            Span::styled("Approved:   ", Style::default().fg(theme::SUBTEXT0)),
            Span::styled(&server.approved_at, Style::default().fg(theme::TEXT)),
        ]),
    ];

    if let Some(ref desc) = server.description {
        lines.push(Line::from(""));
        lines.push(Line::from(vec![Span::styled(
            "Description:",
            Style::default().fg(theme::SUBTEXT0),
        )]));
        lines.push(Line::from(vec![Span::styled(
            format!("  {}", desc),
            Style::default().fg(theme::TEXT),
        )]));
    }

    lines.push(Line::from(""));
    lines.push(Line::from(""));
    lines.push(Line::from(vec![Span::styled(
        "Actions:",
        Style::default().fg(theme::SUBTEXT0),
    )]));
    lines.push(Line::from(vec![
        Span::styled("  [d] ", Style::default().fg(theme::RED)),
        Span::raw("Dismiss server"),
    ]));
    lines.push(Line::from(vec![
        Span::styled("  [i] ", Style::default().fg(theme::BLUE)),
        Span::raw("Re-introspect tools"),
    ]));

    let paragraph = Paragraph::new(lines)
        .block(block)
        .wrap(Wrap { trim: false });
    f.render_widget(paragraph, area);
}

/// Render the pending budget extensions list
fn render_budget_list(f: &mut Frame, state: &AppState, area: Rect) {
    let is_active = state.active_panel == ActivePanel::ApprovalsBudgetList;
    let border_color = if is_active {
        theme::PANEL_BORDER_ACTIVE
    } else {
        theme::PANEL_BORDER
    };

    let block = Block::default()
        .title(format!("Budget Extensions [{}]", state.budget_approvals.len()))
        .borders(Borders::ALL)
        .border_style(Style::default().fg(border_color));

    if state.budget_approvals.is_empty() {
        let content = if state.approvals_loading {
            "Loading budget approvals..."
        } else {
            "No pending budget extensions."
        };
        let paragraph = Paragraph::new(content)
            .style(Style::default().fg(theme::SUBTEXT0))
            .block(block);
        f.render_widget(paragraph, area);
        return;
    }

    // Layout: List (top) | Help Footer (bottom)
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Min(1),
            Constraint::Length(1),
        ])
        .split(area);

    let list_area = chunks[0];
    let help_area = chunks[1];

    let items: Vec<ListItem> = state
        .budget_approvals
        .iter()
        .enumerate()
        .map(|(i, approval)| {
            let is_selected = i == state.budget_selected;

            let risk_color = match approval.risk_level.as_str() {
                "low" => theme::GREEN,
                "medium" => theme::YELLOW,
                "high" => theme::PEACH,
                "critical" => theme::RED,
                _ => theme::SUBTEXT0,
            };

            let line = Line::from(vec![
                Span::styled("💸", Style::default().fg(theme::PEACH)),
                Span::raw(" "),
                Span::styled(
                    truncate(&approval.dimension, 16),
                    if is_selected {
                        Style::default()
                            .fg(theme::TEXT)
                            .add_modifier(Modifier::BOLD)
                    } else {
                        Style::default().fg(theme::SUBTEXT1)
                    },
                ),
                Span::raw(" "),
                Span::styled(
                    format!("+{:.2}", approval.requested_additional),
                    Style::default().fg(theme::SAPPHIRE),
                ),
                Span::raw(" "),
                Span::styled(
                    format!("[{}]", approval.risk_level),
                    Style::default().fg(risk_color),
                ),
            ]);

            let mut item = ListItem::new(line);
            if is_selected {
                item = item.style(
                    Style::default()
                        .bg(theme::SURFACE0)
                        .add_modifier(Modifier::BOLD),
                );
            }
            item
        })
        .collect();

    let list = List::new(items).block(block);
    f.render_widget(list, list_area);

    let help_text = Line::from(vec![
        Span::styled("[a]", Style::default().fg(theme::GREEN)),
        Span::raw(" Approve  "),
        Span::styled("[r]", Style::default().fg(theme::RED)),
        Span::raw(" Reject"),
    ]);
    let help_paragraph = Paragraph::new(help_text).alignment(Alignment::Center);
    f.render_widget(help_paragraph, help_area);
}

/// Render details for the selected budget extension
fn render_budget_details(f: &mut Frame, state: &AppState, area: Rect) {
    let is_active = state.active_panel == ActivePanel::ApprovalsBudgetDetails;
    let border_color = if is_active {
        theme::PANEL_BORDER_ACTIVE
    } else {
        theme::PANEL_BORDER
    };

    let block = Block::default()
        .title("Budget Extension Details")
        .borders(Borders::ALL)
        .border_style(Style::default().fg(border_color));

    if state.budget_approvals.is_empty() || state.budget_selected >= state.budget_approvals.len() {
        let paragraph = Paragraph::new("Select a budget request to view details")
            .style(Style::default().fg(theme::SUBTEXT0))
            .block(block);
        f.render_widget(paragraph, area);
        return;
    }

    let approval = &state.budget_approvals[state.budget_selected];

    let lines = vec![
        Line::from(vec![
            Span::styled("Dimension:  ", Style::default().fg(theme::SUBTEXT0)),
            Span::styled(
                &approval.dimension,
                Style::default()
                    .fg(theme::TEXT)
                    .add_modifier(Modifier::BOLD),
            ),
        ]),
        Line::from(""),
        Line::from(vec![
            Span::styled("Requested:  ", Style::default().fg(theme::SUBTEXT0)),
            Span::styled(
                format!("{:.2}", approval.requested_additional),
                Style::default().fg(theme::SAPPHIRE),
            ),
        ]),
        Line::from(vec![
            Span::styled("Consumed:   ", Style::default().fg(theme::SUBTEXT0)),
            Span::styled(
                format!("{} / {}", approval.consumed, approval.limit),
                Style::default().fg(theme::TEXT),
            ),
        ]),
        Line::from(""),
        Line::from(vec![
            Span::styled("Plan ID:    ", Style::default().fg(theme::SUBTEXT0)),
            Span::styled(truncate(&approval.plan_id, 40), Style::default().fg(theme::LAVENDER)),
        ]),
        Line::from(vec![
            Span::styled("Intent ID:  ", Style::default().fg(theme::SUBTEXT0)),
            Span::styled(
                truncate(&approval.intent_id, 40),
                Style::default().fg(theme::LAVENDER),
            ),
        ]),
        Line::from(""),
        Line::from(vec![
            Span::styled("Requested:  ", Style::default().fg(theme::SUBTEXT0)),
            Span::styled(&approval.requested_at, Style::default().fg(theme::SUBTEXT1)),
        ]),
        Line::from(""),
        Line::from(vec![Span::styled(
            "Actions:",
            Style::default().fg(theme::SUBTEXT0),
        )]),
        Line::from(vec![
            Span::styled("  [a] ", Style::default().fg(theme::GREEN)),
            Span::raw("Approve extension"),
        ]),
        Line::from(vec![
            Span::styled("  [r] ", Style::default().fg(theme::RED)),
            Span::raw("Reject extension"),
        ]),
    ];

    let paragraph = Paragraph::new(lines)
        .block(block)
        .wrap(Wrap { trim: false });
    f.render_widget(paragraph, area);
}

/// Render the auth token input popup
fn render_auth_token_popup(f: &mut Frame, popup: &super::state::AuthTokenPopup) {
    let area = centered_rect(60, 20, f.size()); // Increased height from 12% to 20%

    // Clear the area
    f.render_widget(ratatui::widgets::Clear, area);

    let block = Block::default()
        .title(format!(" Set Auth Token: {} ", popup.server_name))
        .borders(Borders::ALL)
        .border_style(Style::default().fg(theme::MAUVE));

    let inner = block.inner(area);
    f.render_widget(block, area);

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .margin(1)
        .constraints([
            Constraint::Length(1), // Instruction
            Constraint::Length(1), // Env var name
            Constraint::Length(1), // Spacer
            Constraint::Length(5), // Token input (increased from 3 to 5 for better visibility)
            Constraint::Length(3), // Error or hint (increased to 3 lines for better error visibility)
            Constraint::Min(1),    // Actions
        ])
        .split(inner);

    // Instruction
    let instruction = Paragraph::new("Enter token value (will be set for current session):")
        .style(Style::default().fg(theme::SUBTEXT1));
    f.render_widget(instruction, chunks[0]);

    // Env var name
    let env_var_line = Line::from(vec![
        Span::styled(
            "Environment variable: ",
            Style::default().fg(theme::SUBTEXT0),
        ),
        Span::styled(&popup.env_var, Style::default().fg(theme::PEACH)),
    ]);
    f.render_widget(Paragraph::new(env_var_line), chunks[1]);

    // Token input (masked) - show cursor position indicator
    let masked_token = "*".repeat(popup.token_input.len());
    let input_block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(theme::SAPPHIRE));
    let input = Paragraph::new(masked_token)
        .block(input_block)
        .scroll((0, 0)); // Allow horizontal scrolling if token is very long
    f.render_widget(input, chunks[3]);

    // Error or hint
    if let Some(ref error) = popup.error_message {
        let error_block = Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(theme::RED))
            .style(Style::default().bg(theme::MANTLE).fg(theme::RED));
        let error_paragraph = Paragraph::new(error.as_str())
            .style(Style::default().fg(theme::RED).add_modifier(Modifier::BOLD))
            .wrap(Wrap { trim: true })
            .block(error_block);
        f.render_widget(error_paragraph, chunks[4]);
    } else {
        let hint = Paragraph::new("Token will be stored in memory only (not saved to disk)")
            .style(Style::default().fg(theme::SUBTEXT0))
            .wrap(Wrap { trim: true });
        f.render_widget(hint, chunks[4]);
    }

    // Actions
    let actions = Line::from(vec![
        Span::styled("[Enter] ", Style::default().fg(theme::GREEN)),
        Span::raw("Set token  "),
        Span::styled("[Esc] ", Style::default().fg(theme::RED)),
        Span::raw("Cancel"),
    ]);
    f.render_widget(Paragraph::new(actions), chunks[5]);
}

/// Render the Discover view - capability browser
fn render_discover_view(f: &mut Frame, state: &mut AppState, area: Rect) {
    // Vertical split: Search input (top) | Results (bottom)
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3), // Search bar
            Constraint::Min(1),    // Content
        ])
        .split(area);

    render_discover_input(f, state, chunks[0]);

    // Two-column layout: Capability list (left) | Details (right)
    let cols = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(50), Constraint::Percentage(50)])
        .split(chunks[1]);

    render_capability_list(f, state, cols[0]);
    render_capability_details(f, state, cols[1]);
}

fn render_discover_input(f: &mut Frame, state: &mut AppState, area: Rect) {
    let focus_style = if state.discover_input_active {
        Style::default()
            .fg(theme::MAUVE)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(theme::TEXT)
    };

    let border_style = if state.discover_input_active {
        Style::default().fg(theme::MAUVE)
    } else {
        Style::default().fg(theme::PANEL_BORDER)
    };

    let title = if state.discover_loading {
        "Search / Discovery (Searching...)"
    } else {
        "Search / Discovery (Press Enter to search MCP Registry)"
    };

    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(border_style)
        .title(title);

    let input = Paragraph::new(state.discover_search_hint.as_str())
        .style(focus_style)
        .block(block);

    f.render_widget(input, area);
}

/// Render the capability list from all servers, grouped by source
fn render_capability_list(f: &mut Frame, state: &mut AppState, area: Rect) {
    // Calculate visible height (accounting for borders)
    let visible_height = area.height.saturating_sub(2) as usize;
    state.discover_panel_height = visible_height;

    // Apply local filtering based on the search hint
    let filtered_caps = state.filtered_discovered_caps();
    let total_count = filtered_caps.len();

    let block_style = if state.active_panel == ActivePanel::DiscoverList {
        Style::default().fg(theme::MAUVE)
    } else {
        Style::default().fg(theme::PANEL_BORDER)
    };

    let visible_entries = state.visible_discovery_entries();
    let entry_count = visible_entries.len();

    // Show scroll indicator in title if needed
    let title = if entry_count > visible_height {
        format!(
            "Capabilities ({} found) [{}/{}]",
            total_count,
            state.discover_scroll + 1,
            entry_count
        )
    } else {
        format!("Capabilities ({} found)", total_count)
    };

    let block = Block::default()
        .title(title)
        .borders(Borders::ALL)
        .border_style(block_style);

    let mut items: Vec<ListItem> = Vec::new();

    // Apply scrolling - skip items before scroll offset and take only visible_height items
    for (display_idx, (_, entry)) in visible_entries
        .iter()
        .enumerate()
        .skip(state.discover_scroll)
        .take(visible_height)
        .enumerate()
    {
        let real_idx = display_idx + state.discover_scroll;
        let is_selected = real_idx == state.discover_selected;

        match entry {
            DiscoveryEntry::Header { name, is_local } => {
                // Determine collapsed state matching visible_discovery_entries() logic
                let is_collapsed = if *is_local || name == "Local Capabilities" {
                    state.discover_local_collapsed
                        || state.discover_collapsed_sources.contains(name)
                } else {
                    // Non-local: collapsed if explicitly collapsed OR (all_collapsed_by_default AND not explicitly expanded)
                    state.discover_collapsed_sources.contains(name)
                        || (state.discover_all_collapsed_by_default
                            && !state.discover_expanded_sources.contains(name))
                };
                let indicator = if is_collapsed { "▶" } else { "▼" };

                let style = if is_selected {
                    Style::default()
                        .fg(theme::MAUVE)
                        .add_modifier(Modifier::BOLD | Modifier::REVERSED)
                } else {
                    Style::default()
                        .fg(if name == "Local Capabilities" {
                            theme::SAPPHIRE
                        } else {
                            theme::TEAL
                        })
                        .add_modifier(Modifier::BOLD)
                };

                items.push(ListItem::new(Line::from(vec![Span::styled(
                    format!("{} {}", indicator, name),
                    style,
                )])));
            }
            DiscoveryEntry::Capability(idx) => {
                if let Some((_, cap)) = filtered_caps.get(*idx) {
                    let style = if is_selected {
                        Style::default()
                            .fg(theme::MAUVE)
                            .add_modifier(Modifier::BOLD)
                    } else {
                        Style::default().fg(theme::TEXT)
                    };
                    let prefix = if is_selected { "► " } else { "  " };
                    let category_icon = match cap.category {
                        CapabilityCategory::McpTool => "🔧",
                        CapabilityCategory::OpenApiTool => "📡",
                        CapabilityCategory::BrowserApiTool => "🌐",
                        CapabilityCategory::RtfsFunction => "λ",
                        CapabilityCategory::Builtin => "⚙️",
                        CapabilityCategory::Synthesized => "✨",
                    };
                    items.push(ListItem::new(Line::from(vec![
                        Span::styled(prefix, style),
                        Span::styled(
                            format!("{} ", category_icon),
                            Style::default().fg(theme::PEACH),
                        ),
                        Span::styled(&cap.name, style),
                    ])));
                }
            }
        }
    }
    // If no items, show empty state
    if items.is_empty() {
        if !state.discover_search_hint.is_empty() {
            items.push(ListItem::new(Line::from(vec![Span::styled(
                "  No matching capabilities found",
                Style::default().fg(theme::SUBTEXT0),
            )])));
        } else {
            items.push(ListItem::new(Line::from(vec![Span::styled(
                "  No capabilities discovered yet",
                Style::default().fg(theme::SUBTEXT0),
            )])));
            items.push(ListItem::new(Line::from("")));
            items.push(ListItem::new(Line::from(vec![
                Span::styled("  Press ", Style::default().fg(theme::SUBTEXT0)),
                Span::styled("Enter", Style::default().fg(theme::TEAL)),
                Span::styled(" to search registry", Style::default().fg(theme::SUBTEXT0)),
            ])));
        }
    }

    let list = List::new(items).block(block);
    f.render_widget(list, area);
}

/// Render capability details panel
fn render_capability_details(f: &mut Frame, state: &mut AppState, area: Rect) {
    let is_focused = state.active_panel == ActivePanel::DiscoverDetails;
    let border_style = if is_focused {
        Style::default().fg(theme::MAUVE)
    } else {
        Style::default().fg(theme::PANEL_BORDER)
    };

    let block = Block::default()
        .title("Capability Details")
        .borders(Borders::ALL)
        .border_style(border_style);

    let filtered_caps = state.filtered_discovered_caps();

    // Get the currently selected capability's details
    let visible_entries = state.visible_discovery_entries();
    let selected_entry = visible_entries.get(state.discover_selected);

    let lines = if let Some(DiscoveryEntry::Capability(idx)) = selected_entry {
        if let Some((_, cap)) = filtered_caps.get(*idx) {
            let mut lines = vec![
                Line::from(vec![Span::styled(
                    "Name:     ",
                    Style::default().fg(theme::SUBTEXT0),
                )]),
                Line::from(vec![Span::styled(
                    &cap.name,
                    Style::default()
                        .fg(theme::TEXT)
                        .add_modifier(Modifier::BOLD),
                )]),
                Line::from(""),
                Line::from(vec![
                    Span::styled("Source:   ", Style::default().fg(theme::SUBTEXT0)),
                    Span::styled(&cap.source, Style::default().fg(theme::TEAL)),
                ]),
                Line::from(""),
                Line::from(vec![
                    Span::styled("Category: ", Style::default().fg(theme::SUBTEXT0)),
                    Span::styled(
                        format!("{:?}", cap.category),
                        Style::default().fg(theme::PEACH),
                    ),
                ]),
                Line::from(""),
                Line::from(vec![Span::styled(
                    "Description:",
                    Style::default().fg(theme::SUBTEXT0),
                )]),
                Line::from(vec![Span::styled(
                    &cap.description,
                    Style::default().fg(theme::TEXT),
                )]),
            ];

            if let Some(schema) = &cap.input_schema {
                lines.push(Line::from(""));
                lines.push(Line::from(vec![Span::styled(
                    "Input Schema:",
                    Style::default().fg(theme::SUBTEXT0),
                )]));
                lines.extend(format_schema_content(schema));
            }

            if let Some(schema) = &cap.output_schema {
                lines.push(Line::from(""));
                lines.push(Line::from(vec![Span::styled(
                    "Output Schema:",
                    Style::default().fg(theme::SUBTEXT0),
                )]));
                lines.extend(format_schema_content(schema));
            }

            lines
        } else {
            vec![Line::from(vec![Span::styled(
                "Select a capability to view details",
                Style::default().fg(theme::SUBTEXT0),
            )])]
        }
    } else if let Some(DiscoveryEntry::Header { name, .. }) = selected_entry {
        vec![
            Line::from(vec![
                Span::styled("Source:   ", Style::default().fg(theme::SUBTEXT0)),
                Span::styled(
                    name,
                    Style::default()
                        .fg(theme::TEXT)
                        .add_modifier(Modifier::BOLD),
                ),
            ]),
            Line::from(""),
            Line::from(vec![Span::styled(
                "Press 'c' or 'Space' to toggle collapse",
                Style::default().fg(theme::SUBTEXT0),
            )]),
        ]
    } else {
        vec![Line::from(vec![Span::styled(
            "Select a capability to view details",
            Style::default().fg(theme::SUBTEXT0),
        )])]
    };

    let paragraph = Paragraph::new(lines)
        .block(block)
        .wrap(Wrap { trim: false })
        .scroll((state.discover_details_scroll as u16, 0));
    f.render_widget(paragraph, area);
}

/// Helper to format schema content for display
fn format_schema_content(schema_str: &str) -> Vec<Line<'static>> {
    // Try to parse as JSON
    if let Ok(json) = serde_json::from_str::<serde_json::Value>(schema_str) {
        // Check if it's an object with properties (like a JSON schema)
        if let Some(obj) = json.as_object() {
            if let Some(props) = obj.get("properties").and_then(|p| p.as_object()) {
                let mut lines = Vec::new();

                // If there's a type, show it
                if let Some(type_val) = obj.get("type").and_then(|t| t.as_str()) {
                    lines.push(Line::from(vec![
                        Span::styled("Type: ", Style::default().fg(theme::SUBTEXT0)),
                        Span::styled(type_val.to_string(), Style::default().fg(theme::TEXT)),
                    ]));
                }

                lines.push(Line::from(vec![Span::styled(
                    "Properties:",
                    Style::default().fg(theme::SUBTEXT0),
                )]));

                for (key, value) in props {
                    let mut spans = vec![
                        Span::raw("  - "),
                        Span::styled(
                            key.to_string(),
                            Style::default()
                                .fg(theme::BLUE)
                                .add_modifier(Modifier::BOLD),
                        ),
                    ];

                    // Try to get type of property
                    if let Some(prop_obj) = value.as_object() {
                        if let Some(prop_type) = prop_obj.get("type").and_then(|t| t.as_str()) {
                            spans.push(Span::styled(": ", Style::default().fg(theme::SUBTEXT0)));
                            spans.push(Span::styled(
                                prop_type.to_string(),
                                Style::default().fg(theme::YELLOW),
                            ));
                        }

                        // Check if required
                        if let Some(req) = obj.get("required").and_then(|r| r.as_array()) {
                            if req.iter().any(|r| r.as_str() == Some(key)) {
                                spans.push(Span::raw(" "));
                                spans.push(Span::styled("*", Style::default().fg(theme::RED)));
                            }
                        }

                        // Description
                        if let Some(desc) = prop_obj.get("description").and_then(|d| d.as_str()) {
                            spans.push(Span::raw("  "));
                            spans.push(Span::styled(
                                format!("({})", desc),
                                Style::default()
                                    .fg(theme::SUBTEXT0)
                                    .add_modifier(Modifier::ITALIC),
                            ));
                        }
                    }

                    lines.push(Line::from(spans));
                }

                return lines;
            }
        }

        // Fallback to pretty printed JSON for other structures
        if let Ok(pretty) = serde_json::to_string_pretty(&json) {
            return pretty
                .lines()
                .map(|l| {
                    Line::from(vec![Span::styled(
                        l.to_string(),
                        Style::default().fg(theme::TEXT),
                    )])
                })
                .collect();
        }
    }

    // Fallback: assume RTFS format and apply syntax highlighting
    schema_str
        .lines()
        .map(|l| {
            let mut spans = Vec::new();

            // Handle comments
            let (code, comment) = if let Some(idx) = l.find(";;") {
                (&l[0..idx], Some(&l[idx..]))
            } else {
                (l, None)
            };

            // Basic tokenizer for code part
            let mut last_idx = 0;
            // Split by typical delimiters and whitespace for RTFS/Clojure-like syntax
            // We want to preserve delimiters like [ ] { }

            // Function to classify token color
            let get_token_style = |token: &str| -> Style {
                if token.starts_with(':') {
                    // Check for known types vs keys
                    // Strip optional suffix for checking
                    let base_token = token.strip_suffix('?').unwrap_or(token);

                    match base_token {
                        ":string" | ":int" | ":float" | ":bool" | ":nil" | ":any" | ":never"
                        | ":vector" | ":map" | ":set" | ":list" => {
                            Style::default().fg(theme::YELLOW)
                        }
                        _ => Style::default().fg(theme::BLUE), // Keywords/Keys
                    }
                } else if token == "{" || token == "}" || token == "[" || token == "]" {
                    Style::default().fg(theme::TEXT) // Brackets
                } else {
                    Style::default().fg(theme::TEXT)
                }
            };

            // Tokenize by splitting on whitespace but handling brackets
            for (idx, char) in code.char_indices() {
                if char.is_whitespace() || "[]{}".contains(char) {
                    // Flush previous token if any
                    if idx > last_idx {
                        let token = &code[last_idx..idx];
                        spans.push(Span::styled(token.to_string(), get_token_style(token)));
                    }

                    // Add the delimiter/whitespace itself
                    let delimiter = &code[idx..idx + 1];
                    // Whitespace is just styled as text (invisible mostly), brackets handled
                    spans.push(Span::styled(
                        delimiter.to_string(),
                        get_token_style(delimiter),
                    ));

                    last_idx = idx + 1;
                }
            }
            // Flush remaining code
            if last_idx < code.len() {
                let token = &code[last_idx..];
                spans.push(Span::styled(token.to_string(), get_token_style(token)));
            }

            // Add comment if present
            if let Some(c) = comment {
                spans.push(Span::styled(
                    c.to_string(),
                    Style::default()
                        .fg(theme::SUBTEXT0)
                        .add_modifier(Modifier::ITALIC),
                ));
            }

            Line::from(spans)
        })
        .collect()
}

/// Render the Goals view with RTFS Plan, Decomposition Tree, etc.
fn render_goals_view(f: &mut Frame, state: &mut AppState, area: Rect) {
    // Two-column layout: RTFS Plan (left) | Tree + Resolution (right)
    let cols = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(45), Constraint::Percentage(55)])
        .split(area);

    // Left column: RTFS Plan (full height)
    render_rtfs_plan(f, state, cols[0]);

    // Right column: split into 4 panels (2x2)
    let right_rows = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Percentage(50), Constraint::Percentage(50)])
        .split(cols[1]);

    // Top right: Decomposition Tree | Capability Resolution
    let top_cols = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(50), Constraint::Percentage(50)])
        .split(right_rows[0]);

    render_decomposition_tree(f, state, top_cols[0]);
    render_capability_resolution(f, state, top_cols[1]);

    // Bottom right: Trace Timeline | LLM Inspector
    let bottom_cols = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(50), Constraint::Percentage(50)])
        .split(right_rows[1]);

    render_trace_timeline(f, state, bottom_cols[0]);
    render_llm_inspector(f, state, bottom_cols[1]);
}

/// Render the RTFS Plan panel
fn render_rtfs_plan(f: &mut Frame, state: &mut AppState, area: Rect) {
    let is_active = state.active_panel == ActivePanel::RtfsPlan;
    let border_color = if is_active {
        theme::PANEL_BORDER_ACTIVE
    } else {
        theme::PANEL_BORDER
    };

    let line_count = state
        .rtfs_plan
        .as_ref()
        .map(|p| p.lines().count())
        .unwrap_or(0);
    let title = if line_count > 0 {
        format!("RTFS Plan [{} lines]", line_count)
    } else {
        "RTFS Plan".to_string()
    };

    let block = Block::default()
        .title(title)
        .borders(Borders::ALL)
        .border_style(Style::default().fg(border_color));

    match &state.rtfs_plan {
        Some(plan) => {
            // Get visible lines based on scroll position
            let visible_height = area.height.saturating_sub(2) as usize; // Account for borders
            let lines: Vec<Line> = plan
                .lines()
                .skip(state.rtfs_plan_scroll)
                .take(visible_height)
                .map(|line| {
                    // Basic syntax highlighting for RTFS
                    let styled_line = highlight_rtfs_line(line);
                    Line::from(styled_line)
                })
                .collect();

            let paragraph = Paragraph::new(lines)
                .block(block)
                .wrap(Wrap { trim: false });
            f.render_widget(paragraph, area);
        }
        None => {
            let placeholder = Paragraph::new("Plan will appear here after processing...")
                .style(Style::default().fg(theme::SUBTEXT0))
                .block(block);
            f.render_widget(placeholder, area);
        }
    }
}

/// Basic syntax highlighting for RTFS code
fn highlight_rtfs_line(line: &str) -> Vec<Span<'static>> {
    let trimmed = line.trim_start();
    let indent = " ".repeat(line.len() - trimmed.len());

    // Keywords
    let keywords = [
        "def", "defn", "let", "if", "cond", "match", "do", "fn", "loop", "for",
    ];

    // Check for comments
    if trimmed.starts_with(';') || trimmed.starts_with(";;") {
        return vec![Span::styled(
            line.to_string(),
            Style::default().fg(theme::SUBTEXT0),
        )];
    }

    // Check for keywords at start of expression
    if trimmed.starts_with('(') {
        let rest = trimmed.trim_start_matches('(');
        for keyword in keywords {
            if rest.starts_with(keyword)
                && rest[keyword.len()..].starts_with(|c: char| c.is_whitespace() || c == ')')
            {
                let before = format!("{}(", indent);
                let after = rest[keyword.len()..].to_string();
                return vec![
                    Span::styled(before, Style::default().fg(theme::TEXT)),
                    Span::styled(
                        keyword.to_string(),
                        Style::default()
                            .fg(theme::MAUVE)
                            .add_modifier(Modifier::BOLD),
                    ),
                    Span::styled(after, Style::default().fg(theme::TEXT)),
                ];
            }
        }
    }

    // Check for capability calls (symbols with dots)
    if trimmed.contains('.') {
        // Simple approach: highlight capability names
        let colored_line = line.to_string();
        return vec![Span::styled(colored_line, Style::default().fg(theme::TEXT))];
    }

    // Default: plain text
    vec![Span::styled(
        line.to_string(),
        Style::default().fg(theme::TEXT),
    )]
}

/// Render the decomposition tree panel
fn render_decomposition_tree(f: &mut Frame, state: &mut AppState, area: Rect) {
    let is_active = state.active_panel == ActivePanel::DecompositionTree;
    let border_color = if is_active {
        theme::PANEL_BORDER_ACTIVE
    } else {
        theme::PANEL_BORDER
    };

    let block = Block::default()
        .title("Decomposition Tree")
        .borders(Borders::ALL)
        .border_style(Style::default().fg(border_color));

    if state.decomp_nodes.is_empty() {
        let placeholder = Paragraph::new("No decomposition yet...")
            .style(Style::default().fg(theme::SUBTEXT0))
            .block(block);
        f.render_widget(placeholder, area);
        return;
    }

    let items: Vec<ListItem> = state
        .decomp_nodes
        .iter()
        .enumerate()
        .map(|(i, node)| {
            let indent = "  ".repeat(node.depth);
            let prefix = if node.depth == 0 {
                "ROOT".to_string()
            } else {
                format!("[{}]", i)
            };

            let (status_icon, status_color) = match &node.status {
                NodeStatus::Pending => ("○", theme::SUBTEXT0),
                NodeStatus::Resolving => ("◐", theme::BLUE),
                NodeStatus::Resolved { .. } => ("✓", theme::GREEN),
                NodeStatus::Synthesizing => ("⚡", theme::PEACH),
                NodeStatus::Failed { .. } => ("✗", theme::RED),
                NodeStatus::UserInput => ("?", theme::YELLOW),
            };

            let is_selected = i == state.decomp_selected;
            let style = if is_selected {
                Style::default()
                    .fg(theme::TEXT)
                    .add_modifier(Modifier::REVERSED)
            } else {
                Style::default().fg(theme::TEXT)
            };

            // Build owned parts
            let prefix_part = format!("{}{} ", indent, prefix);
            let desc_part = format!(" {}", truncate(&node.description, 40));

            ListItem::new(Line::from(vec![
                Span::styled(prefix_part, style),
                Span::styled(status_icon, Style::default().fg(status_color)),
                Span::styled(desc_part, style),
            ]))
        })
        .collect();

    let list = List::new(items).block(block);
    f.render_widget(list, area);
}

/// Render the capability resolution panel
fn render_capability_resolution(f: &mut Frame, state: &mut AppState, area: Rect) {
    let is_active = state.active_panel == ActivePanel::CapabilityResolution;
    let border_color = if is_active {
        theme::PANEL_BORDER_ACTIVE
    } else {
        theme::PANEL_BORDER
    };

    let block = Block::default()
        .title("Capability Resolution")
        .borders(Borders::ALL)
        .border_style(Style::default().fg(border_color));

    if state.resolutions.is_empty() {
        let placeholder = Paragraph::new("No resolutions yet...")
            .style(Style::default().fg(theme::SUBTEXT0))
            .block(block);
        f.render_widget(placeholder, area);
        return;
    }

    let items: Vec<ListItem> = state
        .resolutions
        .iter()
        .enumerate()
        .map(|(i, res)| {
            let is_selected = i == state.resolution_selected;
            let style = if is_selected {
                Style::default().add_modifier(Modifier::REVERSED)
            } else {
                Style::default()
            };

            let score_text = if let Some(embed) = res.embed_score {
                format!(" ({:.2})", embed)
            } else {
                String::new()
            };

            let lines = vec![
                Line::from(vec![
                    Span::styled("▶ ", Style::default().fg(theme::MAUVE)),
                    Span::styled(
                        res.capability_name.clone(),
                        Style::default().fg(theme::GREEN),
                    ),
                    Span::styled(score_text, Style::default().fg(theme::SUBTEXT1)),
                ]),
                Line::from(vec![
                    Span::styled("  └─ ", Style::default().fg(theme::SURFACE2)),
                    Span::styled(
                        format!("{}", res.source),
                        Style::default().fg(theme::SUBTEXT0),
                    ),
                ]),
            ];

            ListItem::new(lines).style(style)
        })
        .collect();

    let list = List::new(items).block(block);
    f.render_widget(list, area);
}

/// Render the trace timeline panel
fn render_trace_timeline(f: &mut Frame, state: &mut AppState, area: Rect) {
    let is_active = state.active_panel == ActivePanel::TraceTimeline;
    let border_color = if is_active {
        theme::PANEL_BORDER_ACTIVE
    } else {
        theme::PANEL_BORDER
    };

    // Filter entries based on verbose mode
    let filtered_entries: Vec<_> = state
        .trace_entries
        .iter()
        .filter(|e| state.verbose_trace || e.event_type.is_important())
        .collect();

    let elapsed = state.elapsed_secs();
    let verbose_indicator = if state.verbose_trace {
        " [VERBOSE]"
    } else {
        ""
    };
    let title = if elapsed > 0.0 {
        format!(
            "Trace Timeline ({:.1}s) [{} events]{}",
            elapsed,
            filtered_entries.len(),
            verbose_indicator
        )
    } else {
        format!("Trace Timeline{}", verbose_indicator)
    };

    let block = Block::default()
        .title(title)
        .borders(Borders::ALL)
        .border_style(Style::default().fg(border_color));

    if filtered_entries.is_empty() {
        let msg = if state.verbose_trace {
            "No events yet..."
        } else {
            "No important events yet... (press 'v' for all)"
        };
        let placeholder = Paragraph::new(msg)
            .style(Style::default().fg(theme::SUBTEXT0))
            .block(block);
        f.render_widget(placeholder, area);
        return;
    }

    let items: Vec<ListItem> = filtered_entries
        .iter()
        .rev()
        .take(50)
        .enumerate()
        .map(|(display_idx, entry)| {
            let is_selected = is_active && display_idx == state.trace_selected;

            let time_offset = state
                .start_time
                .map(|s| entry.timestamp.duration_since(s).as_secs_f64())
                .unwrap_or(0.0);

            let event_color = match entry.event_type {
                TraceEventType::DecompositionStart | TraceEventType::DecompositionComplete => {
                    theme::BLUE
                }
                TraceEventType::ToolDiscovery => theme::TEAL,
                TraceEventType::LlmCall => theme::MAUVE,
                TraceEventType::ResolutionStart | TraceEventType::ResolutionComplete => {
                    theme::GREEN
                }
                TraceEventType::ResolutionFailed => theme::RED,
                TraceEventType::SynthesisTriggered => theme::PEACH,
                TraceEventType::LearningApplied => theme::LAVENDER,
                TraceEventType::Error => theme::RED,
                TraceEventType::Info => theme::SUBTEXT1,
            };

            // Smarter truncation: for long IDs, show end instead of beginning
            let truncated_msg = smart_truncate(&entry.message, 60);

            let base_style = if is_selected {
                Style::default().add_modifier(Modifier::REVERSED)
            } else {
                Style::default()
            };

            ListItem::new(Line::from(vec![
                Span::styled(
                    format!("{:5.1}s ", time_offset),
                    base_style.fg(theme::SUBTEXT0),
                ),
                Span::styled(
                    format!("{} ", entry.event_type.icon()),
                    base_style.fg(event_color),
                ),
                Span::styled(truncated_msg, base_style.fg(theme::TEXT)),
            ]))
        })
        .collect();

    let list = List::new(items).block(block);
    f.render_widget(list, area);
}

/// Smart truncation: for UUIDs and long IDs, show the end instead of the beginning
/// Uses char-based slicing to avoid Unicode panics
fn smart_truncate(s: &str, max_len: usize) -> String {
    let char_count = s.chars().count();
    if char_count <= max_len {
        return s.to_string();
    }

    // Check if this looks like it contains a UUID (has multiple hyphens in a row pattern)
    let has_uuid_pattern = s.matches('-').count() >= 4;

    if has_uuid_pattern {
        // For UUID-containing strings, show "...last_portion"
        let suffix_len = max_len.saturating_sub(3);
        let start_idx = char_count.saturating_sub(suffix_len);
        let suffix: String = s.chars().skip(start_idx).collect();
        format!("...{}", suffix)
    } else {
        // Regular truncation - take first chars
        let prefix_len = max_len.saturating_sub(3);
        let prefix: String = s.chars().take(prefix_len).collect();
        format!("{}...", prefix)
    }
}

/// Render the LLM inspector panel
fn render_llm_inspector(f: &mut Frame, state: &mut AppState, area: Rect) {
    let is_active = state.active_panel == ActivePanel::LlmInspector;
    let border_color = if is_active {
        theme::PANEL_BORDER_ACTIVE
    } else {
        theme::PANEL_BORDER
    };

    let title = if state.llm_history.is_empty() {
        "LLM Inspector".to_string()
    } else {
        format!(
            "LLM Inspector [{}/{}]",
            state.llm_selected + 1,
            state.llm_history.len()
        )
    };

    let block = Block::default()
        .title(title)
        .borders(Borders::ALL)
        .border_style(Style::default().fg(border_color));

    if state.llm_history.is_empty() {
        let placeholder = Paragraph::new("No LLM calls yet...")
            .style(Style::default().fg(theme::SUBTEXT0))
            .block(block);
        f.render_widget(placeholder, area);
        return;
    }

    let current = &state.llm_history[state.llm_selected];

    // Split into prompt and response sections
    let inner = block.inner(area);
    let sections = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1),      // Header
            Constraint::Percentage(50), // Prompt
            Constraint::Percentage(50), // Response
        ])
        .split(inner);

    f.render_widget(block, area);

    // Header with model info
    let header = Paragraph::new(Line::from(vec![
        Span::styled("Model: ", Style::default().fg(theme::SUBTEXT0)),
        Span::styled(&current.model, Style::default().fg(theme::MAUVE)),
        Span::styled(
            format!(
                " | {}ms | {} tok",
                current.duration_ms,
                current.tokens_prompt + current.tokens_response
            ),
            Style::default().fg(theme::SUBTEXT0),
        ),
    ]));
    f.render_widget(header, sections[0]);

    // Prompt section
    let prompt_block = Block::default()
        .title("Prompt")
        .borders(Borders::TOP)
        .border_style(Style::default().fg(theme::SURFACE1));

    let prompt_lines: Vec<Line> = current
        .prompt
        .lines()
        .skip(state.llm_prompt_scroll)
        .take(sections[1].height as usize)
        .map(|l| {
            Line::from(Span::styled(
                truncate(l, sections[1].width as usize - 2),
                Style::default().fg(theme::SKY),
            ))
        })
        .collect();

    let prompt = Paragraph::new(prompt_lines)
        .block(prompt_block)
        .wrap(Wrap { trim: false });
    f.render_widget(prompt, sections[1]);

    // Response section
    let response_block = Block::default()
        .title("Response")
        .borders(Borders::TOP)
        .border_style(Style::default().fg(theme::SURFACE1));

    let response_text = current.response.as_deref().unwrap_or("(pending...)");
    let response_lines: Vec<Line> = response_text
        .lines()
        .skip(state.llm_response_scroll)
        .take(sections[2].height as usize)
        .map(|l| {
            Line::from(Span::styled(
                truncate(l, sections[2].width as usize - 2),
                Style::default().fg(theme::TEAL),
            ))
        })
        .collect();

    let response = Paragraph::new(response_lines)
        .block(response_block)
        .wrap(Wrap { trim: false });
    f.render_widget(response, sections[2]);
}

/// Render the status bar
fn render_status_bar(f: &mut Frame, state: &mut AppState, area: Rect) {
    let mode_style = match state.mode {
        ExecutionMode::Idle => Style::default().fg(theme::SUBTEXT0).bg(theme::SURFACE0),
        ExecutionMode::Received => Style::default().fg(theme::BASE).bg(theme::TEAL),
        ExecutionMode::Planning => Style::default().fg(theme::BASE).bg(theme::BLUE),
        ExecutionMode::Executing => Style::default().fg(theme::BASE).bg(theme::YELLOW),
        ExecutionMode::Complete => Style::default().fg(theme::BASE).bg(theme::GREEN),
        ExecutionMode::Error => Style::default().fg(theme::BASE).bg(theme::RED),
    };

    // Build mode text with optional spinner
    let mode_text = if state.is_running() {
        format!(" {} {} ", state.spinner_icon(), state.mode)
    } else {
        format!(" {} ", state.mode)
    };

    let mut spans = vec![
        Span::styled(mode_text, mode_style),
        Span::styled(" │ ", Style::default().fg(theme::SURFACE1)),
        Span::styled(
            format!("Caps: {} ", state.capability_count),
            Style::default().fg(theme::TEXT),
        ),
        Span::styled("│ ", Style::default().fg(theme::SURFACE1)),
        Span::styled(
            format!("Patterns: {} ", state.learning_patterns_count),
            Style::default().fg(theme::TEXT),
        ),
    ];

    // Add discovery status if in Discover view or loading
    if state.current_view == View::Discover {
        spans.push(Span::styled("│ ", Style::default().fg(theme::SURFACE1)));
        if state.discover_loading {
            spans.push(Span::styled(
                format!("{} Searching...", state.spinner_icon()),
                Style::default().fg(theme::TEAL),
            ));
        } else {
            spans.push(Span::styled(
                format!("Found: {} ", state.discovered_capabilities.len()),
                Style::default().fg(theme::GREEN),
            ));
        }
    } else if state.servers_loading {
        spans.push(Span::styled("│ ", Style::default().fg(theme::SURFACE1)));
        spans.push(Span::styled(
            format!("{} Loading Servers...", state.spinner_icon()),
            Style::default().fg(theme::BLUE),
        ));
    }

    spans.push(Span::styled("│ ", Style::default().fg(theme::SURFACE1)));
    spans.push(Span::styled("Press ", Style::default().fg(theme::SUBTEXT0)));
    spans.push(Span::styled("?", Style::default().fg(theme::MAUVE)));
    spans.push(Span::styled(
        " for help",
        Style::default().fg(theme::SUBTEXT0),
    ));

    let status = Paragraph::new(Line::from(spans)).style(Style::default().bg(theme::MANTLE));

    f.render_widget(status, area);
}

/// Render help overlay
fn render_help_overlay(f: &mut Frame) {
    let area = f.size();
    let popup_area = centered_rect(60, 60, area);

    let help_text = vec![
        Line::from(vec![Span::styled(
            "Keyboard Shortcuts",
            Style::default()
                .fg(theme::MAUVE)
                .add_modifier(Modifier::BOLD),
        )]),
        Line::from(""),
        Line::from(vec![
            Span::styled("Tab        ", Style::default().fg(theme::GREEN)),
            Span::styled("Next panel", Style::default().fg(theme::TEXT)),
        ]),
        Line::from(vec![
            Span::styled("Shift+Tab  ", Style::default().fg(theme::GREEN)),
            Span::styled("Previous panel", Style::default().fg(theme::TEXT)),
        ]),
        Line::from(vec![
            Span::styled("↑/↓ j/k    ", Style::default().fg(theme::GREEN)),
            Span::styled("Navigate within panel", Style::default().fg(theme::TEXT)),
        ]),
        Line::from(vec![
            Span::styled("PgUp/PgDn  ", Style::default().fg(theme::GREEN)),
            Span::styled("Navigate by 10 items", Style::default().fg(theme::TEXT)),
        ]),
        Line::from(vec![
            Span::styled("Home/End   ", Style::default().fg(theme::GREEN)),
            Span::styled("Jump to start/end", Style::default().fg(theme::TEXT)),
        ]),
        Line::from(vec![
            Span::styled("Enter      ", Style::default().fg(theme::GREEN)),
            Span::styled(
                "Execute goal / Show details",
                Style::default().fg(theme::TEXT),
            ),
        ]),
        Line::from(vec![
            Span::styled("Esc        ", Style::default().fg(theme::GREEN)),
            Span::styled("Cancel / Close popup", Style::default().fg(theme::TEXT)),
        ]),
        Line::from(vec![
            Span::styled("v          ", Style::default().fg(theme::GREEN)),
            Span::styled(
                "Toggle verbose trace events",
                Style::default().fg(theme::TEXT),
            ),
        ]),
        Line::from(vec![
            Span::styled("?          ", Style::default().fg(theme::GREEN)),
            Span::styled("Toggle this help", Style::default().fg(theme::TEXT)),
        ]),
        Line::from(vec![
            Span::styled("q          ", Style::default().fg(theme::GREEN)),
            Span::styled("Quit", Style::default().fg(theme::TEXT)),
        ]),
        Line::from(""),
        Line::from(vec![Span::styled(
            "Discovery View",
            Style::default()
                .fg(theme::MAUVE)
                .add_modifier(Modifier::BOLD),
        )]),
        Line::from(vec![
            Span::styled("/ or f     ", Style::default().fg(theme::GREEN)),
            Span::styled("Search / Find Servers", Style::default().fg(theme::TEXT)),
        ]),
        Line::from(vec![
            Span::styled("Shift+S    ", Style::default().fg(theme::GREEN)),
            Span::styled("Refresh server schema", Style::default().fg(theme::TEXT)),
        ]),
        Line::from(vec![
            Span::styled("Space      ", Style::default().fg(theme::GREEN)),
            Span::styled("Toggle group collapse", Style::default().fg(theme::TEXT)),
        ]),
        Line::from(""),
        Line::from(vec![Span::styled(
            "Approvals View",
            Style::default()
                .fg(theme::MAUVE)
                .add_modifier(Modifier::BOLD),
        )]),
        Line::from(vec![
            Span::styled("[ / ]      ", Style::default().fg(theme::GREEN)),
            Span::styled(
                "Switch Pending / Approved tabs",
                Style::default().fg(theme::TEXT),
            ),
        ]),
        Line::from(vec![
            Span::styled("a / r      ", Style::default().fg(theme::GREEN)),
            Span::styled("Approve / Reject server", Style::default().fg(theme::TEXT)),
        ]),
        Line::from(vec![
            Span::styled("t          ", Style::default().fg(theme::GREEN)),
            Span::styled("Set auth token", Style::default().fg(theme::TEXT)),
        ]),
        Line::from(vec![
            Span::styled("d          ", Style::default().fg(theme::GREEN)),
            Span::styled("Dismiss approved server", Style::default().fg(theme::TEXT)),
        ]),
    ];

    let block = Block::default()
        .title("Help")
        .borders(Borders::ALL)
        .border_style(Style::default().fg(theme::MAUVE))
        .style(Style::default().bg(theme::BASE));

    let help = Paragraph::new(help_text).block(block);

    // Clear the area first
    f.render_widget(ratatui::widgets::Clear, popup_area);
    f.render_widget(help, popup_area);
}

/// Render trace detail popup
fn render_trace_popup(f: &mut Frame, state: &mut AppState) {
    let area = f.size();
    let popup_area = centered_rect(80, 70, area);

    // Get the selected trace entry
    let filtered_entries: Vec<_> = state
        .trace_entries
        .iter()
        .filter(|e| state.verbose_trace || e.event_type.is_important())
        .collect();

    let entry = filtered_entries.get(state.trace_selected);

    let content = if let Some(entry) = entry {
        let elapsed = entry.timestamp.elapsed().as_millis();
        let mut lines = vec![
            Line::from(vec![
                Span::styled(
                    "Type: ",
                    Style::default()
                        .fg(theme::TEXT)
                        .add_modifier(Modifier::BOLD),
                ),
                Span::styled(
                    format!("{}", entry.event_type.icon()),
                    Style::default().fg(theme::GREEN),
                ),
                Span::styled(
                    format!(" {:?}", entry.event_type),
                    Style::default().fg(theme::TEXT),
                ),
            ]),
            Line::from(vec![
                Span::styled(
                    "Time: ",
                    Style::default()
                        .fg(theme::TEXT)
                        .add_modifier(Modifier::BOLD),
                ),
                Span::styled(
                    format!("{}ms ago", elapsed),
                    Style::default().fg(theme::SUBTEXT1),
                ),
            ]),
            Line::from(""),
            Line::from(vec![Span::styled(
                "Message:",
                Style::default()
                    .fg(theme::MAUVE)
                    .add_modifier(Modifier::BOLD),
            )]),
        ];

        // Split message into wrapped lines
        for msg_line in entry.message.lines() {
            lines.push(Line::from(vec![
                Span::styled("  ", Style::default()),
                Span::styled(msg_line.to_string(), Style::default().fg(theme::TEXT)),
            ]));
        }

        // Add details if present
        if let Some(ref details) = entry.details {
            lines.push(Line::from(""));
            lines.push(Line::from(vec![Span::styled(
                "Details:",
                Style::default()
                    .fg(theme::MAUVE)
                    .add_modifier(Modifier::BOLD),
            )]));
            for detail_line in details.lines() {
                lines.push(Line::from(vec![
                    Span::styled("  ", Style::default()),
                    Span::styled(
                        detail_line.to_string(),
                        Style::default().fg(theme::SUBTEXT1),
                    ),
                ]));
            }
        }

        lines.push(Line::from(""));
        lines.push(Line::from(vec![
            Span::styled("Press ", Style::default().fg(theme::SUBTEXT1)),
            Span::styled("Enter", Style::default().fg(theme::GREEN)),
            Span::styled(" or ", Style::default().fg(theme::SUBTEXT1)),
            Span::styled("Esc", Style::default().fg(theme::GREEN)),
            Span::styled(" to close", Style::default().fg(theme::SUBTEXT1)),
        ]));

        lines
    } else {
        vec![Line::from(vec![Span::styled(
            "No trace entry selected",
            Style::default().fg(theme::SUBTEXT1),
        )])]
    };

    let block = Block::default()
        .title(" Trace Detail ")
        .borders(Borders::ALL)
        .border_style(Style::default().fg(theme::LAVENDER))
        .style(Style::default().bg(theme::BASE));

    let paragraph = Paragraph::new(content)
        .block(block)
        .wrap(Wrap { trim: false });

    // Clear the area first
    f.render_widget(ratatui::widgets::Clear, popup_area);
    f.render_widget(paragraph, popup_area);
}

/// Render discovery popup based on current state
fn render_discover_popup(f: &mut Frame, state: &mut AppState) {
    let popup = state.discover_popup.clone();
    match &popup {
        DiscoverPopup::None => {}
        DiscoverPopup::ServerSearchInput {
            query,
            cursor_position,
        } => {
            render_server_search_input_popup(f, query, *cursor_position, state);
        }
        DiscoverPopup::ServerSuggestions {
            results, selected, ..
        } => {
            render_server_suggestions_popup(f, results, *selected);
        }
        DiscoverPopup::SearchResults {
            servers, selected, ..
        } => {
            render_search_results_popup(f, servers, *selected);
        }
        DiscoverPopup::Introspecting {
            server_name,
            endpoint,
            logs,
            ..
        } => {
            render_introspecting_popup(f, server_name, endpoint, logs, state);
        }
        DiscoverPopup::IntrospectionResults {
            server_name,
            tools,
            selected,
            selected_tools,
            added_success,
            pended_success,
            ..
        } => {
            render_introspection_results_popup(
                f,
                server_name,
                tools,
                *selected,
                selected_tools,
                *added_success,
                *pended_success,
                state,
            );
        }
        DiscoverPopup::Error { title, message } => {
            render_error_popup(f, title, message);
        }
        DiscoverPopup::Success { title, message } => {
            render_success_popup(f, title, message);
        }
        DiscoverPopup::DeleteConfirmation { server } => {
            render_delete_confirmation_popup(f, server);
        }
        DiscoverPopup::ToolDetails {
            name,
            endpoint,
            description,
            ..
        } => {
            render_tool_details_popup(f, name, endpoint, description);
        }
    }
}

fn render_delete_confirmation_popup(f: &mut Frame, server: &crate::tui::state::ServerInfo) {
    let area = f.size();
    let popup_area = centered_rect(50, 20, area);

    let is_queue_server = server.queue_id.is_some();
    let is_directory_server = server.directory_path.is_some();
    let is_hide_only = !is_queue_server && !is_directory_server;

    let server_name = server.name.as_str();
    let title = if is_hide_only {
        " Confirm Hide "
    } else {
        " Confirm Deletion "
    };
    let block = Block::default()
        .title(title)
        .borders(Borders::ALL)
        .border_style(Style::default().fg(theme::RED))
        .style(Style::default().bg(theme::BASE));

    let action_label = if is_hide_only { "Hide" } else { "delete" };
    let hint_line = if is_hide_only {
        Line::from(vec![Span::styled(
            "This only hides the built-in/known server entry.",
            Style::default().fg(theme::PEACH),
        )])
    } else {
        Line::from(vec![Span::styled(
            "Deleted servers are archived under capabilities/servers/deleted/.",
            Style::default().fg(theme::PEACH),
        )])
    };
    let primary_cta = if is_hide_only {
        "[y] Yes, Hide"
    } else {
        "[y] Yes, Delete"
    };

    let content = vec![
        Line::from(""),
        Line::from(vec![
            Span::raw(format!("Are you sure you want to {} server ", action_label)),
            Span::styled(
                server_name,
                Style::default()
                    .fg(theme::TEXT)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::raw("?"),
        ]),
        Line::from(""),
        hint_line,
        Line::from(""),
        Line::from(vec![
            Span::styled(
                primary_cta,
                Style::default().fg(theme::RED).add_modifier(Modifier::BOLD),
            ),
            Span::raw("      "),
            Span::styled("[n] Cancel", Style::default().fg(theme::GREEN)),
        ]),
    ];

    let paragraph = Paragraph::new(content)
        .block(block)
        .wrap(Wrap { trim: false })
        .alignment(Alignment::Center);

    f.render_widget(ratatui::widgets::Clear, popup_area);
    f.render_widget(paragraph, popup_area);
}

fn render_server_search_input_popup(
    f: &mut Frame,
    query: &str,
    _cursor_position: usize,
    state: &AppState,
) {
    let area = f.size();
    let popup_area = centered_rect(50, 20, area);

    let title = if state.discover_loading {
        format!(" Find New Servers {} ", state.spinner_icon())
    } else {
        " Find New Servers ".to_string()
    };

    let block = Block::default()
        .title(title)
        .borders(Borders::ALL)
        .border_style(Style::default().fg(theme::MAUVE))
        .style(Style::default().bg(theme::BASE));

    let content = vec![
        Line::from(vec![Span::styled(
            "Describe what you are looking for:",
            Style::default().fg(theme::SUBTEXT0),
        )]),
        Line::from(""),
        Line::from(vec![
            Span::styled("> ", Style::default().fg(theme::TEAL)),
            Span::styled(query, Style::default().fg(theme::TEXT)),
        ]),
        Line::from(""),
        Line::from(vec![Span::styled(
            "Example: 'weather api', 'sms service', 'github'",
            Style::default()
                .fg(theme::SUBTEXT0)
                .add_modifier(Modifier::ITALIC),
        )]),
    ];

    let paragraph = Paragraph::new(content).block(block);

    f.render_widget(ratatui::widgets::Clear, popup_area);
    f.render_widget(paragraph, popup_area);

    // Render cursor manually if simple Paragraph doesn't support it easily without breaking lines
    // For simplicity, we assume the cursor is at end or handled by state logic for display
}

fn render_server_suggestions_popup(
    f: &mut Frame,
    suggestions: &[RegistrySearchResult],
    selected: usize,
) {
    let area = f.size();
    let popup_area = centered_rect(80, 70, area); // Increased size for better info

    let items: Vec<ListItem> = suggestions
        .iter()
        .enumerate()
        .map(|(i, s)| {
            let is_selected = i == selected;
            let prefix = if is_selected { "▶ " } else { "  " };
            let style = if is_selected {
                Style::default()
                    .fg(theme::GREEN)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(theme::TEXT)
            };

            // Category Badge
            let (badge, badge_style) = match s.category {
                DiscoveryCategory::Mcp => {
                    ("MCP", Style::default().fg(theme::MAUVE).bg(theme::MANTLE))
                }
                DiscoveryCategory::OpenApi => (
                    "OPENAPI",
                    Style::default().fg(theme::BLUE).bg(theme::MANTLE),
                ),
                DiscoveryCategory::WebDoc => {
                    ("DOC", Style::default().fg(theme::YELLOW).bg(theme::MANTLE))
                }
                DiscoveryCategory::WebApi => {
                    ("API", Style::default().fg(theme::PEACH).bg(theme::MANTLE))
                }
                DiscoveryCategory::OpenApiTool => (
                    "ENDPOINT",
                    Style::default().fg(theme::GREEN).bg(theme::MANTLE),
                ),
                DiscoveryCategory::BrowserApiTool => (
                    "BROWSER",
                    Style::default().fg(theme::TEAL).bg(theme::MANTLE),
                ),
                DiscoveryCategory::Other => (
                    "OTHER",
                    Style::default().fg(theme::SUBTEXT1).bg(theme::MANTLE),
                ),
            };

            // Format: [BADGE] name
            // Strip protocol for cleaner display
            let mut display_url = s.server_info.endpoint.clone();
            if display_url.starts_with("http://") {
                display_url = display_url.replace("http://", "");
            } else if display_url.starts_with("https://") {
                display_url = display_url.replace("https://", "");
            }
            let endpoint_display = format!(" ({})", display_url);
            // Ensure description exists
            let desc_text = s.server_info.description.clone().unwrap_or_default();
            let desc = format!(" - {}", desc_text);

            ListItem::new(vec![
                Line::from(vec![
                    Span::styled(prefix, style),
                    Span::styled(format!("[{:^7}] ", badge), badge_style),
                    Span::styled(&s.server_info.name, style),
                    Span::styled(endpoint_display, Style::default().fg(theme::SAPPHIRE)),
                ]),
                Line::from(vec![
                    Span::styled("     ", Style::default()), // Indent
                    Span::styled(desc, Style::default().fg(theme::SUBTEXT0)),
                ]),
                Line::from(""), // Empty line for spacing
            ])
        })
        .collect();

    let block = Block::default()
        .title(format!(
            " Suggestions ({} found) - Enter: Explorer, Esc: Back/Close ",
            suggestions.len()
        ))
        .borders(Borders::ALL)
        .border_style(Style::default().fg(theme::MAUVE))
        .style(Style::default().bg(theme::BASE));

    let list = List::new(items).block(block);

    let mut list_state = ListState::default();
    if !suggestions.is_empty() {
        list_state.select(Some(selected));
    }

    f.render_widget(ratatui::widgets::Clear, popup_area);
    f.render_stateful_widget(list, popup_area, &mut list_state);
}

/// Render search results popup with server list
fn render_search_results_popup(f: &mut Frame, servers: &[RegistrySearchResult], selected: usize) {
    let area = f.size();
    let popup_area = centered_rect(70, 60, area);

    let items: Vec<ListItem> = servers
        .iter()
        .enumerate()
        .map(|(i, server)| {
            let is_selected = i == selected;
            let prefix = if is_selected { "▶ " } else { "  " };
            let style = if is_selected {
                Style::default()
                    .fg(theme::GREEN)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(theme::TEXT)
            };

            // Badge for category
            let (badge_text, badge_color) = match server.category {
                DiscoveryCategory::Mcp => (" [MCP] ", theme::BLUE),
                DiscoveryCategory::OpenApi => (" [API] ", theme::MAUVE),
                DiscoveryCategory::WebDoc => (" [DOC] ", theme::YELLOW),
                DiscoveryCategory::WebApi => (" [WEB] ", theme::PEACH),
                DiscoveryCategory::OpenApiTool => (" [EP] ", theme::GREEN),
                DiscoveryCategory::BrowserApiTool => (" [BR] ", theme::TEAL),
                DiscoveryCategory::Other => (" [???] ", theme::SUBTEXT0),
            };

            let badge_span =
                Span::styled(badge_text, Style::default().fg(theme::BASE).bg(badge_color));

            let desc = match &server.server_info.description {
                Some(d) if !d.is_empty() => format!(" - {}", d),
                _ => String::new(),
            };

            // Strip protocol for cleaner display
            let mut display_url = server.server_info.endpoint.clone();
            if display_url.starts_with("http://") {
                display_url = display_url.replace("http://", "");
            } else if display_url.starts_with("https://") {
                display_url = display_url.replace("https://", "");
            }
            let endpoint_display = format!(" ({})", display_url);

            ListItem::new(Line::from(vec![
                Span::styled(prefix, style),
                badge_span,
                Span::styled(format!(" {} ", server.server_info.name), style),
                Span::styled(endpoint_display, Style::default().fg(theme::SAPPHIRE)),
                Span::styled(desc, Style::default().fg(theme::SUBTEXT0)),
            ]))
        })
        .collect();

    let block = Block::default()
        .title(format!(
            " Search Results ({} servers) - Enter: Introspect, Esc: Close ",
            servers.len()
        ))
        .borders(Borders::ALL)
        .border_style(Style::default().fg(theme::MAUVE))
        .style(Style::default().bg(theme::BASE));

    let list = List::new(items).block(block);

    let mut list_state = ListState::default();
    list_state.select(Some(selected));

    f.render_widget(ratatui::widgets::Clear, popup_area);
    f.render_stateful_widget(list, popup_area, &mut list_state);
}

/// Render introspecting popup with spinner
fn render_introspecting_popup(
    f: &mut Frame,
    server_name: &str,
    endpoint: &str,
    logs: &[String],
    state: &mut AppState,
) {
    let area = f.size();
    let popup_area = centered_rect(60, 40, area);

    f.render_widget(ratatui::widgets::Clear, popup_area);

    let block = Block::default()
        .title(" Introspecting ")
        .borders(Borders::ALL)
        .border_style(Style::default().fg(theme::BLUE))
        .style(Style::default().bg(theme::BASE));

    f.render_widget(block, popup_area);

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .margin(1)
        .constraints([Constraint::Length(6), Constraint::Min(0)])
        .split(popup_area);

    let spinner = state.spinner_icon();
    let content = vec![
        Line::from(""),
        Line::from(vec![Span::styled(
            format!("  {} Connecting to server...", spinner),
            Style::default().fg(theme::BLUE),
        )]),
        Line::from(""),
        Line::from(vec![
            Span::styled("  Server:   ", Style::default().fg(theme::SUBTEXT0)),
            Span::styled(server_name, Style::default().fg(theme::TEXT)),
        ]),
        Line::from(vec![
            Span::styled("  Endpoint: ", Style::default().fg(theme::SUBTEXT0)),
            Span::styled(endpoint, Style::default().fg(theme::SUBTEXT1)),
        ]),
    ];

    let paragraph = Paragraph::new(content);
    f.render_widget(paragraph, chunks[0]);

    // Render logs
    let log_items: Vec<ListItem> = logs
        .iter()
        .rev() // Show newest at top? Or follow scrolling? List shows from top.
        .take(15) // Limit view
        .map(|log| {
            ListItem::new(Line::from(Span::styled(
                log,
                Style::default().fg(theme::SUBTEXT1),
            )))
        })
        .collect();

    let logs_list = List::new(log_items).block(
        Block::default()
            .title(" Activity Log ")
            .borders(Borders::TOP)
            .border_style(Style::default().fg(theme::SUBTEXT0)),
    );

    f.render_widget(logs_list, chunks[1]);
}

/// Render introspection results with tool selection
fn render_introspection_results_popup(
    f: &mut Frame,
    server_name: &str,
    tools: &[super::state::DiscoveredCapability],
    selected: usize,
    selected_tools: &std::collections::HashSet<usize>,
    added_success: bool,
    pended_success: bool,
    state: &mut AppState,
) {
    let area = f.size();
    let popup_area = centered_rect(75, 75, area); // Slightly taller for name editing

    let is_editing = match &state.discover_popup {
        DiscoverPopup::IntrospectionResults { editing_name, .. } => *editing_name,
        _ => false,
    };

    let items: Vec<ListItem> = tools
        .iter()
        .enumerate()
        .map(|(i, tool)| {
            let is_selected = i == selected;
            let is_checked = selected_tools.contains(&i);
            let cursor = if is_selected { "▶" } else { " " };
            let checkbox = if is_checked { "[✓]" } else { "[ ]" };

            let name_style = if is_selected {
                Style::default()
                    .fg(theme::GREEN)
                    .add_modifier(Modifier::BOLD)
            } else if is_checked {
                Style::default().fg(theme::MAUVE)
            } else {
                Style::default().fg(theme::TEXT)
            };

            let desc = truncate(&tool.description, 50);

            ListItem::new(Line::from(vec![
                Span::styled(format!("{} {} ", cursor, checkbox), name_style),
                Span::styled(&tool.name, name_style),
                Span::styled(format!(" - {}", desc), Style::default().fg(theme::SUBTEXT0)),
            ]))
        })
        .collect();

    let selected_count = selected_tools.len();
    let mut title = format!(" {} Tools ({} selected) ", server_name, selected_count);

    if added_success && pended_success {
        title = format!("{} - [ADDED & PENDED] ", title);
    } else if added_success {
        title = format!("{} - [ADDED] ", title);
    } else if pended_success {
        title = format!("{} - [PENDED] ", title);
    }

    let legend = if is_editing {
        " Backspace: Delete | Esc/Enter: Stop Editing "
    } else {
        " Space: Toggle | a: Select All | c: Select None | n: Edit Name | Enter: Add/Save | Esc: Back "
    };

    let block = Block::default()
        .title(title)
        .title_bottom(Line::from(legend).alignment(Alignment::Center))
        .borders(Borders::ALL)
        .border_style(Style::default().fg(theme::LAVENDER))
        .style(Style::default().bg(theme::BASE));

    // Split popup into Name area and Tools list
    let layout = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3), // Server Name field
            Constraint::Min(0),    // Tools list
        ])
        .margin(1)
        .split(popup_area);

    let name_color = if is_editing {
        theme::YELLOW
    } else {
        theme::PEACH
    };
    let name_field = Paragraph::new(Line::from(vec![
        Span::styled(" Server Name: ", Style::default().fg(theme::SUBTEXT0)),
        Span::styled(
            server_name,
            Style::default().fg(name_color).add_modifier(Modifier::BOLD),
        ),
        if is_editing {
            Span::styled("█", Style::default().fg(theme::YELLOW)) // Caret
        } else {
            Span::raw("")
        },
    ]))
    .block(
        Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(if is_editing {
                theme::YELLOW
            } else {
                theme::SURFACE1
            })),
    );

    let list = List::new(items).block(Block::default());

    let mut list_state = ListState::default();
    list_state.select(Some(selected));

    f.render_widget(ratatui::widgets::Clear, popup_area);
    f.render_widget(block, popup_area);
    f.render_widget(name_field, layout[0]);

    let list_area = layout[1];
    f.render_stateful_widget(list, list_area, &mut list_state);
}

/// Render error popup
fn render_error_popup(f: &mut Frame, title: &str, message: &str) {
    let area = f.size();
    // Use larger popup for error messages (60% x 40%)
    let popup_area = centered_rect(60, 40, area);

    // Calculate inner area for text wrapping
    let inner_width = popup_area.width.saturating_sub(4) as usize;

    // Wrap the message text to fit within the popup
    let mut lines: Vec<Line> = vec![Line::from("")];

    // Split message by lines first (in case there are newlines)
    for msg_line in message.lines() {
        // Word-wrap each line
        let words: Vec<&str> = msg_line.split_whitespace().collect();
        let mut current_line = String::new();

        for word in words {
            if current_line.is_empty() {
                current_line = word.to_string();
            } else if current_line.len() + 1 + word.len() <= inner_width.saturating_sub(4) {
                current_line.push(' ');
                current_line.push_str(word);
            } else {
                lines.push(Line::from(vec![Span::styled(
                    format!("  {}", current_line),
                    Style::default().fg(theme::RED),
                )]));
                current_line = word.to_string();
            }
        }

        if !current_line.is_empty() {
            lines.push(Line::from(vec![Span::styled(
                format!("  {}", current_line),
                Style::default().fg(theme::RED),
            )]));
        }
    }

    lines.push(Line::from(""));
    lines.push(Line::from(vec![Span::styled(
        "  Press Esc or Enter to close",
        Style::default().fg(theme::SUBTEXT0),
    )]));

    let block = Block::default()
        .title(format!(" ⚠ {} ", title))
        .borders(Borders::ALL)
        .border_style(Style::default().fg(theme::RED))
        .style(Style::default().bg(theme::BASE));

    let paragraph = Paragraph::new(lines)
        .block(block)
        .wrap(Wrap { trim: false });

    f.render_widget(ratatui::widgets::Clear, popup_area);
    f.render_widget(paragraph, popup_area);
}

fn render_success_popup(f: &mut Frame, title: &str, message: &str) {
    let area = f.size();
    let popup_area = centered_rect(60, 40, area);
    let inner_width = popup_area.width.saturating_sub(4) as usize;

    let mut lines: Vec<Line> = vec![Line::from("")];

    for msg_line in message.lines() {
        let words: Vec<&str> = msg_line.split_whitespace().collect();
        let mut current_line = String::new();

        for word in words {
            if current_line.is_empty() {
                current_line = word.to_string();
            } else if current_line.len() + 1 + word.len() <= inner_width.saturating_sub(4) {
                current_line.push(' ');
                current_line.push_str(word);
            } else {
                lines.push(Line::from(vec![Span::styled(
                    format!("  {}", current_line),
                    Style::default().fg(theme::GREEN),
                )]));
                current_line = word.to_string();
            }
        }
        if !current_line.is_empty() {
            lines.push(Line::from(vec![Span::styled(
                format!("  {}", current_line),
                Style::default().fg(theme::GREEN),
            )]));
        }
    }

    lines.push(Line::from(""));
    lines.push(Line::from(vec![Span::styled(
        "  Press Esc or Enter to close",
        Style::default().fg(theme::SUBTEXT0),
    )]));

    let block = Block::default()
        .title(format!(" ✓ {} ", title))
        .borders(Borders::ALL)
        .border_style(Style::default().fg(theme::GREEN))
        .style(Style::default().bg(theme::BASE));

    let paragraph = Paragraph::new(lines)
        .block(block)
        .wrap(Wrap { trim: false });

    f.render_widget(ratatui::widgets::Clear, popup_area);
    f.render_widget(paragraph, popup_area);
}

/// Render tool details popup for discovered API endpoints
fn render_tool_details_popup(f: &mut Frame, name: &str, endpoint: &str, description: &str) {
    let area = f.size();
    let popup_area = centered_rect(70, 50, area);
    let inner_width = popup_area.width.saturating_sub(4) as usize;

    let mut lines: Vec<Line> = vec![
        Line::from(""),
        Line::from(vec![
            Span::styled(
                "  Endpoint: ",
                Style::default()
                    .fg(theme::SUBTEXT0)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled(
                name,
                Style::default()
                    .fg(theme::GREEN)
                    .add_modifier(Modifier::BOLD),
            ),
        ]),
        Line::from(""),
        Line::from(vec![
            Span::styled("  Base URL: ", Style::default().fg(theme::SUBTEXT0)),
            Span::styled(endpoint, Style::default().fg(theme::BLUE)),
        ]),
        Line::from(""),
        Line::from(vec![Span::styled(
            "  Description: ",
            Style::default().fg(theme::SUBTEXT0),
        )]),
    ];

    // Word-wrap the description
    let words: Vec<&str> = description.split_whitespace().collect();
    let mut current_line = String::new();
    for word in words {
        if current_line.is_empty() {
            current_line = word.to_string();
        } else if current_line.len() + 1 + word.len() <= inner_width.saturating_sub(6) {
            current_line.push(' ');
            current_line.push_str(word);
        } else {
            lines.push(Line::from(vec![Span::styled(
                format!("    {}", current_line),
                Style::default().fg(theme::TEXT),
            )]));
            current_line = word.to_string();
        }
    }
    if !current_line.is_empty() {
        lines.push(Line::from(vec![Span::styled(
            format!("    {}", current_line),
            Style::default().fg(theme::TEXT),
        )]));
    }

    lines.push(Line::from(""));
    lines.push(Line::from(vec![Span::styled(
        "  Press Esc to go back, Enter to close",
        Style::default().fg(theme::SUBTEXT0),
    )]));

    let block = Block::default()
        .title(" API Endpoint Details ")
        .borders(Borders::ALL)
        .border_style(Style::default().fg(theme::GREEN))
        .style(Style::default().bg(theme::BASE));

    let paragraph = Paragraph::new(lines)
        .block(block)
        .wrap(Wrap { trim: false });

    f.render_widget(ratatui::widgets::Clear, popup_area);
    f.render_widget(paragraph, popup_area);
}

/// Helper to create centered rect
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

/// Truncate string with ellipsis (UTF-8 safe)
fn truncate(s: &str, max_len: usize) -> String {
    // Count characters, not bytes
    let char_count = s.chars().count();
    if char_count <= max_len {
        s.to_string()
    } else if max_len > 3 {
        // Take (max_len - 3) characters and append ellipsis
        let truncated: String = s.chars().take(max_len - 3).collect();
        format!("{}...", truncated)
    } else {
        s.chars().take(max_len).collect()
    }
}

/// Render intent detail popup
fn render_intent_popup(f: &mut Frame, state: &mut AppState) {
    let area = f.size();
    let popup_area = centered_rect(80, 70, area);

    let content = if let Some(node) = state.decomp_nodes.get(state.decomp_selected) {
        let (status_icon, status_color) = match &node.status {
            NodeStatus::Pending => ("○", theme::SUBTEXT0),
            NodeStatus::Resolving => ("◐", theme::BLUE),
            NodeStatus::Resolved { .. } => ("✓", theme::GREEN),
            NodeStatus::Synthesizing => ("⚡", theme::PEACH),
            NodeStatus::Failed { .. } => ("✗", theme::RED),
            NodeStatus::UserInput => ("?", theme::YELLOW),
        };

        let mut lines = vec![
            Line::from(vec![
                Span::styled(
                    "Intent ID: ",
                    Style::default()
                        .fg(theme::MAUVE)
                        .add_modifier(Modifier::BOLD),
                ),
                Span::styled(node.id.clone(), Style::default().fg(theme::TEXT)),
            ]),
            Line::from(vec![
                Span::styled(
                    "Type:      ",
                    Style::default()
                        .fg(theme::MAUVE)
                        .add_modifier(Modifier::BOLD),
                ),
                Span::styled(node.intent_type.clone(), Style::default().fg(theme::BLUE)),
            ]),
            Line::from(vec![
                Span::styled(
                    "Status:    ",
                    Style::default()
                        .fg(theme::MAUVE)
                        .add_modifier(Modifier::BOLD),
                ),
                Span::styled(status_icon, Style::default().fg(status_color)),
                Span::styled(
                    format!(" {:?}", node.status),
                    Style::default().fg(theme::TEXT),
                ),
            ]),
            Line::from(""),
            Line::from(vec![Span::styled(
                "Description:",
                Style::default()
                    .fg(theme::MAUVE)
                    .add_modifier(Modifier::BOLD),
            )]),
        ];

        for line in node.description.lines() {
            lines.push(Line::from(vec![
                Span::styled("  ", Style::default()),
                Span::styled(line.to_string(), Style::default().fg(theme::TEXT)),
            ]));
        }

        if !node.params.is_empty() {
            lines.push(Line::from(""));
            lines.push(Line::from(vec![Span::styled(
                "Parameters:",
                Style::default()
                    .fg(theme::MAUVE)
                    .add_modifier(Modifier::BOLD),
            )]));
            for (k, v) in &node.params {
                lines.push(Line::from(vec![
                    Span::styled("  ", Style::default()),
                    Span::styled(format!("{}: ", k), Style::default().fg(theme::BLUE)),
                    Span::styled(v.to_string(), Style::default().fg(theme::TEXT)),
                ]));
            }
        }

        lines.push(Line::from(""));
        lines.push(Line::from(vec![
            Span::styled("Press ", Style::default().fg(theme::SUBTEXT1)),
            Span::styled("Esc", Style::default().fg(theme::GREEN)),
            Span::styled(" to close", Style::default().fg(theme::SUBTEXT1)),
        ]));

        lines
    } else {
        vec![Line::from(vec![Span::styled(
            "No intent selected",
            Style::default().fg(theme::SUBTEXT1),
        )])]
    };

    let block = Block::default()
        .title(" Intent Detail ")
        .borders(Borders::ALL)
        .border_style(Style::default().fg(theme::LAVENDER))
        .style(Style::default().bg(theme::BASE));

    let paragraph = Paragraph::new(content)
        .block(block)
        .wrap(Wrap { trim: false });

    f.render_widget(ratatui::widgets::Clear, popup_area);
    f.render_widget(paragraph, popup_area);
}
