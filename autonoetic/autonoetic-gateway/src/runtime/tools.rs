use crate::llm::ToolDefinition;
use crate::policy::PolicyEngine;
use crate::runtime::reevaluation_state::{execute_scheduled_action, persist_reevaluation_state};
use crate::sandbox::{DependencyPlan, DependencyRuntime, SandboxDriverKind, SandboxRunner};
use autonoetic_types::agent::{AgentIdentity, AgentManifest, LlmConfig};
use autonoetic_types::background::{
    BackgroundMode, BackgroundPolicy, BackgroundState, ScheduledAction,
};
use autonoetic_types::capability::Capability;
use autonoetic_types::config::GatewayConfig;
use autonoetic_types::runtime_lock::{
    LockedDependencySet, LockedGateway, LockedSandbox, LockedSdk, RuntimeLock,
};
use autonoetic_types::tool_error::tagged;
use chrono::{Duration, Utc};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::{LazyLock, Mutex};
use std::time::{Duration as StdDuration, Instant};

/// Metadata extracted from a tool call for disclosure, audit, and logging.
#[derive(Debug, Default)]
pub struct ToolMetadata {
    pub path: Option<String>,
}

/// Extracts the `path` field from tool arguments JSON.
/// Shared helper for file-backed native tools (memory.read, memory.write, skill.draft).
fn extract_path_from_args(arguments_json: &str) -> ToolMetadata {
    let mut meta = ToolMetadata::default();
    if let Ok(parsed_args) = serde_json::from_str::<serde_json::Value>(arguments_json) {
        if let Some(path) = parsed_args.get("path").and_then(|v| v.as_str()) {
            meta.path = Some(path.to_string());
        }
    }
    meta
}

fn validate_relative_agent_path(path: &str) -> anyhow::Result<()> {
    anyhow::ensure!(!path.trim().is_empty(), "path must not be empty");
    anyhow::ensure!(
        !path.starts_with('/')
            && !path
                .split('/')
                .any(|part| part.is_empty() || part == "." || part == ".."),
        "path must stay within the agent directory"
    );
    Ok(())
}

fn validate_agent_id(agent_id: &str) -> anyhow::Result<()> {
    anyhow::ensure!(!agent_id.trim().is_empty(), "agent_id must not be empty");
    anyhow::ensure!(
        !agent_id.starts_with('.') && !agent_id.ends_with('.') && !agent_id.contains(".."),
        "agent_id must not start or end with '.', or contain '..'"
    );
    anyhow::ensure!(
        agent_id
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || ch == '-' || ch == '_' || ch == '.'),
        "agent_id may only contain ASCII letters, digits, '.', '-' and '_'"
    );
    Ok(())
}

fn background_state_file_for_child(agent_dir: &Path, child_id: &str) -> anyhow::Result<PathBuf> {
    let agents_dir = agent_dir
        .parent()
        .ok_or_else(|| anyhow::anyhow!("Agent directory is missing its agents root parent"))?;
    Ok(agents_dir
        .join(".gateway")
        .join("scheduler")
        .join("agents")
        .join(format!("{child_id}.json")))
}

fn default_true() -> bool {
    true
}

fn requires_promotion_gate(agent_id: &str) -> bool {
    matches!(
        agent_id,
        "specialized_builder.default" | "evolution-steward.default"
    )
}

fn extract_host(url: &str) -> anyhow::Result<String> {
    let parsed = reqwest::Url::parse(url).map_err(|e| {
        anyhow::Error::from(tagged::Tagged::validation(anyhow::anyhow!(
            "Invalid URL '{}': {}",
            url,
            e
        )))
    })?;
    let host = parsed.host_str().ok_or_else(|| {
        anyhow::Error::from(tagged::Tagged::validation(anyhow::anyhow!(
            "URL '{}' does not contain a host",
            url
        )))
    })?;
    Ok(host.to_string())
}

fn block_on_http<F, T>(future: F) -> anyhow::Result<T>
where
    F: std::future::Future<Output = anyhow::Result<T>>,
{
    if let Ok(handle) = tokio::runtime::Handle::try_current() {
        tokio::task::block_in_place(|| handle.block_on(future))
    } else {
        tokio::runtime::Runtime::new()?.block_on(future)
    }
}

fn capabilities_are_empty(capabilities: &&[Capability]) -> bool {
    capabilities.is_empty()
}

#[derive(Debug, Serialize)]
struct StandardSkillFrontmatter<'a> {
    name: &'a str,
    description: &'a str,
    metadata: StandardMetadataRoot<'a>,
}

#[derive(Debug, Serialize)]
struct StandardMetadataRoot<'a> {
    autonoetic: StandardAutonoeticMetadata<'a>,
}

#[derive(Debug, Serialize)]
struct StandardAutonoeticMetadata<'a> {
    version: &'a str,
    runtime: &'a autonoetic_types::agent::RuntimeDeclaration,
    agent: &'a AgentIdentity,
    #[serde(skip_serializing_if = "capabilities_are_empty")]
    capabilities: &'a [Capability],
    #[serde(skip_serializing_if = "Option::is_none")]
    llm_config: &'a Option<LlmConfig>,
    #[serde(skip_serializing_if = "Option::is_none")]
    limits: &'a Option<autonoetic_types::agent::ResourceLimits>,
    #[serde(skip_serializing_if = "Option::is_none")]
    background: &'a Option<BackgroundPolicy>,
    #[serde(skip_serializing_if = "Option::is_none")]
    disclosure: &'a Option<autonoetic_types::disclosure::DisclosurePolicy>,
}

fn render_skill_frontmatter(manifest: &AgentManifest) -> anyhow::Result<String> {
    let frontmatter = StandardSkillFrontmatter {
        name: &manifest.agent.id,
        description: &manifest.agent.description,
        metadata: StandardMetadataRoot {
            autonoetic: StandardAutonoeticMetadata {
                version: &manifest.version,
                runtime: &manifest.runtime,
                agent: &manifest.agent,
                capabilities: &manifest.capabilities,
                llm_config: &manifest.llm_config,
                limits: &manifest.limits,
                background: &manifest.background,
                disclosure: &manifest.disclosure,
            },
        },
    };
    serde_yaml::to_string(&frontmatter).map_err(Into::into)
}

/// Defines a native, in-process tool handler.
pub trait NativeTool: Send + Sync {
    /// The exact name of the tool as it appears in LLM requests.
    fn name(&self) -> &'static str;

    /// The schema definition exposed to the LLM.
    fn definition(&self) -> ToolDefinition;

    /// Checks if the manifest/policy allows this tool to be exposed or called.
    fn is_available(&self, manifest: &AgentManifest) -> bool;

    /// Executes the tool call.
    fn execute(
        &self,
        manifest: &AgentManifest,
        policy: &PolicyEngine,
        agent_dir: &Path,
        gateway_dir: Option<&Path>,
        arguments_json: &str,
        session_id: Option<&str>,
        turn_id: Option<&str>,
    ) -> anyhow::Result<String>;

    /// Optionally extracts metadata from the tool's JSON arguments for disclosure policy tracking and audit.
    fn extract_metadata(&self, _arguments_json: &str) -> ToolMetadata {
        ToolMetadata::default()
    }
}

/// A thin static registry for native tool handlers.
pub struct NativeToolRegistry {
    tools: Vec<Box<dyn NativeTool>>,
}

impl NativeToolRegistry {
    pub fn new() -> Self {
        Self { tools: Vec::new() }
    }

    pub fn register(&mut self, tool: Box<dyn NativeTool>) {
        self.tools.push(tool);
    }

    /// Returns the definitions for all native tools available to the given agent.
    pub fn available_definitions(&self, manifest: &AgentManifest) -> Vec<ToolDefinition> {
        self.tools
            .iter()
            .filter(|t| t.is_available(manifest))
            .map(|t| t.definition())
            .collect()
    }

    /// Returns true if a native tool with the given name is registered.
    pub fn has_tool(&self, name: &str) -> bool {
        self.tools.iter().any(|t| t.name() == name)
    }

    /// Executes a registered tool call. Enforces availability checks defensively.
    pub fn execute(
        &self,
        name: &str,
        manifest: &AgentManifest,
        policy: &PolicyEngine,
        agent_dir: &Path,
        gateway_dir: Option<&Path>,
        arguments_json: &str,
        session_id: Option<&str>,
        turn_id: Option<&str>,
    ) -> anyhow::Result<String> {
        let tool = self
            .tools
            .iter()
            .find(|t| t.name() == name)
            .ok_or_else(|| anyhow::anyhow!("Unknown native tool '{}'", name))?;

        if !tool.is_available(manifest) {
            anyhow::bail!("Native tool '{}' is not available or permitted", name);
        }

        tool.execute(
            manifest,
            policy,
            agent_dir,
            gateway_dir,
            arguments_json,
            session_id,
            turn_id,
        )
    }

    /// Optionally extracts metadata from the tool's JSON arguments for disclosure policy tracking and audit.
    pub fn extract_metadata(&self, name: &str, arguments_json: &str) -> ToolMetadata {
        self.tools
            .iter()
            .find(|t| t.name() == name)
            .map(|t| t.extract_metadata(arguments_json))
            .unwrap_or_default()
    }
}

// ---------------------------------------------------------------------------
// Sandbox Exec Tool
// ---------------------------------------------------------------------------

pub struct SandboxExecTool;

impl NativeTool for SandboxExecTool {
    fn name(&self) -> &'static str {
        "sandbox.exec"
    }

    fn is_available(&self, manifest: &AgentManifest) -> bool {
        manifest
            .capabilities
            .iter()
            .any(|cap| matches!(cap, Capability::ShellExec { .. }))
    }

    fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: self.name().to_string(),
            description: "Execute an approved shell command in the configured sandbox driver"
                .to_string(),
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "command": { "type": "string" },
                    "dependencies": {
                        "type": "object",
                        "properties": {
                            "runtime": { "type": "string", "enum": ["python", "nodejs", "node"] },
                            "packages": {
                                "type": "array",
                                "items": { "type": "string" },
                                "minItems": 1
                            }
                        },
                        "required": ["runtime", "packages"]
                    }
                },
                "required": ["command"],
                "additionalProperties": false
            }),
        }
    }

    fn execute(
        &self,
        manifest: &AgentManifest,
        policy: &PolicyEngine,
        agent_dir: &Path,
        _gateway_dir: Option<&Path>,
        arguments_json: &str,
        _session_id: Option<&str>,
        _turn_id: Option<&str>,
    ) -> anyhow::Result<String> {
        let args: SandboxExecArgs = serde_json::from_str(arguments_json)
            .map_err(|e| anyhow::anyhow!("Invalid JSON arguments for '{}': {}", self.name(), e))?;

        anyhow::ensure!(
            !args.command.trim().is_empty(),
            "sandbox command must not be empty"
        );
        anyhow::ensure!(
            policy.can_exec_shell(&args.command),
            "sandbox command denied by ShellExec policy"
        );

        let dep_plan = dependency_plan_from_args_or_lock(manifest, agent_dir, args.dependencies)?;
        let driver = SandboxDriverKind::parse(&manifest.runtime.sandbox)?;
        let agent_dir_str = agent_dir
            .to_str()
            .ok_or_else(|| anyhow::anyhow!("Agent directory is not valid UTF-8"))?;

        let runner = SandboxRunner::spawn_with_driver_and_dependencies(
            driver,
            agent_dir_str,
            &args.command,
            dep_plan.as_ref(),
        )?;
        let output = runner.process.wait_with_output()?;
        let body = serde_json::json!({
            "ok": output.status.success(),
            "exit_code": output.status.code(),
            "stdout": String::from_utf8_lossy(&output.stdout),
            "stderr": String::from_utf8_lossy(&output.stderr)
        });
        serde_json::to_string(&body).map_err(Into::into)
    }
}

// ---------------------------------------------------------------------------
// Web Search Tool
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize)]
struct WebSearchArgs {
    query: String,
    #[serde(default)]
    provider: Option<String>,
    #[serde(default)]
    max_results: Option<usize>,
    #[serde(default)]
    timeout_secs: Option<u64>,
    #[serde(default)]
    engine_url: Option<String>,
    #[serde(default)]
    duckduckgo_engine_url: Option<String>,
    #[serde(default)]
    google_engine_url: Option<String>,
    #[serde(default)]
    google_engine_id: Option<String>,
    #[serde(default)]
    google_api_key_env: Option<String>,
    #[serde(default)]
    google_engine_id_env: Option<String>,
    #[serde(default)]
    cache_ttl_secs: Option<u64>,
}

fn default_web_search_engine_url() -> String {
    "https://duckduckgo.com/".to_string()
}

fn default_google_search_engine_url() -> String {
    "https://www.googleapis.com/customsearch/v1".to_string()
}

const GOOGLE_API_KEY_ENV_DEFAULT: &str = "AUTONOETIC_GOOGLE_SEARCH_API_KEY";
const GOOGLE_API_KEY_ENV_LEGACY: &str = "GOOGLE_SEARCH_API_KEY";
const GOOGLE_ENGINE_ID_ENV_DEFAULT: &str = "AUTONOETIC_GOOGLE_SEARCH_ENGINE_ID";
const GOOGLE_ENGINE_ID_ENV_LEGACY: &str = "GOOGLE_SEARCH_ENGINE_ID";
const GOOGLE_ENGINE_ID_ENV_LEGACY_ALT: &str = "GOOGLE_SEARCH_CX";
const WEB_SEARCH_CACHE_TTL_DEFAULT_SECS: u64 = 120;
const WEB_SEARCH_CACHE_TTL_MAX_SECS: u64 = 3_600;

#[derive(Debug, Clone)]
struct WebSearchCacheEntry {
    expires_at: Instant,
    payload: serde_json::Value,
}

static WEB_SEARCH_CACHE: LazyLock<Mutex<HashMap<String, WebSearchCacheEntry>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum WebSearchProvider {
    Auto,
    DuckDuckGo,
    Google,
}

impl WebSearchProvider {
    fn as_str(self) -> &'static str {
        match self {
            Self::Auto => "auto",
            Self::DuckDuckGo => "duckduckgo",
            Self::Google => "google",
        }
    }
}

fn parse_web_search_provider(raw: Option<&str>) -> anyhow::Result<WebSearchProvider> {
    let normalized = raw
        .map(|value| value.trim().to_ascii_lowercase())
        .unwrap_or_else(|| "auto".to_string());
    match normalized.as_str() {
        "auto" => Ok(WebSearchProvider::Auto),
        "duckduckgo" | "ddg" => Ok(WebSearchProvider::DuckDuckGo),
        "google" => Ok(WebSearchProvider::Google),
        other => Err(anyhow::Error::from(tagged::Tagged::validation(
            anyhow::anyhow!(
                "Unsupported web.search provider '{}'. Use 'auto', 'duckduckgo', or 'google'.",
                other
            ),
        ))),
    }
}

fn resolve_duckduckgo_engine_url(args: &WebSearchArgs) -> String {
    args.duckduckgo_engine_url
        .as_ref()
        .map(|value| value.trim())
        .filter(|value| !value.is_empty())
        .map(|value| value.to_string())
        .or_else(|| {
            args.engine_url
                .as_ref()
                .map(|value| value.trim())
                .filter(|value| !value.is_empty())
                .map(|value| value.to_string())
        })
        .unwrap_or_else(default_web_search_engine_url)
}

fn resolve_google_engine_url(args: &WebSearchArgs) -> String {
    args.google_engine_url
        .as_ref()
        .map(|value| value.trim())
        .filter(|value| !value.is_empty())
        .map(|value| value.to_string())
        .or_else(|| {
            args.engine_url
                .as_ref()
                .map(|value| value.trim())
                .filter(|value| !value.is_empty())
                .map(|value| value.to_string())
        })
        .unwrap_or_else(default_google_search_engine_url)
}

fn resolve_web_search_cache_ttl_secs(args: &WebSearchArgs) -> u64 {
    args.cache_ttl_secs
        .unwrap_or(WEB_SEARCH_CACHE_TTL_DEFAULT_SECS)
        .min(WEB_SEARCH_CACHE_TTL_MAX_SECS)
}

fn web_search_cache_key(
    args: &WebSearchArgs,
    provider: WebSearchProvider,
    requested_max_results: usize,
    timeout_secs: u64,
) -> String {
    let query = args.query.trim();
    let ddg_engine_url = resolve_duckduckgo_engine_url(args);
    let google_engine_url = resolve_google_engine_url(args);
    let google_engine_id = args
        .google_engine_id
        .as_ref()
        .map(|value| value.trim())
        .filter(|value| !value.is_empty())
        .unwrap_or("");
    let google_api_key_env = args
        .google_api_key_env
        .as_deref()
        .unwrap_or(GOOGLE_API_KEY_ENV_DEFAULT);
    let google_engine_id_env = args
        .google_engine_id_env
        .as_deref()
        .unwrap_or(GOOGLE_ENGINE_ID_ENV_DEFAULT);
    format!(
        "provider={}|query={}|max_results={}|timeout_secs={}|ddg_engine_url={}|google_engine_url={}|google_engine_id={}|google_api_key_env={}|google_engine_id_env={}",
        provider.as_str(),
        query,
        requested_max_results,
        timeout_secs,
        ddg_engine_url,
        google_engine_url,
        google_engine_id,
        google_api_key_env,
        google_engine_id_env
    )
}

fn web_search_cache_get(key: &str) -> Option<serde_json::Value> {
    let now = Instant::now();
    let mut cache = WEB_SEARCH_CACHE.lock().ok()?;
    cache.retain(|_, entry| entry.expires_at > now);
    cache.get(key).map(|entry| entry.payload.clone())
}

fn web_search_cache_put(key: String, payload: serde_json::Value, ttl_secs: u64) {
    if ttl_secs == 0 {
        return;
    }
    if let Ok(mut cache) = WEB_SEARCH_CACHE.lock() {
        let now = Instant::now();
        cache.retain(|_, entry| entry.expires_at > now);
        cache.insert(
            key,
            WebSearchCacheEntry {
                expires_at: now + StdDuration::from_secs(ttl_secs),
                payload,
            },
        );
    }
}

fn non_empty_env(name: &str) -> Option<String> {
    std::env::var(name).ok().and_then(|value| {
        let trimmed = value.trim();
        if trimmed.is_empty() {
            None
        } else {
            Some(trimmed.to_string())
        }
    })
}

fn resolve_google_api_key(args: &WebSearchArgs) -> anyhow::Result<String> {
    let key_env = args
        .google_api_key_env
        .as_deref()
        .unwrap_or(GOOGLE_API_KEY_ENV_DEFAULT);
    let key = non_empty_env(key_env).or_else(|| {
        if args.google_api_key_env.is_none() {
            non_empty_env(GOOGLE_API_KEY_ENV_LEGACY)
        } else {
            None
        }
    });
    key.ok_or_else(|| {
        anyhow::Error::from(tagged::Tagged::validation(anyhow::anyhow!(
            "Google web.search requires API key env '{}'",
            key_env
        )))
    })
}

fn resolve_google_engine_id(args: &WebSearchArgs) -> anyhow::Result<String> {
    if let Some(explicit) = args
        .google_engine_id
        .as_ref()
        .map(|value| value.trim())
        .filter(|value| !value.is_empty())
    {
        return Ok(explicit.to_string());
    }
    let engine_id_env = args
        .google_engine_id_env
        .as_deref()
        .unwrap_or(GOOGLE_ENGINE_ID_ENV_DEFAULT);
    let engine_id = non_empty_env(engine_id_env).or_else(|| {
        if args.google_engine_id_env.is_none() {
            non_empty_env(GOOGLE_ENGINE_ID_ENV_LEGACY)
                .or_else(|| non_empty_env(GOOGLE_ENGINE_ID_ENV_LEGACY_ALT))
        } else {
            None
        }
    });
    engine_id.ok_or_else(|| {
        anyhow::Error::from(tagged::Tagged::validation(anyhow::anyhow!(
            "Google web.search requires engine id via argument 'google_engine_id' or env '{}'",
            engine_id_env
        )))
    })
}

fn normalize_text(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn collect_duckduckgo_results(
    payload: &serde_json::Value,
    max_results: usize,
) -> Vec<serde_json::Value> {
    fn maybe_push(
        out: &mut Vec<serde_json::Value>,
        seen_urls: &mut HashSet<String>,
        text: &str,
        url: &str,
        max_results: usize,
    ) {
        if out.len() >= max_results {
            return;
        }
        if text.trim().is_empty() || url.trim().is_empty() {
            return;
        }
        if !seen_urls.insert(url.to_string()) {
            return;
        }
        out.push(serde_json::json!({
            "title": normalize_text(text),
            "url": url,
            "snippet": normalize_text(text),
        }));
    }

    fn walk(
        node: &serde_json::Value,
        out: &mut Vec<serde_json::Value>,
        seen_urls: &mut HashSet<String>,
        max_results: usize,
    ) {
        if out.len() >= max_results {
            return;
        }

        if let Some(obj) = node.as_object() {
            if let (Some(text), Some(url)) = (
                obj.get("Text").and_then(|v| v.as_str()),
                obj.get("FirstURL").and_then(|v| v.as_str()),
            ) {
                maybe_push(out, seen_urls, text, url, max_results);
            }
            if let Some(topics) = obj.get("Topics").and_then(|v| v.as_array()) {
                for topic in topics {
                    walk(topic, out, seen_urls, max_results);
                    if out.len() >= max_results {
                        return;
                    }
                }
            }
            return;
        }

        if let Some(arr) = node.as_array() {
            for item in arr {
                walk(item, out, seen_urls, max_results);
                if out.len() >= max_results {
                    return;
                }
            }
        }
    }

    let mut out = Vec::new();
    let mut seen_urls = HashSet::new();

    if let Some(results) = payload.get("Results").and_then(|v| v.as_array()) {
        for result in results {
            walk(result, &mut out, &mut seen_urls, max_results);
            if out.len() >= max_results {
                return out;
            }
        }
    }
    if let Some(related) = payload.get("RelatedTopics").and_then(|v| v.as_array()) {
        for topic in related {
            walk(topic, &mut out, &mut seen_urls, max_results);
            if out.len() >= max_results {
                return out;
            }
        }
    }
    out
}

fn collect_google_results(
    payload: &serde_json::Value,
    max_results: usize,
) -> Vec<serde_json::Value> {
    let mut out = Vec::new();
    let mut seen_urls = HashSet::new();
    if let Some(items) = payload.get("items").and_then(|v| v.as_array()) {
        for item in items {
            if out.len() >= max_results {
                break;
            }
            let title = item
                .get("title")
                .and_then(|v| v.as_str())
                .unwrap_or_default();
            let url = item
                .get("link")
                .and_then(|v| v.as_str())
                .unwrap_or_default();
            let snippet = item
                .get("snippet")
                .and_then(|v| v.as_str())
                .unwrap_or_default();
            if title.trim().is_empty() || url.trim().is_empty() {
                continue;
            }
            if !seen_urls.insert(url.to_string()) {
                continue;
            }
            out.push(serde_json::json!({
                "title": normalize_text(title),
                "url": url,
                "snippet": normalize_text(snippet),
            }));
        }
    }
    out
}

#[derive(Debug)]
struct WebSearchResponse {
    provider: WebSearchProvider,
    engine_url: String,
    status_code: u16,
    results: Vec<serde_json::Value>,
    abstract_text: Option<String>,
    total_results: Option<u64>,
}

fn execute_duckduckgo_search(
    policy: &PolicyEngine,
    query: &str,
    engine_url: String,
    max_results: usize,
    timeout_secs: u64,
) -> anyhow::Result<WebSearchResponse> {
    let engine_host = extract_host(&engine_url)?;
    if !policy.can_connect_net(&engine_host) {
        return Err(anyhow::Error::from(tagged::Tagged::permission(
            anyhow::anyhow!(
                "Permission Denied: NetConnect does not allow host '{}'",
                engine_host
            ),
        )));
    }

    let request_engine_url = engine_url.clone();
    let request_query = query.to_string();
    let (status_code, payload) = block_on_http(async move {
        let mut request_url = reqwest::Url::parse(&request_engine_url).map_err(|e| {
            anyhow::Error::from(tagged::Tagged::validation(anyhow::anyhow!(
                "Invalid search engine URL '{}': {}",
                request_engine_url,
                e
            )))
        })?;
        {
            let mut pairs = request_url.query_pairs_mut();
            pairs.append_pair("q", request_query.as_str());
            pairs.append_pair("format", "json");
            pairs.append_pair("no_html", "1");
            pairs.append_pair("skip_disambig", "1");
        }

        let client = reqwest::Client::builder()
            .redirect(reqwest::redirect::Policy::none())
            .build()
            .map_err(|e| anyhow::anyhow!("web.search client build failed: {}", e))?;
        let response = client
            .get(request_url)
            .timeout(StdDuration::from_secs(timeout_secs))
            .send()
            .await
            .map_err(|e| {
                anyhow::Error::from(tagged::Tagged::resource(anyhow::anyhow!(
                    "web.search request failed: {}",
                    e
                )))
            })?;

        let status = response.status();
        if !status.is_success() {
            return Err(anyhow::Error::from(tagged::Tagged::resource(
                anyhow::anyhow!("web.search request failed with status {}", status),
            )));
        }
        let payload = response.json::<serde_json::Value>().await.map_err(|e| {
            anyhow::Error::from(tagged::Tagged::execution(anyhow::anyhow!(
                "web.search could not decode JSON response: {}",
                e
            )))
        })?;
        Ok((status.as_u16(), payload))
    })?;

    let results = collect_duckduckgo_results(&payload, max_results);
    let abstract_text = payload
        .get("AbstractText")
        .and_then(|v| v.as_str())
        .map(normalize_text)
        .filter(|text| !text.is_empty());

    Ok(WebSearchResponse {
        provider: WebSearchProvider::DuckDuckGo,
        engine_url,
        status_code,
        results,
        abstract_text,
        total_results: None,
    })
}

fn execute_google_search(
    policy: &PolicyEngine,
    query: &str,
    engine_url: String,
    api_key: String,
    engine_id: String,
    max_results: usize,
    timeout_secs: u64,
) -> anyhow::Result<WebSearchResponse> {
    let engine_host = extract_host(&engine_url)?;
    if !policy.can_connect_net(&engine_host) {
        return Err(anyhow::Error::from(tagged::Tagged::permission(
            anyhow::anyhow!(
                "Permission Denied: NetConnect does not allow host '{}'",
                engine_host
            ),
        )));
    }

    let request_engine_url = engine_url.clone();
    let request_query = query.to_string();
    let (status_code, payload) = block_on_http(async move {
        let mut request_url = reqwest::Url::parse(&request_engine_url).map_err(|e| {
            anyhow::Error::from(tagged::Tagged::validation(anyhow::anyhow!(
                "Invalid search engine URL '{}': {}",
                request_engine_url,
                e
            )))
        })?;
        {
            let mut pairs = request_url.query_pairs_mut();
            pairs.append_pair("q", request_query.as_str());
            pairs.append_pair("key", api_key.as_str());
            pairs.append_pair("cx", engine_id.as_str());
            pairs.append_pair("num", &max_results.to_string());
        }

        let client = reqwest::Client::builder()
            .redirect(reqwest::redirect::Policy::none())
            .build()
            .map_err(|e| anyhow::anyhow!("web.search client build failed: {}", e))?;
        let response = client
            .get(request_url)
            .timeout(StdDuration::from_secs(timeout_secs))
            .send()
            .await
            .map_err(|e| {
                anyhow::Error::from(tagged::Tagged::resource(anyhow::anyhow!(
                    "web.search request failed: {}",
                    e
                )))
            })?;
        let status = response.status();
        if !status.is_success() {
            return Err(anyhow::Error::from(tagged::Tagged::resource(
                anyhow::anyhow!("web.search request failed with status {}", status),
            )));
        }
        let payload = response.json::<serde_json::Value>().await.map_err(|e| {
            anyhow::Error::from(tagged::Tagged::execution(anyhow::anyhow!(
                "web.search could not decode JSON response: {}",
                e
            )))
        })?;
        Ok((status.as_u16(), payload))
    })?;

    if let Some(error_payload) = payload.get("error") {
        let message = error_payload
            .get("message")
            .and_then(|v| v.as_str())
            .unwrap_or("unknown google search error");
        return Err(anyhow::Error::from(tagged::Tagged::execution(
            anyhow::anyhow!("web.search google provider returned error: {}", message),
        )));
    }

    let results = collect_google_results(&payload, max_results);
    let total_results = payload
        .pointer("/searchInformation/totalResults")
        .and_then(|v| v.as_str())
        .and_then(|value| value.parse::<u64>().ok());

    Ok(WebSearchResponse {
        provider: WebSearchProvider::Google,
        engine_url,
        status_code,
        results,
        abstract_text: None,
        total_results,
    })
}

fn web_search_response_to_payload(query: &str, response: WebSearchResponse) -> serde_json::Value {
    let mut payload = serde_json::json!({
        "ok": true,
        "provider": response.provider.as_str(),
        "query": query,
        "engine_url": response.engine_url,
        "status_code": response.status_code,
        "result_count": response.results.len(),
        "results": response.results
    });
    if let Some(abstract_text) = response.abstract_text {
        payload["abstract"] = serde_json::json!(abstract_text);
    }
    if let Some(total_results) = response.total_results {
        payload["total_results"] = serde_json::json!(total_results);
    }
    payload
}

pub struct WebSearchTool;

impl NativeTool for WebSearchTool {
    fn name(&self) -> &'static str {
        "web.search"
    }

    fn is_available(&self, manifest: &AgentManifest) -> bool {
        manifest
            .capabilities
            .iter()
            .any(|cap| matches!(cap, Capability::NetConnect { .. }))
    }

    fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: self.name().to_string(),
            description:
                "Search the web via provider-backed JSON APIs (duckduckgo, google, or auto fallback)."
                    .to_string(),
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "query": { "type": "string" },
                    "provider": { "type": "string", "enum": ["auto", "duckduckgo", "google"] },
                    "max_results": { "type": "integer", "minimum": 1, "maximum": 20 },
                    "timeout_secs": { "type": "integer", "minimum": 5, "maximum": 120 },
                    "engine_url": { "type": "string" },
                    "duckduckgo_engine_url": { "type": "string" },
                    "google_engine_url": { "type": "string" },
                    "google_engine_id": { "type": "string" },
                    "google_api_key_env": { "type": "string" },
                    "google_engine_id_env": { "type": "string" },
                    "cache_ttl_secs": { "type": "integer", "minimum": 0, "maximum": 3600 }
                },
                "required": ["query"],
                "additionalProperties": false
            }),
        }
    }

    fn execute(
        &self,
        _manifest: &AgentManifest,
        policy: &PolicyEngine,
        _agent_dir: &Path,
        _gateway_dir: Option<&Path>,
        arguments_json: &str,
        _session_id: Option<&str>,
        _turn_id: Option<&str>,
    ) -> anyhow::Result<String> {
        let args: WebSearchArgs = serde_json::from_str(arguments_json)
            .map_err(|e| anyhow::anyhow!("Invalid JSON arguments for '{}': {}", self.name(), e))?;

        anyhow::ensure!(!args.query.trim().is_empty(), "query must not be empty");
        let query = args.query.trim().to_string();
        let requested_provider = parse_web_search_provider(args.provider.as_deref())?;
        let timeout_secs = args.timeout_secs.unwrap_or(20).clamp(5, 120);
        let requested_max_results = args.max_results.unwrap_or(5).clamp(1, 20);
        let cache_ttl_secs = resolve_web_search_cache_ttl_secs(&args);
        let cache_key = web_search_cache_key(
            &args,
            requested_provider,
            requested_max_results,
            timeout_secs,
        );

        if cache_ttl_secs > 0 {
            if let Some(mut cached_payload) = web_search_cache_get(&cache_key) {
                cached_payload["cache_hit"] = serde_json::json!(true);
                cached_payload["cache_ttl_secs"] = serde_json::json!(cache_ttl_secs);
                return serde_json::to_string(&cached_payload).map_err(Into::into);
            }
        }

        let mut attempted_providers = Vec::new();
        let mut fallback_reason: Option<String> = None;

        let response = match requested_provider {
            WebSearchProvider::DuckDuckGo => {
                attempted_providers.push(WebSearchProvider::DuckDuckGo.as_str().to_string());
                execute_duckduckgo_search(
                    policy,
                    &query,
                    resolve_duckduckgo_engine_url(&args),
                    requested_max_results.clamp(1, 20),
                    timeout_secs,
                )?
            }
            WebSearchProvider::Google => {
                attempted_providers.push(WebSearchProvider::Google.as_str().to_string());
                let api_key = resolve_google_api_key(&args)?;
                let engine_id = resolve_google_engine_id(&args)?;
                execute_google_search(
                    policy,
                    &query,
                    resolve_google_engine_url(&args),
                    api_key,
                    engine_id,
                    requested_max_results.clamp(1, 10),
                    timeout_secs,
                )?
            }
            WebSearchProvider::Auto => {
                let ddg_engine_url = resolve_duckduckgo_engine_url(&args);
                let google_engine_url = resolve_google_engine_url(&args);
                let ddg_max_results = requested_max_results.clamp(1, 20);
                let google_max_results = requested_max_results.clamp(1, 10);

                let google_credentials = resolve_google_api_key(&args).and_then(|api_key| {
                    resolve_google_engine_id(&args).map(|engine_id| (api_key, engine_id))
                });

                match google_credentials {
                    Ok((api_key, engine_id)) => {
                        attempted_providers.push(WebSearchProvider::Google.as_str().to_string());
                        match execute_google_search(
                            policy,
                            &query,
                            google_engine_url,
                            api_key,
                            engine_id,
                            google_max_results,
                            timeout_secs,
                        ) {
                            Ok(google_response) if !google_response.results.is_empty() => {
                                google_response
                            }
                            Ok(_) => {
                                fallback_reason = Some("google returned no results".to_string());
                                attempted_providers
                                    .push(WebSearchProvider::DuckDuckGo.as_str().to_string());
                                execute_duckduckgo_search(
                                    policy,
                                    &query,
                                    ddg_engine_url,
                                    ddg_max_results,
                                    timeout_secs,
                                )?
                            }
                            Err(google_err) => {
                                let google_error_text = google_err.to_string();
                                fallback_reason =
                                    Some(format!("google provider failed: {google_error_text}"));
                                attempted_providers
                                    .push(WebSearchProvider::DuckDuckGo.as_str().to_string());
                                match execute_duckduckgo_search(
                                    policy,
                                    &query,
                                    ddg_engine_url,
                                    ddg_max_results,
                                    timeout_secs,
                                ) {
                                    Ok(ddg_response) => ddg_response,
                                    Err(ddg_err) => {
                                        return Err(anyhow::Error::from(tagged::Tagged::resource(
                                            anyhow::anyhow!(
                                                "web.search auto provider failed: google error: {}; duckduckgo error: {}",
                                                google_error_text,
                                                ddg_err
                                            ),
                                        )));
                                    }
                                }
                            }
                        }
                    }
                    Err(_) => {
                        fallback_reason =
                            Some("google credentials unavailable; used duckduckgo".to_string());
                        attempted_providers
                            .push(WebSearchProvider::DuckDuckGo.as_str().to_string());
                        execute_duckduckgo_search(
                            policy,
                            &query,
                            ddg_engine_url,
                            ddg_max_results,
                            timeout_secs,
                        )?
                    }
                }
            }
        };

        let mut payload = web_search_response_to_payload(&query, response);
        payload["requested_provider"] = serde_json::json!(requested_provider.as_str());
        payload["attempted_providers"] = serde_json::json!(attempted_providers);
        if let Some(reason) = fallback_reason {
            payload["fallback_reason"] = serde_json::json!(reason);
        }
        payload["cache_hit"] = serde_json::json!(false);
        payload["cache_ttl_secs"] = serde_json::json!(cache_ttl_secs);

        if cache_ttl_secs > 0 {
            web_search_cache_put(cache_key, payload.clone(), cache_ttl_secs);
        }

        serde_json::to_string(&payload).map_err(Into::into)
    }
}

// ---------------------------------------------------------------------------
// Web Fetch Tool
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize)]
struct WebFetchArgs {
    url: String,
    #[serde(default)]
    timeout_secs: Option<u64>,
    #[serde(default)]
    max_chars: Option<usize>,
}

pub struct WebFetchTool;

impl NativeTool for WebFetchTool {
    fn name(&self) -> &'static str {
        "web.fetch"
    }

    fn is_available(&self, manifest: &AgentManifest) -> bool {
        manifest
            .capabilities
            .iter()
            .any(|cap| matches!(cap, Capability::NetConnect { .. }))
    }

    fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: self.name().to_string(),
            description: "Fetch a web page by URL and return its textual payload.".to_string(),
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "url": { "type": "string" },
                    "timeout_secs": { "type": "integer", "minimum": 5, "maximum": 120 },
                    "max_chars": { "type": "integer", "minimum": 512, "maximum": 200000 }
                },
                "required": ["url"],
                "additionalProperties": false
            }),
        }
    }

    fn execute(
        &self,
        _manifest: &AgentManifest,
        policy: &PolicyEngine,
        _agent_dir: &Path,
        _gateway_dir: Option<&Path>,
        arguments_json: &str,
        _session_id: Option<&str>,
        _turn_id: Option<&str>,
    ) -> anyhow::Result<String> {
        let args: WebFetchArgs = serde_json::from_str(arguments_json)
            .map_err(|e| anyhow::anyhow!("Invalid JSON arguments for '{}': {}", self.name(), e))?;

        anyhow::ensure!(!args.url.trim().is_empty(), "url must not be empty");
        let host = extract_host(&args.url)?;
        if !policy.can_connect_net(&host) {
            return Err(anyhow::Error::from(tagged::Tagged::permission(
                anyhow::anyhow!(
                    "Permission Denied: NetConnect does not allow host '{}'",
                    host
                ),
            )));
        }

        let timeout_secs = args.timeout_secs.unwrap_or(20).clamp(5, 120);
        let max_chars = args.max_chars.unwrap_or(20_000).clamp(512, 200_000);
        let fetch_url = args.url.clone();
        let (status_code, content_type, body) = block_on_http(async move {
            let client = reqwest::Client::builder()
                .redirect(reqwest::redirect::Policy::none())
                .build()
                .map_err(|e| anyhow::anyhow!("web.fetch client build failed: {}", e))?;
            let response = client
                .get(&fetch_url)
                .timeout(StdDuration::from_secs(timeout_secs))
                .send()
                .await
                .map_err(|e| {
                    anyhow::Error::from(tagged::Tagged::resource(anyhow::anyhow!(
                        "web.fetch request failed: {}",
                        e
                    )))
                })?;

            let status = response.status();
            if !status.is_success() {
                return Err(anyhow::Error::from(tagged::Tagged::resource(
                    anyhow::anyhow!("web.fetch request failed with status {}", status),
                )));
            }
            let content_type = response
                .headers()
                .get(reqwest::header::CONTENT_TYPE)
                .and_then(|v| v.to_str().ok())
                .map(|v| v.to_string());
            let body = response.text().await.map_err(|e| {
                anyhow::Error::from(tagged::Tagged::execution(anyhow::anyhow!(
                    "web.fetch could not decode text response: {}",
                    e
                )))
            })?;
            Ok((status.as_u16(), content_type, body))
        })?;

        let total_chars = body.chars().count();
        let truncated = total_chars > max_chars;
        let content = if truncated {
            body.chars().take(max_chars).collect::<String>()
        } else {
            body
        };

        serde_json::to_string(&serde_json::json!({
            "ok": true,
            "url": args.url,
            "status_code": status_code,
            "content_type": content_type,
            "truncated": truncated,
            "total_chars": total_chars,
            "content": content
        }))
        .map_err(Into::into)
    }
}

// ---------------------------------------------------------------------------
// Memory Read File Tool
// ---------------------------------------------------------------------------

pub struct MemoryReadTool;

impl NativeTool for MemoryReadTool {
    fn name(&self) -> &'static str {
        "memory.read"
    }

    fn is_available(&self, manifest: &AgentManifest) -> bool {
        manifest
            .capabilities
            .iter()
            .any(|cap| matches!(cap, Capability::MemoryRead { .. }))
    }

    fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: self.name().to_string(),
            description: "Read the contents of a file from the agent's memory state. If the file does not exist and a default_value is provided, the default_value will be returned instead of an error.".to_string(),
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "path": { "type": "string" },
                    "default_value": { "type": "string" }
                },
                "required": ["path"],
                "additionalProperties": false
            }),
        }
    }

    fn execute(
        &self,
        _manifest: &AgentManifest,
        policy: &PolicyEngine,
        agent_dir: &Path,
        _gateway_dir: Option<&Path>,
        arguments_json: &str,
        _session_id: Option<&str>,
        _turn_id: Option<&str>,
    ) -> anyhow::Result<String> {
        #[derive(Deserialize)]
        struct Args {
            path: String,
            default_value: Option<String>,
        }
        let args: Args = serde_json::from_str(arguments_json)
            .map_err(|e| anyhow::anyhow!("Invalid JSON arguments for '{}': {}", self.name(), e))?;

        anyhow::ensure!(!args.path.trim().is_empty(), "path must not be empty");
        anyhow::ensure!(
            policy.can_read_path(&args.path),
            "memory read denied by policy"
        );

        let mem = crate::runtime::memory::Tier1Memory::new(agent_dir)?;
        match mem.read_file(&args.path) {
            Ok(content) => Ok(content),
            Err(e) => {
                if let Some(default) = args.default_value {
                    Ok(default)
                } else {
                    Err(e)
                }
            }
        }
    }

    fn extract_metadata(&self, arguments_json: &str) -> ToolMetadata {
        extract_path_from_args(arguments_json)
    }
}

// ---------------------------------------------------------------------------
// Memory Write File Tool
// ---------------------------------------------------------------------------

pub struct MemoryWriteTool;

impl NativeTool for MemoryWriteTool {
    fn name(&self) -> &'static str {
        "memory.write"
    }

    fn is_available(&self, manifest: &AgentManifest) -> bool {
        manifest
            .capabilities
            .iter()
            .any(|cap| matches!(cap, Capability::MemoryWrite { .. }))
    }

    fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: self.name().to_string(),
            description: "Write content to a file in the agent's memory state".to_string(),
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "path": { "type": "string" },
                    "content": { "type": "string" }
                },
                "required": ["path", "content"],
                "additionalProperties": false
            }),
        }
    }

    fn execute(
        &self,
        _manifest: &AgentManifest,
        policy: &PolicyEngine,
        agent_dir: &Path,
        _gateway_dir: Option<&Path>,
        arguments_json: &str,
        _session_id: Option<&str>,
        _turn_id: Option<&str>,
    ) -> anyhow::Result<String> {
        #[derive(Deserialize)]
        struct Args {
            path: String,
            content: String,
        }
        let args: Args = serde_json::from_str(arguments_json)
            .map_err(|e| anyhow::anyhow!("Invalid JSON arguments for '{}': {}", self.name(), e))?;

        anyhow::ensure!(!args.path.trim().is_empty(), "path must not be empty");
        anyhow::ensure!(
            policy.can_write_path(&args.path),
            "memory write denied by policy"
        );

        let mem = crate::runtime::memory::Tier1Memory::new(agent_dir)?;
        mem.write_file(&args.path, &args.content)?;
        serde_json::to_string(&serde_json::json!({
            "ok": true,
            "bytes_written": args.content.len(),
        }))
        .map_err(Into::into)
    }

    fn extract_metadata(&self, arguments_json: &str) -> ToolMetadata {
        extract_path_from_args(arguments_json)
    }
}

// ---------------------------------------------------------------------------
// Tier 2 Memory Remember Tool (Gateway-managed long-term memory)
// ---------------------------------------------------------------------------

pub struct MemoryRememberTool;

impl NativeTool for MemoryRememberTool {
    fn name(&self) -> &'static str {
        "memory.remember"
    }

    fn is_available(&self, manifest: &AgentManifest) -> bool {
        manifest
            .capabilities
            .iter()
            .any(|cap| matches!(cap, Capability::MemoryWrite { .. }))
    }

    fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: self.name().to_string(),
            description: "Store a fact in long-term memory with full provenance tracking. The memory will be stored in the gateway-managed Tier 2 memory substrate and can be shared across agents with proper authorization.".to_string(),
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "id": { "type": "string", "description": "Unique identifier for this memory" },
                    "scope": { "type": "string", "description": "Scope/namespace for organizing memory (e.g., 'facts', 'preferences', 'context')" },
                    "content": { "type": "string", "description": "The fact or information to remember" },
                    "confidence": { "type": "number", "description": "Confidence score (0.0-1.0) for the fact's reliability" },
                    "tags": { "type": "array", "items": { "type": "string" }, "description": "Tags for categorization" }
                },
                "required": ["id", "scope", "content"],
                "additionalProperties": false
            }),
        }
    }

    fn execute(
        &self,
        manifest: &AgentManifest,
        policy: &PolicyEngine,
        _agent_dir: &Path,
        gateway_dir: Option<&Path>,
        arguments_json: &str,
        session_id: Option<&str>,
        turn_id: Option<&str>,
    ) -> anyhow::Result<String> {
        #[derive(Deserialize)]
        struct Args {
            id: String,
            scope: String,
            content: String,
            #[serde(default)]
            confidence: Option<f64>,
            #[serde(default)]
            tags: Vec<String>,
        }
        let args: Args = serde_json::from_str(arguments_json).map_err(|e| {
            anyhow::Error::from(tagged::Tagged::validation(anyhow::anyhow!(
                "Invalid JSON arguments for '{}': {}",
                self.name(),
                e
            )))
        })?;

        if args.id.trim().is_empty() {
            return Err(tagged::Tagged::validation(anyhow::anyhow!("id must not be empty")).into());
        }
        if args.scope.trim().is_empty() {
            return Err(
                tagged::Tagged::validation(anyhow::anyhow!("scope must not be empty")).into(),
            );
        }
        if args.content.trim().is_empty() {
            return Err(
                tagged::Tagged::validation(anyhow::anyhow!("content must not be empty")).into(),
            );
        }

        // Enforce scope-level policy check
        anyhow::ensure!(
            policy.can_write_memory_scope(&args.scope),
            "Cannot write to scope '{}': not in MemoryWrite.scopes capability",
            args.scope
        );

        let Some(gw_dir) = gateway_dir else {
            return Err(tagged::Tagged::validation(anyhow::anyhow!(
                "Tier 2 memory requires gateway directory to be configured"
            ))
            .into());
        };

        // Build source_ref from session/turn context for proper traceability
        let source_ref = if let Some(sid) = session_id {
            if let Some(tid) = turn_id {
                format!("session:{}:turn:{}", sid, tid)
            } else {
                format!("session:{}", sid)
            }
        } else {
            format!("agent:{}:direct", manifest.agent.id)
        };

        let mem = crate::runtime::memory::Tier2Memory::new(gw_dir, &manifest.agent.id)?;
        let mut memory = mem.remember(
            &args.id,
            &args.scope,
            &manifest.agent.id,
            &source_ref,
            &args.content,
        )?;

        // Apply confidence and tags if provided
        if let Some(conf) = args.confidence {
            memory.confidence = Some(conf);
        }
        if !args.tags.is_empty() {
            memory.tags = args.tags;
        }

        // Persist the updated memory with confidence and tags
        mem.save_memory(&memory)?;

        serde_json::to_string(&serde_json::json!({
            "ok": true,
            "memory_id": memory.memory_id,
            "scope": memory.scope,
            "created_at": memory.created_at,
            "content_hash": memory.content_hash,
            "confidence": memory.confidence,
            "tags": memory.tags,
        }))
        .map_err(Into::into)
    }
}

// ---------------------------------------------------------------------------
// Tier 2 Memory Recall Tool
// ---------------------------------------------------------------------------

pub struct MemoryRecallTool;

impl NativeTool for MemoryRecallTool {
    fn name(&self) -> &'static str {
        "memory.recall"
    }

    fn is_available(&self, manifest: &AgentManifest) -> bool {
        manifest
            .capabilities
            .iter()
            .any(|cap| matches!(cap, Capability::MemoryRead { .. }))
    }

    fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: self.name().to_string(),
            description: "Recall a fact from long-term memory by its ID. Access is controlled by visibility and ACL rules.".to_string(),
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "id": { "type": "string", "description": "The unique identifier of the memory to recall" }
                },
                "required": ["id"],
                "additionalProperties": false
            }),
        }
    }

    fn execute(
        &self,
        manifest: &AgentManifest,
        policy: &PolicyEngine,
        _agent_dir: &Path,
        gateway_dir: Option<&Path>,
        arguments_json: &str,
        _session_id: Option<&str>,
        _turn_id: Option<&str>,
    ) -> anyhow::Result<String> {
        #[derive(Deserialize)]
        struct Args {
            id: String,
        }
        let args: Args = serde_json::from_str(arguments_json).map_err(|e| {
            anyhow::Error::from(tagged::Tagged::validation(anyhow::anyhow!(
                "Invalid JSON arguments for '{}': {}",
                self.name(),
                e
            )))
        })?;

        if args.id.trim().is_empty() {
            return Err(tagged::Tagged::validation(anyhow::anyhow!("id must not be empty")).into());
        }

        let Some(gw_dir) = gateway_dir else {
            return Err(tagged::Tagged::validation(anyhow::anyhow!(
                "Tier 2 memory requires gateway directory to be configured"
            ))
            .into());
        };

        let mem = crate::runtime::memory::Tier2Memory::new(gw_dir, &manifest.agent.id)?;
        let memory = mem.recall(&args.id)?;

        // Enforce scope-level policy check on the recalled memory
        anyhow::ensure!(
            policy.can_read_memory_scope(&memory.scope),
            "Cannot read from scope '{}': not in MemoryRead.scopes capability",
            memory.scope
        );

        serde_json::to_string(&serde_json::json!({
            "ok": true,
            "memory_id": memory.memory_id,
            "scope": memory.scope,
            "content": memory.content,
            "owner_agent_id": memory.owner_agent_id,
            "writer_agent_id": memory.writer_agent_id,
            "source_ref": memory.source_ref,
            "visibility": serde_json::to_value(&memory.visibility)?,
            "created_at": memory.created_at,
            "updated_at": memory.updated_at,
        }))
        .map_err(Into::into)
    }
}

// ---------------------------------------------------------------------------
// Tier 2 Memory Search Tool
// ---------------------------------------------------------------------------

pub struct MemorySearchTool;

impl NativeTool for MemorySearchTool {
    fn name(&self) -> &'static str {
        "memory.search"
    }

    fn is_available(&self, manifest: &AgentManifest) -> bool {
        manifest
            .capabilities
            .iter()
            .any(|cap| matches!(cap, Capability::MemorySearch { .. }))
    }

    fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: self.name().to_string(),
            description: "Search long-term memory by scope and optional query. Returns memories visible to this agent.".to_string(),
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "scope": { "type": "string", "description": "Scope to search within" },
                    "query": { "type": "string", "description": "Optional search query (substring match)" }
                },
                "required": ["scope"],
                "additionalProperties": false
            }),
        }
    }

    fn execute(
        &self,
        manifest: &AgentManifest,
        policy: &PolicyEngine,
        _agent_dir: &Path,
        gateway_dir: Option<&Path>,
        arguments_json: &str,
        _session_id: Option<&str>,
        _turn_id: Option<&str>,
    ) -> anyhow::Result<String> {
        #[derive(Deserialize)]
        struct Args {
            scope: String,
            #[serde(default)]
            query: Option<String>,
        }
        let args: Args = serde_json::from_str(arguments_json).map_err(|e| {
            anyhow::Error::from(tagged::Tagged::validation(anyhow::anyhow!(
                "Invalid JSON arguments for '{}': {}",
                self.name(),
                e
            )))
        })?;

        anyhow::ensure!(!args.scope.trim().is_empty(), "scope must not be empty");

        // Enforce scope-level policy check
        anyhow::ensure!(
            policy.can_search_memory(&args.scope),
            "Cannot search scope '{}': not in MemorySearch.scopes capability",
            args.scope
        );

        let Some(gw_dir) = gateway_dir else {
            return Err(tagged::Tagged::validation(anyhow::anyhow!(
                "Tier 2 memory requires gateway directory to be configured"
            ))
            .into());
        };

        let mem = crate::runtime::memory::Tier2Memory::new(gw_dir, &manifest.agent.id)?;
        let results = mem.search(&args.scope, args.query.as_deref())?;

        serde_json::to_string(&serde_json::json!({
            "ok": true,
            "count": results.len(),
            "memories": results.iter().map(|m| serde_json::json!({
                "memory_id": m.memory_id,
                "scope": m.scope,
                "content": m.content,
                "owner_agent_id": m.owner_agent_id,
                "visibility": serde_json::to_value(&m.visibility).unwrap_or_default(),
            })).collect::<Vec<_>>()
        }))
        .map_err(Into::into)
    }
}

// ---------------------------------------------------------------------------
// Tier 2 Memory Share Tool
// ---------------------------------------------------------------------------

pub struct MemoryShareTool;

impl NativeTool for MemoryShareTool {
    fn name(&self) -> &'static str {
        "memory.share"
    }

    fn is_available(&self, manifest: &AgentManifest) -> bool {
        manifest
            .capabilities
            .iter()
            .any(|cap| matches!(cap, Capability::MemoryShare { .. }))
    }

    fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: self.name().to_string(),
            description: "Share a memory record with specific agents. Requires ownership or write access to the memory.".to_string(),
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "id": { "type": "string", "description": "The memory ID to share" },
                    "with_agents": { "type": "array", "items": { "type": "string" }, "description": "List of agent IDs to share with" }
                },
                "required": ["id", "with_agents"],
                "additionalProperties": false
            }),
        }
    }

    fn execute(
        &self,
        manifest: &AgentManifest,
        policy: &PolicyEngine,
        _agent_dir: &Path,
        gateway_dir: Option<&Path>,
        arguments_json: &str,
        _session_id: Option<&str>,
        _turn_id: Option<&str>,
    ) -> anyhow::Result<String> {
        #[derive(Deserialize)]
        struct Args {
            id: String,
            with_agents: Vec<String>,
        }
        let args: Args = serde_json::from_str(arguments_json)
            .map_err(|e| anyhow::anyhow!("Invalid JSON arguments for '{}': {}", self.name(), e))?;

        anyhow::ensure!(!args.id.trim().is_empty(), "id must not be empty");
        anyhow::ensure!(
            !args.with_agents.is_empty(),
            "with_agents must not be empty"
        );

        // Check if agent is allowed to share with these targets
        for target in &args.with_agents {
            anyhow::ensure!(
                policy.can_share_memory(target),
                "Cannot share memory with agent '{}': not in allowed_targets",
                target
            );
        }

        let Some(gw_dir) = gateway_dir else {
            anyhow::bail!("Tier 2 memory requires gateway directory to be configured");
        };

        let mem = crate::runtime::memory::Tier2Memory::new(gw_dir, &manifest.agent.id)?;
        let memory = mem.share_with(&args.id, args.with_agents.clone())?;

        serde_json::to_string(&serde_json::json!({
            "ok": true,
            "memory_id": memory.memory_id,
            "visibility": "shared",
            "allowed_agents": memory.allowed_agents,
        }))
        .map_err(Into::into)
    }
}

// ---------------------------------------------------------------------------
// Skill Draft Tool
// ---------------------------------------------------------------------------

pub struct SkillDraftTool;

impl NativeTool for SkillDraftTool {
    fn name(&self) -> &'static str {
        "skill.draft"
    }

    fn is_available(&self, manifest: &AgentManifest) -> bool {
        // Relies on MemoryWrite capability as well
        manifest
            .capabilities
            .iter()
            .any(|cap| matches!(cap, Capability::MemoryWrite { .. }))
    }

    fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: self.name().to_string(),
            description: "Draft a new skill by proposing its SKILL.md content. Drafting a skill requires human approval before it is loaded. The path must be in the skills/ directory (e.g., skills/my_skill.md).".to_string(),
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "path": { "type": "string" },
                    "content": { "type": "string" },
                    "evidence_ref": { "type": "string" }
                },
                "required": ["path", "content"],
                "additionalProperties": false
            }),
        }
    }

    fn execute(
        &self,
        _manifest: &AgentManifest,
        policy: &PolicyEngine,
        agent_dir: &Path,
        _gateway_dir: Option<&Path>,
        arguments_json: &str,
        _session_id: Option<&str>,
        _turn_id: Option<&str>,
    ) -> anyhow::Result<String> {
        #[derive(Deserialize)]
        struct Args {
            path: String,
            content: String,
            #[serde(default)]
            evidence_ref: Option<String>,
        }
        let args: Args = serde_json::from_str(arguments_json)
            .map_err(|e| anyhow::anyhow!("Invalid JSON arguments for '{}': {}", self.name(), e))?;

        anyhow::ensure!(!args.path.trim().is_empty(), "path must not be empty");
        anyhow::ensure!(
            args.path.starts_with("skills/"),
            "skill path must begin with skills/"
        );
        anyhow::ensure!(
            policy.can_write_path(&args.path),
            "skill draft write denied by policy"
        );

        persist_reevaluation_state(agent_dir, |state| {
            state.pending_scheduled_action = Some(ScheduledAction::WriteFile {
                path: args.path.clone(),
                content: args.content.clone(),
                requires_approval: true,
                evidence_ref: args.evidence_ref,
            });
        })?;

        serde_json::to_string(&serde_json::json!({
            "ok": true,
            "status": "Skill drafted and queued for approval",
            "path": args.path,
            "bytes_proposed": args.content.len(),
        }))
        .map_err(Into::into)
    }

    fn extract_metadata(&self, arguments_json: &str) -> ToolMetadata {
        extract_path_from_args(arguments_json)
    }
}

// ---------------------------------------------------------------------------
// Agent Install Tool
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize)]
struct InstallAgentFile {
    path: String,
    content: String,
}

fn collect_paths_with_prefix(files: &[InstallAgentFile], prefix: &str) -> Vec<String> {
    files
        .iter()
        .filter_map(|file| file.path.strip_prefix(prefix).map(|_| file.path.clone()))
        .collect()
}

fn ensure_output_contract_section(
    instructions: &str,
    files: &[InstallAgentFile],
    requires_contract: bool,
) -> String {
    if !requires_contract {
        return instructions.trim().to_string();
    }

    let trimmed = instructions.trim();
    if trimmed.to_ascii_lowercase().contains("## output contract") {
        return trimmed.to_string();
    }

    let state_files = collect_paths_with_prefix(files, "state/");
    let history_files = collect_paths_with_prefix(files, "history/");

    let mut section = String::new();
    section.push_str("\n\n## Output Contract\n\n");
    section.push_str("memory_keys:\n");
    section.push_str("- \"\"\n");
    section.push_str("state_files:\n");
    if state_files.is_empty() {
        section.push_str("- \"state/\"\n");
    } else {
        for path in state_files {
            section.push_str(&format!("- \"{}\"\n", path));
        }
    }
    section.push_str("history_files:\n");
    if history_files.is_empty() {
        section.push_str("- \"history/\"\n");
    } else {
        for path in history_files {
            section.push_str(&format!("- \"{}\"\n", path));
        }
    }
    section.push_str("return_schema:\n");
    section.push_str("  type: \"object\"\n");
    section.push_str("  properties:\n");
    section.push_str("    status:\n");
    section.push_str("      type: \"string\"\n");
    section.push_str("long_term_memory_mode: \"sdk_preferred_with_file_fallback\"\n");

    format!("{}{}", trimmed, section)
}

#[derive(Debug, Deserialize)]
struct InstallAgentArgs {
    agent_id: String,
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    description: Option<String>,
    instructions: String,
    #[serde(default)]
    llm_config: Option<LlmConfig>,
    #[serde(default)]
    capabilities: Vec<Capability>,
    #[serde(default)]
    background: Option<BackgroundPolicy>,
    #[serde(default)]
    scheduled_action: Option<serde_json::Value>,
    #[serde(default)]
    files: Vec<InstallAgentFile>,
    #[serde(default)]
    runtime_lock_dependencies: Vec<LockedDependencySet>,
    #[serde(default)]
    promotion_gate: Option<InstallPromotionGate>,
    #[serde(default = "default_true")]
    arm_immediately: bool,
    #[serde(default = "default_true")]
    validate_on_install: bool,
}

#[derive(Debug, Deserialize)]
struct InstallPromotionGate {
    evaluator_pass: bool,
    auditor_pass: bool,
    #[serde(default)]
    override_approval_ref: Option<String>,
}

fn parse_install_scheduled_action(
    value: Option<serde_json::Value>,
) -> anyhow::Result<Option<ScheduledAction>> {
    let Some(value) = value else {
        return Ok(None);
    };

    if let Ok(action) = serde_json::from_value::<ScheduledAction>(value.clone()) {
        return Ok(Some(action));
    }

    let object = value.as_object().ok_or_else(|| {
        anyhow::anyhow!(
            "scheduled_action must be an object describing either a sandbox command or a file write"
        )
    })?;

    let requires_approval = object
        .get("requires_approval")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);
    let evidence_ref = object
        .get("evidence_ref")
        .and_then(|v| v.as_str())
        .map(ToOwned::to_owned);

    if let Some(tool_use) = object.get("tool_use").and_then(|v| v.as_object()) {
        let tool_name = tool_use.get("name").and_then(|v| v.as_str());
        let arguments = tool_use
            .get("arguments")
            .and_then(|v| v.as_object())
            .ok_or_else(|| {
                anyhow::anyhow!("scheduled_action.tool_use.arguments must be an object")
            })?;

        if matches!(
            tool_name,
            Some("sandbox.exec") | Some("sandbox_exec") | None
        ) {
            if let Some(command) = arguments
                .get("command")
                .or_else(|| arguments.get("cmd"))
                .or_else(|| arguments.get("script"))
                .and_then(|v| v.as_str())
            {
                return Ok(Some(ScheduledAction::SandboxExec {
                    command: command.to_string(),
                    dependencies: None,
                    requires_approval,
                    evidence_ref,
                }));
            }
        }

        if matches!(
            tool_name,
            Some("memory.write") | Some("memory_write") | None
        ) {
            let path = arguments.get("path").and_then(|v| v.as_str());
            let content = arguments.get("content").and_then(|v| v.as_str());
            if let (Some(path), Some(content)) = (path, content) {
                return Ok(Some(ScheduledAction::WriteFile {
                    path: path.to_string(),
                    content: content.to_string(),
                    requires_approval,
                    evidence_ref,
                }));
            }
        }

        anyhow::bail!(
            "scheduled_action.tool_use must describe either sandbox.exec with command/cmd/script or memory.write with path/content"
        );
    }

    if let Some(command) = object.get("command").and_then(|v| v.as_str()) {
        let dependencies = object
            .get("dependencies")
            .cloned()
            .map(
                serde_json::from_value::<autonoetic_types::background::ScheduledActionDependencies>,
            )
            .transpose()
            .map_err(|e| anyhow::anyhow!("Invalid scheduled_action.dependencies: {}", e))?;
        return Ok(Some(ScheduledAction::SandboxExec {
            command: command.to_string(),
            dependencies,
            requires_approval,
            evidence_ref,
        }));
    }

    if let Some(script) = object.get("script").and_then(|v| v.as_str()) {
        return Ok(Some(ScheduledAction::SandboxExec {
            command: script.to_string(),
            dependencies: None,
            requires_approval,
            evidence_ref,
        }));
    }

    let path = object.get("path").and_then(|v| v.as_str());
    let content = object.get("content").and_then(|v| v.as_str());
    if let (Some(path), Some(content)) = (path, content) {
        return Ok(Some(ScheduledAction::WriteFile {
            path: path.to_string(),
            content: content.to_string(),
            requires_approval,
            evidence_ref,
        }));
    }

    anyhow::bail!(
        "scheduled_action must include either a tagged 'type', a 'command' or 'script' field, a supported 'tool_use' wrapper, or both 'path' and 'content' fields"
    )
}

fn parse_interval_hint_value(value: &serde_json::Value) -> Option<u64> {
    if let Some(interval_secs) = value.as_u64().filter(|secs| *secs > 0) {
        return Some(interval_secs);
    }

    let text = value.as_str()?.trim();
    if text.is_empty() {
        return None;
    }

    if let Ok(secs) = text.parse::<u64>() {
        return (secs > 0).then_some(secs);
    }

    let lowered = text.to_ascii_lowercase();
    let (digits, multiplier) = if let Some(raw) = lowered.strip_suffix("seconds") {
        (raw, 1_u64)
    } else if let Some(raw) = lowered.strip_suffix("second") {
        (raw, 1_u64)
    } else if let Some(raw) = lowered.strip_suffix("secs") {
        (raw, 1_u64)
    } else if let Some(raw) = lowered.strip_suffix("sec") {
        (raw, 1_u64)
    } else if let Some(raw) = lowered.strip_suffix('s') {
        (raw, 1_u64)
    } else if let Some(raw) = lowered.strip_suffix("minutes") {
        (raw, 60_u64)
    } else if let Some(raw) = lowered.strip_suffix("minute") {
        (raw, 60_u64)
    } else if let Some(raw) = lowered.strip_suffix("mins") {
        (raw, 60_u64)
    } else if let Some(raw) = lowered.strip_suffix("min") {
        (raw, 60_u64)
    } else if let Some(raw) = lowered.strip_suffix('m') {
        (raw, 60_u64)
    } else if let Some(raw) = lowered.strip_suffix("hours") {
        (raw, 3600_u64)
    } else if let Some(raw) = lowered.strip_suffix("hour") {
        (raw, 3600_u64)
    } else if let Some(raw) = lowered.strip_suffix("hrs") {
        (raw, 3600_u64)
    } else if let Some(raw) = lowered.strip_suffix("hr") {
        (raw, 3600_u64)
    } else if let Some(raw) = lowered.strip_suffix('h') {
        (raw, 3600_u64)
    } else {
        return None;
    };

    digits
        .trim()
        .parse::<u64>()
        .ok()
        .filter(|secs| *secs > 0)
        .map(|secs| secs * multiplier)
}

fn scheduled_action_interval_hint(value: &Option<serde_json::Value>) -> Option<u64> {
    value.as_ref().and_then(|raw| {
        raw.get("interval_secs")
            .and_then(parse_interval_hint_value)
            .or_else(|| raw.get("cadence").and_then(parse_interval_hint_value))
    })
}

fn normalize_install_background(
    background: Option<BackgroundPolicy>,
    scheduled_action_raw: &Option<serde_json::Value>,
) -> anyhow::Result<Option<BackgroundPolicy>> {
    let interval_hint = scheduled_action_interval_hint(scheduled_action_raw);
    match (background, interval_hint) {
        (Some(mut background), Some(interval_secs)) => {
            background.enabled = true;
            if background.interval_secs == 0 {
                background.interval_secs = interval_secs;
            }
            Ok(Some(background))
        }
        (Some(background), None) => Ok(Some(background)),
        (None, Some(interval_secs)) => Ok(Some(BackgroundPolicy {
            enabled: true,
            interval_secs,
            mode: BackgroundMode::Deterministic,
            wake_predicates: Default::default(),
            validate_on_install: true,
        })),
        (None, None) => Ok(None),
    }
}

#[derive(Debug, Deserialize)]
struct SpawnAgentArgs {
    agent_id: String,
    message: String,
    #[serde(default)]
    metadata: Option<serde_json::Value>,
    #[serde(default)]
    session_id: Option<String>,
}

pub struct AgentSpawnTool;

impl NativeTool for AgentSpawnTool {
    fn name(&self) -> &'static str {
        "agent.spawn"
    }

    fn is_available(&self, manifest: &AgentManifest) -> bool {
        manifest
            .capabilities
            .iter()
            .any(|cap| matches!(cap, Capability::AgentSpawn { .. }))
    }

    fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: self.name().to_string(),
            description: "Delegate the current task to an existing specialist agent and receive its reply in-session.".to_string(),
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "agent_id": { "type": "string" },
                    "message": { "type": "string" },
                    "metadata": { "type": "object" },
                    "session_id": { "type": "string" }
                },
                "required": ["agent_id", "message"],
                "additionalProperties": false
            }),
        }
    }

    fn execute(
        &self,
        manifest: &AgentManifest,
        _policy: &PolicyEngine,
        agent_dir: &Path,
        _gateway_dir: Option<&Path>,
        arguments_json: &str,
        session_id: Option<&str>,
        _turn_id: Option<&str>,
    ) -> anyhow::Result<String> {
        let args: SpawnAgentArgs = serde_json::from_str(arguments_json)
            .map_err(|e| anyhow::anyhow!("Invalid JSON arguments for '{}': {}", self.name(), e))?;
        validate_agent_id(&args.agent_id)?;
        anyhow::ensure!(!args.message.trim().is_empty(), "message must not be empty");

        let resolved_session_id = args
            .session_id
            .clone()
            .or_else(|| session_id.map(ToOwned::to_owned))
            .unwrap_or_else(|| uuid::Uuid::new_v4().to_string());

        let agents_dir = agent_dir
            .parent()
            .ok_or_else(|| anyhow::anyhow!("Agent directory is missing its agents root parent"))?;
        let config = GatewayConfig {
            agents_dir: agents_dir.to_path_buf(),
            ..GatewayConfig::default()
        };
        let execution = crate::execution::GatewayExecutionService::new(config);

        let source_agent_id = manifest.agent.id.clone();
        let target_agent_id = args.agent_id.clone();
        let kickoff_message = match &args.metadata {
            Some(value) => format!("{}\n\nDelegation metadata: {}", args.message, value),
            None => args.message.clone(),
        };
        let spawn_future = async move {
            execution
                .spawn_agent_once(
                    &target_agent_id,
                    &kickoff_message,
                    &resolved_session_id,
                    Some(&source_agent_id),
                    false,
                    None,
                )
                .await
        };

        let result = if let Ok(handle) = tokio::runtime::Handle::try_current() {
            tokio::task::block_in_place(|| handle.block_on(spawn_future))?
        } else {
            tokio::runtime::Runtime::new()?.block_on(spawn_future)?
        };

        Ok(serde_json::json!({
            "ok": true,
            "status": "agent_spawned",
            "agent_id": result.agent_id,
            "session_id": result.session_id,
            "assistant_reply": result.assistant_reply,
        })
        .to_string())
    }
}

pub struct AgentInstallTool;

impl NativeTool for AgentInstallTool {
    fn name(&self) -> &'static str {
        "agent.install"
    }

    fn is_available(&self, manifest: &AgentManifest) -> bool {
        manifest
            .capabilities
            .iter()
            .any(|cap| matches!(cap, Capability::AgentSpawn { .. }))
    }

    fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: self.name().to_string(),
            description: "Install a specialized child agent by writing its SKILL.md, runtime.lock, files, and optional background schedule. Use this when the agent should create a durable worker or specialist that continues operating after the current chat turn.".to_string(),
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "agent_id": { "type": "string" },
                    "name": { "type": "string" },
                    "description": { "type": "string" },
                    "instructions": { "type": "string" },
                    "llm_config": { "type": "object" },
                    "capabilities": {
                        "type": "array",
                        "items": { "type": "object" }
                    },
                    "background": { "type": "object" },
                    "scheduled_action": { "type": "object" },
                    "files": {
                        "type": "array",
                        "items": {
                            "type": "object",
                            "properties": {
                                "path": { "type": "string" },
                                "content": { "type": "string" }
                            },
                            "required": ["path", "content"],
                            "additionalProperties": false
                        }
                    },
                    "runtime_lock_dependencies": {
                        "type": "array",
                        "items": { "type": "object" }
                    },
                    "promotion_gate": {
                        "type": "object",
                        "properties": {
                            "evaluator_pass": { "type": "boolean" },
                            "auditor_pass": { "type": "boolean" },
                            "override_approval_ref": { "type": "string" }
                        },
                        "required": ["evaluator_pass", "auditor_pass"],
                        "additionalProperties": false
                    },
                    "arm_immediately": { "type": "boolean" },
                    "validate_on_install": { "type": "boolean" }
                },
                "required": ["agent_id", "instructions"],
                "additionalProperties": false
            }),
        }
    }

    fn execute(
        &self,
        manifest: &AgentManifest,
        _policy: &PolicyEngine,
        agent_dir: &Path,
        _gateway_dir: Option<&Path>,
        arguments_json: &str,
        _session_id: Option<&str>,
        _turn_id: Option<&str>,
    ) -> anyhow::Result<String> {
        let args: InstallAgentArgs = serde_json::from_str(arguments_json)
            .map_err(|e| anyhow::anyhow!("Invalid JSON arguments for '{}': {}", self.name(), e))?;
        let scheduled_action = parse_install_scheduled_action(args.scheduled_action.clone())?;
        let background =
            normalize_install_background(args.background.clone(), &args.scheduled_action)?;

        validate_agent_id(&args.agent_id)?;
        anyhow::ensure!(
            !args.instructions.trim().is_empty(),
            "instructions must not be empty"
        );
        if requires_promotion_gate(&manifest.agent.id) {
            let gate = args.promotion_gate.as_ref().ok_or_else(|| {
                tagged::Tagged::validation(anyhow::anyhow!(
                    "agent.install from '{}' requires promotion_gate evidence",
                    manifest.agent.id
                ))
            })?;
            let has_override = gate
                .override_approval_ref
                .as_ref()
                .map(|value| !value.trim().is_empty())
                .unwrap_or(false);
            anyhow::ensure!(
                has_override || (gate.evaluator_pass && gate.auditor_pass),
                "promotion gate failed: set evaluator_pass=true and auditor_pass=true, or provide override_approval_ref"
            );
        }

        let agents_dir = agent_dir
            .parent()
            .ok_or_else(|| anyhow::anyhow!("Agent directory is missing its agents root parent"))?;
        let child_dir = agents_dir.join(&args.agent_id);
        anyhow::ensure!(
            !child_dir.exists(),
            "child agent '{}' already exists",
            args.agent_id
        );

        if scheduled_action.is_some() {
            anyhow::ensure!(
                background.as_ref().map(|bg| bg.enabled).unwrap_or(false),
                "scheduled_action requires background.enabled = true"
            );
        }

        let mut capabilities = args.capabilities.clone();
        if let Some(background) = &background {
            anyhow::ensure!(
                background.interval_secs > 0,
                "background.interval_secs must be > 0"
            );
            let allow_reasoning = matches!(background.mode, BackgroundMode::Reasoning);
            if !capabilities.iter().any(|cap| {
                matches!(
                    cap,
                    Capability::BackgroundReevaluation {
                        min_interval_secs: _,
                        allow_reasoning: existing_allow_reasoning
                    } if *existing_allow_reasoning == allow_reasoning
                )
            }) {
                capabilities.push(Capability::BackgroundReevaluation {
                    min_interval_secs: background.interval_secs,
                    allow_reasoning,
                });
            }
        }

        if let Some(action) = &scheduled_action {
            match action {
                ScheduledAction::SandboxExec { command, .. } => {
                    if !capabilities.iter().any(|cap| {
                        matches!(cap, Capability::ShellExec { patterns } if patterns.iter().any(|pattern| pattern == command))
                    }) {
                        capabilities.push(Capability::ShellExec {
                            patterns: vec![command.clone()],
                        });
                    }
                }
                ScheduledAction::WriteFile { path, .. } => {
                    if !capabilities.iter().any(|cap| {
                        matches!(cap, Capability::MemoryWrite { scopes } if scopes.iter().any(|scope| path.starts_with(scope.trim_end_matches('*'))))
                    }) {
                        capabilities.push(Capability::MemoryWrite {
                            scopes: vec![path.clone()],
                        });
                    }
                }
            }
        }

        let llm_config = args
            .llm_config
            .clone()
            .or_else(|| manifest.llm_config.clone());
        if matches!(
            background.as_ref().map(|bg| &bg.mode),
            Some(BackgroundMode::Reasoning)
        ) {
            anyhow::ensure!(
                llm_config.is_some(),
                "reasoning background agents require llm_config or an inheritable parent llm_config"
            );
        }

        let child_manifest = AgentManifest {
            version: manifest.version.clone(),
            runtime: manifest.runtime.clone(),
            agent: AgentIdentity {
                id: args.agent_id.clone(),
                name: args.name.clone().unwrap_or_else(|| args.agent_id.clone()),
                description: args
                    .description
                    .clone()
                    .unwrap_or_else(|| format!("Specialized agent {}", args.agent_id)),
            },
            capabilities,
            llm_config,
            limits: None,
            background: background.clone(),
            disclosure: None,
        };

        std::fs::create_dir_all(child_dir.join("state"))?;
        std::fs::create_dir_all(child_dir.join("history"))?;
        std::fs::create_dir_all(child_dir.join("skills"))?;
        std::fs::create_dir_all(child_dir.join("scripts"))?;

        let instruction_body = ensure_output_contract_section(
            &args.instructions,
            &args.files,
            scheduled_action.is_some(),
        );
        let skill_yaml = render_skill_frontmatter(&child_manifest)?;
        let skill_body = format!("---\n{}---\n{}\n", skill_yaml, instruction_body);
        std::fs::write(child_dir.join("SKILL.md"), skill_body)?;

        let runtime_lock = RuntimeLock {
            gateway: LockedGateway {
                artifact: "autonoetic-gateway".to_string(),
                version: manifest.runtime.gateway_version.clone(),
                sha256: "unmanaged".to_string(),
                signature: None,
            },
            sdk: LockedSdk {
                version: manifest.runtime.sdk_version.clone(),
            },
            sandbox: LockedSandbox {
                backend: manifest.runtime.sandbox.clone(),
            },
            dependencies: args.runtime_lock_dependencies.clone(),
            artifacts: Vec::new(),
        };
        std::fs::write(
            child_dir.join(&child_manifest.runtime.runtime_lock),
            serde_yaml::to_string(&runtime_lock)?,
        )?;

        for file in &args.files {
            validate_relative_agent_path(&file.path)?;
            anyhow::ensure!(
                file.path != "SKILL.md" && file.path != child_manifest.runtime.runtime_lock,
                "files may not overwrite generated SKILL.md or runtime.lock"
            );
            let target = child_dir.join(&file.path);
            if let Some(parent) = target.parent() {
                std::fs::create_dir_all(parent)?;
            }
            std::fs::write(target, &file.content)?;
        }

        if let Some(action) = scheduled_action.clone() {
            persist_reevaluation_state(&child_dir, |state| {
                state.pending_scheduled_action = Some(action.clone());
                state.last_outcome = Some("installed".to_string());
                state.retry_not_before = None;
            })?;

            if args.validate_on_install {
                let registry = default_registry();
                match execute_scheduled_action(&child_manifest, &child_dir, &action, &registry) {
                    Ok(_) => {
                        persist_reevaluation_state(&child_dir, |state| {
                            state.last_outcome = Some("install_validation_success".to_string());
                        })?;
                    }
                    Err(e) => {
                        persist_reevaluation_state(&child_dir, |state| {
                            state.last_outcome = Some(format!("install_validation_failed:{}", e));
                        })?;
                        let tool_error: autonoetic_types::tool_error::ToolError = e.into();
                        if !tool_error.is_recoverable() {
                            return Err(anyhow::anyhow!(
                                "Fatal install validation error in {}: {}",
                                action.kind(),
                                tool_error.message
                            ));
                        }
                        return serde_json::to_string(&tool_error).map_err(Into::into);
                    }
                }
            }
        }

        if let Some(background) = &child_manifest.background {
            if background.enabled {
                let next_due_at = if args.arm_immediately {
                    Utc::now()
                } else {
                    Utc::now() + Duration::seconds(background.interval_secs.max(1) as i64)
                };
                let background_state = BackgroundState {
                    agent_id: child_manifest.agent.id.clone(),
                    session_id: format!("background::{}", child_manifest.agent.id),
                    next_due_at: Some(next_due_at.to_rfc3339()),
                    ..BackgroundState::default()
                };
                let background_state_path =
                    background_state_file_for_child(agent_dir, &child_manifest.agent.id)?;
                if let Some(parent) = background_state_path.parent() {
                    std::fs::create_dir_all(parent)?;
                }
                std::fs::write(
                    background_state_path,
                    serde_json::to_string_pretty(&background_state)?,
                )?;
            }
        }

        serde_json::to_string(&serde_json::json!({
            "ok": true,
            "status": "agent_installed",
            "agent_id": child_manifest.agent.id,
            "background_enabled": child_manifest.background.as_ref().map(|bg| bg.enabled).unwrap_or(false),
            "scheduled_action_kind": scheduled_action.as_ref().map(|action| action.kind()),
            "arm_immediately": args.arm_immediately,
        }))
        .map_err(Into::into)
    }
}

/// Builds the default registry with the core native tools.

#[derive(Debug, Deserialize)]
pub(crate) struct SandboxExecArgs {
    command: String,
    #[serde(default)]
    dependencies: Option<SandboxExecDependencies>,
}

#[derive(Debug, Deserialize)]
pub struct SandboxExecDependencies {
    pub runtime: String,
    pub packages: Vec<String>,
}

fn dependency_plan_from_args_or_lock(
    manifest: &AgentManifest,
    agent_dir: &Path,
    deps: Option<SandboxExecDependencies>,
) -> anyhow::Result<Option<DependencyPlan>> {
    if let Some(deps) = deps {
        return parse_dependency_plan(deps.runtime.as_str(), deps.packages).map(Some);
    }

    let lock_path = agent_dir.join(&manifest.runtime.runtime_lock);
    if !lock_path.exists() {
        return Ok(None);
    }
    let lock = crate::runtime_lock::resolve_runtime_lock(&lock_path)?;
    if lock.dependencies.is_empty() {
        return Ok(None);
    }
    anyhow::ensure!(
        lock.dependencies.len() == 1,
        "runtime.lock currently supports exactly one dependency set"
    );
    let locked = &lock.dependencies[0];
    parse_dependency_plan(locked.runtime.as_str(), locked.packages.clone()).map(Some)
}
fn parse_dependency_plan(runtime: &str, packages: Vec<String>) -> anyhow::Result<DependencyPlan> {
    let runtime = match runtime.to_ascii_lowercase().as_str() {
        "python" => DependencyRuntime::Python,
        "nodejs" | "node" => DependencyRuntime::NodeJs,
        other => anyhow::bail!("Unsupported dependency runtime '{}'", other),
    };
    anyhow::ensure!(
        !packages.is_empty(),
        "dependency packages must not be empty"
    );
    Ok(DependencyPlan { runtime, packages })
}

pub fn default_registry() -> NativeToolRegistry {
    let mut registry = NativeToolRegistry::new();
    registry.register(Box::new(SandboxExecTool));
    registry.register(Box::new(WebSearchTool));
    registry.register(Box::new(WebFetchTool));
    registry.register(Box::new(MemoryReadTool));
    registry.register(Box::new(MemoryWriteTool));
    registry.register(Box::new(MemoryRememberTool));
    registry.register(Box::new(MemoryRecallTool));
    registry.register(Box::new(MemorySearchTool));
    registry.register(Box::new(MemoryShareTool));
    registry.register(Box::new(SkillDraftTool));
    registry.register(Box::new(AgentSpawnTool));
    registry.register(Box::new(AgentInstallTool));
    registry
}

#[cfg(test)]
mod tests {
    use super::*;
    use autonoetic_types::agent::{AgentIdentity, RuntimeDeclaration};
    use autonoetic_types::capability::Capability;
    use std::io::{Read, Write};
    use std::net::TcpListener;
    use std::sync::{
        atomic::{AtomicUsize, Ordering},
        Arc,
    };
    use std::thread;
    use tempfile::tempdir;

    fn test_manifest(capabilities: Vec<Capability>) -> AgentManifest {
        test_manifest_with_id("test-agent", capabilities)
    }

    fn test_manifest_with_id(agent_id: &str, capabilities: Vec<Capability>) -> AgentManifest {
        AgentManifest {
            version: "1.0".to_string(),
            runtime: RuntimeDeclaration {
                engine: "autonoetic".to_string(),
                gateway_version: "0.1.0".to_string(),
                sdk_version: "0.1.0".to_string(),
                runtime_type: "stateful".to_string(),
                sandbox: "bubblewrap".to_string(),
                runtime_lock: "runtime.lock".to_string(),
            },
            agent: AgentIdentity {
                id: agent_id.to_string(),
                name: agent_id.to_string(),
                description: "test".to_string(),
            },
            capabilities,
            llm_config: None,
            limits: None,
            background: None,
            disclosure: None,
        }
    }

    fn spawn_one_shot_http_server(
        status: &str,
        content_type: &str,
        body: String,
    ) -> (String, thread::JoinHandle<()>) {
        let status = status.to_string();
        let content_type = content_type.to_string();
        let listener = TcpListener::bind("127.0.0.1:0").expect("listener should bind");
        let addr = listener
            .local_addr()
            .expect("listener should expose local addr");
        let handle = thread::spawn(move || {
            if let Ok((mut stream, _)) = listener.accept() {
                let mut request_buf = [0_u8; 2048];
                let _ = stream.read(&mut request_buf);
                let response = format!(
                    "HTTP/1.1 {status}\r\nContent-Type: {content_type}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                    body.len(),
                    body
                );
                let _ = stream.write_all(response.as_bytes());
                let _ = stream.flush();
            }
        });
        (format!("http://{}", addr), handle)
    }

    fn spawn_counting_http_server(
        status: &str,
        content_type: &str,
        body: String,
        expected_requests: usize,
    ) -> (String, Arc<AtomicUsize>, thread::JoinHandle<()>) {
        let status = status.to_string();
        let content_type = content_type.to_string();
        let hits = Arc::new(AtomicUsize::new(0));
        let hits_clone = Arc::clone(&hits);
        let listener = TcpListener::bind("127.0.0.1:0").expect("listener should bind");
        let addr = listener
            .local_addr()
            .expect("listener should expose local addr");
        let handle = thread::spawn(move || {
            for _ in 0..expected_requests {
                if let Ok((mut stream, _)) = listener.accept() {
                    hits_clone.fetch_add(1, Ordering::SeqCst);
                    let mut request_buf = [0_u8; 2048];
                    let _ = stream.read(&mut request_buf);
                    let response = format!(
                        "HTTP/1.1 {status}\r\nContent-Type: {content_type}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                        body.len(),
                        body
                    );
                    let _ = stream.write_all(response.as_bytes());
                    let _ = stream.flush();
                }
            }
        });
        (format!("http://{}", addr), hits, handle)
    }

    #[test]
    fn test_native_tool_registry_availability() {
        let registry = default_registry();
        let manifest_none = test_manifest(vec![]);
        assert_eq!(registry.available_definitions(&manifest_none).len(), 0);

        let manifest_shell = test_manifest(vec![Capability::ShellExec {
            patterns: vec!["*".into()],
        }]);
        let defs = registry.available_definitions(&manifest_shell);
        assert_eq!(defs.len(), 1);
        assert_eq!(defs[0].name, "sandbox.exec");

        let manifest_all = test_manifest(vec![
            Capability::ShellExec { patterns: vec![] },
            Capability::MemoryRead { scopes: vec![] },
            Capability::MemoryWrite { scopes: vec![] },
        ]);
        let defs_all = registry.available_definitions(&manifest_all);
        // sandbox.exec, memory.read, memory.write, memory.remember, memory.recall, skill.draft = 6
        assert_eq!(defs_all.len(), 6);

        let manifest_spawn = test_manifest(vec![Capability::AgentSpawn { max_children: 4 }]);
        let defs_spawn = registry.available_definitions(&manifest_spawn);
        assert_eq!(defs_spawn.len(), 2);
        assert!(defs_spawn.iter().any(|d| d.name == "agent.spawn"));
        assert!(defs_spawn.iter().any(|d| d.name == "agent.install"));

        let manifest_net = test_manifest(vec![Capability::NetConnect {
            hosts: vec!["*".to_string()],
        }]);
        let defs_net = registry.available_definitions(&manifest_net);
        assert_eq!(defs_net.len(), 2);
        assert!(defs_net.iter().any(|d| d.name == "web.search"));
        assert!(defs_net.iter().any(|d| d.name == "web.fetch"));
    }

    #[test]
    fn test_web_fetch_tool_roundtrip_local_server() {
        let manifest = test_manifest(vec![Capability::NetConnect {
            hosts: vec!["127.0.0.1".to_string()],
        }]);
        let policy = PolicyEngine::new(manifest.clone());
        let temp = tempdir().expect("tempdir should create");
        let (base_url, handle) = spawn_one_shot_http_server(
            "200 OK",
            "text/plain; charset=utf-8",
            "hello web fetch".to_string(),
        );

        let args = serde_json::json!({
            "url": format!("{}/doc", base_url),
            "timeout_secs": 10,
            "max_chars": 4096
        });

        let registry = default_registry();
        let result = registry
            .execute(
                "web.fetch",
                &manifest,
                &policy,
                temp.path(),
                None,
                &serde_json::to_string(&args).expect("json should encode"),
                None,
                None,
            )
            .expect("web.fetch should succeed");

        let parsed: serde_json::Value =
            serde_json::from_str(&result).expect("web.fetch result should decode");
        assert_eq!(parsed.get("ok"), Some(&serde_json::json!(true)));
        assert!(parsed
            .get("content")
            .and_then(|v| v.as_str())
            .unwrap_or_default()
            .contains("hello web fetch"));

        handle.join().expect("server thread should join");
    }

    #[test]
    fn test_web_fetch_tool_denied_by_netconnect_policy() {
        let manifest = test_manifest(vec![Capability::NetConnect {
            hosts: vec!["example.com".to_string()],
        }]);
        let policy = PolicyEngine::new(manifest.clone());
        let temp = tempdir().expect("tempdir should create");

        let args = serde_json::json!({
            "url": "http://127.0.0.1:65535/forbidden"
        });

        let registry = default_registry();
        let err = registry
            .execute(
                "web.fetch",
                &manifest,
                &policy,
                temp.path(),
                None,
                &serde_json::to_string(&args).expect("json should encode"),
                None,
                None,
            )
            .expect_err("web.fetch should be denied");
        assert!(err.to_string().contains("NetConnect"));
    }

    #[test]
    fn test_web_search_tool_denied_by_netconnect_policy() {
        let manifest = test_manifest(vec![Capability::NetConnect {
            hosts: vec!["example.com".to_string()],
        }]);
        let policy = PolicyEngine::new(manifest.clone());
        let temp = tempdir().expect("tempdir should create");

        let args = serde_json::json!({
            "query": "rust",
            "engine_url": "http://127.0.0.1:65535/search"
        });

        let registry = default_registry();
        let err = registry
            .execute(
                "web.search",
                &manifest,
                &policy,
                temp.path(),
                None,
                &serde_json::to_string(&args).expect("json should encode"),
                None,
                None,
            )
            .expect_err("web.search should be denied");
        assert!(err.to_string().contains("NetConnect"));
    }

    #[test]
    fn test_web_search_tool_roundtrip_local_engine() {
        let manifest = test_manifest(vec![Capability::NetConnect {
            hosts: vec!["127.0.0.1".to_string()],
        }]);
        let policy = PolicyEngine::new(manifest.clone());
        let temp = tempdir().expect("tempdir should create");
        let body = serde_json::json!({
            "Results": [],
            "RelatedTopics": [
                {
                    "Text": "Rust language homepage",
                    "FirstURL": "https://www.rust-lang.org/"
                },
                {
                    "Name": "Docs",
                    "Topics": [
                        {
                            "Text": "The Rust book",
                            "FirstURL": "https://doc.rust-lang.org/book/"
                        }
                    ]
                }
            ]
        })
        .to_string();
        let (engine_url, handle) = spawn_one_shot_http_server("200 OK", "application/json", body);

        let args = serde_json::json!({
            "query": "rust language",
            "provider": "duckduckgo",
            "engine_url": engine_url,
            "max_results": 5
        });

        let registry = default_registry();
        let result = registry
            .execute(
                "web.search",
                &manifest,
                &policy,
                temp.path(),
                None,
                &serde_json::to_string(&args).expect("json should encode"),
                None,
                None,
            )
            .expect("web.search should succeed");

        let parsed: serde_json::Value =
            serde_json::from_str(&result).expect("web.search result should decode");
        assert_eq!(parsed.get("ok"), Some(&serde_json::json!(true)));
        assert!(
            parsed
                .get("result_count")
                .and_then(|v| v.as_u64())
                .unwrap_or(0)
                >= 2
        );

        handle.join().expect("server thread should join");
    }

    #[test]
    fn test_web_search_google_requires_api_key_env() {
        let manifest = test_manifest(vec![Capability::NetConnect {
            hosts: vec!["127.0.0.1".to_string()],
        }]);
        let policy = PolicyEngine::new(manifest.clone());
        let temp = tempdir().expect("tempdir should create");

        let args = serde_json::json!({
            "query": "rust",
            "provider": "google",
            "engine_url": "http://127.0.0.1:65535/search",
            "google_engine_id": "cx-test",
            "google_api_key_env": "AUTONOETIC_TEST_GOOGLE_KEY_MISSING"
        });

        let registry = default_registry();
        let err = registry
            .execute(
                "web.search",
                &manifest,
                &policy,
                temp.path(),
                None,
                &serde_json::to_string(&args).expect("json should encode"),
                None,
                None,
            )
            .expect_err("google search without key should fail");
        assert!(err.to_string().contains("requires API key env"));
    }

    #[test]
    fn test_web_search_google_roundtrip_local_engine() {
        let manifest = test_manifest(vec![Capability::NetConnect {
            hosts: vec!["127.0.0.1".to_string()],
        }]);
        let policy = PolicyEngine::new(manifest.clone());
        let temp = tempdir().expect("tempdir should create");
        let body = serde_json::json!({
            "searchInformation": {
                "totalResults": "123"
            },
            "items": [
                {
                    "title": "Rust language",
                    "link": "https://www.rust-lang.org/",
                    "snippet": "Rust empowers everyone."
                },
                {
                    "title": "The Rust Book",
                    "link": "https://doc.rust-lang.org/book/",
                    "snippet": "Learn Rust."
                }
            ]
        })
        .to_string();
        let (engine_url, handle) = spawn_one_shot_http_server("200 OK", "application/json", body);

        let key_env = "AUTONOETIC_TEST_GOOGLE_KEY_OK";
        let cx_env = "AUTONOETIC_TEST_GOOGLE_CX_OK";
        let prior_key = std::env::var(key_env).ok();
        let prior_cx = std::env::var(cx_env).ok();
        std::env::set_var(key_env, "test-api-key");
        std::env::set_var(cx_env, "test-cx-id");

        let args = serde_json::json!({
            "query": "rust language",
            "provider": "google",
            "engine_url": engine_url,
            "google_api_key_env": key_env,
            "google_engine_id_env": cx_env
        });

        let registry = default_registry();
        let result = registry
            .execute(
                "web.search",
                &manifest,
                &policy,
                temp.path(),
                None,
                &serde_json::to_string(&args).expect("json should encode"),
                None,
                None,
            )
            .expect("google web.search should succeed");

        match prior_key {
            Some(value) => std::env::set_var(key_env, value),
            None => std::env::remove_var(key_env),
        }
        match prior_cx {
            Some(value) => std::env::set_var(cx_env, value),
            None => std::env::remove_var(cx_env),
        }
        handle.join().expect("server thread should join");

        let parsed: serde_json::Value =
            serde_json::from_str(&result).expect("web.search result should decode");
        assert_eq!(parsed.get("ok"), Some(&serde_json::json!(true)));
        assert_eq!(parsed.get("provider"), Some(&serde_json::json!("google")));
        assert_eq!(parsed.get("total_results"), Some(&serde_json::json!(123)));
        assert_eq!(parsed.get("result_count"), Some(&serde_json::json!(2)));
    }

    #[test]
    fn test_web_search_google_legacy_cx_env_alias_roundtrip() {
        let manifest = test_manifest(vec![Capability::NetConnect {
            hosts: vec!["127.0.0.1".to_string()],
        }]);
        let policy = PolicyEngine::new(manifest.clone());
        let temp = tempdir().expect("tempdir should create");

        let body = serde_json::json!({
            "searchInformation": {
                "totalResults": "7"
            },
            "items": [
                {
                    "title": "Example result",
                    "link": "https://example.com/",
                    "snippet": "example"
                }
            ]
        })
        .to_string();
        let (engine_url, handle) = spawn_one_shot_http_server("200 OK", "application/json", body);

        let key_env = "GOOGLE_SEARCH_API_KEY";
        let cx_env = "GOOGLE_SEARCH_CX";
        let prior_key = std::env::var(key_env).ok();
        let prior_cx = std::env::var(cx_env).ok();
        std::env::set_var(key_env, "legacy-test-api-key");
        std::env::set_var(cx_env, "legacy-test-cx");

        let args = serde_json::json!({
            "query": "legacy cx alias",
            "provider": "google",
            "engine_url": engine_url
        });

        let registry = default_registry();
        let result = registry
            .execute(
                "web.search",
                &manifest,
                &policy,
                temp.path(),
                None,
                &serde_json::to_string(&args).expect("json should encode"),
                None,
                None,
            )
            .expect("google web.search should accept GOOGLE_SEARCH_CX legacy alias");

        match prior_key {
            Some(value) => std::env::set_var(key_env, value),
            None => std::env::remove_var(key_env),
        }
        match prior_cx {
            Some(value) => std::env::set_var(cx_env, value),
            None => std::env::remove_var(cx_env),
        }
        handle.join().expect("server thread should join");

        let parsed: serde_json::Value =
            serde_json::from_str(&result).expect("web.search result should decode");
        assert_eq!(parsed.get("ok"), Some(&serde_json::json!(true)));
        assert_eq!(parsed.get("provider"), Some(&serde_json::json!("google")));
        assert_eq!(parsed.get("result_count"), Some(&serde_json::json!(1)));
    }

    #[test]
    fn test_web_search_auto_falls_back_to_duckduckgo_when_google_fails() {
        let manifest = test_manifest(vec![Capability::NetConnect {
            hosts: vec!["127.0.0.1".to_string()],
        }]);
        let policy = PolicyEngine::new(manifest.clone());
        let temp = tempdir().expect("tempdir should create");

        let google_body = serde_json::json!({
            "error": { "message": "quota exceeded" }
        })
        .to_string();
        let (google_engine_url, google_handle) = spawn_one_shot_http_server(
            "500 Internal Server Error",
            "application/json",
            google_body,
        );

        let ddg_body = serde_json::json!({
            "Results": [],
            "RelatedTopics": [
                {
                    "Text": "Rust official site",
                    "FirstURL": "https://www.rust-lang.org/"
                }
            ]
        })
        .to_string();
        let (duckduckgo_engine_url, ddg_handle) =
            spawn_one_shot_http_server("200 OK", "application/json", ddg_body);

        let key_env = "AUTONOETIC_TEST_GOOGLE_KEY_AUTO";
        let cx_env = "AUTONOETIC_TEST_GOOGLE_CX_AUTO";
        let prior_key = std::env::var(key_env).ok();
        let prior_cx = std::env::var(cx_env).ok();
        std::env::set_var(key_env, "test-api-key");
        std::env::set_var(cx_env, "test-cx-id");

        let args = serde_json::json!({
            "query": "rust language",
            "provider": "auto",
            "google_engine_url": google_engine_url,
            "duckduckgo_engine_url": duckduckgo_engine_url,
            "google_api_key_env": key_env,
            "google_engine_id_env": cx_env
        });

        let registry = default_registry();
        let result = registry
            .execute(
                "web.search",
                &manifest,
                &policy,
                temp.path(),
                None,
                &serde_json::to_string(&args).expect("json should encode"),
                None,
                None,
            )
            .expect("auto provider should fall back to duckduckgo");

        match prior_key {
            Some(value) => std::env::set_var(key_env, value),
            None => std::env::remove_var(key_env),
        }
        match prior_cx {
            Some(value) => std::env::set_var(cx_env, value),
            None => std::env::remove_var(cx_env),
        }

        google_handle
            .join()
            .expect("google server thread should join");
        ddg_handle.join().expect("ddg server thread should join");

        let parsed: serde_json::Value =
            serde_json::from_str(&result).expect("web.search result should decode");
        assert_eq!(parsed.get("ok"), Some(&serde_json::json!(true)));
        assert_eq!(
            parsed.get("requested_provider"),
            Some(&serde_json::json!("auto"))
        );
        assert_eq!(
            parsed.get("provider"),
            Some(&serde_json::json!("duckduckgo"))
        );
        let attempted = parsed
            .get("attempted_providers")
            .and_then(|v| v.as_array())
            .cloned()
            .unwrap_or_default();
        assert!(attempted.contains(&serde_json::json!("google")));
        assert!(attempted.contains(&serde_json::json!("duckduckgo")));
        assert!(parsed
            .get("fallback_reason")
            .and_then(|v| v.as_str())
            .unwrap_or_default()
            .contains("google provider failed"));
    }

    #[test]
    fn test_web_search_cache_hits_without_second_network_call() {
        let manifest = test_manifest(vec![Capability::NetConnect {
            hosts: vec!["127.0.0.1".to_string()],
        }]);
        let policy = PolicyEngine::new(manifest.clone());
        let temp = tempdir().expect("tempdir should create");

        let body = serde_json::json!({
            "Results": [],
            "RelatedTopics": [
                {
                    "Text": "Rust language homepage",
                    "FirstURL": "https://www.rust-lang.org/"
                }
            ]
        })
        .to_string();
        let (engine_url, hits, handle) =
            spawn_counting_http_server("200 OK", "application/json", body, 1);

        let unique_query = format!(
            "rust cache {}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("clock should be after unix epoch")
                .as_nanos()
        );
        let args = serde_json::json!({
            "query": unique_query,
            "provider": "duckduckgo",
            "duckduckgo_engine_url": engine_url,
            "cache_ttl_secs": 300
        });

        let registry = default_registry();
        let first = registry
            .execute(
                "web.search",
                &manifest,
                &policy,
                temp.path(),
                None,
                &serde_json::to_string(&args).expect("json should encode"),
                None,
                None,
            )
            .expect("first web.search call should succeed");
        let second = registry
            .execute(
                "web.search",
                &manifest,
                &policy,
                temp.path(),
                None,
                &serde_json::to_string(&args).expect("json should encode"),
                None,
                None,
            )
            .expect("second web.search call should succeed");

        let first_parsed: serde_json::Value =
            serde_json::from_str(&first).expect("first response should decode");
        let second_parsed: serde_json::Value =
            serde_json::from_str(&second).expect("second response should decode");
        assert_eq!(
            first_parsed.get("cache_hit"),
            Some(&serde_json::json!(false))
        );
        assert_eq!(
            second_parsed.get("cache_hit"),
            Some(&serde_json::json!(true))
        );
        assert_eq!(hits.load(Ordering::SeqCst), 1);

        handle.join().expect("server thread should join");
    }

    #[test]
    fn test_agent_spawn_tool_validates_non_empty_message() {
        let manifest = test_manifest(vec![Capability::AgentSpawn { max_children: 2 }]);
        let policy = PolicyEngine::new(manifest.clone());
        let temp = tempdir().expect("tempdir should create");
        let agents_dir = temp.path().join("agents");
        let parent_dir = agents_dir.join("planner.default");
        std::fs::create_dir_all(&parent_dir).expect("parent dir should create");

        let args = serde_json::json!({
            "agent_id": "researcher.default",
            "message": ""
        });

        let registry = default_registry();
        let err = registry
            .execute(
                "agent.spawn",
                &manifest,
                &policy,
                &parent_dir,
                None,
                &serde_json::to_string(&args).expect("json should encode"),
                Some("session-1"),
                None,
            )
            .expect_err("empty message should be rejected");
        assert!(err.to_string().contains("message must not be empty"));
    }

    #[test]
    fn test_agent_spawn_tool_accepts_metadata_argument() {
        let manifest = test_manifest(vec![Capability::AgentSpawn { max_children: 2 }]);
        let policy = PolicyEngine::new(manifest.clone());
        let temp = tempdir().expect("tempdir should create");
        let agents_dir = temp.path().join("agents");
        let parent_dir = agents_dir.join("planner.default");
        std::fs::create_dir_all(&parent_dir).expect("parent dir should create");

        let args = serde_json::json!({
            "agent_id": "researcher.default",
            "message": "",
            "metadata": {
                "delegated_role": "researcher",
                "expected_outputs": ["summary.md", "sources.json"]
            }
        });

        let registry = default_registry();
        let err = registry
            .execute(
                "agent.spawn",
                &manifest,
                &policy,
                &parent_dir,
                None,
                &serde_json::to_string(&args).expect("json should encode"),
                Some("session-1"),
                None,
            )
            .expect_err("empty message should still be rejected");
        assert!(err.to_string().contains("message must not be empty"));
    }

    #[test]
    fn test_agent_install_requires_promotion_gate_for_evolution_roles() {
        let manifest = test_manifest_with_id(
            "specialized_builder.default",
            vec![Capability::AgentSpawn { max_children: 4 }],
        );
        let policy = PolicyEngine::new(manifest.clone());
        let temp = tempdir().expect("tempdir should create");
        let agents_dir = temp.path().join("agents");
        let parent_dir = agents_dir.join("specialized_builder.default");
        std::fs::create_dir_all(&parent_dir).expect("parent dir should create");

        let args = serde_json::json!({
            "agent_id": "child.worker",
            "instructions": "# Child Worker\nDo one job."
        });

        let registry = default_registry();
        let err = registry
            .execute(
                "agent.install",
                &manifest,
                &policy,
                &parent_dir,
                None,
                &serde_json::to_string(&args).expect("json should encode"),
                None,
                None,
            )
            .expect_err("install should require promotion gate");
        assert!(err.to_string().contains("promotion_gate"));
    }

    #[test]
    fn test_agent_install_allows_promotion_gate_for_evolution_roles() {
        let manifest = test_manifest_with_id(
            "specialized_builder.default",
            vec![Capability::AgentSpawn { max_children: 4 }],
        );
        let policy = PolicyEngine::new(manifest.clone());
        let temp = tempdir().expect("tempdir should create");
        let agents_dir = temp.path().join("agents");
        let parent_dir = agents_dir.join("specialized_builder.default");
        std::fs::create_dir_all(&parent_dir).expect("parent dir should create");

        let args = serde_json::json!({
            "agent_id": "child.worker",
            "instructions": "# Child Worker\nDo one job.",
            "promotion_gate": {
                "evaluator_pass": true,
                "auditor_pass": true
            }
        });

        let registry = default_registry();
        let result = registry
            .execute(
                "agent.install",
                &manifest,
                &policy,
                &parent_dir,
                None,
                &serde_json::to_string(&args).expect("json should encode"),
                None,
                None,
            )
            .expect("install should pass with promotion gate");
        assert!(result.contains("\"ok\":true"));
        assert!(agents_dir.join("child.worker").join("SKILL.md").exists());
    }

    #[test]
    fn test_agent_install_tool_creates_background_child_agent() {
        let manifest = test_manifest(vec![Capability::AgentSpawn { max_children: 4 }]);
        let policy = PolicyEngine::new(manifest.clone());
        let temp = tempdir().expect("tempdir should create");
        let agents_dir = temp.path().join("agents");
        let parent_dir = agents_dir.join("builder_agent");
        std::fs::create_dir_all(&parent_dir).expect("parent dir should create");

        let args = serde_json::json!({
            "agent_id": "fib_worker",
            "name": "fib_worker",
            "description": "Computes Fibonacci values on a schedule",
            "instructions": "# Fibonacci Worker\nMaintain the worker assets already installed in this directory.",
            "background": {
                "enabled": true,
                "interval_secs": 20,
                "mode": "deterministic",
                "wake_predicates": {
                    "timer": true,
                    "new_messages": false,
                    "task_completions": false,
                    "queued_work": false,
                    "stale_goals": false,
                    "retryable_failures": false,
                    "approval_resolved": false
                }
            },
            "scheduled_action": {
                "type": "sandbox_exec",
                "command": "python3 scripts/fibonacci_worker.py"
            },
            "validate_on_install": false,
            "files": [
                {
                    "path": "scripts/fibonacci_worker.py",
                    "content": "import json\nfrom pathlib import Path\nstate_path = Path('state/fib.json')\nstate = json.loads(state_path.read_text())\nstate['previous'], state['current'] = state['current'], state['previous'] + state['current']\nstate['index'] += 1\nstate_path.write_text(json.dumps(state))\n"
                },
                {
                    "path": "state/fib.json",
                    "content": "{\"previous\": 0, \"current\": 1, \"index\": 1}"
                }
            ]
        });

        let registry = default_registry();
        registry
            .execute(
                "agent.install",
                &manifest,
                &policy,
                &parent_dir,
                None,
                &serde_json::to_string(&args).expect("json should encode"),
                None,
                None,
            )
            .expect("agent install should succeed");

        let child_dir = agents_dir.join("fib_worker");
        assert!(child_dir.join("SKILL.md").exists());
        assert!(child_dir.join("runtime.lock").exists());
        assert!(child_dir
            .join("scripts")
            .join("fibonacci_worker.py")
            .exists());
        assert!(child_dir.join("state").join("fib.json").exists());
        assert!(child_dir.join("state").join("reevaluation.json").exists());

        let skill = std::fs::read_to_string(child_dir.join("SKILL.md")).expect("skill should read");
        assert!(skill.contains("name: fib_worker"));
        assert!(skill.contains("metadata:\n  autonoetic:"));
        assert!(skill.contains("agent:\n      id: fib_worker"));
        assert!(skill.contains("type: BackgroundReevaluation"));
        assert!(skill.contains("type: ShellExec"));
        assert!(skill.contains("## Output Contract"));

        let background_state = std::fs::read_to_string(
            agents_dir
                .join(".gateway")
                .join("scheduler")
                .join("agents")
                .join("fib_worker.json"),
        )
        .expect("background state should exist");
        assert!(background_state.contains("background::fib_worker"));
    }

    #[test]
    fn test_agent_install_tool_allows_dotted_agent_ids() {
        let manifest = test_manifest(vec![Capability::AgentSpawn { max_children: 2 }]);
        let policy = PolicyEngine::new(manifest.clone());
        let temp = tempdir().expect("tempdir should create");
        let agents_dir = temp.path().join("agents");
        let parent_dir = agents_dir.join("planner.default");
        std::fs::create_dir_all(&parent_dir).expect("parent dir should create");

        let args = serde_json::json!({
            "agent_id": "researcher.default",
            "name": "Researcher Default",
            "description": "Research specialist",
            "instructions": "# Researcher Default\nCollect evidence and summarize it."
        });

        let registry = default_registry();
        registry
            .execute(
                "agent.install",
                &manifest,
                &policy,
                &parent_dir,
                None,
                &serde_json::to_string(&args).expect("json should encode"),
                None,
                None,
            )
            .expect("agent install should accept dotted IDs");

        let child_dir = agents_dir.join("researcher.default");
        assert!(child_dir.join("SKILL.md").exists());
        assert!(child_dir.join("runtime.lock").exists());

        let skill = std::fs::read_to_string(child_dir.join("SKILL.md")).expect("skill should read");
        assert!(skill.contains("id: researcher.default"));
    }

    #[test]
    fn test_agent_install_tool_accepts_scheduled_action_shorthand() {
        let manifest = test_manifest(vec![Capability::AgentSpawn { max_children: 4 }]);
        let policy = PolicyEngine::new(manifest.clone());
        let temp = tempdir().expect("tempdir should create");
        let agents_dir = temp.path().join("agents");
        let parent_dir = agents_dir.join("builder_agent");
        std::fs::create_dir_all(&parent_dir).expect("parent dir should create");

        let args = serde_json::json!({
            "agent_id": "fib_worker",
            "instructions": "# Worker\nInstalled from shorthand scheduled_action.",
            "background": {
                "enabled": true,
                "interval_secs": 20,
                "mode": "deterministic",
                "wake_predicates": {
                    "timer": true,
                    "new_messages": false,
                    "task_completions": false,
                    "queued_work": false,
                    "stale_goals": false,
                    "retryable_failures": false,
                    "approval_resolved": false
                }
            },
            "scheduled_action": {
                "command": "python3 scripts/fibonacci_worker.py"
            }
        });

        let registry = default_registry();
        let result = registry
            .execute(
                "agent.install",
                &manifest,
                &policy,
                &parent_dir,
                None,
                &serde_json::to_string(&args).expect("json should encode"),
                None,
                None,
            )
            .expect("agent install should accept shorthand");

        assert!(result.contains("sandbox_exec"));

        let reevaluation = std::fs::read_to_string(
            agents_dir
                .join("fib_worker")
                .join("state")
                .join("reevaluation.json"),
        )
        .expect("reevaluation state should exist");
        assert!(reevaluation.contains("python3 scripts/fibonacci_worker.py"));
    }

    #[test]
    fn test_agent_install_tool_accepts_script_and_interval_shorthand() {
        let manifest = test_manifest(vec![Capability::AgentSpawn { max_children: 4 }]);
        let policy = PolicyEngine::new(manifest.clone());
        let temp = tempdir().expect("tempdir should create");
        let agents_dir = temp.path().join("agents");
        let parent_dir = agents_dir.join("builder_agent");
        std::fs::create_dir_all(&parent_dir).expect("parent dir should create");

        let args = serde_json::json!({
            "agent_id": "fib_worker",
            "instructions": "# Worker\nInstalled from real model shorthand.",
            "background": {
                "mode": "deterministic"
            },
            "scheduled_action": {
                "interval_secs": 20,
                "script": "python3 scripts/fibonacci_worker.py"
            }
        });

        let registry = default_registry();
        let result = registry
            .execute(
                "agent.install",
                &manifest,
                &policy,
                &parent_dir,
                None,
                &serde_json::to_string(&args).expect("json should encode"),
                None,
                None,
            )
            .expect("agent install should accept script+interval shorthand");

        assert!(result.contains("sandbox_exec"));

        let child_skill = std::fs::read_to_string(agents_dir.join("fib_worker").join("SKILL.md"))
            .expect("child skill should exist");
        assert!(child_skill.contains("metadata:\n  autonoetic:"));
        assert!(child_skill.contains("enabled: true"));
        assert!(child_skill.contains("interval_secs: 20"));
        assert!(child_skill.contains("## Output Contract"));

        let reevaluation = std::fs::read_to_string(
            agents_dir
                .join("fib_worker")
                .join("state")
                .join("reevaluation.json"),
        )
        .expect("reevaluation state should exist");
        assert!(reevaluation.contains("python3 scripts/fibonacci_worker.py"));
    }

    #[test]
    fn test_agent_install_tool_accepts_tool_use_and_cadence_shorthand() {
        let manifest = test_manifest(vec![Capability::AgentSpawn { max_children: 4 }]);
        let policy = PolicyEngine::new(manifest.clone());
        let temp = tempdir().expect("tempdir should create");
        let agents_dir = temp.path().join("agents");
        let parent_dir = agents_dir.join("builder_agent");
        std::fs::create_dir_all(&parent_dir).expect("parent dir should create");

        let args = serde_json::json!({
            "agent_id": "sequence_worker",
            "instructions": "# Worker\nInstalled from tool_use shorthand.",
            "background": {
                "mode": "deterministic"
            },
            "scheduled_action": {
                "cadence": "20s",
                "tool_use": {
                    "name": "sandbox.exec",
                    "arguments": {
                        "cmd": "python3 scripts/compute.py"
                    }
                }
            }
        });

        let registry = default_registry();
        let result = registry
            .execute(
                "agent.install",
                &manifest,
                &policy,
                &parent_dir,
                None,
                &serde_json::to_string(&args).expect("json should encode"),
                None,
                None,
            )
            .expect("agent install should accept tool_use+cadence shorthand");

        assert!(result.contains("sandbox_exec"));

        let child_skill =
            std::fs::read_to_string(agents_dir.join("sequence_worker").join("SKILL.md"))
                .expect("child skill should exist");
        assert!(child_skill.contains("metadata:\n  autonoetic:"));
        assert!(child_skill.contains("enabled: true"));
        assert!(child_skill.contains("interval_secs: 20"));
        assert!(child_skill.contains("## Output Contract"));

        let reevaluation = std::fs::read_to_string(
            agents_dir
                .join("sequence_worker")
                .join("state")
                .join("reevaluation.json"),
        )
        .expect("reevaluation state should exist");
        assert!(reevaluation.contains("python3 scripts/compute.py"));
    }

    #[test]
    fn test_dependency_plan_from_args_python() {
        let manifest = test_manifest(vec![]);
        let temp = tempdir().expect("tempdir should create");
        let plan = dependency_plan_from_args_or_lock(
            &manifest,
            temp.path(),
            Some(SandboxExecDependencies {
                runtime: "python".to_string(),
                packages: vec!["requests==2.32.3".to_string()],
            }),
        )
        .expect("plan should parse")
        .expect("plan should exist");
        assert_eq!(plan.runtime, DependencyRuntime::Python);
        assert_eq!(plan.packages.len(), 1);
    }

    #[test]
    fn test_dependency_plan_from_args_unsupported_runtime() {
        let manifest = test_manifest(vec![]);
        let temp = tempdir().expect("tempdir should create");
        let err = dependency_plan_from_args_or_lock(
            &manifest,
            temp.path(),
            Some(SandboxExecDependencies {
                runtime: "ruby".to_string(),
                packages: vec!["rack".to_string()],
            }),
        )
        .expect_err("unsupported runtime should fail");
        assert!(err.to_string().contains("Unsupported dependency runtime"));
    }

    #[test]
    fn test_dependency_plan_from_runtime_lock_default() {
        let manifest = test_manifest(vec![]);
        let temp = tempdir().expect("tempdir should create");
        let lock_path = temp.path().join("runtime.lock");
        std::fs::write(
            &lock_path,
            r#"
gateway:
  artifact: "autonoetic-gateway"
  version: "0.1.0"
  sha256: "abc"
sdk:
  version: "0.1.0"
sandbox:
  backend: "bubblewrap"
dependencies:
  - runtime: "python"
    packages:
      - "requests==2.32.3"
"#,
        )
        .expect("runtime.lock should write");

        let plan = dependency_plan_from_args_or_lock(&manifest, temp.path(), None)
            .expect("plan should parse")
            .expect("plan should exist");
        assert_eq!(plan.runtime, DependencyRuntime::Python);
        assert_eq!(plan.packages, vec!["requests==2.32.3".to_string()]);
    }

    #[test]
    fn test_dependency_plan_from_args_overrides_runtime_lock() {
        let manifest = test_manifest(vec![]);
        let temp = tempdir().expect("tempdir should create");
        let lock_path = temp.path().join("runtime.lock");
        std::fs::write(
            &lock_path,
            r#"
gateway:
  artifact: "autonoetic-gateway"
  version: "0.1.0"
  sha256: "abc"
sdk:
  version: "0.1.0"
sandbox:
  backend: "bubblewrap"
dependencies:
  - runtime: "python"
    packages:
      - "requests==2.32.3"
"#,
        )
        .expect("runtime.lock should write");

        let plan = dependency_plan_from_args_or_lock(
            &manifest,
            temp.path(),
            Some(SandboxExecDependencies {
                runtime: "nodejs".to_string(),
                packages: vec!["lodash@4.17.21".to_string()],
            }),
        )
        .expect("plan should parse")
        .expect("plan should exist");
        assert_eq!(plan.runtime, DependencyRuntime::NodeJs);
        assert_eq!(plan.packages, vec!["lodash@4.17.21".to_string()]);
    }

    #[test]
    fn test_execute_sandbox_tool_call_denied_by_policy() {
        let manifest = test_manifest(vec![Capability::ShellExec {
            patterns: vec!["python3 scripts/*".to_string()],
        }]);
        let policy = PolicyEngine::new(manifest.clone());
        let temp = tempdir().expect("tempdir should create");
        let args = serde_json::json!({
            "command": "echo should_fail"
        });

        let tool = default_registry();
        let err = tool
            .execute(
                "sandbox.exec",
                &manifest,
                &policy,
                temp.path(),
                None,
                &serde_json::to_string(&args).expect("json should encode"),
                None,
                None,
            )
            .expect_err("policy should deny command");
        assert!(err
            .to_string()
            .contains("sandbox command denied by ShellExec policy"));
    }

    #[test]
    fn test_install_time_validation_successful_first_run() {
        let temp = tempdir().expect("tempdir should create");
        let agents_dir = temp.path().join("agents");
        let parent_dir = agents_dir.join("builder_agent");
        std::fs::create_dir_all(&parent_dir).expect("parent dir should create");

        let manifest = test_manifest(vec![Capability::AgentSpawn { max_children: 10 }]);

        let registry = default_registry();
        let policy = PolicyEngine::new(manifest.clone());

        let args = serde_json::json!({
            "agent_id": "test_worker",
            "instructions": "A test worker agent.",
            "background": {
                "enabled": true,
                "interval_secs": 60
            },
            "scheduled_action": {
                "type": "write_file",
                "path": "state/init.json",
                "content": "{\"initialized\": true}"
            },
            "validate_on_install": true
        });

        let result = registry
            .execute(
                "agent.install",
                &manifest,
                &policy,
                &parent_dir,
                None,
                &args.to_string(),
                None,
                None,
            )
            .expect("install with validation should succeed");

        let parsed: serde_json::Value =
            serde_json::from_str(&result).expect("result should be json");
        assert_eq!(parsed.get("ok").unwrap(), true);
        assert_eq!(parsed.get("status").unwrap(), "agent_installed");

        let reevaluation_path = agents_dir
            .join("test_worker")
            .join("state")
            .join("reevaluation.json");
        let reevaluation =
            std::fs::read_to_string(&reevaluation_path).expect("reevaluation state should exist");
        assert!(reevaluation.contains("install_validation_success"));

        let init_file = agents_dir
            .join("test_worker")
            .join("state")
            .join("init.json");
        assert!(
            init_file.exists(),
            "init file should be created during validation"
        );
    }

    #[test]
    fn test_install_time_validation_structured_error_on_failure() {
        let temp = tempdir().expect("tempdir should create");
        let agents_dir = temp.path().join("agents");
        let parent_dir = agents_dir.join("builder_agent");
        std::fs::create_dir_all(&parent_dir).expect("parent dir should create");

        let manifest = test_manifest(vec![Capability::AgentSpawn { max_children: 10 }]);

        let registry = default_registry();
        let policy = PolicyEngine::new(manifest.clone());

        let args = serde_json::json!({
            "agent_id": "failing_worker",
            "instructions": "A worker that fails validation.",
            "background": {
                "enabled": true,
                "interval_secs": 60
            },
            "scheduled_action": {
                "type": "sandbox_exec",
                "command": "exit 1"
            },
            "validate_on_install": true
        });

        let result = registry
            .execute(
                "agent.install",
                &manifest,
                &policy,
                &parent_dir,
                None,
                &args.to_string(),
                None,
                None,
            )
            .expect("should return structured error, not panic");

        let parsed: serde_json::Value =
            serde_json::from_str(&result).expect("result should be json");
        assert_eq!(parsed.get("ok").unwrap(), false);
        assert_eq!(parsed.get("error_type").unwrap(), "execution");
        assert!(parsed.get("message").is_some());
        assert!(parsed.get("repair_hint").is_some() || parsed.get("message").is_some());

        let reevaluation_path = agents_dir
            .join("failing_worker")
            .join("state")
            .join("reevaluation.json");
        let reevaluation =
            std::fs::read_to_string(&reevaluation_path).expect("reevaluation state should exist");
        assert!(reevaluation.contains("install_validation_failed"));
    }

    #[test]
    fn test_install_validate_on_install_opt_out() {
        let temp = tempdir().expect("tempdir should create");
        let agents_dir = temp.path().join("agents");
        let parent_dir = agents_dir.join("builder_agent");
        std::fs::create_dir_all(&parent_dir).expect("parent dir should create");

        let manifest = test_manifest(vec![Capability::AgentSpawn { max_children: 10 }]);

        let registry = default_registry();
        let policy = PolicyEngine::new(manifest.clone());

        let args = serde_json::json!({
            "agent_id": "deferred_worker",
            "instructions": "A worker with deferred validation.",
            "background": {
                "enabled": true,
                "interval_secs": 60
            },
            "scheduled_action": {
                "type": "sandbox_exec",
                "command": "exit 1"
            },
            "validate_on_install": false
        });

        let result = registry
            .execute(
                "agent.install",
                &manifest,
                &policy,
                &parent_dir,
                None,
                &args.to_string(),
                None,
                None,
            )
            .expect("install with validate_on_install=false should succeed");

        let parsed: serde_json::Value =
            serde_json::from_str(&result).expect("result should be json");
        assert_eq!(parsed.get("ok").unwrap(), true);
        assert_eq!(parsed.get("status").unwrap(), "agent_installed");

        let reevaluation_path = agents_dir
            .join("deferred_worker")
            .join("state")
            .join("reevaluation.json");
        let reevaluation =
            std::fs::read_to_string(&reevaluation_path).expect("reevaluation state should exist");
        assert!(reevaluation.contains("installed"));
        assert!(
            !reevaluation.contains("install_validation"),
            "validation should not have run"
        );
    }
}
