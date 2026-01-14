//! MCP Stdio Client
//!
//! Handles communication with MCP servers running as local processes via stdio.
//! Implements JSON-RPC 2.0 over line-delimited stdio.

use crate::{ccos_eprintln, ccos_println};
use rtfs::runtime::error::{RuntimeError, RuntimeResult};
use serde_json::{json, Value};
use std::collections::HashMap;
use std::process::Stdio;
use std::sync::Arc;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::process::Command;
use tokio::sync::{mpsc, oneshot, RwLock};

/// A client for communicating with an MCP server via stdio
pub struct StdioClient {
    #[allow(dead_code)]
    command: String,
    #[allow(dead_code)]
    args: Vec<String>,
    pending_requests: Arc<RwLock<HashMap<String, oneshot::Sender<Value>>>>,
    tx: mpsc::Sender<String>,
}

impl std::fmt::Debug for StdioClient {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("StdioClient")
            .field("command", &self.command)
            .field("args", &self.args)
            .finish()
    }
}

impl StdioClient {
    /// Spawn a new MCP server process and establish stdio communication
    pub async fn spawn(command_line: &str) -> RuntimeResult<Self> {
        let parts: Vec<&str> = command_line.split_whitespace().collect();
        if parts.is_empty() {
            return Err(RuntimeError::Generic("Empty command line".to_string()));
        }

        let cmd = parts[0];
        let args: Vec<String> = parts[1..].iter().map(|s| s.to_string()).collect();

        let mut child = Command::new(cmd)
            .args(&args)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|e| {
                RuntimeError::Generic(format!("Failed to spawn process {}: {}", cmd, e))
            })?;

        let mut stdin = child.stdin.take().expect("Failed to open stdin");
        let mut stdout = BufReader::new(child.stdout.take().expect("Failed to open stdout"));
        let mut stderr = BufReader::new(child.stderr.take().expect("Failed to open stderr"));

        let (tx, mut rx) = mpsc::channel::<String>(64);
        let pending_requests: Arc<RwLock<HashMap<String, oneshot::Sender<Value>>>> =
            Arc::new(RwLock::new(HashMap::new()));
        let pending_clone = Arc::clone(&pending_requests);

        // Stdout reader task: parses JSON-RPC responses and matches them to pending requests
        tokio::spawn(async move {
            let mut line = String::new();
            loop {
                line.clear();
                match stdout.read_line(&mut line).await {
                    Ok(0) => {
                        log::info!("MCP stdout EOF reached");
                        break;
                    }
                    Ok(_) => {
                        if let Ok(v) = serde_json::from_str::<Value>(&line) {
                            if let Some(id_val) = v.get("id") {
                                let id_str = match id_val {
                                    Value::String(s) => s.clone(),
                                    Value::Number(n) => n.to_string(),
                                    _ => continue,
                                };

                                let mut pending = pending_clone.write().await;
                                if let Some(sender) = pending.remove(&id_str) {
                                    let _ = sender.send(v);
                                }
                            }
                        }
                    }
                    Err(e) => {
                        log::error!("Error reading MCP stdout: {}", e);
                        break;
                    }
                }
            }
        });

        // Stderr reader task: logs server errors to CCOS output
        tokio::spawn(async move {
            let mut line = String::new();
            loop {
                line.clear();
                match stderr.read_line(&mut line).await {
                    Ok(0) => break,
                    Ok(_) => {
                        ccos_eprintln!("[MCP-SERVER-LOG] {}", line.trim());
                    }
                    Err(_) => break,
                }
            }
        });

        // Stdin writer task: serializes and sends requests to the server
        tokio::spawn(async move {
            while let Some(msg) = rx.recv().await {
                if let Err(e) = stdin.write_all(msg.as_bytes()).await {
                    log::error!("Failed to write to MCP stdin: {}", e);
                    break;
                }
                if let Err(e) = stdin.write_all(b"\n").await {
                    log::error!("Failed to write newline to MCP stdin: {}", e);
                    break;
                }
                if let Err(e) = stdin.flush().await {
                    log::error!("Failed to flush MCP stdin: {}", e);
                    break;
                }
            }
        });

        Ok(Self {
            command: cmd.to_string(),
            args,
            pending_requests,
            tx,
        })
    }

    /// Make a JSON-RPC request to the MCP server
    pub async fn make_request(&self, method: &str, params: Value) -> RuntimeResult<Value> {
        let id = uuid::Uuid::new_v4().to_string();
        let request = json!({
            "jsonrpc": "2.0",
            "id": id.clone(),
            "method": method,
            "params": params
        });

        let (resp_tx, resp_rx) = oneshot::channel();
        {
            let mut pending = self.pending_requests.write().await;
            pending.insert(id, resp_tx);
        }

        let msg = serde_json::to_string(&request).map_err(|e| {
            RuntimeError::Generic(format!("Failed to serialize MCP request: {}", e))
        })?;

        self.tx.send(msg).await.map_err(|_| {
            RuntimeError::Generic("Failed to send message to MCP stdin writer task".to_string())
        })?;

        match tokio::time::timeout(std::time::Duration::from_secs(30), resp_rx).await {
            Ok(Ok(resp)) => {
                if let Some(error) = resp.get("error") {
                    return Err(RuntimeError::Generic(format!(
                        "MCP server error: {}",
                        error
                            .get("message")
                            .and_then(|m| m.as_str())
                            .unwrap_or("Unknown error")
                    )));
                }
                Ok(resp)
            }
            Ok(Err(_)) => Err(RuntimeError::Generic(
                "MCP response channel closed".to_string(),
            )),
            Err(_) => Err(RuntimeError::Generic(format!(
                "MCP request '{}' timed out",
                method
            ))),
        }
    }
}
