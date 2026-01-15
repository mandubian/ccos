use crate::approval::queue::{DiscoverySource, ServerInfo};
use crate::discovery::apis_guru::ApisGuruClient;
use crate::mcp::registry::MCPRegistryClient;
use crate::synthesis::runtime::web_search_discovery::WebSearchDiscovery;
use rtfs::runtime::error::{RuntimeError, RuntimeResult};
use serde::Deserialize;
use std::path::PathBuf;

pub struct RegistrySearcher {
    mcp_client: MCPRegistryClient,
    apis_guru_client: ApisGuruClient,
    npm_client: reqwest::Client,
}

#[derive(Debug, Clone)]
pub struct RegistrySearchResult {
    pub source: DiscoverySource,
    pub server_info: ServerInfo,
    pub match_score: f32,
    /// Alternative endpoints (e.g., multiple remotes from MCP registry)
    /// If present, user should be prompted to select which endpoint(s) to use
    pub alternative_endpoints: Vec<String>,
}

impl RegistrySearcher {
    pub fn new() -> Self {
        Self {
            mcp_client: MCPRegistryClient::new(),
            apis_guru_client: ApisGuruClient::new(),
            npm_client: reqwest::Client::new(),
        }
    }
    
    /// Search NPM registry for MCP packages
    async fn search_npm(&self, query: &str) -> RuntimeResult<Vec<RegistrySearchResult>> {
        let mut results = Vec::new();
        let query_lower = query.to_lowercase();
        
        // Well-known official MCP packages that should be checked directly
        let well_known_packages = vec![
            "@modelcontextprotocol/server-puppeteer",
            "@modelcontextprotocol/server-filesystem",
            "@modelcontextprotocol/server-github",
            "@modelcontextprotocol/server-postgres",
        ];
        
        // If query matches a well-known package, try direct lookup first
        for pkg_name in &well_known_packages {
            if query_lower.contains("puppeteer") && pkg_name.contains("puppeteer") {
                if let Ok(pkg_result) = self.lookup_npm_package(pkg_name).await {
                    if let Some(result) = pkg_result {
                        results.push(result);
                    }
                }
            }
        }
        
        let search_url = "https://registry.npmjs.org/-/v1/search";
        let search_params = [
            ("text", query),
            ("size", "20"),
        ];
        
        let response = self
            .npm_client
            .get(search_url)
            .query(&search_params)
            .header("Accept", "application/json")
            .send()
            .await
            .map_err(|e| RuntimeError::Generic(format!("Failed to search NPM: {}", e)))?;
        
        if !response.status().is_success() {
            return Ok(results);
        }
        
        let json: serde_json::Value = response.json().await
            .map_err(|e| RuntimeError::Generic(format!("Failed to parse NPM response: {}", e)))?;
        
        if let Some(objects) = json.get("objects").and_then(|o| o.as_array()) {
            for obj in objects {
                if let Some(package) = obj.get("package") {
                    let name = package.get("name")
                        .and_then(|n| n.as_str())
                        .map(|s| s.to_string());
                    let description = package.get("description")
                        .and_then(|d| d.as_str())
                        .map(|s| s.to_string());
                    let keywords = package.get("keywords")
                        .and_then(|k| k.as_array())
                        .map(|arr| arr.iter()
                            .filter_map(|v| v.as_str().map(|s| s.to_string()))
                            .collect::<Vec<_>>())
                        .unwrap_or_default();
                    
                    // Filter for MCP-related packages
                    // Include packages with "mcp" in name/keywords/description OR in @modelcontextprotocol scope
                    let is_mcp_related = name.as_ref()
                        .map(|n| {
                            n.contains("mcp") 
                            || n.contains("modelcontextprotocol")
                            || n.starts_with("@modelcontextprotocol/") // Official MCP packages
                        })
                        .unwrap_or(false)
                        || keywords.iter().any(|k| k.to_lowercase().contains("mcp"))
                        || description.as_ref()
                            .map(|d| d.to_lowercase().contains("mcp"))
                            .unwrap_or(false);
                    
                    if is_mcp_related {
                        if let Some(name) = name {
                            // Construct stdio command for npm packages
                            let endpoint = format!("npx -y {}", name);
                            
                            results.push(RegistrySearchResult {
                                source: DiscoverySource::NpmRegistry {
                                    package: name.clone(),
                                },
                                server_info: ServerInfo {
                                    name: name.clone(),
                                    endpoint,
                                    description,
                                    auth_env_var: Some(crate::approval::suggest_auth_env_var(&name)),
                                    capabilities_path: None,
                                    alternative_endpoints: Vec::new(),
                                    capability_files: None,
                                },
                                match_score: 0.8, // Slightly lower than MCP registry matches
                                alternative_endpoints: Vec::new(),
                            });
                        }
                    }
                }
            }
        }
        
        Ok(results)
    }
    
    /// Direct lookup of an NPM package by name
    async fn lookup_npm_package(&self, package_name: &str) -> RuntimeResult<Option<RegistrySearchResult>> {
        let url = format!("https://registry.npmjs.org/{}", package_name);
        
        let response = self
            .npm_client
            .get(&url)
            .header("Accept", "application/json")
            .send()
            .await
            .map_err(|e| RuntimeError::Generic(format!("Failed to lookup NPM package: {}", e)))?;
        
        if !response.status().is_success() {
            return Ok(None);
        }
        
        let json: serde_json::Value = response.json().await
            .map_err(|e| RuntimeError::Generic(format!("Failed to parse NPM package response: {}", e)))?;
        
        let description = json.get("description")
            .and_then(|d| d.as_str())
            .map(|s| s.to_string());
        
        // Check if it's MCP-related
        let is_mcp = package_name.contains("mcp") 
            || package_name.contains("modelcontextprotocol")
            || description.as_ref()
                .map(|d| d.to_lowercase().contains("mcp"))
                .unwrap_or(false);
        
        if is_mcp {
            let endpoint = format!("npx -y {}", package_name);
            Ok(Some(RegistrySearchResult {
                source: DiscoverySource::NpmRegistry {
                    package: package_name.to_string(),
                },
                server_info: ServerInfo {
                    name: package_name.to_string(),
                    endpoint,
                    description,
                    auth_env_var: Some(crate::approval::suggest_auth_env_var(package_name)),
                    capabilities_path: None,
                    alternative_endpoints: Vec::new(),
                    capability_files: None,
                },
                match_score: 0.9, // Higher score for well-known packages
                alternative_endpoints: Vec::new(),
            }))
        } else {
            Ok(None)
        }
    }

    pub async fn search(&self, query: &str) -> RuntimeResult<Vec<RegistrySearchResult>> {
        let mut results = Vec::new();
        let debug = std::env::var("CCOS_DEBUG").is_ok();

        // 1. Search MCP Registry (remote)
        // Try the full query first, then try individual words if multi-word query returns nothing
        let mcp_servers_result = self.mcp_client.search_servers(query).await;
        
        let mcp_servers = if let Ok(ref servers) = mcp_servers_result {
            if servers.is_empty() {
                // If query has multiple words and initial search returned nothing, try individual words
                let words: Vec<&str> = query.split_whitespace().collect();
                if words.len() > 1 {
                    // Try each word individually and combine results
                    let mut combined_results = Vec::new();
                    let mut seen_names = std::collections::HashSet::new();
                    
                    for word in words {
                        if let Ok(word_results) = self.mcp_client.search_servers(word).await {
                            for server in word_results {
                                if seen_names.insert(server.name.clone()) {
                                    combined_results.push(server);
                                }
                            }
                        }
                    }
                    
                    if !combined_results.is_empty() {
                        if debug {
                            eprintln!("🔍 Multi-word query '{}' split into words, found {} servers", query, combined_results.len());
                        }
                        combined_results
                    } else {
                        Vec::new()
                    }
                } else {
                    Vec::new()
                }
            } else {
                servers.clone()
            }
        } else {
            // If search failed, return empty
            Vec::new()
        };
        
        if !mcp_servers.is_empty() {
                if debug {
                    eprintln!("🔍 MCP Registry: found {} servers", mcp_servers.len());
                }
                let registry_results: Vec<RegistrySearchResult> = mcp_servers
                    .into_iter()
                    .map(|server| {
                        let (endpoint, alternatives) = if let Some(remotes) = &server.remotes {
                            // Select best remote (prioritizes HTTP/HTTPS, but falls back to stdio commands)
                            let primary = MCPRegistryClient::select_best_remote_url(remotes)
                                .unwrap_or_default();
                            
                            // Collect all remotes as alternatives (including stdio commands)
                            let all_remotes: Vec<String> = remotes
                                .iter()
                                .filter_map(|r| {
                                    let url = r.url.trim();
                                    if !url.is_empty() {
                                        Some(url.to_string())
                                    } else {
                                        None
                                    }
                                })
                                .collect();
                            
                            let mut alternatives = all_remotes;
                            alternatives.retain(|url| url != &primary);

                            (primary, alternatives)
                        } else if let Some(packages) = &server.packages {
                            // If no remotes, try to construct endpoint from packages with stdio transport
                            let mut stdio_endpoints = Vec::new();
                            
                            for package in packages {
                                if package.transport.r#type.to_lowercase() == "stdio" {
                                    let command = match package.registry_type.to_lowercase().as_str() {
                                        "npm" | "npx" => {
                                            // For npm packages, use npx
                                            if let Some(version) = &package.version {
                                                format!("npx -y {}@{}", package.identifier, version)
                                            } else {
                                                format!("npx -y {}", package.identifier)
                                            }
                                        }
                                        "pypi" => {
                                            // For PyPI packages, try python -m or direct command
                                            let module_name = package.identifier.replace("-", "_");
                                            if let Some(runtime_hint) = &package.runtime_hint {
                                                runtime_hint.clone()
                                            } else {
                                                format!("python -m {}", module_name)
                                            }
                                        }
                                        _ => {
                                            // For other registries, use identifier as-is or with runtime hint
                                            if let Some(runtime_hint) = &package.runtime_hint {
                                                runtime_hint.clone()
                                            } else {
                                                package.identifier.clone()
                                            }
                                        }
                                    };
                                    stdio_endpoints.push(command);
                                }
                            }
                            
                            // Use first stdio endpoint as primary, rest as alternatives
                            let primary = stdio_endpoints.first().cloned().unwrap_or_default();
                            let mut alternatives = stdio_endpoints;
                            alternatives.retain(|url| url != &primary);
                            
                            (primary, alternatives)
                        } else {
                            (String::new(), Vec::new())
                        };

                        RegistrySearchResult {
                            source: DiscoverySource::McpRegistry {
                                name: server.name.clone(),
                            },
                            server_info: ServerInfo {
                                name: server.name.clone(),
                                endpoint,
                                description: Some(server.description),
                                auth_env_var: Some(crate::approval::suggest_auth_env_var(
                                    &server.name,
                                )),
                                capabilities_path: None,
                                alternative_endpoints: alternatives,
                                capability_files: None,
                            },
                            match_score: 1.0,                  // Default score
                            alternative_endpoints: Vec::new(), // Not used anymore, kept for compatibility
                        }
                    })
                    .collect();
            results.extend(registry_results);
        } else {
            if debug {
                eprintln!("⚠️  MCP Registry search returned no results for '{}'", query);
            }
        }

        // 2. Search NPM registry for MCP packages
        match self.search_npm(query).await {
            Ok(npm_results) => {
                if debug && !npm_results.is_empty() {
                    eprintln!("🔍 NPM Registry: found {} MCP packages", npm_results.len());
                }
                // Deduplicate by endpoint
                for npm_result in npm_results {
                    if !results.iter().any(|r| r.server_info.endpoint == npm_result.server_info.endpoint) {
                        results.push(npm_result);
                    }
                }
            }
            Err(e) => {
                if debug {
                    eprintln!("⚠️  NPM search failed: {}", e);
                }
            }
        }

        // 3. Search local overrides.json
        let override_results = self.search_overrides(query)?;
        if debug && !override_results.is_empty() {
            eprintln!(
                "🔍 Local overrides: found {} servers",
                override_results.len()
            );
        }
        results.extend(override_results);

        // 4. Search APIs.guru (OpenAPI directory)
        match self.search_apis_guru(query).await {
            Ok(apis_results) => results.extend(apis_results),
            Err(e) => {
                // Log but don't fail - APIs.guru is optional
                eprintln!("⚠️  APIs.guru search failed: {}", e);
            }
        }

        // 5. Web search (fallback) - can be disabled via env var or config file
        if Self::is_web_search_enabled() {
            match self.search_web(query).await {
                Ok(web_results) => results.extend(web_results),
                Err(e) => {
                    // Log but don't fail - web search is optional
                    eprintln!("⚠️  Web search failed: {}", e);
                }
            }
        } else if debug {
            eprintln!("🔍 Web search disabled (via config or CCOS_DISABLE_WEB_SEARCH=1)");
        }

        Ok(results)
    }

    /// Search APIs.guru for OpenAPI specifications
    async fn search_apis_guru(&self, query: &str) -> RuntimeResult<Vec<RegistrySearchResult>> {
        let apis = self.apis_guru_client.search(query).await?;

        let results: Vec<RegistrySearchResult> = apis
            .into_iter()
            .map(|api| {
                // Use OpenAPI URL if available, otherwise Swagger URL
                let endpoint = api.openapi_url.or(api.swagger_url).unwrap_or_default();

                // Extract base URL from OpenAPI/Swagger URL for server endpoint
                // For now, we'll use the spec URL itself - in production you'd parse the spec
                let server_name = api.provider.unwrap_or_else(|| api.name.clone());

                RegistrySearchResult {
                    source: DiscoverySource::ApisGuru {
                        api_name: api.name.clone(),
                    },
                    server_info: ServerInfo {
                        name: format!("apis.guru/{}", api.name),
                        endpoint,
                        description: api.description.or(Some(api.title)),
                        auth_env_var: Some(crate::approval::suggest_auth_env_var(&server_name)),
                        capabilities_path: None,
                        alternative_endpoints: Vec::new(),
                        capability_files: None,
                    },
                    match_score: 0.8, // Slightly lower score than MCP registry
                    alternative_endpoints: Vec::new(),
                }
            })
            .collect();

        Ok(results)
    }

    /// Search web for APIs (MCP servers, OpenAPI specs, and general APIs)
    async fn search_web(&self, query: &str) -> RuntimeResult<Vec<RegistrySearchResult>> {
        let mut web_searcher = WebSearchDiscovery::new("auto".to_string());

        // Pass the query directly - search_for_api_specs will generate appropriate search patterns
        // for general APIs, MCP servers, and OpenAPI specs
        let search_results = web_searcher.search_for_api_specs(query).await?;

        let results: Vec<RegistrySearchResult> = search_results
            .into_iter()
            .filter_map(|result| {
                // Include results that look like API endpoints or specs
                // Prioritize MCP servers but also include OpenAPI specs
                let url_lower = result.url.to_lowercase();
                let title_lower = result.title.to_lowercase();
                let snippet_lower = result.snippet.to_lowercase();

                let is_mcp_server = url_lower.contains("/mcp")
                    || url_lower.contains("mcp://")
                    || url_lower.contains("modelcontextprotocol")
                    || url_lower.contains("smithery.ai") // Known MCP hosting
                    || title_lower.contains("mcp server")
                    || title_lower.contains("model context protocol")
                    || snippet_lower.contains("mcp server")
                    || snippet_lower.contains("model context protocol");

                let is_openapi_spec = url_lower.contains("openapi")
                    || url_lower.contains("swagger")
                    || url_lower.ends_with(".json")
                        && (url_lower.contains("api") || url_lower.contains("spec"))
                    || url_lower.ends_with(".yaml")
                        && (url_lower.contains("api") || url_lower.contains("spec"))
                    || url_lower.ends_with(".yml")
                        && (url_lower.contains("api") || url_lower.contains("spec"))
                    || title_lower.contains("openapi")
                    || title_lower.contains("swagger")
                    || result.result_type == "openapi_spec";

                let is_api_doc = result.result_type == "api_doc"
                    || url_lower.contains("/api")
                    || url_lower.contains("/docs")
                    || title_lower.contains("api documentation");

                // Include MCP servers, OpenAPI specs, and API documentation
                if is_mcp_server || is_openapi_spec || is_api_doc {
                    // Extract server name from URL domain or title
                    let server_name =
                        if let Some(domain) = Self::extract_domain_from_url(&result.url) {
                            domain
                        } else {
                            // Clean title - remove HTML tags and take first meaningful word
                            result
                                .title
                                .replace("<[^>]*>", "") // Remove HTML tags (basic)
                                .split_whitespace()
                                .find(|w| w.len() > 2 && !w.eq_ignore_ascii_case("api"))
                                .unwrap_or(if is_mcp_server { "web-mcp" } else { "web-api" })
                                .to_string()
                        };

                    // Determine server type for better naming
                    let server_type = if is_mcp_server {
                        "mcp"
                    } else if is_openapi_spec {
                        "openapi"
                    } else {
                        "api"
                    };

                    // Sanitize name for ID generation (important for deduplication)
                    let safe_server_name = crate::utils::fs::sanitize_filename(&server_name);

                    // Clean title and snippet more thoroughly to avoid HTML leakage
                    // Use regex to strip HTML tags, and handle truncated tags
                    let html_re = regex::Regex::new(r"<[^>]*>").unwrap();
                    let mut clean_title = html_re.replace_all(&result.title, "").trim().to_string();
                    let mut clean_snippet =
                        html_re.replace_all(&result.snippet, "").trim().to_string();

                    // Helper closure to clean truncated tags
                    let strip_truncated = |s: &str| -> String {
                        if s.starts_with('<') {
                            if let Some(idx) = s.find('>') {
                                s[idx + 1..].trim().to_string()
                            } else {
                                s.trim_start_matches('<')
                                    .trim_start_matches(|c: char| c.is_alphanumeric() || c == '-')
                                    .trim()
                                    .to_string()
                            }
                        } else {
                            s.to_string()
                        }
                    };

                    clean_title = strip_truncated(&clean_title);
                    clean_snippet = strip_truncated(&clean_snippet);

                    // Also check for leading "- " which might happen after stripping
                    clean_snippet = clean_snippet.trim_start_matches("- ").to_string();

                    Some(RegistrySearchResult {
                        source: DiscoverySource::WebSearch {
                            url: result.url.clone(),
                        },
                        server_info: ServerInfo {
                            name: format!("web/{}/{}", server_type, safe_server_name),
                            endpoint: result.url.clone(),
                            description: Some(format!("{} - {}", clean_title, clean_snippet)),
                            auth_env_var: Some(crate::approval::suggest_auth_env_var(
                                &safe_server_name,
                            )),
                            capabilities_path: None,
                            alternative_endpoints: Vec::new(),
                            capability_files: None,
                        },
                        match_score: if is_mcp_server { 0.6 } else { 0.5 }, // MCP servers score slightly higher
                        alternative_endpoints: Vec::new(),
                    })
                } else {
                    None
                }
            })
            .collect();

        Ok(results)
    }

    /// Extract domain name from URL for server naming
    fn extract_domain_from_url(url: &str) -> Option<String> {
        if let Ok(parsed) = url::Url::parse(url) {
            if let Some(domain) = parsed.host_str() {
                // Remove www. prefix and extract main domain
                let domain = domain.strip_prefix("www.").unwrap_or(domain);
                let parts: Vec<&str> = domain.split('.').collect();
                if parts.len() >= 2 {
                    // Take second-to-last part (e.g., "openweathermap" from "openweathermap.org")
                    return Some(parts[parts.len().saturating_sub(2)].to_string());
                }
            }
        }
        None
    }

    /// Search for servers in local overrides.json that match the query
    fn search_overrides(&self, query: &str) -> RuntimeResult<Vec<RegistrySearchResult>> {
        let mut results = Vec::new();
        let query_lower = query.to_lowercase();
        let debug = std::env::var("CCOS_DEBUG").is_ok();

        // Split query into words, filtering out common action verbs
        let action_verbs = [
            "list", "get", "create", "update", "delete", "show", "find", "search",
        ];
        let query_words: Vec<&str> = query_lower
            .split_whitespace()
            .filter(|w| !action_verbs.contains(w) && w.len() > 2)
            .collect();

        let overrides_path = Self::find_overrides_path();
        if let Some(path) = overrides_path {
            if debug {
                eprintln!("🔍 Checking local overrides: {}", path.display());
            }
            if let Ok(content) = std::fs::read_to_string(&path) {
                if let Ok(parsed) = serde_json::from_str::<serde_json::Value>(&content) {
                    if let Some(entries) = parsed.get("entries").and_then(|e| e.as_array()) {
                        for entry in entries {
                            if let Some(server) = entry.get("server") {
                                // Check if server name or description matches query
                                let name = server
                                    .get("name")
                                    .and_then(|n| n.as_str())
                                    .unwrap_or("")
                                    .to_lowercase();
                                let description = server
                                    .get("description")
                                    .and_then(|d| d.as_str())
                                    .unwrap_or("")
                                    .to_lowercase();

                                // Match if ANY domain word matches (more lenient)
                                // This allows "list github issues" to match a server with "github" in name
                                let any_word_match = query_words
                                    .iter()
                                    .any(|word| name.contains(word) || description.contains(word));

                                // Also check full query match for exact searches
                                let full_match = name.contains(&query_lower)
                                    || description.contains(&query_lower);

                                if full_match || any_word_match {
                                    // Extract endpoint from remotes
                                    let (endpoint, alternatives) = if let Some(remotes) =
                                        server.get("remotes").and_then(|r| r.as_array())
                                    {
                                        // Collect all HTTP/HTTPS remotes
                                        let all_http_remotes: Vec<String> = remotes
                                            .iter()
                                            .filter_map(|r| {
                                                r.get("url")
                                                    .and_then(|u| u.as_str())
                                                    .filter(|url| url.starts_with("http"))
                                                    .map(|url| url.to_string())
                                            })
                                            .collect();

                                        // Use first as primary, rest as alternatives
                                        let primary =
                                            all_http_remotes.first().cloned().unwrap_or_default();
                                        let mut alternatives = all_http_remotes;
                                        alternatives.retain(|url| url != &primary);

                                        (primary, alternatives)
                                    } else {
                                        (String::new(), Vec::new())
                                    };

                                    if !endpoint.is_empty() {
                                        let server_name = server
                                            .get("name")
                                            .and_then(|n| n.as_str())
                                            .unwrap_or("unknown")
                                            .to_string();

                                        let server_name_clone = server_name.clone();
                                        results.push(RegistrySearchResult {
                                            source: DiscoverySource::LocalOverride {
                                                path: path.display().to_string(),
                                            },
                                            server_info: ServerInfo {
                                                name: server_name,
                                                endpoint,
                                                description: server
                                                    .get("description")
                                                    .and_then(|d| d.as_str())
                                                    .map(|s| s.to_string()),
                                                auth_env_var: Some(
                                                    crate::approval::suggest_auth_env_var(
                                                        &server_name_clone,
                                                    ),
                                                ),
                                                capabilities_path: None,
                                                alternative_endpoints: alternatives,
                                                capability_files: None,
                                            },
                                            match_score: 1.2, // Slightly higher score for local overrides
                                            alternative_endpoints: Vec::new(),
                                        });
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }

        Ok(results)
    }

    fn find_overrides_path() -> Option<PathBuf> {
        let root = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
        let candidates = vec![
            root.join("capabilities/mcp/overrides.json"),
            root.parent()
                .unwrap_or(&root)
                .join("capabilities/mcp/overrides.json"),
        ];

        for path in candidates {
            if path.exists() {
                return Some(path);
            }
        }
        None
    }

    /// Check if web search is enabled by checking both environment variable and config file
    pub fn is_web_search_enabled() -> bool {
        // First check environment variable to ENABLE (takes precedence)
        if let Ok(enable) = std::env::var("CCOS_ENABLE_WEB_SEARCH") {
            if enable == "1" || enable.to_lowercase() == "true" || enable.to_lowercase() == "on" {
                return true;
            }
        }

        // Check for DISABLE env var
        if let Ok(disable) = std::env::var("CCOS_DISABLE_WEB_SEARCH") {
            if disable == "1" || disable.to_lowercase() == "true" || disable.to_lowercase() == "on"
            {
                return false;
            }
        }

        // Check config file setting
        // Try multiple config file paths
        let config_paths = [
            "config/agent_config.toml",
            "../config/agent_config.toml",
            "agent_config.toml",
        ];

        for config_path in &config_paths {
            if let Ok(content) = std::fs::read_to_string(config_path) {
                #[derive(Deserialize, Default)]
                struct MissingCapabilitiesConfig {
                    #[serde(default)]
                    web_search: Option<bool>,
                }

                #[derive(Deserialize, Default)]
                struct AgentConfigToml {
                    #[serde(default)]
                    missing_capabilities: MissingCapabilitiesConfig,
                }

                if let Ok(config) = toml::from_str::<AgentConfigToml>(&content) {
                    if let Some(web_search) = config.missing_capabilities.web_search {
                        return web_search;
                    }
                }
            }
        }

        // Default: disabled (conservative)
        false
    }
}
