//! Log redaction helpers to avoid leaking secrets in traces.

use serde_json::Value;

const REDACTED: &str = "***REDACTED***";

fn is_sensitive_key(key: &str) -> bool {
    let k = key.to_ascii_lowercase();
    k.contains("secret")
        || k.contains("token")
        || k.contains("password")
        || k.contains("api_key")
        || k.contains("apikey")
        || k.contains("authorization")
        || k.contains("access_key")
        || k.contains("access_token")
        || k.contains("refresh_token")
        || k.contains("client_secret")
}

fn redact_json_value(value: &Value) -> Value {
    match value {
        Value::Object(map) => {
            let mut out = serde_json::Map::new();
            for (k, v) in map {
                if is_sensitive_key(k) {
                    out.insert(k.clone(), Value::String(REDACTED.to_string()));
                } else {
                    out.insert(k.clone(), redact_json_value(v));
                }
            }
            Value::Object(out)
        }
        Value::Array(items) => Value::Array(items.iter().map(redact_json_value).collect()),
        Value::String(s) => {
            // Basic bearer/sk- token masking in free text.
            if s.to_ascii_lowercase().contains("bearer ") || s.starts_with("sk-") {
                Value::String(REDACTED.to_string())
            } else {
                Value::String(s.clone())
            }
        }
        other => other.clone(),
    }
}

/// Redact potentially sensitive content for structured logging.
pub fn redact_text_for_logs(text: &str) -> String {
    match serde_json::from_str::<Value>(text) {
        Ok(v) => serde_json::to_string(&redact_json_value(&v)).unwrap_or_else(|_| REDACTED.to_string()),
        Err(_) => {
            // Non-JSON payloads: avoid accidentally dumping long secrets.
            if text.to_ascii_lowercase().contains("token")
                || text.to_ascii_lowercase().contains("secret")
                || text.to_ascii_lowercase().contains("authorization")
            {
                REDACTED.to_string()
            } else {
                text.to_string()
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::redact_text_for_logs;

    #[test]
    fn test_redacts_sensitive_json_keys() {
        let input = r#"{"token":"abc","nested":{"client_secret":"xyz"},"safe":"ok"}"#;
        let out = redact_text_for_logs(input);
        assert!(out.contains("***REDACTED***"));
        assert!(out.contains("\"safe\":\"ok\""));
        assert!(!out.contains("\"abc\""));
        assert!(!out.contains("\"xyz\""));
    }

    #[test]
    fn test_redacts_secret_like_plain_text() {
        let out = redact_text_for_logs("Authorization: Bearer very-secret-value");
        assert_eq!(out, "***REDACTED***");
    }
}
