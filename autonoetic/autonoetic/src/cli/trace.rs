use std::io::{BufRead, BufReader as StdBufReader};
use std::path::Path;

use super::common::AgentTrace;
use autonoetic_gateway::llm::Message;
use autonoetic_types::causal_chain::{CausalChainEntry, EntryStatus};
use autonoetic_types::workflow::{
    TaskRun, TaskRunStatus, WorkflowEventRecord, WorkflowRun, WorkflowRunStatus,
};
use serde::Serialize;

/// ANSI color helpers for terminal output.
mod color {
    pub const RESET: &str = "\x1b[0m";
    pub const BOLD: &str = "\x1b[1m";
    pub const DIM: &str = "\x1b[2m";
    pub const RED: &str = "\x1b[31m";
    pub const GREEN: &str = "\x1b[32m";
    pub const YELLOW: &str = "\x1b[33m";
    pub const BLUE: &str = "\x1b[34m";
    pub const MAGENTA: &str = "\x1b[35m";
    pub const CYAN: &str = "\x1b[36m";
    pub const WHITE: &str = "\x1b[37m";
    pub const BRIGHT_RED: &str = "\x1b[91m";
    pub const BRIGHT_YELLOW: &str = "\x1b[93m";
    pub const BRIGHT_BLUE: &str = "\x1b[94m";
    pub const BRIGHT_CYAN: &str = "\x1b[96m";

    pub fn status_color(s: &str) -> &'static str {
        match s {
            "SUCCESS" => GREEN,
            "DENIED" => YELLOW,
            "ERROR" => RED,
            _ => WHITE,
        }
    }

    pub fn status_label(s: &str) -> String {
        let c = status_color(s);
        format!("{}{}{}{}", c, BOLD, s, RESET)
    }

    pub fn agent(s: &str) -> String {
        format!("{}{}{}{}", BRIGHT_CYAN, BOLD, s, RESET)
    }

    pub fn category(s: &str) -> String {
        match s {
            "tool_invoke" => format!("{}{}{}{}", MAGENTA, BOLD, s, RESET),
            "gateway" => format!("{}{}{}{}", BLUE, BOLD, s, RESET),
            "lifecycle" => format!("{}{}{}{}", CYAN, BOLD, s, RESET),
            "artifact" => format!("{}{}{}{}", YELLOW, BOLD, s, RESET),
            "llm" => format!("{}{}{}{}", BRIGHT_BLUE, BOLD, s, RESET),
            _ => format!("{}{}{}", DIM, s, RESET),
        }
    }

    pub fn action(s: &str) -> String {
        match s {
            "requested" | "started" => format!("{}{}{}", CYAN, s, RESET),
            "completed" | "success" => format!("{}{}{}", GREEN, s, RESET),
            "error" | "failed" => format!("{}{}{}{}", BRIGHT_RED, BOLD, s, RESET),
            "denied" => format!("{}{}{}{}", YELLOW, BOLD, s, RESET),
            _ => s.to_string(),
        }
    }

    pub fn tool_name(s: &str) -> String {
        format!("{}{}{}{}", BRIGHT_YELLOW, BOLD, s, RESET)
    }

    pub fn seq(s: u64) -> String {
        format!("{}{}{}", DIM, s, RESET)
    }

    pub fn separator(len: usize) -> String {
        format!("{}{}{}", DIM, "─".repeat(len), RESET)
    }

    pub fn dim(s: &str) -> String {
        format!("{}{}{}", DIM, s, RESET)
    }
}

pub fn handle_trace_sessions(
    config_path: &Path,
    requested_agent: Option<&str>,
    json_output: bool,
) -> anyhow::Result<()> {
    let traces = load_agent_traces(config_path, requested_agent)?;
    let sessions = super::common::collect_session_summaries(&traces);
    if json_output {
        let body = sessions
            .iter()
            .map(|s| {
                serde_json::json!({
                    "agent_id": s.agent_id,
                    "session_id": s.session_id,
                    "first_timestamp": s.first_timestamp,
                    "last_timestamp": s.last_timestamp,
                    "event_count": s.event_count,
                    "max_event_seq": s.max_event_seq
                })
            })
            .collect::<Vec<_>>();
        println!("{}", serde_json::to_string_pretty(&body)?);
        return Ok(());
    }

    if sessions.is_empty() {
        println!("No trace sessions found.");
        return Ok(());
    }

    println!(
        "{}{}{:<30} {:<38} {:<26} {:<26} {:<8} {:<10}{}",
        color::DIM, color::BOLD,
        "AGENT", "SESSION ID", "FIRST TS", "LAST TS", "EVENTS", "MAX SEQ",
        color::RESET
    );
    println!("{}", color::separator(146));
    for s in sessions {
        println!(
            "{} {:<38} {:<26} {:<26} {}{} {}",
            color::agent(&s.agent_id),
            s.session_id,
            s.first_timestamp,
            s.last_timestamp,
            color::BRIGHT_YELLOW, s.event_count, color::RESET,
        );
    }
    Ok(())
}

pub fn handle_trace_session(
    config_path: &Path,
    session_id: &str,
    requested_agent: Option<&str>,
    json_output: bool,
) -> anyhow::Result<()> {
    anyhow::ensure!(
        !session_id.trim().is_empty(),
        "session_id must not be empty"
    );
    let traces = load_agent_traces(config_path, requested_agent)?;
    let mut matches: Vec<(String, Vec<CausalChainEntry>)> = Vec::new();
    for trace in traces {
        let events = trace
            .entries
            .into_iter()
            .filter(|entry| entry.session_id == session_id)
            .collect::<Vec<_>>();
        if !events.is_empty() {
            matches.push((trace.agent_id, events));
        }
    }

    anyhow::ensure!(
        !matches.is_empty(),
        "No events found for session '{}'{}",
        session_id,
        requested_agent
            .map(|a| format!(" under agent '{}'", a))
            .unwrap_or_default()
    );
    if requested_agent.is_none() && matches.len() > 1 {
        let owners = matches
            .iter()
            .map(|(agent_id, _)| agent_id.clone())
            .collect::<Vec<_>>()
            .join(", ");
        anyhow::bail!(
            "Session '{}' found in multiple agents ({}). Re-run with --agent.",
            session_id,
            owners
        );
    }

    let (agent_id, mut entries) = matches
        .into_iter()
        .next()
        .ok_or_else(|| anyhow::anyhow!("failed to resolve session match"))?;
    entries.sort_by(|a, b| {
        a.timestamp
            .cmp(&b.timestamp)
            .then_with(|| a.event_seq.cmp(&b.event_seq))
    });

    if json_output {
        let body = serde_json::json!({
            "agent_id": agent_id,
            "session_id": session_id,
            "events": entries,
        });
        println!("{}", serde_json::to_string_pretty(&body)?);
        return Ok(());
    }

    println!("Agent: {}", color::agent(&agent_id));
    println!("Session: {}", color::BRIGHT_YELLOW.to_string() + session_id + color::RESET);
    println!(
        "{}{}{:<8} {:<24} {:<15} {:<18} {:<15} {:<20} {}{}",
        color::DIM, color::BOLD,
        "SEQ", "TIMESTAMP", "CATEGORY", "ACTION", "STATUS", "TARGET", "REASON",
        color::RESET
    );
    println!("{}", color::separator(130));
    for entry in entries {
        let target_str = entry.target.as_deref().unwrap_or("-");
        let reason_str = entry.reason.as_deref().unwrap_or("-");
        let target_display = if target_str.len() > 19 { format!("{}…", &target_str[..18]) } else { target_str.to_string() };
        let reason_display = if reason_str.len() > 35 { format!("{}…", &reason_str[..34]) } else { reason_str.to_string() };

        // Highlight reason in red for errors/denials, dim otherwise
        let reason_colored = match &entry.status {
            EntryStatus::Error => format!("{}{}{}{}", color::BRIGHT_RED, color::BOLD, reason_display, color::RESET),
            EntryStatus::Denied => format!("{}{}{}{}", color::YELLOW, color::BOLD, reason_display, color::RESET),
            _ => color::dim(&reason_display),
        };

        println!(
            "{} {:<24} {} {} {} {} {}",
            color::seq(entry.event_seq),
            entry.timestamp,
            color::category(&entry.category),
            color::action(&entry.action),
            color::status_label(&format!("{:?}", entry.status)),
            color::dim(&target_display),
            reason_colored,
        );

        // Show tool-specific info for tool_invoke events
        if entry.category == "tool_invoke" {
            if let Some(ref payload) = entry.payload {
                if let Some(tool_name) = payload.get("tool_name").and_then(|v| v.as_str()) {
                    let args_preview = payload.get("arguments")
                        .and_then(|v| v.as_str())
                        .map(|a| {
                            if a.len() > 80 { format!("{}…", &a[..79]) } else { a.to_string() }
                        })
                        .unwrap_or_default();
                    let result_preview = payload.get("result_preview")
                        .and_then(|v| v.as_str())
                        .map(|r| {
                            if r.len() > 80 { format!("{}…", &r[..79]) } else { r.to_string() }
                        })
                        .unwrap_or_default();

                    if entry.action == "requested" && !args_preview.is_empty() {
                        println!("      {}├─ {}({}){}", color::DIM, color::tool_name(tool_name), color::dim(&args_preview), color::RESET);
                    } else if entry.action == "completed" && !result_preview.is_empty() {
                        println!("      {}├─ {} → {}{}", color::DIM, color::tool_name(tool_name), color::dim(&result_preview), color::RESET);
                    } else {
                        println!("      {}├─ {}{}", color::DIM, color::tool_name(tool_name), color::RESET);
                    }
                }
            }
        } else {
            if let Some(ref payload) = entry.payload {
                let payload_str = serde_json::to_string(payload).unwrap_or_default();
                if payload_str.len() > 2 && payload_str != "null" {
                    let truncated = if payload_str.len() > 120 { format!("{}…", &payload_str[..119]) } else { payload_str };
                    println!("      {}├─ payload: {}{}{}", color::DIM, color::BRIGHT_BLUE, truncated, color::RESET);
                }
            }
        }
    }
    Ok(())
}

pub fn handle_trace_event(
    config_path: &Path,
    log_id: &str,
    requested_agent: Option<&str>,
    json_output: bool,
) -> anyhow::Result<()> {
    anyhow::ensure!(!log_id.trim().is_empty(), "log_id must not be empty");
    let traces = load_agent_traces(config_path, requested_agent)?;
    let mut matches: Vec<(String, CausalChainEntry)> = Vec::new();
    for trace in traces {
        for entry in trace.entries {
            if entry.log_id == log_id {
                matches.push((trace.agent_id.clone(), entry));
            }
        }
    }

    anyhow::ensure!(
        !matches.is_empty(),
        "No event found for log_id '{}'{}",
        log_id,
        requested_agent
            .map(|a| format!(" under agent '{}'", a))
            .unwrap_or_default()
    );
    if requested_agent.is_none() && matches.len() > 1 {
        let owners = matches
            .iter()
            .map(|(agent_id, _)| agent_id.clone())
            .collect::<Vec<_>>()
            .join(", ");
        anyhow::bail!(
            "Event '{}' found in multiple agents ({}). Re-run with --agent.",
            log_id,
            owners
        );
    }

    let (agent_id, entry) = matches
        .into_iter()
        .next()
        .ok_or_else(|| anyhow::anyhow!("failed to resolve event match"))?;

    if json_output {
        let body = serde_json::json!({
            "agent_id": agent_id,
            "event": entry,
        });
        println!("{}", serde_json::to_string_pretty(&body)?);
        return Ok(());
    }

    println!("Agent: {}", agent_id);
    println!("{}", serde_json::to_string_pretty(&entry)?);
    Ok(())
}

pub fn load_agent_traces(
    config_path: &Path,
    requested_agent: Option<&str>,
) -> anyhow::Result<Vec<AgentTrace>> {
    let config = autonoetic_gateway::config::load_config(config_path)?;
    let repo = autonoetic_gateway::AgentRepository::from_config(&config);

    let filtered: Vec<_> = if let Some(agent_id) = requested_agent {
        let loaded = repo.get_sync(agent_id)?;
        vec![loaded]
    } else {
        repo.list_loaded_sync()?
    };

    let mut traces = Vec::new();
    for agent in filtered {
        let path = agent.dir.join("history").join("causal_chain.jsonl");
        if !path.exists() {
            continue;
        }
        let entries = read_trace_entries(&path)?;
        traces.push(AgentTrace {
            agent_id: agent.id().to_string(),
            entries,
        });
    }
    Ok(traces)
}

fn load_trace_from_path(path: &Path, agent_id: &str) -> anyhow::Result<AgentTrace> {
    let entries = read_trace_entries(path)?;
    Ok(AgentTrace {
        agent_id: agent_id.to_string(),
        entries,
    })
}

pub fn read_trace_entries(path: &Path) -> anyhow::Result<Vec<CausalChainEntry>> {
    let file = std::fs::File::open(path)?;
    let reader = StdBufReader::new(file);
    let mut entries = Vec::new();
    for (idx, line) in reader.lines().enumerate() {
        let line = line?;
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        let entry: CausalChainEntry = serde_json::from_str(trimmed).map_err(|e| {
            anyhow::anyhow!(
                "Invalid JSON in {} at line {}: {}",
                path.display(),
                idx + 1,
                e
            )
        })?;
        validate_trace_entry(&entry, path, idx + 1)?;
        entries.push(entry);
    }
    Ok(entries)
}

pub fn validate_trace_entry(
    entry: &CausalChainEntry,
    path: &Path,
    line_no: usize,
) -> anyhow::Result<()> {
    anyhow::ensure!(
        !entry.session_id.trim().is_empty(),
        "Invalid causal entry in {} at line {}: missing top-level session_id",
        path.display(),
        line_no
    );
    anyhow::ensure!(
        !entry.entry_hash.trim().is_empty(),
        "Invalid causal entry in {} at line {}: missing top-level entry_hash",
        path.display(),
        line_no
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn test_read_trace_entries_rejects_missing_top_level_session_fields() {
        let temp = tempdir().expect("tempdir should create");
        let path = temp.path().join("causal_chain.jsonl");
        std::fs::write(
            &path,
            r#"{"timestamp":"2026-03-06T00:00:00Z","log_id":"l1","actor_id":"a1","category":"lifecycle","action":"wake","target":null,"status":"SUCCESS","reason":null,"payload":{"session_id":"legacy"},"prev_hash":"genesis","entry_hash":"abc"}"#,
        )
        .expect("trace should write");

        let err = read_trace_entries(&path).expect_err("legacy missing session_id should fail");
        assert!(
            err.to_string().contains("missing top-level session_id"),
            "expected strict top-level session_id validation"
        );
    }

    #[test]
    fn test_read_trace_entries_rejects_missing_top_level_hash_fields() {
        let temp = tempdir().expect("tempdir should create");
        let path = temp.path().join("causal_chain.jsonl");
        std::fs::write(
            &path,
            r#"{"timestamp":"2026-03-06T00:00:00Z","log_id":"l1","actor_id":"a1","session_id":"s1","turn_id":"turn-000001","event_seq":1,"category":"lifecycle","action":"wake","target":null,"status":"SUCCESS","reason":null,"payload":{"history_messages":2},"prev_hash":"genesis","entry_hash":""}"#,
        )
        .expect("trace should write");

        let err = read_trace_entries(&path).expect_err("missing entry_hash should fail");
        assert!(
            err.to_string().contains("missing top-level entry_hash"),
            "expected strict top-level entry_hash validation"
        );
    }

    #[tokio::test]
    async fn test_trace_session_ordering_by_timestamp() {
        let temp = tempdir().expect("tempdir should create");
        let agent_dir = temp.path().join("agent_test");
        let history_dir = agent_dir.join("history");
        std::fs::create_dir_all(&history_dir).expect("history dir should create");

        let causal_path = history_dir.join("causal_chain.jsonl");

        let entry1 = r#"{"timestamp":"2026-03-08T00:00:03Z","log_id":"l1","actor_id":"a1","session_id":"s1","turn_id":null,"event_seq":3,"category":"gateway","action":"test.3","target":null,"status":"SUCCESS","reason":null,"payload":null,"payload_hash":null,"prev_hash":"genesis","entry_hash":"h1"}"#;
        let entry2 = r#"{"timestamp":"2026-03-08T00:00:01Z","log_id":"l2","actor_id":"a1","session_id":"s1","turn_id":null,"event_seq":1,"category":"gateway","action":"test.1","target":null,"status":"SUCCESS","reason":null,"payload":null,"payload_hash":null,"prev_hash":"genesis","entry_hash":"h2"}"#;
        let entry3 = r#"{"timestamp":"2026-03-08T00:00:02Z","log_id":"l3","actor_id":"a1","session_id":"s1","turn_id":null,"event_seq":2,"category":"gateway","action":"test.2","target":null,"status":"SUCCESS","reason":null,"payload":null,"payload_hash":null,"prev_hash":"genesis","entry_hash":"h3"}"#;

        std::fs::write(&causal_path, format!("{}\n{}\n{}\n", entry1, entry2, entry3)).expect("should write");

        let traces = vec![AgentTrace {
            agent_id: "agent_test".to_string(),
            entries: read_trace_entries(&causal_path).expect("should read entries"),
        }];

        let entries = &traces[0].entries;
        assert_eq!(entries.len(), 3);

        let first_read_timestamp = &entries[0].timestamp;
        assert_eq!(first_read_timestamp, "2026-03-08T00:00:03Z",
            "First entry should be from file order (00:00:03), not sorted");

        let mut sorted_entries = entries.clone();
        sorted_entries.sort_by(|a, b| {
            a.timestamp
                .cmp(&b.timestamp)
                .then_with(|| a.event_seq.cmp(&b.event_seq))
        });

        let expected_order = vec!["2026-03-08T00:00:01Z", "2026-03-08T00:00:02Z", "2026-03-08T00:00:03Z"];
        let actual_order: Vec<&str> = sorted_entries.iter().map(|e| e.timestamp.as_str()).collect();
        assert_eq!(actual_order, expected_order, "Entries should be sorted by timestamp");

        let actions: Vec<&str> = sorted_entries.iter().map(|e| e.action.as_str()).collect();
        assert_eq!(actions, vec!["test.1", "test.2", "test.3"]);
    }
}

pub fn handle_trace_rebuild(
    config_path: &std::path::Path,
    session_id: &str,
    requested_agent: Option<&str>,
    json_output: bool,
    skip_checks: bool,
) -> anyhow::Result<()> {
    let config = autonoetic_gateway::config::load_config(config_path)?;
    let gateway_dir = config.agents_dir.join(".gateway");
    
    let mut all_events: Vec<super::common::TraceEntry> = Vec::new();
    
    // Load gateway events
    let gateway_causal_path = gateway_dir.join("history/causal_chain.jsonl");
    if gateway_causal_path.exists() {
        let gateway_traces = load_trace_from_path(&gateway_causal_path, "gateway")?;
        for entry in gateway_traces.entries {
            if entry.session_id == session_id {
                all_events.push(super::common::TraceEntry {
                    agent_id: "gateway".to_string(),
                    entry,
                });
            }
        }
    }
    
    // Load agent events
    let agents_dir = &config.agents_dir;
    if let Ok(entries) = std::fs::read_dir(agents_dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                let causal_path = path.join("history/causal_chain.jsonl");
                if causal_path.exists() {
                    let agent_id = path.file_name()
                        .map(|s| s.to_string_lossy().to_string())
                        .unwrap_or_default();
                    
                    if let Some(requested) = requested_agent {
                        if agent_id != requested {
                            continue;
                        }
                    }
                    
                    let traces = load_trace_from_path(&causal_path, &agent_id)?;
                    for entry in traces.entries {
                        if entry.session_id == session_id {
                            all_events.push(super::common::TraceEntry {
                                agent_id: agent_id.clone(),
                                entry,
                            });
                        }
                    }
                }
            }
        }
    }
    
    if all_events.is_empty() {
        anyhow::bail!("No events found for session '{}'", session_id);
    }
    
    // Sort by timestamp then event_seq
    all_events.sort_by(|a, b| {
        a.entry.timestamp
            .cmp(&b.entry.timestamp)
            .then_with(|| a.entry.event_seq.cmp(&b.entry.event_seq))
    });
    
    // Run integrity checks if not skipped
    let mut integrity_issues: Vec<String> = Vec::new();
    if !skip_checks {
        // Check for gaps in event_seq per agent
        let mut agent_seqs: std::collections::HashMap<String, u64> = std::collections::HashMap::new();
        for te in &all_events {
            let prev = agent_seqs.get(&te.agent_id).copied().unwrap_or(0);
            if te.entry.event_seq != prev + 1 && te.entry.event_seq != 1 {
                integrity_issues.push(format!(
                    "Agent '{}': event_seq gap at {} (expected {}, got {})",
                    te.agent_id, te.entry.timestamp, prev + 1, te.entry.event_seq
                ));
            }
            agent_seqs.insert(te.agent_id.clone(), te.entry.event_seq);
        }
    }
    
    if json_output {
        let output = serde_json::json!({
            "session_id": session_id,
            "event_count": all_events.len(),
            "integrity_issues": integrity_issues,
            "events": all_events.iter().map(|te| {
                serde_json::json!({
                    "agent_id": te.agent_id,
                    "timestamp": te.entry.timestamp,
                    "action": te.entry.action,
                    "status": te.entry.status,
                    "event_seq": te.entry.event_seq,
                })
            }).collect::<Vec<_>>()
        });
        println!("{}", serde_json::to_string_pretty(&output)?);
    } else {
        println!("{}Session Reconstruction: {}{}", color::BOLD, session_id, color::RESET);
        println!("Total events: {}{}{}{}", color::BRIGHT_YELLOW, color::BOLD, all_events.len(), color::RESET);
        println!();
        
        if !integrity_issues.is_empty() {
            println!("{}{}⚠ Integrity Issues:{}{}", color::BRIGHT_RED, color::BOLD, color::RESET, color::RESET);
            for issue in &integrity_issues {
                println!("  {}{}{}{}", color::RED, issue, color::RESET, color::RESET);
            }
            println!();
        }
        
        println!(
            "{}{}{:<10} {:<30} {:<30} {:<20} {:<15}{}",
            color::DIM, color::BOLD,
            "SEQ", "TIMESTAMP", "AGENT", "ACTION", "STATUS",
            color::RESET
        );
        println!("{}", color::separator(105));
        
        for te in &all_events {
            println!(
                "{} {:<30} {} {} {}",
                color::seq(te.entry.event_seq),
                te.entry.timestamp,
                color::agent(&te.agent_id),
                color::action(&te.entry.action),
                color::status_label(&format!("{:?}", te.entry.status)),
            );
        }
    }
    
    Ok(())
}

pub async fn handle_trace_follow(
    config_path: &std::path::Path,
    session_id: &str,
    requested_agent: Option<&str>,
    json_output: bool,
) -> anyhow::Result<()> {
    use std::collections::HashSet;
    use tokio::time::{interval, Duration};

    let config = autonoetic_gateway::config::load_config(config_path)?;
    let gateway_dir = config.agents_dir.join(".gateway");
    let agents_dir = &config.agents_dir;

    let mut seen_log_ids: HashSet<String> = HashSet::new();
    let mut poll_interval = interval(Duration::from_secs(1));

    println!("{}Following session '{}'.{} Press Ctrl+C to stop.", color::BOLD, session_id, color::RESET);
    println!();
    if !json_output {
        println!(
            "{}{}{:<8} {:<24} {:<22} {:<15} {:<18} {:<15} {:<20} {}{}",
            color::DIM, color::BOLD,
            "SEQ", "TIMESTAMP", "AGENT", "CATEGORY", "ACTION", "STATUS", "TARGET", "REASON",
            color::RESET
        );
        println!("{}", color::separator(160));
    }

    loop {
        poll_interval.tick().await;

        let mut new_events: Vec<super::common::TraceEntry> = Vec::new();

        // Check gateway causal log
        let gateway_causal_path = gateway_dir.join("history/causal_chain.jsonl");
        if gateway_causal_path.exists() {
            if let Ok(traces) = load_trace_from_path(&gateway_causal_path, "gateway") {
                for entry in traces.entries {
                    if entry.session_id == session_id {
                        let log_id = format!("gateway:{}", entry.log_id);
                        if !seen_log_ids.contains(&log_id) {
                            seen_log_ids.insert(log_id);
                            new_events.push(super::common::TraceEntry {
                                agent_id: "gateway".to_string(),
                                entry,
                            });
                        }
                    }
                }
            }
        }

        // Check agent causal logs
        if let Ok(entries) = std::fs::read_dir(agents_dir) {
            for entry in entries.flatten() {
                let path = entry.path();
                if path.is_dir() {
                    let causal_path = path.join("history/causal_chain.jsonl");
                    if causal_path.exists() {
                        let agent_id = path.file_name()
                            .map(|s| s.to_string_lossy().to_string())
                            .unwrap_or_default();

                        if let Some(requested) = requested_agent {
                            if agent_id != requested {
                                continue;
                            }
                        }

                        if let Ok(traces) = load_trace_from_path(&causal_path, &agent_id) {
                            for entry in traces.entries {
                                if entry.session_id == session_id {
                                    let log_id = format!("{}:{}", agent_id, entry.log_id);
                                    if !seen_log_ids.contains(&log_id) {
                                        seen_log_ids.insert(log_id);
                                        new_events.push(super::common::TraceEntry {
                                            agent_id: agent_id.clone(),
                                            entry,
                                        });
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }

        if !new_events.is_empty() {
            new_events.sort_by(|a, b| {
                a.entry.timestamp
                    .cmp(&b.entry.timestamp)
                    .then_with(|| a.entry.event_seq.cmp(&b.entry.event_seq))
            });

            for te in new_events {
                if json_output {
                    println!("{}",
                        serde_json::to_string(&serde_json::json!({
                            "agent_id": te.agent_id,
                            "timestamp": te.entry.timestamp,
                            "category": te.entry.category,
                            "action": te.entry.action,
                            "status": te.entry.status,
                            "event_seq": te.entry.event_seq,
                            "turn_id": te.entry.turn_id,
                            "target": te.entry.target,
                            "reason": te.entry.reason,
                            "payload": te.entry.payload,
                        }))?
                    );
                } else {
                    let target_str = te.entry.target.as_deref().unwrap_or("-");
                    let reason_str = te.entry.reason.as_deref().unwrap_or("-");
                    let _target_display = if target_str.len() > 19 { format!("{}…", &target_str[..18]) } else { target_str.to_string() };
                    let reason_display = if reason_str.len() > 35 { format!("{}…", &reason_str[..34]) } else { reason_str.to_string() };

                    // Highlight reason in red for errors/denials, dim otherwise
                    let reason_colored = match &te.entry.status {
                        EntryStatus::Error => format!("{}{}{}{}", color::BRIGHT_RED, color::BOLD, reason_display, color::RESET),
                        EntryStatus::Denied => format!("{}{}{}{}", color::YELLOW, color::BOLD, reason_display, color::RESET),
                        _ => color::dim(&reason_display),
                    };

                    println!(
                        "{} {:<24} {} {} {} {} {}",
                        color::seq(te.entry.event_seq),
                        te.entry.timestamp,
                        color::agent(&te.agent_id),
                        color::category(&te.entry.category),
                        color::action(&te.entry.action),
                        color::status_label(&format!("{:?}", te.entry.status)),
                        reason_colored,
                    );

                    // Show tool-specific info for tool_invoke events
                    if te.entry.category == "tool_invoke" {
                        if let Some(ref payload) = te.entry.payload {
                            if let Some(tool_name) = payload.get("tool_name").and_then(|v| v.as_str()) {
                                let args_preview = payload.get("arguments")
                                    .and_then(|v| v.as_str())
                                    .map(|a| {
                                        if a.len() > 80 { format!("{}…", &a[..79]) } else { a.to_string() }
                                    })
                                    .unwrap_or_default();
                                let result_preview = payload.get("result_preview")
                                    .and_then(|v| v.as_str())
                                    .map(|r| {
                                        if r.len() > 80 { format!("{}…", &r[..79]) } else { r.to_string() }
                                    })
                                    .unwrap_or_default();

                                if te.entry.action == "requested" && !args_preview.is_empty() {
                                    println!("      {}├─ {}({}){}", color::DIM, color::tool_name(tool_name), color::dim(&args_preview), color::RESET);
                                } else if te.entry.action == "completed" && !result_preview.is_empty() {
                                    println!("      {}├─ {} → {}{}", color::DIM, color::tool_name(tool_name), color::dim(&result_preview), color::RESET);
                                } else {
                                    println!("      {}├─ {}{}", color::DIM, color::tool_name(tool_name), color::RESET);
                                }
                            }
                        }
                    } else {
                        // Generic payload for non-tool events
                        if let Some(ref payload) = te.entry.payload {
                            let payload_str = serde_json::to_string(payload).unwrap_or_default();
                            if payload_str.len() > 2 && payload_str != "null" {
                                let truncated = if payload_str.len() > 120 { format!("{}…", &payload_str[..119]) } else { payload_str };
                                println!("      {}├─ payload: {}{}{}", color::DIM, color::BRIGHT_BLUE, truncated, color::RESET);
                            }
                        }
                    }
                }
            }
        }
    }
}

/// Handle `autonoetic trace fork` command.
pub async fn handle_trace_fork(
    config_path: &Path,
    source_session_id: &str,
    branch_message: Option<&str>,
    new_session_id: Option<&str>,
    at_turn: Option<usize>,
    agent_id: Option<&str>,
    interactive: bool,
    json_output: bool,
) -> anyhow::Result<()> {
    let config = autonoetic_gateway::config::load_config(config_path)?;
    let gateway_dir = config.agents_dir.join(".gateway");
    let _store = autonoetic_gateway::runtime::content_store::ContentStore::new(&gateway_dir)?;

    // Load snapshot from source session
    let mut snapshot =
        autonoetic_gateway::runtime::session_snapshot::SessionSnapshot::load_from_session(
            source_session_id,
            &gateway_dir,
        )?;

    // If at_turn is specified, truncate history to that turn
    if let Some(turn) = at_turn {
        // Each turn is typically user + assistant messages, so we estimate:
        // turn 1 = 2 messages (user, assistant), turn 2 = 4 messages, etc.
        // But we need to be more precise - find the turn boundaries
        let target_message_count = turn * 2; // Approximate: user + assistant per turn
        if target_message_count < snapshot.history.len() {
            snapshot.history.truncate(target_message_count);
            snapshot.turn_count = turn;
        }
    }

    // Fork the session
    let fork = autonoetic_gateway::runtime::session_snapshot::SessionFork::fork(
        &snapshot,
        new_session_id,
        branch_message,
        &gateway_dir,
    )?;

    if !json_output {
        println!("Session forked successfully!");
        println!("  Source session:    {}", fork.source_session_id);
        println!("  New session:       {}", fork.new_session_id);
        println!("  Fork turn:         {}", fork.fork_turn);
        if let Some(turn) = at_turn {
            println!("  Forked at turn:    {}", turn);
        }
        println!("  History messages:  {}", fork.initial_history.len());
        println!("  History handle:    {}", fork.history_handle);
        if let Some(msg) = branch_message {
            println!("  Branch message:    {}", msg);
        }
    }

    // If interactive mode, start a chat session with the forked session
    if interactive {
        if json_output {
            // In JSON mode, output the fork info first
            println!(
                "{}",
                serde_json::to_string_pretty(&serde_json::json!({
                    "new_session_id": fork.new_session_id,
                    "source_session_id": fork.source_session_id,
                    "fork_turn": fork.fork_turn,
                    "history_handle": fork.history_handle,
                    "message_count": fork.initial_history.len(),
                    "at_turn": at_turn,
                }))?
            );
        }

        println!();
        println!("Starting interactive session with forked session...");
        println!("Type /exit to quit.");
        println!();

        // Use the existing chat functionality to continue the session
        let chat_args = super::common::ChatArgs {
            agent_id: agent_id.map(|a| a.to_string()),
            session_id: Some(fork.new_session_id.clone()),
            sender_id: None,
            channel_id: None,
            test_mode: false,
        };
        super::chat::handle_chat(config_path, &chat_args).await?;
    } else if json_output {
        println!(
            "{}",
            serde_json::to_string_pretty(&serde_json::json!({
                "new_session_id": fork.new_session_id,
                "source_session_id": fork.source_session_id,
                "fork_turn": fork.fork_turn,
                "history_handle": fork.history_handle,
                "message_count": fork.initial_history.len(),
                "at_turn": at_turn,
            }))?
        );
    }

    Ok(())
}

/// Handle `autonoetic trace history` command.
pub fn handle_trace_history(
    config_path: &Path,
    session_id: &str,
    _requested_agent: Option<&str>,
    json_output: bool,
) -> anyhow::Result<()> {
    let config = autonoetic_gateway::config::load_config(config_path)?;
    let gateway_dir = config.agents_dir.join(".gateway");
    let store = autonoetic_gateway::runtime::content_store::ContentStore::new(&gateway_dir)?;

    // Try to load history from session
    let history = store.read_by_name(session_id, "session_history");

    match history {
        Ok(content) => {
            let messages: Vec<Message> = serde_json::from_slice(&content)?;

            if json_output {
                println!(
                    "{}",
                    serde_json::to_string_pretty(&serde_json::json!({
                        "session_id": session_id,
                        "message_count": messages.len(),
                        "messages": messages.iter().map(|m| serde_json::json!({
                            "role": format!("{:?}", m.role),
                            "content": m.content,
                        })).collect::<Vec<_>>(),
                    }))?
                );
            } else {
                println!("Session history: {} messages", messages.len());
                println!();
                for (i, msg) in messages.iter().enumerate() {
                    let role = match msg.role {
                        autonoetic_gateway::llm::Role::System => "system",
                        autonoetic_gateway::llm::Role::User => "user",
                        autonoetic_gateway::llm::Role::Assistant => "assistant",
                        autonoetic_gateway::llm::Role::Tool => "tool",
                    };
                    println!("[{}] {}: {}", i + 1, role, msg.content);
                }
            }
        }
        Err(_) => {
            if json_output {
                println!(
                    "{}",
                    serde_json::to_string_pretty(&serde_json::json!({
                        "session_id": session_id,
                        "error": "History not found",
                        "message_count": 0,
                        "messages": [],
                    }))?
                );
            } else {
                println!("No history found for session '{}'.", session_id);
                println!("The session may not have been snapshotted yet.");
            }
        }
    }

    Ok(())
}

fn shorten_json_value(payload: &serde_json::Value, max_chars: usize) -> String {
    match payload {
        serde_json::Value::Null => String::new(),
        serde_json::Value::Object(o) if o.is_empty() => String::new(),
        _ => {
            let s = payload.to_string();
            let count = s.chars().count();
            if count <= max_chars {
                s
            } else {
                let take: String = s.chars().take(max_chars).collect();
                format!("{take}…")
            }
        }
    }
}

fn print_workflow_event_row(ev: &WorkflowEventRecord, json_output: bool) -> anyhow::Result<()> {
    if json_output {
        println!("{}", serde_json::to_string(ev)?);
        return Ok(());
    }
    let task = ev.task_id.as_deref().unwrap_or("-");
    let date_short: String = ev.occurred_at.chars().take(10).collect();
    println!(
        "{}{:<10} {:<28} {:<36} {:<18} {}",
        color::DIM,
        date_short,
        ev.event_type,
        ev.event_id,
        task,
        color::RESET
    );
    let p = shorten_json_value(&ev.payload, 56);
    if !p.is_empty() {
        println!("  {}payload:{} {}", color::DIM, color::RESET, p);
    }
    Ok(())
}

fn print_workflow_events_table(
    workflow_id: &str,
    run: Option<&WorkflowRun>,
    events: &[WorkflowEventRecord],
    json_output: bool,
) -> anyhow::Result<()> {
    if json_output {
        println!(
            "{}",
            serde_json::to_string_pretty(&serde_json::json!({
                "workflow_id": workflow_id,
                "workflow": run,
                "events": events,
            }))?
        );
        return Ok(());
    }

    println!(
        "{}Workflow{} {}  ({} events)",
        color::BOLD,
        color::RESET,
        workflow_id,
        events.len()
    );
    if let Some(r) = run {
        println!(
            "{}  root_session: {}  status: {:?}{}",
            color::DIM,
            r.root_session_id,
            r.status,
            color::RESET
        );
    }
    println!();

    if events.is_empty() {
        println!("{}No events in events.jsonl.{}", color::DIM, color::RESET);
        return Ok(());
    }

    println!(
        "{}{}{:<10} {:<28} {:<36} {:<18} {}",
        color::DIM,
        color::BOLD,
        "DATE",
        "TYPE",
        "EVENT_ID",
        "TASK",
        color::RESET
    );
    println!("{}", color::separator(120));
    for ev in events {
        print_workflow_event_row(ev, false)?;
    }
    Ok(())
}

/// Print durable workflow store events (`events.jsonl`), optionally following new lines.
pub async fn handle_trace_workflow(
    config_path: &Path,
    workflow_or_root: &str,
    as_root: bool,
    json_output: bool,
    follow: bool,
) -> anyhow::Result<()> {
    let config = autonoetic_gateway::config::load_config(config_path)?;
    let workflow_id = if as_root {
        match autonoetic_gateway::scheduler::resolve_workflow_id_for_root_session(
            &config,
            workflow_or_root,
        )? {
            Some(w) => w,
            None => anyhow::bail!(
                "No workflow index for root session '{}'. (Has `agent.spawn` run for this root?)",
                workflow_or_root
            ),
        }
    } else {
        workflow_or_root.to_string()
    };

    let run = autonoetic_gateway::scheduler::load_workflow_run(&config, None, &workflow_id)?;
    if !follow && run.is_none() {
        anyhow::bail!(
            "No workflow run '{}' in gateway scheduler store.",
            workflow_id
        );
    }

    if follow {
        trace_workflow_follow(&config, &workflow_id, run.as_ref(), json_output).await
    } else {
        let events =
            autonoetic_gateway::scheduler::load_workflow_events(&config, None, &workflow_id)?;
        print_workflow_events_table(&workflow_id, run.as_ref(), &events, json_output)?;
        Ok(())
    }
}

async fn trace_workflow_follow(
    config: &autonoetic_gateway::GatewayConfig,
    workflow_id: &str,
    run: Option<&WorkflowRun>,
    json_output: bool,
) -> anyhow::Result<()> {
    use std::collections::HashSet;
    use tokio::time::{interval, Duration};

    let mut seen: HashSet<String> = HashSet::new();
    let mut poll_interval = interval(Duration::from_secs(1));

    println!(
        "{}Following workflow '{}'.{} Press Ctrl+C to stop.",
        color::BOLD,
        workflow_id,
        color::RESET
    );
    if let Some(r) = run {
        println!(
            "{}  root_session: {}  status: {:?}{}",
            color::DIM,
            r.root_session_id,
            r.status,
            color::RESET
        );
    }
    println!();

    if !json_output {
        println!(
            "{}{}{:<10} {:<28} {:<36} {:<18} {}",
            color::DIM,
            color::BOLD,
            "DATE",
            "TYPE",
            "EVENT_ID",
            "TASK",
            color::RESET
        );
        println!("{}", color::separator(120));
    }

    loop {
        poll_interval.tick().await;
        let events = autonoetic_gateway::scheduler::load_workflow_events(config, None, workflow_id)?;
        for ev in events {
            if seen.insert(ev.event_id.clone()) {
                print_workflow_event_row(&ev, json_output)?;
            }
        }
    }
}

// ---------------------------------------------------------------------------
// trace graph (workflow store projection, Phase 7)
// ---------------------------------------------------------------------------

fn workflow_status_snake(s: WorkflowRunStatus) -> &'static str {
    match s {
        WorkflowRunStatus::Active => "active",
        WorkflowRunStatus::WaitingChildren => "waiting_children",
        WorkflowRunStatus::BlockedApproval => "blocked_approval",
        WorkflowRunStatus::Resumable => "resumable",
        WorkflowRunStatus::Completed => "completed",
        WorkflowRunStatus::Failed => "failed",
        WorkflowRunStatus::Cancelled => "cancelled",
    }
}

fn task_status_snake(s: TaskRunStatus) -> &'static str {
    match s {
        TaskRunStatus::Pending => "pending",
        TaskRunStatus::Runnable => "runnable",
        TaskRunStatus::Running => "running",
        TaskRunStatus::AwaitingApproval => "awaiting_approval",
        TaskRunStatus::Paused => "paused",
        TaskRunStatus::Succeeded => "succeeded",
        TaskRunStatus::Failed => "failed",
        TaskRunStatus::Cancelled => "cancelled",
    }
}

fn resolve_workflow_id_for_graph(
    config: &autonoetic_gateway::GatewayConfig,
    session_or_wf: &str,
) -> anyhow::Result<String> {
    let s = session_or_wf.trim();
    if s.starts_with("wf-") {
        if autonoetic_gateway::scheduler::load_workflow_run(config, None, s)?.is_none() {
            anyhow::bail!("No workflow run '{}' in gateway scheduler store.", s);
        }
        return Ok(s.to_string());
    }
    match autonoetic_gateway::scheduler::resolve_workflow_id_for_root_session(config, s)? {
        Some(w) => Ok(w),
        None => anyhow::bail!(
            "No workflow for root session '{}'. Pass the root session used with `agent.spawn`, or a `wf-…` id from `trace workflow`.",
            s
        ),
    }
}

#[derive(Debug, Serialize)]
struct WorkflowGraphTaskView {
    task_id: String,
    agent_id: String,
    session_id: String,
    parent_session_id: String,
    status: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    result_summary: Option<String>,
}

#[derive(Debug, Serialize)]
struct WorkflowGraphEventView {
    event_type: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    task_id: Option<String>,
    occurred_at: String,
}

#[derive(Debug, Serialize)]
struct WorkflowGraphView {
    workflow_id: String,
    root_session_id: String,
    workflow_status: String,
    lead_agent_id: String,
    active_task_ids: Vec<String>,
    blocked_task_ids: Vec<String>,
    pending_approval_ids: Vec<String>,
    tasks: Vec<WorkflowGraphTaskView>,
    recent_events: Vec<WorkflowGraphEventView>,
}

fn build_workflow_graph_view(
    config: &autonoetic_gateway::GatewayConfig,
    run: &WorkflowRun,
) -> anyhow::Result<WorkflowGraphView> {
    let tasks = autonoetic_gateway::scheduler::list_task_runs_for_workflow(
        config,
        None,
        &run.workflow_id,
    )?;
    let events = autonoetic_gateway::scheduler::load_workflow_events(config, None, &run.workflow_id)?;
    let start = events.len().saturating_sub(12);
    let recent_slice = &events[start..];

    let task_views: Vec<WorkflowGraphTaskView> = tasks
        .into_iter()
        .map(|t: TaskRun| WorkflowGraphTaskView {
            task_id: t.task_id,
            agent_id: t.agent_id,
            session_id: t.session_id,
            parent_session_id: t.parent_session_id,
            status: task_status_snake(t.status).to_string(),
            result_summary: t.result_summary,
        })
        .collect();

    let event_views: Vec<WorkflowGraphEventView> = recent_slice
        .iter()
        .map(|e| WorkflowGraphEventView {
            event_type: e.event_type.clone(),
            task_id: e.task_id.clone(),
            occurred_at: e.occurred_at.clone(),
        })
        .collect();

    Ok(WorkflowGraphView {
        workflow_id: run.workflow_id.clone(),
        root_session_id: run.root_session_id.clone(),
        workflow_status: workflow_status_snake(run.status).to_string(),
        lead_agent_id: run.lead_agent_id.clone(),
        active_task_ids: run.active_task_ids.clone(),
        blocked_task_ids: run.blocked_task_ids.clone(),
        pending_approval_ids: run.pending_approval_ids.clone(),
        tasks: task_views,
        recent_events: event_views,
    })
}

fn print_workflow_graph_text(view: &WorkflowGraphView) {
    println!(
        "{}workflow{} {}  {}wf={}{}  [{}]",
        color::BOLD,
        color::RESET,
        view.root_session_id,
        color::DIM,
        view.workflow_id,
        color::RESET,
        color::status_label(&view.workflow_status)
    );
    let lead = if view.lead_agent_id.is_empty() {
        format!("{}(unknown){}", color::DIM, color::RESET)
    } else {
        color::agent(&view.lead_agent_id)
    };
    println!(
        "planner {}  [{}]",
        lead,
        color::status_label(&view.workflow_status)
    );

    if view.tasks.is_empty() {
        println!("{}  (no delegated tasks yet){}", color::DIM, color::RESET);
    } else {
        for t in &view.tasks {
            println!(
                "|- {}{}#{}  [{}]",
                color::agent(&t.agent_id),
                color::RESET,
                t.task_id,
                color::status_label(&t.status)
            );
            println!(
                "   {}session:{} {}",
                color::DIM,
                color::RESET,
                t.session_id
            );
        }
    }

    if !view.pending_approval_ids.is_empty() {
        println!(
            "{}pending_approvals:{} {}",
            color::YELLOW,
            color::RESET,
            view.pending_approval_ids.join(", ")
        );
    }
    if !view.active_task_ids.is_empty() {
        println!(
            "{}active_task_ids:{} {}",
            color::DIM,
            color::RESET,
            view.active_task_ids.join(", ")
        );
    }
    if !view.blocked_task_ids.is_empty() {
        println!(
            "{}blocked_task_ids:{} {}",
            color::DIM,
            color::RESET,
            view.blocked_task_ids.join(", ")
        );
    }

    if !view.recent_events.is_empty() {
        println!();
        println!("{}recent workflow events:{}", color::BOLD, color::RESET);
        for e in &view.recent_events {
            let tid = e
                .task_id
                .as_deref()
                .map(|s| format!(" ({s})"))
                .unwrap_or_default();
            println!(
                "  {}{} {}{} {}",
                color::DIM,
                &e.occurred_at.chars().take(19).collect::<String>(),
                color::RESET,
                e.event_type,
                tid
            );
        }
    }
}

fn print_workflow_graph(view: &WorkflowGraphView, json_output: bool) -> anyhow::Result<()> {
    if json_output {
        println!("{}", serde_json::to_string_pretty(view)?);
    } else {
        print_workflow_graph_text(view);
    }
    Ok(())
}

/// Text tree + recent events from the durable workflow store (`trace graph`).
pub async fn handle_trace_graph(
    config_path: &Path,
    session_or_wf: &str,
    json_output: bool,
    follow: bool,
) -> anyhow::Result<()> {
    use std::io::{stdout, Write};
    use tokio::time::{interval, Duration};

    let config = autonoetic_gateway::config::load_config(config_path)?;
    let workflow_id = resolve_workflow_id_for_graph(&config, session_or_wf)?;

    let run = autonoetic_gateway::scheduler::load_workflow_run(&config, None, &workflow_id)?
        .ok_or_else(|| anyhow::anyhow!("workflow run '{}' disappeared", workflow_id))?;

    if !follow {
        let view = build_workflow_graph_view(&config, &run)?;
        return print_workflow_graph(&view, json_output);
    }

    if !json_output {
        println!(
            "{}Following workflow graph ({}).{} Press Ctrl+C to stop.",
            color::BOLD,
            workflow_id,
            color::RESET
        );
        println!();
    }

    let mut poll_interval = interval(Duration::from_secs(1));
    loop {
        poll_interval.tick().await;
        let run = match autonoetic_gateway::scheduler::load_workflow_run(&config, None, &workflow_id)? {
            Some(r) => r,
            None => {
                tracing::warn!("workflow run removed while following");
                continue;
            }
        };
        let view = match build_workflow_graph_view(&config, &run) {
            Ok(v) => v,
            Err(e) => {
                tracing::warn!(error = %e, "rebuild workflow graph view failed");
                continue;
            }
        };
        if json_output {
            println!("{}", serde_json::to_string(&view)?);
        } else {
            print!("\x1b[2J\x1b[H");
            let _ = stdout().flush();
            print_workflow_graph_text(&view);
            println!();
            println!(
                "{}— refreshed — {}Ctrl+C to stop{}",
                color::DIM,
                color::DIM,
                color::RESET
            );
        }
    }
}
