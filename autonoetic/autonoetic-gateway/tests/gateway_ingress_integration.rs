//! End-to-end integration test for live JSON-RPC ingress.

use autonoetic_gateway::router::{JsonRpcResponse, JsonRpcRouter};
use autonoetic_gateway::server::jsonrpc::start_jsonrpc_server;
use autonoetic_types::causal_chain::CausalChainEntry;
use autonoetic_types::config::GatewayConfig;
use std::io;
use std::path::Path;
use std::sync::{Arc, Mutex};
use tempfile::tempdir;
use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader};
use tokio::net::{TcpListener, TcpStream};

const LLM_BASE_URL_OVERRIDE_ENV: &str = "AUTONOETIC_LLM_BASE_URL";
const LLM_API_KEY_OVERRIDE_ENV: &str = "AUTONOETIC_LLM_API_KEY";

struct EnvGuard {
    key: &'static str,
    previous: Option<String>,
}

impl EnvGuard {
    fn set(key: &'static str, value: impl Into<String>) -> Self {
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

fn write_test_agent(agent_dir: &Path, agent_id: &str) -> anyhow::Result<()> {
    std::fs::create_dir_all(agent_dir)?;
    let skill = format!(
        "---\nversion: \"1.0\"\nruntime:\n  engine: \"autonoetic\"\n  gateway_version: \"0.1.0\"\n  sdk_version: \"0.1.0\"\n  type: \"stateful\"\n  sandbox: \"bubblewrap\"\n  runtime_lock: \"runtime.lock\"\nagent:\n  id: \"{agent_id}\"\n  name: \"{agent_id}\"\n  description: \"Ingress test agent\"\nllm_config:\n  provider: \"openai\"\n  model: \"test-model\"\n  temperature: 0.0\n---\n# Instructions\nReply with the model output.\n",
    );
    std::fs::write(agent_dir.join("SKILL.md"), skill)?;
    Ok(())
}

async fn spawn_openai_stub(
    captured_bodies: Arc<Mutex<Vec<serde_json::Value>>>,
) -> anyhow::Result<(
    std::net::SocketAddr,
    tokio::task::JoinHandle<anyhow::Result<()>>,
)> {
    let listener = TcpListener::bind("127.0.0.1:0").await?;
    let addr = listener.local_addr()?;
    let handle = tokio::spawn(async move {
        loop {
            let (mut stream, _) = listener.accept().await?;
            let captured = captured_bodies.clone();
            tokio::spawn(async move {
                if let Err(err) = handle_stub_connection(&mut stream, captured).await {
                    tracing::warn!(error = %err, "stub connection failed");
                }
            });
        }
        #[allow(unreachable_code)]
        Ok(())
    });
    Ok((addr, handle))
}

async fn handle_stub_connection(
    stream: &mut TcpStream,
    captured_bodies: Arc<Mutex<Vec<serde_json::Value>>>,
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
    let body_json: serde_json::Value = serde_json::from_slice(&body)?;
    captured_bodies.lock().unwrap().push(body_json);

    let response_body = serde_json::json!({
        "choices": [{
            "message": { "content": "stub assistant reply" },
            "finish_reason": "stop"
        }],
        "usage": {
            "prompt_tokens": 12,
            "completion_tokens": 3
        }
    });
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

fn read_jsonl_entries(path: &Path) -> anyhow::Result<Vec<CausalChainEntry>> {
    let content = std::fs::read_to_string(path)?;
    content
        .lines()
        .filter(|line| !line.trim().is_empty())
        .map(|line| serde_json::from_str(line).map_err(anyhow::Error::from))
        .collect()
}

#[tokio::test]
async fn test_event_ingest_live_jsonrpc_ingress_writes_gateway_and_agent_traces(
) -> anyhow::Result<()> {
    let temp = tempdir()?;
    let agents_dir = temp.path().join("agents");
    let target_agent_id = "agent_ingress_test";
    write_test_agent(&agents_dir.join(target_agent_id), target_agent_id)?;

    let captured_bodies = Arc::new(Mutex::new(Vec::new()));
    let (stub_addr, stub_handle) = spawn_openai_stub(captured_bodies.clone()).await?;
    let _base_url = EnvGuard::set(
        LLM_BASE_URL_OVERRIDE_ENV,
        format!("http://{}/v1/chat/completions", stub_addr),
    );
    let _api_key = EnvGuard::set(LLM_API_KEY_OVERRIDE_ENV, "test-key");

    let listener = std::net::TcpListener::bind("127.0.0.1:0")?;
    let jsonrpc_addr = listener.local_addr()?;
    drop(listener);

    let router = JsonRpcRouter::new(GatewayConfig {
        agents_dir: agents_dir.clone(),
        port: jsonrpc_addr.port(),
        ..GatewayConfig::default()
    });
    let server = tokio::spawn(async move { start_jsonrpc_server(jsonrpc_addr, router).await });

    tokio::time::sleep(std::time::Duration::from_millis(50)).await;

    let stream = TcpStream::connect(jsonrpc_addr).await?;
    let (read_half, mut write_half) = stream.into_split();
    let mut lines = BufReader::new(read_half).lines();

    let session_id = "session-e2e-ingress";
    let request = serde_json::json!({
        "jsonrpc": "2.0",
        "id": "ingress-1",
        "method": "event.ingest",
        "params": {
            "event_type": "webhook",
            "target_agent_id": target_agent_id,
            "message": "Incoming deployment event",
            "metadata": {"source": "integration-test"},
            "session_id": session_id
        }
    });
    write_half
        .write_all(format!("{}\n", request).as_bytes())
        .await?;

    let response_line = lines
        .next_line()
        .await?
        .ok_or_else(|| io::Error::new(io::ErrorKind::UnexpectedEof, "missing JSON-RPC response"))?;
    let response: JsonRpcResponse = serde_json::from_str(&response_line)?;

    assert!(
        response.error.is_none(),
        "unexpected error: {:?}",
        response.error
    );
    let result = response.result.expect("result should exist");
    assert_eq!(result["assistant_reply"], "stub assistant reply");
    assert_eq!(result["session_id"], session_id);

    let request_bodies = captured_bodies.lock().unwrap();
    assert_eq!(request_bodies.len(), 1);
    let body = &request_bodies[0];
    assert_eq!(body["model"], "test-model");
    let joined_messages = body["messages"]
        .as_array()
        .expect("messages should be an array")
        .iter()
        .filter_map(|msg| msg["content"].as_str())
        .collect::<Vec<_>>()
        .join("\n");
    assert!(joined_messages.contains("Gateway event type: webhook"));
    assert!(joined_messages.contains("Incoming deployment event"));
    drop(request_bodies);

    let gateway_entries = read_jsonl_entries(
        &agents_dir
            .join(".gateway")
            .join("history")
            .join("causal_chain.jsonl"),
    )?;
    assert!(gateway_entries.iter().any(|entry| {
        entry.session_id == session_id && entry.action == "event.ingest.requested"
    }));
    assert!(gateway_entries.iter().any(|entry| {
        entry.session_id == session_id && entry.action == "event.ingest.completed"
    }));

    let agent_entries = read_jsonl_entries(
        &agents_dir
            .join(target_agent_id)
            .join("history")
            .join("causal_chain.jsonl"),
    )?;
    assert!(agent_entries.iter().any(|entry| {
        entry.session_id == session_id && entry.category == "session" && entry.action == "start"
    }));
    assert!(agent_entries.iter().any(|entry| {
        entry.session_id == session_id && entry.category == "session" && entry.action == "end"
    }));

    server.abort();
    stub_handle.abort();
    Ok(())
}
