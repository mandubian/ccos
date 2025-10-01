//! LLM Provider Abstraction
//!
//! This module provides the abstraction layer for different LLM providers,
//! allowing the Arbiter to work with various LLM services while maintaining
//! a consistent interface.

use crate::ccos::arbiter::prompt::{FilePromptStore, PromptManager};
use crate::ccos::types::{
    GenerationContext, IntentStatus, Plan, PlanBody, PlanLanguage, StorableIntent, TriggerSource,
};
use crate::parser;
use crate::runtime::error::RuntimeError;
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::collections::HashMap; // for validating reduced-grammar RTFS plans
use std::sync::atomic::{AtomicU64, Ordering};

/// Result of plan validation by an LLM provider
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ValidationResult {
    pub is_valid: bool,
    pub confidence: f64,
    pub reasoning: String,
    pub suggestions: Vec<String>,
    pub errors: Vec<String>,
}

/// Metrics for tracking retry behavior
#[derive(Debug)]
pub struct RetryMetrics {
    /// Total number of plan generation attempts (including first attempts)
    pub total_attempts: AtomicU64,
    /// Number of successful retries (attempts > 1 that succeeded)
    pub successful_retries: AtomicU64,
    /// Number of failed retries (attempts > 1 that failed)
    pub failed_retries: AtomicU64,
    /// Number of first attempts that succeeded (no retry needed)
    pub first_attempt_successes: AtomicU64,
    /// Number of first attempts that failed (required retry)
    pub first_attempt_failures: AtomicU64,
}

impl RetryMetrics {
    pub fn new() -> Self {
        Self {
            total_attempts: AtomicU64::new(0),
            successful_retries: AtomicU64::new(0),
            failed_retries: AtomicU64::new(0),
            first_attempt_successes: AtomicU64::new(0),
            first_attempt_failures: AtomicU64::new(0),
        }
    }

    /// Record a successful plan generation
    pub fn record_success(&self, attempt_number: u32) {
        self.total_attempts.fetch_add(1, Ordering::Relaxed);
        if attempt_number == 1 {
            self.first_attempt_successes.fetch_add(1, Ordering::Relaxed);
        } else {
            self.successful_retries.fetch_add(1, Ordering::Relaxed);
        }
    }

    /// Record a failed plan generation
    pub fn record_failure(&self, attempt_number: u32) {
        self.total_attempts.fetch_add(1, Ordering::Relaxed);
        if attempt_number == 1 {
            self.first_attempt_failures.fetch_add(1, Ordering::Relaxed);
        } else {
            self.failed_retries.fetch_add(1, Ordering::Relaxed);
        }
    }

    /// Get current metrics as a summary
    pub fn get_summary(&self) -> RetryMetricsSummary {
        RetryMetricsSummary {
            total_attempts: self.total_attempts.load(Ordering::Relaxed),
            successful_retries: self.successful_retries.load(Ordering::Relaxed),
            failed_retries: self.failed_retries.load(Ordering::Relaxed),
            first_attempt_successes: self.first_attempt_successes.load(Ordering::Relaxed),
            first_attempt_failures: self.first_attempt_failures.load(Ordering::Relaxed),
        }
    }
}

/// Summary of retry metrics for reporting
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RetryMetricsSummary {
    pub total_attempts: u64,
    pub successful_retries: u64,
    pub failed_retries: u64,
    pub first_attempt_successes: u64,
    pub first_attempt_failures: u64,
}

impl RetryMetricsSummary {
    /// Calculate retry success rate (successful retries / total retries)
    pub fn retry_success_rate(&self) -> f64 {
        let total_retries = self.successful_retries + self.failed_retries;
        if total_retries == 0 {
            0.0
        } else {
            self.successful_retries as f64 / total_retries as f64
        }
    }

    /// Calculate overall success rate (all successes / all attempts)
    pub fn overall_success_rate(&self) -> f64 {
        if self.total_attempts == 0 {
            0.0
        } else {
            (self.first_attempt_successes + self.successful_retries) as f64 / self.total_attempts as f64
        }
    }

    /// Calculate first attempt success rate
    pub fn first_attempt_success_rate(&self) -> f64 {
        let first_attempts = self.first_attempt_successes + self.first_attempt_failures;
        if first_attempts == 0 {
            0.0
        } else {
            self.first_attempt_successes as f64 / first_attempts as f64
        }
    }
}

/// Configuration for LLM providers
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LlmProviderConfig {
    pub provider_type: LlmProviderType,
    pub model: String,
    pub api_key: Option<String>,
    pub base_url: Option<String>,
    pub max_tokens: Option<u32>,
    pub temperature: Option<f64>,
    pub timeout_seconds: Option<u64>,
    pub retry_config: crate::ccos::arbiter::arbiter_config::RetryConfig,
}

/// Supported LLM provider types
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum LlmProviderType {
    Stub,      // For testing - deterministic responses
    OpenAI,    // OpenAI GPT models
    Anthropic, // Anthropic Claude models
    Local,     // Local models (Ollama, etc.)
}

/// Abstract interface for LLM providers
#[async_trait]
pub trait LlmProvider: Send + Sync {
    /// Generate an Intent from natural language
    async fn generate_intent(
        &self,
        prompt: &str,
        context: Option<HashMap<String, String>>,
    ) -> Result<StorableIntent, RuntimeError>;

    /// Generate a Plan from an Intent
    async fn generate_plan(
        &self,
        intent: &StorableIntent,
        context: Option<HashMap<String, String>>,
    ) -> Result<Plan, RuntimeError>;

    /// Generate a Plan from an Intent with retry logic
    async fn generate_plan_with_retry(
        &self,
        intent: &StorableIntent,
        context: Option<HashMap<String, String>>,
    ) -> Result<Plan, RuntimeError> {
        // Default implementation just calls generate_plan
        // Individual providers can override this for custom retry logic
        self.generate_plan(intent, context).await
    }

    /// Get retry metrics summary for monitoring and debugging
    fn get_retry_metrics(&self) -> Option<RetryMetricsSummary> {
        // Default implementation returns None
        // Individual providers can override this to provide metrics
        None
    }

    /// Validate a generated Plan (using string representation to avoid Send/Sync issues)
    async fn validate_plan(&self, plan_content: &str) -> Result<ValidationResult, RuntimeError>;

    /// Generate text from a prompt (generic text generation)
    async fn generate_text(&self, prompt: &str) -> Result<String, RuntimeError>;

    /// Get provider information
    fn get_info(&self) -> LlmProviderInfo;
}

/// Information about an LLM provider
#[derive(Debug, Clone)]
pub struct LlmProviderInfo {
    pub name: String,
    pub version: String,
    pub model: String,
    pub capabilities: Vec<String>,
}

/// OpenAI-compatible provider (works with OpenAI and OpenRouter)
pub struct OpenAILlmProvider {
    config: LlmProviderConfig,
    client: reqwest::Client,
    metrics: RetryMetrics,
    prompt_manager: PromptManager<FilePromptStore>,
}

impl OpenAILlmProvider {
    pub fn new(config: LlmProviderConfig) -> Result<Self, RuntimeError> {
        let client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(
                config.timeout_seconds.unwrap_or(30),
            ))
            .build()
            .map_err(|e| RuntimeError::Generic(format!("Failed to create HTTP client: {}", e)))?;

        // Assets are at workspace root, so try ../assets first, then assets (for when run from workspace root)
        let prompt_path = if std::path::Path::new("../assets/prompts/arbiter").exists() {
            "../assets/prompts/arbiter"
        } else {
            "assets/prompts/arbiter"
        };
        let prompt_store = FilePromptStore::new(prompt_path);
        let prompt_manager = PromptManager::new(prompt_store);

        Ok(Self { 
            config, 
            client,
            metrics: RetryMetrics::new(),
            prompt_manager,
        })
    }

    /// Get current retry metrics summary
    pub fn get_retry_metrics(&self) -> RetryMetricsSummary {
        self.metrics.get_summary()
    }

    /// Extracts the first top-level (do ...) s-expression from a text blob.
    fn extract_do_block(text: &str) -> Option<String> {
        let start = text.find("(do");
        let start = match start {
            Some(s) => s,
            None => return None,
        };
        let mut depth = 0usize;
        for (idx, ch) in text[start..].char_indices() {
            match ch {
                '(' => depth += 1,
                ')' => {
                    if depth == 0 {
                        return None;
                    }
                    depth -= 1;
                    if depth == 0 {
                        let end = start + idx + 1;
                        return Some(text[start..end].to_string());
                    }
                }
                _ => {}
            }
        }
        None
    }

    /// Extracts the first top-level (plan ...) s-expression from a text blob.
    fn extract_plan_block(text: &str) -> Option<String> {
        let start = text.find("(plan");
        let start = match start {
            Some(s) => s,
            None => return None,
        };
        let mut depth = 0usize;
        for (idx, ch) in text[start..].char_indices() {
            match ch {
                '(' => depth += 1,
                ')' => {
                    if depth == 0 {
                        return None;
                    }
                    depth -= 1;
                    if depth == 0 {
                        let end = start + idx + 1;
                        return Some(text[start..end].to_string());
                    }
                }
                _ => {}
            }
        }
        None
    }

    /// Very small helper to extract a quoted string value following a given keyword in a plan block.
    /// Example: for key ":name" extracts the first "..." after it.
    fn extract_quoted_value_after_key(plan_block: &str, key: &str) -> Option<String> {
        if let Some(kpos) = plan_block.find(key) {
            let after = &plan_block[kpos + key.len()..];
            if let Some(q1) = after.find('"') {
                let rest = &after[q1 + 1..];
                if let Some(q2) = rest.find('"') {
                    return Some(rest[..q2].to_string());
                }
            }
        }
        None
    }

    /// Extracts the first top-level s-expression immediately following a given keyword key.
    /// Example: for key ":body", extracts the (do ...) s-expression right after it, skipping quoted text.
    fn extract_s_expr_after_key(text: &str, key: &str) -> Option<String> {
        let kpos = text.find(key)?;
        let after = &text[kpos + key.len()..];
        // Find the first unquoted '(' after the key
        let mut in_string = false;
        let mut prev: Option<char> = None;
        let mut rel_start: Option<usize> = None;
        for (i, ch) in after.char_indices() {
            match ch {
                '"' => {
                    if prev != Some('\\') {
                        in_string = !in_string;
                    }
                }
                '(' if !in_string => {
                    rel_start = Some(i);
                    break;
                }
                _ => {}
            }
            prev = Some(ch);
        }
        let rel_start = rel_start?;
        let start = kpos + key.len() + rel_start;

        // Extract balanced s-expression starting at start
        let mut depth = 0usize;
        for (idx, ch) in text[start..].char_indices() {
            match ch {
                '(' => depth += 1,
                ')' => {
                    if depth == 0 {
                        return None;
                    }
                    depth -= 1;
                    if depth == 0 {
                        let end = start + idx + 1;
                        return Some(text[start..end].to_string());
                    }
                }
                _ => {}
            }
        }
        None
    }

    async fn make_request(&self, messages: Vec<OpenAIMessage>) -> Result<String, RuntimeError> {
        let api_key = self.config.api_key.as_ref().ok_or_else(|| {
            RuntimeError::Generic("API key required for OpenAI provider".to_string())
        })?;

        let base_url = self
            .config
            .base_url
            .as_deref()
            .unwrap_or("https://api.openai.com/v1");
        let url = format!("{}/chat/completions", base_url);

        let request_body = OpenAIRequest {
            model: self.config.model.clone(),
            messages,
            max_tokens: self.config.max_tokens,
            temperature: self.config.temperature,
        };

        let response = self
            .client
            .post(&url)
            .header("Authorization", format!("Bearer {}", api_key))
            .header("Content-Type", "application/json")
            .json(&request_body)
            .send()
            .await
            .map_err(|e| RuntimeError::Generic(format!("HTTP request failed: {}", e)))?;

        if !response.status().is_success() {
            let error_text = response
                .text()
                .await
                .unwrap_or_else(|_| "Unknown error".to_string());
            return Err(RuntimeError::Generic(format!(
                "API request failed: {}",
                error_text
            )));
        }

        let response_body: OpenAIResponse = response
            .json()
            .await
            .map_err(|e| RuntimeError::Generic(format!("Failed to parse response: {}", e)))?;

        Ok(response_body.choices[0].message.content.clone())
    }

    fn parse_intent_from_json(&self, json_str: &str) -> Result<StorableIntent, RuntimeError> {
        // Try to extract JSON from the response (it might be wrapped in markdown)
        let json_start = json_str.find('{').unwrap_or(0);
        let json_end = json_str.rfind('}').map(|i| i + 1).unwrap_or(json_str.len());
        let json_content = &json_str[json_start..json_end];

        #[derive(Deserialize)]
        struct IntentJson {
            name: Option<String>,
            goal: String,
            constraints: Option<HashMap<String, String>>,
            preferences: Option<HashMap<String, String>>,
            success_criteria: Option<String>,
        }

        let intent_json: IntentJson = serde_json::from_str(json_content)
            .map_err(|e| RuntimeError::Generic(format!("Failed to parse intent JSON: {}", e)))?;

        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs();

        Ok(StorableIntent {
            intent_id: format!("openai_intent_{}", uuid::Uuid::new_v4()),
            name: intent_json.name,
            original_request: "".to_string(), // Will be set by caller
            rtfs_intent_source: "".to_string(),
            goal: intent_json.goal,
            constraints: intent_json.constraints.unwrap_or_default(),
            preferences: intent_json.preferences.unwrap_or_default(),
            success_criteria: intent_json.success_criteria,
            parent_intent: None,
            child_intents: vec![],
            triggered_by: TriggerSource::HumanRequest,
            generation_context: GenerationContext {
                arbiter_version: "openai-provider-1.0".to_string(),
                generation_timestamp: now,
                input_context: HashMap::new(),
                reasoning_trace: None,
            },
            status: IntentStatus::Active,
            priority: 0,
            created_at: now,
            updated_at: now,
            metadata: HashMap::new(),
        })
    }

    fn parse_plan_from_json(&self, json_str: &str, intent_id: &str) -> Result<Plan, RuntimeError> {
        // Try to extract JSON from the response
        let json_start = json_str.find('{').unwrap_or(0);
        let json_end = json_str.rfind('}').map(|i| i + 1).unwrap_or(json_str.len());
        let json_content = &json_str[json_start..json_end];

        #[derive(Deserialize)]
        struct PlanJson {
            name: Option<String>,
            steps: Vec<String>,
        }

        let plan_json: PlanJson = serde_json::from_str(json_content)
            .map_err(|e| RuntimeError::Generic(format!("Failed to parse plan JSON: {}", e)))?;

        let rtfs_body = format!("(do\n  {}\n)", plan_json.steps.join("\n  "));

        Ok(Plan {
            plan_id: format!("openai_plan_{}", uuid::Uuid::new_v4()),
            name: plan_json.name,
            intent_ids: vec![intent_id.to_string()],
            language: PlanLanguage::Rtfs20,
            body: PlanBody::Rtfs(rtfs_body),
            status: crate::ccos::types::PlanStatus::Draft,
            created_at: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_secs(),
            metadata: HashMap::new(),
            input_schema: None,
            output_schema: None,
            policies: HashMap::new(),
            capabilities_required: vec![],
            annotations: HashMap::new(),
        })
    }
}


#[async_trait]
impl LlmProvider for OpenAILlmProvider {
    async fn generate_intent(
        &self,
        prompt: &str,
        _context: Option<HashMap<String, String>>,
    ) -> Result<StorableIntent, RuntimeError> {
        // Load prompt from assets with fallback
        let vars = HashMap::from([
            ("user_request".to_string(), prompt.to_string()),
        ]);
        
        let system_message = self.prompt_manager
            .render("intent_generation", "v1", &vars)
            .unwrap_or_else(|e| {
                eprintln!("Warning: Failed to load intent_generation prompt from assets: {}. Using fallback.", e);
                r#"You are an AI assistant that converts natural language requests into structured intents for a cognitive computing system.

Generate a JSON response with the following structure:
{
  "name": "descriptive_name_for_intent",
  "goal": "clear_description_of_what_should_be_achieved",
  "constraints": {
    "constraint_name": "constraint_value_as_string"
  },
  "preferences": {
    "preference_name": "preference_value_as_string"
  },
  "success_criteria": "how_to_determine_if_intent_was_successful"
}

IMPORTANT: All values in constraints and preferences must be strings, not numbers or arrays.
Examples:
- "max_cost": "100" (not 100)
- "priority": "high" (not ["high"])
- "timeout": "30_seconds" (not 30)

Only respond with valid JSON."#.to_string()
            });

        let messages = vec![
            OpenAIMessage {
                role: "system".to_string(),
                content: system_message.to_string(),
            },
            OpenAIMessage {
                role: "user".to_string(),
                content: prompt.to_string(),
            },
        ];

        let response = self.make_request(messages).await?;
        let show_prompts = std::env::var("RTFS_SHOW_PROMPTS")
            .map(|v| v == "1")
            .unwrap_or(false)
            || std::env::var("CCOS_DEBUG")
                .map(|v| v == "1")
                .unwrap_or(false);
        if show_prompts {
            println!(
                "\n=== LLM Raw Response (Plan Generation) ===\n{}\n=== END RESPONSE ===\n",
                response
            );
        }
        self.parse_intent_from_json(&response)
    }

    async fn generate_plan(
        &self,
        intent: &StorableIntent,
        _context: Option<HashMap<String, String>>,
    ) -> Result<Plan, RuntimeError> {
        // Choose prompt mode: full-plan or reduced (do ...) body only
        let full_plan_mode = std::env::var("RTFS_FULL_PLAN")
            .map(|v| v == "1")
            .unwrap_or(false);

        // Prepare variables for prompt rendering
        let vars = HashMap::from([
            ("goal".to_string(), intent.goal.clone()),
            ("constraints".to_string(), format!("{:?}", intent.constraints)),
            ("preferences".to_string(), format!("{:?}", intent.preferences)),
        ]);

        // Load appropriate prompt based on mode
        let prompt_id = if full_plan_mode {
            "plan_generation_full"
        } else {
            "plan_generation_reduced"
        };

        let system_message = self.prompt_manager
            .render(prompt_id, "v1", &vars)
            .unwrap_or_else(|e| {
                eprintln!("Warning: Failed to load {} prompt from assets: {}. Using fallback.", prompt_id, e);
                // Fallback to original hard-coded prompts based on mode
                if full_plan_mode {
                    r#"You translate an RTFS intent into a concrete RTFS plan using a constrained schema.
Output format: ONLY a single well-formed RTFS s-expression starting with (plan ...). No prose, no JSON, no fences."#.to_string()
                } else {
                    r#"You translate an RTFS intent into a concrete RTFS execution body using a reduced grammar.
Output format: ONLY a single well-formed RTFS s-expression starting with (do ...). No prose, no JSON, no fences."#.to_string()
                }
            });

                let user_message = if full_plan_mode {
            format!(
                "Intent goal: {}\nConstraints: {:?}\nPreferences: {:?}\n\nGenerate the (plan ...) now, following the constraints above:",
                intent.goal, intent.constraints, intent.preferences
            )
        } else {
            format!(
                "Intent goal: {}\nConstraints: {:?}\nPreferences: {:?}\n\nGenerate the (do ...) body now:",
                intent.goal, intent.constraints, intent.preferences
            )
        };

        // Optional: display prompts during live runtime when enabled
        // Enable by setting RTFS_SHOW_PROMPTS=1 or CCOS_DEBUG=1
        let show_prompts = std::env::var("RTFS_SHOW_PROMPTS")
            .map(|v| v == "1")
            .unwrap_or(false)
            || std::env::var("CCOS_DEBUG")
                .map(|v| v == "1")
                .unwrap_or(false);
        if show_prompts {
            println!(
                "\n=== LLM Plan Generation Prompt ===\n[system]\n{}\n\n[user]\n{}\n=== END PROMPT ===\n",
                system_message,
                user_message
            );
        }

        let messages = vec![
            OpenAIMessage {
                role: "system".to_string(),
                content: system_message,
            },
            OpenAIMessage {
                role: "user".to_string(),
                content: user_message,
            },
        ];

        let response = self.make_request(messages).await?;
        if show_prompts {
            println!(
                "\n=== LLM Raw Response (Plan Generation) ===\n{}\n=== END RESPONSE ===\n",
                response
            );
        }
        // Preferred: try full-plan extraction first (if requested), then reduced (do ...) body
        if full_plan_mode {
            if let Some(plan_block) = Self::extract_plan_block(&response) {
                // Prefer extracting the (do ...) right after :body; fallback to generic do search
                if let Some(do_block) = Self::extract_s_expr_after_key(&plan_block, ":body")
                    .or_else(|| Self::extract_do_block(&plan_block))
                {
                    // If we extracted a do block from the plan, use it
                    // Parser validation is skipped because LLM may generate function calls
                    // that aren't yet defined in the parser's symbol table
                    let mut plan_name: Option<String> = None;
                    if let Some(name) =
                        Self::extract_quoted_value_after_key(&plan_block, ":name")
                    {
                        plan_name = Some(name);
                    }
                    return Ok(Plan {
                        plan_id: format!("openai_plan_{}", uuid::Uuid::new_v4()),
                        name: plan_name,
                        intent_ids: vec![intent.intent_id.clone()],
                        language: PlanLanguage::Rtfs20,
                        body: PlanBody::Rtfs(do_block),
                        status: crate::ccos::types::PlanStatus::Draft,
                        created_at: std::time::SystemTime::now()
                            .duration_since(std::time::UNIX_EPOCH)
                            .unwrap()
                            .as_secs(),
                        metadata: HashMap::new(),
                        input_schema: None,
                        output_schema: None,
                        policies: HashMap::new(),
                        capabilities_required: vec![],
                        annotations: HashMap::new(),
                    });
                }
            }
        }

        // Fallback: direct RTFS (do ...) body
        if let Some(do_block) = Self::extract_do_block(&response) {
            // If we successfully extracted a (do ...) block, use it
            // Parser validation is skipped because the LLM may generate function calls
            // that aren't yet defined in the parser's symbol table
            return Ok(Plan {
                plan_id: format!("openai_plan_{}", uuid::Uuid::new_v4()),
                name: None,
                intent_ids: vec![intent.intent_id.clone()],
                language: PlanLanguage::Rtfs20,
                body: PlanBody::Rtfs(do_block),
                status: crate::ccos::types::PlanStatus::Draft,
                created_at: std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap()
                    .as_secs(),
                metadata: HashMap::new(),
                input_schema: None,
                output_schema: None,
                policies: HashMap::new(),
                capabilities_required: vec![],
                annotations: HashMap::new(),
            });
        }

        // Fallback: previous JSON-wrapped steps contract
        self.parse_plan_from_json(&response, &intent.intent_id)
    }

    async fn generate_plan_with_retry(
        &self,
        intent: &StorableIntent,
        _context: Option<HashMap<String, String>>,
    ) -> Result<Plan, RuntimeError> {
        let mut last_error = None;
        let mut last_plan_text = None;
        
        for attempt in 1..=self.config.retry_config.max_retries {
            // Create prompt based on attempt
            let prompt = if attempt == 1 {
                // Initial prompt
                let system_message = r#"You translate an RTFS intent into a concrete RTFS execution body using a reduced grammar.

Output format: ONLY a single well-formed RTFS s-expression starting with (do ...). No prose, no JSON, no fences.

Allowed forms (reduced grammar):
- (do <step> <step> ...)
- (step "Descriptive Name" (<expr>)) ; name must be a double-quoted string
- (call :cap.namespace.op <args...>)   ; capability ids MUST be RTFS keywords starting with a colon
- (if <condition> <then> <else>)  ; conditional execution (use for binary yes/no)
- (match <value> <pattern1> <result1> <pattern2> <result2> ...)  ; pattern matching (use for multiple choices)
- (let [var1 expr1 var2 expr2] <body>)  ; local bindings
- (str <arg1> <arg2> ...)  ; string concatenation
- (= <arg1> <arg2>)  ; equality comparison

Arguments allowed:
- strings: "..."
- numbers: 1 2 3
- simple maps with keyword keys: {:key "value" :a 1 :b 2}
- lists: [1 2 3] or ["a" "b" "c"]

Available capabilities (use exact names with colons):
- :ccos.echo - print message to output
- :ccos.user.ask - prompt user for input, returns their response
- :ccos.math.add - add numbers
- :ccos.math.subtract - subtract numbers
- :ccos.math.multiply - multiply numbers
- :ccos.math.divide - divide numbers

CRITICAL: let bindings are LOCAL to a single step. You CANNOT use variables across step boundaries.

CORRECT - capture and reuse within single step:
  (step "Greet User"
    (let [name (call :ccos.user.ask "What is your name?")]
      (call :ccos.echo {:message (str "Hello, " name "!")})))

CORRECT - multiple prompts with summary in one step:
  (step "Survey"
    (let [name (call :ccos.user.ask "What is your name?")
          age (call :ccos.user.ask "How old are you?")
          hobby (call :ccos.user.ask "What is your hobby?")]
      (call :ccos.echo {:message (str "Summary: " name ", age " age ", enjoys " hobby)})))

WRONG - let has no body:
  (step "Bad" (let [name (call :ccos.user.ask "Name?")])  ; Missing body expression!

WRONG - variables out of scope across steps:
  (step "Get" (let [n (call :ccos.user.ask "Name?")] n))
  (step "Use" (call :ccos.echo {:message n}))  ; n not in scope here!

Conditional branching (CORRECT - if for yes/no):
  (step "Pizza Check" 
    (let [likes (call :ccos.user.ask "Do you like pizza? (yes/no)")]
      (if (= likes "yes")
        (call :ccos.echo {:message "Great! Pizza is delicious!"})
        (call :ccos.echo {:message "Maybe try it sometime!"}))))

Multiple choice (CORRECT - match for many options):
  (step "Language Hello World" 
    (let [lang (call :ccos.user.ask "Choose: rust, python, or javascript")]
      (match lang
        "rust" (call :ccos.echo {:message "println!(\"Hello\")"})
        "python" (call :ccos.echo {:message "print('Hello')"})
        "javascript" (call :ccos.echo {:message "console.log('Hello')"})
        _ (call :ccos.echo {:message "Unknown language"}))))

Return exactly one (plan ...) with these constraints.
"#;
                let user_message = format!(
                    "Intent goal: {}\nConstraints: {:?}\nPreferences: {:?}\n\nGenerate the (do ...) body now:",
                    intent.goal, intent.constraints, intent.preferences
                );
                vec![
                    OpenAIMessage {
                        role: "system".to_string(),
                        content: system_message.to_string(),
                    },
                    OpenAIMessage {
                        role: "user".to_string(),
                        content: user_message,
                    },
                ]
            } else if self.config.retry_config.send_error_feedback {
                // Retry prompt with error feedback
                let system_message = if attempt == self.config.retry_config.max_retries && self.config.retry_config.simplify_on_final_attempt {
                    r#"You translate an RTFS intent into a concrete RTFS execution body using a SIMPLIFIED grammar.

This is your final attempt. Keep it simple and basic.

Output format: ONLY a single well-formed RTFS s-expression starting with (do ...). No prose, no JSON, no fences.

SIMPLIFIED forms only:
- (do <step> <step> ...)
- (step "Name" (call :cap.op <args>))
- (call :ccos.echo {:message "text"})
- (call :ccos.user.ask "question")

Available capabilities:
- :ccos.echo - print message
- :ccos.user.ask - ask user question

Keep it simple. No complex logic, no let bindings, no conditionals.
"#
                } else {
                    r#"You translate an RTFS intent into a concrete RTFS execution body using a reduced grammar.

The previous attempt failed. Please fix the error and try again.

Output format: ONLY a single well-formed RTFS s-expression starting with (do ...). No prose, no JSON, no fences.

Allowed forms (reduced grammar):
- (do <step> <step> ...)
- (step "Descriptive Name" (<expr>)) ; name must be a double-quoted string
- (call :cap.namespace.op <args...>)   ; capability ids MUST be RTFS keywords starting with a colon
- (if <condition> <then> <else>)  ; conditional execution (use for binary yes/no)
- (match <value> <pattern1> <result1> <pattern2> <result2> ...)  ; pattern matching (use for multiple choices)
- (let [var1 expr1 var2 expr2] <body>)  ; local bindings
- (str <arg1> <arg2> ...)  ; string concatenation
- (= <arg1> <arg2>)  ; equality comparison

Arguments allowed:
- strings: "..."
- numbers: 1 2 3
- simple maps with keyword keys: {:key "value" :a 1 :b 2}
- lists: [1 2 3] or ["a" "b" "c"]

Available capabilities (use exact names with colons):
- :ccos.echo - print message to output
- :ccos.user.ask - prompt user for input, returns their response
- :ccos.math.add - add numbers
- :ccos.math.subtract - subtract numbers
- :ccos.math.multiply - multiply numbers
- :ccos.math.divide - divide numbers

CRITICAL: let bindings are LOCAL to a single step. You CANNOT use variables across step boundaries.

CORRECT - capture and reuse within single step:
  (step "Greet User"
    (let [name (call :ccos.user.ask "What is your name?")]
      (call :ccos.echo {:message (str "Hello, " name "!")})))

CORRECT - multiple prompts with summary in one step:
  (step "Survey"
    (let [name (call :ccos.user.ask "What is your name?")
          age (call :ccos.user.ask "How old are you?")
          hobby (call :ccos.user.ask "What is your hobby?")]
      (call :ccos.echo {:message (str "Summary: " name ", age " age ", enjoys " hobby)})))

WRONG - let has no body:
  (step "Bad" (let [name (call :ccos.user.ask "Name?")])  ; Missing body expression!

WRONG - variables out of scope across steps:
  (step "Get" (let [n (call :ccos.user.ask "Name?")] n))
  (step "Use" (call :ccos.echo {:message n}))  ; n not in scope here!

Return exactly one (plan ...) with these constraints.
"#
                };
                let user_message = format!(
                    "Intent goal: {}\nConstraints: {:?}\nPreferences: {:?}\n\nPrevious attempt that failed:\n{}\n\nError: {}\n\nPlease generate a corrected (do ...) body:",
                    intent.goal, intent.constraints, intent.preferences, last_plan_text.as_ref().unwrap(), last_error.as_ref().unwrap()
                );
                vec![
                    OpenAIMessage {
                        role: "system".to_string(),
                        content: system_message.to_string(),
                    },
                    OpenAIMessage {
                        role: "user".to_string(),
                        content: user_message,
                    },
                ]
            } else {
                // Simple retry without feedback
                let system_message = r#"You translate an RTFS intent into a concrete RTFS execution body using a reduced grammar.

Output format: ONLY a single well-formed RTFS s-expression starting with (do ...). No prose, no JSON, no fences.

Allowed forms (reduced grammar):
- (do <step> <step> ...)
- (step "Descriptive Name" (<expr>)) ; name must be a double-quoted string
- (call :cap.namespace.op <args...>)   ; capability ids MUST be RTFS keywords starting with a colon
- (if <condition> <then> <else>)  ; conditional execution (use for binary yes/no)
- (match <value> <pattern1> <result1> <pattern2> <result2> ...)  ; pattern matching (use for multiple choices)
- (let [var1 expr1 var2 expr2] <body>)  ; local bindings
- (str <arg1> <arg2> ...)  ; string concatenation
- (= <arg1> <arg2>)  ; equality comparison

Arguments allowed:
- strings: "..."
- numbers: 1 2 3
- simple maps with keyword keys: {:key "value" :a 1 :b 2}
- lists: [1 2 3] or ["a" "b" "c"]

Available capabilities (use exact names with colons):
- :ccos.echo - print message to output
- :ccos.user.ask - prompt user for input, returns their response
- :ccos.math.add - add numbers
- :ccos.math.subtract - subtract numbers
- :ccos.math.multiply - multiply numbers
- :ccos.math.divide - divide numbers

CRITICAL: let bindings are LOCAL to a single step. You CANNOT use variables across step boundaries.

CORRECT - capture and reuse within single step:
  (step "Greet User"
    (let [name (call :ccos.user.ask "What is your name?")]
      (call :ccos.echo {:message (str "Hello, " name "!")})))

CORRECT - multiple prompts with summary in one step:
  (step "Survey"
    (let [name (call :ccos.user.ask "What is your name?")
          age (call :ccos.user.ask "How old are you?")
          hobby (call :ccos.user.ask "What is your hobby?")]
      (call :ccos.echo {:message (str "Summary: " name ", age " age ", enjoys " hobby)})))

WRONG - let has no body:
  (step "Bad" (let [name (call :ccos.user.ask "Name?")])  ; Missing body expression!

WRONG - variables out of scope across steps:
  (step "Get" (let [n (call :ccos.user.ask "Name?")] n))
  (step "Use" (call :ccos.echo {:message n}))  ; n not in scope here!

Conditional branching (CORRECT - if for yes/no):
  (step "Pizza Check" 
    (let [likes (call :ccos.user.ask "Do you like pizza? (yes/no)")]
      (if (= likes "yes")
        (call :ccos.echo {:message "Great! Pizza is delicious!"})
        (call :ccos.echo {:message "Maybe try it sometime!"}))))

Multiple choice (CORRECT - match for many options):
  (step "Language Hello World" 
    (let [lang (call :ccos.user.ask "Choose: rust, python, or javascript")]
      (match lang
        "rust" (call :ccos.echo {:message "println!(\"Hello\")"})
        "python" (call :ccos.echo {:message "print('Hello')"})
        "javascript" (call :ccos.echo {:message "console.log('Hello')"})
        _ (call :ccos.echo {:message "Unknown language"}))))

Return exactly one (plan ...) with these constraints.
"#;
                let user_message = format!(
                    "Intent goal: {}\nConstraints: {:?}\nPreferences: {:?}\n\nGenerate the (do ...) body now:",
                    intent.goal, intent.constraints, intent.preferences
                );
                vec![
                    OpenAIMessage {
                        role: "system".to_string(),
                        content: system_message.to_string(),
                    },
                    OpenAIMessage {
                        role: "user".to_string(),
                        content: user_message,
                    },
                ]
            };
            
            let response = self.make_request(prompt).await?;
            
            // Validate and parse the plan
            let plan_result = if let Some(do_block) = OpenAILlmProvider::extract_do_block(&response) {
                if parser::parse(&do_block).is_ok() {
                    Ok(Plan {
                        plan_id: format!("openai_plan_{}", uuid::Uuid::new_v4()),
                        name: None,
                        intent_ids: vec![intent.intent_id.clone()],
                        language: PlanLanguage::Rtfs20,
                        body: PlanBody::Rtfs(do_block.to_string()),
                        status: crate::ccos::types::PlanStatus::Draft,
                        created_at: std::time::SystemTime::now()
                            .duration_since(std::time::UNIX_EPOCH)
                            .unwrap()
                            .as_secs(),
                        metadata: HashMap::new(),
                        input_schema: None,
                        output_schema: None,
                        policies: HashMap::new(),
                        capabilities_required: vec![],
                        annotations: HashMap::new(),
                    })
                } else {
                    Err(RuntimeError::Generic(format!("Failed to parse RTFS plan: {}", do_block)))
                }
            } else {
                // Fallback to JSON parsing
                self.parse_plan_from_json(&response, &intent.intent_id)
            };
            
            match plan_result {
                Ok(plan) => {
                    // Record successful attempt
                    self.metrics.record_success(attempt);
                    if attempt > 1 {
                        log::info!("✅ Plan retry succeeded on attempt {}", attempt);
                    }
                    return Ok(plan);
                }
                Err(e) => {
                    // Record failed attempt
                    self.metrics.record_failure(attempt);
                    
                    // Create detailed error message for logging
                    let error_context = if attempt == 1 {
                        format!("Initial attempt failed: {}", e)
                    } else {
                        format!("Retry attempt {}/{} failed: {}", attempt, self.config.retry_config.max_retries, e)
                    };
                    
                    log::warn!("❌ {}", error_context);
                    
                    // Store enhanced error message for final error reporting
                    let enhanced_error = format!(
                        "Attempt {}: {} (Response: {})",
                        attempt,
                        e,
                        if response.len() > 200 {
                            format!("{}...", &response[..200])
                        } else {
                            response.clone()
                        }
                    );
                    last_error = Some(enhanced_error);
                    last_plan_text = Some(response.clone());
                    
                    if attempt < self.config.retry_config.max_retries {
                        continue; // Retry
                    }
                }
            }
        }
        
        // All retries exhausted
        if self.config.retry_config.use_stub_fallback {
            log::warn!("⚠️  Using stub fallback after {} failed attempts", self.config.retry_config.max_retries);
            // Record stub fallback as a success (since we're providing a working plan)
            self.metrics.record_success(self.config.retry_config.max_retries + 1);
            let stub_body = format!(
                r#"(do
    (step "Echo Intent" (call :ccos.echo {{:message "Intent: {}"}}))
    (step "Ask User" (call :ccos.user.ask "Please provide more details about your request"))
    (step "Echo Response" (call :ccos.echo {{:message "Thank you for your input"}}))
)"#,
                intent.goal
            );
            return Ok(Plan {
                plan_id: format!("stub_plan_{}", uuid::Uuid::new_v4()),
                name: Some("Stub Plan".to_string()),
                intent_ids: vec![intent.intent_id.clone()],
                language: PlanLanguage::Rtfs20,
                body: PlanBody::Rtfs(stub_body),
                status: crate::ccos::types::PlanStatus::Draft,
                created_at: std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap()
                    .as_secs(),
                metadata: HashMap::new(),
                input_schema: None,
                output_schema: None,
                policies: HashMap::new(),
                capabilities_required: vec![],
                annotations: HashMap::new(),
            });
        }
        
        // Record final failure (all retries exhausted, no stub fallback)
        self.metrics.record_failure(self.config.retry_config.max_retries);
        
        // Create detailed error message with helpful suggestions
        let detailed_error = format!(
            "❌ Plan generation failed after {} attempts.\n\n\
            🔍 **What went wrong:**\n\
            The LLM was unable to generate a valid RTFS plan for your request: \"{}\"\n\
            Last error: {}\n\n\
            💡 **Suggestions to try:**\n\
            1. **Simplify your request** - Break complex tasks into smaller, simpler steps\n\
            2. **Use clearer language** - Be more specific about what you want to accomplish\n\
            3. **Try basic patterns** - Start with simple tasks like:\n\
               - \"Echo a message\"\n\
               - \"Ask the user for their name\"\n\
               - \"Add two numbers together\"\n\n\
            📚 **Working examples:**\n\
            - \"Greet the user and ask for their name\"\n\
            - \"Ask the user if they like pizza and respond accordingly\"\n\
            - \"Ask the user to choose between options and show the result\"\n\n\
            🔧 **Technical details:**\n\
            - Total attempts: {}\n\
            - Retry configuration: max_retries={}, feedback={}, stub_fallback={}\n\
            - Intent constraints: {:?}\n\
            - Intent preferences: {:?}",
            self.config.retry_config.max_retries,
            intent.goal,
            last_error.unwrap_or_else(|| "Unknown error".to_string()),
            self.config.retry_config.max_retries,
            self.config.retry_config.max_retries,
            self.config.retry_config.send_error_feedback,
            self.config.retry_config.use_stub_fallback,
            intent.constraints,
            intent.preferences
        );
        
        Err(RuntimeError::Generic(detailed_error))
    }


    async fn validate_plan(&self, plan_content: &str) -> Result<ValidationResult, RuntimeError> {
        let system_message = r#"You are an AI assistant that validates RTFS plans.

Analyze the plan and respond with JSON:
{
  "is_valid": true/false,
  "confidence": 0.0-1.0,
  "reasoning": "explanation",
  "suggestions": ["suggestion1", "suggestion2"],
  "errors": ["error1", "error2"]
}

Check for:
- Valid RTFS syntax
- Appropriate step usage
- Logical flow
- Error handling

Only respond with valid JSON."#;

        let user_message = format!("Validate this RTFS plan:\n{}", plan_content);

        let messages = vec![
            OpenAIMessage {
                role: "system".to_string(),
                content: system_message.to_string(),
            },
            OpenAIMessage {
                role: "user".to_string(),
                content: user_message,
            },
        ];

        let response = self.make_request(messages).await?;

        // Parse validation result
        let json_start = response.find('{').unwrap_or(0);
        let json_end = response.rfind('}').map(|i| i + 1).unwrap_or(response.len());
        let json_content = &response[json_start..json_end];

        #[derive(Deserialize)]
        struct ValidationJson {
            is_valid: bool,
            confidence: f64,
            reasoning: String,
            suggestions: Vec<String>,
            errors: Vec<String>,
        }

        let validation: ValidationJson = serde_json::from_str(json_content).map_err(|e| {
            RuntimeError::Generic(format!("Failed to parse validation JSON: {}", e))
        })?;

        Ok(ValidationResult {
            is_valid: validation.is_valid,
            confidence: validation.confidence,
            reasoning: validation.reasoning,
            suggestions: validation.suggestions,
            errors: validation.errors,
        })
    }

    async fn generate_text(&self, prompt: &str) -> Result<String, RuntimeError> {
        let messages = vec![OpenAIMessage {
            role: "user".to_string(),
            content: prompt.to_string(),
        }];

        self.make_request(messages).await
    }

    fn get_info(&self) -> LlmProviderInfo {
        LlmProviderInfo {
            name: "OpenAI LLM Provider".to_string(),
            version: "1.0.0".to_string(),
            model: self.config.model.clone(),
            capabilities: vec![
                "intent_generation".to_string(),
                "plan_generation".to_string(),
                "plan_validation".to_string(),
            ],
        }
    }
}

// OpenAI API types
#[derive(Serialize)]
struct OpenAIRequest {
    model: String,
    messages: Vec<OpenAIMessage>,
    max_tokens: Option<u32>,
    temperature: Option<f64>,
}

#[derive(Serialize, Deserialize)]
struct OpenAIMessage {
    role: String,
    content: String,
}

#[derive(Deserialize)]
struct OpenAIResponse {
    choices: Vec<OpenAIChoice>,
}

#[derive(Deserialize)]
struct OpenAIChoice {
    message: OpenAIMessage,
}

/// Anthropic Claude provider
pub struct AnthropicLlmProvider {
    config: LlmProviderConfig,
    client: reqwest::Client,
    metrics: RetryMetrics,
    prompt_manager: PromptManager<FilePromptStore>,
}

impl AnthropicLlmProvider {
    pub fn new(config: LlmProviderConfig) -> Result<Self, RuntimeError> {
        let client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(
                config.timeout_seconds.unwrap_or(30),
            ))
            .build()
            .map_err(|e| RuntimeError::Generic(format!("Failed to create HTTP client: {}", e)))?;

        // Assets are at workspace root, so try ../assets first, then assets (for when run from workspace root)
        let prompt_path = if std::path::Path::new("../assets/prompts/arbiter").exists() {
            "../assets/prompts/arbiter"
        } else {
            "assets/prompts/arbiter"
        };
        let prompt_store = FilePromptStore::new(prompt_path);
        let prompt_manager = PromptManager::new(prompt_store);

        Ok(Self { 
            config, 
            client,
            metrics: RetryMetrics::new(),
            prompt_manager,
        })
    }

    /// Get current retry metrics summary
    pub fn get_retry_metrics(&self) -> RetryMetricsSummary {
        self.metrics.get_summary()
    }

    async fn make_request(&self, messages: Vec<AnthropicMessage>) -> Result<String, RuntimeError> {
        let api_key = self.config.api_key.as_ref().ok_or_else(|| {
            RuntimeError::Generic("API key required for Anthropic provider".to_string())
        })?;

        let base_url = self
            .config
            .base_url
            .as_deref()
            .unwrap_or("https://api.anthropic.com/v1");
        let url = format!("{}/messages", base_url);

        let request_body = AnthropicRequest {
            model: self.config.model.clone(),
            messages,
            max_tokens: self.config.max_tokens,
            temperature: self.config.temperature,
        };

        let response = self
            .client
            .post(&url)
            .header("x-api-key", api_key)
            .header("anthropic-version", "2023-06-01")
            .header("Content-Type", "application/json")
            .json(&request_body)
            .send()
            .await
            .map_err(|e| RuntimeError::Generic(format!("HTTP request failed: {}", e)))?;

        if !response.status().is_success() {
            let error_text = response
                .text()
                .await
                .unwrap_or_else(|_| "Unknown error".to_string());
            return Err(RuntimeError::Generic(format!(
                "API request failed: {}",
                error_text
            )));
        }

        let response_body: AnthropicResponse = response
            .json()
            .await
            .map_err(|e| RuntimeError::Generic(format!("Failed to parse response: {}", e)))?;

        Ok(response_body.content[0].text.clone())
    }

    fn parse_intent_from_json(&self, json_str: &str) -> Result<StorableIntent, RuntimeError> {
        // Try to extract JSON from the response (it might be wrapped in markdown)
        let json_start = json_str.find('{').unwrap_or(0);
        let json_end = json_str.rfind('}').map(|i| i + 1).unwrap_or(json_str.len());
        let json_content = &json_str[json_start..json_end];

        #[derive(Deserialize)]
        struct IntentJson {
            name: Option<String>,
            goal: String,
            constraints: Option<HashMap<String, String>>,
            preferences: Option<HashMap<String, String>>,
            success_criteria: Option<String>,
        }

        let intent_json: IntentJson = serde_json::from_str(json_content)
            .map_err(|e| RuntimeError::Generic(format!("Failed to parse intent JSON: {}", e)))?;

        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs();

        Ok(StorableIntent {
            intent_id: format!("anthropic_intent_{}", uuid::Uuid::new_v4()),
            name: intent_json.name,
            original_request: "".to_string(), // Will be set by caller
            rtfs_intent_source: "".to_string(),
            goal: intent_json.goal,
            constraints: intent_json.constraints.unwrap_or_default(),
            preferences: intent_json.preferences.unwrap_or_default(),
            success_criteria: intent_json.success_criteria,
            parent_intent: None,
            child_intents: vec![],
            triggered_by: TriggerSource::HumanRequest,
            generation_context: GenerationContext {
                arbiter_version: "anthropic-provider-1.0".to_string(),
                generation_timestamp: now,
                input_context: HashMap::new(),
                reasoning_trace: None,
            },
            status: IntentStatus::Active,
            priority: 0,
            created_at: now,
            updated_at: now,
            metadata: HashMap::new(),
        })
    }

    fn parse_plan_from_json(&self, json_str: &str, intent_id: &str) -> Result<Plan, RuntimeError> {
        // Try to extract JSON from the response
        let json_start = json_str.find('{').unwrap_or(0);
        let json_end = json_str.rfind('}').map(|i| i + 1).unwrap_or(json_str.len());
        let json_content = &json_str[json_start..json_end];

        #[derive(Deserialize)]
        struct PlanJson {
            name: Option<String>,
            steps: Vec<String>,
        }

        let plan_json: PlanJson = serde_json::from_str(json_content)
            .map_err(|e| RuntimeError::Generic(format!("Failed to parse plan JSON: {}", e)))?;

        let rtfs_body = format!("(do\n  {}\n)", plan_json.steps.join("\n  "));

        Ok(Plan {
            plan_id: format!("anthropic_plan_{}", uuid::Uuid::new_v4()),
            name: plan_json.name,
            intent_ids: vec![intent_id.to_string()],
            language: PlanLanguage::Rtfs20,
            body: PlanBody::Rtfs(rtfs_body),
            status: crate::ccos::types::PlanStatus::Draft,
            created_at: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_secs(),
            metadata: HashMap::new(),
            input_schema: None,
            output_schema: None,
            policies: HashMap::new(),
            capabilities_required: vec![],
            annotations: HashMap::new(),
        })
    }
}

#[async_trait]
impl LlmProvider for AnthropicLlmProvider {
    async fn generate_intent(
        &self,
        prompt: &str,
        context: Option<HashMap<String, String>>,
    ) -> Result<StorableIntent, RuntimeError> {
        // Load prompt from assets with fallback
        let vars = HashMap::from([
            ("user_request".to_string(), prompt.to_string()),
        ]);
        
        let system_message = self.prompt_manager
            .render("intent_generation", "v1", &vars)
            .unwrap_or_else(|e| {
                eprintln!("Warning: Failed to load intent_generation prompt from assets: {}. Using fallback.", e);
                r#"You are an AI assistant that converts natural language requests into structured intents for a cognitive computing system.

Generate a JSON response with the following structure:
{
  "name": "descriptive_name_for_intent",
  "goal": "clear_description_of_what_should_be_achieved",
  "constraints": {
    "constraint_name": "constraint_value_as_string"
  },
  "preferences": {
    "preference_name": "preference_value_as_string"
  },
  "success_criteria": "how_to_determine_if_intent_was_successful"
}

IMPORTANT: All values in constraints and preferences must be strings, not numbers or arrays.
Examples:
- "max_cost": "100" (not 100)
- "priority": "high" (not ["high"])
- "timeout": "30_seconds" (not 30)

Only respond with valid JSON."#.to_string()
            });

        let user_message = if let Some(ctx) = context {
            let context_str = ctx
                .iter()
                .map(|(k, v)| format!("{}: {}", k, v))
                .collect::<Vec<_>>()
                .join("\n");
            format!("Context:\n{}\n\nRequest: {}", context_str, prompt)
        } else {
            prompt.to_string()
        };

        let messages = vec![AnthropicMessage {
            role: "user".to_string(),
            content: format!("{}\n\n{}", system_message, user_message),
        }];

        let response = self.make_request(messages).await?;
        let mut intent = self.parse_intent_from_json(&response)?;
        intent.original_request = prompt.to_string();

        Ok(intent)
    }

    async fn generate_plan(
        &self,
        intent: &StorableIntent,
        _context: Option<HashMap<String, String>>,
    ) -> Result<Plan, RuntimeError> {
        let system_message = r#"You are an AI assistant that generates executable plans from structured intents.

Generate a JSON response with the following structure:
{
  "name": "descriptive_plan_name",
  "steps": [
    "step 1 description",
    "step 2 description",
    "step 3 description"
  ]
}

Each step should be a clear, actionable instruction that can be executed by the system.
Only respond with valid JSON."#;

        let user_message = format!(
            "Intent: {}\nGoal: {}\nConstraints: {:?}\nPreferences: {:?}\nSuccess Criteria: {:?}",
            intent.name.as_deref().unwrap_or("unnamed"),
            intent.goal,
            intent.constraints,
            intent.preferences,
            intent.success_criteria.as_deref().unwrap_or("none")
        );

        let messages = vec![AnthropicMessage {
            role: "user".to_string(),
            content: format!("{}\n\n{}", system_message, user_message),
        }];

        let response = self.make_request(messages).await?;
        self.parse_plan_from_json(&response, &intent.intent_id)
    }

    async fn validate_plan(&self, plan_content: &str) -> Result<ValidationResult, RuntimeError> {
        let system_message = r#"You are an AI assistant that validates executable plans.

Analyze the provided plan and return a JSON response with the following structure:
{
  "is_valid": true/false,
  "confidence": 0.0-1.0,
  "reasoning": "explanation of validation decision",
  "suggestions": ["suggestion1", "suggestion2"],
  "errors": ["error1", "error2"]
}

Only respond with valid JSON."#;

        let user_message = format!("Plan to validate:\n{}", plan_content);

        let messages = vec![AnthropicMessage {
            role: "user".to_string(),
            content: format!("{}\n\n{}", system_message, user_message),
        }];

        let response = self.make_request(messages).await?;

        // Try to extract JSON from the response
        let json_start = response.find('{').unwrap_or(0);
        let json_end = response.rfind('}').map(|i| i + 1).unwrap_or(response.len());
        let json_content = &response[json_start..json_end];

        serde_json::from_str(json_content)
            .map_err(|e| RuntimeError::Generic(format!("Failed to parse validation JSON: {}", e)))
    }

    async fn generate_text(&self, prompt: &str) -> Result<String, RuntimeError> {
        let messages = vec![AnthropicMessage {
            role: "user".to_string(),
            content: prompt.to_string(),
        }];

        self.make_request(messages).await
    }

    fn get_info(&self) -> LlmProviderInfo {
        LlmProviderInfo {
            name: "Anthropic Claude".to_string(),
            version: "1.0".to_string(),
            model: self.config.model.clone(),
            capabilities: vec![
                "intent_generation".to_string(),
                "plan_generation".to_string(),
                "plan_validation".to_string(),
            ],
        }
    }
}

// Anthropic API types
#[derive(Serialize)]
struct AnthropicRequest {
    model: String,
    messages: Vec<AnthropicMessage>,
    max_tokens: Option<u32>,
    temperature: Option<f64>,
}

#[derive(Serialize)]
struct AnthropicMessage {
    role: String,
    content: String,
}

#[derive(Deserialize)]
struct AnthropicResponse {
    content: Vec<AnthropicContent>,
}

#[derive(Deserialize)]
struct AnthropicContent {
    text: String,
}

/// Stub LLM provider for testing and development
pub struct StubLlmProvider {
    config: LlmProviderConfig,
}

impl StubLlmProvider {
    pub fn new(config: LlmProviderConfig) -> Self {
        Self { config }
    }

    /// Generate a deterministic storable intent based on natural language
    fn generate_stub_intent(&self, nl: &str) -> StorableIntent {
        let lower_nl = nl.to_lowercase();
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs();

        if lower_nl.contains("sentiment") || lower_nl.contains("analyze") {
            StorableIntent {
                intent_id: format!("stub_sentiment_{}", uuid::Uuid::new_v4()),
                name: Some("analyze_user_sentiment".to_string()),
                original_request: nl.to_string(),
                rtfs_intent_source: "".to_string(),
                goal: "Analyze user sentiment from interactions".to_string(),
                constraints: HashMap::from([("accuracy".to_string(), "\"high\"".to_string())]),
                preferences: HashMap::from([("speed".to_string(), "\"medium\"".to_string())]),
                success_criteria: Some("\"sentiment_analyzed\"".to_string()),
                parent_intent: None,
                child_intents: vec![],
                triggered_by: TriggerSource::HumanRequest,
                generation_context: GenerationContext {
                    arbiter_version: "stub-1.0".to_string(),
                    generation_timestamp: now,
                    input_context: HashMap::new(),
                    reasoning_trace: None,
                },
                status: IntentStatus::Active,
                priority: 0,
                created_at: now,
                updated_at: now,
                metadata: HashMap::new(),
            }
        } else if lower_nl.contains("optimize") || lower_nl.contains("improve") {
            StorableIntent {
                intent_id: format!("stub_optimize_{}", uuid::Uuid::new_v4()),
                name: Some("optimize_system_performance".to_string()),
                original_request: nl.to_string(),
                rtfs_intent_source: "".to_string(),
                goal: "Optimize system performance".to_string(),
                constraints: HashMap::from([("budget".to_string(), "\"low\"".to_string())]),
                preferences: HashMap::from([("speed".to_string(), "\"high\"".to_string())]),
                success_criteria: Some("\"performance_optimized\"".to_string()),
                parent_intent: None,
                child_intents: vec![],
                triggered_by: TriggerSource::HumanRequest,
                generation_context: GenerationContext {
                    arbiter_version: "stub-1.0".to_string(),
                    generation_timestamp: now,
                    input_context: HashMap::new(),
                    reasoning_trace: None,
                },
                status: IntentStatus::Active,
                priority: 0,
                created_at: now,
                updated_at: now,
                metadata: HashMap::new(),
            }
        } else {
            // Default intent
            StorableIntent {
                intent_id: format!("stub_general_{}", uuid::Uuid::new_v4()),
                name: Some("general_assistance".to_string()),
                original_request: nl.to_string(),
                rtfs_intent_source: "".to_string(),
                goal: "Perform a small delegated task".to_string(),
                constraints: HashMap::new(),
                preferences: HashMap::from([("helpfulness".to_string(), "\"high\"".to_string())]),
                success_criteria: Some("\"assistance_provided\"".to_string()),
                parent_intent: None,
                child_intents: vec![],
                triggered_by: TriggerSource::HumanRequest,
                generation_context: GenerationContext {
                    arbiter_version: "stub-1.0".to_string(),
                    generation_timestamp: now,
                    input_context: HashMap::new(),
                    reasoning_trace: None,
                },
                status: IntentStatus::Active,
                priority: 0,
                created_at: now,
                updated_at: now,
                metadata: HashMap::new(),
            }
        }
    }

    /// Generate a deterministic plan based on intent
    fn generate_stub_plan(&self, intent: &StorableIntent) -> Plan {
        let plan_body = match intent.name.as_deref() {
            Some("analyze_user_sentiment") => {
                r#"
(do
    (step "Fetch User Data" (call :ccos.echo "fetched user interactions"))
    (step "Analyze Sentiment" (call :ccos.echo "sentiment analysis completed"))
    (step "Generate Report" (call :ccos.echo "sentiment report generated"))
)
"#
            }
            Some("optimize_system_performance") => {
                r#"
(do
    (step "Collect Metrics" (call :ccos.echo "system metrics collected"))
    (step "Identify Bottlenecks" (call :ccos.echo "bottlenecks identified"))
    (step "Apply Optimizations" (call :ccos.echo "optimizations applied"))
    (step "Verify Improvements" (call :ccos.echo "performance improvements verified"))
)
"#
            }
            _ => {
                r#"
(do
    (step "Process Request" (call :ccos.echo "processing your request"))
    (step "Complete Task" (call :ccos.echo "stub done"))
)
"#
            }
        };

        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs();

        Plan {
            plan_id: format!("stub_plan_{}", uuid::Uuid::new_v4()),
            name: Some(format!(
                "stub_plan_for_{}",
                intent.name.as_deref().unwrap_or("general")
            )),
            intent_ids: vec![intent.intent_id.clone()],
            language: PlanLanguage::Rtfs20,
            body: PlanBody::Rtfs(plan_body.trim().to_string()),
            status: crate::ccos::types::PlanStatus::Draft,
            created_at: now,
            metadata: HashMap::new(),
            input_schema: None,
            output_schema: None,
            policies: HashMap::new(),
            capabilities_required: vec!["ccos.echo".to_string()],
            annotations: HashMap::new(),
        }
    }
}

#[async_trait]
impl LlmProvider for StubLlmProvider {
    async fn generate_intent(
        &self,
        prompt: &str,
        _context: Option<HashMap<String, String>>,
    ) -> Result<StorableIntent, RuntimeError> {
        // For stub provider, we'll use a simple pattern matching approach
        // In a real implementation, this would parse the prompt and context
        let intent = self.generate_stub_intent(prompt);
        Ok(intent)
    }

    async fn generate_plan(
        &self,
        intent: &StorableIntent,
        _context: Option<HashMap<String, String>>,
    ) -> Result<Plan, RuntimeError> {
        let plan = self.generate_stub_plan(intent);
        Ok(plan)
    }

    async fn validate_plan(&self, _plan_content: &str) -> Result<ValidationResult, RuntimeError> {
        // Stub validation - always returns valid
        Ok(ValidationResult {
            is_valid: true,
            confidence: 0.95,
            reasoning: "Stub provider validation - always valid".to_string(),
            suggestions: vec!["Consider adding more specific steps".to_string()],
            errors: vec![],
        })
    }

    async fn generate_text(&self, prompt: &str) -> Result<String, RuntimeError> {
        // Check if this is a delegation analysis prompt
        let lower_prompt = prompt.to_lowercase();
        // Shortcut: detect arbiter graph-generation marker and return RTFS (do ...) intent graph
        if lower_prompt.contains("generate_intent_graph") || lower_prompt.contains("intent graph") {
            return Ok(r#"(do
  {:type "intent" :name "root" :goal "Say hi and add numbers"}
  {:type "intent" :name "greet" :goal "Greet the user"}
  {:type "intent" :name "compute" :goal "Add two numbers"}
  (edge :IsSubgoalOf "greet" "root")
  (edge :IsSubgoalOf "compute" "root")
  (edge :DependsOn "compute" "greet")
)"#
            .to_string());
        }

        if lower_prompt.contains("delegation analysis") || lower_prompt.contains("should_delegate")
        {
            // This is a delegation analysis request - return JSON
            if lower_prompt.contains("sentiment") || lower_prompt.contains("analyze") {
                Ok(r#"{
  "should_delegate": true,
  "reasoning": "Sentiment analysis requires specialized NLP capabilities available in sentiment_agent",
  "required_capabilities": ["sentiment_analysis", "text_processing"],
  "delegation_confidence": 0.92
}"#.to_string())
            } else if lower_prompt.contains("optimize") || lower_prompt.contains("performance") {
                Ok(r#"{
  "should_delegate": true,
  "reasoning": "Performance optimization requires specialized capabilities available in optimization_agent",
  "required_capabilities": ["performance_optimization", "system_analysis"],
  "delegation_confidence": 0.88
}"#.to_string())
            } else if lower_prompt.contains("backup") || lower_prompt.contains("database") {
                Ok(r#"{
  "should_delegate": true,
  "reasoning": "Database backup requires specialized backup and encryption capabilities available in backup_agent",
  "required_capabilities": ["backup", "encryption"],
  "delegation_confidence": 0.95
}"#.to_string())
            } else {
                // Default delegation analysis response
                Ok(r#"{
  "should_delegate": false,
  "reasoning": "Task can be handled directly without specialized agent delegation",
  "required_capabilities": ["general_processing"],
  "delegation_confidence": 0.75
}"#
                .to_string())
            }
        } else {
            // Regular intent generation - returns RTFS intent
            if lower_prompt.contains("sentiment") || lower_prompt.contains("analyze") {
                Ok(r#"(intent "analyze_user_sentiment"
  :goal "Analyze user sentiment from interactions and provide insights"
  :constraints {
    :accuracy (> confidence 0.85)
    :privacy :maintain-user-privacy
  }
  :preferences {
    :speed :medium
    :detail :comprehensive
  }
  :success-criteria (and (sentiment-analyzed? data) (> confidence 0.85)))"#
                    .to_string())
            } else if lower_prompt.contains("optimize")
                || lower_prompt.contains("improve")
                || lower_prompt.contains("performance")
            {
                Ok(r#"(intent "optimize_system_performance"
  :goal "Optimize system performance and efficiency"
  :constraints {
    :budget (< cost 1000)
    :downtime (< downtime 0.01)
  }
  :preferences {
    :speed :high
    :method :automated
  }
  :success-criteria (and (> performance 0.2) (< latency 100)))"#
                    .to_string())
            } else if lower_prompt.contains("backup") || lower_prompt.contains("database") {
                Ok(r#"(intent "create_database_backup"
  :goal "Create a comprehensive backup of the database"
  :constraints {
    :integrity :maintain-data-integrity
    :availability (> uptime 0.99)
  }
  :preferences {
    :compression :high
    :encryption :enabled
  }
  :success-criteria (and (backup-created? db) (backup-verified? db)))"#
                    .to_string())
            } else if lower_prompt.contains("machine learning")
                || lower_prompt.contains("ml")
                || lower_prompt.contains("pipeline")
            {
                Ok(r#"(intent "create_ml_pipeline"
  :goal "Create a machine learning pipeline for data processing"
  :constraints {
    :accuracy (> model-accuracy 0.9)
    :scalability :handle-large-datasets
  }
  :preferences {
    :framework :tensorflow
    :deployment :cloud
  }
  :success-criteria (and (pipeline-deployed? ml) (> accuracy 0.9)))"#
                    .to_string())
            } else if lower_prompt.contains("microservices")
                || lower_prompt.contains("architecture")
            {
                Ok(r#"(intent "design_microservices_architecture"
  :goal "Design a scalable microservices architecture"
  :constraints {
    :scalability :horizontal-scaling
    :reliability (> uptime 0.999)
  }
  :preferences {
    :technology :kubernetes
    :communication :rest-api
  }
  :success-criteria (and (architecture-designed? ms) (deployment-ready? ms)))"#
                    .to_string())
            } else if lower_prompt.contains("real-time") || lower_prompt.contains("streaming") {
                Ok(r#"(intent "implement_realtime_processing"
  :goal "Implement real-time data processing with streaming analytics"
  :constraints {
    :latency (< processing-time 100)
    :throughput (> events-per-second 10000)
  }
  :preferences {
    :technology :apache-kafka
    :processing :streaming
  }
  :success-criteria (and (streaming-active? rt) (< latency 100)))"#
                    .to_string())
            } else {
                // Default fallback
                Ok(r#"(intent "generic_task"
  :goal "Complete the requested task efficiently"
  :constraints {
    :quality :high
    :time (< duration 3600)
  }
  :preferences {
    :method :automated
    :priority :normal
  }
  :success-criteria (and (task-completed? task) (quality-verified? task)))"#
                    .to_string())
            }
        }
    }

    fn get_info(&self) -> LlmProviderInfo {
        LlmProviderInfo {
            name: "Stub LLM Provider".to_string(),
            version: "1.0.0".to_string(),
            model: self.config.model.clone(),
            capabilities: vec![
                "intent_generation".to_string(),
                "plan_generation".to_string(),
                "plan_validation".to_string(),
            ],
        }
    }
}

/// Factory for creating LLM providers
pub struct LlmProviderFactory;

impl LlmProviderFactory {
    /// Create an LLM provider based on configuration
    pub async fn create_provider(
        config: LlmProviderConfig,
    ) -> Result<Box<dyn LlmProvider>, RuntimeError> {
        match config.provider_type {
            LlmProviderType::Stub => Ok(Box::new(StubLlmProvider::new(config))),
            LlmProviderType::OpenAI => {
                let provider = OpenAILlmProvider::new(config)?;
                Ok(Box::new(provider))
            }
            LlmProviderType::Anthropic => {
                let provider = AnthropicLlmProvider::new(config)?;
                Ok(Box::new(provider))
            }
            LlmProviderType::Local => {
                // TODO: Implement Local provider
                Err(RuntimeError::Generic(
                    "Local provider not yet implemented".to_string(),
                ))
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_stub_provider_intent_generation() {
        let config = LlmProviderConfig {
            provider_type: LlmProviderType::Stub,
            model: "stub-model".to_string(),
            api_key: None,
            base_url: None,
            max_tokens: None,
            temperature: None,
            timeout_seconds: None,
            retry_config: crate::ccos::arbiter::arbiter_config::RetryConfig::default(),
        };

        let provider = StubLlmProvider::new(config);
        let intent = provider
            .generate_intent("analyze sentiment", None)
            .await
            .unwrap();

        // The stub provider responds based on prompt content
        assert_eq!(intent.name, Some("analyze_user_sentiment".to_string()));
        assert!(intent.goal.contains("Analyze user sentiment"));
    }

    #[tokio::test]
    async fn test_stub_provider_plan_generation() {
        let config = LlmProviderConfig {
            provider_type: LlmProviderType::Stub,
            model: "stub-model".to_string(),
            api_key: None,
            base_url: None,
            max_tokens: None,
            temperature: None,
            timeout_seconds: None,
            retry_config: crate::ccos::arbiter::arbiter_config::RetryConfig::default(),
        };

        let provider = StubLlmProvider::new(config);
        let intent = provider
            .generate_intent("optimize performance", None)
            .await
            .unwrap();
        let plan = provider.generate_plan(&intent, None).await.unwrap();

        // The stub provider responds based on intent content
        assert_eq!(
            plan.name,
            Some("stub_plan_for_optimize_system_performance".to_string())
        );
        assert!(matches!(plan.body, PlanBody::Rtfs(_)));
    }

    #[tokio::test]
    async fn test_stub_provider_validation() {
        let config = LlmProviderConfig {
            provider_type: LlmProviderType::Stub,
            model: "stub-model".to_string(),
            api_key: None,
            base_url: None,
            max_tokens: None,
            temperature: None,
            timeout_seconds: None,
            retry_config: crate::ccos::arbiter::arbiter_config::RetryConfig::default(),
        };

        let provider = StubLlmProvider::new(config);
        let intent = provider.generate_intent("test", None).await.unwrap();
        let plan = provider.generate_plan(&intent, None).await.unwrap();

        // Extract plan content for validation
        let plan_content = match &plan.body {
            PlanBody::Rtfs(content) => content.as_str(),
            PlanBody::Wasm(_) => "(wasm plan)",
        };

        let validation = provider.validate_plan(plan_content).await.unwrap();

        assert!(validation.is_valid);
        assert!(validation.confidence > 0.9);
        assert!(!validation.reasoning.is_empty());
    }

    #[tokio::test]
    async fn test_anthropic_provider_creation() {
        let config = LlmProviderConfig {
            provider_type: LlmProviderType::Anthropic,
            model: "claude-3-sonnet-20240229".to_string(),
            api_key: Some("test-key".to_string()),
            base_url: None,
            max_tokens: Some(1000),
            temperature: Some(0.7),
            timeout_seconds: Some(30),
            retry_config: crate::ccos::arbiter::arbiter_config::RetryConfig::default(),
        };

        // Test that provider can be created (even without valid API key)
        let provider = AnthropicLlmProvider::new(config);
        assert!(provider.is_ok());

        let provider = provider.unwrap();
        let info = provider.get_info();
        assert_eq!(info.name, "Anthropic Claude");
        assert_eq!(info.version, "1.0");
        assert!(info.capabilities.contains(&"intent_generation".to_string()));
        assert!(info.capabilities.contains(&"plan_generation".to_string()));
        assert!(info.capabilities.contains(&"plan_validation".to_string()));
    }

    #[tokio::test]
    async fn test_anthropic_provider_factory() {
        let config = LlmProviderConfig {
            provider_type: LlmProviderType::Anthropic,
            model: "claude-3-sonnet-20240229".to_string(),
            api_key: Some("test-key".to_string()),
            base_url: None,
            max_tokens: Some(1000),
            temperature: Some(0.7),
            timeout_seconds: Some(30),
            retry_config: crate::ccos::arbiter::arbiter_config::RetryConfig::default(),
        };

        // Test that factory can create Anthropic provider
        let provider = LlmProviderFactory::create_provider(config).await;
        assert!(provider.is_ok());

        let provider = provider.unwrap();
        let info = provider.get_info();
        assert_eq!(info.name, "Anthropic Claude");
    }

    #[test]
    fn test_extract_do_block_simple() {
        let text = r#"
Some header text
(do
    (step \"A\" (call :ccos.echo {:message \"hi\"}))
    (step \"B\" (call :ccos.math.add 2 3))
)
Trailing
"#;
        let do_block = OpenAILlmProvider::extract_do_block(text).expect("should find do block");
        assert!(do_block.starts_with("(do"));
        assert!(do_block.contains(":ccos.echo"));
        assert!(do_block.ends_with(")"));
    }

    #[test]
    fn test_extract_plan_block_and_name_and_body() {
        let text = r#"
Intro
(plan
    :name "Sample Plan"
    :language rtfs20
    :body (do
                     (step "Greet" (call :ccos.echo {:message "hi"}))
                     (step "Add" (call :ccos.math.add 2 3)))
    :annotations {:source "unit"}
)
Footer
"#;

        let plan_block =
            OpenAILlmProvider::extract_plan_block(text).expect("should find plan block");
        assert!(plan_block.starts_with("(plan"));
        let name = OpenAILlmProvider::extract_quoted_value_after_key(&plan_block, ":name")
            .expect("should extract name");
        assert_eq!(name, "Sample Plan");
        let do_block =
            OpenAILlmProvider::extract_do_block(&plan_block).expect("should find nested do block");
        assert!(do_block.contains(":ccos.math.add 2 3"));
    }

    #[test]
    fn test_extract_plan_block_with_fences_and_prose() {
        let text = r#"
Here is your plan. I've ensured it follows the schema:

```rtfs
(plan
  :name "Fenced Plan"
  :language rtfs20
  :body (do
       (step "Say" (call :ccos.echo {:message "yo"}))
       (step "Sum" (call :ccos.math.add 1 2)))
)
```

Some trailing commentary that should be ignored.
"#;

        let plan_block =
            OpenAILlmProvider::extract_plan_block(text).expect("should find plan inside fences");
        assert!(plan_block.starts_with("(plan"));
        let do_block =
            OpenAILlmProvider::extract_do_block(&plan_block).expect("nested do should be found");
        assert!(do_block.contains(":ccos.echo"));
    }

    #[test]
    fn test_extract_do_block_with_fences_and_prefix() {
        let text = r#"
Model: Here's the body you requested:

```lisp
(do
  (step "One" (call :ccos.echo {:message "a"}))
  (step "Two" (call :ccos.math.add 3 4))
)
```
"#;

        let do_block =
            OpenAILlmProvider::extract_do_block(text).expect("should find do inside fences");
        assert!(do_block.starts_with("(do"));
        assert!(parser::parse(&do_block).is_ok());
    }

    #[test]
    fn test_extract_quoted_value_after_key_multiple_occurrences() {
        let text = r#"
(plan
  :name "First"
  :annotations {:name "not this one"}
  :body (do (step "n" (call :ccos.echo {:message "m"})))
)
"#;
        let plan_block = OpenAILlmProvider::extract_plan_block(text).unwrap();
        let name = OpenAILlmProvider::extract_quoted_value_after_key(&plan_block, ":name").unwrap();
        assert_eq!(name, "First");
    }

    #[test]
    fn test_extract_do_after_body_key_normal() {
        let text = r#"
(plan
  :name "X"
  :language rtfs20
  :body (do
      (step "A" (call :ccos.echo {:message "m"}))
      (step "B" (call :ccos.math.add 5 6)))
)
"#;
        let plan_block = OpenAILlmProvider::extract_plan_block(text).unwrap();
        let do_block = OpenAILlmProvider::extract_s_expr_after_key(&plan_block, ":body").unwrap();
        assert!(do_block.starts_with("(do"));
        assert!(do_block.contains(":ccos.math.add 5 6"));
    }

    #[test]
    fn test_extract_do_after_body_key_missing_returns_none() {
        let text = r#"
(plan
  :name "No Body"
  :language rtfs20
  :annotations {:note "no body key"}
)
"#;
        let plan_block = OpenAILlmProvider::extract_plan_block(text).unwrap();
        assert!(OpenAILlmProvider::extract_s_expr_after_key(&plan_block, ":body").is_none());
    }

    #[test]
    fn test_extract_do_after_body_skips_quoted_parens() {
        let text = r#"
(plan
  :name "Quoted"
  :body "not this (do wrong)"
  :body (do (step "Only" (call :ccos.echo {:message "ok"})))
)
"#;
        let plan_block = OpenAILlmProvider::extract_plan_block(text).unwrap();
        let do_block = OpenAILlmProvider::extract_s_expr_after_key(&plan_block, ":body").unwrap();
        assert!(do_block.contains(":ccos.echo"));
    }
}
