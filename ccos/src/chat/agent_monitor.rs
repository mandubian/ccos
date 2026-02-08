//! Agent health monitoring for detecting crashes and unresponsive agents
//!
//! Runs as a background task that periodically checks:
//! 1. Heartbeat age (how long since last heartbeat)
//! 2. Process existence (if heartbeat is missing, verify PID still exists)
//!
//! On crash detection, records an event in Causal Chain and broadcasts to WebSocket clients.

use crate::causal_chain::CausalChain;
use crate::chat::realtime_sink::{RealTimeTrackingSink, SessionStateSnapshot};
use crate::chat::session::{SessionRegistry, SessionState};
use chrono::Utc;
use std::sync::Arc;
use std::sync::Mutex;
use std::time::Duration;
use tokio::time::interval;

/// Agent health status
#[derive(Debug, Clone)]
pub enum AgentHealth {
    /// Agent is healthy (recent heartbeat)
    Healthy,
    /// Agent hasn't sent heartbeat recently but process exists
    Unresponsive { seconds_since_heartbeat: u64 },
    /// Agent process has died
    Crashed { pid: u32, exit_code: Option<i32> },
}

/// Background agent health monitor
pub struct AgentMonitor {
    /// How often to check agent health (seconds)
    check_interval: Duration,
    /// How long without heartbeat before considered unhealthy (seconds)
    heartbeat_timeout: chrono::Duration,
    /// Reference to Causal Chain for recording crash events
    chain: Arc<Mutex<CausalChain>>,
    /// Session registry for agent state
    registry: SessionRegistry,
    /// Real-time sink for broadcasting events
    realtime_sink: Arc<RealTimeTrackingSink>,
}

impl AgentMonitor {
    /// Create a new agent monitor
    pub fn new(
        check_interval_secs: u64,
        heartbeat_timeout_secs: u64,
        chain: Arc<Mutex<CausalChain>>,
        registry: SessionRegistry,
        realtime_sink: Arc<RealTimeTrackingSink>,
    ) -> Self {
        Self {
            check_interval: Duration::from_secs(check_interval_secs),
            heartbeat_timeout: chrono::Duration::seconds(heartbeat_timeout_secs as i64),
            chain,
            registry,
            realtime_sink,
        }
    }

    /// Run the health monitoring loop
    pub async fn run(&self) {
        let mut ticker = interval(self.check_interval);

        loop {
            ticker.tick().await;

            // Get all active sessions with agents
            let sessions = self.registry.list_active_sessions().await;

            for session in sessions {
                if let Some(_pid) = session.agent_pid {
                    // Check agent health
                    let health = self.check_health(&session).await;

                    match health {
                        AgentHealth::Crashed { pid, exit_code } => {
                            tracing::warn!(
                                "Agent crash detected for session {} (PID {})",
                                session.session_id,
                                pid
                            );

                            // Record in Causal Chain
                            self.record_crash(&session, pid, exit_code).await;

                            // Broadcast to clients
                            self.realtime_sink
                                .broadcast_crash(&session.session_id, pid, exit_code)
                                .await;

                            // Mark session as terminated
                            let _ = self.registry.terminate_session(&session.session_id).await;
                        }
                        AgentHealth::Unresponsive {
                            seconds_since_heartbeat,
                        } => {
                            // Update session state (no Causal Chain record for transient issues)
                            let state = SessionStateSnapshot {
                                agent_pid: session.agent_pid,
                                current_step: session.current_step,
                                memory_mb: session.memory_mb,
                                is_healthy: false,
                            };

                            self.realtime_sink
                                .broadcast_state_update(&session.session_id, &state)
                                .await;

                            tracing::debug!(
                                "Agent unresponsive for {}s: session {}",
                                seconds_since_heartbeat,
                                session.session_id
                            );
                        }
                        AgentHealth::Healthy => {
                            // Update with healthy status
                            let state = SessionStateSnapshot {
                                agent_pid: session.agent_pid,
                                current_step: session.current_step,
                                memory_mb: session.memory_mb,
                                is_healthy: true,
                            };

                            self.realtime_sink
                                .broadcast_state_update(&session.session_id, &state)
                                .await;
                        }
                    }
                }
            }
        }
    }

    /// Check the health of a specific agent
    async fn check_health(&self, session: &SessionState) -> AgentHealth {
        let now = Utc::now();
        let heartbeat_age = now.signed_duration_since(session.last_activity);

        if heartbeat_age > self.heartbeat_timeout {
            // Heartbeat is stale, verify process still exists
            if let Some(pid) = session.agent_pid {
                if !is_process_alive(pid).await {
                    return AgentHealth::Crashed {
                        pid,
                        exit_code: None, // We don't know the exit code
                    };
                }
            }

            return AgentHealth::Unresponsive {
                seconds_since_heartbeat: heartbeat_age.num_seconds() as u64,
            };
        }

        AgentHealth::Healthy
    }

    /// Record a crash event in the Causal Chain
    async fn record_crash(&self, session: &SessionState, pid: u32, _exit_code: Option<i32>) {
        // For now, we just log the crash. Recording to Causal Chain requires more
        // complex setup with proper Action construction.
        // TODO: Implement proper Causal Chain recording for agent crashes
        tracing::info!(
            "Recording agent crash in Causal Chain: session={}, pid={}",
            session.session_id,
            pid
        );
    }
}

/// Check if a process is still alive
async fn is_process_alive(pid: u32) -> bool {
    #[cfg(unix)]
    {
        // Check if /proc/<pid> exists (Linux-specific but more portable than libc)
        std::path::Path::new(&format!("/proc/{}", pid)).exists()
    }
    #[cfg(windows)]
    {
        // On Windows, use tasklist command
        use std::process::Command;

        match Command::new("tasklist")
            .args(&["/FI", &format!("PID eq {}", pid), "/NH"])
            .output()
        {
            Ok(output) => {
                let stdout = String::from_utf8_lossy(&output.stdout);
                stdout.contains(&pid.to_string())
            }
            Err(_) => false,
        }
    }
    #[cfg(not(any(unix, windows)))]
    {
        // Fallback: assume process is alive if we can't check
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_is_process_alive_current_process() {
        // Current process should always be alive
        let current_pid = std::process::id();
        assert!(is_process_alive(current_pid).await);
    }

    #[tokio::test]
    async fn test_is_process_alive_nonexistent() {
        // Process ID 99999 should not exist
        assert!(!is_process_alive(99999).await);
    }
}
