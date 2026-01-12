//! LLM-based API Documentation Parser
//!
//! This module uses an LLM (via the Arbiter's LlmProvider) to extract API
//! endpoint information from human-readable documentation when OpenAPI specs
//! are not available.
//!
//! # Architecture
//!
//! The parser integrates with the CCOS arbiter system:
//! - Uses `LlmProvider` trait for LLM calls (supports OpenAI, Anthropic, etc.)
//! - All LLM calls go through the arbiter's governance layer
//! - Responses are structured JSON extracted from documentation text

use crate::arbiter::llm_provider::LlmProvider;
use crate::synthesis::introspection::auth_injector::{AuthConfig, AuthType};
use crate::synthesis::introspection::{
    APIIntrospectionResult, AuthRequirements, DiscoveredEndpoint, EndpointParameter,
};
use rtfs::ast::{Keyword, MapTypeEntry, PrimitiveType, TypeExpr};
use rtfs::runtime::error::{RuntimeError, RuntimeResult};
use serde::{Deserialize, Serialize};

/// Extracted endpoint from LLM parsing
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExtractedEndpoint {
    pub name: String,
    pub description: String,
    pub method: String,
    pub path: String,
    pub parameters: Vec<ExtractedParameter>,
    pub requires_auth: bool,
}

/// Extracted parameter from LLM parsing
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExtractedParameter {
    pub name: String,
    #[serde(rename = "type")]
    pub param_type: String,
    pub location: String, // query, path, header, body
    pub required: bool,
    pub description: String,
}

/// Extracted auth info from LLM parsing
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExtractedAuth {
    pub auth_type: String,
    pub location: String,
    pub param_name: String,
    pub env_var_suggestion: String,
}

/// LLM response structure
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LlmApiParseResponse {
    pub api_name: String,
    pub base_url: String,
    pub version: String,
    pub description: String,
    pub endpoints: Vec<ExtractedEndpoint>,
    pub auth: Option<ExtractedAuth>,
}

/// Discovered API link from documentation exploration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DiscoveredApiLink {
    /// Type of API: "rest", "websocket", "openapi", "graphql", "documentation"
    pub api_type: String,
    /// Full URL to the API documentation or spec
    pub url: String,
    /// Human-readable label for the link
    pub label: String,
    /// Brief description of what this API does
    pub description: String,
}

/// LLM response for link discovery
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LlmLinkDiscoveryResponse {
    /// List of discovered API links
    pub api_links: Vec<DiscoveredApiLink>,
    /// OpenAPI/Swagger spec URLs if found
    pub openapi_specs: Vec<String>,
    /// Whether this page appears to be API documentation
    pub is_api_documentation: bool,
}

/// LLM-based API documentation parser
/// Uses the Arbiter's LlmProvider for all LLM calls
pub struct LlmDocParser {
    /// HTTP client for fetching docs
    client: reqwest::Client,
}

impl LlmDocParser {
    /// Create a new LLM doc parser
    pub fn new() -> Self {
        Self {
            client: reqwest::Client::new(),
        }
    }

    /// Parse API documentation from a URL using an LLM provider
    pub async fn parse_from_url(
        &self,
        doc_url: &str,
        api_domain: &str,
        llm_provider: &dyn LlmProvider,
    ) -> RuntimeResult<APIIntrospectionResult> {
        log::info!("🔍 Fetching API documentation from: {}", doc_url);

        // Fetch the documentation page
        let html_content = self.fetch_page(doc_url).await?;

        // Extract text content from HTML
        let text_content = self.extract_text_from_html(&html_content)?;

        // Use LLM to parse the documentation
        let parsed = self
            .parse_with_llm(&text_content, api_domain, llm_provider)
            .await?;

        // Convert to APIIntrospectionResult
        self.convert_to_introspection_result(parsed)
    }

    /// Parse API documentation for a domain using default doc URLs
    /// NOTE: This method is deprecated - use parse_from_url() with a user-provided URL instead
    pub async fn parse_for_domain(
        &self,
        api_domain: &str,
        _llm_provider: &dyn LlmProvider,
    ) -> RuntimeResult<APIIntrospectionResult> {
        // No hardcoded URLs - user must provide documentation URL
        Err(RuntimeError::Generic(format!(
            "No documentation URLs available for domain: {}. Please provide a documentation URL manually.",
            api_domain
        )))
    }

    /// Explore a documentation page to find links to API documentation
    /// This is the first step in the documentation crawler - it identifies:
    /// - REST API documentation links
    /// - WebSocket API links
    /// - OpenAPI/Swagger specification URLs
    /// - GraphQL endpoints
    pub async fn explore_documentation(
        &self,
        landing_url: &str,
        llm_provider: &dyn LlmProvider,
    ) -> RuntimeResult<LlmLinkDiscoveryResponse> {
        log::info!("🔍 Exploring documentation at: {}", landing_url);

        // Fetch the landing page
        let html_content = self.fetch_page(landing_url).await?;

        // Extract links and text from HTML (keeping href attributes)
        let (links, text_content) = self.extract_links_and_text(&html_content, landing_url)?;

        // Use LLM to identify API-related links
        let response = self
            .discover_api_links(&links, &text_content, landing_url, llm_provider)
            .await?;

        log::info!(
            "✅ Found {} API links, {} OpenAPI specs",
            response.api_links.len(),
            response.openapi_specs.len()
        );

        Ok(response)
    }

    /// Parse documentation page to extract API endpoints
    /// This is the second step - once a relevant page is found, this extracts:
    /// - API Endpoints (methods, paths, parameters)
    /// - Authentication requirements
    /// - Data types
    pub async fn parse_documentation(
        &self,
        doc_url: &str,
        llm_provider: &dyn LlmProvider,
    ) -> RuntimeResult<LlmApiParseResponse> {
        log::info!("📖 Parsing documentation at: {}", doc_url);

        // Fetch the page
        let html_content = self.fetch_page(doc_url).await?;

        // Extract text content
        let text_content = self.extract_text_from_html(&html_content)?;

        // Extract domain from URL for context
        let api_domain = doc_url
            .split("://")
            .nth(1)
            .unwrap_or(doc_url)
            .split('/')
            .next()
            .unwrap_or("unknown");

        // Use LLM to parse endpoints
        self.parse_with_llm(&text_content, api_domain, llm_provider)
            .await
    }

    /// Extract links and text content from HTML
    fn extract_links_and_text(
        &self,
        html: &str,
        base_url: &str,
    ) -> RuntimeResult<(Vec<(String, String)>, String)> {
        let mut links = Vec::new();

        // Extract href links with their text
        let link_re =
            regex::Regex::new(r#"<a[^>]+href=["']([^"']+)["'][^>]*>([^<]*)</a>"#).unwrap();
        for cap in link_re.captures_iter(html) {
            let href = cap.get(1).map(|m| m.as_str()).unwrap_or("");
            let text = cap.get(2).map(|m| m.as_str()).unwrap_or("").trim();

            // Make URL absolute if relative
            let full_url = if href.starts_with("http") {
                href.to_string()
            } else if href.starts_with('/') {
                // Extract base domain from URL
                if let Some(domain_end) = base_url.find("://").map(|i| {
                    base_url[i + 3..]
                        .find('/')
                        .map(|j| i + 3 + j)
                        .unwrap_or(base_url.len())
                }) {
                    format!("{}{}", &base_url[..domain_end], href)
                } else {
                    href.to_string()
                }
            } else {
                href.to_string()
            };

            if !full_url.is_empty() && !text.is_empty() {
                links.push((full_url, text.to_string()));
            }
        }

        // Also extract text content
        let text_content = self.extract_text_from_html(html)?;

        Ok((links, text_content))
    }

    /// Use LLM to discover API links from page content
    async fn discover_api_links(
        &self,
        links: &[(String, String)],
        text_content: &str,
        base_url: &str,
        llm_provider: &dyn LlmProvider,
    ) -> RuntimeResult<LlmLinkDiscoveryResponse> {
        let prompt = self.create_link_discovery_prompt(links, text_content, base_url);

        let llm_response = llm_provider.generate_text(&prompt).await?;

        self.parse_link_discovery_response(&llm_response)
    }

    /// Create prompt for LLM to identify API links
    fn create_link_discovery_prompt(
        &self,
        links: &[(String, String)],
        text_content: &str,
        base_url: &str,
    ) -> String {
        let links_text: String = links
            .iter()
            .take(100) // Limit to avoid token overflow
            .map(|(url, text)| format!("- [{}]({})", text, url))
            .collect::<Vec<_>>()
            .join("\n");

        // Truncate text content
        let truncated_text = if text_content.len() > 5000 {
            &text_content[..5000]
        } else {
            text_content
        };

        format!(
            r#"You are analyzing a developer documentation website to find API documentation links.

Base URL: {}

Links found on the page:
{}

Page text excerpt:
{}

Find and categorize all API-related links. Look for:
1. REST API documentation (paths like /api, /rest, /v1, /docs/api)
2. WebSocket API documentation (paths containing websocket, ws, streaming)
3. OpenAPI/Swagger specification files (swagger.json, openapi.yaml, /swagger, /openapi)
4. GraphQL endpoints (graphql, /graphql)
5. General API reference pages

Respond with ONLY this JSON structure:
{{
  "api_links": [
    {{
      "api_type": "rest|websocket|graphql|documentation",
      "url": "full URL to the API documentation",
      "label": "Link text or title",
      "description": "What this API does based on context"
    }}
  ],
  "openapi_specs": [
    "full URL to any OpenAPI/Swagger JSON or YAML files"
  ],
  "is_api_documentation": true
}}

If no API links are found, return empty arrays. Only include links that are clearly API-related."#,
            base_url, links_text, truncated_text
        )
    }

    /// Parse LLM response for link discovery
    fn parse_link_discovery_response(
        &self,
        response: &str,
    ) -> RuntimeResult<LlmLinkDiscoveryResponse> {
        let json_str = self.extract_json_from_response(response);

        match serde_json::from_str::<LlmLinkDiscoveryResponse>(&json_str) {
            Ok(parsed) => Ok(parsed),
            Err(e) => {
                log::warn!("Failed to parse link discovery response: {}", e);
                // Return empty result on parse failure
                Ok(LlmLinkDiscoveryResponse {
                    api_links: vec![],
                    openapi_specs: vec![],
                    is_api_documentation: false,
                })
            }
        }
    }

    /// Fetch a web page
    async fn fetch_page(&self, url: &str) -> RuntimeResult<String> {
        let response = self
            .client
            .get(url)
            .header("User-Agent", "Mozilla/5.0 (compatible; CCOS/1.0)")
            .timeout(std::time::Duration::from_secs(30))
            .send()
            .await
            .map_err(|e| RuntimeError::Generic(format!("Failed to fetch documentation: {}", e)))?;

        if !response.status().is_success() {
            return Err(RuntimeError::Generic(format!(
                "Failed to fetch documentation: HTTP {}",
                response.status()
            )));
        }

        response
            .text()
            .await
            .map_err(|e| RuntimeError::Generic(format!("Failed to read response: {}", e)))
    }

    /// Extract text content from HTML
    fn extract_text_from_html(&self, html: &str) -> RuntimeResult<String> {
        // Simple HTML to text conversion
        // In production, use a proper HTML parser like scraper
        let mut text = html.to_string();

        // Remove script and style tags
        let script_re = regex::Regex::new(r"(?s)<script[^>]*>.*?</script>").unwrap();
        text = script_re.replace_all(&text, "").to_string();

        let style_re = regex::Regex::new(r"(?s)<style[^>]*>.*?</style>").unwrap();
        text = style_re.replace_all(&text, "").to_string();

        // Replace common HTML entities
        text = text.replace("&nbsp;", " ");
        text = text.replace("&lt;", "<");
        text = text.replace("&gt;", ">");
        text = text.replace("&amp;", "&");
        text = text.replace("&quot;", "\"");

        // Remove HTML tags
        let tag_re = regex::Regex::new(r"<[^>]+>").unwrap();
        text = tag_re.replace_all(&text, " ").to_string();

        // Collapse whitespace
        let ws_re = regex::Regex::new(r"\s+").unwrap();
        text = ws_re.replace_all(&text, " ").to_string();

        // Limit length to avoid token limits
        if text.len() > 15000 {
            text = text[..15000].to_string();
        }

        Ok(text.trim().to_string())
    }

    /// Parse documentation using LLM (public)
    pub async fn parse_with_llm(
        &self,
        text_content: &str,
        api_domain: &str,
        llm_provider: &dyn LlmProvider,
    ) -> RuntimeResult<LlmApiParseResponse> {
        log::info!("🤖 Using LLM to parse API documentation...");

        let prompt = self.create_extraction_prompt(api_domain, text_content);

        // Call the LLM provider
        let llm_response = llm_provider.generate_text(&prompt).await?;

        // Parse the JSON response
        self.parse_llm_response(&llm_response, api_domain)
    }

    /// Create a structured prompt for API endpoint extraction (public)
    pub fn create_extraction_prompt(&self, api_domain: &str, text_content: &str) -> String {
        format!(
            r#"You are an API documentation parser. Extract REST API endpoint information from the following documentation text.

CRITICAL INSTRUCTION:
'base_url' must be the actual API endpoint where requests are sent (e.g. 'https://api.example.com/v1'), NOT the documentation URL. Look for "Base URL", "Endpoint", "Host", or code blocks showing curl requests.

API Domain: {}

Documentation text:
{}

Extract the following information in JSON format:
{{
  "api_name": "Human readable API name",
  "base_url": "https://api.example.com",
  "version": "1.0",
  "description": "Brief description of what this API does",
  "endpoints": [
    {{
      "name": "Get Weather",
      "description": "Get current weather for a location",
      "method": "GET",
      "path": "/weather",
      "parameters": [
        {{
          "name": "city",
          "type": "string",
          "location": "query",
          "required": true,
          "description": "City name"
        }}
      ],
      "requires_auth": true
    }}
  ],
  "auth": {{
    "auth_type": "api_key",
    "location": "query",
    "param_name": "appid",
    "env_var_suggestion": "EXAMPLE_API_KEY"
  }}
}}

Guidelines:
- auth_type values: bearer, api_key, basic, oauth2, custom
- location values: header, query, cookie
- Only include endpoints that are clearly documented.
- If authentication details are unclear, set auth to null.
Respond with ONLY the JSON object, no additional text."#,
            api_domain, text_content
        )
    }

    /// Parse the LLM response JSON
    fn parse_llm_response(
        &self,
        response: &str,
        api_domain: &str,
    ) -> RuntimeResult<LlmApiParseResponse> {
        // Try to extract JSON from the response (it might have markdown fences)
        let json_str = self.extract_json_from_response(response);

        match serde_json::from_str::<LlmApiParseResponse>(&json_str) {
            Ok(parsed) => {
                log::info!(
                    "✅ Successfully parsed {} endpoints from LLM response",
                    parsed.endpoints.len()
                );
                Ok(parsed)
            }
            Err(e) => {
                log::warn!("Failed to parse LLM response as JSON: {}", e);
                log::debug!("Response was: {}", response);
                Err(RuntimeError::Generic(format!(
                    "Failed to parse LLM response for {}: {}",
                    api_domain, e
                )))
            }
        }
    }

    /// Extract JSON from an LLM response (handles markdown code fences)
    fn extract_json_from_response(&self, response: &str) -> String {
        let trimmed = response.trim();

        // Try to find JSON within markdown code fences
        if let Some(start) = trimmed.find("```json") {
            if let Some(end) = trimmed[start..]
                .find("```\n")
                .or_else(|| trimmed[start..].rfind("```").filter(|&pos| pos > 7))
            {
                let json_start = start + 7; // Skip "```json"
                let actual_end = if trimmed[json_start..].starts_with('\n') {
                    start + end
                } else {
                    start + end
                };
                return trimmed[json_start..actual_end]
                    .trim_start_matches('\n')
                    .to_string();
            }
        }

        // Try generic code fences
        if let Some(start) = trimmed.find("```") {
            if let Some(end) = trimmed[start + 3..].find("```") {
                let inner = &trimmed[start + 3..start + 3 + end];
                // Skip any language identifier on first line
                if let Some(newline_pos) = inner.find('\n') {
                    return inner[newline_pos + 1..].to_string();
                }
            }
        }

        // Return as-is if no fences found
        trimmed.to_string()
    }

    /// Convert LLM response to APIIntrospectionResult
    fn convert_to_introspection_result(
        &self,
        parsed: LlmApiParseResponse,
    ) -> RuntimeResult<APIIntrospectionResult> {
        let endpoints: Vec<DiscoveredEndpoint> = parsed
            .endpoints
            .iter()
            .map(|ep| self.convert_endpoint(ep))
            .collect();

        let auth_requirements = match parsed.auth {
            Some(auth) => AuthRequirements {
                auth_type: auth.auth_type,
                auth_location: auth.location,
                auth_param_name: auth.param_name,
                required: true,
                env_var_name: Some(auth.env_var_suggestion),
            },
            None => AuthRequirements {
                auth_type: "none".to_string(),
                auth_location: String::new(),
                auth_param_name: String::new(),
                required: false,
                env_var_name: None,
            },
        };

        Ok(APIIntrospectionResult {
            base_url: parsed.base_url,
            api_title: parsed.api_name,
            api_version: parsed.version,
            endpoints,
            auth_requirements,
            rate_limits: None,
        })
    }

    /// Convert extracted endpoint to DiscoveredEndpoint
    fn convert_endpoint(&self, ep: &ExtractedEndpoint) -> DiscoveredEndpoint {
        let parameters: Vec<EndpointParameter> = ep
            .parameters
            .iter()
            .map(|p| EndpointParameter {
                name: p.name.clone(),
                param_type: self.parse_type(&p.param_type),
                required: p.required,
                location: p.location.clone(),
                description: Some(p.description.clone()),
            })
            .collect();

        let input_entries: Vec<MapTypeEntry> = ep
            .parameters
            .iter()
            .map(|p| MapTypeEntry {
                key: Keyword(p.name.clone()),
                value_type: Box::new(self.parse_type(&p.param_type)),
                optional: !p.required,
            })
            .collect();

        let input_schema = if input_entries.is_empty() {
            None
        } else {
            Some(TypeExpr::Map {
                entries: input_entries,
                wildcard: None,
            })
        };

        DiscoveredEndpoint {
            endpoint_id: ep.name.to_lowercase().replace(' ', "_"),
            name: ep.name.clone(),
            description: ep.description.clone(),
            method: ep.method.clone(),
            path: ep.path.clone(),
            input_schema,
            output_schema: None,
            requires_auth: ep.requires_auth,
            parameters,
        }
    }

    /// Parse type string to TypeExpr
    fn parse_type(&self, type_str: &str) -> TypeExpr {
        match type_str.to_lowercase().as_str() {
            "string" => TypeExpr::Primitive(PrimitiveType::String),
            "int" | "integer" | "number" => TypeExpr::Primitive(PrimitiveType::Int),
            "float" | "double" => TypeExpr::Primitive(PrimitiveType::Float),
            "bool" | "boolean" => TypeExpr::Primitive(PrimitiveType::Bool),
            _ => TypeExpr::Primitive(PrimitiveType::String),
        }
    }
}

impl Default for LlmDocParser {
    fn default() -> Self {
        Self::new()
    }
}

impl TryFrom<ExtractedAuth> for AuthConfig {
    type Error = rtfs::runtime::error::RuntimeError;

    fn try_from(auth: ExtractedAuth) -> Result<Self, Self::Error> {
        let auth_type_str = auth.auth_type.to_lowercase();
        let auth_type = match auth_type_str.as_str() {
            "apikey" | "api_key" => AuthType::ApiKey,
            "bearer" | "token" => AuthType::Bearer,
            "oauth2" | "oauth" => AuthType::OAuth2,
            "basic" => AuthType::Basic,
            _ => AuthType::Custom,
        };

        // Determine if it's in header or query based on location string
        let in_header = if auth.location.to_lowercase().contains("header") {
            Some(true)
        } else if auth.location.to_lowercase().contains("query") {
            Some(false)
        } else {
            // Default to header for most auth types
            Some(true)
        };

        let header_name = if in_header == Some(true) && !auth.param_name.is_empty() {
            Some(auth.param_name.clone())
        } else {
            None
        };

        Ok(AuthConfig {
            auth_type,
            provider: "default".to_string(),
            key_location: Some(auth.location),
            in_header,
            header_name,
            header_prefix: None,
            username_param: None,
            password_param: None,
            env_var: if !auth.env_var_suggestion.is_empty() {
                Some(auth.env_var_suggestion)
            } else {
                None
            },
            required: true,
            is_secret: true,
        })
    }
}
