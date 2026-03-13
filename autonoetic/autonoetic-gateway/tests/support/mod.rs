#![allow(dead_code)]

pub mod agents;

use autonoetic_gateway::router::{JsonRpcRequest, JsonRpcResponse, JsonRpcRouter};
use autonoetic_gateway::scheduler::{approve_request, load_approval_requests, run_scheduler_tick};
use autonoetic_gateway::server::jsonrpc::start_jsonrpc_server;
use autonoetic_gateway::GatewayExecutionService;
use autonoetic_types::background::{ApprovalDecision, ApprovalRequest};
use autonoetic_types::causal_chain::CausalChainEntry;
use autonoetic_types::config::GatewayConfig;
use serde::de::DeserializeOwned;
use std::future::Future;
use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::pin::Pin;
use std::sync::{Arc, Mutex};
use tempfile::TempDir;
use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader};
use tokio::net::{TcpListener, TcpStream};

pub struct EnvGuard {
    key: &'static str,
    previous: Option<String>,
}

impl EnvGuard {
    pub fn set(key: &'static str, value: impl Into<String>) -> Self {
        let previous = std::env::var(key).ok();
        std::env::set_var(key, value.into());
        Self { key, previous }
    }
}

impl Drop for EnvGuard {
    fn drop(&mut self) {
        if let Some(previous) = self.previous.take() {
            std::env::set_var(self.key, previous);
        } else {
            std::env::remove_var(self.key);
        }
    }
}

pub struct TestWorkspace {
    tempdir: TempDir,
    pub agents_dir: PathBuf,
}

impl TestWorkspace {
    pub fn new() -> anyhow::Result<Self> {
        let tempdir = tempfile::tempdir()?;
        let agents_dir = tempdir.path().join("agents");
        std::fs::create_dir_all(&agents_dir)?;
        Ok(Self {
            tempdir,
            agents_dir,
        })
    }

    pub fn path(&self) -> &Path {
        self.tempdir.path()
    }

    pub fn gateway_config(&self) -> GatewayConfig {
        GatewayConfig {
            agents_dir: self.agents_dir.clone(),
            ..GatewayConfig::default()
        }
    }
}

type StubResponder = Arc<
    dyn Fn(String, serde_json::Value) -> Pin<Box<dyn Future<Output = serde_json::Value> + Send>>
        + Send
        + Sync,
>;

pub struct OpenAiStub {
    addr: SocketAddr,
    captured_bodies: Arc<Mutex<Vec<serde_json::Value>>>,
    handle: tokio::task::JoinHandle<anyhow::Result<()>>,
}

impl OpenAiStub {
    pub async fn spawn<F, Fut>(responder: F) -> anyhow::Result<Self>
    where
        F: Fn(String, serde_json::Value) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = serde_json::Value> + Send + 'static,
    {
        let responder: StubResponder =
            Arc::new(move |raw_body, body_json| Box::pin(responder(raw_body, body_json)));
        let captured_bodies = Arc::new(Mutex::new(Vec::new()));
        let listener = TcpListener::bind("127.0.0.1:0").await?;
        let addr = listener.local_addr()?;
        let captured = Arc::clone(&captured_bodies);
        let handle = tokio::spawn(async move {
            loop {
                let (mut stream, _) = listener.accept().await?;
                let responder = Arc::clone(&responder);
                let captured = Arc::clone(&captured);
                tokio::spawn(async move {
                    if let Err(err) = handle_stub_connection(&mut stream, captured, responder).await
                    {
                        tracing::warn!(error = %err, "stub connection failed");
                    }
                });
            }
            #[allow(unreachable_code)]
            Ok(())
        });
        Ok(Self {
            addr,
            captured_bodies,
            handle,
        })
    }

    pub fn completion_url(&self) -> String {
        format!("http://{}/v1/chat/completions", self.addr)
    }

    pub fn captured_bodies(&self) -> Vec<serde_json::Value> {
        self.captured_bodies.lock().unwrap().clone()
    }
}

impl Drop for OpenAiStub {
    fn drop(&mut self) {
        self.handle.abort();
    }
}

async fn handle_stub_connection(
    stream: &mut TcpStream,
    captured_bodies: Arc<Mutex<Vec<serde_json::Value>>>,
    responder: StubResponder,
) -> anyhow::Result<()> {
    let mut header_buf = Vec::new();
    let mut byte = [0_u8; 1];
    loop {
        stream.read_exact(&mut byte).await?;
        header_buf.push(byte[0]);
        if header_buf.ends_with(b"\r\n\r\n") {
            break;
        }
    }

    let headers = String::from_utf8(header_buf)?;
    let content_length = headers
        .lines()
        .find_map(|line| {
            let (name, value) = line.split_once(':')?;
            if name.eq_ignore_ascii_case("content-length") {
                value.trim().parse::<usize>().ok()
            } else {
                None
            }
        })
        .ok_or_else(|| anyhow::anyhow!("missing Content-Length header"))?;

    let mut body = vec![0_u8; content_length];
    stream.read_exact(&mut body).await?;
    let raw_body = String::from_utf8(body.clone())?;
    let body_json: serde_json::Value = serde_json::from_slice(&body)?;
    captured_bodies.lock().unwrap().push(body_json.clone());

    let response_body = responder(raw_body, body_json).await;
    let encoded = serde_json::to_vec(&response_body)?;
    let response = format!(
        "HTTP/1.1 200 OK\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n",
        encoded.len()
    );
    stream.write_all(response.as_bytes()).await?;
    stream.write_all(&encoded).await?;
    stream.flush().await?;
    Ok(())
}

pub async fn spawn_gateway_server(
    mut config: GatewayConfig,
) -> anyhow::Result<(SocketAddr, tokio::task::JoinHandle<anyhow::Result<()>>)> {
    let listener = std::net::TcpListener::bind("127.0.0.1:0")?;
    let addr = listener.local_addr()?;
    drop(listener);
    config.port = addr.port();
    let router = JsonRpcRouter::new(config);
    let handle = tokio::spawn(async move { start_jsonrpc_server(addr, router).await });
    tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    Ok((addr, handle))
}

pub struct JsonRpcClient {
    lines: tokio::io::Lines<BufReader<tokio::net::tcp::OwnedReadHalf>>,
    write_half: tokio::net::tcp::OwnedWriteHalf,
}

impl JsonRpcClient {
    pub async fn connect(addr: SocketAddr) -> anyhow::Result<Self> {
        let stream = TcpStream::connect(addr).await?;
        let (read_half, write_half) = stream.into_split();
        Ok(Self {
            lines: BufReader::new(read_half).lines(),
            write_half,
        })
    }

    pub async fn send(&mut self, request: JsonRpcRequest) -> anyhow::Result<()> {
        let msg = serde_json::to_string(&request)?;
        self.write_half.write_all(msg.as_bytes()).await?;
        self.write_half.write_all(b"\n").await?;
        self.write_half.flush().await?;
        Ok(())
    }

    pub async fn recv(&mut self) -> anyhow::Result<JsonRpcResponse> {
        let line = self.lines.next_line().await?.ok_or_else(|| {
            std::io::Error::new(
                std::io::ErrorKind::UnexpectedEof,
                "missing JSON-RPC response",
            )
        })?;
        Ok(serde_json::from_str(&line)?)
    }

    pub async fn event_ingest(
        &mut self,
        id: impl Into<String>,
        target_agent_id: &str,
        session_id: &str,
        event_type: &str,
        message: &str,
        metadata: Option<serde_json::Value>,
    ) -> anyhow::Result<JsonRpcResponse> {
        let mut params = serde_json::json!({
            "target_agent_id": target_agent_id,
            "session_id": session_id,
            "event_type": event_type,
            "message": message,
        });
        if let Some(metadata) = metadata {
            params["metadata"] = metadata;
        }
        self.send(JsonRpcRequest {
            jsonrpc: "2.0".to_string(),
            id: id.into(),
            method: "event.ingest".to_string(),
            params,
        })
        .await?;
        self.recv().await
    }

    pub async fn agent_spawn(
        &mut self,
        id: impl Into<String>,
        target_agent_id: &str,
        message: &str,
        metadata: Option<serde_json::Value>,
        session_id: Option<&str>,
    ) -> anyhow::Result<JsonRpcResponse> {
        let mut params = serde_json::json!({
            "agent_id": target_agent_id,
            "message": message,
        });
        if let Some(metadata) = metadata {
            params["metadata"] = metadata;
        }
        if let Some(session_id) = session_id {
            params["session_id"] = serde_json::json!(session_id);
        }
        self.send(JsonRpcRequest {
            jsonrpc: "2.0".to_string(),
            id: id.into(),
            method: "agent.spawn".to_string(),
            params,
        })
        .await?;
        self.recv().await
    }
}

pub fn read_jsonl_entries<T: DeserializeOwned>(path: &Path) -> anyhow::Result<Vec<T>> {
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

pub fn read_causal_entries(path: &Path) -> anyhow::Result<Vec<CausalChainEntry>> {
    read_jsonl_entries(path)
}

pub async fn require_single_pending_approval(
    execution: Arc<GatewayExecutionService>,
    config: &GatewayConfig,
) -> anyhow::Result<ApprovalRequest> {
    for _ in 0..5 {
        run_scheduler_tick(execution.clone()).await?;
        let approvals = load_approval_requests(config)?;
        if approvals.len() == 1 {
            return Ok(approvals.into_iter().next().expect("approval should exist"));
        }
        tokio::time::sleep(std::time::Duration::from_millis(500)).await;
    }
    let approvals = load_approval_requests(config)?;
    anyhow::ensure!(
        approvals.len() == 1,
        "expected exactly 1 pending approval request, found {}",
        approvals.len()
    );
    Ok(approvals.into_iter().next().expect("approval should exist"))
}

pub async fn approve_pending_request_and_tick(
    execution: Arc<GatewayExecutionService>,
    config: &GatewayConfig,
    request: &ApprovalRequest,
    decided_by: &str,
    reason: Option<String>,
) -> anyhow::Result<ApprovalDecision> {
    let decision = approve_request(config, &request.request_id, decided_by, reason)?;
    run_scheduler_tick(execution).await?;
    Ok(decision)
}
