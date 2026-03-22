//! JSON-RPC TCP listener for local event ingress.

use crate::router::{JsonRpcRequest, JsonRpcResponse, JsonRpcRouter};
use std::net::SocketAddr;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::{TcpListener, TcpStream};

/// Start a line-delimited JSON-RPC server over TCP.
pub async fn start_jsonrpc_server(
    listen_addr: SocketAddr,
    router: JsonRpcRouter,
) -> anyhow::Result<()> {
    let listener = TcpListener::bind(listen_addr).await?;
    serve_jsonrpc_listener(listener, router).await
}

pub(crate) async fn serve_jsonrpc_listener(
    listener: TcpListener,
    router: JsonRpcRouter,
) -> anyhow::Result<()> {
    tracing::info!("JSON-RPC server listening on {}", listener.local_addr()?);

    loop {
        let (stream, peer_addr) = listener.accept().await?;
        let router = router.clone();
        tokio::spawn(async move {
            if let Err(e) = handle_connection(stream, router).await {
                tracing::warn!(peer = %peer_addr, error = %e, "JSON-RPC client disconnected");
            }
        });
    }
}

async fn handle_connection(stream: TcpStream, router: JsonRpcRouter) -> anyhow::Result<()> {
    let (read_half, mut write_half) = stream.into_split();
    let mut lines = BufReader::new(read_half).lines();

    while let Some(line) = lines.next_line().await? {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }

        let response = match serde_json::from_str::<JsonRpcRequest>(trimmed) {
            Ok(req) => router.dispatch(req).await,
            Err(e) => {
                JsonRpcResponse::error("null".to_string(), -32700, format!("Parse error: {}", e))
            }
        };

        let encoded = serde_json::to_string(&response)?;
        write_half.write_all(encoded.as_bytes()).await?;
        write_half.write_all(b"\n").await?;
        write_half.flush().await?;
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use autonoetic_types::config::GatewayConfig;
    use tempfile::TempDir;
    use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

    fn test_router() -> (TempDir, JsonRpcRouter) {
        let temp = tempfile::tempdir().expect("tempdir should create");
        let router = JsonRpcRouter::new(GatewayConfig {
            agents_dir: temp.path().join("agents"),
            ..GatewayConfig::default()
        }, None);
        (temp, router)
    }

    #[tokio::test]
    async fn test_jsonrpc_tcp_ping_roundtrip() {
        let (_temp, router) = test_router();
        let listener = TcpListener::bind("127.0.0.1:0")
            .await
            .expect("listener should bind");
        let addr = listener
            .local_addr()
            .expect("listener should expose local addr");
        let server = tokio::spawn(async move {
            serve_jsonrpc_listener(listener, router)
                .await
                .expect("server should run");
        });

        let stream = TcpStream::connect(addr)
            .await
            .expect("client should connect");
        let (read_half, mut write_half) = stream.into_split();
        let mut lines = BufReader::new(read_half).lines();
        let request = serde_json::json!({
            "jsonrpc": "2.0",
            "id": "1",
            "method": "ping",
            "params": {}
        });

        write_half
            .write_all(format!("{}\n", request).as_bytes())
            .await
            .expect("request should write");

        let line = lines
            .next_line()
            .await
            .expect("response should read")
            .expect("response line should exist");
        let response: JsonRpcResponse =
            serde_json::from_str(&line).expect("response should decode");

        assert_eq!(response.result, Some(serde_json::json!("pong")));
        assert!(response.error.is_none());

        server.abort();
    }

    #[tokio::test]
    async fn test_jsonrpc_tcp_agent_spawn_unknown_agent() {
        let (_temp, router) = test_router();
        let listener = TcpListener::bind("127.0.0.1:0")
            .await
            .expect("listener should bind");
        let addr = listener
            .local_addr()
            .expect("listener should expose local addr");
        let server = tokio::spawn(async move {
            serve_jsonrpc_listener(listener, router)
                .await
                .expect("server should run");
        });

        let stream = TcpStream::connect(addr)
            .await
            .expect("client should connect");
        let (read_half, mut write_half) = stream.into_split();
        let mut lines = BufReader::new(read_half).lines();
        let request = serde_json::json!({
            "jsonrpc": "2.0",
            "id": "2",
            "method": "agent.spawn",
            "params": {
                "agent_id": "missing",
                "message": "hello"
            }
        });

        write_half
            .write_all(format!("{}\n", request).as_bytes())
            .await
            .expect("request should write");

        let line = lines
            .next_line()
            .await
            .expect("response should read")
            .expect("response line should exist");
        let response: JsonRpcResponse =
            serde_json::from_str(&line).expect("response should decode");

        assert!(response.result.is_none());
        assert!(response
            .error
            .as_ref()
            .expect("error should exist")
            .message
            .contains("not found"));

        server.abort();
    }
}
