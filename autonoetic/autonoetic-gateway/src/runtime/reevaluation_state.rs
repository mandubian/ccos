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
    _config: Option<&autonoetic_types::config::GatewayConfig>,
) -> anyhow::Result<String> {
    let policy = PolicyEngine::new(manifest.clone());
    match action {
        ScheduledAction::AgentInstall {
            agent_id,
            summary: _,
            ..
        } => {
            // Approval-only: we do not run an install here. The caller retries agent.install with install_approval_ref to perform the install.
            Ok(serde_json::json!({
                "ok": true,
                "kind": "agent_install_approval_resolved",
                "agent_id": agent_id
            })
            .to_string())
        }
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
            let result = registry.execute(
                "sandbox.exec",
                manifest,
                &policy,
                agent_dir,
                None,
                &args,
                None,
                None,
                _config,
            )?;

            let parsed: serde_json::Value = serde_json::from_str(&result).map_err(|error| {
                anyhow::anyhow!("sandbox.exec returned non-JSON result: {error}")
            })?;

            let ok = parsed
                .get("ok")
                .and_then(|value| value.as_bool())
                .unwrap_or(false);
            anyhow::ensure!(
                ok,
                "scheduled sandbox_exec failed: {}",
                parsed
                    .get("stderr")
                    .and_then(|value| value.as_str())
                    .unwrap_or("unknown error")
            );

            Ok(result)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use autonoetic_types::agent::{AgentIdentity, RuntimeDeclaration};

    fn minimal_manifest() -> AgentManifest {
        AgentManifest {
            version: "1.0".to_string(),
            runtime: RuntimeDeclaration {
                engine: "autonoetic".to_string(),
                gateway_version: "0.1.0".to_string(),
                sdk_version: "0.1.0".to_string(),
                runtime_type: "stateful".to_string(),
                sandbox: "bubblewrap".to_string(),
                runtime_lock: "runtime.lock".to_string(),
            },
            agent: AgentIdentity {
                id: "caller-agent".to_string(),
                name: "caller-agent".to_string(),
                description: "Test".to_string(),
            },
            capabilities: vec![],
            llm_config: None,
            limits: None,
            background: None,
            disclosure: None,
            adaptation_hooks: None,
        }
    }

    /// Regression: AgentInstall is not executed by the scheduler; it only resolves as approval
    /// metadata. The background path returns a success payload and performs no install.
    #[test]
    fn test_agent_install_in_background_path_resolves_as_approval_metadata_only() {
        let action = ScheduledAction::AgentInstall {
            agent_id: "would-be-child".to_string(),
            summary: "Test install".to_string(),
            requested_by_agent_id: "caller-agent".to_string(),
            install_fingerprint: "abc123".to_string(),
        };
        assert!(
            !action.is_executable_by_scheduler(),
            "AgentInstall must not be considered executable by the scheduler"
        );

        let manifest = minimal_manifest();
        let temp = tempfile::tempdir().expect("tempdir");
        let agent_dir = temp.path();
        let registry = crate::runtime::tools::default_registry();

        let result = execute_scheduled_action(&manifest, agent_dir, &action, &registry, None)
            .expect(
            "execute_scheduled_action(AgentInstall) must succeed with approval-resolved payload",
        );

        let json: serde_json::Value = serde_json::from_str(&result).expect("result must be JSON");
        assert_eq!(
            json.get("ok").and_then(|v| v.as_bool()),
            Some(true),
            "payload must indicate success"
        );
        assert_eq!(
            json.get("kind").and_then(|v| v.as_str()),
            Some("agent_install_approval_resolved"),
            "payload must be approval-resolution only, not an actual install"
        );
        assert_eq!(
            json.get("agent_id").and_then(|v| v.as_str()),
            Some("would-be-child"),
            "agent_id must echo the approval subject"
        );

        // No install must have occurred: no new agent directory under agent_dir
        let state_dir = agent_dir.join("state");
        let skills_dir = agent_dir.join("skills");
        assert!(!state_dir.exists(), "AgentInstall must not create state");
        assert!(!skills_dir.exists(), "AgentInstall must not create skills");
        assert!(
            std::fs::read_dir(agent_dir).map(|d| d.count()).unwrap_or(0) == 0,
            "agent_dir must remain empty; no install side-effects"
        );
    }
}
