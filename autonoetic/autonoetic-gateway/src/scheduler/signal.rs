//! Signal system for inter-component communication.
//!
//! Signals are lightweight JSON files written to a shared directory
//! that allow asynchronous communication between the gateway and CLI.
//!
//! Primary use case: Notify CLI that an approval has been resolved,
//! enabling automatic session resume without user intervention.
//!
//! Signal location: `{gateway_dir}/signal/{session_id}/{request_id}.json`

use autonoetic_types::config::GatewayConfig;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::sync::Once;

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

static POLLER_STARTED: Once = Once::new();

/// Start a background signal poller thread if not already started.
/// The poller scans the signal directory every 5 seconds and attempts to deliver
/// any pending signals via JSON-RPC event.ingest.
pub fn start_signal_poller_if_needed(agents_dir: PathBuf, port: u16) {
    POLLER_STARTED.call_once(move || {
        std::thread::spawn(move || {
            let rt = tokio::runtime::Runtime::new();
            match rt {
                Ok(runtime) => {
                    runtime.block_on(async {
                        loop {
                            tokio::time::sleep(std::time::Duration::from_secs(2)).await;
                            if let Err(e) = poll_pending_signals(&agents_dir, port).await {
                                tracing::warn!(
                                    target: "signal",
                                    error = %e,
                                    "Signal poller error"
                                );
                            }
                        }
                    });
                }
                Err(e) => {
                    tracing::error!(
                        target: "signal",
                        error = %e,
                        "Failed to create tokio runtime for signal poller"
                    );
                }
            }
        });
        tracing::info!(
            target: "signal",
            "Background signal poller started"
        );
    });
}

/// Poll pending signals in all session directories and attempt delivery.
async fn poll_pending_signals(agents_dir: &Path, port: u16) -> anyhow::Result<()> {
    let gateway_dir = agents_dir.join(".gateway");
    let signal_dir = gateway_dir.join("signal");
    if !signal_dir.exists() {
        tracing::info!(
            target: "signal",
            "Signal directory does not exist, skipping poll"
        );
        return Ok(());
    }
    
    tracing::info!(
        target: "signal",
        "Starting signal poll"
    );
    
    // Collect all session directories recursively
    let session_dirs = collect_session_directories(&signal_dir)?;
    if session_dirs.is_empty() {
        tracing::info!(
            target: "signal",
            "No signal directories found"
        );
        return Ok(());
    }
    
    tracing::info!(
        target: "signal",
        count = session_dirs.len(),
        "Found session directories with signals"
    );
    
    for (session_dir, session_id) in session_dirs {
        let pending = check_pending_signals(&gateway_dir, &session_id)?;
        if pending.is_empty() {
            continue;
        }
        tracing::info!(
            target: "signal",
            session_id = %session_id,
            count = pending.len(),
            "Processing pending signals"
        );
        for signal in pending {
            tracing::info!(
                target: "signal",
                request_id = %signal.request_id,
                session_id = %session_id,
                "Attempting to deliver signal"
            );
            // Attempt to deliver signal via JSON-RPC
            if let Err(e) = deliver_signal(&signal, &session_id, port).await {
                tracing::warn!(
                    target: "signal",
                    request_id = %signal.request_id,
                    session_id = %session_id,
                    error = %e,
                    "Failed to deliver signal, will retry later"
                );
                continue;
            }
            tracing::info!(
                target: "signal",
                request_id = %signal.request_id,
                session_id = %session_id,
                "Signal delivered successfully"
            );
            // Delivery succeeded, consume signal file
            if let Err(e) = consume_signal(&gateway_dir, &session_id, &signal.request_id) {
                tracing::warn!(
                    target: "signal",
                    request_id = %signal.request_id,
                    session_id = %session_id,
                    error = %e,
                    "Failed to consume signal file after delivery"
                );
            }
        }
    }
    tracing::info!(
        target: "signal",
        "Signal poll completed"
    );
    Ok(())
}

/// Recursively collect all directories under signal_dir that contain .json files.
/// Returns list of (absolute_path, session_id) where session_id is relative to signal_dir.
fn collect_session_directories(signal_dir: &Path) -> anyhow::Result<Vec<(PathBuf, String)>> {
    let mut result = Vec::new();
    collect_session_directories_recursive(signal_dir, signal_dir, &mut result)?;
    Ok(result)
}

fn collect_session_directories_recursive(
    base: &Path,
    current: &Path,
    result: &mut Vec<(PathBuf, String)>,
) -> anyhow::Result<()> {
    if !current.is_dir() {
        return Ok(());
    }
    
    // Check if directory contains any .json files
    let mut has_json = false;
    for entry in std::fs::read_dir(current)? {
        let entry = entry?;
        let path = entry.path();
        if path.is_file() && path.extension().and_then(|e| e.to_str()) == Some("json") {
            has_json = true;
            break;
        }
    }
    
    if has_json {
        let session_id = current
            .strip_prefix(base)
            .unwrap_or(current)
            .to_string_lossy()
            .replace('\\', "/"); // normalize for Windows
        result.push((current.to_path_buf(), session_id.to_string()));
    }
    
    // Recurse into subdirectories
    for entry in std::fs::read_dir(current)? {
        let entry = entry?;
        let path = entry.path();
        if path.is_dir() {
            collect_session_directories_recursive(base, &path, result)?;
        }
    }
    Ok(())
}

/// Deliver a single signal via JSON-RPC event.ingest to the gateway.
async fn deliver_signal(pending: &PendingSignal, session_id: &str, port: u16) -> anyhow::Result<()> {
    use crate::router::JsonRpcRequest;
    
    let request_id = &pending.request_id;
    let signal = &pending.signal;
    
    tracing::info!(
        target: "signal",
        request_id = %request_id,
        session_id = %session_id,
        "Delivering signal via JSON-RPC"
    );
    
    // Build message based on signal type
    let message = match signal {
        Signal::ApprovalResolved { message, .. } => message.clone(),
    };
    
    let request = JsonRpcRequest {
        jsonrpc: "2.0".to_string(),
        id: format!("signal-deliver-{}", request_id),
        method: "event.ingest".to_string(),
        params: serde_json::json!({
            "event_type": "chat",
            "message": message,
            "session_id": session_id,
            "metadata": {
                "sender_id": "gateway-signal-poller",
                "channel_id": format!("signal-poller-{}", session_id),
                "signal_delivered": true,
                "approval_request_id": request_id,
                "approval_status": match signal {
                    Signal::ApprovalResolved { status, .. } => status.clone(),
                },
            }
        }),
    };
    
    let gateway_addr = format!("127.0.0.1:{}", port);
    let addr = gateway_addr.clone();
    
    // Connect with retry (3 attempts)
    const MAX_ATTEMPTS: u32 = 3;
    for attempt in 1..=MAX_ATTEMPTS {
        let delay = std::time::Duration::from_secs(1 << (attempt - 1));
        if attempt > 1 {
            tokio::time::sleep(delay).await;
        }
        match tokio::net::TcpStream::connect(&addr).await {
            Ok(stream) => {
                use tokio::io::{AsyncWriteExt, BufWriter};
                use tokio::io::AsyncBufReadExt;
                
                let (read_half, write_half) = stream.into_split();
                let mut writer = BufWriter::new(write_half);
                let mut reader = tokio::io::BufReader::new(read_half);
                
                let encoded = serde_json::to_string(&request).unwrap_or_default();
                writer.write_all(encoded.as_bytes()).await?;
                writer.write_all(b"\n").await?;
                writer.flush().await?;
                
                tracing::info!(
                    target: "signal",
                    request_id = %request_id,
                    session_id = %session_id,
                    attempt,
                    "Signal delivered via JSON-RPC"
                );
                
                // Read response (don't block forever)
                let mut response_line = String::new();
                let read_result = tokio::time::timeout(
                    std::time::Duration::from_secs(2),
                    reader.read_line(&mut response_line)
                ).await;
                
                match read_result {
                    Ok(Ok(_)) => {
                        tracing::debug!(
                            target: "signal",
                            request_id = %request_id,
                            "Received response from gateway"
                        );
                    }
                    Ok(Err(e)) => {
                        tracing::warn!(
                            target: "signal",
                            error = %e,
                            "Failed to read response from gateway"
                        );
                    }
                    Err(_) => {
                        tracing::debug!(
                            target: "signal",
                            "Gateway response timeout (non-fatal)"
                        );
                    }
                }
                return Ok(());
            }
            Err(e) => {
                if attempt == MAX_ATTEMPTS {
                    return Err(anyhow::anyhow!(
                        "Failed to connect to gateway after {} attempts: {}",
                        MAX_ATTEMPTS, e
                    ));
                }
                continue;
            }
        }
    }
    Ok(())
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
