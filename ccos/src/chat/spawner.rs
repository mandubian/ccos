//! Agent Spawner for Gateway "Sheriff"
//!
//! Abstract interface for launching agent runtime workers.
//! The Gateway spawns agents; agents do the work.

use rtfs::runtime::error::{RuntimeError, RuntimeResult};
use std::future::Future;
use std::pin::Pin;
use std::process::Stdio;
use tokio::process::Command;

/// Result of spawning an agent
#[derive(Debug)]
pub struct SpawnResult {
    /// Process ID (if local process)
    pub pid: Option<u32>,
    /// Whether spawn was successful
    pub success: bool,
    /// Message about the spawn attempt
    pub message: String,
}

/// Configuration for spawning an agent
#[derive(Debug, Clone, Default)]
pub struct SpawnConfig {
    /// Maximum steps the agent can execute (0 = unlimited)
    pub max_steps: u32,
    /// Maximum duration in seconds (0 = unlimited)
    pub max_duration_secs: u64,
    /// Budget policy: "hard_stop" or "pause_approval"
    pub budget_policy: Option<String>,
    /// Optional run ID for correlation
    pub run_id: Option<String>,
}

impl SpawnConfig {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_max_steps(mut self, max_steps: u32) -> Self {
        self.max_steps = max_steps;
        self
    }

    pub fn with_max_duration_secs(mut self, secs: u64) -> Self {
        self.max_duration_secs = secs;
        self
    }

    pub fn with_budget_policy(mut self, policy: impl Into<String>) -> Self {
        self.budget_policy = Some(policy.into());
        self
    }

    pub fn with_run_id(mut self, run_id: impl Into<String>) -> Self {
        self.run_id = Some(run_id.into());
        self
    }
}

/// Trait for spawning agent runtimes
pub trait AgentSpawner: Send + Sync + std::fmt::Debug {
    /// Spawn an agent runtime for the given session
    fn spawn(
        &self,
        session_id: String,
        token: String,
        config: SpawnConfig,
    ) -> Pin<Box<dyn Future<Output = RuntimeResult<SpawnResult>> + Send>>;
}

/// Log-only spawner for testing - just logs what it would do
#[derive(Debug, Clone)]
pub struct LogOnlySpawner;

impl LogOnlySpawner {
    pub fn new() -> Self {
        Self
    }
}

impl AgentSpawner for LogOnlySpawner {
    fn spawn(
        &self,
        session_id: String,
        token: String,
        config: SpawnConfig,
    ) -> Pin<Box<dyn Future<Output = RuntimeResult<SpawnResult>> + Send>> {
        Box::pin(async move {
            log::info!(
                "[LogOnlySpawner] WOULD SPAWN AGENT for session {} with token {}... config={:?}",
                session_id,
                &token[..8.min(token.len())],
                config
            );

            // Simulate some delay
            tokio::time::sleep(tokio::time::Duration::from_millis(10)).await;

            Ok(SpawnResult {
                pid: None,
                success: true,
                message: format!("LogOnly: Would spawn agent for session {}", session_id),
            })
        })
    }
}

/// Process spawner - actually launches agent as child process
#[derive(Debug, Clone)]
pub struct ProcessSpawner {
    /// Path to the agent binary
    agent_binary: String,
    /// Environment variables to set
    env_vars: Vec<(String, String)>,
}

impl ProcessSpawner {
    pub fn new(agent_binary: impl Into<String>) -> Self {
        Self {
            agent_binary: agent_binary.into(),
            env_vars: Vec::new(),
        }
    }

    pub fn with_env_var(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.env_vars.push((key.into(), value.into()));
        self
    }
}

impl AgentSpawner for ProcessSpawner {
    fn spawn(
        &self,
        session_id: String,
        token: String,
        config: SpawnConfig,
    ) -> Pin<Box<dyn Future<Output = RuntimeResult<SpawnResult>> + Send>> {
        let binary = self.agent_binary.clone();
        let env_vars = self.env_vars.clone();

        Box::pin(async move {
            log::info!(
                "[ProcessSpawner] Spawning agent process for session {}: {} config={:?}",
                session_id,
                binary,
                config
            );

            let mut cmd = Command::new(&binary);
            cmd.arg("--session-id")
                .arg(&session_id)
                .arg("--token")
                .arg(&token)
                .stdin(Stdio::null())
                .stdout(Stdio::inherit())
                .stderr(Stdio::inherit());

            // Add budget parameters if set
            if config.max_steps > 0 {
                cmd.arg("--max-steps").arg(config.max_steps.to_string());
            }
            if config.max_duration_secs > 0 {
                cmd.arg("--max-duration-secs")
                    .arg(config.max_duration_secs.to_string());
            }
            if let Some(policy) = &config.budget_policy {
                cmd.arg("--budget-policy").arg(policy);
            }
            if let Some(run_id) = &config.run_id {
                cmd.arg("--run-id").arg(run_id);
            }

            // Add environment variables
            for (key, value) in env_vars {
                cmd.env(key, value);
            }

            match cmd.spawn() {
                Ok(child) => {
                    let pid = child.id();
                    log::info!("[ProcessSpawner] Agent spawned with PID: {:?}", pid);

                    // TODO: Monitor the child process
                    // For now, we just spawn it and return
                    // In production, we'd want to watch it and restart if it crashes

                    Ok(SpawnResult {
                        pid,
                        success: true,
                        message: format!("Agent spawned with PID {:?}", pid),
                    })
                }
                Err(e) => {
                    log::error!("[ProcessSpawner] Failed to spawn agent: {}", e);
                    Err(RuntimeError::Generic(format!(
                        "Failed to spawn agent: {}",
                        e
                    )))
                }
            }
        })
    }
}

/// Factory for creating appropriate spawner based on configuration
pub struct SpawnerFactory;

impl SpawnerFactory {
    /// Create a spawner based on environment/configuration
    pub fn create() -> Box<dyn AgentSpawner> {
        // For now, always return LogOnlySpawner
        // In the future, this could check env vars or config
        if std::env::var("CCOS_GATEWAY_SPAWN_AGENTS").is_ok() {
            // Try to find the agent binary
            let binary =
                std::env::var("CCOS_AGENT_BINARY").unwrap_or_else(|_| "ccos-agent".to_string());

            Box::new(ProcessSpawner::new(binary))
        } else {
            Box::new(LogOnlySpawner::new())
        }
    }

    /// Create a log-only spawner for testing
    pub fn create_log_only() -> Box<dyn AgentSpawner> {
        Box::new(LogOnlySpawner::new())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_log_only_spawner() {
        let spawner = LogOnlySpawner::new();
        let result = spawner
            .spawn(
                "test-session".to_string(),
                "test-token".to_string(),
                SpawnConfig::default(),
            )
            .await
            .unwrap();

        assert!(result.success);
        assert!(result.pid.is_none());
        assert!(result.message.contains("test-session"));
    }

    #[test]
    fn test_spawner_factory() {
        let spawner = SpawnerFactory::create_log_only();
        // Just verify it creates without panicking
        let _ = format!("{:?}", spawner);
    }
}
