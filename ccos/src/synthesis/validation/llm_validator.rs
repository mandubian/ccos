//! LLM-based validation for schemas and plans.
//!
//! This module provides configurable validation using LLM:
//! - Schema validation: verify inferred schemas are correct
//! - Plan validation: check schema compatibility, dependencies, parameters

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

use crate::config::types::ValidationConfig;

fn rtfs_candidate_parses(code: &str) -> bool {
    let trimmed = code.trim();
    if trimmed.is_empty() {
        return false;
    }

    // Accept either multi-form programs or single expressions.
    rtfs::parser::parse(trimmed).is_ok() || rtfs::parser::parse_expression(trimmed).is_ok()
}

fn rtfs_candidate_parse_error(code: &str) -> Option<String> {
    let trimmed = code.trim();
    if trimmed.is_empty() {
        return Some("empty RTFS output".to_string());
    }

    match rtfs::parser::parse(trimmed) {
        Ok(_) => None,
        Err(parse_err) => match rtfs::parser::parse_expression(trimmed) {
            Ok(_) => None,
            Err(expr_err) => Some(format!(
                "parse: {:?}; parse_expression: {:?}",
                parse_err, expr_err
            )),
        },
    }
}

/// Result of a validation attempt.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ValidationResult {
    pub is_valid: bool,
    pub errors: Vec<ValidationError>,
    pub suggestions: Vec<String>,
}

/// A specific validation error.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ValidationError {
    pub error_type: ValidationErrorType,
    pub message: String,
    pub location: Option<String>,
    pub suggested_fix: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ValidationErrorType {
    SchemaMismatch,
    MissingParameter,
    InvalidDependency,
    CyclicDependency,
    UnavailableCapability,
    TypeIncompatibility,
    Other,
}

impl ValidationResult {
    pub fn valid() -> Self {
        Self {
            is_valid: true,
            errors: vec![],
            suggestions: vec![],
        }
    }

    pub fn invalid(errors: Vec<ValidationError>) -> Self {
        Self {
            is_valid: false,
            errors,
            suggestions: vec![],
        }
    }
}

/// Get an LLM provider based on ValidationConfig.
/// Uses the config's validation_profile if specified, otherwise falls back to default.
async fn get_validation_provider(
    config: &ValidationConfig,
) -> Option<Box<dyn crate::arbiter::llm_provider::LlmProvider + Send + Sync>> {
    // If a specific validation profile is configured, try to resolve it
    if let Some(profile_name) = &config.validation_profile {
        // Try to get agent config and resolve profile
        if let Some(provider) = resolve_profile_to_provider(profile_name).await {
            // The provider is Send + Sync because all our LLM implementations are
            return Some(provider);
        }
        log::debug!(
            "Could not resolve profile '{}', falling back to default",
            profile_name
        );
    }

    // Fall back to default provider
    crate::arbiter::get_default_llm_provider().await
}

/// Resolve a profile name to an LLM provider.
/// Profile names can be "set:model" (e.g., "openrouter_free:fast") or a direct profile name.
async fn resolve_profile_to_provider(
    profile_name: &str,
) -> Option<Box<dyn crate::arbiter::llm_provider::LlmProvider + Send + Sync>> {
    use crate::arbiter::llm_provider::{LlmProviderConfig, LlmProviderFactory, LlmProviderType};

    // Parse profile name - could be "set:model" format
    let (set_name, model_name) = if profile_name.contains(':') {
        let parts: Vec<&str> = profile_name.splitn(2, ':').collect();
        (Some(parts[0]), parts.get(1).copied())
    } else {
        (None, Some(profile_name))
    };

    // For now, extract provider info from profile name conventions
    // In a full implementation, this would load from AgentConfig
    let (provider_type, model, api_key_env, base_url) = match set_name {
        Some("openrouter_free") | Some("openrouter") => {
            let model = match model_name {
                Some("fast") => "nvidia/nemotron-nano-9b-v2:free",
                Some("balanced") => "deepseek/deepseek-v3.2-exp",
                Some("balanced_gfl") => "google/gemini-2.5-flash-lite",
                Some("premium") => "x-ai/grok-4-fast:free",
                Some(m) => m,
                None => "deepseek/deepseek-v3.2-exp",
            };
            (
                LlmProviderType::OpenAI,
                model.to_string(),
                "OPENROUTER_API_KEY",
                Some("https://openrouter.ai/api/v1".to_string()),
            )
        }
        _ => {
            // Try to infer from model name
            let model = model_name.unwrap_or("gpt-4o-mini");
            if model.starts_with("claude-") {
                (
                    LlmProviderType::Anthropic,
                    model.to_string(),
                    "ANTHROPIC_API_KEY",
                    None,
                )
            } else {
                (
                    LlmProviderType::OpenAI,
                    model.to_string(),
                    "OPENAI_API_KEY",
                    None,
                )
            }
        }
    };

    // Get API key
    let api_key = std::env::var(api_key_env).ok()?;

    let provider_config = LlmProviderConfig {
        provider_type,
        model,
        api_key: Some(api_key),
        base_url,
        max_tokens: Some(4096),
        temperature: Some(0.3), // Lower temperature for validation
        timeout_seconds: None,
        retry_config: Default::default(),
    };

    // All our LlmProvider implementations are Send + Sync
    // We use the "as" cast because the concrete types returned implement these traits
    match LlmProviderFactory::create_provider(provider_config).await {
        Ok(provider) => {
            // SAFETY: All LlmProvider implementations (OpenAI, Anthropic, Stub) are Send+Sync
            // The trait object just doesn't carry those bounds in create_provider's signature
            Some(unsafe {
                std::mem::transmute::<
                    Box<dyn crate::arbiter::llm_provider::LlmProvider>,
                    Box<dyn crate::arbiter::llm_provider::LlmProvider + Send + Sync>,
                >(provider)
            })
        }
        Err(_) => None,
    }
}

/// Validate an inferred schema using LLM.
///
/// # Arguments
/// - `schema`: The inferred RTFS schema string
/// - `capability_description`: Description of what the capability does
/// - `sample_output`: Optional sample output that was used to infer the schema
pub async fn validate_schema(
    schema: &str,
    capability_description: &str,
    sample_output: Option<&str>,
    config: &ValidationConfig,
) -> Result<ValidationResult, String> {
    // Get LLM provider based on config
    let provider = match get_validation_provider(config).await {
        Some(p) => p,
        None => {
            log::debug!("No LLM provider available, skipping schema validation");
            return Ok(ValidationResult::valid());
        }
    };

    let sample_section = sample_output
        .map(|s| {
            format!(
                "\n\nSample output that was used to infer this schema:\n```\n{}\n```",
                s
            )
        })
        .unwrap_or_default();

    let prompt = format!(
        r#"You are validating an RTFS schema that was inferred from runtime output.

Capability description: {}

Inferred schema:
```
{}
```{}

Analyze this schema and determine:
1. Is the schema correctly generalized? (e.g., should `int?` be `float?` for numeric values)
2. Are there any obvious errors or improvements?
3. Does the schema match what the capability description suggests?

Respond in JSON format:
{{
  "is_valid": true/false,
  "errors": ["error message 1", "error message 2"],
  "suggestions": ["suggestion 1", "suggestion 2"],
  "corrected_schema": "[:map ...]" // only if corrections are needed
}}

Respond with ONLY the JSON, no additional text."#,
        capability_description, schema, sample_section
    );

    match provider.generate_text(&prompt).await {
        Ok(response) => parse_validation_response(&response),
        Err(e) => {
            log::warn!("LLM schema validation failed: {}", e);
            Ok(ValidationResult::valid()) // Fail open
        }
    }
}

/// Validate a generated RTFS plan.
///
/// # Arguments
/// - `plan`: The RTFS plan code
/// - `resolutions`: Map of intent IDs to resolved capabilities
/// - `context`: Additional context about the plan goal
pub async fn validate_plan(
    plan: &str,
    resolutions: &HashMap<String, String>,
    context: &str,
    config: &ValidationConfig,
) -> Result<ValidationResult, String> {
    // Get LLM provider based on config
    let provider = match get_validation_provider(config).await {
        Some(p) => p,
        None => {
            log::debug!("No LLM provider available, skipping plan validation");
            return Ok(ValidationResult::valid());
        }
    };

    let resolutions_str = resolutions
        .iter()
        .map(|(k, v)| format!("  {} → {}", k, v))
        .collect::<Vec<_>>()
        .join("\n");

    let prompt = format!(
        r#"You are validating an RTFS plan for correctness.

Goal/Context: {}

Plan:
```rtfs
{}
```

Capability resolutions:
{}

Validate:
1. Schema compatibility: Does each step's output match the next step's input requirements?
2. Dependencies: Are `depends_on` references correct (no cycles, valid indices)?
3. Parameters: Are all required parameters provided or derivable from previous steps?
4. Calls: Does the plan only call available capabilities from the resolutions?

Respond in JSON format:
{{
  "is_valid": true/false,
  "errors": [
    {{"type": "schema_mismatch", "message": "...", "location": "step_2"}},
    {{"type": "missing_param", "message": "...", "location": "step_3"}}
  ],
  "suggestions": ["suggestion 1"]
}}

Respond with ONLY the JSON, no additional text."#,
        context, plan, resolutions_str
    );

    match provider.generate_text(&prompt).await {
        Ok(response) => parse_validation_response(&response),
        Err(e) => {
            log::warn!("LLM plan validation failed: {}", e);
            Ok(ValidationResult::valid()) // Fail open
        }
    }
}

/// Try to auto-repair a plan based on validation errors.
///
/// Uses the comprehensive prompt templates from `assets/prompts/cognitive_engine/auto_repair/v1/`
/// which include grammar hints, repair strategy, and anti-patterns for better LLM guidance.
///
/// # Arguments
/// - `plan`: The original plan code
/// - `errors`: Validation errors to fix
/// - `attempt`: Current repair attempt (1-indexed)
pub async fn auto_repair_plan(
    plan: &str,
    errors: &[ValidationError],
    attempt: usize,
    config: &ValidationConfig,
) -> Result<Option<String>, String> {
    if attempt > config.max_repair_attempts {
        log::info!(
            "Max repair attempts ({}) exceeded, queuing for external review",
            config.max_repair_attempts
        );
        return Ok(None);
    }

    // Get LLM provider based on config
    let provider = match get_validation_provider(config).await {
        Some(p) => p,
        None => {
            log::debug!("No LLM provider available, cannot auto-repair");
            return Ok(None);
        }
    };

    // Load prompt templates at compile time for comprehensive repair guidance
    let grammar_hints =
        include_str!("../../../../assets/prompts/cognitive_engine/auto_repair/v1/grammar.md");
    let strategy =
        include_str!("../../../../assets/prompts/cognitive_engine/auto_repair/v1/strategy.md");
    let anti_patterns =
        include_str!("../../../../assets/prompts/cognitive_engine/auto_repair/v1/anti_patterns.md");
    let task_template =
        include_str!("../../../../assets/prompts/cognitive_engine/auto_repair/v1/task.md");

    // Format diagnostics from validation errors
    let diagnostics = errors
        .iter()
        .map(|e| {
            let location = e.location.as_deref().unwrap_or("unknown");
            let fix_hint = e.suggested_fix.as_deref().unwrap_or("");
            format!(
                "- [{:?}] at {}: {}\n  Suggested fix: {}",
                e.error_type, location, e.message, fix_hint
            )
        })
        .collect::<Vec<_>>()
        .join("\n");

    // Build the full prompt from templates
    let prompt = task_template
        .replace(
            "{fixture_name}",
            &format!("auto_repair_attempt_{}", attempt),
        )
        .replace("{diagnostics}", &diagnostics)
        .replace("{grammar_hint_block}", grammar_hints)
        .replace("{fixture_guidance_block}", strategy)
        .replace("{anti_patterns_block}", anti_patterns)
        .replace("{plan}", plan);

    match provider.generate_text(&prompt).await {
        Ok(response) => {
            let repaired = extract_rtfs_code(&response);
            // Validate the repaired code looks like valid RTFS (contains at least one balanced-looking expression)
            if (repaired.contains('(') && repaired.contains(')'))
                || (repaired.contains('[') && repaired.contains(']'))
                || (repaired.contains('{') && repaired.contains('}'))
            {
                log::info!("Auto-repair attempt {} succeeded", attempt);
                Ok(Some(repaired))
            } else {
                log::warn!(
                    "Auto-repair produced response that doesn't look like RTFS: {}",
                    repaired
                );
                Ok(None)
            }
        }
        Err(e) => {
            log::warn!("LLM auto-repair failed: {}", e);
            Ok(None)
        }
    }
}

/// Try to repair a plan that failed at runtime using LLM dialog.
///
/// This is called when execution fails with a runtime error (type mismatch, etc.)
/// The LLM analyzes the error and attempts to fix the RTFS code.
///
/// # Arguments
/// - `plan`: The original RTFS plan code that failed
/// - `runtime_error`: The error message from execution
/// - `attempt`: Current repair attempt (1-indexed)
pub async fn llm_repair_runtime_error(
    plan: &str,
    runtime_error: &str,
    attempt: usize,
    config: &ValidationConfig,
) -> Result<Option<String>, String> {
    if attempt > config.max_runtime_repair_attempts {
        log::info!(
            "Max runtime repair attempts ({}) exceeded",
            config.max_runtime_repair_attempts
        );
        return Ok(None);
    }

    // Get LLM provider based on config
    let provider = match get_validation_provider(config).await {
        Some(p) => p,
        None => {
            log::debug!("No LLM provider available, cannot repair runtime error");
            return Ok(None);
        }
    };

    llm_repair_runtime_error_with_provider(
        provider.as_ref(),
        plan,
        runtime_error,
        attempt,
        config.max_runtime_repair_attempts,
    )
    .await
}

async fn llm_repair_runtime_error_with_provider(
    provider: &(dyn crate::arbiter::llm_provider::LlmProvider + Send + Sync),
    plan: &str,
    runtime_error: &str,
    attempt: usize,
    max_attempts: usize,
) -> Result<Option<String>, String> {
    let candidate =
        generate_runtime_repair_candidate(provider, plan, runtime_error, attempt, max_attempts)
            .await?;

    if rtfs_candidate_parses(&candidate) {
        log::info!(
            "Runtime repair attempt {} produced parsable candidate fix",
            attempt
        );
        Ok(Some(candidate))
    } else {
        log::warn!(
            "Runtime repair attempt {} produced non-parsable RTFS candidate",
            attempt
        );
        Ok(None)
    }
}

async fn generate_runtime_repair_candidate(
    provider: &(dyn crate::arbiter::llm_provider::LlmProvider + Send + Sync),
    plan: &str,
    runtime_error: &str,
    attempt: usize,
    max_attempts: usize,
) -> Result<String, String> {
    let prompt = format!(
        r#"You are fixing an RTFS plan that failed during execution.

Original plan:
```rtfs
{}
```

Runtime error:
{}

Repair attempt: {} of {}

Analyze the error and fix the plan. Common runtime issues:
- `expected vector, got map`: Using (get x :key) on a vector instead of accessing elements
- `expected map, got vector`: Trying to use keyword access on a vector result  
- `Type error in map`: The collection argument to `map` is not iterable

Fix guidelines:
1. If the error mentions "got map" when expecting vector, check if you need to extract an array field first
2. If the error mentions "got vector" when expecting map, the data is already a collection - use it directly
3. Look at the step bindings to trace what type each variable holds
4. If `step_N` is the result of `map`, it's a vector - don't use (get step_N :key) on it

Respond with ONLY the corrected RTFS plan code, no explanations."#,
        plan, runtime_error, attempt, max_attempts
    );

    let response = provider
        .generate_text(&prompt)
        .await
        .map_err(|e| format!("LLM runtime repair failed: {}", e))?;

    Ok(extract_rtfs_code(&response))
}

/// Repair a runtime error with bounded retries, using parse validation between attempts.
///
/// This is a convenience wrapper around `llm_repair_runtime_error` that:
/// - iterates attempts up to `config.max_runtime_repair_attempts`
/// - returns the first parsable candidate plan
pub async fn repair_runtime_error_with_retry(
    plan: &str,
    runtime_error: &str,
    config: &ValidationConfig,
) -> Result<Option<String>, String> {
    // Get LLM provider based on config
    let provider = match get_validation_provider(config).await {
        Some(p) => p,
        None => {
            log::debug!("No LLM provider available, cannot repair runtime error");
            return Ok(None);
        }
    };

    repair_runtime_error_with_retry_with_provider(
        provider.as_ref(),
        plan,
        runtime_error,
        config.max_runtime_repair_attempts,
    )
    .await
}

async fn repair_runtime_error_with_retry_with_provider(
    provider: &(dyn crate::arbiter::llm_provider::LlmProvider + Send + Sync),
    plan: &str,
    runtime_error: &str,
    max_attempts: usize,
) -> Result<Option<String>, String> {
    let mut current_plan = plan.to_string();
    let mut last_error = runtime_error.to_string();

    for attempt in 1..=max_attempts {
        let candidate =
            generate_runtime_repair_candidate(provider, &current_plan, &last_error, attempt, max_attempts)
                .await?;

        if rtfs_candidate_parses(&candidate) {
            return Ok(Some(candidate));
        }

        let parse_err = rtfs_candidate_parse_error(&candidate)
            .unwrap_or_else(|| "unknown parse error".to_string());

        // Feed the model its own failed output + the parse diagnostic to converge on valid RTFS.
        current_plan = candidate;
        last_error = format!(
            "{}\n\nPrevious candidate failed to parse: {}\nReturn ONLY corrected RTFS (no markdown).",
            runtime_error, parse_err
        );
    }

    Ok(None)
}

/// Parse LLM validation response JSON
fn parse_validation_response(response: &str) -> Result<ValidationResult, String> {
    // Extract JSON from response (may be wrapped in markdown code block)
    let json_str = if let Some(start) = response.find('{') {
        if let Some(end) = response.rfind('}') {
            &response[start..=end]
        } else {
            response
        }
    } else {
        response
    };

    #[derive(serde::Deserialize)]
    struct LlmValidationResponse {
        is_valid: bool,
        #[serde(default)]
        errors: Vec<LlmError>,
        #[serde(default)]
        suggestions: Vec<String>,
    }

    #[derive(serde::Deserialize)]
    struct LlmError {
        #[serde(default, rename = "type")]
        error_type: Option<String>,
        message: String,
        #[serde(default)]
        location: Option<String>,
    }

    match serde_json::from_str::<LlmValidationResponse>(json_str) {
        Ok(parsed) => {
            let errors: Vec<ValidationError> = parsed
                .errors
                .iter()
                .map(|e| ValidationError {
                    error_type: match e.error_type.as_deref() {
                        Some("schema_mismatch") => ValidationErrorType::SchemaMismatch,
                        Some("missing_param") => ValidationErrorType::MissingParameter,
                        Some("invalid_dependency") => ValidationErrorType::InvalidDependency,
                        Some("cyclic_dependency") => ValidationErrorType::CyclicDependency,
                        Some("unavailable_capability") => {
                            ValidationErrorType::UnavailableCapability
                        }
                        Some("type_incompatibility") => ValidationErrorType::TypeIncompatibility,
                        _ => ValidationErrorType::Other,
                    },
                    message: e.message.clone(),
                    location: e.location.clone(),
                    suggested_fix: None,
                })
                .collect();

            Ok(ValidationResult {
                is_valid: parsed.is_valid,
                errors,
                suggestions: parsed.suggestions,
            })
        }
        Err(e) => {
            log::debug!("Failed to parse validation response: {}", e);
            Ok(ValidationResult::valid()) // Fail open on parse errors
        }
    }
}

/// Extract RTFS code from LLM response
fn extract_rtfs_code(response: &str) -> String {
    // Check for markdown code block
    if let Some(start) = response.find("```rtfs") {
        if let Some(end) = response[start + 7..].find("```") {
            return response[start + 7..start + 7 + end].trim().to_string();
        }
    }
    if let Some(start) = response.find("```") {
        if let Some(end) = response[start + 3..].find("```") {
            return response[start + 3..start + 3 + end].trim().to_string();
        }
        // If it starts with ``` but doesn't have a closing one, assume the rest is code
        return response[start + 3..].trim().to_string();
    }
    // Return as-is if no code block found
    response.trim().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use async_trait::async_trait;
    use rtfs::runtime::error::RuntimeError;
    use std::collections::HashMap;
    use std::sync::{Arc, Mutex};

    use crate::arbiter::llm_provider::{
        LlmProvider, LlmProviderInfo, ValidationResult as ProviderValidationResult,
    };
    use crate::types::{Plan, StorableIntent};

    struct SequenceLlmProvider {
        responses: Arc<Mutex<Vec<String>>>,
        prompts: Arc<Mutex<Vec<String>>>,
    }

    impl SequenceLlmProvider {
        fn new(responses: Vec<String>) -> Self {
            Self {
                responses: Arc::new(Mutex::new(responses)),
                prompts: Arc::new(Mutex::new(Vec::new())),
            }
        }

        fn prompts(&self) -> Vec<String> {
            self.prompts
                .lock()
                .map(|p| p.clone())
                .unwrap_or_default()
        }
    }

    #[async_trait]
    impl LlmProvider for SequenceLlmProvider {
        async fn generate_intent(
            &self,
            _prompt: &str,
            _context: Option<HashMap<String, String>>,
        ) -> Result<StorableIntent, RuntimeError> {
            Err(RuntimeError::Generic(
                "SequenceLlmProvider: generate_intent not implemented".to_string(),
            ))
        }

        async fn generate_plan(
            &self,
            _intent: &StorableIntent,
            _context: Option<HashMap<String, String>>,
        ) -> Result<Plan, RuntimeError> {
            Err(RuntimeError::Generic(
                "SequenceLlmProvider: generate_plan not implemented".to_string(),
            ))
        }

        async fn validate_plan(
            &self,
            _plan_content: &str,
        ) -> Result<ProviderValidationResult, RuntimeError> {
            Ok(ProviderValidationResult {
                is_valid: true,
                confidence: 1.0,
                reasoning: "stub".to_string(),
                suggestions: vec![],
                errors: vec![],
            })
        }

        async fn generate_text(&self, prompt: &str) -> Result<String, RuntimeError> {
            // Capture prompt so tests can assert retry feedback is included.
            if let Ok(mut prompts) = self.prompts.lock() {
                prompts.push(prompt.to_string());
            }

            let mut guard = self
                .responses
                .lock()
                .map_err(|_| RuntimeError::Generic("poisoned responses mutex".to_string()))?;
            if guard.is_empty() {
                return Err(RuntimeError::Generic(
                    "SequenceLlmProvider: no more responses".to_string(),
                ));
            }
            Ok(guard.remove(0))
        }

        fn get_info(&self) -> LlmProviderInfo {
            LlmProviderInfo {
                name: "SequenceLlmProvider".to_string(),
                version: "test".to_string(),
                model: "test".to_string(),
                capabilities: vec!["generate_text".to_string()],
            }
        }
    }

    #[test]
    fn test_default_config() {
        let config = ValidationConfig::default();
        assert!(!config.enable_schema_validation);
        assert!(!config.enable_plan_validation);
        assert!(config.enable_auto_repair);
        assert_eq!(config.max_repair_attempts, 2);
    }

    #[tokio::test]
    async fn test_validate_schema_placeholder() {
        let config = ValidationConfig::default();
        let result = validate_schema("[:map]", "test capability", None, &config).await;
        assert!(result.is_ok());
        assert!(result.unwrap().is_valid);
    }

    #[tokio::test]
    async fn test_llm_repair_runtime_error_rejects_non_parsable_candidate() {
        let provider = SequenceLlmProvider::new(vec![
            // Missing closing paren -> should fail parse validation.
            "(do (step \"Divide\" (/ 42 0))".to_string(),
        ]);

        let repaired = llm_repair_runtime_error_with_provider(
            &provider,
            "(do (step \"Divide\" (/ 42 0)))",
            "division by zero",
            1,
            3,
        )
        .await
        .expect("repair attempt should not error");

        assert!(repaired.is_none());
    }

    #[tokio::test]
    async fn test_runtime_repair_with_retry_uses_parse_feedback() {
        let provider = SequenceLlmProvider::new(vec![
            // First response: invalid RTFS
            "(do (step \"Divide\" (/ 42 0))".to_string(),
            // Second response: valid RTFS (still has the runtime bug, but parses)
            "```rtfs\n(do (step \"Divide\" (/ 42 0)))\n```".to_string(),
        ]);

        let repaired = repair_runtime_error_with_retry_with_provider(
            &provider,
            "(do (step \"Divide\" (/ 42 0)))",
            "division by zero",
            2,
        )
        .await
        .expect("retry repair should not error")
        .expect("should return a parsable candidate on second attempt");

        assert!(rtfs_candidate_parses(&repaired));

        let prompts = provider.prompts();
        assert_eq!(prompts.len(), 2);
        assert!(prompts[1].contains("Previous candidate failed to parse"));
    }
}
