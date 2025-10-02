//! GitHub MCP Capability Registry Demo
//!
//! This example discovers tools exposed by the hosted GitHub MCP Server
//! (https://api.githubcopilot.com/mcp/) and registers them as CCOS capabilities
//! inside the `CapabilityRegistry`. Once registered, any capability can be
//! executed through the registry just like the built-in ones.
//!
//! ## Usage
//!
//! ```bash
//! # Provide a PAT with repo/org scope (must allow the GitHub MCP Server)
//! export GITHUB_PERSONAL_ACCESS_TOKEN="ghp_your_token"
//!
//! # Pull read-only repo tooling and list the first 10 capabilities
//! cargo run --example github_mcp_registry -- \
//!   --toolset repos \
//!   --readonly \
//!   --limit 10 \
//!   --list-only
//!
//! # Execute a specific MCP tool with JSON arguments
//! cargo run --example github_mcp_registry -- \
//!   --toolset repos \
//!   --tool github.mcp.repos_get_repository \
//!   --payload '{"owner":"github","repo":"github-mcp-server"}'
//! ```
//!
//! The tool names follow the pattern `github.mcp.<mcp_tool_name>` so they are
//! easy to reference from RTFS programs or tests.

use clap::Parser;
use reqwest::blocking::Client;
use reqwest::header::{ACCEPT, CONTENT_TYPE, USER_AGENT};
use rtfs_compiler::ast::MapKey;
use rtfs_compiler::ccos::capabilities::capability::Capability;
use rtfs_compiler::ccos::capabilities::registry::CapabilityRegistry;
use rtfs_compiler::ccos::capability_marketplace::mcp_discovery::MCPTool;
use rtfs_compiler::ccos::capability_marketplace::CapabilityMarketplace;
use rtfs_compiler::runtime::error::{RuntimeError, RuntimeResult};
use rtfs_compiler::runtime::values::{Arity, Value};
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::collections::{hash_map::DefaultHasher, HashMap};
use std::env;
use std::fs;
use std::hash::{Hash, Hasher};
use std::io;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use uuid::Uuid;

#[derive(Debug, Clone)]
struct McpHandshake {
    session_header: Option<String>,
}

const CACHE_VERSION: u32 = 1;

#[derive(Debug, Clone)]
struct CacheConfig {
    path: PathBuf,
    refresh: bool,
    ttl_seconds: u64,
    endpoint: String,
    toolset: Option<String>,
    readonly: bool,
}

#[derive(Debug)]
struct CachedTools {
    tools: Vec<MCPTool>,
    age_seconds: u64,
}

#[derive(Debug, Serialize, Deserialize)]
struct ToolCacheRecord {
    version: u32,
    endpoint: String,
    toolset: Option<String>,
    readonly: bool,
    fetched_at: u64,
    tools: Vec<MCPTool>,
}

#[derive(Parser, Debug)]
#[command(about = "Build a CapabilityRegistry from GitHub MCP toolsets")]
struct Args {
    /// Base MCP endpoint (defaults to hosted GitHub MCP server)
    #[arg(long, default_value = "https://api.githubcopilot.com/mcp/")]
    endpoint: String,

    /// Optional single toolset (e.g. repos, issues, actions)
    #[arg(long)]
    toolset: Option<String>,

    /// Use the read-only variant of the selected toolset
    #[arg(long, default_value_t = false)]
    readonly: bool,

    /// Personal access token (falls back to GITHUB_PERSONAL_ACCESS_TOKEN env var)
    #[arg(long)]
    token: Option<String>,

    /// HTTP timeout in seconds for discovery and tool execution
    #[arg(long, default_value_t = 30)]
    timeout_seconds: u64,

    /// Maximum number of tools to register
    #[arg(long, default_value_t = 25)]
    limit: usize,

    /// Refresh the MCP tool cache, forcing a new discovery request
    #[arg(long, default_value_t = false)]
    refresh: bool,

    /// Disable the MCP tool cache entirely
    #[arg(long, default_value_t = false)]
    no_cache: bool,

    /// Maximum age (in seconds) for cached MCP tool catalogs (0 = no expiration)
    #[arg(long, default_value_t = 3600)]
    cache_ttl_seconds: u64,

    /// Override the directory used to persist cached MCP tool catalogs
    #[arg(long)]
    cache_dir: Option<String>,

    /// Only list discovered tools without executing anything
    #[arg(long, default_value_t = false)]
    list_only: bool,

    /// Capability identifier to execute after registration
    #[arg(long)]
    tool: Option<String>,

    /// JSON payload passed to the executed tool (defaults to `{}`)
    #[arg(long)]
    payload: Option<String>,
}

fn main() -> Result<(), RuntimeError> {
    let args = Args::parse();
    let base_endpoint = args.endpoint.trim_end_matches('/').to_string();
    let endpoint = build_endpoint(&base_endpoint, args.toolset.as_deref(), args.readonly);
    let timeout = Duration::from_secs(args.timeout_seconds);
    let token = resolve_token(args.token)?;

    println!("🔗 Using GitHub MCP endpoint: {endpoint}");

    let cache_config = build_cache_config(
        args.no_cache,
        args.cache_dir.as_deref(),
        &endpoint,
        args.toolset.as_deref(),
        args.readonly,
        args.cache_ttl_seconds,
        args.refresh,
    )?;

    let handshake = initialize_mcp_server(&base_endpoint, token.as_deref(), timeout)?;

    let mut session_token = handshake.session_header;

    if session_token.is_some() {
        println!("🪪 Using MCP session from initialize header");
    }

    let created_session = create_mcp_session(
        &base_endpoint,
        token.as_deref(),
        session_token.as_deref(),
        timeout,
    )?;

    if let Some(new_session) = created_session {
        session_token = Some(new_session.clone());
        println!("🪪 Established MCP session");
    }

    let mut used_cache = false;
    let mut maybe_tools: Option<Vec<MCPTool>> = None;

    if let Some(config) = &cache_config {
        if config.refresh {
            println!("🔄 Cache refresh requested; fetching latest tool catalog");
        } else {
            match load_cached_tools(config) {
                Ok(Some(cached)) => {
                    println!(
                        "📦 Loaded {} cached tool(s) (age {})",
                        cached.tools.len(),
                        format_duration_human(cached.age_seconds)
                    );
                    maybe_tools = Some(cached.tools);
                    used_cache = true;
                }
                Ok(None) => {}
                Err(err) => {
                    println!("⚠️ Failed to read MCP tool cache: {err}");
                }
            }
        }
    }

    let tools = if let Some(cached_tools) = maybe_tools {
        cached_tools
    } else {
        let fetched = fetch_mcp_tools(
            &endpoint,
            token.as_deref(),
            session_token.as_deref(),
            timeout,
        )?;

        if fetched.is_empty() {
            return Err(RuntimeError::Generic(
                "No tools returned by the GitHub MCP server".to_string(),
            ));
        }

        if let Some(config) = &cache_config {
            if let Err(err) = save_cached_tools(config, &fetched) {
                println!("⚠️ Failed to cache MCP tools: {err}");
            } else {
                println!(
                    "💾 Cached {} tool(s) at {}",
                    fetched.len(),
                    config.path.display()
                );
            }
        }

        fetched
    };

    if !used_cache {
        println!(
            "✨ Discovered {} tool(s); registering first {}",
            tools.len(),
            args.limit.min(tools.len())
        );
    } else {
        println!(
            "✨ Using cached {} tool(s); registering first {}",
            tools.len(),
            args.limit.min(tools.len())
        );
    }

    let mut registry = CapabilityRegistry::new();
    let registered_ids = register_tools(
        &mut registry,
        &endpoint,
        token.clone(),
        timeout,
        session_token.clone(),
        &tools,
        args.limit,
    )?;

    for cap_id in &registered_ids {
        println!("  • {cap_id}");
    }

    if args.list_only || args.tool.is_none() {
        return Ok(());
    }

    let requested_tool = args
        .tool
        .as_ref()
        .map(String::from)
        .ok_or_else(|| RuntimeError::Generic("Tool name must be provided".to_string()))?;

    if !registered_ids.contains(&requested_tool) {
        return Err(RuntimeError::Generic(format!(
            "Requested tool '{requested_tool}' was not registered; increase --limit or adjust toolset"
        )));
    }

    let call_value = if let Some(payload) = &args.payload {
        let json_value: serde_json::Value = serde_json::from_str(payload)
            .map_err(|err| RuntimeError::Generic(format!("Failed to parse payload JSON: {err}")))?;
        CapabilityMarketplace::json_to_rtfs_value(&json_value)?
    } else {
        Value::Map(HashMap::new())
    };

    println!("🚀 Executing {requested_tool} ...");
    let result =
        registry.execute_capability_with_microvm(&requested_tool, vec![call_value], None)?;

    println!("✅ Result:\n{result:#?}");
    Ok(())
}

fn resolve_token(explicit: Option<String>) -> RuntimeResult<Option<String>> {
    if let Some(token) = explicit {
        if token.trim().is_empty() {
            return Err(RuntimeError::Generic(
                "Provided token cannot be empty".to_string(),
            ));
        }
        return Ok(Some(token));
    }

    match env::var("GITHUB_PERSONAL_ACCESS_TOKEN") {
        Ok(value) if !value.trim().is_empty() => Ok(Some(value)),
        Ok(_) => Err(RuntimeError::Generic(
            "GITHUB_PERSONAL_ACCESS_TOKEN was set but empty".to_string(),
        )),
        Err(env::VarError::NotPresent) => Err(RuntimeError::Generic(
            "GitHub PAT missing: pass --token or set GITHUB_PERSONAL_ACCESS_TOKEN".to_string(),
        )),
        Err(env::VarError::NotUnicode(_)) => Err(RuntimeError::Generic(
            "GitHub PAT contained invalid unicode".to_string(),
        )),
    }
}

fn build_cache_config(
    no_cache: bool,
    cache_dir_override: Option<&str>,
    endpoint: &str,
    toolset: Option<&str>,
    readonly: bool,
    ttl_seconds: u64,
    refresh: bool,
) -> RuntimeResult<Option<CacheConfig>> {
    if no_cache {
        return Ok(None);
    }

    let cache_dir = resolve_cache_dir(cache_dir_override)?;
    fs::create_dir_all(&cache_dir).map_err(|err| {
        RuntimeError::Generic(format!(
            "Failed to create cache directory {}: {err}",
            cache_dir.display()
        ))
    })?;

    let key = build_cache_key(endpoint, toolset, readonly);
    let path = cache_dir.join(format!("github_mcp_registry_{key}.json"));

    Ok(Some(CacheConfig {
        path,
        refresh,
        ttl_seconds,
        endpoint: endpoint.to_string(),
        toolset: toolset.map(|value| value.to_string()),
        readonly,
    }))
}

fn resolve_cache_dir(override_dir: Option<&str>) -> RuntimeResult<PathBuf> {
    if let Some(dir) = override_dir {
        return Ok(PathBuf::from(dir));
    }

    if let Ok(dir) = env::var("CCOS_CACHE_DIR") {
        return Ok(PathBuf::from(dir));
    }

    if let Ok(dir) = env::var("XDG_CACHE_HOME") {
        return Ok(PathBuf::from(dir).join("ccos"));
    }

    if let Ok(home) = env::var("HOME") {
        return Ok(PathBuf::from(home).join(".cache").join("ccos"));
    }

    env::current_dir()
        .map(|cwd| cwd.join(".ccos-cache"))
        .map_err(|err| RuntimeError::Generic(format!("Failed to determine cache directory: {err}")))
}

fn build_cache_key(endpoint: &str, toolset: Option<&str>, readonly: bool) -> String {
    let mut hasher = DefaultHasher::new();
    endpoint.hash(&mut hasher);
    toolset.unwrap_or("all").hash(&mut hasher);
    readonly.hash(&mut hasher);
    format!("{:016x}", hasher.finish())
}

fn load_cached_tools(config: &CacheConfig) -> RuntimeResult<Option<CachedTools>> {
    let contents = match fs::read_to_string(&config.path) {
        Ok(text) => text,
        Err(err) => {
            if err.kind() == io::ErrorKind::NotFound {
                return Ok(None);
            }
            return Err(RuntimeError::Generic(format!(
                "Failed to read cache file {}: {err}",
                config.path.display()
            )));
        }
    };

    let record: ToolCacheRecord = serde_json::from_str(&contents).map_err(|err| {
        RuntimeError::Generic(format!(
            "Failed to parse MCP cache payload at {}: {err}",
            config.path.display()
        ))
    })?;

    if record.version != CACHE_VERSION {
        println!(
            "ℹ️ Ignoring cached MCP catalog at {} due to version mismatch ({} != {})",
            config.path.display(),
            record.version,
            CACHE_VERSION
        );
        return Ok(None);
    }

    if record.endpoint != config.endpoint
        || record.readonly != config.readonly
        || record.toolset.as_deref() != config.toolset.as_deref()
    {
        return Ok(None);
    }

    let fetched_at = record.fetched_at;
    let now = now_unix_timestamp()?;
    let age_seconds = now.saturating_sub(fetched_at);

    if config.ttl_seconds > 0 && age_seconds > config.ttl_seconds {
        println!(
            "ℹ️ Cached MCP catalog at {} expired after {} (TTL {}); refreshing",
            config.path.display(),
            format_duration_human(age_seconds),
            format_duration_human(config.ttl_seconds)
        );
        return Ok(None);
    }

    Ok(Some(CachedTools {
        tools: record.tools,
        age_seconds,
    }))
}

fn save_cached_tools(config: &CacheConfig, tools: &[MCPTool]) -> RuntimeResult<()> {
    let fetched_at = now_unix_timestamp()?;
    let record = ToolCacheRecord {
        version: CACHE_VERSION,
        endpoint: config.endpoint.clone(),
        toolset: config.toolset.clone(),
        readonly: config.readonly,
        fetched_at,
        tools: tools.to_vec(),
    };

    if let Some(parent) = config.path.parent() {
        fs::create_dir_all(parent).map_err(|err| {
            RuntimeError::Generic(format!(
                "Failed to ensure cache directory {}: {err}",
                parent.display()
            ))
        })?;
    }

    let payload = serde_json::to_string_pretty(&record).map_err(|err| {
        RuntimeError::Generic(format!("Failed to serialize MCP cache payload: {err}"))
    })?;

    fs::write(&config.path, payload).map_err(|err| {
        RuntimeError::Generic(format!(
            "Failed to write MCP cache file {}: {err}",
            config.path.display()
        ))
    })?;

    Ok(())
}

fn now_unix_timestamp() -> RuntimeResult<u64> {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs())
        .map_err(|err| RuntimeError::Generic(format!("System time is before UNIX_EPOCH: {err}")))
}

fn format_duration_human(seconds: u64) -> String {
    let hours = seconds / 3600;
    let minutes = (seconds % 3600) / 60;
    let secs = seconds % 60;

    if hours > 0 {
        format!("{}h {}m {}s", hours, minutes, secs)
    } else if minutes > 0 {
        format!("{}m {}s", minutes, secs)
    } else {
        format!("{}s", secs)
    }
}

fn initialize_mcp_server(
    endpoint: &str,
    token: Option<&str>,
    timeout: Duration,
) -> RuntimeResult<McpHandshake> {
    let client = Client::builder()
        .timeout(timeout)
        .build()
        .map_err(|err| RuntimeError::Generic(format!("Failed to build HTTP client: {err}")))?;

    let request_body = json!({
        "jsonrpc": "2.0",
        "id": "initialize",
        "method": "initialize",
        "params": {
            "protocolVersion": "2025-06-18",
            "capabilities": serde_json::Value::Object(serde_json::Map::new()),
            "clientInfo": {
                "name": "ccos-github-mcp-demo",
                "version": env!("CARGO_PKG_VERSION"),
            }
        }
    });

    let mut request = client
        .post(endpoint)
        .header(USER_AGENT, "CCOS-GitHub-MCP-Demo/1.0")
        .header(CONTENT_TYPE, "application/json")
        .json(&request_body);

    if let Some(auth) = token {
        request = request.bearer_auth(auth);
    }

    let response = request
        .send()
        .map_err(|err| RuntimeError::Generic(format!("Failed to initialize MCP server: {err}")))?;

    let status = response.status();
    let headers = response.headers().clone();
    let body = response.text().map_err(|err| {
        RuntimeError::Generic(format!("Failed to read MCP initialize response: {err}"))
    })?;

    if !status.is_success() {
        if status.as_u16() == 404 || status.as_u16() == 405 {
            println!(
                "⚠️ MCP server does not advertise initialize support (status {}); continuing",
                status
            );
            return Ok(McpHandshake {
                session_header: None,
            });
        }

        if body.contains("Invalid session ID") {
            println!(
                "⚠️ MCP server rejected initialize handshake with 'Invalid session ID'; continuing"
            );
            return Ok(McpHandshake {
                session_header: None,
            });
        }

        return Err(RuntimeError::Generic(format!(
            "MCP initialize failed: {} - {}",
            status, body
        )));
    }

    let rpc_value: serde_json::Value = serde_json::from_str(&body).map_err(|err| {
        RuntimeError::Generic(format!("Failed to parse MCP initialize response: {err}"))
    })?;

    if let Some(error) = rpc_value.get("error") {
        if error
            .get("code")
            .and_then(|c| c.as_i64())
            .map(|code| code == -32601)
            .unwrap_or(false)
        {
            println!("⚠️ MCP server reported initialize as unsupported; continuing");
            return Ok(McpHandshake {
                session_header: None,
            });
        }

        return Err(RuntimeError::Generic(format!(
            "MCP initialize error: {error}"
        )));
    }

    if let Some(result) = rpc_value.get("result") {
        if let Some(protocol) = result.get("protocolVersion").and_then(|v| v.as_str()) {
            println!("🤝 MCP initialize acknowledged (protocol {protocol})");
        }
        if let Some(instructions) = result.get("instructions").and_then(|v| v.as_str()) {
            println!("📝 MCP server instructions: {instructions}");
        }
    }

    let session_header = headers
        .get("Mcp-Session-Id")
        .and_then(|value| value.to_str().ok())
        .map(ToString::to_string);

    if session_header.is_some() {
        println!("🧾 MCP server issued session header");
    }

    Ok(McpHandshake { session_header })
}

fn create_mcp_session(
    endpoint: &str,
    token: Option<&str>,
    existing_session: Option<&str>,
    timeout: Duration,
) -> RuntimeResult<Option<String>> {
    let client = Client::builder()
        .timeout(timeout)
        .build()
        .map_err(|err| RuntimeError::Generic(format!("Failed to build HTTP client: {err}")))?;

    let mut request = client
        .post(endpoint)
        .header(USER_AGENT, "CCOS-GitHub-MCP-Demo/1.0")
        .header(CONTENT_TYPE, "application/json")
        .json(&json!({
            "jsonrpc": "2.0",
            "id": "session_create",
            "method": "session/create",
            "params": {}
        }));

    if let Some(auth) = token {
        request = request.bearer_auth(auth);
    }

    if let Some(session) = existing_session {
        request = request.header("Mcp-Session-Id", session);
    }

    let response = request
        .send()
        .map_err(|err| RuntimeError::Generic(format!("Failed to create MCP session: {err}")))?;

    if response.status().is_client_error() && response.status().as_u16() == 404 {
        return Ok(None);
    }

    let status = response.status();
    let headers = response.headers().clone();
    let body = response.text().map_err(|err| {
        RuntimeError::Generic(format!("Failed to read MCP session response: {err}"))
    })?;

    if !status.is_success() {
        if status.as_u16() == 405 || (status.as_u16() == 400 && body.contains("Invalid session ID"))
        {
            println!(
                "⚠️ MCP server declined session handshake (status {}); continuing without session",
                status
            );
            return Ok(None);
        }
        return Err(RuntimeError::Generic(format!(
            "MCP session create failed: {} - {}",
            status, body
        )));
    }

    let rpc_value: serde_json::Value = serde_json::from_str(&body).map_err(|err| {
        RuntimeError::Generic(format!("Failed to parse MCP session response: {err}"))
    })?;

    if let Some(error) = rpc_value.get("error") {
        if error
            .get("code")
            .and_then(|c| c.as_i64())
            .map(|code| code == -32601)
            .unwrap_or(false)
        {
            return Ok(None);
        }
        if error
            .get("message")
            .and_then(|m| m.as_str())
            .map(|msg| msg.contains("Invalid session ID"))
            .unwrap_or(false)
        {
            println!("⚠️ MCP server reported invalid session support; continuing without session");
            return Ok(None);
        }
        return Err(RuntimeError::Generic(format!("MCP session error: {error}")));
    }

    let mut session_id = rpc_value
        .get("result")
        .and_then(|result| result.get("session"))
        .and_then(|session| session.get("id"))
        .and_then(|id| id.as_str())
        .map(|s| s.to_string());

    if session_id.is_none() {
        session_id = headers
            .get("Mcp-Session-Id")
            .and_then(|value| value.to_str().ok())
            .map(ToString::to_string);
    }

    Ok(session_id)
}

fn build_endpoint(base: &str, toolset: Option<&str>, readonly: bool) -> String {
    let mut normalized = base.trim_end_matches('/').to_string();
    if let Some(toolset_name) = toolset {
        if !toolset_name.trim().is_empty() && toolset_name != "all" {
            normalized = join_path(&normalized, &format!("x/{toolset_name}"));
        }
    }
    if readonly {
        normalized = join_path(&normalized, "readonly");
    }
    normalized
}

fn join_path(base: &str, suffix: &str) -> String {
    format!(
        "{}/{}",
        base.trim_end_matches('/'),
        suffix.trim_start_matches('/')
    )
}

fn fetch_mcp_tools(
    endpoint: &str,
    token: Option<&str>,
    session_id: Option<&str>,
    timeout: Duration,
) -> RuntimeResult<Vec<MCPTool>> {
    #[derive(Deserialize)]
    struct RpcToolsResult {
        tools: Vec<MCPTool>,
    }

    #[derive(Deserialize)]
    struct RpcError {
        message: String,
        #[serde(default)]
        code: Option<i64>,
        #[serde(default)]
        data: Option<serde_json::Value>,
    }

    #[derive(Deserialize)]
    struct RpcResponse {
        #[allow(dead_code)]
        jsonrpc: Option<String>,
        result: Option<RpcToolsResult>,
        error: Option<RpcError>,
    }

    let client = Client::builder()
        .timeout(timeout)
        .build()
        .map_err(|err| RuntimeError::Generic(format!("Failed to build HTTP client: {err}")))?;

    let mut params = serde_json::Map::new();
    if let Some(id) = session_id {
        params.insert(
            "sessionId".to_string(),
            serde_json::Value::String(id.to_string()),
        );
    }

    let request_body = serde_json::Value::Object({
        let mut map = serde_json::Map::new();
        map.insert(
            "jsonrpc".to_string(),
            serde_json::Value::String("2.0".to_string()),
        );
        map.insert(
            "id".to_string(),
            serde_json::Value::String("tools_discovery".to_string()),
        );
        map.insert(
            "method".to_string(),
            serde_json::Value::String("tools/list".to_string()),
        );
        map.insert("params".to_string(), serde_json::Value::Object(params));
        map
    });

    let mut request = client
        .post(endpoint)
        .header(USER_AGENT, "CCOS-GitHub-MCP-Demo/1.0")
        .header(ACCEPT, "application/json")
        .header(CONTENT_TYPE, "application/json")
        .json(&request_body);

    if let Some(auth) = token {
        request = request.bearer_auth(auth);
    }

    if let Some(id) = session_id {
        request = request.header("Mcp-Session-Id", id);
    }

    let response = request
        .send()
        .map_err(|err| RuntimeError::Generic(format!("Failed to query MCP tools: {err}")))?;

    let status = response.status();
    let body = response.text().map_err(|err| {
        RuntimeError::Generic(format!("Failed to read MCP tools response: {err}"))
    })?;

    if !status.is_success() {
        return Err(RuntimeError::Generic(format!(
            "GitHub MCP tools request failed: {} - {}",
            status, body
        )));
    }

    let rpc_response: RpcResponse = serde_json::from_str(&body).map_err(|err| {
        RuntimeError::Generic(format!("Failed to parse MCP tools response: {err}"))
    })?;

    if let Some(error) = rpc_response.error {
        let mut message = error.message;
        if let Some(code) = error.code {
            message = format!("{message} (code {code})");
        }
        if let Some(data) = error.data {
            message = format!("{message} – data: {data}");
        }
        return Err(RuntimeError::Generic(format!(
            "MCP server error: {message}"
        )));
    }

    let result = rpc_response.result.ok_or_else(|| {
        RuntimeError::Generic("MCP server response missing result field".to_string())
    })?;

    Ok(result.tools)
}

fn register_tools(
    registry: &mut CapabilityRegistry,
    endpoint: &str,
    token: Option<String>,
    timeout: Duration,
    session_id: Option<String>,
    tools: &[MCPTool],
    limit: usize,
) -> RuntimeResult<Vec<String>> {
    let mut registered = Vec::new();
    let trimmed_endpoint = endpoint.trim_end_matches('/').to_string();

    for tool in tools.iter().take(limit) {
        let capability_id = format!("github.mcp.{}", tool.name);
        let description = tool
            .description
            .clone()
            .unwrap_or_else(|| format!("GitHub MCP tool '{}'", tool.name));

        let closure_endpoint = trimmed_endpoint.clone();
        let closure_token = token.clone();
        let tool_name = tool.name.clone();
        let request_timeout = timeout;
        let closure_session = session_id.clone();

        let implementation = Arc::new(move |args: Vec<Value>| {
            let arguments = args_to_json(&args)?;
            call_github_tool(
                &closure_endpoint,
                closure_token.as_deref(),
                closure_session.as_deref(),
                &tool_name,
                &arguments,
                request_timeout,
            )
        });

        let capability = Capability::new(capability_id.clone(), Arity::Fixed(1), implementation);
        registry.register_custom_capability(capability);

        println!("Registered {capability_id} – {description}");
        registered.push(capability_id);
    }

    Ok(registered)
}

fn args_to_json(args: &[Value]) -> RuntimeResult<serde_json::Value> {
    match args.len() {
        0 => Ok(serde_json::Value::Object(serde_json::Map::new())),
        1 => rtfs_value_to_json(&args[0]),
        _ => {
            let mut items = Vec::with_capacity(args.len());
            for value in args {
                items.push(rtfs_value_to_json(value)?);
            }
            Ok(serde_json::Value::Array(items))
        }
    }
}

fn rtfs_value_to_json(value: &Value) -> RuntimeResult<serde_json::Value> {
    match value {
        Value::Nil => Ok(serde_json::Value::Null),
        Value::Integer(i) => Ok(serde_json::Value::Number((*i).into())),
        Value::Float(f) => serde_json::Number::from_f64(*f)
            .map(serde_json::Value::Number)
            .ok_or_else(|| RuntimeError::Generic("Invalid float value".to_string())),
        Value::String(s) => Ok(serde_json::Value::String(s.clone())),
        Value::Boolean(b) => Ok(serde_json::Value::Bool(*b)),
        Value::ResourceHandle(handle) => Ok(serde_json::Value::String(handle.clone())),
        Value::Vector(items) | Value::List(items) => {
            let mut json_items = Vec::with_capacity(items.len());
            for item in items {
                json_items.push(rtfs_value_to_json(item)?);
            }
            Ok(serde_json::Value::Array(json_items))
        }
        Value::Map(map) => {
            let mut json_map = serde_json::Map::new();
            for (key, val) in map {
                let key_str = match key {
                    MapKey::String(s) => s.clone(),
                    MapKey::Keyword(s) => s.0.clone(),
                    MapKey::Integer(i) => i.to_string(),
                };
                json_map.insert(key_str, rtfs_value_to_json(val)?);
            }
            Ok(serde_json::Value::Object(json_map))
        }
        Value::Timestamp(ts) => Ok(serde_json::Value::String(ts.clone())),
        Value::Uuid(uid) => Ok(serde_json::Value::String(uid.clone())),
        Value::Keyword(k) => Ok(serde_json::Value::String(k.0.clone())),
        Value::Symbol(s) => Ok(serde_json::Value::String(s.0.clone())),
        Value::Function(_) | Value::FunctionPlaceholder(_) => Err(RuntimeError::Generic(
            "Cannot serialize function values into JSON for MCP calls".to_string(),
        )),
        Value::Error(err) => Ok(serde_json::Value::String(format!(
            "#<error: {}>",
            err.message
        ))),
    }
}

fn call_github_tool(
    endpoint: &str,
    token: Option<&str>,
    session_id: Option<&str>,
    tool_name: &str,
    arguments: &serde_json::Value,
    timeout: Duration,
) -> RuntimeResult<Value> {
    let client = Client::builder()
        .timeout(timeout)
        .build()
        .map_err(|err| RuntimeError::Generic(format!("Failed to build HTTP client: {err}")))?;

    let mut params = serde_json::Map::new();
    params.insert(
        "name".to_string(),
        serde_json::Value::String(tool_name.to_string()),
    );
    if !arguments.is_null() {
        params.insert("arguments".to_string(), arguments.clone());
    }
    if let Some(id) = session_id {
        params.insert(
            "sessionId".to_string(),
            serde_json::Value::String(id.to_string()),
        );
    }

    let mut request = client
        .post(endpoint)
        .header(USER_AGENT, "CCOS-GitHub-MCP-Demo/1.0")
        .header(CONTENT_TYPE, "application/json")
        .json(&json!({
            "jsonrpc": "2.0",
            "id": format!("tool_call_{}", Uuid::new_v4()),
            "method": "tools/call",
            "params": serde_json::Value::Object(params)
        }));

    if let Some(auth) = token {
        request = request.bearer_auth(auth);
    }

    if let Some(id) = session_id {
        request = request.header("Mcp-Session-Id", id);
    }

    let response = request
        .send()
        .map_err(|err| RuntimeError::Generic(format!("Failed to execute MCP tool: {err}")))?;

    let status = response.status();
    let body = response
        .text()
        .map_err(|err| RuntimeError::Generic(format!("Failed to read MCP tool response: {err}")))?;

    if !status.is_success() {
        return Err(RuntimeError::Generic(format!(
            "MCP tool execution failed: {} - {}",
            status, body
        )));
    }

    let json_response: serde_json::Value = serde_json::from_str(&body).map_err(|err| {
        RuntimeError::Generic(format!("Failed to parse MCP tool response: {err}"))
    })?;

    if let Some(error) = json_response.get("error") {
        return Err(RuntimeError::Generic(format!(
            "MCP tool returned error: {error}"
        )));
    }

    let result = json_response
        .get("result")
        .ok_or_else(|| RuntimeError::Generic("Missing result field in MCP response".to_string()))?;

    if let Some(content) = result.get("content") {
        CapabilityMarketplace::json_to_rtfs_value(content)
    } else {
        CapabilityMarketplace::json_to_rtfs_value(result)
    }
}
