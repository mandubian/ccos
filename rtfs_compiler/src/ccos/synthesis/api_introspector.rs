use crate::ast::{Keyword, MapTypeEntry, TypeExpr};
use crate::ccos::capability_marketplace::types::CapabilityManifest;
use crate::ccos::synthesis::schema_serializer::type_expr_to_rtfs_pretty;
use crate::runtime::error::{RuntimeError, RuntimeResult};
use serde::{Deserialize, Serialize};

/// API introspection result containing discovered endpoints and their schemas
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct APIIntrospectionResult {
    /// Base URL of the API
    pub base_url: String,
    /// API title/name
    pub api_title: String,
    /// API version
    pub api_version: String,
    /// Discovered endpoints with their schemas
    pub endpoints: Vec<DiscoveredEndpoint>,
    /// Authentication requirements
    pub auth_requirements: AuthRequirements,
    /// Rate limiting information
    pub rate_limits: Option<RateLimitInfo>,
}

/// A discovered API endpoint with its input/output schemas
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DiscoveredEndpoint {
    /// Endpoint identifier (e.g., "get_user_profile")
    pub endpoint_id: String,
    /// Human-readable name
    pub name: String,
    /// Description of what this endpoint does
    pub description: String,
    /// HTTP method (GET, POST, etc.)
    pub method: String,
    /// API path (e.g., "/v1/users/{userId}")
    pub path: String,
    /// Input schema as RTFS TypeExpr
    pub input_schema: Option<TypeExpr>,
    /// Output schema as RTFS TypeExpr
    pub output_schema: Option<TypeExpr>,
    /// Whether authentication is required
    pub requires_auth: bool,
    /// Parameter information
    pub parameters: Vec<EndpointParameter>,
}

/// Parameter information for an endpoint
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EndpointParameter {
    /// Parameter name
    pub name: String,
    /// Parameter type as RTFS TypeExpr
    pub param_type: TypeExpr,
    /// Whether parameter is required
    pub required: bool,
    /// Parameter location (query, path, header, body)
    pub location: String,
    /// Description of the parameter
    pub description: Option<String>,
}

/// Authentication requirements for the API
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuthRequirements {
    /// Type of authentication (api_key, oauth2, bearer, none)
    pub auth_type: String,
    /// Where to put the auth (header, query, body)
    pub auth_location: String,
    /// Auth parameter name (e.g., "api_key", "Authorization")
    pub auth_param_name: String,
    /// Whether auth is required
    pub required: bool,
    /// Environment variable name for the secret
    pub env_var_name: Option<String>,
}

/// Rate limiting information
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RateLimitInfo {
    /// Requests per minute
    pub requests_per_minute: Option<u32>,
    /// Requests per day
    pub requests_per_day: Option<u32>,
    /// Requests per second
    pub requests_per_second: Option<u32>,
}

/// API Introspector for discovering endpoints and schemas from various sources
pub struct APIIntrospector {
    /// Mock mode for testing
    mock_mode: bool,
}

impl APIIntrospector {
    /// Create a new API introspector
    pub fn new() -> Self {
        Self { mock_mode: false }
    }

    /// Create in mock mode for testing
    pub fn mock() -> Self {
        Self { mock_mode: true }
    }

    /// Introspect an API from OpenAPI specification
    pub async fn introspect_from_openapi(
        &self,
        spec_url: &str,
        api_domain: &str,
    ) -> RuntimeResult<APIIntrospectionResult> {
        if self.mock_mode {
            return self.introspect_mock_api(api_domain);
        }

        eprintln!("🔍 Introspecting API from OpenAPI spec: {}", spec_url);

        // Fetch OpenAPI spec
        let spec = self.fetch_openapi_spec(spec_url).await?;

        // Parse API information
        let api_info = self.parse_api_info(&spec)?;

        // Extract endpoints with schemas
        let endpoints = self.extract_endpoints_with_schemas(&spec)?;

        // Extract auth requirements
        let auth_requirements = self.extract_auth_requirements(&spec)?;

        // Extract rate limits
        let rate_limits = self.extract_rate_limits(&spec)?;

        Ok(APIIntrospectionResult {
            base_url: self.extract_base_url(&spec)?,
            api_title: api_info.title,
            api_version: api_info.version,
            endpoints,
            auth_requirements,
            rate_limits,
        })
    }

    /// Introspect an API by making discovery calls
    pub async fn introspect_from_discovery(
        &self,
        base_url: &str,
        api_domain: &str,
    ) -> RuntimeResult<APIIntrospectionResult> {
        if self.mock_mode {
            return self.introspect_mock_api(api_domain);
        }

        eprintln!("🔍 Introspecting API through discovery: {}", base_url);

        // Try to find OpenAPI spec at common locations
        let spec_urls = vec![
            format!("{}/openapi.json", base_url),
            format!("{}/swagger.json", base_url),
            format!("{}/api-docs", base_url),
            format!("{}/.well-known/openapi", base_url),
        ];

        for spec_url in spec_urls {
            if let Ok(result) = self.introspect_from_openapi(&spec_url, api_domain).await {
                return Ok(result);
            }
        }

        // If no OpenAPI spec found, try to discover endpoints by making calls
        self.discover_endpoints_by_calls(base_url, api_domain).await
    }

    /// Create capabilities from introspection results
    pub fn create_capabilities_from_introspection(
        &self,
        introspection: &APIIntrospectionResult,
    ) -> RuntimeResult<Vec<CapabilityManifest>> {
        let mut capabilities = Vec::new();

        for endpoint in &introspection.endpoints {
            let capability = self.create_capability_from_endpoint(endpoint, introspection)?;
            capabilities.push(capability);
        }

        Ok(capabilities)
    }

    /// Fetch OpenAPI specification from URL
    async fn fetch_openapi_spec(&self, spec_url: &str) -> RuntimeResult<serde_json::Value> {
        let client = reqwest::Client::new();
        let response = client
            .get(spec_url)
            .timeout(std::time::Duration::from_secs(30))
            .send()
            .await
            .map_err(|e| RuntimeError::Generic(format!("Failed to fetch OpenAPI spec: {}", e)))?;

        if !response.status().is_success() {
            return Err(RuntimeError::Generic(format!(
                "Failed to fetch OpenAPI spec: HTTP {}",
                response.status()
            )));
        }

        let spec_text = response
            .text()
            .await
            .map_err(|e| RuntimeError::Generic(format!("Failed to read OpenAPI spec: {}", e)))?;

        // Parse JSON or YAML
        if spec_url.ends_with(".yaml") || spec_url.ends_with(".yml") {
            // For now, assume JSON - in production you'd use a YAML parser
            serde_json::from_str(&spec_text)
                .map_err(|e| RuntimeError::Generic(format!("Failed to parse OpenAPI spec: {}", e)))
        } else {
            serde_json::from_str(&spec_text)
                .map_err(|e| RuntimeError::Generic(format!("Failed to parse OpenAPI spec: {}", e)))
        }
    }

    /// Parse API information from OpenAPI spec
    fn parse_api_info(&self, spec: &serde_json::Value) -> RuntimeResult<APIInfo> {
        let info = spec.get("info").ok_or_else(|| {
            RuntimeError::Generic("No 'info' section found in OpenAPI spec".to_string())
        })?;

        let title = info
            .get("title")
            .and_then(|t| t.as_str())
            .unwrap_or("Unknown API")
            .to_string();

        let version = info
            .get("version")
            .and_then(|v| v.as_str())
            .unwrap_or("1.0.0")
            .to_string();

        Ok(APIInfo { title, version })
    }

    /// Extract base URL from OpenAPI spec
    fn extract_base_url(&self, spec: &serde_json::Value) -> RuntimeResult<String> {
        // Try servers section first
        if let Some(servers) = spec.get("servers").and_then(|s| s.as_array()) {
            if let Some(server) = servers.first() {
                if let Some(url) = server.get("url").and_then(|u| u.as_str()) {
                    return Ok(url.to_string());
                }
            }
        }

        // Try host and basePath (OpenAPI 2.0 style)
        if let Some(host) = spec.get("host").and_then(|h| h.as_str()) {
            let scheme = spec
                .get("schemes")
                .and_then(|s| s.as_array())
                .and_then(|arr| arr.first())
                .and_then(|s| s.as_str())
                .unwrap_or("https");
            let base_path = spec.get("basePath").and_then(|p| p.as_str()).unwrap_or("");
            return Ok(format!("{}://{}{}", scheme, host, base_path));
        }

        Err(RuntimeError::Generic(
            "No base URL found in OpenAPI spec".to_string(),
        ))
    }

    /// Extract endpoints with their schemas from OpenAPI spec
    fn extract_endpoints_with_schemas(
        &self,
        spec: &serde_json::Value,
    ) -> RuntimeResult<Vec<DiscoveredEndpoint>> {
        let paths = spec
            .get("paths")
            .and_then(|p| p.as_object())
            .ok_or_else(|| RuntimeError::Generic("No paths found in OpenAPI spec".to_string()))?;

        let mut endpoints = Vec::new();

        for (path, path_item) in paths {
            if let Some(item) = path_item.as_object() {
                for (method, operation) in item {
                    if method == "parameters" || method == "$ref" {
                        continue;
                    }

                    if let Some(op) = operation.as_object() {
                        let endpoint = self.parse_endpoint_with_schema(method, path, op)?;
                        endpoints.push(endpoint);
                    }
                }
            }
        }

        Ok(endpoints)
    }

    /// Parse a single endpoint with its input/output schemas
    fn parse_endpoint_with_schema(
        &self,
        method: &str,
        path: &str,
        operation: &serde_json::Map<String, serde_json::Value>,
    ) -> RuntimeResult<DiscoveredEndpoint> {
        let operation_id = operation
            .get("operationId")
            .and_then(|id| id.as_str())
            .unwrap_or(&format!("{}_{}", method, path.replace("/", "_")))
            .to_string();

        let summary = operation
            .get("summary")
            .and_then(|s| s.as_str())
            .unwrap_or("API endpoint");

        let description = operation
            .get("description")
            .and_then(|d| d.as_str())
            .unwrap_or(summary);

        // Extract parameters and build input schema
        let (parameters, input_schema) = self.extract_parameters_and_schema(operation)?;

        // Extract response schema
        let output_schema = self.extract_response_schema(operation)?;

        // Check if auth is required
        let requires_auth = operation
            .get("security")
            .and_then(|s| s.as_array())
            .map(|arr| !arr.is_empty())
            .unwrap_or(false);

        Ok(DiscoveredEndpoint {
            endpoint_id: operation_id.clone(),
            name: summary.to_string(),
            description: description.to_string(),
            method: method.to_uppercase(),
            path: path.to_string(),
            input_schema,
            output_schema,
            requires_auth,
            parameters,
        })
    }

    /// Extract parameters and build input schema
    fn extract_parameters_and_schema(
        &self,
        operation: &serde_json::Map<String, serde_json::Value>,
    ) -> RuntimeResult<(Vec<EndpointParameter>, Option<TypeExpr>)> {
        let mut parameters = Vec::new();
        let mut map_entries = Vec::new();

        // Extract parameters
        if let Some(params) = operation.get("parameters").and_then(|p| p.as_array()) {
            for param in params {
                if let Some(param_obj) = param.as_object() {
                    let param_info = self.parse_parameter(param_obj)?;
                    parameters.push(param_info.clone());

                    // Add to schema map
                    map_entries.push(MapTypeEntry {
                        key: Keyword(param_info.name.clone()),
                        value_type: Box::new(param_info.param_type.clone()),
                        optional: !param_info.required,
                    });
                }
            }
        }

        // Extract request body
        if let Some(request_body) = operation.get("requestBody") {
            if let Some(content) = request_body.get("content") {
                if let Some(json_content) = content.get("application/json") {
                    if let Some(schema) = json_content.get("schema") {
                        let body_type = self.json_schema_to_rtfs_type(schema)?;
                        map_entries.push(MapTypeEntry {
                            key: Keyword("body".to_string()),
                            value_type: Box::new(body_type),
                            optional: !request_body
                                .get("required")
                                .and_then(|r| r.as_bool())
                                .unwrap_or(false),
                        });
                    }
                }
            }
        }

        let input_schema = if map_entries.is_empty() {
            None
        } else {
            Some(TypeExpr::Map {
                entries: map_entries,
                wildcard: None,
            })
        };

        Ok((parameters, input_schema))
    }

    /// Extract response schema
    fn extract_response_schema(
        &self,
        operation: &serde_json::Map<String, serde_json::Value>,
    ) -> RuntimeResult<Option<TypeExpr>> {
        if let Some(responses) = operation.get("responses").and_then(|r| r.as_object()) {
            // Look for 200 response first
            if let Some(success_response) = responses.get("200") {
                if let Some(content) = success_response.get("content") {
                    if let Some(json_content) = content.get("application/json") {
                        if let Some(schema) = json_content.get("schema") {
                            return Ok(Some(self.json_schema_to_rtfs_type(schema)?));
                        }
                    }
                }
            }

            // Fallback to any successful response
            for (code, response) in responses {
                if code.starts_with('2') {
                    if let Some(content) = response.get("content") {
                        if let Some(json_content) = content.get("application/json") {
                            if let Some(schema) = json_content.get("schema") {
                                return Ok(Some(self.json_schema_to_rtfs_type(schema)?));
                            }
                        }
                    }
                }
            }
        }

        Ok(None)
    }

    /// Parse a parameter from OpenAPI spec
    fn parse_parameter(
        &self,
        param: &serde_json::Map<String, serde_json::Value>,
    ) -> RuntimeResult<EndpointParameter> {
        let name = param
            .get("name")
            .and_then(|n| n.as_str())
            .ok_or_else(|| RuntimeError::Generic("Parameter missing name".to_string()))?
            .to_string();

        let location = param
            .get("in")
            .and_then(|i| i.as_str())
            .unwrap_or("query")
            .to_string();

        let required = param
            .get("required")
            .and_then(|r| r.as_bool())
            .unwrap_or(false);

        let description = param
            .get("description")
            .and_then(|d| d.as_str())
            .map(|s| s.to_string());

        let schema = param
            .get("schema")
            .cloned()
            .unwrap_or_else(|| serde_json::json!({"type": "string"}));

        let param_type = self.json_schema_to_rtfs_type(&schema)?;

        Ok(EndpointParameter {
            name,
            param_type,
            required,
            location,
            description,
        })
    }

    /// Convert JSON schema to RTFS TypeExpr
    fn json_schema_to_rtfs_type(&self, schema: &serde_json::Value) -> RuntimeResult<TypeExpr> {
        if let Some(type_str) = schema.get("type").and_then(|t| t.as_str()) {
            match type_str {
                "string" => Ok(TypeExpr::Primitive(crate::ast::PrimitiveType::String)),
                "integer" => Ok(TypeExpr::Primitive(crate::ast::PrimitiveType::Int)),
                "number" => Ok(TypeExpr::Primitive(crate::ast::PrimitiveType::Float)),
                "boolean" => Ok(TypeExpr::Primitive(crate::ast::PrimitiveType::Bool)),
                "array" => {
                    let element_type = if let Some(items) = schema.get("items") {
                        self.json_schema_to_rtfs_type(items)?
                    } else {
                        TypeExpr::Any
                    };
                    Ok(TypeExpr::Vector(Box::new(element_type)))
                }
                "object" => {
                    let mut entries = Vec::new();
                    if let Some(properties) = schema.get("properties").and_then(|p| p.as_object()) {
                        let required_fields: std::collections::HashSet<String> = schema
                            .get("required")
                            .and_then(|r| r.as_array())
                            .map(|arr| {
                                arr.iter()
                                    .filter_map(|v| v.as_str())
                                    .map(|s| s.to_string())
                                    .collect()
                            })
                            .unwrap_or_default();

                        for (key, prop_schema) in properties {
                            let prop_type = self.json_schema_to_rtfs_type(prop_schema)?;
                            entries.push(MapTypeEntry {
                                key: Keyword(key.clone()),
                                value_type: Box::new(prop_type),
                                optional: !required_fields.contains(key),
                            });
                        }
                    }
                    Ok(TypeExpr::Map {
                        entries,
                        wildcard: None,
                    })
                }
                _ => Ok(TypeExpr::Any),
            }
        } else {
            Ok(TypeExpr::Any)
        }
    }

    /// Extract authentication requirements
    fn extract_auth_requirements(
        &self,
        spec: &serde_json::Value,
    ) -> RuntimeResult<AuthRequirements> {
        // Check for security schemes
        if let Some(security_schemes) = spec
            .get("components")
            .and_then(|c| c.get("securitySchemes"))
        {
            for (name, scheme) in security_schemes
                .as_object()
                .unwrap_or(&serde_json::Map::new())
            {
                if let Some(scheme_type) = scheme.get("type").and_then(|t| t.as_str()) {
                    match scheme_type {
                        "apiKey" => {
                            let location = scheme
                                .get("in")
                                .and_then(|i| i.as_str())
                                .unwrap_or("header");

                            let param_name = scheme
                                .get("name")
                                .and_then(|n| n.as_str())
                                .unwrap_or("api_key");

                            return Ok(AuthRequirements {
                                auth_type: "api_key".to_string(),
                                auth_location: location.to_string(),
                                auth_param_name: param_name.to_string(),
                                required: true,
                                env_var_name: Some(format!("{}_API_KEY", name.to_uppercase())),
                            });
                        }
                        "http" => {
                            return Ok(AuthRequirements {
                                auth_type: "bearer".to_string(),
                                auth_location: "header".to_string(),
                                auth_param_name: "Authorization".to_string(),
                                required: true,
                                env_var_name: Some(format!("{}_TOKEN", name.to_uppercase())),
                            });
                        }
                        _ => continue,
                    }
                }
            }
        }

        // Default to no auth
        Ok(AuthRequirements {
            auth_type: "none".to_string(),
            auth_location: "none".to_string(),
            auth_param_name: "none".to_string(),
            required: false,
            env_var_name: None,
        })
    }

    /// Extract rate limits
    fn extract_rate_limits(
        &self,
        spec: &serde_json::Value,
    ) -> RuntimeResult<Option<RateLimitInfo>> {
        // Try to extract from x-rate-limit extensions
        if let Some(extensions) = spec.get("x-ccos-rate-limits") {
            Ok(Some(RateLimitInfo {
                requests_per_minute: extensions
                    .get("requests_per_minute")
                    .and_then(|v| v.as_u64())
                    .map(|v| v as u32),
                requests_per_day: extensions
                    .get("requests_per_day")
                    .and_then(|v| v.as_u64())
                    .map(|v| v as u32),
                requests_per_second: extensions
                    .get("requests_per_second")
                    .and_then(|v| v.as_u64())
                    .map(|v| v as u32),
            }))
        } else {
            Ok(None)
        }
    }

    /// Create capability from endpoint
    fn create_capability_from_endpoint(
        &self,
        endpoint: &DiscoveredEndpoint,
        introspection: &APIIntrospectionResult,
    ) -> RuntimeResult<CapabilityManifest> {
        let capability_id = format!(
            "{}.{}",
            introspection.api_title.to_lowercase().replace(" ", "_"),
            endpoint.endpoint_id
        );

        let mut effects = vec!["network_request".to_string()];
        if endpoint.requires_auth {
            effects.push("auth_required".to_string());
        }

        let mut metadata = std::collections::HashMap::new();
        metadata.insert("endpoint_method".to_string(), endpoint.method.clone());
        metadata.insert("endpoint_path".to_string(), endpoint.path.clone());
        metadata.insert("base_url".to_string(), introspection.base_url.clone());
        metadata.insert("api_title".to_string(), introspection.api_title.clone());
        metadata.insert("api_version".to_string(), introspection.api_version.clone());
        metadata.insert("introspected".to_string(), "true".to_string());

        if let Some(rate_limits) = &introspection.rate_limits {
            if let Some(rpm) = rate_limits.requests_per_minute {
                metadata.insert("rate_limit_per_minute".to_string(), rpm.to_string());
            }
            if let Some(rpd) = rate_limits.requests_per_day {
                metadata.insert("rate_limit_per_day".to_string(), rpd.to_string());
            }
        }

        Ok(CapabilityManifest {
            id: capability_id,
            name: endpoint.name.clone(),
            description: endpoint.description.clone(),
            provider: crate::ccos::capability_marketplace::types::ProviderType::Local(
                crate::ccos::capability_marketplace::types::LocalCapability {
                    handler: std::sync::Arc::new(|_| {
                        Ok(crate::runtime::values::Value::String(
                            "Introspected API capability placeholder".to_string(),
                        ))
                    }),
                },
            ),
            version: introspection.api_version.clone(),
            input_schema: endpoint.input_schema.clone(),
            output_schema: endpoint.output_schema.clone(),
            attestation: None,
            provenance: Some(
                crate::ccos::capability_marketplace::types::CapabilityProvenance {
                    source: "api_introspector".to_string(),
                    version: Some("1.0.0".to_string()),
                    content_hash: format!("introspected_{}", endpoint.endpoint_id),
                    custody_chain: vec!["api_introspector".to_string()],
                    registered_at: chrono::Utc::now(),
                },
            ),
            permissions: vec!["network.http".to_string()],
            effects,
            metadata,
            agent_metadata: None,
        })
    }

    /// Discover endpoints by making API calls (fallback method)
    async fn discover_endpoints_by_calls(
        &self,
        _base_url: &str,
        api_domain: &str,
    ) -> RuntimeResult<APIIntrospectionResult> {
        // This would implement API discovery by making actual HTTP calls
        // For now, return a mock result
        self.introspect_mock_api(api_domain)
    }

    /// Mock API introspection for testing
    fn introspect_mock_api(&self, api_domain: &str) -> RuntimeResult<APIIntrospectionResult> {
        // Special handling for OpenWeather API
        if api_domain.contains("openweather") {
            return self.introspect_openweather_api();
        }

        // Default mock API
        Ok(APIIntrospectionResult {
            base_url: format!("https://api.{}.example.com", api_domain),
            api_title: format!("{} API", api_domain),
            api_version: "1.0.0".to_string(),
            endpoints: vec![
                DiscoveredEndpoint {
                    endpoint_id: "get_user_profile".to_string(),
                    name: "Get User Profile".to_string(),
                    description: "Retrieve user profile information".to_string(),
                    method: "GET".to_string(),
                    path: "/v1/users/{userId}".to_string(),
                    input_schema: Some(TypeExpr::Map {
                        entries: vec![
                            MapTypeEntry {
                                key: Keyword("userId".to_string()),
                                value_type: Box::new(TypeExpr::Primitive(
                                    crate::ast::PrimitiveType::String,
                                )),
                                optional: false,
                            },
                            MapTypeEntry {
                                key: Keyword("expand".to_string()),
                                value_type: Box::new(TypeExpr::Primitive(
                                    crate::ast::PrimitiveType::Bool,
                                )),
                                optional: true,
                            },
                        ],
                        wildcard: None,
                    }),
                    output_schema: Some(TypeExpr::Map {
                        entries: vec![
                            MapTypeEntry {
                                key: Keyword("id".to_string()),
                                value_type: Box::new(TypeExpr::Primitive(
                                    crate::ast::PrimitiveType::String,
                                )),
                                optional: false,
                            },
                            MapTypeEntry {
                                key: Keyword("name".to_string()),
                                value_type: Box::new(TypeExpr::Primitive(
                                    crate::ast::PrimitiveType::String,
                                )),
                                optional: false,
                            },
                            MapTypeEntry {
                                key: Keyword("email".to_string()),
                                value_type: Box::new(TypeExpr::Primitive(
                                    crate::ast::PrimitiveType::String,
                                )),
                                optional: false,
                            },
                        ],
                        wildcard: None,
                    }),
                    requires_auth: true,
                    parameters: vec![
                        EndpointParameter {
                            name: "userId".to_string(),
                            param_type: TypeExpr::Primitive(crate::ast::PrimitiveType::String),
                            required: true,
                            location: "path".to_string(),
                            description: Some("User identifier".to_string()),
                        },
                        EndpointParameter {
                            name: "expand".to_string(),
                            param_type: TypeExpr::Primitive(crate::ast::PrimitiveType::Bool),
                            required: false,
                            location: "query".to_string(),
                            description: Some("Expand user details".to_string()),
                        },
                    ],
                },
                DiscoveredEndpoint {
                    endpoint_id: "create_activity".to_string(),
                    name: "Create Activity".to_string(),
                    description: "Record user activity events".to_string(),
                    method: "POST".to_string(),
                    path: "/v1/activities".to_string(),
                    input_schema: Some(TypeExpr::Map {
                        entries: vec![MapTypeEntry {
                            key: Keyword("events".to_string()),
                            value_type: Box::new(TypeExpr::Vector(Box::new(TypeExpr::Map {
                                entries: vec![],
                                wildcard: None,
                            }))),
                            optional: false,
                        }],
                        wildcard: None,
                    }),
                    output_schema: Some(TypeExpr::Map {
                        entries: vec![
                            MapTypeEntry {
                                key: Keyword("id".to_string()),
                                value_type: Box::new(TypeExpr::Primitive(
                                    crate::ast::PrimitiveType::String,
                                )),
                                optional: false,
                            },
                            MapTypeEntry {
                                key: Keyword("status".to_string()),
                                value_type: Box::new(TypeExpr::Primitive(
                                    crate::ast::PrimitiveType::String,
                                )),
                                optional: false,
                            },
                        ],
                        wildcard: None,
                    }),
                    requires_auth: true,
                    parameters: vec![EndpointParameter {
                        name: "events".to_string(),
                        param_type: TypeExpr::Vector(Box::new(TypeExpr::Map {
                            entries: vec![],
                            wildcard: None,
                        })),
                        required: true,
                        location: "body".to_string(),
                        description: Some("Activity events to record".to_string()),
                    }],
                },
            ],
            auth_requirements: AuthRequirements {
                auth_type: "bearer".to_string(),
                auth_location: "header".to_string(),
                auth_param_name: "Authorization".to_string(),
                required: true,
                env_var_name: Some(format!("{}_TOKEN", api_domain.to_uppercase())),
            },
            rate_limits: Some(RateLimitInfo {
                requests_per_minute: Some(60),
                requests_per_day: Some(1000),
                requests_per_second: Some(1),
            }),
        })
    }

    /// Mock introspection specifically for OpenWeather API
    fn introspect_openweather_api(&self) -> RuntimeResult<APIIntrospectionResult> {
        Ok(APIIntrospectionResult {
            base_url: "https://api.openweathermap.org".to_string(),
            api_title: "OpenWeather API".to_string(),
            api_version: "2.5".to_string(),
            endpoints: vec![
                // Current Weather Data
                DiscoveredEndpoint {
                    endpoint_id: "get_current_weather".to_string(),
                    name: "Get Current Weather".to_string(),
                    description: "Access current weather data for any location including over 200,000 cities".to_string(),
                    method: "GET".to_string(),
                    path: "/data/2.5/weather".to_string(),
                    input_schema: Some(TypeExpr::Map {
                        entries: vec![
                            MapTypeEntry {
                                key: Keyword("q".to_string()),
                                value_type: Box::new(TypeExpr::Primitive(crate::ast::PrimitiveType::String)),
                                optional: true,
                            },
                            MapTypeEntry {
                                key: Keyword("lat".to_string()),
                                value_type: Box::new(TypeExpr::Primitive(crate::ast::PrimitiveType::Float)),
                                optional: true,
                            },
                            MapTypeEntry {
                                key: Keyword("lon".to_string()),
                                value_type: Box::new(TypeExpr::Primitive(crate::ast::PrimitiveType::Float)),
                                optional: true,
                            },
                            MapTypeEntry {
                                key: Keyword("units".to_string()),
                                value_type: Box::new(TypeExpr::Primitive(crate::ast::PrimitiveType::String)),
                                optional: true,
                            },
                            MapTypeEntry {
                                key: Keyword("lang".to_string()),
                                value_type: Box::new(TypeExpr::Primitive(crate::ast::PrimitiveType::String)),
                                optional: true,
                            },
                        ],
                        wildcard: None,
                    }),
                    output_schema: Some(TypeExpr::Map {
                        entries: vec![
                            MapTypeEntry {
                                key: Keyword("coord".to_string()),
                                value_type: Box::new(TypeExpr::Map {
                                    entries: vec![
                                        MapTypeEntry {
                                            key: Keyword("lon".to_string()),
                                            value_type: Box::new(TypeExpr::Primitive(crate::ast::PrimitiveType::Float)),
                                            optional: false,
                                        },
                                        MapTypeEntry {
                                            key: Keyword("lat".to_string()),
                                            value_type: Box::new(TypeExpr::Primitive(crate::ast::PrimitiveType::Float)),
                                            optional: false,
                                        },
                                    ],
                                    wildcard: None,
                                }),
                                optional: false,
                            },
                            MapTypeEntry {
                                key: Keyword("weather".to_string()),
                                value_type: Box::new(TypeExpr::Vector(Box::new(TypeExpr::Map {
                                    entries: vec![],
                                    wildcard: None,
                                }))),
                                optional: false,
                            },
                            MapTypeEntry {
                                key: Keyword("main".to_string()),
                                value_type: Box::new(TypeExpr::Map {
                                    entries: vec![
                                        MapTypeEntry {
                                            key: Keyword("temp".to_string()),
                                            value_type: Box::new(TypeExpr::Primitive(crate::ast::PrimitiveType::Float)),
                                            optional: false,
                                        },
                                        MapTypeEntry {
                                            key: Keyword("humidity".to_string()),
                                            value_type: Box::new(TypeExpr::Primitive(crate::ast::PrimitiveType::Int)),
                                            optional: false,
                                        },
                                    ],
                                    wildcard: None,
                                }),
                                optional: false,
                            },
                            MapTypeEntry {
                                key: Keyword("name".to_string()),
                                value_type: Box::new(TypeExpr::Primitive(crate::ast::PrimitiveType::String)),
                                optional: false,
                            },
                        ],
                        wildcard: None,
                    }),
                    requires_auth: true,
                    parameters: vec![
                        EndpointParameter {
                            name: "q".to_string(),
                            param_type: TypeExpr::Primitive(crate::ast::PrimitiveType::String),
                            required: false,
                            location: "query".to_string(),
                            description: Some("City name, state code and country code divided by comma (e.g., 'London,UK')".to_string()),
                        },
                        EndpointParameter {
                            name: "lat".to_string(),
                            param_type: TypeExpr::Primitive(crate::ast::PrimitiveType::Float),
                            required: false,
                            location: "query".to_string(),
                            description: Some("Latitude coordinate".to_string()),
                        },
                        EndpointParameter {
                            name: "lon".to_string(),
                            param_type: TypeExpr::Primitive(crate::ast::PrimitiveType::Float),
                            required: false,
                            location: "query".to_string(),
                            description: Some("Longitude coordinate".to_string()),
                        },
                        EndpointParameter {
                            name: "units".to_string(),
                            param_type: TypeExpr::Primitive(crate::ast::PrimitiveType::String),
                            required: false,
                            location: "query".to_string(),
                            description: Some("Units of measurement: 'standard', 'metric', or 'imperial'".to_string()),
                        },
                    ],
                },
                // 5 Day Forecast
                DiscoveredEndpoint {
                    endpoint_id: "get_forecast".to_string(),
                    name: "Get 5 Day Weather Forecast".to_string(),
                    description: "5 day weather forecast with data every 3 hours".to_string(),
                    method: "GET".to_string(),
                    path: "/data/2.5/forecast".to_string(),
                    input_schema: Some(TypeExpr::Map {
                        entries: vec![
                            MapTypeEntry {
                                key: Keyword("q".to_string()),
                                value_type: Box::new(TypeExpr::Primitive(crate::ast::PrimitiveType::String)),
                                optional: true,
                            },
                            MapTypeEntry {
                                key: Keyword("lat".to_string()),
                                value_type: Box::new(TypeExpr::Primitive(crate::ast::PrimitiveType::Float)),
                                optional: true,
                            },
                            MapTypeEntry {
                                key: Keyword("lon".to_string()),
                                value_type: Box::new(TypeExpr::Primitive(crate::ast::PrimitiveType::Float)),
                                optional: true,
                            },
                            MapTypeEntry {
                                key: Keyword("cnt".to_string()),
                                value_type: Box::new(TypeExpr::Primitive(crate::ast::PrimitiveType::Int)),
                                optional: true,
                            },
                        ],
                        wildcard: None,
                    }),
                    output_schema: Some(TypeExpr::Map {
                        entries: vec![
                            MapTypeEntry {
                                key: Keyword("cnt".to_string()),
                                value_type: Box::new(TypeExpr::Primitive(crate::ast::PrimitiveType::Int)),
                                optional: false,
                            },
                            MapTypeEntry {
                                key: Keyword("list".to_string()),
                                value_type: Box::new(TypeExpr::Vector(Box::new(TypeExpr::Map {
                                    entries: vec![],
                                    wildcard: None,
                                }))),
                                optional: false,
                            },
                            MapTypeEntry {
                                key: Keyword("city".to_string()),
                                value_type: Box::new(TypeExpr::Map {
                                    entries: vec![
                                        MapTypeEntry {
                                            key: Keyword("name".to_string()),
                                            value_type: Box::new(TypeExpr::Primitive(crate::ast::PrimitiveType::String)),
                                            optional: false,
                                        },
                                    ],
                                    wildcard: None,
                                }),
                                optional: false,
                            },
                        ],
                        wildcard: None,
                    }),
                    requires_auth: true,
                    parameters: vec![
                        EndpointParameter {
                            name: "q".to_string(),
                            param_type: TypeExpr::Primitive(crate::ast::PrimitiveType::String),
                            required: false,
                            location: "query".to_string(),
                            description: Some("City name".to_string()),
                        },
                        EndpointParameter {
                            name: "cnt".to_string(),
                            param_type: TypeExpr::Primitive(crate::ast::PrimitiveType::Int),
                            required: false,
                            location: "query".to_string(),
                            description: Some("Number of timestamps to return (max 40)".to_string()),
                        },
                    ],
                },
            ],
            auth_requirements: AuthRequirements {
                auth_type: "api_key".to_string(),
                auth_location: "query".to_string(),
                auth_param_name: "appid".to_string(),
                required: true,
                env_var_name: Some("OPENWEATHERMAP_ORG_API_KEY".to_string()),
            },
            rate_limits: Some(RateLimitInfo {
                requests_per_minute: Some(60),
                requests_per_day: Some(1000),
                requests_per_second: None,
            }),
        })
    }

    /// Convert TypeExpr to RTFS schema string (using shared utility)
    fn type_expr_to_rtfs_string(expr: &TypeExpr) -> String {
        type_expr_to_rtfs_pretty(expr)
    }

    /// Serialize capability to RTFS format
    pub fn capability_to_rtfs_string(
        &self,
        capability: &CapabilityManifest,
        implementation_code: &str,
    ) -> String {
        let input_schema_str = capability
            .input_schema
            .as_ref()
            .map(|s| Self::type_expr_to_rtfs_string(s))
            .unwrap_or_else(|| ":any".to_string());

        let output_schema_str = capability
            .output_schema
            .as_ref()
            .map(|s| Self::type_expr_to_rtfs_string(s))
            .unwrap_or_else(|| ":any".to_string());

        let permissions_str = if capability.permissions.is_empty() {
            "[]".to_string()
        } else {
            format!(
                "[{}]",
                capability
                    .permissions
                    .iter()
                    .map(|p| format!(":{}", p))
                    .collect::<Vec<_>>()
                    .join(" ")
            )
        };

        let effects_str = if capability.effects.is_empty() {
            "[]".to_string()
        } else {
            format!(
                "[{}]",
                capability
                    .effects
                    .iter()
                    .map(|e| format!(":{}", e))
                    .collect::<Vec<_>>()
                    .join(" ")
            )
        };

        let base_url = capability
            .metadata
            .get("base_url")
            .map(|s| s.as_str())
            .unwrap_or("");
        let endpoint_path = capability
            .metadata
            .get("endpoint_path")
            .map(|s| s.as_str())
            .unwrap_or("/");
        let endpoint_method = capability
            .metadata
            .get("endpoint_method")
            .map(|s| s.as_str())
            .unwrap_or("GET");
        let api_title = capability
            .metadata
            .get("api_title")
            .map(|s| s.as_str())
            .unwrap_or("API");

        format!(
            r#";; Capability: {}
;; Generated from API introspection
;; Base URL: {}
;; Endpoint: {} {}

(capability "{}"
  :name "{}"
  :version "{}"
  :description "{}"
  :provider "{}"
  :permissions {}
  :effects {}
  :metadata {{
    :openapi {{
      :base_url "{}"
      :endpoint_method "{}"
      :endpoint_path "{}"
    }}
    :discovery {{
      :method "api_introspection"
      :source_url "{}"
      :created_at "{}"
      :capability_type "specialized_http_api"
    }}
  }}
  :input-schema {}
  :output-schema {}
  :implementation
    {}
)
"#,
            capability.name,
            base_url,
            endpoint_method,
            endpoint_path,
            capability.id,
            capability.name,
            capability.version,
            capability.description,
            api_title, // Use api_title from metadata instead of provider enum
            permissions_str,
            effects_str,
            base_url,
            endpoint_method,
            endpoint_path,
            base_url,
            chrono::Utc::now().to_rfc3339(),
            input_schema_str,
            output_schema_str,
            implementation_code
        )
    }

    /// Save capability to RTFS file
    ///
    /// Uses hierarchical directory structure:
    /// output_dir/openapi/<api_name>/<endpoint_name>.rtfs
    ///
    /// Example: capabilities/openapi/openweather/get_current_weather.rtfs
    pub fn save_capability_to_rtfs(
        &self,
        capability: &CapabilityManifest,
        implementation_code: &str,
        output_dir: &std::path::Path,
    ) -> RuntimeResult<std::path::PathBuf> {
        // Parse capability ID: "openweather_api.get_current_weather" or similar
        // Extract API name and endpoint name from ID
        let parts: Vec<&str> = capability.id.split('.').collect();
        if parts.len() < 2 {
            return Err(RuntimeError::Generic(format!(
                "Invalid capability ID format: {}. Expected: <api>.<endpoint>",
                capability.id
            )));
        }

        // Extract api name (remove _api suffix if present)
        let api_name_raw = parts[0];
        let api_name = api_name_raw.trim_end_matches("_api");

        // Extract endpoint name (join all remaining parts)
        let endpoint_name = parts[1..].join("_");

        // Create directory: output_dir/openapi/<api_name>/
        let capability_dir = output_dir.join("openapi").join(api_name);
        std::fs::create_dir_all(&capability_dir).map_err(|e| {
            RuntimeError::Generic(format!("Failed to create capability directory: {}", e))
        })?;

        let rtfs_content = self.capability_to_rtfs_string(capability, implementation_code);
        let rtfs_file = capability_dir.join(format!("{}.rtfs", endpoint_name));

        std::fs::write(&rtfs_file, rtfs_content)
            .map_err(|e| RuntimeError::Generic(format!("Failed to write RTFS file: {}", e)))?;

        Ok(rtfs_file)
    }
}

/// API information
#[derive(Debug, Clone)]
struct APIInfo {
    title: String,
    version: String,
}

impl Default for APIIntrospector {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_api_introspector_creation() {
        let introspector = APIIntrospector::new();
        assert!(!introspector.mock_mode);
    }

    #[test]
    fn test_api_introspector_mock() {
        let introspector = APIIntrospector::mock();
        assert!(introspector.mock_mode);
    }

    #[tokio::test]
    async fn test_introspect_mock_api() {
        let introspector = APIIntrospector::mock();
        let result = introspector.introspect_mock_api("testapi").unwrap();

        assert_eq!(result.api_title, "testapi API");
        assert_eq!(result.endpoints.len(), 2);
        assert!(result
            .endpoints
            .iter()
            .any(|e| e.endpoint_id == "get_user_profile"));
        assert!(result
            .endpoints
            .iter()
            .any(|e| e.endpoint_id == "create_activity"));
    }

    #[test]
    fn test_create_capabilities_from_introspection() {
        let introspector = APIIntrospector::mock();
        let introspection = introspector.introspect_mock_api("testapi").unwrap();
        let capabilities = introspector
            .create_capabilities_from_introspection(&introspection)
            .unwrap();

        assert_eq!(capabilities.len(), 2);
        assert!(capabilities
            .iter()
            .any(|c| c.id.contains("get_user_profile")));
        assert!(capabilities
            .iter()
            .any(|c| c.id.contains("create_activity")));

        // Check that schemas are properly encoded
        let profile_cap = capabilities
            .iter()
            .find(|c| c.id.contains("get_user_profile"))
            .unwrap();
        assert!(profile_cap.input_schema.is_some());
        assert!(profile_cap.output_schema.is_some());
    }
}
