use std::path::Path;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::TcpStream;

use super::common::{
    default_terminal_channel_id, default_terminal_sender_id, terminal_channel_envelope,
};
use autonoetic_gateway::router::{
    JsonRpcRequest as GatewayJsonRpcRequest, JsonRpcResponse as GatewayJsonRpcResponse,
};

/// Regex-like pattern matching for UUID-style request IDs in approval messages.
/// Extracts request_id from patterns like:
/// - "request_id: c19a8a50-d6c8-4c5f-aa3c-6ba119751b11"
/// - "Request ID: c19a8a50-d6c8-4c5f-aa3c-6ba119751b11"
fn extract_approval_request_id(text: &str) -> Option<String> {
    // UUID pattern: 8-4-4-4-12 hex digits
    let lower = text.to_lowercase();

    // Check for approval-related keywords
    if !lower.contains("approval") && !lower.contains("approve") {
        return None;
    }

    // Try to find UUID pattern after common prefixes
    let prefixes = ["request_id:", "request id:", "request_id :", "request id :"];
    for prefix in &prefixes {
        if let Some(start) = lower.find(prefix) {
            let after_prefix = &text[start + prefix.len()..].trim();
            if let Some(uuid) = extract_uuid(after_prefix) {
                return Some(uuid);
            }
        }
    }

    // Try to find any UUID in the text if approval keywords present
    extract_uuid(text)
}

fn extract_uuid(text: &str) -> Option<String> {
    // UUID pattern: 8-4-4-4-12 hex digits
    let chars: Vec<char> = text.chars().collect();
    let mut i = 0;

    while i < chars.len() {
        // Look for start of UUID (8 hex chars)
        if i + 8 <= chars.len() && is_hex_string(&chars[i..i + 8]) {
            let mut pos = i + 8;

            // Check for dashes and remaining segments
            let segments = [4, 4, 12];
            let mut valid = true;

            for &seg_len in &segments {
                if pos + 1 + seg_len > chars.len() {
                    valid = false;
                    break;
                }
                if chars[pos] != '-' {
                    valid = false;
                    break;
                }
                pos += 1;
                if !is_hex_string(&chars[pos..pos + seg_len]) {
                    valid = false;
                    break;
                }
                pos += seg_len;
            }

            if valid {
                return Some(chars[i..pos].iter().collect());
            }
        }
        i += 1;
    }
    None
}

fn is_hex_string(chars: &[char]) -> bool {
    chars.iter().all(|c| c.is_ascii_hexdigit())
}

/// Formats and displays an approval request notification.
fn display_approval_notification(reply: &str) {
    // Check for approval_resolved message (from gateway auto-resume)
    if reply.contains("approval_resolved") {
        if let Ok(json) = serde_json::from_str::<serde_json::Value>(reply) {
            if json.get("type").and_then(|t| t.as_str()) == Some("approval_resolved") {
                let status = json.get("status").and_then(|s| s.as_str()).unwrap_or("unknown");
                let request_id = json.get("request_id").and_then(|r| r.as_str()).unwrap_or("?");
                eprintln!();
                eprintln!("┌─────────────────────────────────────────────────────────────────┐");
                if status == "approved" {
                    eprintln!("│ ✅ APPROVAL GRANTED                                             │");
                } else {
                    eprintln!("│ ❌ APPROVAL REJECTED                                            │");
                }
                eprintln!("├─────────────────────────────────────────────────────────────────┤");
                eprintln!("│ Request ID:  {:<50} │", request_id);
                if status == "approved" {
                    eprintln!("│ The agent can now retry the install with the approval ref.     │");
                }
                eprintln!("└─────────────────────────────────────────────────────────────────┘");
                eprintln!();
                return;
            }
        }
    }
    
    // Check for approval_required (pending approval)
    if let Some(request_id) = extract_approval_request_id(reply) {
        eprintln!();
        eprintln!("┌─────────────────────────────────────────────────────────────────┐");
        eprintln!("│ ⚠️  INSTALL REQUIRES YOUR APPROVAL                              │");
        eprintln!("├─────────────────────────────────────────────────────────────────┤");
        eprintln!("│ Request ID:  {:<50} │", request_id);
        eprintln!("├─────────────────────────────────────────────────────────────────┤");
        eprintln!("│ To approve, run in another terminal:                            │");
        eprintln!("│   autonoetic gateway approvals approve {}   │", request_id);
        eprintln!("│                                                                 │");
        eprintln!("│ The gateway will automatically resume the session after.        │");
        eprintln!("└─────────────────────────────────────────────────────────────────┘");
        eprintln!();
    }
}

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

        // Check for approval required in the reply and display notification
        display_approval_notification(&reply);

        stdout.write_all(reply.as_bytes()).await?;
        stdout.write_all(b"\n").await?;
        stdout.flush().await?;
    }

    Ok(())
}
