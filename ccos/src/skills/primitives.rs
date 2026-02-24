//! Primitive Command Mapper
//!
//! Maps common shell commands to CCOS capabilities.
//! For example: `curl -X POST ...` → `ccos.network.http-fetch`

use std::collections::HashMap;

/// Result of mapping a command to a capability
#[derive(Debug, Clone)]
pub struct MappedCapability {
    /// CCOS capability ID
    pub capability_id: String,
    /// Extracted parameters for the capability
    pub params: HashMap<String, serde_json::Value>,
    /// Confidence score (0.0 - 1.0)
    pub confidence: f64,
    /// Human-readable explanation
    pub explanation: String,
}

/// Maps shell commands to CCOS capabilities
pub struct PrimitiveMapper {
    /// Command prefix → capability mappings
    mappings: HashMap<&'static str, &'static str>,
}

impl Default for PrimitiveMapper {
    fn default() -> Self {
        Self::new()
    }
}

impl PrimitiveMapper {
    /// Create a new primitive mapper with default mappings
    pub fn new() -> Self {
        let mut mappings = HashMap::new();

        // HTTP commands
        mappings.insert("curl", "ccos.network.http-fetch");
        mappings.insert("wget", "ccos.network.http-fetch");
        mappings.insert("http", "ccos.network.http-fetch"); // httpie

        // JSON processing
        mappings.insert("jq", "ccos.json.parse");

        // Python
        mappings.insert("python", "ccos.sandbox.python");
        mappings.insert("python3", "ccos.sandbox.python");
        mappings.insert("pip", "ccos.sandbox.python");

        // Node.js
        mappings.insert("node", "ccos.sandbox.node");
        mappings.insert("npm", "ccos.sandbox.node");
        mappings.insert("npx", "ccos.sandbox.node");

        // Shell I/O
        mappings.insert("echo", "ccos.io.println");
        mappings.insert("cat", "ccos.io.read-file");
        mappings.insert("head", "ccos.io.read-file");
        mappings.insert("tail", "ccos.io.read-file");

        // Go
        mappings.insert("go", "ccos.sandbox.go");

        Self { mappings }
    }

    /// Map a command to a CCOS capability
    pub fn map_command(
        &self,
        command: &str,
        context_code: Option<&str>,
    ) -> Option<MappedCapability> {
        let trimmed = command.trim();
        let first_word_raw = trimmed.split_whitespace().next()?;
        let first_word = first_word_raw.split(':').next().unwrap_or(first_word_raw);

        // Look up mapping
        let capability_id = match self.mappings.get(first_word) {
            Some(id) => id.to_string(),
            None if matches!(first_word, "GET" | "POST" | "PUT" | "DELETE") => {
                "ccos.network.http-fetch".to_string()
            }
            None => return None,
        };

        // Extract parameters based on command type
        let (mut params, confidence, explanation) = match first_word {
            "curl" => self.parse_curl_command(trimmed),
            "wget" => self.parse_wget_command(trimmed),
            "python" | "python3" => self.parse_python_command(trimmed),
            "jq" => self.parse_jq_command(trimmed),
            "node" | "npm" | "npx" => self.parse_node_command(trimmed),
            "GET" | "POST" | "PUT" | "DELETE" => self.parse_http_command(trimmed),
            _ => (
                HashMap::new(),
                0.7,
                format!("Mapped {} to {}", first_word, capability_id),
            ),
        };

        // If the command is a run variant (like python:run or node:run) and context_code is provided, default to injecting it
        if let Some(code) = context_code {
            if first_word_raw.ends_with(":run") {
                params.insert(
                    "code".to_string(),
                    serde_json::Value::String(code.to_string()),
                );
            }
        }

        Some(MappedCapability {
            capability_id: capability_id.to_string(),
            params,
            confidence,
            explanation,
        })
    }

    /// Parse curl command into http-fetch parameters
    fn parse_curl_command(
        &self,
        command: &str,
    ) -> (HashMap<String, serde_json::Value>, f64, String) {
        let mut params = HashMap::new();
        let mut method = "GET".to_string();
        let mut url = String::new();
        let mut headers: Vec<(String, String)> = Vec::new();
        let mut body = String::new();

        let parts: Vec<&str> = command.split_whitespace().collect();
        let mut i = 0;

        while i < parts.len() {
            let part = parts[i];
            match part {
                "-X" | "--request" => {
                    if i + 1 < parts.len() {
                        method = parts[i + 1].to_string();
                        i += 1;
                    }
                }
                "-H" | "--header" => {
                    if i + 1 < parts.len() {
                        let header = parts[i + 1].trim_matches('"').trim_matches('\'');
                        if let Some((key, value)) = header.split_once(':') {
                            headers.push((key.trim().to_string(), value.trim().to_string()));
                        }
                        i += 1;
                    }
                }
                "-d" | "--data" | "--data-raw" => {
                    if i + 1 < parts.len() {
                        body = parts[i + 1]
                            .trim_matches('"')
                            .trim_matches('\'')
                            .to_string();
                        i += 1;
                    }
                }
                s if s.starts_with("http://") || s.starts_with("https://") => {
                    url = s.trim_matches('"').trim_matches('\'').to_string();
                }
                _ => {}
            }
            i += 1;
        }

        params.insert("url".to_string(), serde_json::Value::String(url.clone()));
        params.insert(
            "method".to_string(),
            serde_json::Value::String(method.clone()),
        );

        if !headers.is_empty() {
            let headers_map: serde_json::Map<String, serde_json::Value> = headers
                .into_iter()
                .map(|(k, v)| (k, serde_json::Value::String(v)))
                .collect();
            params.insert(
                "headers".to_string(),
                serde_json::Value::Object(headers_map),
            );
        }

        if !body.is_empty() {
            params.insert("body".to_string(), serde_json::Value::String(body));
        }

        let confidence = if !url.is_empty() { 0.95 } else { 0.5 };
        let explanation = format!("curl {} {} → ccos.network.http-fetch", method, url);

        (params, confidence, explanation)
    }

    /// Parse wget command
    fn parse_wget_command(
        &self,
        command: &str,
    ) -> (HashMap<String, serde_json::Value>, f64, String) {
        let mut params = HashMap::new();
        let mut found_url = String::from("unknown");

        // Extract URL (simple case)
        for part in command.split_whitespace() {
            if part.starts_with("http://") || part.starts_with("https://") {
                found_url = part.to_string();
                params.insert(
                    "url".to_string(),
                    serde_json::Value::String(part.to_string()),
                );
                params.insert(
                    "method".to_string(),
                    serde_json::Value::String("GET".to_string()),
                );
                break;
            }
        }

        (
            params,
            0.8,
            format!("wget {} → ccos.network.http-fetch", found_url),
        )
    }

    /// Parse python command
    fn parse_python_command(
        &self,
        command: &str,
    ) -> (HashMap<String, serde_json::Value>, f64, String) {
        let mut params = HashMap::new();

        // Extract script path or inline code
        let parts: Vec<&str> = command.split_whitespace().collect();
        if parts.len() > 1 {
            if parts[1] == "-c" && parts.len() > 2 {
                params.insert(
                    "code".to_string(),
                    serde_json::Value::String(parts[2..].join(" ")),
                );
            } else if !parts[0].ends_with(":run") {
                params.insert(
                    "script".to_string(),
                    serde_json::Value::String(parts[1].to_string()),
                );
            }
        }

        (
            params,
            0.85,
            "python → ccos.sandbox.python (sandboxed execution)".to_string(),
        )
    }

    /// Parse node command
    fn parse_node_command(
        &self,
        command: &str,
    ) -> (HashMap<String, serde_json::Value>, f64, String) {
        let mut params = HashMap::new();

        let parts: Vec<&str> = command.split_whitespace().collect();
        if parts.len() > 1 {
            if parts[1] == "-e" && parts.len() > 2 {
                params.insert(
                    "code".to_string(),
                    serde_json::Value::String(parts[2..].join(" ")),
                );
            } else if !parts[0].ends_with(":run") {
                params.insert(
                    "script".to_string(),
                    serde_json::Value::String(parts[1].to_string()),
                );
            }
        }

        (
            params,
            0.85,
            "node → ccos.sandbox.node (sandboxed execution)".to_string(),
        )
    }

    /// Parse jq command
    fn parse_jq_command(&self, command: &str) -> (HashMap<String, serde_json::Value>, f64, String) {
        let mut params = HashMap::new();

        // Extract jq filter
        let parts: Vec<&str> = command.split_whitespace().collect();
        if parts.len() > 1 {
            let filter = parts[1].trim_matches('"').trim_matches('\'');
            params.insert(
                "path".to_string(),
                serde_json::Value::String(filter.to_string()),
            );
        }

        (
            params,
            0.9,
            "jq → ccos.json.parse (JSON path extraction)".to_string(),
        )
    }

    /// Parse descriptive HTTP command (e.g. POST /api/register)
    fn parse_http_command(
        &self,
        command: &str,
    ) -> (HashMap<String, serde_json::Value>, f64, String) {
        let mut params = HashMap::new();
        let mut lines = command.lines();
        let first_line = lines.next().unwrap_or("");
        let mut parts = first_line.split_whitespace();
        let method = parts.next().unwrap_or("GET").to_string();
        let url = parts.next().unwrap_or("/").to_string();

        params.insert(
            "method".to_string(),
            serde_json::Value::String(method.clone()),
        );
        params.insert("url".to_string(), serde_json::Value::String(url.clone()));

        let mut headers = serde_json::Map::new();
        let mut body = String::new();
        let mut in_body = false;

        for line in lines {
            let trimmed = line.trim();
            if trimmed.is_empty() {
                in_body = true;
                continue;
            }

            if in_body {
                body.push_str(trimmed);
                body.push('\n');
            } else if trimmed.to_lowercase().starts_with("body:") {
                in_body = true;
                if let Some((_, json_body)) = trimmed.split_once(':') {
                    body.push_str(json_body.trim());
                }
            } else if trimmed.to_lowercase().starts_with("headers:") {
                if let Some((_, header_blob)) = trimmed.split_once(':') {
                    let header_blob = header_blob.trim();
                    if let Some((key, value)) = header_blob.split_once(':') {
                        headers.insert(
                            key.trim().to_string(),
                            serde_json::Value::String(value.trim().to_string()),
                        );
                    }
                }
            } else if let Some((key, value)) = trimmed.split_once(':') {
                headers.insert(
                    key.trim().to_string(),
                    serde_json::Value::String(value.trim().to_string()),
                );
            }
        }

        if !headers.is_empty() {
            params.insert("headers".to_string(), serde_json::Value::Object(headers));
        }

        if !body.is_empty() {
            params.insert(
                "body".to_string(),
                serde_json::Value::String(body.trim().to_string()),
            );
        }

        (
            params,
            0.9,
            format!("HTTP {} {} → ccos.network.http-fetch", method, url),
        )
    }

    /// Check if a command is a known primitive
    pub fn is_known_primitive(&self, command: &str) -> bool {
        let first_word_raw = command.trim().split_whitespace().next().unwrap_or("");
        let first_word = first_word_raw.split(':').next().unwrap_or(first_word_raw);
        if self.mappings.contains_key(first_word) {
            return true;
        }
        // Support descriptive HTTP methods
        matches!(first_word, "GET" | "POST" | "PUT" | "DELETE")
    }

    /// Get all known primitives
    pub fn list_primitives(&self) -> Vec<(&'static str, &'static str)> {
        self.mappings.iter().map(|(&k, &v)| (k, v)).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_curl_mapping() {
        let mapper = PrimitiveMapper::new();

        let result = mapper.map_command("curl -X POST https://api.example.com/v1/search -H 'Content-Type: application/json' -d '{\"query\": \"test\"}'", None);
        assert!(result.is_some());

        let mapped = result.unwrap();
        assert_eq!(mapped.capability_id, "ccos.network.http-fetch");
        assert_eq!(
            mapped.params.get("method").and_then(|v| v.as_str()),
            Some("POST")
        );
        assert!(mapped.confidence > 0.9);
    }

    #[test]
    fn test_simple_curl() {
        let mapper = PrimitiveMapper::new();

        let result = mapper.map_command("curl https://httpbin.org/get", None);
        assert!(result.is_some());

        let mapped = result.unwrap();
        assert_eq!(mapped.capability_id, "ccos.network.http-fetch");
        assert_eq!(
            mapped.params.get("url").and_then(|v| v.as_str()),
            Some("https://httpbin.org/get")
        );
    }

    #[test]
    fn test_python_mapping() {
        let mapper = PrimitiveMapper::new();

        let result = mapper.map_command("python script.py", None);
        assert!(result.is_some());
        assert_eq!(result.unwrap().capability_id, "ccos.sandbox.python");
    }

    #[test]
    fn test_jq_mapping() {
        let mapper = PrimitiveMapper::new();

        let result = mapper.map_command("jq '.data.items[]'", None);
        assert!(result.is_some());

        let mapped = result.unwrap();
        assert_eq!(mapped.capability_id, "ccos.json.parse");
        assert_eq!(
            mapped.params.get("path").and_then(|v| v.as_str()),
            Some(".data.items[]")
        );
    }

    #[test]
    fn test_unknown_command() {
        let mapper = PrimitiveMapper::new();

        let result = mapper.map_command("ffmpeg -i input.mp4 output.avi", None);
        assert!(result.is_none()); // Unknown, should route to sandbox
    }

    #[test]
    fn test_is_known_primitive() {
        let mapper = PrimitiveMapper::new();

        assert!(mapper.is_known_primitive("curl https://example.com"));
        assert!(mapper.is_known_primitive("python script.py"));
        assert!(!mapper.is_known_primitive("ffmpeg -i input.mp4"));
    }
}
