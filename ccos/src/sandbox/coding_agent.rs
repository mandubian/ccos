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
    /// Available skills/tools the generated code may use
    #[serde(default)]
    pub skill_hints: Vec<SkillHint>,
    /// Declared output slots the generated code MUST store via ccos_sdk.memory.store()
    #[serde(default)]
    pub expected_outputs: Vec<ExpectedOutput>,
}

/// Declares a required output slot the generated code must persist via ccos_sdk.memory.store()
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ExpectedOutput {
    /// The key to use with ccos_sdk.memory.store(key, ...)
    pub key: String,
    /// Human-readable description of what should be stored
    pub description: String,
    /// RTFS type hint for the stored value, e.g. "{:items [{:title str :url str}]}"
    #[serde(default)]
    pub schema_hint: Option<String>,
}

/// Describes data actually stored via ccos_sdk.memory.store() during execution
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct StoredArtifact {
    /// The memory key used in ccos_sdk.memory.store(key, ...)
    pub key: String,
    /// Human-readable description of what was stored
    pub description: String,
    /// RTFS type hint for the stored value
    #[serde(default)]
    pub schema_hint: Option<String>,
}

/// A hint about an available skill/tool the code generator can use
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct SkillHint {
    /// Human-readable skill name
    pub name: String,
    /// Short description of what the skill provides
    pub description: String,
    /// Raw instructions / markdown for how to use this skill (HTTP endpoints, etc.)
    pub instructions: String,
}

/// Classification of why a code attempt failed — used to generate targeted fix guidance
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AttemptErrorType {
    /// DNS or network unreachable — try different hosts/mirrors or add fallback logic
    NetworkFailure,
    /// Code executed but produced wrong or empty output
    LogicError,
    /// Code crashed with a runtime exception
    RuntimeException,
    /// Execution exceeded the allowed time
    Timeout,
    /// Stored output did not match the expected schema
    SchemaValidationFailure,
}

impl AttemptErrorType {
    /// Returns targeted fix guidance to inject into the prompt
    pub fn guidance(&self) -> &'static str {
        match self {
            AttemptErrorType::NetworkFailure =>
                "The host was unreachable (DNS/connection error). Use a list of fallback URLs/mirrors \
                 and loop over them with a short per-request timeout (e.g. 5s). Catch each exception \
                 individually and only raise after all options are exhausted.",
            AttemptErrorType::LogicError =>
                "The code ran but produced incorrect or empty output. Review the logic carefully, \
                 check parsing assumptions, and add defensive checks for empty/unexpected responses.",
            AttemptErrorType::RuntimeException =>
                "The code raised an unhandled exception. Add try/except around the critical section, \
                 log the error, and ensure all edge cases are handled.",
            AttemptErrorType::Timeout =>
                "Execution timed out. Reduce the scope per request, add explicit short timeouts \
                 (e.g. requests timeout=5), and avoid blocking calls without a deadline.",
            AttemptErrorType::SchemaValidationFailure =>
                "The stored output did not match the required schema. Review the expected schema \
                 carefully and ensure every required field is present with the correct type.",
        }
    }
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
    /// Classified error type for targeted fix guidance in the prompt
    #[serde(default)]
    pub error_type: Option<AttemptErrorType>,
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
    /// Artifacts stored via ccos_sdk.memory.store() — declared by the LLM based on expected_outputs
    #[serde(default)]
    pub stored_artifacts: Vec<StoredArtifact>,
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
                if let Some(ref et) = attempt.error_type {
                    p.push_str(&format!("**Fix guidance**: {}\n", et.guidance()));
                }
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

        if !request.skill_hints.is_empty() {
            prompt.push_str("\n**Available Tools & Data Sources**:\n");
            prompt.push_str("The following skills/tools are available. Prefer them over simulating data or using restricted APIs (e.g. use Nitter instead of authenticating to X/Twitter directly).\n");
            for hint in &request.skill_hints {
                prompt.push_str(&format!("\n### {}\n", hint.name));
                if !hint.description.is_empty() {
                    prompt.push_str(&format!("{} \n", hint.description));
                }
                if !hint.instructions.is_empty() {
                    prompt.push_str(&format!("{}\n", hint.instructions));
                }
            }
            prompt.push_str("\n");
        }

        // Inject ccos_sdk documentation — hard contract when expected_outputs declared, soft guidance otherwise
        if !request.expected_outputs.is_empty() {
            prompt.push_str("\n**Required Outputs (CCOS SDK — MANDATORY)**:\n");
            prompt.push_str("You MUST store your results using `ccos_sdk.memory.store()`. The following outputs are required:\n");
            for eo in &request.expected_outputs {
                prompt.push_str(&format!("\n- **Key**: `\"{}\"`\n", eo.key));
                if !eo.description.is_empty() {
                    prompt.push_str(&format!("  Description: {}\n", eo.description));
                }
                if let Some(ref sh) = eo.schema_hint {
                    prompt.push_str(&format!("  Schema (RTFS): `{}`\n", sh));
                }
                prompt.push_str(&format!("  → `ccos_sdk.memory.store(\"{}\", your_data)`\n", eo.key));
            }
            prompt.push_str(r#"
```python
import ccos_sdk
ccos_sdk.memory.store("key", data)   # REQUIRED at end of script
data = ccos_sdk.memory.get("key", default=None)  # read prior stored value
```
Do NOT write local files — they are lost when the sandbox exits.
Also print a human-readable summary to stdout.
"#);
        } else {
            prompt.push_str(r#"
**Persistence & State (CCOS SDK)**:
A `ccos_sdk` module is always available. Use it to persist results instead of writing local files
(which are lost when the sandbox exits).

```python
import ccos_sdk
ccos_sdk.memory.store("result_key", {"data": ...})  # persists beyond sandbox exit
data = ccos_sdk.memory.get("result_key", default=None)
```

Choose a descriptive key (e.g. `"tweets_result"`, `"analysis_output"`) and always call
`ccos_sdk.memory.store()` at the end of your script. Also print a human-readable summary to stdout.
"#);
        }

        // Generic resilience instruction — always injected regardless of skill or task
        prompt.push_str(r#"
**Resilience Requirements**:
If your code makes any network requests:
- Use a short per-request timeout (e.g. 5 seconds). Never block without a deadline.
- If multiple endpoints/mirrors are available, loop over them and try each in order.
- Catch exceptions per attempt individually; only fail after all options are exhausted.
- Do not let a single unreachable host cause the entire script to fail.
"#);

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
  "tests": "<optional test code>",
  "stored_artifacts": [
    {
      "key": "<the key passed to ccos_sdk.memory.store()>",
      "description": "<what was stored>",
      "schema_hint": "<RTFS type hint, e.g. {:items [{:title str :url str}]}>"
    }
  ]
}
```

`stored_artifacts` must list every key your code passes to `ccos_sdk.memory.store()`.
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
            skill_hints: vec![],
            expected_outputs: vec![],
        };

        let prompt = agent.build_prompt(&request);
        assert!(prompt.contains("Create a bar chart"));
        assert!(prompt.contains("python"));
        assert!(prompt.contains("data.csv"));
    }
}
