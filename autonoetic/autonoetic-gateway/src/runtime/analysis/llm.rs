//! LLM-based analysis provider (stub for future implementation).
//!
//! This provider uses an LLM to analyze code for capabilities and security threats.
//! It is triggered by the gateway during agent.install, NOT by the planner.
//!
//! # Future Implementation
//!
//! The LLM analyzer will:
//! 1. Send code to a configured LLM (e.g., Gemini, Claude)
//! 2. Ask it to analyze for required capabilities
//! 3. Ask it to detect security threats
//! 4. Return structured results
//!
//! This provides more accurate analysis than pattern matching but is slower.
//!
//! # Configuration
//!
//! ```yaml
//! code_analysis:
//!   capability_provider: "llm"
//!   llm_config:
//!     provider: "openrouter"
//!     model: "google/gemini-3-flash-preview"
//!     temperature: 0.1
//!     timeout_secs: 30
//! ```

use super::provider::*;
use serde::{Deserialize, Serialize};

/// LLM analysis provider (stub implementation).
///
/// This is a placeholder for future LLM-based code analysis.
/// When enabled, it will use the gateway's LLM driver to analyze code.
#[derive(Debug, Clone)]
pub struct LlmAnalyzer {
    /// Configuration for the LLM provider
    config: LlmAnalysisConfig,
}

/// Configuration for LLM-based analysis.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LlmAnalysisConfig {
    /// LLM provider (e.g., "openrouter", "openai", "anthropic")
    #[serde(default = "default_provider")]
    pub provider: String,

    /// Model to use for analysis
    #[serde(default = "default_model")]
    pub model: String,

    /// Temperature for analysis (lower = more deterministic)
    #[serde(default = "default_temperature")]
    pub temperature: f32,

    /// Timeout for analysis in seconds
    #[serde(default = "default_timeout")]
    pub timeout_secs: u64,

    /// Maximum tokens for response
    #[serde(default = "default_max_tokens")]
    pub max_tokens: u32,
}

fn default_provider() -> String {
    "openrouter".to_string()
}

fn default_model() -> String {
    "google/gemini-3-flash-preview".to_string()
}

fn default_temperature() -> f32 {
    0.1
}

fn default_timeout() -> u64 {
    30
}

fn default_max_tokens() -> u32 {
    2000
}

impl Default for LlmAnalysisConfig {
    fn default() -> Self {
        Self {
            provider: default_provider(),
            model: default_model(),
            temperature: default_temperature(),
            timeout_secs: default_timeout(),
            max_tokens: default_max_tokens(),
        }
    }
}

impl LlmAnalyzer {
    /// Create a new LLM analyzer with default configuration.
    pub fn new() -> Self {
        Self {
            config: LlmAnalysisConfig::default(),
        }
    }

    /// Create with custom configuration.
    pub fn with_config(config: LlmAnalysisConfig) -> Self {
        Self { config }
    }

    /// Build the analysis prompt for capability detection.
    #[allow(dead_code)]
    fn build_capability_prompt(files: &[FileToAnalyze]) -> String {
        let mut prompt = String::from(
            "Analyze the following code and identify the required capabilities.\n\n\
             Capabilities to detect:\n\
             - NetworkAccess: HTTP requests, websockets, socket connections\n\
             - ReadAccess: File reading, config loading\n\
             - WriteAccess: File writing, directory creation\n\
             - CodeExecution: subprocess, exec, shell commands\n\n\
             Return JSON with format:\n\
             {\n\
               \"capabilities\": [\"NetworkAccess\", ...],\n\
               \"confidence\": 0.95,\n\
               \"reasoning\": \"...\"\n\
             }\n\n\
             Code:\n",
        );

        for file in files {
            prompt.push_str(&format!("--- {} ---\n{}\n\n", file.path, file.content));
        }

        prompt
    }

    /// Build the analysis prompt for security detection.
    #[allow(dead_code)]
    fn build_security_prompt(files: &[FileToAnalyze]) -> String {
        let mut prompt = String::from(
            "Analyze the following code for security threats.\n\n\
             Threats to detect:\n\
             - Command injection: eval(), exec(), shell=True with user input\n\
             - Privilege escalation: sudo, chmod 777, setuid\n\
             - Sandbox escape: rm -rf /, accessing /etc/passwd\n\
             - Resource exhaustion: fork bombs, infinite loops\n\
             - Data exfiltration: Sending data to external services\n\n\
             Return JSON with format:\n\
             {\n\
               \"passed\": true/false,\n\
               \"threats\": [{\"type\": \"...\", \"severity\": \"high\", \"description\": \"...\"}],\n\
               \"reasoning\": \"...\"\n\
             }\n\n\
             Code:\n"
        );

        for file in files {
            prompt.push_str(&format!("--- {} ---\n{}\n\n", file.path, file.content));
        }

        prompt
    }
}

impl Default for LlmAnalyzer {
    fn default() -> Self {
        Self::new()
    }
}

impl AnalysisProvider for LlmAnalyzer {
    fn name(&self) -> &str {
        "llm"
    }

    fn analyze_capabilities(&self, files: &[FileToAnalyze]) -> CapabilityAnalysis {
        // TODO: Implement actual LLM call
        // For now, fall back to pattern analysis
        tracing::warn!(
            target: "analysis",
            "LlmAnalyzer::analyze_capabilities called but not yet implemented. \
             Falling back to pattern analysis."
        );

        // Use pattern analyzer as fallback
        let pattern_analyzer = super::pattern::PatternAnalyzer::new();
        let mut result = pattern_analyzer.analyze_capabilities(files);
        result.provider = "llm (fallback to pattern)".to_string();
        result
    }

    fn analyze_security(&self, files: &[FileToAnalyze]) -> SecurityAnalysis {
        // TODO: Implement actual LLM call
        // For now, fall back to pattern analysis
        tracing::warn!(
            target: "analysis",
            "LlmAnalyzer::analyze_security called but not yet implemented. \
             Falling back to pattern analysis."
        );

        // Use pattern analyzer as fallback
        let pattern_analyzer = super::pattern::PatternAnalyzer::new();
        let mut result = pattern_analyzer.analyze_security(files);
        result.provider = "llm (fallback to pattern)".to_string();
        result
    }

    fn is_async(&self) -> bool {
        // LLM calls are async
        true
    }

    fn estimated_duration_ms(&self) -> u64 {
        // LLM analysis typically takes 2-5 seconds
        3000
    }
}

/// Template for future LLM response parsing.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[allow(dead_code)]
struct LlmCapabilityResponse {
    pub capabilities: Vec<String>,
    pub confidence: f32,
    pub reasoning: String,
}

/// Template for future LLM security response parsing.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[allow(dead_code)]
struct LlmSecurityResponse {
    pub passed: bool,
    pub threats: Vec<LlmThreat>,
    pub reasoning: String,
}

/// Threat detected by LLM.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[allow(dead_code)]
struct LlmThreat {
    #[serde(rename = "type")]
    pub threat_type: String,
    pub severity: String,
    pub description: String,
    pub line: Option<usize>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_llm_analyzer_stub_returns_fallback() {
        let analyzer = LlmAnalyzer::new();
        let files = vec![FileToAnalyze {
            path: "test.py".to_string(),
            content: "import urllib.request\nurllib.request.urlopen('https://example.com')"
                .to_string(),
        }];

        // Should fall back to pattern analysis
        let result = analyzer.analyze_capabilities(&files);
        assert!(result.inferred_types.contains(&"NetworkAccess".to_string()));
        assert!(result.provider.contains("fallback"));
    }

    #[test]
    fn test_llm_analyzer_is_async() {
        let analyzer = LlmAnalyzer::new();
        assert!(analyzer.is_async());
    }

    #[test]
    fn test_llm_analyzer_estimated_duration() {
        let analyzer = LlmAnalyzer::new();
        assert!(analyzer.estimated_duration_ms() > 1000); // LLM takes seconds
    }

    #[test]
    fn test_llm_capability_prompt_generation() {
        let files = vec![FileToAnalyze {
            path: "main.py".to_string(),
            content: "import requests".to_string(),
        }];

        let prompt = LlmAnalyzer::build_capability_prompt(&files);
        assert!(prompt.contains("--- main.py ---"));
        assert!(prompt.contains("import requests"));
        assert!(prompt.contains("NetworkAccess"));
    }
}
