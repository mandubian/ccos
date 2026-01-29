use rtfs::ast::{Expression, Keyword, Literal};
use rtfs::runtime::error::{RuntimeError, RuntimeResult};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::env;

/// Supported authentication types
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "lowercase")]
pub enum AuthType {
    Bearer,
    ApiKey,
    Basic,
    OAuth2,
    Custom,
}

impl std::fmt::Display for AuthType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Bearer => write!(f, "bearer"),
            Self::ApiKey => write!(f, "api_key"),
            Self::Basic => write!(f, "basic"),
            Self::OAuth2 => write!(f, "oauth2"),
            Self::Custom => write!(f, "custom"),
        }
    }
}

/// Auth configuration for a capability
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuthConfig {
    /// Authentication type (bearer, api_key, basic, oauth2)
    pub auth_type: AuthType,
    /// Provider name (github, stripe, openai, etc.)
    pub provider: String,
    /// For API Key auth: header name or query parameter name
    pub key_location: Option<String>,
    /// For API Key auth: whether it's in header (true) or query (false)
    pub in_header: Option<bool>,
    /// For Bearer/Custom: header name (default: "Authorization")
    pub header_name: Option<String>,
    /// For Bearer/Custom: header prefix (default: "Bearer " for bearer)
    pub header_prefix: Option<String>,
    /// For Basic auth: username field name in parameters
    pub username_param: Option<String>,
    /// For Basic auth: password field name in parameters
    pub password_param: Option<String>,
    /// Environment variable name to retrieve auth token
    pub env_var: Option<String>,
    /// Whether this auth is required for the capability
    pub required: bool,
    /// Whether the auth token is sensitive (should not be logged)
    pub is_secret: bool,
}

impl Default for AuthConfig {
    fn default() -> Self {
        Self {
            auth_type: AuthType::Bearer,
            provider: "default".to_string(),
            key_location: None,
            in_header: None,
            header_name: Some("Authorization".to_string()),
            header_prefix: Some("Bearer ".to_string()),
            username_param: None,
            password_param: None,
            env_var: None,
            required: true,
            is_secret: true,
        }
    }
}

/// Auth injection result
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuthInjectionResult {
    /// The formatted auth header value
    pub auth_value: String,
    /// Audit entry for this injection
    pub audit_id: String,
    /// Timestamp of injection
    pub injected_at: String,
}

/// Central auth injector for managing credentials across capabilities
pub struct AuthInjector {
    /// Cached auth configs by provider
    configs: HashMap<String, AuthConfig>,
    /// Mock mode for testing
    mock_mode: bool,
}

impl AuthInjector {
    /// Create a new auth injector
    pub fn new() -> Self {
        Self {
            configs: HashMap::new(),
            mock_mode: false,
        }
    }

    /// Create in mock mode (for testing)
    pub fn mock() -> Self {
        Self {
            configs: HashMap::new(),
            mock_mode: true,
        }
    }

    /// Register an auth configuration
    pub fn register_auth_config(&mut self, provider: String, config: AuthConfig) {
        self.configs.insert(provider, config);
    }

    /// Generate auth injection RTFS code for a capability
    pub fn generate_auth_injection_code(
        &self,
        provider: &str,
        auth_type: AuthType,
        token_param_name: &str,
    ) -> RuntimeResult<Expression> {
        // Generate RTFS: (call :ccos.auth.inject {:provider "github" :type :bearer :token token_param})
        let call_args = vec![
            (
                rtfs::ast::MapKey::Keyword(Keyword("provider".to_string())),
                Expression::Literal(Literal::String(provider.to_string())),
            ),
            (
                rtfs::ast::MapKey::Keyword(Keyword("type".to_string())),
                Expression::Literal(Literal::Keyword(Keyword(auth_type.to_string()))),
            ),
            (
                rtfs::ast::MapKey::Keyword(Keyword("token".to_string())),
                Expression::Symbol(rtfs::ast::Symbol(token_param_name.to_string())),
            ),
        ];

        Ok(Expression::FunctionCall {
            callee: Box::new(Expression::Literal(Literal::Keyword(Keyword(
                "ccos.auth.inject".to_string(),
            )))),
            arguments: vec![Expression::Map(call_args.into_iter().collect())],
        })
    }

    /// Retrieve auth token from environment
    pub fn retrieve_from_env(&self, provider: &str) -> RuntimeResult<String> {
        if self.mock_mode {
            return Ok(format!("mock_token_{}", provider));
        }

        let env_var_names = vec![
            format!("{}_TOKEN", provider.to_uppercase()),
            format!("{}_API_KEY", provider.to_uppercase()),
            format!("CCOS_AUTH_{}", provider.to_uppercase()),
        ];

        for env_var in &env_var_names {
            if let Ok(token) = env::var(env_var) {
                if !token.is_empty() {
                    return Ok(token);
                }
            }
        }

        Err(RuntimeError::Generic(format!(
            "Auth token not found for provider '{}'. Tried env vars: {}",
            provider,
            env_var_names.join(", ")
        )))
    }

    /// Format auth header based on type
    pub fn format_auth_header(
        &self,
        auth_type: AuthType,
        token: &str,
        header_prefix: Option<&str>,
    ) -> RuntimeResult<String> {
        match auth_type {
            AuthType::Bearer => {
                let prefix = header_prefix.unwrap_or("Bearer ");
                Ok(format!("{}{}", prefix, token))
            }
            AuthType::ApiKey => Ok(token.to_string()),
            AuthType::Basic => Ok(format!("Basic {}", token)),
            AuthType::OAuth2 => {
                let prefix = header_prefix.unwrap_or("Bearer ");
                Ok(format!("{}{}", prefix, token))
            }
            AuthType::Custom => Ok(token.to_string()),
        }
    }

    /// Generate HTTP header for auth
    pub fn generate_auth_header(
        &self,
        provider: &str,
        auth_type: AuthType,
    ) -> RuntimeResult<(String, String)> {
        let token = self.retrieve_from_env(provider)?;
        let auth_value = self.format_auth_header(auth_type.clone(), &token, None)?;

        let header_name = match auth_type {
            AuthType::ApiKey => "X-API-Key".to_string(),
            AuthType::Basic => "Authorization".to_string(),
            AuthType::Bearer | AuthType::OAuth2 => "Authorization".to_string(),
            AuthType::Custom => "Authorization".to_string(),
        };

        Ok((header_name, auth_value))
    }

    /// Extract auth requirements from OpenAPI security schemes
    pub fn extract_from_openapi_security(
        &self,
        security_schemes: &serde_json::Map<String, serde_json::Value>,
    ) -> RuntimeResult<Vec<AuthConfig>> {
        let mut configs = Vec::new();

        for (scheme_name, scheme_def) in security_schemes {
            let scheme_type = scheme_def
                .get("type")
                .and_then(|v| v.as_str())
                .unwrap_or("unknown");

            let auth_type = match scheme_type {
                "http" => {
                    let http_scheme = scheme_def
                        .get("scheme")
                        .and_then(|v| v.as_str())
                        .unwrap_or("bearer");
                    match http_scheme {
                        "bearer" => AuthType::Bearer,
                        "basic" => AuthType::Basic,
                        _ => AuthType::Custom,
                    }
                }
                "apiKey" => AuthType::ApiKey,
                "oauth2" => AuthType::OAuth2,
                _ => AuthType::Custom,
            };

            let mut config = AuthConfig {
                auth_type,
                provider: scheme_name.clone(),
                ..Default::default()
            };

            // Extract additional properties from OpenAPI spec
            if let Some(in_loc) = scheme_def.get("in").and_then(|v| v.as_str()) {
                config.key_location = Some(in_loc.to_string());
                config.in_header = Some(in_loc == "header");
            }

            configs.push(config);
        }

        Ok(configs)
    }

    /// Mark a capability as requiring auth
    pub fn mark_capability_with_auth(
        &self,
        mut metadata: HashMap<String, String>,
        providers: &[String],
    ) -> HashMap<String, String> {
        metadata.insert("auth_required".to_string(), "true".to_string());
        metadata.insert("auth_providers".to_string(), providers.join(","));
        metadata
    }

    /// Generate audit event for auth injection
    pub fn create_auth_audit_event(
        provider: &str,
        auth_type: AuthType,
        success: bool,
    ) -> serde_json::Value {
        serde_json::json!({
            "event_type": "auth_injection",
            "provider": provider,
            "auth_type": auth_type.to_string(),
            "success": success,
            "timestamp": chrono::Utc::now().to_rfc3339(),
        })
    }
}

impl Default for AuthInjector {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_auth_config_default() {
        let config = AuthConfig::default();
        assert_eq!(config.auth_type, AuthType::Bearer);
        assert_eq!(config.required, true);
        assert_eq!(config.is_secret, true);
    }

    #[test]
    fn test_format_bearer_auth() {
        let injector = AuthInjector::new();
        let header = injector
            .format_auth_header(AuthType::Bearer, "my_token", None)
            .unwrap();
        assert_eq!(header, "Bearer my_token");
    }

    #[test]
    fn test_format_api_key_auth() {
        let injector = AuthInjector::new();
        let header = injector
            .format_auth_header(AuthType::ApiKey, "secret_key_123", None)
            .unwrap();
        assert_eq!(header, "secret_key_123");
    }

    #[test]
    fn test_auth_injection_in_mock_mode() {
        let injector = AuthInjector::mock();
        let token = injector.retrieve_from_env("github").unwrap();
        assert_eq!(token, "mock_token_github");
    }

    #[test]
    fn test_mark_capability_with_auth() {
        let injector = AuthInjector::new();
        let metadata = HashMap::new();
        let providers = vec!["github".to_string(), "stripe".to_string()];
        let updated = injector.mark_capability_with_auth(metadata, &providers);

        assert_eq!(updated.get("auth_required"), Some(&"true".to_string()));
        assert_eq!(
            updated.get("auth_providers"),
            Some(&"github,stripe".to_string())
        );
    }
}
