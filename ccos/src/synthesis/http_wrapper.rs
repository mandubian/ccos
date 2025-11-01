use crate::capability_marketplace::types::CapabilityManifest;
use crate::synthesis::auth_injector::{AuthInjector, AuthType};
use rtfs::runtime::error::{RuntimeError, RuntimeResult};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// HTTP endpoint information
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HTTPEndpoint {
    /// HTTP method (GET, POST, PUT, DELETE, etc.)
    pub method: String,
    /// URL path
    pub path: String,
    /// Query parameters
    pub query_params: Vec<String>,
    /// Path parameters (e.g., {id} in /users/{id})
    pub path_params: Vec<String>,
    /// Headers that might be required
    pub headers: Vec<String>,
    /// Whether auth is required
    pub requires_auth: bool,
    /// Detected auth type
    pub auth_type: Option<AuthType>,
    /// Example request body (if any)
    pub example_body: Option<serde_json::Value>,
    /// Example response
    pub example_response: Option<serde_json::Value>,
}

/// HTTP API introspection result
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HTTPAPIInfo {
    /// Base URL of the API
    pub base_url: String,
    /// API name
    pub api_name: String,
    /// Discovered endpoints
    pub endpoints: Vec<HTTPEndpoint>,
    /// Common headers used across endpoints
    pub common_headers: HashMap<String, String>,
    /// Authentication requirements
    pub auth_requirements: Vec<AuthType>,
}

/// HTTP/JSON Generic Wrapper for wrapping unknown HTTP APIs
pub struct HTTPWrapper {
    /// Base URL for the API
    pub base_url: String,
    /// Auth injector for handling credentials
    auth_injector: AuthInjector,
    /// Mock mode for testing
    mock_mode: bool,
}

impl HTTPWrapper {
    /// Create a new HTTP wrapper
    pub fn new(base_url: String) -> Self {
        Self {
            base_url,
            auth_injector: AuthInjector::new(),
            mock_mode: false,
        }
    }

    /// Create in mock mode for testing
    pub fn mock(base_url: String) -> Self {
        Self {
            base_url,
            auth_injector: AuthInjector::mock(),
            mock_mode: true,
        }
    }

    /// Introspect an HTTP API to discover endpoints
    pub async fn introspect_api(&self, api_name: &str) -> RuntimeResult<HTTPAPIInfo> {
        if self.mock_mode {
            return self.get_mock_api_info(api_name);
        }

        eprintln!("🔍 Introspecting HTTP API: {}", self.base_url);

        // In real implementation, this would:
        // 1. Try common endpoints (/api, /docs, /swagger, /openapi.json)
        // 2. Parse API documentation if available
        // 3. Make sample requests to detect auth requirements
        // 4. Extract parameter patterns from responses

        Err(RuntimeError::Generic(
            "HTTP API introspection not yet implemented - requires HTTP client".to_string(),
        ))
    }

    /// Detect authentication scheme by making test requests
    pub async fn detect_auth_scheme(&self) -> RuntimeResult<Option<AuthType>> {
        if self.mock_mode {
            return Ok(Some(AuthType::Bearer));
        }

        // Strategy:
        // 1. Make request without auth → expect 401
        // 2. Check response headers for auth hints (WWW-Authenticate, etc.)
        // 3. Try different auth schemes based on hints
        // 4. Return detected auth type

        eprintln!("🔐 Detecting auth scheme for: {}", self.base_url);

        // Placeholder implementation
        Ok(Some(AuthType::Bearer))
    }

    /// Extract parameters from URL pattern
    pub fn extract_parameters_from_path(&self, path: &str) -> Vec<String> {
        let mut params = Vec::new();

        // Find {param} patterns
        let mut chars = path.chars().peekable();
        while let Some(c) = chars.next() {
            if c == '{' {
                let mut param = String::new();
                while let Some(next_c) = chars.next() {
                    if next_c == '}' {
                        break;
                    }
                    param.push(next_c);
                }
                if !param.is_empty() {
                    params.push(param);
                }
            }
        }

        params
    }

    /// Infer query parameters from example URLs or documentation
    pub fn infer_query_parameters(&self, _endpoint: &HTTPEndpoint) -> Vec<String> {
        // In real implementation, this would analyze:
        // - Documentation
        // - Example requests
        // - Response error messages that mention missing parameters

        vec!["page".to_string(), "limit".to_string(), "sort".to_string()]
    }

    /// Convert HTTP endpoint to CCOS capability
    pub fn endpoint_to_capability(
        &self,
        endpoint: &HTTPEndpoint,
        api_name: &str,
    ) -> RuntimeResult<CapabilityManifest> {
        let capability_id = format!(
            "http.{}.{}.{}",
            api_name,
            endpoint.method.to_lowercase(),
            endpoint
                .path
                .replace("/", "_")
                .replace("{", "")
                .replace("}", "")
        );

        let description = format!(
            "HTTP {} {} - {}",
            endpoint.method,
            endpoint.path,
            if endpoint.requires_auth {
                "Requires authentication"
            } else {
                "Public endpoint"
            }
        );

        // Build parameters map
        let mut parameters_map = HashMap::new();

        // Add path parameters
        for param in &endpoint.path_params {
            parameters_map.insert(param.clone(), ":string".to_string());
        }

        // Add query parameters
        for param in &endpoint.query_params {
            if !parameters_map.contains_key(param) {
                parameters_map.insert(param.clone(), ":string".to_string());
            }
        }

        // Add auth_token parameter if auth is required
        let mut effects = vec![":network".to_string()];
        if endpoint.requires_auth {
            effects.push(":auth".to_string());
            parameters_map.insert("auth_token".to_string(), ":string".to_string());
        }

        // Add optional request body parameter for POST/PUT/PATCH
        if matches!(endpoint.method.as_str(), "POST" | "PUT" | "PATCH") {
            parameters_map.insert("request_body".to_string(), ":map".to_string());
        }

        // Build metadata
        let mut metadata = HashMap::new();
        metadata.insert("http_method".to_string(), endpoint.method.clone());
        metadata.insert("http_path".to_string(), endpoint.path.clone());
        metadata.insert("http_base_url".to_string(), self.base_url.clone());
        metadata.insert("wrapper_type".to_string(), "http_generic".to_string());

        if endpoint.requires_auth {
            metadata.insert("auth_required".to_string(), "true".to_string());
            if let Some(auth_type) = &endpoint.auth_type {
                metadata.insert("auth_type".to_string(), auth_type.to_string());
            }
        }

        // Add parameter information
        if !endpoint.path_params.is_empty() {
            metadata.insert("path_params".to_string(), endpoint.path_params.join(","));
        }
        if !endpoint.query_params.is_empty() {
            metadata.insert("query_params".to_string(), endpoint.query_params.join(","));
        }

        Ok(CapabilityManifest {
            id: capability_id,
            name: format!(
                "{}_{}",
                endpoint.method.to_lowercase(),
                endpoint
                    .path
                    .replace("/", "_")
                    .replace("{", "")
                    .replace("}", "")
            ),
            description,
            provider: crate::capability_marketplace::types::ProviderType::Local(
                crate::capability_marketplace::types::LocalCapability {
                    handler: std::sync::Arc::new(|_| {
                        Ok(rtfs::runtime::values::Value::String(
                            "HTTP endpoint placeholder".to_string(),
                        ))
                    }),
                },
            ),
            version: "1.0.0".to_string(),
            input_schema: None,
            output_schema: None,
            attestation: None,
            provenance: Some(
                crate::capability_marketplace::types::CapabilityProvenance {
                    source: "http_wrapper".to_string(),
                    version: Some("1.0.0".to_string()),
                    content_hash: format!("http_{}_{}", endpoint.method, endpoint.path),
                    custody_chain: vec!["http_wrapper".to_string()],
                    registered_at: chrono::Utc::now(),
                },
            ),
            permissions: vec![],
            effects,
            metadata,
            agent_metadata: None,
        })
    }

    /// Generate HTTP request code for the capability
    pub fn generate_http_request_code(&self, endpoint: &HTTPEndpoint) -> RuntimeResult<String> {
        let mut code = String::new();

        // Build URL construction
        code.push_str("(let url (str base_url \"/\" ");

        // Replace path parameters
        let path_parts: Vec<String> = endpoint
            .path
            .split('/')
            .map(|part| {
                if part.starts_with('{') && part.ends_with('}') {
                    let param_name = &part[1..part.len() - 1]; // Remove { }
                    format!("(str \"{}\")", param_name)
                } else {
                    part.to_string()
                }
            })
            .collect();

        code.push_str(&format!("\"{}\"", path_parts.join("/")));
        code.push_str("))\n");

        // Build headers
        code.push_str("(let headers {:Content-Type \"application/json\"");
        if endpoint.requires_auth {
            code.push_str(" :Authorization (call :ccos.auth.inject {:provider \"http_api\" :type :bearer :token auth_token})");
        }
        code.push_str("})\n");

        // Build query parameters
        if !endpoint.query_params.is_empty() {
            code.push_str("(let query_params {");
            for param in &endpoint.query_params {
                code.push_str(&format!(" :{} {} ", param, param));
            }
            code.push_str("})\n");
        } else {
            code.push_str("(let query_params {})\n");
        }

        // Make HTTP request
        let method_lower = endpoint.method.to_lowercase();
        code.push_str(&format!("(let response (call :http.{} ", method_lower));
        code.push_str("{:url url :headers headers");

        if !endpoint.query_params.is_empty() {
            code.push_str(" :query query_params");
        }

        // Add request body for POST/PUT/PATCH
        if matches!(endpoint.method.as_str(), "POST" | "PUT" | "PATCH") {
            code.push_str(" :body (call :json.serialize request_body)");
        }

        code.push_str("}))\n");

        // Parse response
        code.push_str("(call :json.parse response)");

        Ok(code)
    }

    /// Analyze response to infer parameter types
    pub fn infer_parameter_types(&self, _endpoint: &HTTPEndpoint) -> HashMap<String, String> {
        let mut types = HashMap::new();

        // In real implementation, this would:
        // - Analyze example responses
        // - Look at error messages for type hints
        // - Use heuristics based on parameter names

        // Default type mapping based on common patterns
        types.insert("page".to_string(), ":number".to_string());
        types.insert("limit".to_string(), ":number".to_string());
        types.insert("id".to_string(), ":string".to_string());
        types.insert("sort".to_string(), ":string".to_string());
        types.insert("filter".to_string(), ":string".to_string());

        types
    }

    /// Get mock API info for testing
    fn get_mock_api_info(&self, api_name: &str) -> RuntimeResult<HTTPAPIInfo> {
        let endpoints = vec![
            HTTPEndpoint {
                method: "GET".to_string(),
                path: "/users".to_string(),
                query_params: vec!["page".to_string(), "limit".to_string()],
                path_params: vec![],
                headers: vec!["Content-Type".to_string()],
                requires_auth: false,
                auth_type: None,
                example_body: None,
                example_response: Some(serde_json::json!({"users": [], "total": 0})),
            },
            HTTPEndpoint {
                method: "GET".to_string(),
                path: "/users/{id}".to_string(),
                query_params: vec![],
                path_params: vec!["id".to_string()],
                headers: vec!["Content-Type".to_string()],
                requires_auth: true,
                auth_type: Some(AuthType::Bearer),
                example_body: None,
                example_response: Some(serde_json::json!({"id": "123", "name": "John Doe"})),
            },
            HTTPEndpoint {
                method: "POST".to_string(),
                path: "/users".to_string(),
                query_params: vec![],
                path_params: vec![],
                headers: vec!["Content-Type".to_string()],
                requires_auth: true,
                auth_type: Some(AuthType::Bearer),
                example_body: Some(
                    serde_json::json!({"name": "John Doe", "email": "john@example.com"}),
                ),
                example_response: Some(
                    serde_json::json!({"id": "123", "name": "John Doe", "email": "john@example.com"}),
                ),
            },
        ];

        Ok(HTTPAPIInfo {
            base_url: self.base_url.clone(),
            api_name: api_name.to_string(),
            endpoints,
            common_headers: HashMap::from([
                ("Content-Type".to_string(), "application/json".to_string()),
                ("Accept".to_string(), "application/json".to_string()),
            ]),
            auth_requirements: vec![AuthType::Bearer],
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_http_wrapper_creation() {
        let wrapper = HTTPWrapper::new("https://api.example.com".to_string());
        assert_eq!(wrapper.base_url, "https://api.example.com");
    }

    #[test]
    fn test_extract_parameters_from_path() {
        let wrapper = HTTPWrapper::mock("https://api.example.com".to_string());

        let params = wrapper.extract_parameters_from_path("/users/{id}/posts/{post_id}");
        assert_eq!(params, vec!["id", "post_id"]);

        let params = wrapper.extract_parameters_from_path("/users");
        assert_eq!(params, Vec::<String>::new());
    }

    #[test]
    fn test_infer_parameter_types() {
        let wrapper = HTTPWrapper::mock("https://api.example.com".to_string());
        let endpoint = HTTPEndpoint {
            method: "GET".to_string(),
            path: "/users".to_string(),
            query_params: vec!["page".to_string(), "id".to_string()],
            path_params: vec![],
            headers: vec![],
            requires_auth: false,
            auth_type: None,
            example_body: None,
            example_response: None,
        };

        let types = wrapper.infer_parameter_types(&endpoint);
        assert_eq!(types.get("page"), Some(&":number".to_string()));
        assert_eq!(types.get("id"), Some(&":string".to_string()));
    }

    #[tokio::test]
    async fn test_introspect_mock_api() {
        let wrapper = HTTPWrapper::mock("https://api.example.com".to_string());
        let api_info = wrapper.introspect_api("test_api").await.unwrap();

        assert_eq!(api_info.api_name, "test_api");
        assert!(!api_info.endpoints.is_empty());
        assert!(api_info.endpoints.iter().any(|e| e.path == "/users"));
    }

    #[test]
    fn test_endpoint_to_capability() {
        let wrapper = HTTPWrapper::mock("https://api.example.com".to_string());

        let endpoint = HTTPEndpoint {
            method: "GET".to_string(),
            path: "/users/{id}".to_string(),
            query_params: vec!["include".to_string()],
            path_params: vec!["id".to_string()],
            headers: vec!["Content-Type".to_string()],
            requires_auth: true,
            auth_type: Some(AuthType::Bearer),
            example_body: None,
            example_response: None,
        };

        let capability = wrapper
            .endpoint_to_capability(&endpoint, "test_api")
            .unwrap();

        assert!(
            capability.id.contains("http")
                && capability.id.contains("test_api")
                && capability.id.contains("get")
                && capability.id.contains("users")
        );
        assert!(capability.effects.contains(&":auth".to_string()));
        assert!(capability.metadata.get("auth_required") == Some(&"true".to_string()));
    }

    #[test]
    fn test_generate_http_request_code() {
        let wrapper = HTTPWrapper::mock("https://api.example.com".to_string());

        let endpoint = HTTPEndpoint {
            method: "POST".to_string(),
            path: "/users".to_string(),
            query_params: vec![],
            path_params: vec![],
            headers: vec!["Content-Type".to_string()],
            requires_auth: true,
            auth_type: Some(AuthType::Bearer),
            example_body: None,
            example_response: None,
        };

        let code = wrapper.generate_http_request_code(&endpoint).unwrap();

        assert!(code.contains("(call :http.post"));
        assert!(code.contains(":body (call :json.serialize request_body)"));
        assert!(code.contains("(call :ccos.auth.inject"));
    }
}
