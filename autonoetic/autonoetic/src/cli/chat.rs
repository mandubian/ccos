use std::path::Path;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::TcpStream;

use super::common::{
    default_terminal_channel_id, default_terminal_sender_id, terminal_channel_envelope,
};
use autonoetic_gateway::router::{
    JsonRpcRequest as GatewayJsonRpcRequest, JsonRpcResponse as GatewayJsonRpcResponse,
};

pub async fn handle_chat(config_path: &Path, args: &super::common::ChatArgs) -> anyhow::Result<()> {
    let config = autonoetic_gateway::config::load_config(config_path)?;
    let target_hint = args.agent_id.as_deref().unwrap_or("default-lead");
    let session_id = args
        .session_id
        .clone()
        .unwrap_or_else(|| format!("terminal-session::{}", uuid::Uuid::new_v4()));
    let sender_id = args
        .sender_id
        .clone()
        .unwrap_or_else(default_terminal_sender_id);
    let channel_id = args
        .channel_id
        .clone()
        .unwrap_or_else(|| default_terminal_channel_id(&sender_id, target_hint));
    let gateway_addr = format!("127.0.0.1:{}", config.port);
    let stream = TcpStream::connect(&gateway_addr).await.map_err(|e| {
        anyhow::anyhow!(
            "Failed to connect to gateway JSON-RPC at {}: {}",
            gateway_addr,
            e
        )
    })?;
    let (read_half, mut write_half) = stream.into_split();
    let mut gateway_lines = BufReader::new(read_half).lines();
    let mut stdin_lines = BufReader::new(tokio::io::stdin()).lines();
    let mut stdout = tokio::io::stdout();
    let envelope = terminal_channel_envelope(&channel_id, &sender_id, &session_id);
    let mut request_counter = 0_u64;

    if !args.test_mode {
        stdout
            .write_all(
                format!(
                    "Gateway terminal chat enabled via {} (target: {}). Type /exit to quit.\n",
                    gateway_addr, target_hint
                )
                .as_bytes(),
            )
            .await?;
        stdout.flush().await?;
    }

    loop {
        if !args.test_mode {
            stdout.write_all(b"> ").await?;
            stdout.flush().await?;
        }

        let Some(line) = stdin_lines.next_line().await? else {
            break;
        };
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        if trimmed == "/exit" || trimmed == "/quit" {
            break;
        }

        request_counter += 1;
        let mut params = serde_json::json!({
            "event_type": "chat",
            "message": trimmed,
            "session_id": &session_id,
            "metadata": envelope.clone(),
        });
        if let Some(agent_id) = args.agent_id.as_ref() {
            params["target_agent_id"] = serde_json::Value::String(agent_id.clone());
        }
        let request = GatewayJsonRpcRequest {
            jsonrpc: "2.0".to_string(),
            id: format!("terminal-chat-{}", request_counter),
            method: "event.ingest".to_string(),
            params,
        };
        let encoded = serde_json::to_string(&request)?;
        write_half.write_all(encoded.as_bytes()).await?;
        write_half.write_all(b"\n").await?;
        write_half.flush().await?;

        let response_line = gateway_lines.next_line().await?.ok_or_else(|| {
            anyhow::anyhow!("Gateway JSON-RPC connection closed before a response was received")
        })?;
        let response: GatewayJsonRpcResponse = serde_json::from_str(&response_line)?;
        if let Some(error) = response.error {
            anyhow::bail!(
                "Gateway chat request failed (code {}): {}",
                error.code,
                error.message
            );
        }

        let reply = response
            .result
            .and_then(|value| {
                value
                    .get("assistant_reply")
                    .and_then(|reply| reply.as_str().map(ToOwned::to_owned))
            })
            .unwrap_or_else(|| "[No assistant text returned]".to_string());
        stdout.write_all(reply.as_bytes()).await?;
        stdout.write_all(b"\n").await?;
        stdout.flush().await?;
    }

    Ok(())
}
