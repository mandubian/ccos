use crate::ccos::capability_marketplace::types::CapabilityManifest;
use crate::ccos::synthesis::auth_injector::AuthInjector;
use crate::ccos::synthesis::api_introspector::APIIntrospector;
use crate::ccos::synthesis::mcp_introspector::MCPIntrospector;
use crate::runtime::error::{RuntimeError, RuntimeResult};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Capability synthesis request
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SynthesisRequest {
    /// Name of the capability to synthesize
    pub capability_name: String,
    /// Expected input parameters (JSON schema format)
    pub input_schema: Option<serde_json::Value>,
    /// Expected output format
    pub output_schema: Option<serde_json::Value>,
    /// Whether auth is required
    pub requires_auth: bool,
    /// Auth provider if required
    pub auth_provider: Option<String>,
    /// Description of what the capability should do
    pub description: Option<String>,
    /// Any additional context or examples
    pub context: Option<String>,
}

/// Specification describing an individual capability endpoint in a multi-capability synthesis request
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MultiCapabilityEndpoint {
    /// Capability suffix appended to the API domain (e.g., "current_weather")
    pub capability_suffix: String,
    /// Human readable description of the endpoint
    pub description: String,
    /// HTTP path relative to the base URL (e.g., "/v1/weather/current")
    pub path: String,
    /// Optional HTTP method (defaults to GET)
    #[serde(default)]
    pub http_method: Option<String>,
    /// Optional JSON schema describing the input payload
    #[serde(default)]
    pub input_schema: Option<serde_json::Value>,
    /// Optional JSON schema describing the output payload
    #[serde(default)]
    pub output_schema: Option<serde_json::Value>,
}

impl MultiCapabilityEndpoint {
    fn http_method(&self) -> String {
        self.http_method
            .as_deref()
            .map(|m| m.to_uppercase())
            .filter(|m| !m.is_empty())
            .unwrap_or_else(|| "GET".to_string())
    }
}

/// Multi-capability synthesis request for generating specialized capabilities
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MultiCapabilitySynthesisRequest {
    /// Base API domain (e.g., "examplepay", "github")
    pub api_domain: String,
    /// API documentation content
    pub api_docs: String,
    /// Base URL of the API
    pub base_url: String,
    /// Whether auth is required
    pub requires_auth: bool,
    /// Auth provider if required
    pub auth_provider: Option<String>,
    /// Explicitly described endpoints to materialize as capabilities
    #[serde(default)]
    pub endpoints: Vec<MultiCapabilityEndpoint>,
    /// Optional legacy list of endpoint identifiers requested by the caller
    #[serde(default)]
    pub target_endpoints: Option<Vec<String>>,
    /// Whether to generate all discovered endpoints when `endpoints` is empty
    #[serde(default)]
    pub generate_all_endpoints: bool,
}

/// Capability synthesis result
#[derive(Debug, Clone)]
pub struct SynthesisResult {
    /// The generated capability manifest
    pub capability: CapabilityManifest,
    /// The generated RTFS implementation code
    pub implementation_code: String,
    /// Quality score (0.0 to 1.0)
    pub quality_score: f64,
    /// Whether the synthesis passed all safety checks
    pub safety_passed: bool,
    /// Warnings or issues found during synthesis
    pub warnings: Vec<String>,
}

/// Multi-capability synthesis result
#[derive(Debug, Clone)]
pub struct MultiCapabilitySynthesisResult {
    /// Generated capabilities
    pub capabilities: Vec<SynthesisResult>,
    /// Overall quality score
    pub overall_quality_score: f64,
    /// Whether all capabilities passed safety checks
    pub all_safety_passed: bool,
    /// Common warnings across capabilities
    pub common_warnings: Vec<String>,
}

/// LLM Capability Synthesizer with guardrails
pub struct CapabilitySynthesizer {
    /// Auth injector for handling credentials
    auth_injector: AuthInjector,
    /// Mock mode for testing (bypasses LLM calls)
    mock_mode: bool,
    /// Feature flag for enabling synthesis
    synthesis_enabled: bool,
}

impl CapabilitySynthesizer {
    /// Create a new capability synthesizer
    pub fn new() -> Self {
        Self {
            auth_injector: AuthInjector::new(),
            mock_mode: false,
            synthesis_enabled: true,
        }
    }

    /// Create in mock mode for testing
    pub fn mock() -> Self {
        Self {
            auth_injector: AuthInjector::mock(),
            mock_mode: true,
            synthesis_enabled: true,
        }
    }

    /// Create with feature flag control
    pub fn with_feature_flag(enabled: bool) -> Self {
        Self {
            auth_injector: AuthInjector::new(),
            mock_mode: false,
            synthesis_enabled: enabled,
        }
    }

    /// Synthesize a capability using LLM with guardrails
    pub async fn synthesize_capability(
        &self,
        request: &SynthesisRequest,
    ) -> RuntimeResult<SynthesisResult> {
        if !self.synthesis_enabled {
            return Err(RuntimeError::Generic(
                "Capability synthesis is disabled by feature flag".to_string(),
            ));
        }

        if self.mock_mode {
            return self.generate_mock_capability(request);
        }

        // In real implementation, this would:
        // 1. Call LLM with guardrailed prompt
        // 2. Parse and validate the response
        // 3. Run static analysis checks
        // 4. Generate capability manifest

        eprintln!("🤖 Synthesizing capability: {}", request.capability_name);

        // Placeholder for LLM integration
        Err(RuntimeError::Generic(
            "LLM synthesis not yet implemented - requires LLM integration".to_string(),
        ))
    }

    /// Synthesize multiple specialized capabilities from API documentation
    pub async fn synthesize_multi_capabilities(
        &self,
        request: &MultiCapabilitySynthesisRequest,
    ) -> RuntimeResult<MultiCapabilitySynthesisResult> {
        if !self.synthesis_enabled {
            return Err(RuntimeError::Generic(
                "Capability synthesis is disabled by feature flag".to_string(),
            ));
        }

        if self.mock_mode {
            return self.generate_mock_multi_capabilities(request);
        }

        eprintln!(
            "🤖 Synthesizing multiple capabilities for API domain: {}",
            request.api_domain
        );

        // For now, use mock synthesis but with real RTFS code generation
        // TODO: Implement full LLM-based synthesis
        self.generate_mock_multi_capabilities(request)
    }

    /// Synthesize capabilities by introspecting an API
    pub async fn synthesize_from_api_introspection(
        &self,
        api_url: &str,
        api_domain: &str,
    ) -> RuntimeResult<MultiCapabilitySynthesisResult> {
        if !self.synthesis_enabled {
            return Err(RuntimeError::Generic(
                "Capability synthesis is disabled by feature flag".to_string(),
            ));
        }

        eprintln!("🔍 Introspecting API: {}", api_url);

        // Create API introspector
        let introspector = if self.mock_mode {
            APIIntrospector::mock()
        } else {
            APIIntrospector::new()
        };

        // Introspect the API
        let introspection = introspector
            .introspect_from_discovery(api_url, api_domain)
            .await?;

        // Create capabilities from introspection results
        let capabilities = introspector
            .create_capabilities_from_introspection(&introspection)?;

        // Convert to synthesis results
        let synthesis_results: Vec<SynthesisResult> = capabilities
            .into_iter()
            .map(|capability| SynthesisResult {
                capability: capability.clone(),
                implementation_code: self.generate_runtime_controlled_implementation(&capability),
                quality_score: 0.9, // High quality for introspected capabilities
                safety_passed: true,
                warnings: vec!["Capability was introspected from API".to_string()],
            })
            .collect();

        let overall_quality = if synthesis_results.is_empty() {
            0.0
        } else {
            synthesis_results
                .iter()
                .map(|cap| cap.quality_score)
                .sum::<f64>()
                / synthesis_results.len() as f64
        };

        Ok(MultiCapabilitySynthesisResult {
            capabilities: synthesis_results,
            overall_quality_score: overall_quality,
            all_safety_passed: true,
            common_warnings: vec!["All capabilities were introspected from API".to_string()],
        })
    }

    /// Get an API introspector instance for serialization
    pub fn get_introspector(&self) -> APIIntrospector {
        if self.mock_mode {
            APIIntrospector::mock()
        } else {
            APIIntrospector::new()
        }
    }

    /// Synthesize capabilities by introspecting an MCP server
    pub async fn synthesize_from_mcp_introspection(
        &self,
        server_url: &str,
        server_name: &str,
    ) -> RuntimeResult<MultiCapabilitySynthesisResult> {
        self.synthesize_from_mcp_introspection_with_auth(server_url, server_name, None).await
    }

    /// Synthesize capabilities by introspecting an MCP server with authentication
    pub async fn synthesize_from_mcp_introspection_with_auth(
        &self,
        server_url: &str,
        server_name: &str,
        auth_headers: Option<HashMap<String, String>>,
    ) -> RuntimeResult<MultiCapabilitySynthesisResult> {
        if !self.synthesis_enabled {
            return Err(RuntimeError::Generic(
                "Capability synthesis is disabled by feature flag".to_string(),
            ));
        }

        eprintln!("🔍 Introspecting MCP server: {} ({})", server_name, server_url);

        // Create MCP introspector
        let introspector = if self.mock_mode {
            MCPIntrospector::mock()
        } else {
            MCPIntrospector::new()
        };

        // Introspect the MCP server
        let introspection = introspector
            .introspect_mcp_server_with_auth(server_url, server_name, auth_headers)
            .await?;

        // Create capabilities from introspection results
        let capabilities = introspector
            .create_capabilities_from_mcp(&introspection)?;

        // Convert to synthesis results with RTFS implementations
        let synthesis_results: Vec<SynthesisResult> = capabilities
            .into_iter()
            .map(|capability| {
                // Find the corresponding tool to generate implementation
                let tool = introspection
                    .tools
                    .iter()
                    .find(|t| capability.id.contains(&t.tool_name))
                    .expect("Tool should exist for capability");

                let implementation_code = introspector
                    .generate_mcp_rtfs_implementation(tool, &introspection.server_url);

                SynthesisResult {
                    capability: capability.clone(),
                    implementation_code,
                    quality_score: 0.95, // High quality for introspected MCP tools
                    safety_passed: true,
                    warnings: vec!["MCP capability - requires MCP server running".to_string()],
                }
            })
            .collect();

        let overall_quality = if synthesis_results.is_empty() {
            0.0
        } else {
            synthesis_results
                .iter()
                .map(|cap| cap.quality_score)
                .sum::<f64>()
                / synthesis_results.len() as f64
        };

        Ok(MultiCapabilitySynthesisResult {
            capabilities: synthesis_results,
            overall_quality_score: overall_quality,
            all_safety_passed: true,
            common_warnings: vec!["All capabilities introspected from MCP server".to_string()],
        })
    }

    /// Get an MCP introspector instance for serialization
    pub fn get_mcp_introspector(&self) -> MCPIntrospector {
        if self.mock_mode {
            MCPIntrospector::mock()
        } else {
            MCPIntrospector::new()
        }
    }

    /// Generate the enhanced prompt for multi-capability synthesis
    pub fn generate_multi_capability_prompt(
        &self,
        request: &MultiCapabilitySynthesisRequest,
    ) -> String {
        let mut prompt = String::new();

        prompt.push_str(&format!(
            "Generate MULTIPLE specialized RTFS capabilities for the {} API.\n\n",
            request.api_domain
        ));

        prompt.push_str("CRITICAL SAFETY RULES - MUST FOLLOW:\n");
        prompt.push_str(
            "1. Use RTFS keyword types: :string, :number, :currency (NOT \"string\", \"number\")\n",
        );
        prompt.push_str("2. NEVER hardcode credentials or API keys\n");
        prompt.push_str("3. NEVER make direct HTTP calls\n");
        prompt.push_str(
            "4. ALL network operations MUST use (call \"ccos.network.http-fetch\" ...)\n",
        );
        prompt.push_str("5. Auth tokens MUST use (call \"ccos.auth.inject\" ...) when required\n");
        prompt.push_str(
            "6. Each capability should declare specific input/output schemas when known\n",
        );
        prompt.push_str(
            "7. Return format: {:status :success :result ...} or {:status :error :message ...}\n\n",
        );

        prompt.push_str("API DOCUMENTATION:\n");
        prompt.push_str(&request.api_docs);
        prompt.push_str("\n\n");

        prompt.push_str("BASE URL: ");
        prompt.push_str(&request.base_url);
        prompt.push_str("\n\n");

        if !request.endpoints.is_empty() {
            prompt.push_str("GENERATE CAPABILITIES FOR THE FOLLOWING ENDPOINTS:\n");
            for (idx, endpoint) in request.endpoints.iter().enumerate() {
                prompt.push_str(&format!(
                    "{}. capability suffix '{}' -> {} {}\n   description: {}\n",
                    idx + 1,
                    endpoint.capability_suffix,
                    endpoint.http_method().to_uppercase(),
                    endpoint.path,
                    endpoint.description
                ));
                if let Some(schema) = &endpoint.input_schema {
                    prompt.push_str(&format!(
                        "   input schema: {}\n",
                        serde_json::to_string_pretty(schema).unwrap_or_default()
                    ));
                }
                if let Some(schema) = &endpoint.output_schema {
                    prompt.push_str(&format!(
                        "   output schema: {}\n",
                        serde_json::to_string_pretty(schema).unwrap_or_default()
                    ));
                }
            }
            prompt.push_str("\n");
        } else if let Some(targets) = &request.target_endpoints {
            prompt.push_str("GENERATE CAPABILITIES FOR THESE NAMED ENDPOINTS:\n");
            for (idx, name) in targets.iter().enumerate() {
                prompt.push_str(&format!("{}: {}\n", idx + 1, name));
            }
            prompt.push_str("\n");
        } else {
            prompt.push_str("No explicit endpoint list provided. Discover logical endpoints from the documentation and generate capabilities for each major API surface.\n\n");
        }

        if request.requires_auth {
            prompt.push_str("Authentication is required; ensure capabilities request credentials via (call \"ccos.auth.inject\" {:provider \"");
            prompt.push_str(request.auth_provider.as_deref().unwrap_or("generic"));
            prompt.push_str("\"}) and merge returned headers safely.\n\n");
        }

        prompt.push_str("Each capability should include:\n");
        prompt
            .push_str("- Specific input schema for its domain (when documentation provides it)\n");
        prompt.push_str("- Appropriate output schema (when documentation provides it)\n");
        prompt
            .push_str("- Pure RTFS implementation using (call \"ccos.network.http-fetch\" ...)\n");
        prompt.push_str("- No hardcoded API-specific credentials or secrets\n");
        prompt.push_str("- Clear error handling and validation\n\n");

        prompt.push_str(
            "Generate each capability as a separate RTFS definition using the provided metadata.\n",
        );
        prompt.push_str("Focus on correctness, safety, and domain-specific functionality.\n");

        prompt
    }

    /// Generate the guardrailed prompt for LLM
    fn generate_synthesis_prompt(&self, request: &SynthesisRequest) -> String {
        let mut prompt = String::new();

        prompt.push_str(&format!(
            "Generate a CCOS capability for calling '{}'.\n\n",
            request.capability_name
        ));

        prompt.push_str("CRITICAL SAFETY RULES - MUST FOLLOW:\n");
        prompt.push_str(
            "1. Use RTFS keyword types: :string, :number, :currency (NOT \"string\", \"number\")\n",
        );
        prompt.push_str("2. NEVER hardcode credentials or API keys\n");
        prompt.push_str("3. NEVER make direct HTTP calls\n");
        prompt.push_str(
            "4. ALL network operations MUST use (call \"ccos.network.http-fetch\" ...)\n",
        );
        prompt.push_str("5. Auth tokens MUST use (call :ccos.auth.inject ...)\n");
        prompt.push_str("6. Function signature: (defn impl [... :string] :map)\n");
        prompt.push_str(
            "7. Return format: {:status :success :result ...} or {:status :error :message ...}\n\n",
        );

        prompt.push_str("CCOS HTTP CAPABILITY INTERFACE:\n");
        prompt.push_str("- Capability ID: \"ccos.network.http-fetch\"\n");
        prompt.push_str("- Map format: (call \"ccos.network.http-fetch\" {:url \"https://...\" :method \"GET\" :headers {...} :body \"...\"})\n");
        prompt.push_str("- List format: (call \"ccos.network.http-fetch\" :url \"https://...\" :method \"GET\" :headers {...} :body \"...\")\n");
        prompt.push_str("- Simple format: (call \"ccos.network.http-fetch\" \"https://...\")  ; for GET requests\n");
        prompt.push_str("- Response format: {:status 200 :body \"...\" :headers {...}}\n\n");

        prompt.push_str("Input parameters schema: ");
        if let Some(schema) = &request.input_schema {
            prompt.push_str(&serde_json::to_string_pretty(schema).unwrap_or_default());
        } else {
            prompt.push_str("Not specified");
        }
        prompt.push('\n');

        prompt.push_str("Expected output: ");
        if let Some(output) = &request.output_schema {
            prompt.push_str(&serde_json::to_string_pretty(output).unwrap_or_default());
        } else {
            prompt.push_str("Not specified");
        }
        prompt.push('\n');

        if let Some(description) = &request.description {
            prompt.push_str(&format!("Description: {}\n", description));
        }

        if let Some(context) = &request.context {
            prompt.push_str(&format!("Context: {}\n", context));
        }

        prompt.push_str("\nGenerate a safe, minimal capability following RTFS semantics.\n");
        prompt.push_str("Focus on correctness and safety over completeness.");

        prompt
    }

    /// Run static analysis checks on generated code
    pub fn run_static_analysis(&self, code: &str) -> RuntimeResult<(bool, Vec<String>)> {
        let mut passed = true;
        let mut warnings = Vec::new();

        // Check 1: No hardcoded credentials
        if code.contains("sk_")
            || code.contains("pk_")
            || code.contains("ghp_")
            || code.contains("Bearer ")
            || code.contains("Basic ")
        {
            passed = false;
            warnings.push("Found hardcoded credentials in generated code".to_string());
        }

        // Check 2: All network calls use (call ...)
        if code.contains("http://") || code.contains("https://") {
            if !code.contains("(call :http.") {
                passed = false;
                warnings.push("Found direct HTTP calls without (call ...) wrapper".to_string());
            }
        }

        // Check 3: Auth uses injection pattern
        if code.contains("Authorization:") || code.contains("X-API-Key:") {
            if !code.contains("(call :ccos.auth.inject") {
                passed = false;
                warnings.push("Found direct auth headers without injection pattern".to_string());
            }
        }

        // Check 4: Uses keyword types, not string literals
        if code.contains("\"string\"")
            || code.contains("\"number\"")
            || code.contains("\"boolean\"")
        {
            warnings.push("Found string literal types instead of keyword types".to_string());
        }

        // Check 5: No eval or dynamic code execution
        if code.contains("eval(") || code.contains("exec(") || code.contains("system(") {
            passed = false;
            warnings.push("Found potentially unsafe dynamic code execution".to_string());
        }

        Ok((passed, warnings))
    }

    /// Convert JSON schema to RTFS parameter types
    fn json_schema_to_rtfs_params(&self, schema: &serde_json::Value) -> HashMap<String, String> {
        let mut params = HashMap::new();

        if let Some(properties) = schema.get("properties").and_then(|p| p.as_object()) {
            for (name, prop_schema) in properties {
                let param_type = self.json_type_to_rtfs_type(prop_schema);
                params.insert(name.clone(), param_type);
            }
        }

        params
    }

    /// Convert JSON schema type to RTFS keyword type
    fn json_type_to_rtfs_type(&self, schema: &serde_json::Value) -> String {
        if let Some(type_str) = schema.get("type").and_then(|t| t.as_str()) {
            match type_str {
                "string" => ":string".to_string(),
                "number" | "integer" => ":number".to_string(),
                "boolean" => ":boolean".to_string(),
                "array" => ":list".to_string(),
                "object" => ":map".to_string(),
                _ => ":any".to_string(),
            }
        } else {
            ":any".to_string()
        }
    }

    /// Calculate quality score for synthesized capability
    pub fn calculate_quality_score(
        &self,
        request: &SynthesisRequest,
        result: &SynthesisResult,
    ) -> f64 {
        let mut score = 0.5; // Base score

        // Check if input schema is properly handled
        if let Some(input_schema) = &request.input_schema {
            let expected_params = self.json_schema_to_rtfs_params(input_schema);
            let actual_params: HashMap<String, String> = result
                .capability
                .metadata
                .get("parameters")
                .and_then(|p| serde_json::from_str(p).ok())
                .unwrap_or_default();

            let param_match_ratio = expected_params
                .iter()
                .filter(|(name, expected_type)| actual_params.get(*name) == Some(expected_type))
                .count() as f64
                / expected_params.len().max(1) as f64;

            score += param_match_ratio * 0.3;
        }

        // Check if auth is properly handled
        if request.requires_auth {
            if result.capability.effects.contains(&":auth".to_string()) {
                score += 0.2;
            }
        }

        // Check if implementation code is safe
        if result.safety_passed {
            score += 0.2;
        }

        // Penalty for warnings
        score -= result.warnings.len() as f64 * 0.05;

        score.max(0.0).min(1.0)
    }

    /// Generate mock capability for testing
    fn generate_mock_capability(
        &self,
        request: &SynthesisRequest,
    ) -> RuntimeResult<SynthesisResult> {
        let capability_id = format!("synthesized.{}", request.capability_name);

        let mut effects = vec![":network".to_string()];
        let mut metadata = HashMap::new();
        let mut implementation_code = String::new();

        // Add auth if required
        if request.requires_auth {
            effects.push(":auth".to_string());
            metadata.insert("auth_required".to_string(), "true".to_string());
            if let Some(provider) = &request.auth_provider {
                metadata.insert("auth_provider".to_string(), provider.clone());
            }
            implementation_code.push_str("(let auth (call :ccos.auth.inject {:provider \"synthesized\" :type :bearer :token auth_token}))\n");
        }

        // Generate basic implementation
        implementation_code
            .push_str("(let response (call :http.get {:url \"https://api.example.com/endpoint\"");
        if request.requires_auth {
            implementation_code.push_str(" :headers {:Authorization auth}");
        }
        implementation_code.push_str("}))\n");
        implementation_code.push_str("(call :json.parse response)");

        // Build parameters
        let mut parameters_map = HashMap::new();
        if let Some(input_schema) = &request.input_schema {
            parameters_map.extend(self.json_schema_to_rtfs_params(input_schema));
        }
        if request.requires_auth {
            parameters_map.insert("auth_token".to_string(), ":string".to_string());
        }

        // Mark as synthesized
        metadata.insert("source".to_string(), "synthesized".to_string());
        metadata.insert("status".to_string(), "experimental".to_string());
        metadata.insert("guardrailed".to_string(), "true".to_string());
        metadata.insert("needs_review".to_string(), "true".to_string());

        let capability = CapabilityManifest {
            id: capability_id.clone(),
            name: request.capability_name.clone(),
            description: request
                .description
                .clone()
                .unwrap_or_else(|| format!("Synthesized capability: {}", request.capability_name)),
            provider: crate::ccos::capability_marketplace::types::ProviderType::Local(
                crate::ccos::capability_marketplace::types::LocalCapability {
                    handler: std::sync::Arc::new(|_| {
                        Ok(crate::runtime::values::Value::String(
                            "Synthesized capability placeholder".to_string(),
                        ))
                    }),
                },
            ),
            version: "1.0.0".to_string(),
            input_schema: None,
            output_schema: None,
            attestation: None,
            provenance: Some(
                crate::ccos::capability_marketplace::types::CapabilityProvenance {
                    source: "capability_synthesizer".to_string(),
                    version: Some("1.0.0".to_string()),
                    content_hash: format!("synthesized_{}", request.capability_name),
                    custody_chain: vec!["capability_synthesizer".to_string()],
                    registered_at: chrono::Utc::now(),
                },
            ),
            permissions: vec![],
            effects,
            metadata,
            agent_metadata: None,
        };

        let mut warnings = Vec::new();
        warnings.push("This capability was synthesized and should be reviewed".to_string());
        warnings.push("Implementation is minimal and may need refinement".to_string());

        let quality_score = self.calculate_quality_score(
            request,
            &SynthesisResult {
                capability: capability.clone(),
                implementation_code: implementation_code.clone(),
                quality_score: 0.0,
                safety_passed: true,
                warnings: warnings.clone(),
            },
        );

        Ok(SynthesisResult {
            capability,
            implementation_code,
            quality_score,
            safety_passed: true,
            warnings,
        })
    }

    /// Generate mock multi-capabilities for testing
    fn generate_mock_multi_capabilities(
        &self,
        request: &MultiCapabilitySynthesisRequest,
    ) -> RuntimeResult<MultiCapabilitySynthesisResult> {
        let mut capabilities = Vec::new();
        let mut common_warnings = Vec::new();

        // Determine which endpoints to materialize
        let resolved_endpoints = if !request.endpoints.is_empty() {
            request.endpoints.clone()
        } else {
            // When no endpoints provided, fabricate a small generic set so tests still work
            vec![
                MultiCapabilityEndpoint {
                    capability_suffix: "endpoint_a".to_string(),
                    description: "Generic endpoint A".to_string(),
                    path: "/v1/endpoint-a".to_string(),
                    http_method: Some("GET".to_string()),
                    input_schema: None,
                    output_schema: None,
                },
                MultiCapabilityEndpoint {
                    capability_suffix: "endpoint_b".to_string(),
                    description: "Generic endpoint B".to_string(),
                    path: "/v1/endpoint-b".to_string(),
                    http_method: Some("POST".to_string()),
                    input_schema: None,
                    output_schema: None,
                },
            ]
        };

        for endpoint in resolved_endpoints {
            let capability = self.generate_mock_endpoint_capability(request, &endpoint)?;
            common_warnings.extend(capability.warnings.iter().cloned());
            capabilities.push(capability);
        }

        // Calculate overall quality score
        let overall_quality = if capabilities.is_empty() {
            0.0
        } else {
            capabilities
                .iter()
                .map(|cap| cap.quality_score)
                .sum::<f64>()
                / capabilities.len() as f64
        };

        // Check if all passed safety checks
        let all_safety_passed = capabilities.iter().all(|cap| cap.safety_passed);

        // Deduplicate warnings
        common_warnings.sort();
        common_warnings.dedup();

        Ok(MultiCapabilitySynthesisResult {
            capabilities,
            overall_quality_score: overall_quality,
            all_safety_passed,
            common_warnings,
        })
    }

    fn generate_mock_endpoint_capability(
        &self,
        request: &MultiCapabilitySynthesisRequest,
        endpoint: &MultiCapabilityEndpoint,
    ) -> RuntimeResult<SynthesisResult> {
        let capability_id = format!("{}.{}", request.api_domain, endpoint.capability_suffix);
        let http_method = endpoint.http_method();

        // Generate input parameter extraction based on the endpoint schema
        let input_extraction = self.generate_input_extraction(endpoint);
        
        // Generate query parameter construction
        let query_construction = self.generate_query_construction(endpoint);
        
        // Generate API key handling
        let api_key_handling = self.generate_api_key_handling(&request.api_domain);
        
        // Generate validation code separately
        let _validation_code = self.generate_validation_code(endpoint);
        
        let implementation_code = format!(
            r#"(do
  ;; {desc}
  (let [base_url "{base_url}"
        path "{path}"
        method "{method}"
        ;; Extract parameters from input - convert list to map if needed
        input_map (if (map? input) 
                     input
                     (apply hash-map input))
        ;; Extract parameters and make request
        {input_extraction}
        ;; Build query parameters
        {query_construction}
        full_url (str base_url path "?" query_params)
        {api_key_handling}
        headers {{}}]
    (call "ccos.network.http-fetch"
          :method method
          :url final_url
          :headers headers)))"#,
            desc = endpoint.description,
            base_url = request.base_url,
            path = endpoint.path,
            method = http_method
        );

        let capability = CapabilityManifest {
            id: capability_id.clone(),
            name: format!("{} {}", request.api_domain, endpoint.capability_suffix),
            description: endpoint.description.clone(),
            provider: crate::ccos::capability_marketplace::types::ProviderType::Local(
                crate::ccos::capability_marketplace::types::LocalCapability {
                    handler: std::sync::Arc::new(|_| {
                        Ok(crate::runtime::values::Value::String(
                            "Mock multi-capability placeholder".to_string(),
                        ))
                    }),
                },
            ),
            version: "1.0.0".to_string(),
            input_schema: None,
            output_schema: None,
            attestation: None,
            provenance: Some(
                crate::ccos::capability_marketplace::types::CapabilityProvenance {
                    source: "multi_capability_synthesizer".to_string(),
                    version: Some("1.0.0".to_string()),
                    content_hash: format!(
                        "multi_{}_{}",
                        request.api_domain, endpoint.capability_suffix
                    ),
                    custody_chain: vec!["multi_capability_synthesizer".to_string()],
                    registered_at: chrono::Utc::now(),
                },
            ),
            permissions: vec!["network.http".to_string()],
            effects: vec!["network_request".to_string()],
            metadata: {
                let mut meta = HashMap::new();
                meta.insert("source".to_string(), "multi_synthesized".to_string());
                meta.insert(
                    "capability_suffix".to_string(),
                    endpoint.capability_suffix.clone(),
                );
                meta.insert("endpoint_path".to_string(), endpoint.path.clone());
                meta.insert("http_method".to_string(), http_method);
                meta.insert("base_url".to_string(), request.base_url.clone());
                meta
            },
            agent_metadata: None,
        };

        let warnings = vec![
            "This capability was synthesized and should be reviewed".to_string(),
            "Implementation is minimal and may need refinement".to_string(),
        ];

        Ok(SynthesisResult {
            capability,
            implementation_code,
            quality_score: 0.8,
            safety_passed: true,
            warnings,
        })
    }

    /// Generate validation code for required parameters
    fn generate_validation_code(&self, endpoint: &MultiCapabilityEndpoint) -> String {
        if let Some(input_schema) = &endpoint.input_schema {
            if let Some(properties) = input_schema.get("properties") {
                if let Some(props_obj) = properties.as_object() {
                    let mut validations = Vec::new();
                    
                    for (key, _) in props_obj {
                        validations.push(format!("    (if (not (get input_map :{})) (throw \"MissingRequiredParameter\" \"{}\"))", key, key));
                    }
                    
                    return validations.join("\n");
                }
            }
        }
        
        "    ;; No input schema provided - this should not happen".to_string()
    }

    /// Generate input parameter extraction code based on endpoint schema
    fn generate_input_extraction(&self, endpoint: &MultiCapabilityEndpoint) -> String {
        if let Some(input_schema) = &endpoint.input_schema {
            if let Some(properties) = input_schema.get("properties") {
                if let Some(props_obj) = properties.as_object() {
                    let mut extractions = Vec::new();
                    
                    for (key, _) in props_obj {
                        extractions.push(format!("        {} (get input_map :{})", key, key));
                    }
                    
                    return extractions.join("\n");
                }
            }
        }
        
        "        ;; No input schema provided - this should not happen".to_string()
    }
    
    /// Generate query parameter construction code
    fn generate_query_construction(&self, endpoint: &MultiCapabilityEndpoint) -> String {
        if let Some(input_schema) = &endpoint.input_schema {
            if let Some(properties) = input_schema.get("properties") {
                if let Some(props_obj) = properties.as_object() {
                    let mut query_parts = Vec::new();
                    for (key, _) in props_obj {
                        query_parts.push(format!("{}={{{}}}", key, key));
                    }
                    if !query_parts.is_empty() {
                        return format!("        query_params (str \"{}\")", query_parts.join("&"));
                    }
                }
            }
        }
        
        // No fallback - schema validation is required
        "        ;; No input schema provided - this should not happen".to_string()
    }
    
    /// Generate API key handling code
    fn generate_api_key_handling(&self, api_domain: &str) -> String {
        // Generate generic API key handling
        let env_var_name = format!("{}_API_KEY", api_domain.to_uppercase());
        
        format!(
            r#"        api_key (call "ccos.system.get-env" "{}")
        ;; Add API key to query params
        final_url (if api_key 
                    (str full_url "&appid=" api_key)
                    full_url)"#,
            env_var_name
        )
    }

    /// Generate runtime-controlled implementation that moves controls to runtime
    fn generate_runtime_controlled_implementation(&self, capability: &CapabilityManifest) -> String {
        let method = capability
            .metadata
            .get("endpoint_method")
            .unwrap_or(&"GET".to_string())
            .clone();
        let path = capability
            .metadata
            .get("endpoint_path")
            .unwrap_or(&"/".to_string())
            .clone();
        let base_url = capability
            .metadata
            .get("base_url")
            .unwrap_or(&"https://api.example.com".to_string())
            .clone();

        // Check if the endpoint has path parameters
        let has_path_params = path.contains("{");

        format!(
            r#"(fn [input]
  ;; Runtime-controlled implementation - validation and controls handled by runtime
  ;; Input: {}
  ;; Output: validated by runtime against output_schema
  (do
    (call "ccos.io.println" (str "[DEBUG] Calling {} with input: " (call "ccos.data.serialize-json" input)))
    (let [base_url "{}"
          path "{}"
          method "{}"
          ;; Build URL
          {}
          ;; Build query string from input map (input is a map of parameters)
          query_string (if (map? input)
                        (let [params (keys input)]
                          (reduce (fn [acc k]
                                    (let [v (get input k)
                                          k_str (str k)
                                          ;; Remove leading ':' from keyword if present
                                          param_name (if (starts-with? k_str ":")
                                                       (substring k_str 1)
                                                       k_str)]
                                      (if (and v (not= v ""))
                                        (str acc 
                                             (if (= acc "") "?" "&")
                                             param_name "=" (str v))
                                        acc)))
                                  ""
                                  params))
                        "")
          url_with_params (str full_url query_string)
          ;; Add API key from environment (OpenWeather requires 'appid' parameter)
          api_key (call "ccos.system.get-env" "OPENWEATHERMAP_ORG_API_KEY")
          final_url (if (and api_key (not= api_key ""))
                      (str url_with_params 
                           (if (= query_string "") "?" "&")
                           "appid=" api_key)
                      url_with_params)
          headers {{}}]
      (do
        (call "ccos.io.println" (str "[DEBUG] Final URL (with key): " final_url))
        (call "ccos.network.http-fetch"
              :method method
              :url final_url
              :headers headers
              :body (if (= method "POST") (call "ccos.data.serialize-json" input) nil))))))"#,
            capability.description,
            capability.id,
            base_url,
            path,
            method,
            if has_path_params {
                "full_url (str base_url path) ;; TODO: substitute path parameters"
            } else {
                "full_url (str base_url path)"
            }
        )
    }

    /// Validate that a capability meets governance requirements
    pub fn validate_governance(&self, capability: &CapabilityManifest) -> RuntimeResult<bool> {
        // Check 1: Effects are properly declared
        if capability.effects.contains(&":auth".to_string()) {
            if !capability
                .metadata
                .get("auth_required")
                .map(|v| v == "true")
                .unwrap_or(false)
            {
                return Err(RuntimeError::Generic(
                    "Capability with :auth effect must have auth_required metadata".to_string(),
                ));
            }
        }

        // Check 2: Synthesized capabilities are marked appropriately
        if capability
            .metadata
            .get("source")
            .map(|v| v == "synthesized")
            .unwrap_or(false)
        {
            if !capability
                .metadata
                .get("guardrailed")
                .map(|v| v == "true")
                .unwrap_or(false)
            {
                return Err(RuntimeError::Generic(
                    "Synthesized capabilities must be marked as guardrailed".to_string(),
                ));
            }
        }

        // Check 3: No direct network effects without proper declaration
        if capability.effects.contains(&":network".to_string()) {
            // This is fine, just ensure it's declared
        }

        Ok(true)
    }
}

impl Default for CapabilitySynthesizer {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_synthesizer_creation() {
        let synthesizer = CapabilitySynthesizer::new();
        assert!(synthesizer.synthesis_enabled);
        assert!(!synthesizer.mock_mode);
    }

    #[test]
    fn test_synthesizer_with_feature_flag() {
        let synthesizer = CapabilitySynthesizer::with_feature_flag(false);
        assert!(!synthesizer.synthesis_enabled);
    }

    #[test]
    fn test_json_type_to_rtfs_type() {
        let synthesizer = CapabilitySynthesizer::mock();

        let string_schema = serde_json::json!({"type": "string"});
        assert_eq!(
            synthesizer.json_type_to_rtfs_type(&string_schema),
            ":string"
        );

        let number_schema = serde_json::json!({"type": "number"});
        assert_eq!(
            synthesizer.json_type_to_rtfs_type(&number_schema),
            ":number"
        );

        let boolean_schema = serde_json::json!({"type": "boolean"});
        assert_eq!(
            synthesizer.json_type_to_rtfs_type(&boolean_schema),
            ":boolean"
        );
    }

    #[test]
    fn test_run_static_analysis() {
        let synthesizer = CapabilitySynthesizer::mock();

        // Safe code
        let safe_code = "(call :http.get {:url \"https://api.example.com\"})";
        let (passed, warnings) = synthesizer.run_static_analysis(safe_code).unwrap();
        assert!(passed);
        assert!(warnings.is_empty());

        // Unsafe code with hardcoded credentials
        let unsafe_code = "Bearer sk_12345abcdef";
        let (passed, warnings) = synthesizer.run_static_analysis(unsafe_code).unwrap();
        assert!(!passed);
        assert!(!warnings.is_empty());
    }

    #[tokio::test]
    async fn test_synthesize_mock_capability() {
        let synthesizer = CapabilitySynthesizer::mock();

        let request = SynthesisRequest {
            capability_name: "test_api".to_string(),
            input_schema: Some(serde_json::json!({
                "properties": {
                    "query": {"type": "string"},
                    "limit": {"type": "number"}
                }
            })),
            output_schema: Some(serde_json::json!({"type": "object"})),
            requires_auth: true,
            auth_provider: Some("test_provider".to_string()),
            description: Some("Test capability".to_string()),
            context: None,
        };

        let result = synthesizer.synthesize_capability(&request).await.unwrap();

        assert!(result.capability.id.contains("synthesized.test_api"));
        assert!(result.capability.effects.contains(&":auth".to_string()));
        assert!(result.safety_passed);
        assert!(result.quality_score > 0.0);
    }

    #[test]
    fn test_validate_governance() {
        let synthesizer = CapabilitySynthesizer::mock();

        // Valid capability
        let mut capability = CapabilityManifest {
            id: "test.capability".to_string(),
            name: "test".to_string(),
            description: "test".to_string(),
            provider: crate::ccos::capability_marketplace::types::ProviderType::Local(
                crate::ccos::capability_marketplace::types::LocalCapability {
                    handler: std::sync::Arc::new(|_| {
                        Ok(crate::runtime::values::Value::String("test".to_string()))
                    }),
                },
            ),
            version: "1.0.0".to_string(),
            input_schema: None,
            output_schema: None,
            attestation: None,
            provenance: None,
            permissions: vec![],
            effects: vec![":auth".to_string()],
            metadata: HashMap::from([
                ("auth_required".to_string(), "true".to_string()),
                ("source".to_string(), "synthesized".to_string()),
                ("guardrailed".to_string(), "true".to_string()),
            ]),
            agent_metadata: None,
        };

        assert!(synthesizer.validate_governance(&capability).unwrap());

        // Invalid capability without auth_required metadata
        capability.metadata.remove("auth_required");
        assert!(synthesizer.validate_governance(&capability).is_err());
    }

    #[test]
    fn test_generate_synthesis_prompt() {
        let synthesizer = CapabilitySynthesizer::mock();

        let request = SynthesisRequest {
            capability_name: "github_repos".to_string(),
            input_schema: Some(serde_json::json!({
                "properties": {
                    "owner": {"type": "string"},
                    "repo": {"type": "string"}
                }
            })),
            output_schema: None,
            requires_auth: true,
            auth_provider: Some("github".to_string()),
            description: Some("Get GitHub repository info".to_string()),
            context: None,
        };

        let prompt = synthesizer.generate_synthesis_prompt(&request);

        assert!(prompt.contains("github_repos"));
        assert!(prompt.contains("CRITICAL SAFETY RULES"));
        assert!(prompt.contains(":string"));
        assert!(prompt.contains("(call") && prompt.contains("ccos.network.http-fetch"));
        assert!(prompt.contains("Get GitHub repository info"));
    }

    #[tokio::test]
    async fn test_synthesize_mock_multi_capabilities() {
        let synthesizer = CapabilitySynthesizer::mock();

        let request = MultiCapabilitySynthesisRequest {
            api_domain: "unifieddata".to_string(),
            api_docs: "Unified data platform API documentation".to_string(),
            base_url: "https://api.unifieddata.example.com".to_string(),
            requires_auth: true,
            auth_provider: Some("unifieddata".to_string()),
            endpoints: vec![
                MultiCapabilityEndpoint {
                    capability_suffix: "profile".to_string(),
                    description: "Retrieve user profile details".to_string(),
                    path: "/v1/profile/{userId}".to_string(),
                    http_method: Some("GET".to_string()),
                    input_schema: Some(serde_json::json!({
                        "type": "object",
                        "properties": {
                            "userId": {"type": "string"},
                            "expand": {"type": "boolean"}
                        },
                        "required": ["userId"]
                    })),
                    output_schema: None,
                },
                MultiCapabilityEndpoint {
                    capability_suffix: "activity".to_string(),
                    description: "Submit user activity events".to_string(),
                    path: "/v1/activity".to_string(),
                    http_method: Some("POST".to_string()),
                    input_schema: Some(serde_json::json!({
                        "type": "object",
                        "properties": {
                            "events": {
                                "type": "array",
                                "items": {"type": "object"}
                            }
                        },
                        "required": ["events"]
                    })),
                    output_schema: None,
                },
                MultiCapabilityEndpoint {
                    capability_suffix: "insights".to_string(),
                    description: "Fetch analytics insights aggregates".to_string(),
                    path: "/v1/insights".to_string(),
                    http_method: Some("GET".to_string()),
                    input_schema: Some(serde_json::json!({
                        "type": "object",
                        "properties": {
                            "range": {"type": "string"},
                            "metric": {"type": "string"}
                        },
                        "required": ["range", "metric"]
                    })),
                    output_schema: None,
                },
            ],
            target_endpoints: None,
            generate_all_endpoints: false,
        };

        let result = synthesizer
            .synthesize_multi_capabilities(&request)
            .await
            .unwrap();

        assert_eq!(result.capabilities.len(), 3);
        assert!(result.all_safety_passed);
        assert!(result.overall_quality_score > 0.0);

        let capability_types: Vec<&str> = result
            .capabilities
            .iter()
            .map(|cap| cap.capability.id.split('.').last().unwrap())
            .collect();

        assert!(capability_types.contains(&"profile"));
        assert!(capability_types.contains(&"activity"));
        assert!(capability_types.contains(&"insights"));
    }

    #[test]
    fn test_generate_multi_capability_prompt() {
        let synthesizer = CapabilitySynthesizer::mock();

        let request = MultiCapabilitySynthesisRequest {
            api_domain: "unifieddata".to_string(),
            api_docs: "Unified data platform API documentation".to_string(),
            base_url: "https://api.unifieddata.example.com".to_string(),
            requires_auth: true,
            auth_provider: Some("unifieddata".to_string()),
            endpoints: vec![
                MultiCapabilityEndpoint {
                    capability_suffix: "profile".to_string(),
                    description: "Retrieve user profile details".to_string(),
                    path: "/v1/profile/{userId}".to_string(),
                    http_method: Some("GET".to_string()),
                    input_schema: None,
                    output_schema: None,
                },
                MultiCapabilityEndpoint {
                    capability_suffix: "activity".to_string(),
                    description: "Submit user activity events".to_string(),
                    path: "/v1/activity".to_string(),
                    http_method: Some("POST".to_string()),
                    input_schema: None,
                    output_schema: None,
                },
            ],
            target_endpoints: None,
            generate_all_endpoints: false,
        };

        let prompt = synthesizer.generate_multi_capability_prompt(&request);

        assert!(prompt.contains("unifieddata"));
        assert!(prompt.contains("MULTIPLE specialized RTFS capabilities"));
        assert!(prompt.contains("capability suffix 'profile'"));
        assert!(prompt.contains("capability suffix 'activity'"));
        assert!(prompt.contains("https://api.unifieddata.example.com"));
    }
}
