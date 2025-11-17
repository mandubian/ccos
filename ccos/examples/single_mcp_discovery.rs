//! Single MCP discovery helper
//!
//! Usage: cargo run --example single_mcp_discovery -- --server-url <url> --server-name <name> --hint <tool-or-description>
//!
//! This example introspects a single MCP server, finds the best-matching tool by name/description
//! and attempts to infer its output schema by calling it once with safe inputs. If the call
//! returns an error (for example "missing required parameter: owner"), the example will ask
//! the Arbiter LLM to propose plausible values for the missing parameters and retry.

use clap::Parser;
use std::error::Error;
use std::sync::Arc;

use ccos::CCOS;

use crate::single_mcp_discovery_impl::{run_discovery, Args};

mod single_mcp_discovery_impl {
    use super::*;
    use ccos::synthesis::mcp_introspector::{DiscoveredMCPTool, MCPIntrospector};
    use ccos::arbiter::ArbiterFactory;
    use ccos::synthesis::mcp_session::MCPSessionManager;
    use ccos::discovery::capability_matcher::{
        calculate_description_match_score_with_embedding,
        calculate_description_match_score,
    };
    use ccos::discovery::embedding_service::EmbeddingService;

    #[derive(Parser, Debug)]
    pub struct Args {
        /// MCP server base URL (http(s)://.../)
        #[arg(long)]
        pub server_url: String,

        /// Friendly server name (e.g. github/github-mcp)
        #[arg(long, default_value = "github")]
        pub server_name: String,

        /// Hint for tool to discover (tool name or description keyword)
        #[arg(long)]
        pub hint: String,

        /// Optional auth token (if not set, example will read env vars)
        #[arg(long)]
        pub token: Option<String>,

        /// Whether to be verbose
        #[arg(long, default_value_t = false)]
        pub verbose: bool,
    }

    pub async fn run_discovery(args: Args) -> Result<(), Box<dyn Error>> {
        // Create CCOS (used to access arbiter and marketplace if needed)
        let ccos = Arc::new(CCOS::new().await?);

        let _introspector = MCPIntrospector::new();

        eprintln!("🔍 Introspecting MCP server: {} ({})", args.server_name, args.server_url);

        let mut auth_headers = args.token.map(|t| {
            let mut m = std::collections::HashMap::new();
            m.insert("Authorization".to_string(), format!("Bearer {}", t));
            m
        });

        // If no token provided on CLI, fall back to environment variables commonly used
        // for MCP authentication so the example behaves like the discovery engine.
        if auth_headers.is_none() {
            let candidates = vec![
                "GITHUB_MCP_TOKEN",
                "MCP_AUTH_TOKEN",
                "GITHUB_PAT",
                "GITHUB_TOKEN",
            ];
            for cand in candidates {
                if let Ok(tok) = std::env::var(cand) {
                    if !tok.is_empty() {
                        let mut m = std::collections::HashMap::new();
                        m.insert("Authorization".to_string(), format!("Bearer {}", tok));
                        auth_headers = Some(m);
                        eprintln!("     ✓ Using auth token from env var: {}", cand);
                        break;
                    }
                }
            }
        }

        // Resolve overrides from `capabilities/mcp/overrides.json` if present
        // If the user provided a friendly `server_name`, check curated overrides for a remote URL.
        // If not found, try to derive a canonical server name from the `server_url` and try again.
        let server_name_candidate = args.server_name.clone();
        let mut server_url = match resolve_server_url_from_overrides(&server_name_candidate) {
            Some(url) => {
                eprintln!("  → Using curated override remote URL: {}", url);
                url
            }
            None => args.server_url.clone(),
        };

        // If args.server_url looks like a repository or non-mcp URL and we didn't map it above,
        // try deriving a name from the URL (owner/name), and consult overrides again.
        if server_url.starts_with("https://github.com") || server_url.starts_with("http://github.com") {
            if let Some(derive) = derive_server_name_from_repo_url(&server_url) {
                if let Some(url2) = resolve_server_url_from_overrides(&derive) {
                    eprintln!("  → Found curated mapping for derived server_name '{}': {}", derive, url2);
                    server_url = url2;
                }
            }
        }

        // Create a session manager and initialize a session once — reuse it for tools/list and tools/call
        let session_manager = MCPSessionManager::new(auth_headers.clone());
        let client_info = ccos::synthesis::mcp_session::MCPServerInfo {
            name: "ccos-single-discovery".to_string(),
            version: "1.0.0".to_string(),
        };

        let session = match session_manager.initialize_session(&server_url, &client_info).await {
            Ok(s) => s,
            Err(e) => {
                eprintln!("✗ MCP initialization failed: {}", e);
                return Err(Box::new(e));
            }
        };

        // Call tools/list on the same session so server can use stateful session
        let tools_resp = session_manager
            .make_request(&session, "tools/list", serde_json::json!({}))
            .await?;

        // Build introspection result using `MCPIntrospector` helpers (we parse tools manually so we can keep session open)
        let tools_array = tools_resp
            .get("result")
            .and_then(|r| r.get("tools"))
            .and_then(|t| t.as_array())
            .ok_or_else(|| {
                Box::<dyn Error>::from("Invalid MCP tools/list response — no tools array")
            })?;

        let mut discovered_tools: Vec<DiscoveredMCPTool> = Vec::new();
        for tool_json in tools_array {
            // Parse name + description
            let tool_name = tool_json
                .get("name")
                .and_then(|n| n.as_str())
                .unwrap_or("unknown")
                .to_string();

            let description = tool_json
                .get("description")
                .and_then(|d| d.as_str())
                .map(|s| s.to_string());

            let input_schema_json = tool_json.get("inputSchema").cloned();
            let input_schema = if let Some(schema_json) = &input_schema_json {
                match MCPIntrospector::type_expr_from_json_schema(schema_json) {
                    Ok(t) => Some(t),
                    Err(_) => None,
                }
            } else {
                None
            };

            discovered_tools.push(DiscoveredMCPTool {
                tool_name,
                description,
                input_schema,
                output_schema: None,
                input_schema_json,
            });
        }

        let introspection = ccos::synthesis::mcp_introspector::MCPIntrospectionResult {
            server_url: server_url.clone(),
            server_name: args.server_name.clone(),
            protocol_version: session.protocol_version.clone(),
            tools: discovered_tools,
        };

        if introspection.tools.is_empty() {
            eprintln!("No tools discovered on server");
            return Ok(());
        }

        // Find best matching tool by hint using the same semantic matcher as discovery engine.
        // We treat the hint as the "need_rationale" and compare against each tool's
        // description + name, optionally using embeddings when configured.
        let mut best_tool: Option<DiscoveredMCPTool> = None;
        let mut best_score: f64 = f64::MIN;

        // Detect embedding service from environment (if available), otherwise fall back to keywords.
        let mut embedding_service = EmbeddingService::from_env();

        for tool in &introspection.tools {
            let desc = tool.description.clone().unwrap_or_default();
            let name = tool.tool_name.clone();

            let score = if let Some(ref mut emb_svc) = embedding_service {
                // Embedding + keyword hybrid scoring (synchronous helper that may consult embeddings).
                calculate_description_match_score_with_embedding(
                    &args.hint,
                    &desc,
                    &name,
                    Some(emb_svc),
                )
            } else {
                // Pure keyword-based scoring.
                calculate_description_match_score(&args.hint, &desc, &name)
            };

            if score > best_score {
                best_score = score;
                best_tool = Some(tool.clone());
            }
        }

        // Simple tie-breaker: if the hint contains certain keywords, prefer
        // tools whose name contains the same keyword when scores are close.
        if let Some(ref current) = best_tool {
            let hint_l = args.hint.to_lowercase();
            // Only apply this heuristic when we actually have a reasonably
            // good semantic match already.
            if best_score > 0.0 {
                let mut candidate = current.clone();
                let mut candidate_score = best_score;

                for tool in &introspection.tools {
                    let name_l = tool.tool_name.to_lowercase();

                    // Prefer tools with "issues" in the name when the hint
                    // mentions issues. This remains provider-agnostic but
                    // helps disambiguate closely related tools.
                    if hint_l.contains("issues") && name_l.contains("issues") {
                        // Require that the semantic score is reasonably close
                        // to the best score to avoid wild jumps.
                        let desc = tool.description.clone().unwrap_or_default();
                        let score = if let Some(ref mut emb_svc) = embedding_service {
                            calculate_description_match_score_with_embedding(
                                &args.hint,
                                &desc,
                                &tool.tool_name,
                                Some(emb_svc),
                            )
                        } else {
                            calculate_description_match_score(&args.hint, &desc, &tool.tool_name)
                        };

                        if score >= candidate_score - 0.1 && score > candidate_score {
                            candidate = tool.clone();
                            candidate_score = score;
                        }
                    }
                }

                best_tool = Some(candidate);
                best_score = candidate_score;
            }
        }

        let tool = match best_tool {
            Some(t) => {
                eprintln!("→ Selected tool: {} (score {:.3})", t.tool_name, best_score);
                t
            }
            None => {
                eprintln!("No matching tool found for hint '{}', listing tools:", args.hint);
                for t in &introspection.tools {
                    eprintln!(" - {}: {}", t.tool_name, t.description.as_deref().unwrap_or(""));
                }
                return Ok(());
            }
        };
 

        // Try to infer output schema by calling tool once with safe inputs
        // Try to introspect output by calling the tool once with safe inputs using the active session
        // (we reuse the `session_manager` and `session` created earlier to avoid session-mismatch errors)
        let test_inputs = build_safe_test_inputs_from_schema(tool.input_schema_json.as_ref(), false);

        // Call the tool via the open session
        let call_res = session_manager
            .make_request(&session, "tools/call", serde_json::json!({"name": tool.tool_name, "arguments": test_inputs}))
            .await;

        let (schema_opt, sample_opt) = match call_res {
            Ok(r) => {
                let sample_snippet = serde_json::to_string_pretty(&r.get("result").cloned().unwrap_or(serde_json::Value::Null)).ok();
                // Only accept result as positive if the body doesn't look like an error
                let is_error = sample_snippet
                    .as_ref()
                    .map(|s| {
                        let s_l = s.to_lowercase();
                        s_l.contains("missing required")
                            || s_l.contains("required parameter")
                            || s_l.contains("error")
                            || s_l.contains("unauthorized")
                            || s_l.contains("forbidden")
                    })
                    .unwrap_or(true);

                if is_error {
                    (None::<rtfs::ast::TypeExpr>, sample_snippet)
                } else {
                    (None::<rtfs::ast::TypeExpr>, sample_snippet)
                }
            }
            Err(e) => {
                eprintln!("⚠️ Failed to call tool for output schema introspection: {}", e);
                (None, None)
            }
        };

        if schema_opt.is_some() {
            eprintln!("✅ Inferred output schema for {}", tool.tool_name);
            if let Some(sample) = sample_opt {
                eprintln!("Sample output (first lines):\n{}", sample.lines().take(8).collect::<Vec<_>>().join("\n"));
            }
            return Ok(());
        }

        // If introspection returned no schema but provided a sample with an error-like message,
        // attempt to synthesize plausible inputs heuristically and retry the call.
        if let Some(sample) = sample_opt {
            eprintln!("⚠️ Introspection produced sample/error: {}", sample.lines().next().unwrap_or(""));

            // Try to extract missing parameter names from the sample text (e.g. "missing required parameter: owner")
            // Try to extract missing parameter names from the sample text (generic, provider-agnostic)
            let re = regex::Regex::new(r"missing required (?:field|parameter)[: ]+([a-zA-Z0-9_-]+)").ok();
            let mut suggested_args = serde_json::Map::new();

            if let Some(r) = &re {
                for cap in r.captures_iter(&sample.to_lowercase()) {
                    if let Some(name) = cap.get(1) {
                        let key = name.as_str();
                        // for now, use a generic placeholder and let the Arbiter refine it later
                        let val = serde_json::Value::String("example".to_string());
                        suggested_args.insert(key.to_string(), val);
                    }
                }
            }

            // If we couldn't find missing parameter names, fall back to schema hints if present
            if suggested_args.is_empty() {
                if let Some(input_schema_json) = &tool.input_schema_json {
                    if let Some(props) = input_schema_json.get("properties").and_then(|p| p.as_object()) {
                        let required: Vec<String> = input_schema_json
                            .get("required")
                            .and_then(|r| r.as_array())
                            .map(|arr| arr.iter().filter_map(|s| s.as_str()).map(|s| s.to_string()).collect())
                            .unwrap_or_default();

                        for req in required.iter() {
                            // Try enum default or a generic placeholder; do not assume provider-specific semantics
                            let val = if let Some(prop_schema) = props.get(req) {
                                if let Some(enum_vals) = prop_schema.get("enum").and_then(|e| e.as_array()) {
                                    if let Some(first) = enum_vals.first().and_then(|v| v.as_str()) {
                                        serde_json::Value::String(first.to_string())
                                    } else {
                                        serde_json::Value::String("example".to_string())
                                    }
                                } else {
                                    serde_json::Value::String("example".to_string())
                                }
                            } else {
                                serde_json::Value::String("example".to_string())
                            };
                            suggested_args.insert(req.clone(), val);
                        }
                    }
                }
            }

            if suggested_args.is_empty() {
                eprintln!("✗ Could not synthesize any plausible inputs to retry");
            } else {
                    // If we found some inputs but still have missing required fields (like 'repo'),
                    // use the Arbiter (LLM) to suggest the missing values.
                    // note: missing_keys can be useful for logging & future multi-key suggestions

                    // Identify missing required inputs that we haven't filled yet
                    let mut missing: Vec<String> = Vec::new();
                    if let Some(input_schema_json) = &tool.input_schema_json {
                        if let Some(required_arr) = input_schema_json.get("required") {
                            if let Some(required_list) = required_arr.as_array() {
                                for r in required_list {
                                    if let Some(req_name) = r.as_str() {
                                        if !suggested_args.contains_key(req_name) {
                                            missing.push(req_name.to_string());
                                        }
                                    }
                                }
                            }
                        }
                    }

                                        if !missing.is_empty() {

                                            // Create an arbiter from env config (consistent with other examples)

                                            let intent_graph = ccos.get_intent_graph();

                                            let marketplace = ccos.get_capability_marketplace();

                                            match ArbiterFactory::create_arbiter_from_env(intent_graph, Some(marketplace)).await {

                                                Ok(arbiter) => {

                                                    // Ask the arbiter once for a coherent set of arguments

                                                    // for all required parameters, using the schema and the

                                                    // current suggested_args as context.

                                                    let schema_snippet = tool

                                                        .input_schema_json

                                                        .clone()

                                                        .unwrap_or(serde_json::Value::Null);

                    

                                                    let prompt = format!(

                                                        "You are an expert API tester. Your goal is to provide realistic arguments to successfully call an API tool for introspection purposes.\n\nI tried to call the tool '{tool_name}' with these inputs:\n`{inputs}`\n\nThe server responded with this error:\n`{error}`\n\nHere is the tool's input schema:\n`{schema}`\n\nBased on the parameter names, their descriptions in the schema, and the error message, generate a single JSON object with realistic, valid-looking values for all required parameters. The values should be for publicly known and accessible resources to maximize the chance of a successful API call. For example, for a parameter named 'project_id', use a name of a well-known open-source project. Do not use generic placeholders like 'test', 'dummy', or 'example'.\n\nRespond with ONLY the JSON object.",

                                                        tool_name = tool.tool_name,

                                                        inputs = serde_json::Value::Object(suggested_args.clone()),

                                                        error = sample,

                                                        schema = serde_json::to_string_pretty(&schema_snippet).unwrap_or_default(),

                                                    );

                    

                                                    match arbiter.process_natural_language(&prompt, None).await {

                                                        Ok(exec) => {

                                                            let suggestion_text = format!("{}", exec.value);

                                                            // Try to parse the arbiter response directly as JSON

                                                            match serde_json::from_str::<serde_json::Value>(&suggestion_text) {

                                                                Ok(serde_json::Value::Object(obj)) => {

                                                                    for (k, v) in obj.iter() {

                                                                        eprintln!("→ Arbiter suggested {} -> {}", k, v);

                                                                        suggested_args.insert(k.clone(), v.clone());

                                                                    }

                                                                }

                                                                Ok(_) | Err(_) => {

                                                                    eprintln!("⚠️ Arbiter did not return a clean JSON object, attempting fallback extraction");

                                                                    // Fallback: try to extract each missing key from the text response

                                                                    for key in missing.iter() {

                                                                        if let Some(val) = extract_suggestion_from_text(&suggestion_text, key) {

                                                                            eprintln!("→ Arbiter (fallback) suggested {} -> {}", key, val);

                                                                            suggested_args.insert(key.clone(), serde_json::Value::String(val));

                                                                        }

                                                                    }

                                                                }

                                                            }

                                                        }

                                                        Err(e) => {

                                                            eprintln!("⚠️ Arbiter call failed when suggesting arguments: {}", e);

                                                        }

                                                    }

                                                }

                                                Err(e) => {

                                                    eprintln!("⚠️ Failed to create Arbiter for LLM suggestions: {}", e);

                                                }

                                            }

                                        }

                    

                                                                    // Final pass to replace any remaining placeholder values with more realistic examples.

                    

                                                                    // This is a safeguard against the LLM returning generic values like "dummy" or "example".

                    

                                                                    let mut owner_from_repo = None;

                    

                                                                    let mut repo_name_only = None;

                    

                                                    

                    

                                                                    for (key, val) in suggested_args.iter_mut() {

                    

                                                                        if let Some(s_val) = val.as_str() {

                    

                                                                            if s_val.to_lowercase() == "dummy" || s_val.to_lowercase() == "example" || s_val.is_empty() {

                    

                                                                                if key.to_lowercase().contains("repo") {

                    

                                                                                    let full_repo = "microsoft/vscode";

                    

                                                                                    *val = full_repo.into();

                    

                                                                                    if let Some(owner) = full_repo.split('/').next() {

                    

                                                                                        owner_from_repo = Some(owner.to_string());

                    

                                                                                    }

                    

                                                                                    if let Some(repo_name) = full_repo.split('/').last() {

                    

                                                                                        repo_name_only = Some(repo_name.to_string());

                    

                                                                                    }

                    

                                                                                    eprintln!("  → Replacing placeholder for '{}' with '{}'", key, val);

                    

                                                                                } else if key.to_lowercase().contains("owner") {

                    

                                                                                    *val = "microsoft".into();

                    

                                                                                    eprintln!("  → Replacing placeholder for '{}' with '{}'", key, val);

                    

                                                                                }

                    

                                                                            }

                    

                                                                        }

                    

                                                                    }

                    

                                                    

                    

                                                                    // If we derived an owner from a full repo name, ensure it's consistent

                    

                                                                    if let Some(owner) = owner_from_repo {

                    

                                                                        if let Some(owner_val) = suggested_args.get_mut("owner") {

                    

                                                                            *owner_val = owner.into();

                    

                                                                            eprintln!("  → Aligning owner with repo suggestion.");

                    

                                                                        }

                    

                                                                    }

                    

                                                                    if let Some(repo_name) = repo_name_only {

                    

                                                                        if let Some(repo_val) = suggested_args.get_mut("repo") {

                    

                                                                            *repo_val = repo_name.into();

                    

                                                                            eprintln!("  → Aligning repo name with repo suggestion.");

                    

                                                                        }

                    

                                                                    }

                    

                                                    

                    

                                                                    eprintln!("→ Synthesized inputs for retry: {}", serde_json::Value::Object(suggested_args.clone()).to_string());

                    

                                                    

                    

                                                                    // Retry the tool call using the same open session (session_manager)/recreate only if exp

                    

                                                                    let client_info = ccos::synthesis::mcp_session::MCPServerInfo {
                    name: "ccos-single-discovery".to_string(),
                    version: "1.0.0".to_string(),
                };

                // Try up to two attempts (reinit session if first fails)
                let mut attempt = 0usize;
                let mut last_err: Option<anyhow::Error> = None;
                while attempt < 2 {
                    attempt += 1;
                    // Reuse session - re-init if server requires it.
                    let fresh_session = match session_manager.initialize_session(&server_url, &client_info).await {
                        Ok(s) => s,
                        Err(e) => {
                            eprintln!("Failed to reinitialize session on attempt {}: {}", attempt, e);
                            last_err = Some(anyhow::Error::new(e));
                            continue;
                        }
                    };

                    let call_res = session_manager
                        .make_request(
                            &fresh_session,
                            "tools/call",
                            serde_json::json!({"name": tool.tool_name, "arguments": serde_json::Value::Object(suggested_args.clone())}),
                        )
                        .await;

                    let _ = session_manager.terminate_session(&fresh_session).await;

                    match call_res {
                        Ok(res) => {
                            eprintln!("✅ Retry succeeded: {}", serde_json::to_string_pretty(&res).unwrap_or_default());
                            last_err = None;
                            break;
                        }
                        Err(e) => {
                            eprintln!("Attempt {} failed: {}", attempt, e);
                            last_err = Some(anyhow::Error::new(e));
                        }
                    }
                }

                if last_err.is_some() {
                    eprintln!("✗ All retry attempts failed");
                }
            }
        } else {
            eprintln!("⚠️ No sample or schema could be obtained from introspection and no error snippet provided.");
        }

        Ok(())
    }

    fn resolve_server_url_from_overrides(server_name: &str) -> Option<String> {
        // Try to load curated overrides from 'capabilities/mcp/overrides.json'
        let root = std::env::current_dir().unwrap_or_else(|_| std::path::PathBuf::from("."));
        let overrides_path = if root.ends_with("rtfs_compiler") {
            root.parent().unwrap_or(&root).join("capabilities/mcp/overrides.json")
        } else {
            root.join("capabilities/mcp/overrides.json")
        };

        if !overrides_path.exists() {
            return None;
        }

        let content = std::fs::read_to_string(&overrides_path).ok()?;
        let parsed: serde_json::Value = serde_json::from_str(&content).ok()?;
        let entries = parsed.get("entries")?.as_array()?;

        for entry in entries {
            // Check server name equality first
            if let Some(server) = entry.get("server") {
                if let Some(name) = server.get("name").and_then(|n| n.as_str()) {
                    if name == server_name {
                        // Get best HTTP remote
                        if let Some(remotes) = server.get("remotes").and_then(|r| r.as_array()) {
                            for remote in remotes {
                                if let Some(url) = remote.get("url").and_then(|u| u.as_str()) {
                                    if url.starts_with("http://") || url.starts_with("https://") {
                                        return Some(url.to_string());
                                    }
                                }
                            }
                        }
                    }
                }

                // Fallback: see if `matches` patterns include server_name
                if let Some(matches) = entry.get("matches").and_then(|m| m.as_array()) {
                    for pat in matches {
                        if let Some(p) = pat.as_str() {
                            if simple_pattern_match(p, server_name) {
                                if let Some(remotes) = server.get("remotes").and_then(|r| r.as_array()) {
                                    for remote in remotes {
                                        if let Some(url) = remote.get("url").and_then(|u| u.as_str()) {
                                            if url.starts_with("http://") || url.starts_with("https://") {
                                                return Some(url.to_string());
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }

        None
    }

    fn simple_pattern_match(pattern: &str, text: &str) -> bool {
        // Use the same matching rules as discovery::engine::pattern_match:
        // - exact match
        // - suffix '.*' -> allow either exact or namespaced (prefix + '.')
        // - suffix '*' -> prefix match
        // - prefix '*' -> suffix match
        // - '*' anywhere -> anchored contains match (starts & ends)
        let pattern_norm = pattern.to_ascii_lowercase();
        let text_norm = text.to_ascii_lowercase();

        if pattern_norm == text_norm {
            return true;
        }
        if pattern_norm.ends_with(".*") {
            let namespace = &pattern_norm[..pattern_norm.len() - 2];
            return text_norm == namespace || text_norm.starts_with(&format!("{}.", namespace));
        }
        if pattern_norm.ends_with('*') {
            let prefix = &pattern_norm[..pattern_norm.len() - 1];
            return text_norm.starts_with(prefix);
        }
        if pattern_norm.starts_with('*') {
            let suffix = &pattern_norm[1..];
            return text_norm.ends_with(suffix);
        }
        if pattern_norm.contains('*') {
            let parts: Vec<&str> = pattern_norm.split('*').collect();
            if parts.len() == 2 {
                return text_norm.starts_with(parts[0]) && text_norm.ends_with(parts[1]);
            }
        }
        text_norm.contains(&pattern_norm)
    }

    use ccos::examples_common::discovery_utils::derive_server_name_from_repo_url;

    fn build_safe_test_inputs_from_schema(schema: Option<&serde_json::Value>, plausible: bool) -> serde_json::Value {
        let mut inputs = serde_json::Map::new();

        if let Some(schema) = schema {
            if let Some(properties) = schema.get("properties").and_then(|p| p.as_object()) {
                let required: Vec<String> = schema
                    .get("required")
                    .and_then(|r| r.as_array())
                    .map(|arr| arr.iter().filter_map(|s| s.as_str()).map(|s| s.to_string()).collect())
                    .unwrap_or_default();

                for (key, prop_schema) in properties {
                    if required.contains(key) {
                        let val = generate_safe_default_value(prop_schema, key, plausible);
                        inputs.insert(key.clone(), val);
                    }
                }
            }
        }

        serde_json::Value::Object(inputs)
    }

    fn generate_safe_default_value(schema: &serde_json::Value, _name: &str, _plausible: bool) -> serde_json::Value {
        if let Some(type_str) = schema.get("type").and_then(|t| t.as_str()) {
            match type_str {
                "string" => {
                    if let Some(enum_vals) = schema.get("enum").and_then(|e| e.as_array()) {
                        if let Some(first) = enum_vals.first().and_then(|v| v.as_str()) {
                            return serde_json::Value::String(first.to_string());
                        }
                    }
                    // For generic examples, avoid provider-specific heuristics. If an enum
                    // is not provided, fall back to an empty string even when `plausible`
                    // is true; provider-specific examples should override this behavior
                    // in their own code.
                    serde_json::Value::String(String::new())
                }
                "integer" | "number" => {
                    if let Some(min) = schema.get("minimum").and_then(|m| m.as_i64()) {
                        serde_json::Value::Number(serde_json::Number::from(min))
                    } else {
                        serde_json::Value::Number(serde_json::Number::from(0))
                    }
                }
                "boolean" => serde_json::Value::Bool(false),
                "array" => serde_json::Value::Array(vec![]),
                "object" => serde_json::Value::Object(serde_json::Map::new()),
                _ => serde_json::Value::Null,
            }
        } else {
            serde_json::Value::Null
        }
    }

    use ccos::examples_common::discovery_utils::extract_suggestion_from_text;

    // Note: we intentionally avoid instantiating or implementing `ArbiterEngine` here.
    // Examples that need to call an LLM should use the public arbiter factory APIs
    // from the crate root (`ccos::arbiter`) or call into a running CCOS instance.
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    let args = Args::parse();
    run_discovery(args).await
}
