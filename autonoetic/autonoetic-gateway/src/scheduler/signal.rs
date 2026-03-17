//! Signal system for inter-component communication.
//!
//! Signals are lightweight JSON files written to a shared directory
//! that allow asynchronous communication between the gateway and CLI.
//!
//! Primary use case: Notify CLI that an approval has been resolved,
//! enabling automatic session resume without user intervention.
//!
//! Signal location: `{gateway_dir}/signal/{session_id}/{request_id}.json`

use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

/// Signal types that can be sent between components.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum Signal {
    /// Approval has been resolved (approved, rejected, or timed out)
    ApprovalResolved {
        request_id: String,
        agent_id: String,
        status: String, // "approved", "rejected", "timed_out"
        install_completed: bool,
        message: String,
        timestamp: String,
    },
}

/// Write a signal file to the signal directory.
pub fn write_signal(
    gateway_dir: &Path,
    session_id: &str,
    request_id: &str,
    signal: &Signal,
) -> anyhow::Result<PathBuf> {
    let signal_dir = gateway_dir.join("signal").join(session_id);
    std::fs::create_dir_all(&signal_dir)?;

    let signal_path = signal_dir.join(format!("{}.json", request_id));
    let signal_json = serde_json::to_string_pretty(signal)?;
    std::fs::write(&signal_path, &signal_json)?;

    tracing::info!(
        target: "signal",
        session_id = %session_id,
        request_id = %request_id,
        path = %signal_path.display(),
        "Signal written"
    );

    Ok(signal_path)
}

/// Signal file content with the filename for tracking.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PendingSignal {
    pub request_id: String,
    pub signal: Signal,
    pub filename: String,
}

/// Check for pending signals in a session's signal directory.
/// Returns all signals that haven't been consumed yet.
pub fn check_pending_signals(
    gateway_dir: &Path,
    session_id: &str,
) -> anyhow::Result<Vec<PendingSignal>> {
    let signal_dir = gateway_dir.join("signal").join(session_id);

    if !signal_dir.exists() {
        return Ok(Vec::new());
    }

    let mut signals = Vec::new();

    for entry in std::fs::read_dir(&signal_dir)? {
        let entry = entry?;
        let path = entry.path();

        if path.extension().and_then(|e| e.to_str()) == Some("json") {
            let filename = path
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or("")
                .to_string();

            // Extract request_id from filename (remove .json extension)
            let request_id = filename.trim_end_matches(".json").to_string();

            let content = std::fs::read_to_string(&path)?;
            match serde_json::from_str::<Signal>(&content) {
                Ok(signal) => {
                    signals.push(PendingSignal {
                        request_id,
                        signal,
                        filename: filename.clone(),
                    });
                }
                Err(e) => {
                    tracing::warn!(
                        target: "signal",
                        path = %path.display(),
                        error = %e,
                        "Failed to parse signal file"
                    );
                }
            }
        }
    }

    // Sort by request_id for consistent ordering
    signals.sort_by(|a, b| a.request_id.cmp(&b.request_id));

    Ok(signals)
}

/// Consume (delete) a signal file after processing.
pub fn consume_signal(
    gateway_dir: &Path,
    session_id: &str,
    request_id: &str,
) -> anyhow::Result<()> {
    let signal_path = gateway_dir
        .join("signal")
        .join(session_id)
        .join(format!("{}.json", request_id));

    if signal_path.exists() {
        std::fs::remove_file(&signal_path)?;
        tracing::info!(
            target: "signal",
            session_id = %session_id,
            request_id = %request_id,
            "Signal consumed"
        );
    }

    Ok(())
}

/// Consume all signals in a session's signal directory.
/// Returns the list of consumed request IDs.
pub fn consume_all_signals(gateway_dir: &Path, session_id: &str) -> anyhow::Result<Vec<String>> {
    let signals = check_pending_signals(gateway_dir, session_id)?;
    let mut consumed = Vec::new();

    for pending in signals {
        consume_signal(gateway_dir, session_id, &pending.request_id)?;
        consumed.push(pending.request_id);
    }

    Ok(consumed)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn test_signal_write_and_read() {
        let temp = tempdir().unwrap();
        let gateway_dir = temp.path();
        let session_id = "test-session";
        let request_id = "apr-12345678";

        let signal = Signal::ApprovalResolved {
            request_id: request_id.to_string(),
            agent_id: "weather.script".to_string(),
            status: "approved".to_string(),
            install_completed: true,
            message: "Agent installed successfully".to_string(),
            timestamp: "2026-03-17T10:00:00Z".to_string(),
        };

        // Write signal
        write_signal(gateway_dir, session_id, request_id, &signal).unwrap();

        // Check pending
        let pending = check_pending_signals(gateway_dir, session_id).unwrap();
        assert_eq!(pending.len(), 1);
        assert_eq!(pending[0].request_id, request_id);

        // Consume signal
        consume_signal(gateway_dir, session_id, request_id).unwrap();

        // Check empty after consume
        let pending = check_pending_signals(gateway_dir, session_id).unwrap();
        assert_eq!(pending.len(), 0);
    }
}
