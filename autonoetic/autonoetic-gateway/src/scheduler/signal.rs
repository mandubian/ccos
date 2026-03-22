//! Signal system for inter-component communication.
//!
//! Signals are persisted as notification records in the GatewayStore (SQLite)
//! and delivered asynchronously by the scheduler's notification pump via TCP
//! JSON-RPC (`event.ingest`). There are no filesystem signal files.
//!
//! Primary use case: Notify waiting sessions that an approval has been resolved,
//! enabling automatic session resume without user intervention.

use serde::{Deserialize, Serialize};
use std::path::PathBuf;


use super::gateway_store::GatewayStore;
use autonoetic_types::notification::{NotificationRecord, NotificationType};

// ... (Signal enum stays the same)

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
    /// All tasks in a workflow join group have completed.
    /// Sent to the planner session so it can resume.
    WorkflowJoinSatisfied {
        workflow_id: String,
        join_task_ids: Vec<String>,
        message: String,
        timestamp: String,
    },
}

/// No-op: the scheduler now handles notification polling.
pub fn start_signal_poller_if_needed(_agents_dir: PathBuf, _port: u16) -> anyhow::Result<()> {
    Ok(())
}

/// Signal file content with the filename for tracking.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PendingSignal {
    pub request_id: String,
    pub signal: Signal,
    pub filename: String,
}

/// Deliver a single signal via JSON-RPC event.ingest to the gateway.
pub async fn deliver_signal(pending: &PendingSignal, session_id: &str, port: u16) -> anyhow::Result<()> {
    let request_id = &pending.request_id;
    
    tracing::info!(
        target: "signal",
        request_id = %request_id,
        session_id = %session_id,
        "Delivering signal via JSON-RPC"
    );
    
    let request = build_delivery_request(pending, session_id);
    let addr = format!("127.0.0.1:{}", port);
    
    // Connect with retry (3 attempts)
    const MAX_ATTEMPTS: u32 = 3;
    for attempt in 1..=MAX_ATTEMPTS {
        match tokio::net::TcpStream::connect(&addr).await {
            Ok(stream) => {
                use tokio::io::{AsyncWriteExt, BufWriter, AsyncBufReadExt};
                let (read_half, write_half) = stream.into_split();
                let mut writer = BufWriter::new(write_half);
                let mut reader = tokio::io::BufReader::new(read_half);
                
                let encoded = serde_json::to_string(&request).unwrap_or_default();
                writer.write_all(encoded.as_bytes()).await?;
                writer.write_all(b"\n").await?;
                writer.flush().await?;
                
                let mut response_line = String::new();
                let read_result = tokio::time::timeout(
                    std::time::Duration::from_secs(2),
                    reader.read_line(&mut response_line),
                )
                .await
                .map_err(|_| anyhow::anyhow!("Timed out waiting for JSON-RPC response"))??;
                anyhow::ensure!(
                    read_result > 0,
                    "Gateway closed connection without JSON-RPC response"
                );

                let response: crate::router::JsonRpcResponse = serde_json::from_str(response_line.trim())
                    .map_err(|e| anyhow::anyhow!("Invalid JSON-RPC response: {}", e))?;
                if let Some(error) = response.error {
                    anyhow::bail!("Signal delivery failed: {}", error.message);
                }
                return Ok(());
            }
            Err(_) if attempt < MAX_ATTEMPTS => {
                tokio::time::sleep(std::time::Duration::from_secs(1 << (attempt - 1))).await;
            }
            Err(e) => return Err(e.into()),
        }
    }
    Ok(())
}

fn build_delivery_request(pending: &PendingSignal, session_id: &str) -> crate::router::JsonRpcRequest {
    let request_id = &pending.request_id;
    let signal = &pending.signal;

    let (message, target_agent_id, approval_status) = match signal {
        Signal::ApprovalResolved {
            request_id, agent_id, status, install_completed, message, ..
        } => (
            serde_json::json!({
                "type": "approval_resolved",
                "request_id": request_id,
                "agent_id": agent_id,
                "status": status,
                "install_completed": install_completed,
                "message": message,
            }).to_string(),
            Some(agent_id.clone()),
            status.clone(),
        ),
        Signal::WorkflowJoinSatisfied {
            workflow_id, join_task_ids, message, ..
        } => (
            serde_json::json!({
                "type": "workflow_join_satisfied",
                "workflow_id": workflow_id,
                "join_task_ids": join_task_ids,
                "message": message,
            }).to_string(),
            None,
            "completed".to_string(),
        ),
    };

    crate::router::JsonRpcRequest {
        jsonrpc: "2.0".to_string(),
        id: format!("signal-deliver-{}", request_id),
        method: "event.ingest".to_string(),
        params: serde_json::json!({
            "event_type": "chat",
            "target_agent_id": target_agent_id,
            "message": message,
            "session_id": session_id,
            "metadata": {
                "sender_id": "gateway-signal-poller",
                "channel_id": format!("signal-poller-{}", session_id),
                "signal_delivered": true,
                "approval_request_id": request_id,
                "approval_status": approval_status,
            }
        }),
    }
}

/// Write a signal to the GatewayStore as a notification.
pub fn write_signal(
    store: Option<&GatewayStore>,
    session_id: &str,
    request_id: &str,
    signal: &Signal,
) -> anyhow::Result<()> {
    let Some(store) = store else {
        return Ok(());
    };
    let n_type = match signal {
        Signal::ApprovalResolved { .. } => NotificationType::ApprovalResolved,
        Signal::WorkflowJoinSatisfied { .. } => NotificationType::WorkflowJoinSatisfied,
    };

    let n = NotificationRecord::new(
        request_id.to_string(),
        n_type,
        session_id.to_string(),
        serde_json::to_value(signal)?,
    );
    store.create_notification_record(&n)?;
    Ok(())
}
/// Write a WorkflowJoinSatisfied signal to a planner session's signal directory.
pub fn send_workflow_join_satisfied(
    store: Option<&GatewayStore>,
    root_session_id: &str,
    workflow_id: &str,
    join_task_ids: Vec<String>,
) -> anyhow::Result<()> {
    let signal_id = format!("wf-join-{}", &uuid::Uuid::new_v4().to_string()[..8]);
    let signal = Signal::WorkflowJoinSatisfied {
        workflow_id: workflow_id.to_string(),
        join_task_ids: join_task_ids.clone(),
        message: format!(
            "Workflow join satisfied: all {} tasks completed. You may resume planning.",
            join_task_ids.len()
        ),
        timestamp: chrono::Utc::now().to_rfc3339(),
    };
    write_signal(store, root_session_id, &signal_id, &signal)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{build_delivery_request, deliver_signal, PendingSignal, Signal};

    #[test]
    fn workflow_join_signal_omits_explicit_target_agent() {
        let pending = PendingSignal {
            request_id: "wf-join-test".to_string(),
            signal: Signal::WorkflowJoinSatisfied {
                workflow_id: "wf-123".to_string(),
                join_task_ids: vec!["task-a".to_string()],
                message: "ready".to_string(),
                timestamp: chrono::Utc::now().to_rfc3339(),
            },
            filename: "wf-join-test.json".to_string(),
        };

        let request = build_delivery_request(&pending, "demo-session");
        assert_eq!(request.method, "event.ingest");
        assert_eq!(
            request.params.get("target_agent_id"),
            Some(&serde_json::Value::Null)
        );
    }

    #[tokio::test]
    async fn deliver_signal_fails_on_jsonrpc_error_response() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("listener should bind");
        let port = listener
            .local_addr()
            .expect("listener should have addr")
            .port();

        let server = tokio::spawn(async move {
            let (socket, _) = listener.accept().await.expect("accept should succeed");
            let (read_half, mut write_half) = socket.into_split();
            let mut reader = tokio::io::BufReader::new(read_half);
            let mut line = String::new();
            let _ = tokio::io::AsyncBufReadExt::read_line(&mut reader, &mut line)
                .await
                .expect("request should read");
            let response = serde_json::json!({
                "jsonrpc": "2.0",
                "id": "signal-deliver-wf-join-test",
                "error": {
                    "code": -32000,
                    "message": "event.ingest routing failed: target_agent_id must not be empty when provided"
                }
            })
            .to_string();
            tokio::io::AsyncWriteExt::write_all(&mut write_half, response.as_bytes())
                .await
                .expect("response should write");
            tokio::io::AsyncWriteExt::write_all(&mut write_half, b"\n")
                .await
                .expect("newline should write");
        });

        let pending = PendingSignal {
            request_id: "wf-join-test".to_string(),
            signal: Signal::WorkflowJoinSatisfied {
                workflow_id: "wf-123".to_string(),
                join_task_ids: vec!["task-a".to_string()],
                message: "ready".to_string(),
                timestamp: chrono::Utc::now().to_rfc3339(),
            },
            filename: "wf-join-test.json".to_string(),
        };

        let err = deliver_signal(&pending, "demo-session", port)
            .await
            .expect_err("delivery should fail on JSON-RPC error");
        assert!(err
            .to_string()
            .contains("Signal delivery failed: event.ingest routing failed"));

        server.await.expect("server task should complete");
    }
}

