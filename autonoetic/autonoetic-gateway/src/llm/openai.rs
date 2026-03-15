//! OpenAI-compatible LLM Driver.
//!
//! Handles OpenAI, OpenRouter, Groq, Together, DeepSeek, Mistral, Ollama, etc.
//! All routing decisions (base URL, auth headers, capabilities) are resolved
//! externally by `provider::resolve()` before this driver is instantiated.

use super::{
    CompletionRequest, CompletionResponse, LlmDriver, StopReason, StreamEvent, TokenUsage, ToolCall,
};
use crate::llm::provider::{AuthStrategy, ResolvedProvider};
use reqwest::Client;
use serde_json::json;

/// Whether a model uses `max_completion_tokens` instead of `max_tokens`.
/// (GPT-5 and o-series reasoning models require this.)
fn uses_completion_tokens(model: &str) -> bool {
    let m = model.to_lowercase();
    m.starts_with("gpt-5") || m.starts_with("o1") || m.starts_with("o3") || m.starts_with("o4")
}

pub struct OpenAiDriver {
    client: Client,
    provider: ResolvedProvider,
}

impl OpenAiDriver {
    pub fn new(client: Client, provider: ResolvedProvider) -> Self {
        Self { client, provider }
    }

    fn apply_auth(&self, builder: reqwest::RequestBuilder) -> reqwest::RequestBuilder {
        let mut b = builder;
        match &self.provider.auth {
            AuthStrategy::BearerToken(key) => {
                b = b.header("Authorization", format!("Bearer {}", key));
            }
            AuthStrategy::None => {}
            _ => {} // unreachable for OpenAI, but handled gracefully
        }
        for (k, v) in &self.provider.extra_headers {
            b = b.header(k, v);
        }
        b
    }

    fn build_body(&self, req: &CompletionRequest, stream: bool) -> serde_json::Value {
        let messages: Vec<serde_json::Value> = req
            .messages
            .iter()
            .map(|m| {
                let mut msg = json!({ "role": m.role.as_str() });

                if !m.content.is_empty() {
                    msg["content"] = json!(m.content);
                }
                if !m.tool_calls.is_empty() {
                    msg["tool_calls"] = json!(m
                        .tool_calls
                        .iter()
                        .map(|tc| json!({
                            "id": tc.id,
                            "type": "function",
                            "function": { "name": tc.name, "arguments": tc.arguments }
                        }))
                        .collect::<Vec<_>>());
                }
                if let Some(ref id) = m.tool_call_id {
                    msg["tool_call_id"] = json!(id);
                }
                msg
            })
            .collect();

        let (token_key, token_val) = if uses_completion_tokens(&self.provider.model) {
            (
                "max_completion_tokens",
                req.max_tokens.or(self.provider.max_tokens),
            )
        } else {
            ("max_tokens", req.max_tokens.or(self.provider.max_tokens))
        };

        let mut body = json!({
            "model": self.provider.model,
            "messages": messages,
            "stream": stream,
        });

        if let Some(v) = token_val {
            body[token_key] = json!(v);
        }

        let t = req.temperature.or(self.provider.temperature);
        if let Some(t) = t {
            if t > 0.0 {
                body["temperature"] = json!(t);
            }
        }

        // Only include tools if provider supports them
        if !req.tools.is_empty() && self.provider.capabilities.supports_tools {
            body["tools"] = json!(req
                .tools
                .iter()
                .map(|t| json!({
                    "type": "function",
                    "function": {
                        "name": t.name,
                        "description": t.description,
                        "parameters": t.input_schema,
                    }
                }))
                .collect::<Vec<_>>());
            // Only include tool_choice if provider supports it
            if self.provider.capabilities.supports_tool_choice {
                body["tool_choice"] = json!("auto");
            }
        }

        body
    }
}

#[async_trait::async_trait]
impl LlmDriver for OpenAiDriver {
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
            let status = response.status();

            if status.as_u16() == 429 || status.as_u16() == 529 {
                if attempt < MAX_RETRIES {
                    let wait_ms = (attempt + 1) as u64 * 2000;
                    tracing::warn!(
                        status = status.as_u16(),
                        attempt,
                        wait_ms,
                        "Rate limited, retrying"
                    );
                    tokio::time::sleep(std::time::Duration::from_millis(wait_ms)).await;
                    continue;
                }
                anyhow::bail!("OpenAI API rate limited after {} retries", MAX_RETRIES);
            }

            if !status.is_success() {
                let text = response.text().await.unwrap_or_default();
                anyhow::bail!("OpenAI API error {}: {}", status, text);
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

        if !self.provider.capabilities.supports_streaming {
            // Fall back to complete() and emit one chunk
            return super::LlmDriver::stream(self as &dyn super::LlmDriver, req, tx).await;
        }

        let body = self.build_body(req, true);
        let builder = self.apply_auth(
            self.client
                .post(&self.provider.base_url)
                .header("Content-Type", "application/json")
                .header("Accept", "text/event-stream")
                .json(&body),
        );

        let response = builder.send().await?;
        if !response.status().is_success() {
            let status = response.status();
            let text = response.text().await.unwrap_or_default();
            anyhow::bail!("OpenAI stream error {}: {}", status, text);
        }

        let mut text_accum = String::new();
        let mut tool_calls_accum: Vec<ToolCall> = Vec::new();
        let mut stop_reason = StopReason::EndTurn;
        let mut buffer = String::new();
        let mut byte_stream = response.bytes_stream();

        while let Some(chunk) = byte_stream.next().await {
            let chunk = chunk?;
            buffer.push_str(&String::from_utf8_lossy(&chunk));

            while let Some(pos) = buffer.find("\n\n") {
                let event_text = buffer[..pos].to_string();
                buffer = buffer[pos + 2..].to_string();

                let data = event_text
                    .lines()
                    .find_map(|l| l.strip_prefix("data: "))
                    .unwrap_or("")
                    .trim()
                    .to_string();

                if data.is_empty() || data == "[DONE]" {
                    continue;
                }

                let Ok(j) = serde_json::from_str::<serde_json::Value>(&data) else {
                    continue;
                };
                let delta = &j["choices"][0]["delta"];

                if let Some(text) = delta["content"].as_str() {
                    if !text.is_empty() {
                        text_accum.push_str(text);
                        let _ = tx.send(StreamEvent::TextDelta(text.to_string())).await;
                    }
                }

                if self.provider.capabilities.supports_tool_stream_deltas {
                    if let Some(tcs) = delta["tool_calls"].as_array() {
                        for tc_delta in tcs {
                            let idx = tc_delta["index"].as_u64().unwrap_or(0) as usize;
                            while tool_calls_accum.len() <= idx {
                                tool_calls_accum.push(ToolCall {
                                    id: String::new(),
                                    name: String::new(),
                                    arguments: String::new(),
                                });
                            }
                            if let Some(id) = tc_delta["id"].as_str() {
                                tool_calls_accum[idx].id = id.to_string();
                            }
                            if let Some(name) = tc_delta["function"]["name"].as_str() {
                                tool_calls_accum[idx].name = name.to_string();
                            }
                            if let Some(args) = tc_delta["function"]["arguments"].as_str() {
                                tool_calls_accum[idx].arguments.push_str(args);
                            }
                        }
                    }
                }

                if let Some(reason) = j["choices"][0]["finish_reason"].as_str() {
                    stop_reason = parse_stop_reason(reason);
                }
            }
        }

        for tc in &tool_calls_accum {
            let _ = tx
                .send(StreamEvent::ToolUseEnd {
                    id: tc.id.clone(),
                    name: tc.name.clone(),
                    arguments: tc.arguments.clone(),
                })
                .await;
        }

        let resp = CompletionResponse {
            text: text_accum,
            tool_calls: tool_calls_accum,
            stop_reason: stop_reason.clone(),
            usage: TokenUsage::default(),
        };
        let _ = tx
            .send(StreamEvent::Complete {
                stop_reason,
                usage: resp.usage.clone(),
            })
            .await;
        Ok(resp)
    }
}

/// Parse a non-streaming JSON response body.
fn parse_response(j: &serde_json::Value) -> CompletionResponse {
    let choice = &j["choices"][0];
    let text = extract_text_content(&choice["message"]["content"]);

    let tool_calls = choice["message"]["tool_calls"]
        .as_array()
        .map(|arr| {
            arr.iter()
                .filter_map(|tc| {
                    Some(ToolCall {
                        id: tc["id"].as_str()?.to_string(),
                        name: tc["function"]["name"].as_str()?.to_string(),
                        arguments: tc["function"]["arguments"]
                            .as_str()
                            .unwrap_or("{}")
                            .to_string(),
                    })
                })
                .collect()
        })
        .unwrap_or_default();

    let stop_reason = parse_stop_reason(choice["finish_reason"].as_str().unwrap_or(""));

    let usage = TokenUsage {
        input_tokens: j["usage"]["prompt_tokens"].as_u64().unwrap_or(0),
        output_tokens: j["usage"]["completion_tokens"].as_u64().unwrap_or(0),
    };

    CompletionResponse {
        text,
        tool_calls,
        stop_reason,
        usage,
    }
}

fn extract_text_content(content: &serde_json::Value) -> String {
    if let Some(s) = content.as_str() {
        return s.to_string();
    }
    if let Some(arr) = content.as_array() {
        let mut out = String::new();
        for item in arr {
            if let Some(s) = item.as_str() {
                out.push_str(s);
                continue;
            }
            if let Some(s) = item["text"].as_str() {
                out.push_str(s);
                continue;
            }
            if let Some(s) = item["content"].as_str() {
                out.push_str(s);
                continue;
            }
        }
        return out;
    }
    if let Some(s) = content["text"].as_str() {
        return s.to_string();
    }
    if let Some(s) = content["content"].as_str() {
        return s.to_string();
    }
    String::new()
}

fn parse_stop_reason(s: &str) -> StopReason {
    match s {
        "stop" | "end_turn" => StopReason::EndTurn,
        "length" => StopReason::MaxTokens,
        "tool_calls" | "tool_use" => StopReason::ToolUse,
        other => StopReason::Other(other.to_string()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn test_parse_response_with_string_content() {
        let j = json!({
            "choices": [{
                "message": { "content": "hello" },
                "finish_reason": "stop"
            }],
            "usage": { "prompt_tokens": 1, "completion_tokens": 2 }
        });
        let resp = parse_response(&j);
        assert_eq!(resp.text, "hello");
    }

    #[test]
    fn test_parse_response_with_array_content_blocks() {
        let j = json!({
            "choices": [{
                "message": {
                    "content": [
                        {"type": "text", "text": "hello "},
                        {"type": "text", "text": "world"}
                    ]
                },
                "finish_reason": "stop"
            }],
            "usage": { "prompt_tokens": 1, "completion_tokens": 2 }
        });
        let resp = parse_response(&j);
        assert_eq!(resp.text, "hello world");
    }
}
