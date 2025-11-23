use once_cell::sync::OnceCell;
use serde::Deserialize;
use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};

use crate::capability_marketplace::types::{CapabilityManifest, ProviderType};
use crate::capability_marketplace::CapabilityMarketplace;
use rtfs::ast::TypeExpr;
use rtfs::runtime::error::{RuntimeError, RuntimeResult};

/// Collect directories containing `.rtfs` capability manifests under `root`.
fn collect_rtfs_directories(path: &Path, dirs: &mut HashSet<PathBuf>) -> std::io::Result<bool> {
    if !path.is_dir() {
        return Ok(false);
    }

    let mut has_local_rtfs = false;
    let mut has_rtfs_in_children = false;

    for entry in fs::read_dir(path)? {
        let entry = entry?;
        let child = entry.path();
        if child.is_dir() {
            if collect_rtfs_directories(&child, dirs)? {
                has_rtfs_in_children = true;
            }
        } else if child
            .extension()
            .and_then(|ext| if ext == "rtfs" { Some(()) } else { None })
            .is_some()
        {
            has_local_rtfs = true;
        }
    }

    if has_local_rtfs {
        dirs.insert(path.to_path_buf());
    }

    Ok(has_local_rtfs || has_rtfs_in_children)
}

/// Preload capability manifests discovered under `root` into the marketplace.
pub async fn preload_discovered_capabilities(
    marketplace: &CapabilityMarketplace,
    root: &Path,
) -> RuntimeResult<usize> {
    let mut dirs = HashSet::new();
    if let Err(e) = collect_rtfs_directories(root, &mut dirs) {
        return Err(RuntimeError::Generic(format!(
            "Failed to scan discovered capabilities: {}",
            e
        )));
    }

    let mut dirs_vec: Vec<PathBuf> = dirs.into_iter().collect();
    dirs_vec.sort();

    let mut total_loaded = 0usize;
    for dir in dirs_vec {
        eprintln!("DEBUG: preload_discovered_capabilities dir={}", dir.display());
        match marketplace.import_capabilities_from_rtfs_dir(&dir).await {
            Ok(count) => {
                eprintln!("DEBUG: import_capabilities_from_rtfs_dir count={}", count);
                if count == 0 {
                    // Fallback: parse simple MCP RTFS files one by one to register alias manifests.
                    if let Ok(entries) = fs::read_dir(&dir) {
                        let mut fallback_count = 0usize;
                        for entry in entries.flatten() {
                            let path = entry.path();
                            if path
                                .extension()
                                .and_then(|ext| if ext == "rtfs" { Some(()) } else { None })
                                .is_none()
                            {
                                continue;
                            }
                            eprintln!("DEBUG: fallback parsing path={}", path.display());
                            if let Some(manifest) = parse_simple_mcp_rtfs(&path)? {
                                eprintln!("DEBUG: registering manifest id={}", manifest.id);
                                marketplace.register_capability_manifest(manifest).await?;
                                fallback_count += 1;
                            }
                        }
                        total_loaded += fallback_count;
                    }
                } else {
                    total_loaded += count;
                }
            }
            Err(e) => {
                return Err(RuntimeError::Generic(format!(
                    "Failed to import capabilities from {}: {}",
                    dir.display(),
                    e
                )));
            }
        }
    }

    Ok(total_loaded)
}

/// Parse a simple RTFS capability exported from heuristic MCP discovery.
pub fn parse_simple_mcp_rtfs(path: &Path) -> RuntimeResult<Option<CapabilityManifest>> {
    let content = match fs::read_to_string(path) {
        Ok(c) => c,
        Err(e) => {
            return Err(RuntimeError::Generic(format!(
                "Failed to read RTFS file {}: {}",
                path.display(),
                e
            )))
        }
    };

    let extract_quoted = |key: &str| -> Option<String> {
        let start = content.find(key)? + key.len();
        let after_key = &content[start..];
        let mut chars = after_key.chars();
        let mut offset = 0;
        
        // Skip whitespace to find opening quote
        loop {
             match chars.next() {
                 Some(c) if c.is_whitespace() => offset += c.len_utf8(),
                 Some('"') => {
                     offset += 1; // skip quote
                     break;
                 }
                 _ => return None, // Found non-whitespace before quote, or EOF
             }
        }

        let after_quote = &content[start + offset..];
        let end = after_quote.find('"')?;
        let result = after_quote[..end].to_string();
        // eprintln!("DEBUG: extract_quoted key='{}' found='{}'", key, result);
        Some(result)
    };

    let id = extract_quoted("(capability").or_else(|| extract_quoted(":id"));
    let name = extract_quoted(":name");
    let description = extract_quoted(":description");
    let server_url = extract_quoted(":server_url").or_else(|| extract_quoted(":server-url"));
    let tool_name = extract_quoted(":tool_name").or_else(|| extract_quoted(":tool-name"));
    
    if std::env::var("CCOS_DEBUG_DISCOVERY").is_ok() {
        eprintln!("DEBUG: parse_simple_mcp_rtfs path={:?}", path);
        eprintln!("DEBUG: id={:?} name={:?} server_url={:?} tool_name={:?}", id, name, server_url, tool_name);
    }

    let requires_session =
        extract_quoted(":requires_session").or_else(|| extract_quoted(":requires-session"));
    let auth_env_var =
        extract_quoted(":auth_env_var").or_else(|| extract_quoted(":auth-env-var"));
    let version = extract_quoted(":version").unwrap_or_else(|| "1.0.0".to_string());

    let id = match id {
        Some(id) => id,
        None => return Ok(None),
    };

    let name = name.unwrap_or_else(|| id.split('.').last().unwrap_or(&id).to_string());
    let description = description.unwrap_or_default();

    let server_url = match server_url {
        Some(url) => url,
        None => return Ok(None),
    };

    let tool_name = match tool_name {
        Some(name) => name,
        None => return Ok(None),
    };

    let mut manifest = CapabilityManifest::new(
        id.clone(),
        name,
        description.clone(),
        ProviderType::MCP(crate::capability_marketplace::types::MCPCapability {
            server_url: server_url.clone(),
            tool_name: tool_name.clone(),
            timeout_ms: 30_000,
            auth_token: None,
        }),
        version,
    );

    if let Some(req) = requires_session {
        manifest
            .metadata
            .insert("mcp_requires_session".to_string(), req);
    }
    if let Some(auth) = auth_env_var {
        manifest
            .metadata
            .insert("mcp_auth_env_var".to_string(), auth);
    }
    manifest
        .metadata
        .insert("mcp_server_url".to_string(), server_url);
    manifest
        .metadata
        .insert("mcp_tool_name".to_string(), tool_name);
    manifest.metadata.insert(
        "capability_source".to_string(),
        "discovered_rtfs".to_string(),
    );
    manifest
        .metadata
        .insert("original_description".to_string(), description);

    if let Some(schema) = extract_type_expr(&content, ":input-schema") {
        manifest.input_schema = Some(schema);
    }
    if let Some(schema) = extract_type_expr(&content, ":output-schema") {
        manifest.output_schema = Some(schema);
    }

    Ok(Some(manifest))
}

fn extract_type_expr(content: &str, key: &str) -> Option<TypeExpr> {
    let start = content.find(key)? + key.len();
    let bytes = content.as_bytes();
    let mut idx = start;
    while idx < bytes.len() && bytes[idx].is_ascii_whitespace() {
        idx += 1;
    }
    if idx >= bytes.len() {
        return None;
    }

    let first = bytes[idx] as char;
    let mut end = idx;

    match first {
        '[' | '(' | '{' => {
            let mut depth = 0isize;
            while end < bytes.len() {
                let ch = bytes[end] as char;
                match ch {
                    '[' | '(' | '{' => {
                        depth += 1;
                    }
                    ']' | ')' | '}' => {
                        depth -= 1;
                        if depth == 0 {
                            end += 1;
                            break;
                        }
                    }
                    _ => {}
                }
                end += 1;
            }
        }
        _ => {
            while end < bytes.len()
                && !bytes[end].is_ascii_whitespace()
                && bytes[end] != b','
                && bytes[end] != b')'
            {
                end += 1;
            }
        }
    }

    if end <= idx {
        return None;
    }

    let mut expr = content[idx..end].trim().to_string();
    expr = expr.trim_end_matches(',').trim().to_string();
    if expr.is_empty() {
        return None;
    }

    if std::env::var("CCOS_DEBUG_SCHEMA").is_ok() {
        eprintln!("Parsing type expression '{}' for key {}", expr, key);
    }

    match TypeExpr::from_str(&expr) {
        Ok(value) => Some(value),
        Err(e) => {
            eprintln!(
                "⚠️  Failed to parse type expression for {}: {} (expr: {})",
                key, e, expr
            );
            None
        }
    }
}

#[derive(Debug, Deserialize)]
struct OverrideParameter {
    #[serde(rename = "type")]
    param_type: String,
    #[serde(default)]
    description: Option<String>,
}

#[derive(Debug, Deserialize)]
struct OverrideEntry {
    #[serde(default)]
    description: Option<String>,
    #[serde(default)]
    parameters: HashMap<String, OverrideParameter>,
}

type OverrideMap = HashMap<String, OverrideEntry>;

fn overrides_cache() -> &'static OnceCell<OverrideMap> {
    static CACHE: OnceCell<OverrideMap> = OnceCell::new();
    &CACHE
}

pub fn load_override_parameters(capability_id: &str) -> Option<(Vec<String>, Vec<String>)> {
    let cache = overrides_cache().get_or_init(|| {
        let path = Path::new("capabilities/mcp/overrides.json");
        if let Ok(contents) = fs::read_to_string(path) {
            serde_json::from_str::<OverrideMap>(&contents).unwrap_or_default()
        } else {
            HashMap::new()
        }
    });

    cache.get(capability_id).map(|entry| {
        eprintln!("Override parameters loaded for {}", capability_id);
        let mut required = Vec::new();
        let mut optional = Vec::new();
        for (name, param) in &entry.parameters {
            let normalized = param.param_type.trim();
            let is_optional = normalized.starts_with("[:optional")
                || normalized.ends_with("?")
                || normalized.contains("optional");
            if is_optional {
                optional.push(name.clone());
            } else {
                required.push(name.clone());
            }
        }
        (required, optional)
    })
}

/// Tokenize an identifier or free text into deduplicated lowercase tokens.
pub fn tokenize_identifier(text: &str) -> Vec<String> {
    let mut tokens = Vec::new();
    let mut seen = HashSet::new();
    
    // Common English stopwords to filter out
    let stopwords: HashSet<&str> = [
        "of", "for", "the", "a", "an", "in", "on", "at", "to", "from", "by", 
        "with", "and", "or", "is", "are", "was", "were", "be", "been", "being",
        "have", "has", "had", "do", "does", "did", "but", "if", "then", "else",
        "when", "where", "why", "how", "all", "any", "some", "no", "not",
    ].iter().cloned().collect();

    for token in text.split(|c: char| !c.is_ascii_alphanumeric()) {
        let tk = token.trim().to_ascii_lowercase();
        if tk.is_empty() || seen.contains(&tk) || stopwords.contains(tk.as_str()) {
            continue;
        }
        seen.insert(tk.clone());
        tokens.push(tk);
    }

    tokens
}

/// Compute a heuristic score between a capability manifest and a token set.
pub fn score_manifest_against_tokens(manifest: &CapabilityManifest, tokens: &[String]) -> usize {
    if tokens.is_empty() {
        return 0;
    }

    let id = manifest.id.to_ascii_lowercase();
    let name = manifest.name.to_ascii_lowercase();
    let description = manifest.description.to_ascii_lowercase();

    let metadata_values: Vec<String> = manifest
        .metadata
        .values()
        .map(|value| value.to_ascii_lowercase())
        .collect();

    let mut score = 0usize;
    for token in tokens {
        if id.contains(token) {
            score += 6;
        }
        if name.contains(token) {
            score += 3;
        }
        if description.contains(token) {
            score += 1;
        }
        if metadata_values.iter().any(|value| value.contains(token)) {
            score += 1;
        }
    }

    score
}

/// Count how many tokens appear within a capability manifest.
pub fn count_token_matches(manifest: &CapabilityManifest, tokens: &[String]) -> usize {
    if tokens.is_empty() {
        return 0;
    }

    let id = manifest.id.to_ascii_lowercase();
    let name = manifest.name.to_ascii_lowercase();
    let description = manifest.description.to_ascii_lowercase();
    let metadata_values: Vec<String> = manifest
        .metadata
        .values()
        .map(|value| value.to_ascii_lowercase())
        .collect();

    tokens
        .iter()
        .filter(|token| {
            id.contains(*token)
                || name.contains(*token)
                || description.contains(*token)
                || metadata_values.iter().any(|value| value.contains(*token))
        })
        .count()
}

/// Minimum number of matches required to consider a manifest relevant.
pub fn minimum_token_matches(token_count: usize) -> usize {
    match token_count {
        0 => 0,
        1 => 1,
        2..=3 => 2,
        _ => 3,
    }
}

/// Convenience wrapper to tokenize an arbitrary string map for search features.
pub fn tokenize_map_values<K: AsRef<str>, V: AsRef<str>>(map: &HashMap<K, V>) -> HashSet<String> {
    let mut set = HashSet::new();
    for value in map.values() {
        for token in tokenize_identifier(value.as_ref()) {
            set.insert(token);
        }
    }
    set
}
