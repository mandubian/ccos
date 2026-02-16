use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolDefinition {
    pub capability_id: String,
    pub tool_name: String,
    pub description: String,
    pub input_schema: Value,
}

impl ToolDefinition {
    pub fn to_openai_tool_json(&self) -> Value {
        json!({
            "type": "function",
            "function": {
                "name": self.tool_name,
                "description": self.description,
                "parameters": self.input_schema,
            }
        })
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolCall {
    pub id: String,
    pub tool_name: String,
    pub arguments: Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolChatMessage {
    pub role: String,
    pub content: String,
    #[serde(default)]
    pub tool_call_id: Option<String>,
    #[serde(default)]
    pub name: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolChatRequest {
    pub messages: Vec<ToolChatMessage>,
    pub tools: Vec<ToolDefinition>,
    #[serde(default)]
    pub max_tokens: Option<u32>,
    #[serde(default)]
    pub temperature: Option<f64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolChatResponse {
    #[serde(default)]
    pub content: String,
    #[serde(default)]
    pub tool_calls: Vec<ToolCall>,
}

impl ToolChatResponse {
    pub fn has_tool_calls(&self) -> bool {
        !self.tool_calls.is_empty()
    }
}

pub fn capabilities_to_tool_definitions(capabilities: &[String]) -> Vec<ToolDefinition> {
    capabilities
        .iter()
        .filter_map(|line| capability_line_to_tool_definition(line))
        .collect()
}

pub fn resolve_capability_id<'a>(tool_name: &str, defs: &'a [ToolDefinition]) -> Option<&'a str> {
    defs.iter()
        .find(|d| d.tool_name == tool_name)
        .map(|d| d.capability_id.as_str())
}

pub fn extract_openai_assistant_content(response_json: &Value) -> String {
    response_json["choices"][0]["message"]["content"]
        .as_str()
        .unwrap_or_default()
        .to_string()
}

pub fn extract_openai_tool_calls(response_json: &Value) -> Vec<ToolCall> {
    let Some(tool_calls) = response_json["choices"][0]["message"]["tool_calls"].as_array() else {
        return Vec::new();
    };

    tool_calls
        .iter()
        .enumerate()
        .filter_map(|(idx, item)| {
            let tool_name = item["function"]["name"].as_str()?.to_string();
            let id = item["id"]
                .as_str()
                .map(|s| s.to_string())
                .unwrap_or_else(|| format!("tool_call_{}", idx + 1));

            let raw_args = item["function"]["arguments"]
                .as_str()
                .unwrap_or("{}");

            let arguments = match serde_json::from_str::<Value>(raw_args) {
                Ok(v) => v,
                Err(_) => json!({ "raw_arguments": raw_args }),
            };

            Some(ToolCall {
                id,
                tool_name,
                arguments,
            })
        })
        .collect()
}

fn capability_line_to_tool_definition(line: &str) -> Option<ToolDefinition> {
    let capability_id = extract_capability_id(line)?;
    let description = extract_description(line);
    let tool_name = capability_id_to_tool_name(&capability_id);

    Some(ToolDefinition {
        capability_id,
        tool_name,
        description,
        input_schema: default_tool_input_schema(),
    })
}

fn extract_capability_id(line: &str) -> Option<String> {
    let trimmed = line.trim().trim_start_matches("- ").trim();
    if trimmed.is_empty() {
        return None;
    }

    let end = trimmed
        .find(|c: char| c.is_whitespace() || c == '(' || c == '|')
        .unwrap_or(trimmed.len());
    let id = trimmed[..end].trim();

    if id.is_empty() {
        None
    } else {
        Some(id.to_string())
    }
}

fn extract_description(line: &str) -> String {
    if let Some(idx) = line.find(" - ") {
        return line[idx + 3..].trim().to_string();
    }
    line.trim().to_string()
}

fn default_tool_input_schema() -> Value {
    json!({
        "type": "object",
        "additionalProperties": true,
    })
}

pub fn capability_id_to_tool_name(capability_id: &str) -> String {
    let mut base = capability_id
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() || c == '_' || c == '-' { c } else { '_' })
        .collect::<String>();

    if base.is_empty() || base.starts_with(|c: char| c.is_ascii_digit()) {
        base = format!("cap_{}", base);
    }

    if base.len() <= 64 {
        return base;
    }

    let mut hasher = Sha256::new();
    hasher.update(capability_id.as_bytes());
    let hash = format!("{:x}", hasher.finalize());
    let short_hash = &hash[..8];

    let keep = 64usize.saturating_sub(1 + short_hash.len());
    format!("{}_{}", &base[..keep], short_hash)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_capability_line() {
        let defs = capabilities_to_tool_definitions(&["- ccos.memory.get (1.0.0) - Get value".to_string()]);
        assert_eq!(defs.len(), 1);
        assert_eq!(defs[0].capability_id, "ccos.memory.get");
        assert_eq!(defs[0].tool_name, "ccos_memory_get");
    }

    #[test]
    fn converts_tool_calls_from_openai_shape() {
        let payload = json!({
            "choices": [{
                "message": {
                    "content": "",
                    "tool_calls": [{
                        "id": "call_1",
                        "function": {
                            "name": "ccos_memory_get",
                            "arguments": "{\"key\":\"last_fibonacci\"}"
                        }
                    }]
                }
            }]
        });

        let calls = extract_openai_tool_calls(&payload);
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].tool_name, "ccos_memory_get");
        assert_eq!(calls[0].arguments["key"], "last_fibonacci");
    }
}
