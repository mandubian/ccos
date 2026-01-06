use crate::capability_marketplace::types::{
    CapabilityManifest, CapabilityProvenance, EffectType, OpenApiAuth, OpenApiCapability,
    OpenApiOperation, ProviderType,
};
use crate::synthesis::introspection::auth_injector::AuthInjector;
// removed unused find_workspace_root import
use chrono::Utc;
use rtfs::ast::TypeExpr;
use rtfs::runtime::error::{RuntimeError, RuntimeResult};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;

/// OpenAPI Operation (endpoint) information
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OpenAPIOperation {
    /// HTTP method (get, post, put, delete, etc.)
    pub method: String,
    /// Path template (e.g., /repos/{owner}/{repo})
    pub path: String,
    /// Operation ID
    pub operation_id: Option<String>,
    /// Summary/title
    pub summary: Option<String>,
    /// Description
    pub description: Option<String>,
    /// Required parameters
    pub parameters: Vec<OpenAPIParameter>,
    /// Request body schema
    pub request_body: Option<serde_json::Value>,
    /// Response schemas
    pub responses: HashMap<String, serde_json::Value>,
    /// Security requirements
    pub security: Vec<String>,
}

/// OpenAPI Parameter information
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OpenAPIParameter {
    /// Parameter name
    pub name: String,
    /// Parameter location (query, path, header, cookie)
    pub in_location: String,
    /// Parameter description
    pub description: Option<String>,
    /// Whether parameter is required
    pub required: bool,
    /// Parameter schema/type
    pub schema: serde_json::Value,
}

/// RTFS Capability Metadata for OpenAPI-based capabilities
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OpenAPICapabilityMetadata {
    /// API information
    pub api_info: OpenAPIInfo,
    /// Rate limiting information
    pub rate_limits: RateLimitInfo,
    /// Authentication requirements
    pub auth_requirements: AuthRequirements,
    /// Available endpoints
    pub endpoints: Vec<OpenAPIOperation>,
    /// RTFS function definitions
    pub rtfs_functions: Vec<RTFSFunction>,
}

/// OpenAPI Info section
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OpenAPIInfo {
    pub title: String,
    pub description: Option<String>,
    pub version: String,
    pub contact: Option<ContactInfo>,
    pub license: Option<LicenseInfo>,
}

impl Default for OpenAPIInfo {
    fn default() -> Self {
        Self {
            title: "Unknown API".to_string(),
            description: None,
            version: "1.0.0".to_string(),
            contact: None,
            license: None,
        }
    }
}

/// Contact information
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContactInfo {
    pub name: Option<String>,
    pub url: Option<String>,
    pub email: Option<String>,
}

/// License information
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LicenseInfo {
    pub name: String,
    pub url: Option<String>,
}

/// Rate limiting information
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RateLimitInfo {
    /// Free tier limits
    pub free_tier: Option<TierLimits>,
    /// Paid tier limits
    pub paid_tier: Option<TierLimits>,
    /// Current tier being used
    pub current_tier: String,
}

/// Tier limits
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TierLimits {
    /// Requests per day
    pub requests_per_day: Option<u32>,
    /// Requests per minute
    pub requests_per_minute: Option<u32>,
    /// Requests per second
    pub requests_per_second: Option<u32>,
    /// Monthly requests
    pub requests_per_month: Option<u32>,
}

/// Authentication requirements
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuthRequirements {
    /// Authentication type (api_key, oauth2, bearer, etc.)
    pub auth_type: String,
    /// Where to put the auth (header, query, body)
    pub auth_location: String,
    /// Auth parameter name (e.g., "api_key", "Authorization")
    pub auth_param_name: String,
    /// Whether auth is required
    pub required: bool,
    /// Environment variable name for the secret (NOT stored in capability)
    pub env_var_name: Option<String>,
}

/// RTFS Function definition
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RTFSFunction {
    /// Function name in RTFS
    pub name: String,
    /// Description
    pub description: String,
    /// HTTP method
    pub method: String,
    /// API path
    pub path: String,
    /// RTFS function signature
    pub signature: String,
    /// RTFS implementation
    pub implementation: String,
    /// Input parameters
    pub parameters: Vec<RTFSParameter>,
    /// Return type
    pub return_type: String,
}

/// RTFS Parameter definition
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RTFSParameter {
    pub name: String,
    pub param_type: String,
    pub description: Option<String>,
    pub required: bool,
    pub default_value: Option<String>,
}

/// OpenAPI Importer for converting OpenAPI specs to CCOS capabilities
pub struct OpenAPIImporter {
    /// Base URL for the API
    pub base_url: String,
    /// Auth injector for handling credentials
    auth_injector: AuthInjector,
    /// Mock mode for testing
    mock_mode: bool,
    /// Storage directory for capabilities
    storage_dir: PathBuf,
}

impl OpenAPIImporter {
    /// Create a new OpenAPI importer
    pub fn new(base_url: String) -> Self {
        let storage_dir = std::env::var("CCOS_CAPABILITY_STORAGE")
            .map(PathBuf::from)
            .unwrap_or_else(|_| crate::utils::fs::get_configured_capabilities_path());

        Self {
            base_url,
            auth_injector: AuthInjector::new(),
            mock_mode: false,
            storage_dir,
        }
    }

    /// Create in mock mode for testing
    pub fn mock(base_url: String) -> Self {
        Self {
            base_url,
            auth_injector: AuthInjector::mock(),
            mock_mode: true,
            storage_dir: PathBuf::from("/tmp/ccos_capabilities"),
        }
    }

    /// Create a complete RTFS capability from OpenAPI spec
    pub async fn create_rtfs_capability(
        &self,
        spec_url: &str,
        capability_id: &str,
    ) -> RuntimeResult<CapabilityManifest> {
        eprintln!(
            "🔧 Creating RTFS capability from OpenAPI spec: {}",
            spec_url
        );

        // Load OpenAPI spec
        let spec = self.load_spec(spec_url).await?;

        // Extract operations
        let operations = self.extract_operations(&spec)?;

        // Parse API info
        let api_info = self.parse_api_info(&spec)?;

        // Extract rate limits from spec or infer from common patterns
        let rate_limits = self.extract_rate_limits(&spec, &api_info)?;

        // Extract auth requirements
        let auth_requirements = self.extract_auth_requirements(&spec)?;

        // Generate RTFS functions
        let rtfs_functions = self.generate_rtfs_functions(&operations, &api_info)?;

        // Create metadata
        let metadata = OpenAPICapabilityMetadata {
            api_info,
            rate_limits,
            auth_requirements,
            endpoints: operations,
            rtfs_functions,
        };

        // Create capability manifest
        let mut manifest =
            self.create_capability_manifest(capability_id, Some(spec_url), &metadata)?;

        // Save capability to storage
        let storage_path = self.save_capability(&manifest, &metadata).await?;

        manifest.metadata.insert(
            "storage_path".to_string(),
            storage_path.display().to_string(),
        );

        eprintln!("✅ Created and saved RTFS capability: {}", capability_id);
        Ok(manifest)
    }

    /// Load and parse an OpenAPI specification
    pub async fn load_spec(&self, spec_url: &str) -> RuntimeResult<serde_json::Value> {
        if self.mock_mode {
            return self.load_mock_spec();
        }

        // Try to fetch from URL
        eprintln!("📥 Loading OpenAPI spec from: {}", spec_url);

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

    /// Parse OpenAPI spec and extract operations
    pub fn extract_operations(
        &self,
        spec: &serde_json::Value,
    ) -> RuntimeResult<Vec<OpenAPIOperation>> {
        let paths = spec
            .get("paths")
            .and_then(|p| p.as_object())
            .ok_or_else(|| RuntimeError::Generic("No paths found in OpenAPI spec".to_string()))?;

        let mut operations = Vec::new();

        for (path, path_item) in paths {
            if let Some(item) = path_item.as_object() {
                for (method, operation) in item {
                    if method == "parameters" || method == "$ref" {
                        continue;
                    }

                    if let Some(op) = operation.as_object() {
                        let op_info = self.parse_operation(method, path, op)?;
                        operations.push(op_info);
                    }
                }
            }
        }

        Ok(operations)
    }

    /// Parse API info from OpenAPI spec
    fn parse_api_info(&self, spec: &serde_json::Value) -> RuntimeResult<OpenAPIInfo> {
        let info = spec.get("info").ok_or_else(|| {
            RuntimeError::Generic("No 'info' section found in OpenAPI spec".to_string())
        })?;

        let title = info
            .get("title")
            .and_then(|t| t.as_str())
            .unwrap_or("Unknown API")
            .to_string();

        let description = info
            .get("description")
            .and_then(|d| d.as_str())
            .map(|s| s.to_string());

        let version = info
            .get("version")
            .and_then(|v| v.as_str())
            .unwrap_or("1.0.0")
            .to_string();

        let contact = info.get("contact").map(|c| ContactInfo {
            name: c
                .get("name")
                .and_then(|n| n.as_str())
                .map(|s| s.to_string()),
            url: c.get("url").and_then(|u| u.as_str()).map(|s| s.to_string()),
            email: c
                .get("email")
                .and_then(|e| e.as_str())
                .map(|s| s.to_string()),
        });

        let license = info.get("license").map(|l| LicenseInfo {
            name: l
                .get("name")
                .and_then(|n| n.as_str())
                .unwrap_or("Unknown")
                .to_string(),
            url: l.get("url").and_then(|u| u.as_str()).map(|s| s.to_string()),
        });

        Ok(OpenAPIInfo {
            title,
            description,
            version,
            contact,
            license,
        })
    }

    /// Extract rate limits from OpenAPI spec or infer from common patterns
    fn extract_rate_limits(
        &self,
        spec: &serde_json::Value,
        api_info: &OpenAPIInfo,
    ) -> RuntimeResult<RateLimitInfo> {
        // Try to extract from x-rate-limit extensions
        let free_tier = if let Some(extensions) = spec.get("x-ccos-rate-limits") {
            Some(TierLimits {
                requests_per_day: extensions
                    .get("free_requests_per_day")
                    .and_then(|v| v.as_u64())
                    .map(|v| v as u32),
                requests_per_minute: extensions
                    .get("free_requests_per_minute")
                    .and_then(|v| v.as_u64())
                    .map(|v| v as u32),
                requests_per_second: extensions
                    .get("free_requests_per_second")
                    .and_then(|v| v.as_u64())
                    .map(|v| v as u32),
                requests_per_month: extensions
                    .get("free_requests_per_month")
                    .and_then(|v| v.as_u64())
                    .map(|v| v as u32),
            })
        } else {
            // Infer from common API patterns
            match api_info.title.to_lowercase().as_str() {
                title if title.contains("openweather") => Some(TierLimits {
                    requests_per_day: Some(1000),
                    requests_per_minute: Some(60),
                    requests_per_second: Some(1),
                    requests_per_month: None,
                }),
                title if title.contains("github") => Some(TierLimits {
                    requests_per_day: Some(5000),
                    requests_per_minute: Some(60),
                    requests_per_second: Some(10),
                    requests_per_month: None,
                }),
                _ => Some(TierLimits {
                    requests_per_day: Some(1000),
                    requests_per_minute: Some(60),
                    requests_per_second: Some(1),
                    requests_per_month: None,
                }),
            }
        };

        Ok(RateLimitInfo {
            free_tier,
            paid_tier: None, // Could be extracted from spec extensions
            current_tier: "free".to_string(),
        })
    }

    /// Extract authentication requirements from OpenAPI spec
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
                            let _scheme_name = scheme
                                .get("scheme")
                                .and_then(|s| s.as_str())
                                .unwrap_or("bearer");

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

    /// Generate RTFS functions from OpenAPI operations
    fn generate_rtfs_functions(
        &self,
        operations: &[OpenAPIOperation],
        api_info: &OpenAPIInfo,
    ) -> RuntimeResult<Vec<RTFSFunction>> {
        let mut functions = Vec::new();

        for operation in operations {
            let function_name = self.generate_function_name(operation, api_info);
            let signature = self.generate_function_signature(operation);
            let implementation = self.generate_function_implementation(operation, api_info);
            let parameters = self.generate_function_parameters(operation);

            functions.push(RTFSFunction {
                name: function_name.clone(),
                description: operation
                    .summary
                    .clone()
                    .unwrap_or_else(|| operation.description.clone().unwrap_or_default()),
                method: operation.method.to_uppercase(),
                path: operation.path.clone(),
                signature,
                implementation,
                parameters,
                return_type: "Map".to_string(), // Default to Map for JSON responses
            });
        }

        Ok(functions)
    }

    /// Generate function name from operation
    fn generate_function_name(
        &self,
        operation: &OpenAPIOperation,
        _api_info: &OpenAPIInfo,
    ) -> String {
        if let Some(operation_id) = &operation.operation_id {
            // Use operation ID if available
            operation_id.to_lowercase().replace("-", "_")
        } else {
            // Generate from method and path
            let method = operation.method.to_lowercase();
            let path_parts: Vec<&str> = operation
                .path
                .split('/')
                .filter(|part| !part.is_empty() && !part.starts_with('{'))
                .collect();

            if path_parts.is_empty() {
                format!("{}_{}", method, "root")
            } else {
                format!("{}_{}", method, path_parts.join("_"))
            }
        }
    }

    /// Generate RTFS function signature
    fn generate_function_signature(&self, operation: &OpenAPIOperation) -> String {
        let mut params = Vec::new();

        for param in &operation.parameters {
            let param_type = self.map_openapi_type_to_rtfs(&param.schema);
            let param_def = if param.required {
                format!(":{} {}", param.name, param_type)
            } else {
                format!(":{} {}?", param.name, param_type)
            };
            params.push(param_def);
        }

        format!(
            "(defn {} [{}] ...)",
            self.generate_function_name(operation, &OpenAPIInfo::default()),
            params.join(" ")
        )
    }

    /// Generate RTFS function implementation
    fn generate_function_implementation(
        &self,
        operation: &OpenAPIOperation,
        api_info: &OpenAPIInfo,
    ) -> String {
        let method = operation.method.to_uppercase();
        let path = &operation.path;

        format!(
            r#"
(defn {} [{}]
  "{}"
  (call :http.request
    :method "{}"
    :url (str "{}" "{}")
    :headers {{"Content-Type" "application/json"}}
    :params {{{}}}
    :auth (call :ccos.auth.inject :service "{}")))
"#,
            self.generate_function_name(operation, api_info),
            operation
                .parameters
                .iter()
                .map(|p| format!(":{}", p.name))
                .collect::<Vec<_>>()
                .join(" "),
            operation.summary.as_deref().unwrap_or("API call"),
            method,
            self.base_url,
            path,
            operation
                .parameters
                .iter()
                .map(|p| format!(":{} {}", p.name, p.name))
                .collect::<Vec<_>>()
                .join(" "),
            api_info.title.to_lowercase().replace(" ", "_")
        )
    }

    /// Generate function parameters
    fn generate_function_parameters(&self, operation: &OpenAPIOperation) -> Vec<RTFSParameter> {
        operation
            .parameters
            .iter()
            .map(|param| RTFSParameter {
                name: param.name.clone(),
                param_type: self.map_openapi_type_to_rtfs(&param.schema),
                description: param.description.clone(),
                required: param.required,
                default_value: None,
            })
            .collect()
    }

    /// Map OpenAPI type to RTFS type
    fn map_openapi_type_to_rtfs(&self, schema: &serde_json::Value) -> String {
        if let Some(type_str) = schema.get("type").and_then(|t| t.as_str()) {
            match type_str {
                "string" => "String".to_string(),
                "integer" => "Integer".to_string(),
                "number" => "Float".to_string(),
                "boolean" => "Boolean".to_string(),
                "array" => "List".to_string(),
                "object" => "Map".to_string(),
                _ => "Any".to_string(),
            }
        } else {
            "Any".to_string()
        }
    }

    /// Create capability manifest
    fn create_capability_manifest(
        &self,
        capability_id: &str,
        spec_url: Option<&str>,
        metadata: &OpenAPICapabilityMetadata,
    ) -> RuntimeResult<CapabilityManifest> {
        let rtfs_code = self.generate_rtfs_module(metadata);

        // Create simple TypeExpr for schemas
        let input_schema = TypeExpr::Map {
            entries: vec![],
            wildcard: None,
        };
        let output_schema = TypeExpr::Map {
            entries: vec![],
            wildcard: None,
        };

        // Create metadata map
        let mut manifest_metadata = HashMap::new();
        manifest_metadata.insert("rtfs_code".to_string(), rtfs_code);
        manifest_metadata.insert(
            "openapi_metadata".to_string(),
            serde_json::to_string(metadata).unwrap_or_default(),
        );
        manifest_metadata.insert(
            "rate_limits".to_string(),
            serde_json::to_string(&metadata.rate_limits).unwrap_or_default(),
        );
        manifest_metadata.insert(
            "auth_requirements".to_string(),
            serde_json::to_string(&metadata.auth_requirements).unwrap_or_default(),
        );

        if let Some(spec) = spec_url {
            manifest_metadata.insert("openapi_spec_url".to_string(), spec.to_string());
        }
        manifest_metadata.insert("openapi_base_url".to_string(), self.base_url.clone());

        Ok(CapabilityManifest {
            id: capability_id.to_string(),
            name: metadata.api_info.title.clone(),
            description: metadata.api_info.description.clone().unwrap_or_default(),
            version: metadata.api_info.version.clone(),
            provider: ProviderType::OpenApi(OpenApiCapability {
                base_url: self.base_url.clone(),
                spec_url: spec_url.map(|s| s.to_string()),
                operations: metadata
                    .endpoints
                    .iter()
                    .map(Self::convert_operation)
                    .collect(),
                auth: Self::convert_auth(&metadata.auth_requirements),
                timeout_ms: 30000,
            }),
            input_schema: Some(input_schema),
            output_schema: Some(output_schema),
            attestation: None,
            provenance: Some(CapabilityProvenance {
                source: "openapi_importer".to_string(),
                version: Some("1.0.0".to_string()),
                content_hash: format!("openapi_{}", capability_id.replace("/", "_")),
                custody_chain: vec!["openapi_importer".to_string()],
                registered_at: Utc::now(),
            }),
            permissions: vec!["http_request".to_string()],
            effects: vec!["network_call".to_string()],
            metadata: manifest_metadata,
            agent_metadata: None,
            domains: Vec::new(),
            categories: Vec::new(),
            effect_type: EffectType::default(),
        })
    }

    fn convert_operation(operation: &OpenAPIOperation) -> OpenApiOperation {
        OpenApiOperation {
            operation_id: operation.operation_id.clone(),
            method: operation.method.to_uppercase(),
            path: operation.path.clone(),
            summary: operation.summary.clone(),
            description: operation.description.clone(),
        }
    }

    fn convert_auth(requirements: &AuthRequirements) -> Option<OpenApiAuth> {
        if !requirements.required
            && requirements.auth_type.is_empty()
            && requirements.auth_param_name.is_empty()
        {
            return None;
        }
        Some(OpenApiAuth {
            auth_type: requirements.auth_type.clone(),
            location: requirements.auth_location.clone(),
            parameter_name: requirements.auth_param_name.clone(),
            env_var_name: requirements.env_var_name.clone(),
            required: requirements.required,
        })
    }

    /// Generate complete RTFS module
    fn generate_rtfs_module(&self, metadata: &OpenAPICapabilityMetadata) -> String {
        let mut module = String::new();

        module.push_str(&format!(
            ";; RTFS Module for {} API\n",
            metadata.api_info.title
        ));
        module.push_str(&format!(
            ";; Generated from OpenAPI spec v{}\n",
            metadata.api_info.version
        ));
        module.push_str(&format!(";; Base URL: {}\n\n", self.base_url));

        // Add rate limiting metadata
        if let Some(free_tier) = &metadata.rate_limits.free_tier {
            module.push_str(";; Rate Limits (Free Tier):\n");
            if let Some(per_day) = free_tier.requests_per_day {
                module.push_str(&format!(";; - {} requests per day\n", per_day));
            }
            if let Some(per_minute) = free_tier.requests_per_minute {
                module.push_str(&format!(";; - {} requests per minute\n", per_minute));
            }
            module.push_str("\n");
        }

        // Add auth requirements
        if metadata.auth_requirements.required {
            module.push_str(&format!(
                ";; Authentication: {} ({}: {})\n",
                metadata.auth_requirements.auth_type,
                metadata.auth_requirements.auth_location,
                metadata.auth_requirements.auth_param_name
            ));
            if let Some(env_var) = &metadata.auth_requirements.env_var_name {
                module.push_str(&format!(";; Environment variable: {}\n", env_var));
            }
            module.push_str("\n");
        }

        // Add all function definitions
        for function in &metadata.rtfs_functions {
            module.push_str(&function.implementation);
            module.push_str("\n\n");
        }

        module
    }

    /// Save capability to storage
    async fn save_capability(
        &self,
        manifest: &CapabilityManifest,
        metadata: &OpenAPICapabilityMetadata,
    ) -> RuntimeResult<PathBuf> {
        // Create storage directory if it doesn't exist
        fs::create_dir_all(&self.storage_dir).map_err(|e| {
            RuntimeError::Generic(format!("Failed to create storage directory: {}", e))
        })?;

        let capability_dir = self.storage_dir.join(&manifest.id);
        fs::create_dir_all(&capability_dir).map_err(|e| {
            RuntimeError::Generic(format!("Failed to create capability directory: {}", e))
        })?;

        // Save manifest (skip serialization for now since CapabilityManifest doesn't implement Serialize)
        // TODO: Add Serialize derive to CapabilityManifest or create a serializable version
        eprintln!("💾 Manifest created for capability: {}", manifest.id);

        // Save capability metadata as RTFS
        let metadata_path = capability_dir.join("capability.rtfs");
        let metadata_rtfs = format!(
            r#"
;; Capability metadata for {}
;; Generated from OpenAPI import
;; Source URL: {}

(capability "{}"
  :name "{}"
  :version "{}"
  :description "{}"
  :source_url "{}"
  :discovery_method "openapi_import"
  :created_at "{}"
  :capability_type "openapi_generated"
  :provider :http
  :permissions [:network.http]
  :effects [:network_request]
  :api_info {{:title "{}"
              :version "{}"
              :description "{}"}}
  :rate_limits {{:free_tier {{:requests_per_minute {}
                              :requests_per_day {}}}
                 :paid_tier {{:requests_per_minute {}
                              :requests_per_day {}}}}}
  :auth_requirements {{:type "{}"
                       :location "{}"
                       :parameter_name "{}"
                       :env_var_hint "{}"}}
  :implementation
    (do
      ;; Load the module functions
      (load "module.rtfs")
      ;; Return a generic HTTP request function
      (fn [endpoint params]
        "Generic HTTP request to the API"
        (get endpoint params))))

;; Additional metadata functions
(defn is-recent? []
  "Check if this capability was discovered recently"
  (< (days-since (parse-date "{}")) 30))

(defn source-domain []
  "Extract domain from source URL"
  (let [url "{}"]
    (second (split url "/"))))

(defn api-title []
  "Get the API title from OpenAPI spec"
  "{}")

(defn rate-limits []
  "Get rate limit information"
  {{:free_tier {{:requests_per_minute {}
                 :requests_per_day {}}}
    :paid_tier {{:requests_per_minute {}
                 :requests_per_day {}}}}})

(defn auth-requirements []
  "Get authentication requirements"
  {{:type "{}"
    :location "{}"
    :parameter_name "{}"
    :env_var_hint "{}"}})
"#,
            manifest.name,
            metadata.api_info.title,
            manifest.id,
            manifest.name,
            manifest.version,
            manifest.description,
            metadata.api_info.title,
            chrono::Utc::now().to_rfc3339(),
            metadata.api_info.title,
            metadata.api_info.version,
            metadata.api_info.description.as_deref().unwrap_or(""),
            metadata
                .rate_limits
                .free_tier
                .as_ref()
                .and_then(|t| t.requests_per_minute)
                .unwrap_or(0),
            metadata
                .rate_limits
                .free_tier
                .as_ref()
                .and_then(|t| t.requests_per_day)
                .unwrap_or(0),
            metadata
                .rate_limits
                .paid_tier
                .as_ref()
                .and_then(|t| t.requests_per_minute)
                .unwrap_or(0),
            metadata
                .rate_limits
                .paid_tier
                .as_ref()
                .and_then(|t| t.requests_per_day)
                .unwrap_or(0),
            metadata.auth_requirements.auth_type,
            metadata.auth_requirements.auth_location,
            metadata.auth_requirements.auth_param_name,
            metadata
                .auth_requirements
                .env_var_name
                .as_deref()
                .unwrap_or(""),
            chrono::Utc::now().to_rfc3339(),
            metadata.api_info.title,
            metadata.api_info.title,
            metadata
                .rate_limits
                .free_tier
                .as_ref()
                .and_then(|t| t.requests_per_minute)
                .unwrap_or(0),
            metadata
                .rate_limits
                .free_tier
                .as_ref()
                .and_then(|t| t.requests_per_day)
                .unwrap_or(0),
            metadata
                .rate_limits
                .paid_tier
                .as_ref()
                .and_then(|t| t.requests_per_minute)
                .unwrap_or(0),
            metadata
                .rate_limits
                .paid_tier
                .as_ref()
                .and_then(|t| t.requests_per_day)
                .unwrap_or(0),
            metadata.auth_requirements.auth_type,
            metadata.auth_requirements.auth_location,
            metadata.auth_requirements.auth_param_name,
            metadata
                .auth_requirements
                .env_var_name
                .as_deref()
                .unwrap_or("")
        );

        fs::write(&metadata_path, metadata_rtfs).map_err(|e| {
            RuntimeError::Generic(format!("Failed to write capability metadata: {}", e))
        })?;

        // Save RTFS code from metadata
        if let Some(rtfs_code) = manifest.metadata.get("rtfs_code") {
            let rtfs_path = capability_dir.join("module.rtfs");
            fs::write(&rtfs_path, rtfs_code)
                .map_err(|e| RuntimeError::Generic(format!("Failed to write RTFS code: {}", e)))?;
        }

        eprintln!("💾 Saved capability to: {}", capability_dir.display());
        Ok(metadata_path)
    }

    /// Parse a single OpenAPI operation
    fn parse_operation(
        &self,
        method: &str,
        path: &str,
        operation: &serde_json::Map<String, serde_json::Value>,
    ) -> RuntimeResult<OpenAPIOperation> {
        let operation_id = operation
            .get("operationId")
            .and_then(|id| id.as_str())
            .map(|s| s.to_string());

        let summary = operation
            .get("summary")
            .and_then(|s| s.as_str())
            .map(|s| s.to_string());

        let description = operation
            .get("description")
            .and_then(|d| d.as_str())
            .map(|s| s.to_string());

        let mut parameters = Vec::new();
        if let Some(params) = operation.get("parameters").and_then(|p| p.as_array()) {
            for param in params {
                if let Some(param_obj) = param.as_object() {
                    parameters.push(self.parse_parameter(param_obj)?);
                }
            }
        }

        let request_body = operation.get("requestBody").cloned();

        let mut responses = HashMap::new();
        if let Some(resp) = operation.get("responses").and_then(|r| r.as_object()) {
            for (code, schema) in resp {
                responses.insert(code.clone(), schema.clone());
            }
        }

        let security = operation
            .get("security")
            .and_then(|s| s.as_array())
            .map(|arr| {
                arr.iter()
                    .filter_map(|v| v.as_object())
                    .flat_map(|obj| obj.keys().cloned())
                    .collect()
            })
            .unwrap_or_default();

        Ok(OpenAPIOperation {
            method: method.to_uppercase(),
            path: path.to_string(),
            operation_id,
            summary,
            description,
            parameters,
            request_body,
            responses,
            security,
        })
    }

    /// Parse an OpenAPI parameter
    fn parse_parameter(
        &self,
        param: &serde_json::Map<String, serde_json::Value>,
    ) -> RuntimeResult<OpenAPIParameter> {
        let name = param
            .get("name")
            .and_then(|n| n.as_str())
            .ok_or_else(|| RuntimeError::Generic("Parameter missing name".to_string()))?
            .to_string();

        let in_location = param
            .get("in")
            .and_then(|i| i.as_str())
            .unwrap_or("query")
            .to_string();

        let description = param
            .get("description")
            .and_then(|d| d.as_str())
            .map(|s| s.to_string());

        let required = param
            .get("required")
            .and_then(|r| r.as_bool())
            .unwrap_or(false);

        let schema = param
            .get("schema")
            .cloned()
            .unwrap_or_else(|| serde_json::json!({"type": "string"}));

        Ok(OpenAPIParameter {
            name,
            in_location,
            description,
            required,
            schema,
        })
    }

    /// Convert OpenAPI operation to CCOS capability
    pub fn operation_to_capability(
        &self,
        operation: &OpenAPIOperation,
        api_name: &str,
    ) -> RuntimeResult<CapabilityManifest> {
        // Generate capability ID from operation
        let capability_id = format!(
            "openapi.{}.{}.{}",
            api_name,
            operation.method.to_lowercase(),
            operation
                .operation_id
                .as_ref()
                .unwrap_or(&"operation".to_string())
        );

        let description = operation
            .description
            .clone()
            .or_else(|| operation.summary.clone())
            .unwrap_or_else(|| format!("{} {}", operation.method, operation.path));

        // Build parameters map
        let mut parameters_map = HashMap::new();
        for param in &operation.parameters {
            let param_type = self.json_schema_to_rtfs_type(&param.schema);
            parameters_map.insert(param.name.clone(), param_type);
        }

        // Mark if auth is required
        let mut effects = vec![":network".to_string()];
        if !operation.security.is_empty() {
            effects.push(":auth".to_string());
            // Add auth_token parameter if security is required
            if parameters_map.get("auth_token").is_none() {
                parameters_map.insert("auth_token".to_string(), ":string".to_string());
            }
        }

        // Build metadata
        let mut metadata = HashMap::new();
        metadata.insert("openapi_method".to_string(), operation.method.clone());
        metadata.insert("openapi_path".to_string(), operation.path.clone());
        metadata.insert("openapi_base_url".to_string(), self.base_url.clone());
        if !operation.security.is_empty() {
            metadata.insert("auth_required".to_string(), "true".to_string());
            metadata.insert("auth_providers".to_string(), operation.security.join(","));
        }

        Ok(CapabilityManifest {
            id: capability_id,
            name: operation
                .operation_id
                .clone()
                .unwrap_or_else(|| format!("{} {}", operation.method, operation.path)),
            description,
            provider: crate::capability_marketplace::types::ProviderType::Local(
                crate::capability_marketplace::types::LocalCapability {
                    handler: std::sync::Arc::new(|_| {
                        Ok(rtfs::runtime::values::Value::String(
                            "OpenAPI operation placeholder".to_string(),
                        ))
                    }),
                },
            ),
            version: "1.0.0".to_string(),
            input_schema: None,
            output_schema: None,
            attestation: None,
            provenance: Some(crate::capability_marketplace::types::CapabilityProvenance {
                source: "openapi_importer".to_string(),
                version: Some("1.0.0".to_string()),
                content_hash: format!("openapi_{}_{}", operation.method, operation.path),
                custody_chain: vec!["openapi_importer".to_string()],
                registered_at: chrono::Utc::now(),
            }),
            permissions: vec![],
            effects,
            metadata,
            agent_metadata: None,
            domains: Vec::new(),
            categories: Vec::new(),
            effect_type: EffectType::default(),
        })
    }

    /// Convert JSON Schema type to RTFS keyword type
    pub fn json_schema_to_rtfs_type(&self, schema: &serde_json::Value) -> String {
        if let Some(schema_type) = schema.get("type").and_then(|t| t.as_str()) {
            match schema_type {
                "string" => ":string".to_string(),
                "number" => ":number".to_string(),
                "integer" => ":number".to_string(),
                "boolean" => ":boolean".to_string(),
                "array" => ":list".to_string(),
                "object" => ":map".to_string(),
                _ => ":any".to_string(),
            }
        } else {
            ":any".to_string()
        }
    }

    /// Load mock OpenAPI spec for testing
    fn load_mock_spec(&self) -> RuntimeResult<serde_json::Value> {
        Ok(serde_json::json!({
            "openapi": "3.0.0",
            "info": {
                "title": "Test API",
                "version": "1.0.0"
            },
            "paths": {
                "/test": {
                    "get": {
                        "operationId": "getTest",
                        "summary": "Get test data",
                        "parameters": [
                            {
                                "name": "id",
                                "in": "query",
                                "required": true,
                                "schema": {"type": "string"}
                            }
                        ],
                        "responses": {
                            "200": {
                                "description": "Success"
                            }
                        }
                    }
                }
            }
        }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_openapi_importer_creation() {
        let importer = OpenAPIImporter::new("https://api.example.com".to_string());
        assert_eq!(importer.base_url, "https://api.example.com");
    }

    #[test]
    fn test_json_schema_to_rtfs_type() {
        let importer = OpenAPIImporter::mock("https://api.example.com".to_string());

        let string_schema = serde_json::json!({"type": "string"});
        assert_eq!(importer.json_schema_to_rtfs_type(&string_schema), ":string");

        let number_schema = serde_json::json!({"type": "number"});
        assert_eq!(importer.json_schema_to_rtfs_type(&number_schema), ":number");

        let boolean_schema = serde_json::json!({"type": "boolean"});
        assert_eq!(
            importer.json_schema_to_rtfs_type(&boolean_schema),
            ":boolean"
        );
    }

    #[tokio::test]
    async fn test_load_mock_spec() {
        let importer = OpenAPIImporter::mock("https://api.example.com".to_string());
        let spec = importer.load_spec("mock").await.unwrap();

        assert_eq!(
            spec.get("openapi"),
            Some(&serde_json::Value::String("3.0.0".to_string()))
        );
    }
}
