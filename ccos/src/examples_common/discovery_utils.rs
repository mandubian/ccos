use url::Url;

/// Derive an `owner/repo` style server name from a GitHub repository URL.
/// Returns `Some(owner/repo)` if parse succeeds and there are at least two path segments.
pub fn derive_server_name_from_repo_url(url: &str) -> Option<String> {
    if let Ok(parsed) = Url::parse(url) {
        let mut segs: Vec<&str> = parsed
            .path_segments()
            .map(|s| s.collect())
            .unwrap_or_default();
        segs.retain(|s| !s.is_empty());
        if segs.len() >= 2 {
            let owner = segs[0];
            let repo = segs[1];
            return Some(format!("{}/{}", owner, repo));
        }
    }
    None
}

/// Extract a suggestion for `key` (like "repo") from `text`.
/// Tries JSON parse, then `key: value` or `key -> value` patterns, then bare tokens.
pub fn extract_suggestion_from_text(text: &str, key: &str) -> Option<String> {
    // Try JSON first
    if let Some(obj_start) = text.find('{') {
        if let Ok(parsed) = serde_json::from_str::<serde_json::Value>(&text[obj_start..]) {
            if let Some(v) = parsed.get(key).and_then(|v| v.as_str()) {
                return Some(v.to_string());
            }
        }
    }

    // key: value or key -> value — allow case-insensitive, word-boundary matches and quoted values
    // e.g. "repo: hello-world" or "Repo -> \"hello-world\""
    // Support several separator forms: ":", "->", "=", or a plain hyphen.
    let colon_re = regex::Regex::new(&format!(
        "(?i)\\b{}\\b\\s*(?::|->|=|-)\\s*\"?([a-zA-Z0-9_\\-]+(?:/[a-zA-Z0-9_\\-]+)?)\"?",
        regex::escape(key)
    ))
    .ok();
    if let Some(re) = colon_re {
        if let Some(cap) = re.captures(text) {
            if let Some(m) = cap.get(1) {
                let s = m.as_str();
                if key == "repo" && s.contains('/') {
                    if let Some(parts) = s.split('/').nth(1) {
                        return Some(parts.to_string());
                    }
                }
                return Some(s.to_string());
            }
        }
    }

    // Bare token or owner/repo
    // Bare token or owner/repo. Skip tokens that match the key itself (e.g., "repo").
    let re = regex::Regex::new(r"[a-zA-Z0-9_\-]+/[a-zA-Z0-9_\-]+|[a-zA-Z0-9_\-]+").ok()?;
    for cap in re.captures_iter(text) {
        if let Some(m) = cap.get(0) {
            let s = m.as_str();
            // Don't return the query key itself as a suggestion (e.g., when text == "repo").
            if s.eq_ignore_ascii_case(key) {
                continue;
            }
            if key == "repo" {
                if s.contains('/') {
                    let parts: Vec<&str> = s.split('/').collect();
                    if parts.len() >= 2 {
                        return Some(parts[1].to_string());
                    }
                }
            }
            return Some(s.to_string());
        }
    }
    None
}
