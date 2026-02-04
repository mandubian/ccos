use regex::Regex;
use serde_json::Value;

const REDACTED: &str = "***REDACTED***";

fn is_sensitive_key(key: &str) -> bool {
    let key = key.to_ascii_lowercase();
    key.contains("secret")
        || key.contains("token")
        || key.contains("password")
        || key.contains("api_key")
        || key.contains("apikey")
        || key.contains("authorization")
        || key.contains("access_key")
        || key.contains("access_token")
        || key.contains("refresh_token")
        || key.contains("client_secret")
        || key.contains("skill_definition")
}

pub fn redact_token_for_logs(token: &str) -> String {
    if token.is_empty() {
        return "<empty>".to_string();
    }
    if token.len() <= 8 {
        return REDACTED.to_string();
    }
    let prefix = &token[..4];
    let suffix = &token[token.len() - 2..];
    format!("{}...{}", prefix, suffix)
}

pub fn redact_text_for_logs(text: &str) -> String {
    let mut out = text.to_string();

    // Redact JSON-like key/value pairs with sensitive keys.
    let re_json = Regex::new(
        r#"(?i)("(?:(?:client_)?secret|token|password|api_key|apikey|authorization|access_token|refresh_token)"\s*:\s*")([^"]+)(")"#,
    )
    .ok();
    if let Some(re) = re_json {
        out = re.replace_all(&out, format!("$1{}$3", REDACTED)).to_string();
    }

    // Redact Authorization header values.
    let re_bearer = Regex::new(r#"(?i)(authorization\s*:\s*bearer\s+)[^\s"',]+"#).ok();
    if let Some(re) = re_bearer {
        out = re.replace_all(&out, format!("$1{}", REDACTED)).to_string();
    }

    // Redact common secret-looking tokens (e.g., sk_...).
    let re_sk = Regex::new(r"(?i)\bsk_[a-z0-9]{8,}\b").ok();
    if let Some(re) = re_sk {
        out = re.replace_all(&out, REDACTED).to_string();
    }

    out
}

pub fn redact_json_for_logs(value: &Value) -> Value {
    match value {
        Value::Object(map) => {
            let mut out = serde_json::Map::new();
            for (k, v) in map {
                if is_sensitive_key(k) {
                    let redacted = if k.eq_ignore_ascii_case("skill_definition") {
                        "<omitted>".to_string()
                    } else {
                        REDACTED.to_string()
                    };
                    out.insert(k.clone(), Value::String(redacted));
                } else {
                    out.insert(k.clone(), redact_json_for_logs(v));
                }
            }
            Value::Object(out)
        }
        Value::Array(items) => {
            let redacted = items.iter().map(redact_json_for_logs).collect();
            Value::Array(redacted)
        }
        Value::String(s) => Value::String(redact_text_for_logs(s)),
        other => other.clone(),
    }
}
