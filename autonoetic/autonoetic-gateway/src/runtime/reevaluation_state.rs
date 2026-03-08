//! Reevaluation State Management.
//!
//! Helpers for persisting and loading agent reevaluation state.

use crate::policy::PolicyEngine;
use crate::runtime::tools::NativeToolRegistry;
use autonoetic_types::agent::AgentManifest;
use autonoetic_types::background::{ReevaluationState, ScheduledAction};
use std::path::Path;

pub fn reevaluation_state_path(agent_dir: &Path) -> std::path::PathBuf {
    agent_dir.join("state").join("reevaluation.json")
}

pub fn load_reevaluation_state(agent_dir: &Path) -> anyhow::Result<ReevaluationState> {
    let path = reevaluation_state_path(agent_dir);
    if !path.exists() {
        return Ok(ReevaluationState::default());
    }
    let body = std::fs::read_to_string(path)?;
    Ok(serde_json::from_str(&body)?)
}

pub fn persist_reevaluation_state<F>(
    agent_dir: &Path,
    mutate: F,
) -> anyhow::Result<ReevaluationState>
where
    F: FnOnce(&mut ReevaluationState),
{
    let mut state = load_reevaluation_state(agent_dir)?;
    mutate(&mut state);
    let path = reevaluation_state_path(agent_dir);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(&path, serde_json::to_string_pretty(&state)?)?;
    Ok(state)
}

pub fn execute_scheduled_action(
    manifest: &AgentManifest,
    agent_dir: &Path,
    action: &ScheduledAction,
    registry: &NativeToolRegistry,
) -> anyhow::Result<String> {
    let policy = PolicyEngine::new(manifest.clone());
    match action {
        ScheduledAction::WriteFile { path, content, .. } => {
            anyhow::ensure!(
                !path.trim().is_empty(),
                "scheduled file path must not be empty"
            );
            anyhow::ensure!(
                !path.starts_with('/') && !path.split('/').any(|part| part == ".."),
                "scheduled file path must stay within the agent directory"
            );
            anyhow::ensure!(
                policy.can_write_path(path),
                "scheduled file write denied by MemoryWrite policy"
            );
            let target = agent_dir.join(path);
            if let Some(parent) = target.parent() {
                std::fs::create_dir_all(parent)?;
            }
            std::fs::write(&target, content)?;
            serde_json::to_string(
                &serde_json::json!({ "ok": true, "path": path, "bytes_written": content.len() }),
            )
            .map_err(Into::into)
        }
        ScheduledAction::SandboxExec {
            command,
            dependencies,
            ..
        } => {
            let args = serde_json::to_string(&serde_json::json!({
                "command": command,
                "dependencies": dependencies.as_ref().map(|deps| serde_json::json!({ "runtime": deps.runtime, "packages": deps.packages }))
            }))?;
            registry.execute("sandbox.exec", manifest, &policy, agent_dir, &args)
        }
    }
}
