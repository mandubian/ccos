use std::io::{BufRead, BufReader as StdBufReader};
use std::path::Path;

use super::common::AgentTrace;
use autonoetic_gateway::llm::Message;
use autonoetic_types::causal_chain::CausalChainEntry;

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
        "{:<30} {:<38} {:<26} {:<26} {:<8} {:<10}",
        "AGENT", "SESSION ID", "FIRST TS", "LAST TS", "EVENTS", "MAX SEQ"
    );
    for s in sessions {
        println!(
            "{:<30} {:<38} {:<26} {:<26} {:<8} {:<10}",
            s.agent_id,
            s.session_id,
            s.first_timestamp,
            s.last_timestamp,
            s.event_count,
            s.max_event_seq
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

    println!("Agent: {}", agent_id);
    println!("Session: {}", session_id);
    println!(
        "{:<10} {:<28} {:<10} {:<24} {}",
        "EVENT_SEQ", "TIMESTAMP", "STATUS", "CATEGORY.ACTION", "LOG_ID"
    );
    for entry in entries {
        println!(
            "{:<10} {:<28} {:<10} {:<24} {}",
            entry.event_seq,
            entry.timestamp,
            format!("{:?}", entry.status),
            format!("{}.{}", entry.category, entry.action),
            entry.log_id
        );
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
        println!("Session Reconstruction: {}", session_id);
        println!("Total events: {}", all_events.len());
        println!();
        
        if !integrity_issues.is_empty() {
            println!("Integrity Issues:");
            for issue in &integrity_issues {
                println!("  - {}", issue);
            }
            println!();
        }
        
        println!("{:<10} {:<30} {:<30} {:<20} {:<15}",
            "SEQ", "TIMESTAMP", "AGENT", "ACTION", "STATUS");
        println!("{}", "-".repeat(105));
        
        for te in &all_events {
            println!("{:<10} {:<30} {:<30} {:<20} {:<15}",
                te.entry.event_seq,
                te.entry.timestamp,
                te.agent_id,
                te.entry.action,
                format!("{:?}", te.entry.status)
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

    println!("Following session '{}'. Press Ctrl+C to stop.", session_id);
    println!();

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
                            "action": te.entry.action,
                            "status": te.entry.status,
                            "event_seq": te.entry.event_seq,
                        }))?
                    );
                } else {
                    println!("{:<30} {:<30} {:<20} {:<15}",
                        te.entry.timestamp,
                        te.agent_id,
                        te.entry.action,
                        format!("{:?}", te.entry.status)
                    );
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
