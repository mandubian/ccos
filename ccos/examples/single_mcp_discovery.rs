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
use rtfs::config::types::AgentConfig;

use crate::single_mcp_discovery_impl::{run_discovery, Args};

mod single_mcp_discovery_impl {
    use super::*;
    use ccos::synthesis::mcp_introspector::MCPIntrospector;
    use ccos::mcp::types::DiscoveredMCPTool;
    use ccos::mcp::registry::MCPRegistryClient;
    use ccos::mcp::discovery_session::{MCPSessionManager, MCPServerInfo};
    use rtfs::runtime::values::Value as RtfsValue;
    use serde_json;
    use std::path::Path;

    /// Convert RTFS Value to serde_json::Value by extracting the actual value
    fn rtfs_value_to_json(rtfs_value: &RtfsValue) -> serde_json::Value {
        match rtfs_value {
            RtfsValue::Nil => serde_json::Value::Null,
            RtfsValue::Boolean(b) => serde_json::Value::Bool(*b),
            RtfsValue::Integer(i) => serde_json::Value::Number(serde_json::Number::from(*i)),
            RtfsValue::Float(f) => serde_json::Number::from_f64(*f)
                .map(serde_json::Value::Number)
                .unwrap_or(serde_json::Value::Null),
            RtfsValue::String(s) => serde_json::Value::String(s.clone()),
            RtfsValue::Vector(vec) => {
                let json_array: Vec<serde_json::Value> =
                    vec.iter().map(rtfs_value_to_json).collect();
                serde_json::Value::Array(json_array)
            }
            RtfsValue::List(list) => {
                // Treat List same as Vector for JSON serialization
                let json_array: Vec<serde_json::Value> =
                    list.iter().map(rtfs_value_to_json).collect();
                serde_json::Value::Array(json_array)
            }
            RtfsValue::Map(map) => {
                let mut json_obj = serde_json::Map::new();
                for (key, value) in map {
                    let key_str = match key {
                        rtfs::ast::MapKey::String(s) => s.clone(),
                        rtfs::ast::MapKey::Keyword(k) => k.0.clone(),
                        rtfs::ast::MapKey::Integer(i) => i.to_string(),
                    };
                    json_obj.insert(key_str, rtfs_value_to_json(value));
                }
                serde_json::Value::Object(json_obj)
            }
            RtfsValue::Keyword(k) => serde_json::Value::String(format!(":{}", k.0)),
            RtfsValue::Symbol(s) => serde_json::Value::String(s.0.clone()),
            RtfsValue::Timestamp(ts) => serde_json::Value::String(format!("@{}", ts)),
            RtfsValue::Uuid(uuid) => serde_json::Value::String(format!("@{}", uuid)),
            RtfsValue::ResourceHandle(handle) => serde_json::Value::String(format!("@{}", handle)),
            RtfsValue::Function(_) => serde_json::Value::Null, // Functions can't be serialized
            RtfsValue::FunctionPlaceholder(_) => serde_json::Value::Null, // Function placeholders can't be serialized
            RtfsValue::Error(e) => serde_json::Value::String(format!("error: {}", e.message)), // Serialize error as string
        }
    }

    #[derive(Parser, Debug)]
    pub struct Args {
        /// MCP server base URL (http(s)://.../)
        #[arg(long)]
        pub server_url: Option<String>,

        /// Friendly server name (e.g. github/github-mcp)
        #[arg(long)]
        pub server_name: Option<String>,

        /// Hint for tool to discover (tool name or description keyword)
        #[arg(long)]
        pub hint: String,

        /// Optional: specific tool name to introspect
        #[arg(long)]
        pub tool_name: Option<String>,

        /// Optional auth token (if not set, example will read env vars)
        #[arg(long)]
        pub token: Option<String>,

        /// Where to save the generated capability RTFS file
        #[arg(long, default_value = "capabilities/discovered")]
        pub output_dir: String,

        /// Path to agent config file
        #[arg(long, default_value = "config/agent_config.toml")]
        pub config: String,

        /// Profile to use for LLM
        #[arg(long)]
        pub profile: Option<String>,
    }

    pub async fn run_discovery(args: Args) -> Result<(), Box<dyn Error>> {
        let agent_config = load_agent_config(&args.config)?;
        apply_llm_profile(&agent_config, args.profile.as_deref())?;

        // Create CCOS (used to access arbiter and marketplace if needed)
        let ccos = Arc::new(
            CCOS::new_with_agent_config_and_configs_and_debug_callback(
                Default::default(),
                None,
                Some(agent_config),
                None,
            )
            .await?,
        );

        eprintln!(
            "🔍 Introspecting MCP server: {:?} ({})",
            args.server_name.as_deref().unwrap_or(""),
            args.server_url.as_deref().unwrap_or("")
        );

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
        let mut server_url = if let Some(ref server_name_candidate) = args.server_name {
            match resolve_server_url_from_overrides(server_name_candidate) {
                Some(url) => {
                    eprintln!("  → Using curated override remote URL: {}", url);
                    url
                }
                None => {
                    if let Some(url) = args.server_url.clone() {
                        url
                    } else {
                        // Try registry search
                        eprintln!(
                            "🔍 Searching MCP Registry for '{}'...",
                            server_name_candidate
                        );
                        let client = MCPRegistryClient::new();
                        let mut found_url = None;
                        match client.search_servers(server_name_candidate).await {
                            Ok(servers) => {
                                for server in servers {
                                    if let Some(remotes) = &server.remotes {
                                        if let Some(url) =
                                            MCPRegistryClient::select_best_remote_url(remotes)
                                        {
                                            eprintln!(
                                                "  → Found server in registry: {} ({})",
                                                server.name, url
                                            );
                                            found_url = Some(url);
                                            break;
                                        }
                                    }
                                }
                            }
                            Err(e) => {
                                eprintln!("⚠️ Registry search failed: {}", e);
                            }
                        }

                        if let Some(url) = found_url {
                            url
                        } else {
                            eprintln!("✗ No server URL provided, no override found, and registry search failed for server '{:?}'", server_name_candidate);
                            return Err(Box::<dyn Error>::from("Missing server URL"));
                        }
                    }
                }
            }
        } else {
            if let Some(url) = args.server_url.clone() {
                url
            } else {
                // First, try to resolve overrides using the hint as a server name
                if let Some(url) = resolve_server_url_from_overrides(&args.hint) {
                    eprintln!("  → Using curated override remote URL from hint: {}", url);
                    url
                } else {
                    // Try using hint as query
                    eprintln!(
                        "🔍 No server specified. Searching MCP Registry for hint '{}'...",
                        args.hint
                    );
                    let client = MCPRegistryClient::new();
                    let mut found_url = None;
                    match client.search_servers(&args.hint).await {
                        Ok(servers) => {
                            for server in servers {
                                if let Some(remotes) = &server.remotes {
                                    if let Some(url) =
                                        MCPRegistryClient::select_best_remote_url(remotes)
                                    {
                                        eprintln!(
                                            "  → Found server in registry: {} ({})",
                                            server.name, url
                                        );
                                        found_url = Some(url);
                                        break;
                                    }
                                }
                            }
                        }
                        Err(e) => {
                            eprintln!("⚠️ Registry search failed: {}", e);
                        }
                    }

                    if let Some(url) = found_url {
                        url
                    } else {
                        eprintln!("✗ No server URL provided and no server name provided");
                        return Err(Box::<dyn Error>::from("Missing server URL"));
                    }
                }
            }
        };

        // If the provided URL looks like a repository, try to derive a better server name and check overrides again.
        if server_url.starts_with("https://github.com")
            || server_url.starts_with("http://github.com")
        {
            if let Some(derive) = derive_server_name_from_repo_url(&server_url) {
                if let Some(url2) = resolve_server_url_from_overrides(&derive) {
                    eprintln!(
                        "  → Found curated mapping for derived server_name '{}': {}",
                        derive, url2
                    );
                    server_url = url2;
                }
            }
        }

        // Create a session manager and initialize a session once — reuse it for tools/list and tools/call
        let session_manager = MCPSessionManager::new(auth_headers.clone());
        let client_info = MCPServerInfo {
            name: "ccos-single-discovery".to_string(),
            version: "1.0.0".to_string(),
        };

        let session = match session_manager
            .initialize_session(&server_url, &client_info)
            .await
        {
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

            // Convert JSON schema to RTFS TypeExpr if available
            let input_schema = if let Some(schema) = &input_schema_json {
                MCPIntrospector::type_expr_from_json_schema(schema).ok()
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

        // If a specific tool name is provided, filter the list
        if let Some(target_tool_name) = &args.tool_name {
            discovered_tools.retain(|tool| &tool.tool_name == target_tool_name);
            if discovered_tools.is_empty() {
                eprintln!("✗ Tool '{}' not found on the server.", target_tool_name);
                return Ok(());
            }
        }

        let inspector = MCPIntrospector::new();

        let introspection = ccos::synthesis::mcp_introspector::MCPIntrospectionResult {
            server_url: server_url.clone(),
            server_name: args.server_name.clone().unwrap_or_default(),
            protocol_version: session.protocol_version.clone(),
            tools: discovered_tools,
        };

        if introspection.tools.is_empty() {
            eprintln!("No tools discovered on server");
            return Ok(());
        }

        eprintln!("📋 Discovered {} tools:", introspection.tools.len());
        for tool in &introspection.tools {
            eprintln!("  - {}", tool.tool_name);
        }

        // Use the Arbiter to select the best tool and proactively extract arguments.
        let (tool, extracted_args) = if let Some((tool_name, args)) =
            select_tool_and_extract_args_with_arbiter(&ccos, &introspection.tools, &args.hint).await
        {
            // Try exact match first
            if let Some(found_tool) = introspection
                .tools
                .iter()
                .find(|t| t.tool_name == tool_name)
            {
                (found_tool.clone(), args)
            } else {
                // Try fuzzy matching - find tool name that contains the selected name or vice versa
                let found_tool = introspection
                    .tools
                    .iter()
                    .find(|t| t.tool_name.contains(&tool_name) || tool_name.contains(&t.tool_name));
                if let Some(found_tool) = found_tool {
                    eprintln!(
                        "⚠️ Arbiter selected '{}', using fuzzy match: '{}'",
                        tool_name, found_tool.tool_name
                    );
                    (found_tool.clone(), args)
                } else {
                    eprintln!("⚠️ Arbiter selected tool '{}' but it was not found in the introspection results.", tool_name);
                    eprintln!(
                        "   Available tools: {}",
                        introspection
                            .tools
                            .iter()
                            .map(|t| t.tool_name.as_str())
                            .collect::<Vec<_>>()
                            .join(", ")
                    );
                    return Ok(());
                }
            }
        } else {
            eprintln!(
                "⚠️ Arbiter could not select a tool for the hint '{}'.",
                args.hint
            );
            // As a fallback, we could use the keyword-based search here, but for this example, we'll just exit.
            return Ok(());
        };

        // Try to infer output schema by calling tool once with safe inputs
        // (we reuse the `session_manager` and `session` created earlier to avoid session-mismatch errors)
        let mut test_inputs =
            build_safe_test_inputs_from_schema(tool.input_schema_json.as_ref(), false);

        // Map extracted parameter names to match the tool's schema parameter names
        let mut mapped_args =
            map_parameters_to_schema(&extracted_args, tool.input_schema_json.as_ref());
        eprintln!("📋 Parameter mapping:");
        eprintln!(
            "  Extracted: {:?}",
            extracted_args.keys().collect::<Vec<_>>()
        );
        eprintln!("  Mapped: {:?}", mapped_args.keys().collect::<Vec<_>>());

        // Merge the mapped arguments into the test inputs.
        eprintln!("📥 Merging extracted parameters into test inputs...");
        eprintln!(
            "  Before merge: {}",
            serde_json::to_string_pretty(&test_inputs).unwrap_or_default()
        );
        if let Some(obj) = test_inputs.as_object_mut() {
            obj.append(&mut mapped_args);
        } else {
            eprintln!("⚠️ test_inputs is not an object, cannot merge parameters");
        }
        eprintln!(
            "  After merge: {}",
            serde_json::to_string_pretty(&test_inputs).unwrap_or_default()
        );

        // To keep sample output small, ask for just one result if a common pagination
        // parameter is supported by the tool's input schema.
        if let Some(obj) = test_inputs.as_object_mut() {
            if let Some(schema) = tool.input_schema_json.as_ref() {
                if let Some(properties) = schema.get("properties").and_then(|p| p.as_object()) {
                    const PAGINATION_CANDIDATES: &[&str] =
                        &["perPage", "limit", "count", "pageSize", "maxItems"];
                    for (key, prop_schema) in properties {
                        if PAGINATION_CANDIDATES.contains(&key.as_str()) {
                            if let Some(type_str) = prop_schema.get("type").and_then(|t| t.as_str())
                            {
                                if type_str == "integer" || type_str == "number" {
                                    if !obj.contains_key(key) {
                                        obj.insert(key.clone(), serde_json::json!(1));
                                        eprintln!("  → Found pagination parameter '{}', setting to 1 to limit output.", key);
                                    }
                                    break;
                                }
                            }
                        }
                    }
                }
            }
        }

        // Ensure required parameters aren't missing before we actually call the tool
        if let Some(obj) = test_inputs.as_object() {
            // Check against schema required fields if available
            let required_keys: Vec<String> = tool
                .input_schema_json
                .as_ref()
                .and_then(|schema| schema.get("required"))
                .and_then(|r| r.as_array())
                .map(|arr| {
                    arr.iter()
                        .filter_map(|v| v.as_str())
                        .map(|s| s.to_string())
                        .collect()
                })
                .unwrap_or_default();

            let missing: Vec<&str> = required_keys
                .iter()
                .filter(|key| {
                    obj.get(*key)
                        .map(|value| {
                            // Check if value is null or empty string
                            value.is_null()
                                || value.as_str().map(|s| s.trim().is_empty()).unwrap_or(false)
                        })
                        .unwrap_or(true)
                })
                .map(|s| s.as_str())
                .collect();

            if !missing.is_empty() {
                eprintln!(
                    "⚠️ Tool '{}' requires {} before it can be called. Please extend the hint to include these parameters.",
                    tool.tool_name,
                    missing.join(", ")
                );
                return Ok(());
            }
        }

        // Call the tool via the open session
        eprintln!("🔧 Calling tool '{}' with arguments:", tool.tool_name);
        eprintln!(
            "{}",
            serde_json::to_string_pretty(&test_inputs).unwrap_or_default()
        );
        let call_res = session_manager
            .make_request(
                &session,
                "tools/call",
                serde_json::json!({"name": tool.tool_name, "arguments": test_inputs}),
            )
            .await;

        let (schema_opt, sample_opt) = match call_res {
            Ok(r) => {
                let result_val = r.get("result").cloned().unwrap_or(serde_json::Value::Null);
                let sample_snippet = serde_json::to_string_pretty(&result_val).ok();

                // Try to unwrap stringified JSON in "content"[0]["text"] if present
                // This is common for MCP tools that return complex data serialized as text
                let actual_val = if let Some(content) =
                    result_val.get("content").and_then(|c| c.as_array())
                {
                    if let Some(first) = content.first() {
                        if let Some(text) = first.get("text").and_then(|t| t.as_str()) {
                            if let Ok(inner_json) = serde_json::from_str::<serde_json::Value>(text)
                            {
                                eprintln!("  → Detected stringified JSON in output, inferring schema from inner content.");
                                inner_json
                            } else {
                                result_val.clone()
                            }
                        } else {
                            result_val.clone()
                        }
                    } else {
                        result_val.clone()
                    }
                } else {
                    result_val.clone()
                };

                let is_mcp_error = r.get("isError").and_then(|v| v.as_bool()).unwrap_or(false);

                let is_content_error = sample_snippet
                    .as_ref()
                    .map(|s| {
                        let s_l = s.to_lowercase();
                        s_l.contains("missing required")
                            || s_l.contains("required parameter")
                            || s_l.contains("unauthorized")
                            || s_l.contains("forbidden")
                    })
                    .unwrap_or(false);

                let is_error = is_mcp_error || is_content_error;

                if is_error {
                    (None::<rtfs::ast::TypeExpr>, sample_snippet)
                } else {
                    // Attempt to infer schema from the successful result (using unwrapped value)
                    if let Ok(inferred_schema) = inspector.infer_type_from_json_value(&actual_val) {
                        (Some(inferred_schema), sample_snippet)
                    } else {
                        eprintln!("⚠️ Schema inference failed even though call succeeded.");
                        (None::<rtfs::ast::TypeExpr>, sample_snippet)
                    }
                }
            }
            Err(e) => {
                eprintln!(
                    "⚠️ Failed to call tool for output schema introspection: {}",
                    e
                );
                (None, None)
            }
        };

        if schema_opt.is_some() {
            eprintln!("✅ Inferred output schema for {}", tool.tool_name);
            if let Some(sample) = sample_opt.as_ref() {
                eprintln!(
                    "Sample output (first lines):\n{}",
                    sample.lines().take(8).collect::<Vec<_>>().join("\n")
                );
            }
            // Proceed to save the capability with the inferred schema
        } else {
            // If introspection returned no schema but provided a sample with an error-like message,
            // attempt to synthesize plausible inputs heuristically and retry the call.
            if let Some(sample) = sample_opt.as_ref() {
                eprintln!(
                    "⚠️ Introspection with initial inputs failed or produced an error-like sample:"
                );
                // Show the full error, but limit to first 50 lines to avoid overwhelming output
                let error_lines: Vec<&str> = sample.lines().take(50).collect();
                eprintln!("{}", error_lines.join("\n"));
                if sample.lines().count() > 50 {
                    eprintln!(
                        "... (truncated, {} more lines)",
                        sample.lines().count() - 50
                    );
                }
            } else {
                eprintln!("⚠️ No sample or schema could be obtained from introspection and no error snippet provided.");
            }
        }

        if let Ok(manifest) = inspector.create_capability_from_mcp_tool(&tool, &introspection) {
            // Update manifest with inferred output schema if available
            let mut manifest = manifest;
            if let Some(output_schema) = schema_opt.clone() {
                manifest.output_schema = Some(output_schema);
            }

            let implementation_code =
                inspector.generate_mcp_rtfs_implementation(&tool, &server_url);
            match inspector.save_capability_to_rtfs(
                &manifest,
                &implementation_code,
                Path::new(&args.output_dir),
                sample_opt.as_deref(),
            ) {
                Ok(path) => eprintln!("💾 Saved discovered capability to {}", path.display()),
                Err(err) => eprintln!("⚠️ Failed to save capability: {}", err),
            }
        } else {
            eprintln!("⚠️ Failed to synthesize capability manifest from introspection");
        }

        Ok(())
    }

    fn resolve_server_url_from_overrides(server_name: &str) -> Option<String> {
        // Try to load curated overrides from 'capabilities/mcp/overrides.json'
        let root = std::env::current_dir().unwrap_or_else(|_| std::path::PathBuf::from("."));
        let overrides_path = if root.ends_with("rtfs_compiler") {
            root.parent()
                .unwrap_or(&root)
                .join("capabilities/mcp/overrides.json")
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
                            // Check if pattern matches server_name (pattern may contain wildcards like "github.*")
                            // Simple pattern matching: check if server_name matches the pattern
                            // For patterns like "github.*", we check if server_name starts with "github"
                            let pattern_clean = p.trim_end_matches(".*");
                            if server_name.starts_with(pattern_clean)
                                || server_name == pattern_clean
                            {
                                if let Some(remotes) =
                                    server.get("remotes").and_then(|r| r.as_array())
                                {
                                    for remote in remotes {
                                        if let Some(url) =
                                            remote.get("url").and_then(|u| u.as_str())
                                        {
                                            if url.starts_with("http://")
                                                || url.starts_with("https://")
                                            {
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

    // Helper function to derive server name from repo URL
    fn derive_server_name_from_repo_url(url: &str) -> Option<String> {
        use url::Url;
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

    async fn select_tool_and_extract_args_with_arbiter(
        ccos: &Arc<CCOS>,
        tools: &[DiscoveredMCPTool],
        hint: &str,
    ) -> Option<(String, serde_json::Map<String, serde_json::Value>)> {
        let tool_names: Vec<String> = tools.iter().map(|t| t.tool_name.clone()).collect();

        // Build a map of tool names to their input schemas for the prompt
        let mut tool_schemas = std::collections::HashMap::new();
        for tool in tools {
            if let Some(ref schema_json) = tool.input_schema_json {
                tool_schemas.insert(tool.tool_name.clone(), schema_json.clone());
            }
        }

        if let Some(arbiter_arc) = ccos.get_delegating_arbiter() {
            // Use the specialized tool selection method that bypasses delegation analysis
            // Note: This method is defined in impl DelegatingArbiter block
            // Use as_ref() to get &DelegatingArbiter from Arc
            // Call using fully qualified path to ensure method is found
            // Pass tool schemas so the LLM can use exact parameter names
            match ccos::arbiter::DelegatingArbiter::select_mcp_tool(
                arbiter_arc.as_ref(),
                hint,
                &tool_names,
                Some(&tool_schemas),
            )
            .await
            {
                Ok((tool_name, constraints)) => {
                    // Convert RTFS Value constraints to serde_json::Value
                    let mut args = serde_json::Map::new();
                    for (k, v) in &constraints {
                        let json_val = rtfs_value_to_json(v);
                        args.insert(k.to_string(), json_val);
                    }

                    // Debug: Show extracted parameters
                    if !args.is_empty() {
                        eprintln!("📋 Extracted parameters from hint:");
                        for (key, value) in &args {
                            eprintln!("  {}: {}", key, value);
                        }
                    } else {
                        eprintln!("⚠️ No parameters extracted from hint");
                    }

                    // Validate that the selected tool exists in our list
                    if tool_names.contains(&tool_name) {
                        eprintln!("🔧 Selected tool: '{}'", tool_name);
                        Some((tool_name, args))
                    } else {
                        eprintln!("⚠️ Arbiter selected tool '{}' but it was not in the provided list. Available: {}", tool_name, tool_names.join(", "));
                        // Try fuzzy matching as fallback
                        let hint_lower = hint.to_lowercase();
                        tool_names
                            .iter()
                            .find(|&tn| {
                                let tn_lower = tn.to_lowercase();
                                (hint_lower.contains("list") && tn_lower.contains("list"))
                                    || (hint_lower.contains("issue") && tn_lower.contains("issue"))
                                    || (hint_lower.contains("repository")
                                        && tn_lower.contains("repo"))
                            })
                            .map(|tn| {
                                eprintln!("🔧 Using fuzzy match: '{}'", tn);
                                (tn.clone(), args)
                            })
                    }
                }
                Err(e) => {
                    eprintln!("Arbiter failed to select tool: {}", e);
                    None
                }
            }
        } else {
            eprintln!("Delegating arbiter not available.");
            None
        }
    }

    /// Map extracted parameter names to match the tool's schema parameter names
    /// This handles cases where the LLM might still use slightly different names
    /// (though with the updated prompt, it should use exact schema names)
    fn map_parameters_to_schema(
        extracted_args: &serde_json::Map<String, serde_json::Value>,
        schema: Option<&serde_json::Value>,
    ) -> serde_json::Map<String, serde_json::Value> {
        let mut mapped = serde_json::Map::new();

        // Get schema property names and types
        let schema_props_map: std::collections::HashMap<String, String> =
            if let Some(schema) = schema {
                schema
                    .get("properties")
                    .and_then(|p| p.as_object())
                    .map(|props| {
                        props
                            .iter()
                            .map(|(k, v)| {
                                let type_str = v
                                    .get("type")
                                    .and_then(|t| t.as_str())
                                    .unwrap_or("unknown")
                                    .to_string();
                                (k.clone(), type_str)
                            })
                            .collect()
                    })
                    .unwrap_or_default()
            } else {
                std::collections::HashMap::new()
            };
        let schema_properties: Vec<String> = schema_props_map.keys().cloned().collect();

        for (extracted_key, value) in extracted_args {
            let mut matched = false;
            let mut matched_key = String::new();

            // First, check for exact match (case-insensitive)
            let extracted_lower = extracted_key.to_lowercase();
            for schema_key in &schema_properties {
                if extracted_lower == schema_key.to_lowercase() {
                    matched_key = schema_key.clone();
                    if extracted_key != schema_key {
                        eprintln!(
                            "  → Mapped '{}' → '{}' (case-insensitive match)",
                            extracted_key, schema_key
                        );
                    }
                    matched = true;
                    break;
                }
            }

            // If no exact match, try fuzzy matching as fallback
            if !matched {
                for schema_key in &schema_properties {
                    let schema_lower = schema_key.to_lowercase();
                    // Check if one contains the other (case-insensitive)
                    if extracted_lower.contains(&schema_lower)
                        || schema_lower.contains(&extracted_lower)
                    {
                        // Check if they're similar enough (one is substring of the other)
                        let min_len = extracted_lower.len().min(schema_lower.len());
                        let max_len = extracted_lower.len().max(schema_lower.len());
                        // If the shorter is at least 70% of the longer, consider it a match
                        if min_len as f64 / max_len as f64 >= 0.7 {
                            matched_key = schema_key.clone();
                            eprintln!(
                                "  → Mapped '{}' → '{}' (fuzzy match)",
                                extracted_key, schema_key
                            );
                            matched = true;
                            break;
                        }
                    }
                }
            }

            let final_key = if matched {
                matched_key
            } else {
                extracted_key.clone()
            };

            // Check for enum casing mismatch if schema has enums for this property
            let value_with_enum_fix = if let Some(schema) = schema {
                if let Some(prop) = schema
                    .get("properties")
                    .and_then(|p| p.as_object())
                    .and_then(|p| p.get(&final_key))
                {
                    if let Some(enums) = prop.get("enum").and_then(|e| e.as_array()) {
                        if let serde_json::Value::String(s) = value {
                            let s_upper = s.to_uppercase();
                            // Check if any enum matches case-insensitively
                            if let Some(matched_enum) = enums.iter().find(|e| {
                                if let Some(es) = e.as_str() {
                                    es.to_uppercase() == s_upper
                                } else {
                                    false
                                }
                            }) {
                                if matched_enum.as_str() != Some(s.as_str()) {
                                    eprintln!(
                                        "  → Correcting case for parameter '{}': '{}' -> '{}'",
                                        final_key,
                                        s,
                                        matched_enum.as_str().unwrap_or("")
                                    );
                                    matched_enum.clone()
                                } else {
                                    value.clone()
                                }
                            } else {
                                value.clone()
                            }
                        } else {
                            value.clone()
                        }
                    } else {
                        value.clone()
                    }
                } else {
                    value.clone()
                }
            } else {
                value.clone()
            };

            // Perform type coercion if we have schema info
            let final_value = if let Some(expected_type) = schema_props_map.get(&final_key) {
                match (expected_type.as_str(), &value_with_enum_fix) {
                    ("array", serde_json::Value::String(s)) => {
                        eprintln!(
                            "  → Coercing string '{}' to array for parameter '{}'",
                            s, final_key
                        );
                        // Try to split by comma if it looks like a list, otherwise single item
                        if s.contains(',') {
                            let items: Vec<serde_json::Value> = s
                                .split(',')
                                .map(|item| serde_json::Value::String(item.trim().to_string()))
                                .collect();
                            serde_json::Value::Array(items)
                        } else {
                            serde_json::Value::Array(vec![serde_json::Value::String(s.clone())])
                        }
                    }
                    ("integer", serde_json::Value::String(s)) => {
                        if let Ok(i) = s.parse::<i64>() {
                            eprintln!(
                                "  → Coercing string '{}' to integer for parameter '{}'",
                                s, final_key
                            );
                            serde_json::Value::Number(serde_json::Number::from(i))
                        } else {
                            value_with_enum_fix.clone()
                        }
                    }
                    ("number", serde_json::Value::String(s)) => {
                        if let Ok(f) = s.parse::<f64>() {
                            if let Some(n) = serde_json::Number::from_f64(f) {
                                eprintln!(
                                    "  → Coercing string '{}' to number for parameter '{}'",
                                    s, final_key
                                );
                                serde_json::Value::Number(n)
                            } else {
                                value_with_enum_fix.clone()
                            }
                        } else {
                            value_with_enum_fix.clone()
                        }
                    }
                    ("boolean", serde_json::Value::String(s)) => {
                        let s_lower = s.to_lowercase();
                        if s_lower == "true" {
                            serde_json::Value::Bool(true)
                        } else if s_lower == "false" {
                            serde_json::Value::Bool(false)
                        } else {
                            value_with_enum_fix.clone()
                        }
                    }
                    _ => value_with_enum_fix.clone(),
                }
            } else {
                value_with_enum_fix.clone()
            };

            mapped.insert(final_key, final_value);
            if !matched {
                eprintln!("  → Kept '{}' as-is (no schema match found)", extracted_key);
            }
        }

        mapped
    }

    fn build_safe_test_inputs_from_schema(
        schema: Option<&serde_json::Value>,
        plausible: bool,
    ) -> serde_json::Value {
        let mut inputs = serde_json::Map::new();

        if let Some(schema) = schema {
            if let Some(properties) = schema.get("properties").and_then(|p| p.as_object()) {
                let required: Vec<String> = schema
                    .get("required")
                    .and_then(|r| r.as_array())
                    .map(|arr| {
                        arr.iter()
                            .filter_map(|s| s.as_str())
                            .map(|s| s.to_string())
                            .collect()
                    })
                    .unwrap_or_default();

                for (key, prop_schema) in properties {
                    // Only include required fields
                    if required.contains(key) {
                        let default_value =
                            generate_safe_default_value(prop_schema, key, plausible);
                        inputs.insert(key.clone(), default_value);
                    }
                }
            }
        }

        serde_json::Value::Object(inputs)
    }

    fn generate_safe_default_value(
        schema: &serde_json::Value,
        name: &str,
        plausible: bool,
    ) -> serde_json::Value {
        if let Some(type_str) = schema.get("type").and_then(|t| t.as_str()) {
            match type_str {
                "string" => {
                    // Use empty string or a safe default from enum if available
                    if let Some(enum_vals) = schema.get("enum").and_then(|e| e.as_array()) {
                        if let Some(first) = enum_vals.first().and_then(|v| v.as_str()) {
                            return serde_json::Value::String(first.to_string());
                        }
                    }
                    // For string fields, use empty string or a safe placeholder
                    let name_l = name.to_lowercase();
                    if plausible {
                        if name_l.contains("owner") || name_l.contains("user") {
                            return serde_json::Value::String("octocat".to_string());
                        }
                        if name_l.contains("repo") || name_l.contains("repository") {
                            return serde_json::Value::String("hello-world".to_string());
                        }
                        if name_l.contains("sha") || name_l.contains("commit") {
                            return serde_json::Value::String(
                                "0000000000000000000000000000000000000000".to_string(),
                            );
                        }
                        if name_l.contains("email") {
                            return serde_json::Value::String("example@example.com".to_string());
                        }
                        if name_l.contains("url") || name_l.contains("uri") {
                            return serde_json::Value::String("https://example.com".to_string());
                        }
                        if name_l.contains("name")
                            || name_l.contains("title")
                            || name_l.contains("label")
                        {
                            return serde_json::Value::String("example".to_string());
                        }
                        if name_l.contains("path") {
                            return serde_json::Value::String("/path/to/file".to_string());
                        }
                    }
                    serde_json::Value::String("".to_string())
                }
                "integer" | "number" => {
                    // Use 0 or minimum value if specified
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
}

fn load_agent_config(config_path: &str) -> Result<AgentConfig, Box<dyn Error>> {
    let mut content = std::fs::read_to_string(config_path)?;
    if content.starts_with("# RTFS") {
        content = content.lines().skip(1).collect::<Vec<_>>().join("\n");
    }
    toml::from_str(&content).map_err(|e| format!("failed to parse agent config: {}", e).into())
}

fn apply_llm_profile(
    agent_config: &AgentConfig,
    profile: Option<&str>,
) -> Result<(), Box<dyn Error>> {
    if let Some(profile_name) = profile {
        let (expanded_profiles, _, _) =
            rtfs::config::profile_selection::expand_profiles(agent_config);

        if let Some(llm_profile) = expanded_profiles.iter().find(|p| p.name == profile_name) {
            std::env::set_var("CCOS_DELEGATING_PROVIDER", llm_profile.provider.clone());
            std::env::set_var("CCOS_DELEGATING_MODEL", llm_profile.model.clone());
            if let Some(api_key_env) = &llm_profile.api_key_env {
                if let Ok(api_key) = std::env::var(api_key_env) {
                    std::env::set_var("OPENAI_API_KEY", api_key);
                }
            } else if let Some(api_key) = &llm_profile.api_key {
                std::env::set_var("OPENAI_API_KEY", api_key.clone());
            }
        } else {
            return Err(format!("LLM profile '{}' not found in config", profile_name).into());
        }
    }
    Ok(())
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    let args = Args::parse();
    run_discovery(args).await
}
