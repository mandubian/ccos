use crate::ccos::capability_marketplace::types::CapabilityManifest;
use crate::ccos::synthesis::auth_injector::AuthInjector;
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
    pub fn calculate_quality_score(&self, request: &SynthesisRequest, result: &SynthesisResult) -> f64 {
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
}
