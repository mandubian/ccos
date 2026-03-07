//! Store directive handling for onboarding flows.
//!
//! Parses `Store` mappings from skill markdown and applies secure secret
//! persistence/redaction on tool responses before they are returned to the LLM.

use crate::vault::Vault;
use serde_json::Value;
use std::path::PathBuf;

const VAULT_PATH_ENV: &str = "AUTONOETIC_VAULT_PATH";

#[derive(Debug, Clone)]
pub struct SecretStoreDirective {
    source_path: Vec<String>,
    secret_name: String,
}

pub struct SecretStoreRuntime {
    directives: Vec<SecretStoreDirective>,
    vault_path: PathBuf,
    vault: Vault,
}

impl SecretStoreRuntime {
    pub fn from_instructions(instructions: &str) -> anyhow::Result<Option<Self>> {
        let directives = parse_secret_store_directives(instructions);
        if directives.is_empty() {
            return Ok(None);
        }
        let vault_path = std::env::var(VAULT_PATH_ENV).map_err(|_| {
            anyhow::anyhow!("Missing required environment variable {}", VAULT_PATH_ENV)
        })?;
        let vault_path = PathBuf::from(vault_path);
        let vault = Vault::load_from_file(&vault_path)?;
        Ok(Some(Self {
            directives,
            vault_path,
            vault,
        }))
    }

    pub fn apply_and_redact(&mut self, response_text: &str) -> anyhow::Result<String> {
        let mut value: Value = match serde_json::from_str(response_text) {
            Ok(v) => v,
            Err(_) => return Ok(response_text.to_string()),
        };
        let mut changed = false;

        for d in &self.directives {
            if let Some(secret_val) = extract_json_path_as_string(&value, &d.source_path) {
                self.vault.set_secret(&d.secret_name, secret_val);
                redact_json_path(&mut value, &d.source_path, "[REDACTED]");
                changed = true;
            }
        }

        if changed {
            self.vault.persist_to_file(&self.vault_path)?;
        }
        Ok(serde_json::to_string(&value)?)
    }
}

fn parse_secret_store_directives(markdown: &str) -> Vec<SecretStoreDirective> {
    let mut out = Vec::new();
    for line in markdown.lines() {
        if !line.contains("From:") || !line.contains("To:") {
            continue;
        }
        let parts = extract_backticked_segments(line);
        if parts.len() < 2 {
            continue;
        }
        let from = parts[0].trim();
        let to = parts[1].trim();

        if !from.starts_with("response.") || !to.starts_with("secret:") {
            continue;
        }
        let source_path = from
            .trim_start_matches("response.")
            .split('.')
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .collect::<Vec<_>>();
        if source_path.is_empty() {
            continue;
        }
        let secret_name = to.trim_start_matches("secret:").trim().to_string();
        if secret_name.is_empty() {
            continue;
        }
        out.push(SecretStoreDirective {
            source_path,
            secret_name,
        });
    }
    out
}

fn extract_backticked_segments(line: &str) -> Vec<String> {
    let mut segments = Vec::new();
    let mut current = String::new();
    let mut in_tick = false;
    for ch in line.chars() {
        if ch == '`' {
            if in_tick {
                segments.push(current.clone());
                current.clear();
            }
            in_tick = !in_tick;
            continue;
        }
        if in_tick {
            current.push(ch);
        }
    }
    segments
}

fn extract_json_path_as_string(value: &Value, path: &[String]) -> Option<String> {
    let mut cur = value;
    for seg in path {
        cur = cur.get(seg)?;
    }
    match cur {
        Value::String(s) => Some(s.clone()),
        Value::Number(n) => Some(n.to_string()),
        Value::Bool(b) => Some(b.to_string()),
        _ => None,
    }
}

fn redact_json_path(value: &mut Value, path: &[String], redaction: &str) {
    if path.is_empty() {
        return;
    }
    let mut cur = value;
    for seg in &path[..path.len() - 1] {
        let Some(next) = cur.get_mut(seg) else {
            return;
        };
        cur = next;
    }
    if let Some(last) = path.last() {
        if let Some(slot) = cur.get_mut(last) {
            *slot = Value::String(redaction.to_string());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_secret_store_directive_from_markdown() {
        let md = r#"- From: `response.secret` → To: `secret:MOLTBOOK_SECRET` (Requires Approval)"#;
        let directives = parse_secret_store_directives(md);
        assert_eq!(directives.len(), 1);
        assert_eq!(directives[0].secret_name, "MOLTBOOK_SECRET");
        assert_eq!(directives[0].source_path, vec!["secret".to_string()]);
    }

    #[test]
    fn test_redact_json_path() {
        let mut value: Value = serde_json::json!({"secret":"abc","nested":{"token":"xyz"}});
        redact_json_path(&mut value, &["secret".to_string()], "[REDACTED]");
        redact_json_path(
            &mut value,
            &["nested".to_string(), "token".to_string()],
            "[REDACTED]",
        );
        assert_eq!(value["secret"], "[REDACTED]");
        assert_eq!(value["nested"]["token"], "[REDACTED]");
    }
}
