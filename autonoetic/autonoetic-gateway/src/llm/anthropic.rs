//! Anthropic Messages API driver.
//!
//! Uses the unique Anthropic format: system at top level, tool_use/tool_result
//! content blocks, and Anthropic-specific SSE events.
//! All credential/endpoint resolution is done by `provider::resolve()`.

use super::{
    CompletionRequest, CompletionResponse, LlmDriver, Role, StopReason, StreamEvent, TokenUsage,
    ToolCall,
};
use crate::llm::provider::{AuthStrategy, ResolvedProvider};
use reqwest::Client;
use serde_json::json;

pub struct AnthropicDriver {
    client: Client,
    provider: ResolvedProvider,
}

impl AnthropicDriver {
    pub fn new(client: Client, provider: ResolvedProvider) -> Self {
        Self { client, provider }
    }

    fn apply_auth(&self, builder: reqwest::RequestBuilder) -> reqwest::RequestBuilder {
        match &self.provider.auth {
            AuthStrategy::XApiKey(key) => builder
                .header("x-api-key", key)
                .header("anthropic-version", "2023-06-01"),
            _ => builder,
        }
    }

    fn build_body(&self, req: &CompletionRequest, stream: bool) -> serde_json::Value {
        let mut system_text = String::new();
        let mut messages: Vec<serde_json::Value> = Vec::new();

        for m in &req.messages {
            match m.role {
                Role::System => {
                    system_text.push_str(&m.content);
                }
                Role::User => {
                    if let Some(ref tool_call_id) = m.tool_call_id {
                        messages.push(json!({
                            "role": "user",
                            "content": [{ "type": "tool_result", "tool_use_id": tool_call_id, "content": m.content }]
                        }));
                    } else {
                        messages.push(json!({ "role": "user", "content": m.content }));
                    }
                }
                Role::Assistant => {
                    if !m.tool_calls.is_empty() {
                        let mut content: Vec<serde_json::Value> = Vec::new();
                        if !m.content.is_empty() {
                            content.push(json!({ "type": "text", "text": m.content }));
                        }
                        for tc in &m.tool_calls {
                            let input: serde_json::Value =
                                serde_json::from_str(&tc.arguments).unwrap_or(json!({}));
                            content.push(json!({ "type": "tool_use", "id": tc.id, "name": tc.name, "input": input }));
                        }
                        messages.push(json!({ "role": "assistant", "content": content }));
                    } else {
                        messages.push(json!({ "role": "assistant", "content": m.content }));
                    }
                }
                Role::Tool => {} // handled as tool_result inside User turn above
            }
        }

        let mut body = json!({
            "model": self.provider.model,
            "max_tokens": req.max_tokens.or(self.provider.max_tokens).unwrap_or(4096),
            "messages": messages,
            "stream": stream,
        });

        if !system_text.is_empty() {
            body["system"] = json!(system_text.trim());
        }

        let t = req.temperature.or(self.provider.temperature);
        if let Some(t) = t {
            if t > 0.0 {
                body["temperature"] = json!(t);
            }
        }

        if !req.tools.is_empty() {
            body["tools"] = json!(req
                .tools
                .iter()
                .map(|t| json!({
                    "name": t.name,
                    "description": t.description,
                    "input_schema": t.input_schema,
                }))
                .collect::<Vec<_>>());
        }

        body
    }
}

#[async_trait::async_trait]
impl LlmDriver for AnthropicDriver {
    async fn complete(&self, req: &CompletionRequest) -> anyhow::Result<CompletionResponse> {
        let body = self.build_body(req, false);

        const MAX_RETRIES: u32 = 3;
        for attempt in 0..=MAX_RETRIES {
            let builder = self.apply_auth(
                self.client
                    .post(&self.provider.base_url)
                    .header("Content-Type", "application/json")
                    .json(&body),
            );
            let response = builder.send().await?;
            let status = response.status().as_u16();

            if status == 429 || status == 529 {
                if attempt < MAX_RETRIES {
                    let wait_ms = (attempt + 1) as u64 * 2000;
                    tracing::warn!(status, attempt, wait_ms, "Anthropic rate limited, retrying");
                    tokio::time::sleep(std::time::Duration::from_millis(wait_ms)).await;
                    continue;
                }
                anyhow::bail!("Anthropic rate limited after {} retries", MAX_RETRIES);
            }

            if !reqwest::StatusCode::from_u16(status)
                .map(|s| s.is_success())
                .unwrap_or(false)
            {
                let text = response.text().await.unwrap_or_default();
                anyhow::bail!("Anthropic API error {}: {}", status, text);
            }

            let j: serde_json::Value = response.json().await?;
            return Ok(parse_response(&j));
        }
        anyhow::bail!("Max retries exceeded");
    }

    async fn stream(
        &self,
        req: &CompletionRequest,
        tx: tokio::sync::mpsc::Sender<StreamEvent>,
    ) -> anyhow::Result<CompletionResponse> {
        use futures::StreamExt;

        let body = self.build_body(req, true);
        const MAX_RETRIES: u32 = 3;
        for attempt in 0..=MAX_RETRIES {
            let builder = self.apply_auth(
                self.client
                    .post(&self.provider.base_url)
                    .header("Content-Type", "application/json")
                    .json(&body),
            );
            let response = builder.send().await?;
            let status = response.status().as_u16();

            if status == 429 || status == 529 {
                if attempt < MAX_RETRIES {
                    let wait_ms = (attempt + 1) as u64 * 2000;
                    tracing::warn!(
                        status,
                        attempt,
                        wait_ms,
                        "Anthropic stream rate limited, retrying"
                    );
                    tokio::time::sleep(std::time::Duration::from_millis(wait_ms)).await;
                    continue;
                }
                anyhow::bail!("Anthropic rate limited after {} retries", MAX_RETRIES);
            }

            if !reqwest::StatusCode::from_u16(status)
                .map(|s| s.is_success())
                .unwrap_or(false)
            {
                let text = response.text().await.unwrap_or_default();
                anyhow::bail!("Anthropic stream error {}: {}", status, text);
            }

            let mut buffer = String::new();
            let mut text_accum = String::new();
            let mut tool_calls: Vec<ToolCall> = Vec::new();
            let mut current_tool: Option<(String, String, String)> = None;
            let mut stop_reason = StopReason::EndTurn;
            let mut usage = TokenUsage::default();

            let mut byte_stream = response.bytes_stream();
            while let Some(chunk) = byte_stream.next().await {
                buffer.push_str(&String::from_utf8_lossy(&chunk?));

                while let Some(pos) = buffer.find("\n\n") {
                    let event_text = buffer[..pos].to_string();
                    buffer = buffer[pos + 2..].to_string();

                    let mut event_type = String::new();
                    let mut data = String::new();
                    for line in event_text.lines() {
                        if let Some(et) = line.strip_prefix("event: ") {
                            event_type = et.to_string();
                        } else if let Some(d) = line.strip_prefix("data: ") {
                            data = d.to_string();
                        }
                    }
                    if data.is_empty() {
                        continue;
                    }
                    let Ok(j) = serde_json::from_str::<serde_json::Value>(&data) else {
                        continue;
                    };

                    match event_type.as_str() {
                        "message_start" => {
                            if self.provider.capabilities.supports_usage_in_stream {
                                usage.input_tokens =
                                    j["message"]["usage"]["input_tokens"].as_u64().unwrap_or(0);
                            }
                        }
                        "content_block_start" => {
                            let block = &j["content_block"];
                            if block["type"].as_str() == Some("tool_use") {
                                let id = block["id"].as_str().unwrap_or("").to_string();
                                let name = block["name"].as_str().unwrap_or("").to_string();
                                let _ = tx
                                    .send(StreamEvent::ToolUseStart {
                                        id: id.clone(),
                                        name: name.clone(),
                                    })
                                    .await;
                                current_tool = Some((id, name, String::new()));
                            }
                        }
                        "content_block_delta" => {
                            let delta = &j["delta"];
                            match delta["type"].as_str().unwrap_or("") {
                                "text_delta" => {
                                    if let Some(text) = delta["text"].as_str() {
                                        text_accum.push_str(text);
                                        let _ =
                                            tx.send(StreamEvent::TextDelta(text.to_string())).await;
                                    }
                                }
                                "input_json_delta" => {
                                    if let Some(ref mut tool) = current_tool {
                                        if let Some(partial) = delta["partial_json"].as_str() {
                                            tool.2.push_str(partial);
                                            let _ = tx
                                                .send(StreamEvent::ToolInputDelta(
                                                    partial.to_string(),
                                                ))
                                                .await;
                                        }
                                    }
                                }
                                _ => {}
                            }
                        }
                        "content_block_stop" => {
                            if let Some((id, name, args)) = current_tool.take() {
                                let _ = tx
                                    .send(StreamEvent::ToolUseEnd {
                                        id: id.clone(),
                                        name: name.clone(),
                                        arguments: args.clone(),
                                    })
                                    .await;
                                tool_calls.push(ToolCall {
                                    id,
                                    name,
                                    arguments: args,
                                });
                            }
                        }
                        "message_delta" => {
                            if let Some(sr) = j["delta"]["stop_reason"].as_str() {
                                stop_reason = parse_stop_reason(sr);
                            }
                            if self.provider.capabilities.supports_usage_in_stream {
                                usage.output_tokens =
                                    j["usage"]["output_tokens"].as_u64().unwrap_or(0);
                            }
                        }
                        _ => {}
                    }
                }
            }

            let resp = CompletionResponse {
                text: text_accum,
                tool_calls,
                stop_reason: stop_reason.clone(),
                usage: usage.clone(),
            };
            let _ = tx.send(StreamEvent::Complete { stop_reason, usage }).await;
            return Ok(resp);
        }
        anyhow::bail!("Max retries exceeded");
    }
}

fn parse_response(j: &serde_json::Value) -> CompletionResponse {
    let mut text = String::new();
    let mut tool_calls: Vec<ToolCall> = Vec::new();

    if let Some(content) = j["content"].as_array() {
        for block in content {
            match block["type"].as_str().unwrap_or("") {
                "text" => {
                    if let Some(t) = block["text"].as_str() {
                        text.push_str(t);
                    }
                }
                "tool_use" => {
                    if let (Some(id), Some(name)) = (block["id"].as_str(), block["name"].as_str()) {
                        let arguments = serde_json::to_string(&block["input"]).unwrap_or_default();
                        tool_calls.push(ToolCall {
                            id: id.to_string(),
                            name: name.to_string(),
                            arguments,
                        });
                    }
                }
                _ => {}
            }
        }
    }

    let stop_reason = parse_stop_reason(j["stop_reason"].as_str().unwrap_or(""));
    let usage = TokenUsage {
        input_tokens: j["usage"]["input_tokens"].as_u64().unwrap_or(0),
        output_tokens: j["usage"]["output_tokens"].as_u64().unwrap_or(0),
    };
    CompletionResponse {
        text,
        tool_calls,
        stop_reason,
        usage,
    }
}

fn parse_stop_reason(s: &str) -> StopReason {
    match s {
        "end_turn" => StopReason::EndTurn,
        "max_tokens" => StopReason::MaxTokens,
        "tool_use" => StopReason::ToolUse,
        other => StopReason::Other(other.to_string()),
    }
}
