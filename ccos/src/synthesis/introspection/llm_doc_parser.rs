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
use crate::synthesis::introspection::{
    APIIntrospectionResult, AuthRequirements, DiscoveredEndpoint, EndpointParameter,
};
use rtfs::ast::{Keyword, MapTypeEntry, PrimitiveType, TypeExpr};
use rtfs::runtime::error::{RuntimeError, RuntimeResult};
use serde::{Deserialize, Serialize};

/// Default documentation URLs for common API domains
/// Used when we need to fetch documentation for LLM parsing
fn get_default_doc_urls(domain: &str) -> Vec<&'static str> {
    match domain {
        "openweathermap.org" | "api.openweathermap.org" => vec![
            "https://openweathermap.org/current",
            "https://openweathermap.org/forecast5",
            "https://openweathermap.org/api/one-call-3",
        ],
        "jsonplaceholder.typicode.com" => {
            vec!["https://jsonplaceholder.typicode.com/guide/"]
        }
        _ => vec![],
    }
}

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
    pub async fn parse_for_domain(
        &self,
        api_domain: &str,
        llm_provider: &dyn LlmProvider,
    ) -> RuntimeResult<APIIntrospectionResult> {
        let doc_urls = get_default_doc_urls(api_domain);

        if doc_urls.is_empty() {
            return Err(RuntimeError::Generic(format!(
                "No documentation URLs known for domain: {}",
                api_domain
            )));
        }

        // Fetch all doc pages and combine
        let mut combined_text = String::new();
        for url in &doc_urls {
            match self.fetch_page(url).await {
                Ok(html) => {
                    if let Ok(text) = self.extract_text_from_html(&html) {
                        combined_text.push_str(&text);
                        combined_text.push_str("\n\n---\n\n");
                    }
                }
                Err(e) => {
                    log::warn!("Failed to fetch {}: {}", url, e);
                }
            }
        }

        if combined_text.is_empty() {
            return Err(RuntimeError::Generic(format!(
                "Failed to fetch any documentation for domain: {}",
                api_domain
            )));
        }

        // Limit total size
        if combined_text.len() > 25000 {
            combined_text = combined_text[..25000].to_string();
        }

        log::info!(
            "🤖 Parsing combined documentation ({} chars) for {}",
            combined_text.len(),
            api_domain
        );

        let parsed = self
            .parse_with_llm(&combined_text, api_domain, llm_provider)
            .await?;
        self.convert_to_introspection_result(parsed)
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

    /// Parse documentation using LLM
    async fn parse_with_llm(
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

    /// Create a structured prompt for API endpoint extraction
    fn create_extraction_prompt(&self, api_domain: &str, text_content: &str) -> String {
        format!(
            r#"You are an API documentation parser. Extract REST API endpoint information from the following documentation text.

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

Only include endpoints that are clearly documented. If authentication details are unclear, set auth to null.
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
            if let Some(end) = trimmed[start..].find("```\n").or_else(|| {
                trimmed[start..]
                    .rfind("```")
                    .filter(|&pos| pos > 7)
            }) {
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
