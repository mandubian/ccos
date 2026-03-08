//! Persistence helpers for the background scheduler.
//! Handles loading and saving state, events, tasks, and approvals to disk.

use crate::execution::gateway_root_dir;
use autonoetic_types::background::BackgroundState;
use autonoetic_types::config::GatewayConfig;
use autonoetic_types::task_board::TaskBoardEntry;
use serde::{de::DeserializeOwned, Serialize};
use std::path::{Path, PathBuf};

pub fn scheduler_root(config: &GatewayConfig) -> PathBuf {
    gateway_root_dir(config).join("scheduler")
}

pub fn background_state_path(config: &GatewayConfig, agent_id: &str) -> PathBuf {
    scheduler_root(config)
        .join("agents")
        .join(format!("{agent_id}.json"))
}

pub fn inbox_path(config: &GatewayConfig, agent_id: &str) -> PathBuf {
    scheduler_root(config)
        .join("inbox")
        .join(format!("{agent_id}.jsonl"))
}

pub fn task_board_path(config: &GatewayConfig) -> PathBuf {
    scheduler_root(config).join("task_board.jsonl")
}

pub fn pending_approvals_dir(config: &GatewayConfig) -> PathBuf {
    scheduler_root(config).join("approvals").join("pending")
}

pub fn approved_approvals_dir(config: &GatewayConfig) -> PathBuf {
    scheduler_root(config).join("approvals").join("approved")
}

pub fn rejected_approvals_dir(config: &GatewayConfig) -> PathBuf {
    scheduler_root(config).join("approvals").join("rejected")
}

pub fn load_background_state(
    path: &Path,
    agent_id: &str,
    session_id: &str,
) -> anyhow::Result<BackgroundState> {
    if !path.exists() {
        return Ok(BackgroundState {
            agent_id: agent_id.to_string(),
            session_id: session_id.to_string(),
            ..BackgroundState::default()
        });
    }
    read_json_file(path)
}

pub fn save_background_state(path: &Path, state: &BackgroundState) -> anyhow::Result<()> {
    write_json_file(path, state)
}

pub fn load_inbox_events(
    config: &GatewayConfig,
    agent_id: &str,
) -> anyhow::Result<Vec<super::InboxEvent>> {
    load_jsonl_file(&inbox_path(config, agent_id))
}

pub fn load_task_board_entries(config: &GatewayConfig) -> anyhow::Result<Vec<TaskBoardEntry>> {
    load_jsonl_file(&task_board_path(config))
}

pub fn load_json_dir<T>(dir: &Path) -> anyhow::Result<Vec<T>>
where
    T: DeserializeOwned,
{
    if !dir.exists() {
        return Ok(Vec::new());
    }
    let mut entries = Vec::new();
    for entry in std::fs::read_dir(dir)? {
        let entry = entry?;
        if !entry.path().is_file() {
            continue;
        }
        entries.push(read_json_file(&entry.path())?);
    }
    Ok(entries)
}

pub fn load_jsonl_file<T>(path: &Path) -> anyhow::Result<Vec<T>>
where
    T: DeserializeOwned,
{
    if !path.exists() {
        return Ok(Vec::new());
    }
    let content = std::fs::read_to_string(path)?;
    content
        .lines()
        .filter(|line| !line.trim().is_empty())
        .map(|line| serde_json::from_str(line).map_err(anyhow::Error::from))
        .collect()
}

pub fn read_json_file<T>(path: &Path) -> anyhow::Result<T>
where
    T: DeserializeOwned,
{
    let body = std::fs::read_to_string(path)?;
    Ok(serde_json::from_str(&body)?)
}

pub fn append_jsonl_record<T>(path: &Path, value: &T) -> anyhow::Result<()>
where
    T: Serialize,
{
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let encoded = serde_json::to_string(value)?;
    let mut file = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)?;
    use std::io::Write;
    writeln!(file, "{encoded}")?;
    Ok(())
}

pub fn write_json_file<T>(path: &Path, value: &T) -> anyhow::Result<()>
where
    T: Serialize,
{
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(path, serde_json::to_string_pretty(value)?)?;
    Ok(())
}
