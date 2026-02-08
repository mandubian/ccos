//! Coding Agent for specialized code generation
//!
//! Delegates code generation tasks to specialized coding LLMs based on language
//! and task requirements.

use crate::cognitive_engine::llm_provider::{
    LlmProviderConfig, LlmProviderFactory, LlmProviderType,
};
use crate::config::types::{CodingAgentProfile, CodingAgentsConfig};
use rtfs::runtime::error::{RuntimeError, RuntimeResult};
use serde::{Deserialize, Serialize};

/// Request for code generation
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CodingRequest {
    /// Task description for the coding agent
    pub task: String,
    /// Target programming language (optional, agent will infer if not provided)
    #[serde(default)]
    pub language: Option<String>,
    /// Input file names/descriptions
    #[serde(default)]
    pub inputs: Vec<String>,
    /// Expected output file names/descriptions
    #[serde(default)]
    pub outputs: Vec<String>,
    /// Optional constraints for code generation
    #[serde(default)]
    pub constraints: Option<CodingConstraints>,
    /// Optional profile name to use (overrides default selection)
    #[serde(default)]
    pub profile: Option<String>,
    /// Optional prior attempts context for refinement
    #[serde(default)]
    pub prior_attempts: Vec<AttemptContext>,
}

/// Context for a prior failed attempt
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AttemptContext {
    /// The code that failed
    pub code: String,
    /// The error message or classification
    pub error: String,
    /// The attempt number (1-based)
    pub attempt: u32,
}

/// Constraints for code generation
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct CodingConstraints {
    /// Maximum lines of code
    #[serde(default)]
    pub max_lines: Option<u32>,
    /// Whether external dependencies are allowed
    #[serde(default = "default_true")]
    pub dependencies_allowed: bool,
    /// Timeout in milliseconds for execution
    #[serde(default)]
    pub timeout_ms: Option<u64>,
}

fn default_true() -> bool {
    true
}

/// Response from coding agent
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CodingResponse {
    /// Generated code
    pub code: String,
    /// Programming language of the generated code
    pub language: String,
    /// Required dependencies
    #[serde(default)]
    pub dependencies: Vec<String>,
    /// Explanation of the code
    pub explanation: String,
    /// Optional test code
    #[serde(default)]
    pub tests: Option<String>,
}

/// Coding Agent handles code generation delegated to specialized LLMs
pub struct CodingAgent {
    config: CodingAgentsConfig,
}

impl CodingAgent {
    /// Create a new coding agent with the given configuration
    pub fn new(config: CodingAgentsConfig) -> Self {
        Self { config }
    }

    /// Select the best profile for the given request
    fn select_profile(&self, request: &CodingRequest) -> Option<&CodingAgentProfile> {
        // If profile explicitly specified, use it
        if let Some(ref profile_name) = request.profile {
            return self
                .config
                .profiles
                .iter()
                .find(|p| &p.name == profile_name);
        }

        // Otherwise use default
        self.config
            .profiles
            .iter()
            .find(|p| p.name == self.config.default)
    }

    /// Build the prompt for code generation
    fn build_prompt(&self, request: &CodingRequest) -> String {
        let mut prompt = if !request.prior_attempts.is_empty() {
            let mut p = format!(
                "You previously attempted the following task but it failed. Please review the failure history, fix the code, and try again.\n\n**Initial Task**: {}\n",
                request.task
            );

            p.push_str("\n**Failure History**:\n");
            for attempt in &request.prior_attempts {
                p.push_str(&format!(
                    "\n--- Attempt #{} ---\n**Failed Code**:\n```{}\n{}\n```\n**Error**:\n{}\n",
                    attempt.attempt,
                    request.language.as_ref().unwrap_or(&"code".to_string()),
                    attempt.code,
                    attempt.error
                ));
            }

            p.push_str("\n**Goal**: Analyze why previous attempts failed and generate a corrected version that completes the task correctly.\n");
            p
        } else {
            format!(
                "Generate code for the following task:\n\n**Task**: {}\n",
                request.task
            )
        };

        if let Some(ref lang) = request.language {
            prompt.push_str(&format!("\n**Language**: {}\n", lang));
        }

        if !request.inputs.is_empty() {
            prompt.push_str("\n**Input files**:\n");
            for input in &request.inputs {
                prompt.push_str(&format!("- {}\n", input));
            }
        }

        if !request.outputs.is_empty() {
            prompt.push_str("\n**Expected outputs**:\n");
            for output in &request.outputs {
                prompt.push_str(&format!("- {}\n", output));
            }
        }

        if let Some(ref constraints) = request.constraints {
            prompt.push_str("\n**Constraints**:\n");
            if let Some(max_lines) = constraints.max_lines {
                prompt.push_str(&format!("- Maximum {} lines of code\n", max_lines));
            }
            if !constraints.dependencies_allowed {
                prompt.push_str("- No external dependencies allowed\n");
            }
        }

        prompt.push_str(
            r#"

**Response Format** (JSON):
```json
{
  "code": "<the generated code>",
  "language": "<programming language>",
  "dependencies": ["<list>", "<of>", "<dependencies>"],
  "explanation": "<brief explanation of the code>",
  "tests": "<optional test code>"
}
```

Respond ONLY with the JSON object, no additional text.
"#,
        );

        prompt
    }

    /// Parse the LLM response into a structured CodingResponse
    fn parse_response(&self, response: &str) -> Result<CodingResponse, String> {
        // Try to find JSON in the response (handle markdown code blocks)
        let json_str = if response.contains("```json") {
            response
                .split("```json")
                .nth(1)
                .and_then(|s| s.split("```").next())
                .unwrap_or(response)
                .trim()
        } else if response.contains("```") {
            response.split("```").nth(1).unwrap_or(response).trim()
        } else {
            response.trim()
        };

        serde_json::from_str(json_str).map_err(|e| {
            format!(
                "Failed to parse coding response: {}. Response: {}",
                e, json_str
            )
        })
    }

    /// Generate code for the given request
    pub async fn generate(&self, request: &CodingRequest) -> RuntimeResult<CodingResponse> {
        let profile = self.select_profile(request).ok_or_else(|| {
            RuntimeError::Generic(format!(
                "No coding profile found. Available: {:?}",
                self.config
                    .profiles
                    .iter()
                    .map(|p| &p.name)
                    .collect::<Vec<_>>()
            ))
        })?;

        // Resolve API key from environment
        let api_key = std::env::var(&profile.api_key_env).map_err(|_| {
            RuntimeError::Generic(format!(
                "API key not found in environment variable: {}",
                profile.api_key_env
            ))
        })?;

        // Map provider string to LlmProviderType
        let provider_type = match profile.provider.as_str() {
            "openai" => LlmProviderType::OpenAI,
            "anthropic" => LlmProviderType::Anthropic,
            "openrouter" => LlmProviderType::OpenAI, // OpenRouter uses OpenAI-compatible API
            "local" => LlmProviderType::Local,
            _ => LlmProviderType::OpenAI,
        };

        // Determine base URL for OpenRouter
        let base_url = if profile.provider == "openrouter" {
            Some("https://openrouter.ai/api/v1".to_string())
        } else {
            None
        };

        // Create LLM provider
        let provider_config = LlmProviderConfig {
            provider_type,
            model: profile.model.clone(),
            api_key: Some(api_key),
            base_url,
            max_tokens: Some(profile.max_tokens),
            temperature: Some(profile.temperature as f64),
            timeout_seconds: Some(120),
            retry_config: Default::default(),
        };

        let provider = LlmProviderFactory::create_provider(provider_config)
            .await
            .map_err(|e| RuntimeError::Generic(format!("Failed to create LLM provider: {}", e)))?;

        // Build prompt with system context
        let system_prompt = &profile.system_prompt;
        let user_prompt = self.build_prompt(request);

        // Combine system and user prompt (simple approach for now)
        let full_prompt = format!("{}\n\n---\n\n{}", system_prompt, user_prompt);

        // Generate response
        let response_text = provider.generate_text(&full_prompt).await?;

        // Parse response
        let response = self
            .parse_response(&response_text)
            .map_err(|e| RuntimeError::Generic(e))?;

        Ok(response)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_response_json() {
        let config = CodingAgentsConfig::default();
        let agent = CodingAgent::new(config);

        let response = r#"```json
{
  "code": "print('hello')",
  "language": "python",
  "dependencies": [],
  "explanation": "A simple hello world"
}
```"#;

        let parsed = agent.parse_response(response).unwrap();
        assert_eq!(parsed.code, "print('hello')");
        assert_eq!(parsed.language, "python");
    }

    #[test]
    fn test_build_prompt() {
        let config = CodingAgentsConfig::default();
        let agent = CodingAgent::new(config);

        let request = CodingRequest {
            task: "Create a bar chart".to_string(),
            language: Some("python".to_string()),
            inputs: vec!["data.csv".to_string()],
            outputs: vec!["chart.png".to_string()],
            constraints: None,
            profile: None,
            prior_attempts: vec![],
        };

        let prompt = agent.build_prompt(&request);
        assert!(prompt.contains("Create a bar chart"));
        assert!(prompt.contains("python"));
        assert!(prompt.contains("data.csv"));
    }
}
