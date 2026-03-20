//! Trait definitions for pluggable analysis providers.
//!
//! All code analysis providers must implement the `AnalysisProvider` trait.
//! This allows different strategies (pattern-based, LLM-based, hybrid) to be
//! swapped at runtime.

use serde::{Deserialize, Serialize};

/// A file to be analyzed.
#[derive(Debug, Clone)]
pub struct FileToAnalyze {
    pub path: String,
    pub content: String,
}

/// Analysis provider type for configuration.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum AnalysisProviderType {
    /// Fast pattern-based analysis (default)
    Pattern,
    /// LLM-powered code review (future)
    Llm,
    /// Combines multiple providers
    Composite,
    /// Python 3 stdlib `ast` scan (bundled script, no pip deps); falls back to pattern if `python3` fails
    #[serde(rename = "python_ast")]
    PythonAst,
    /// No analysis (disabled)
    None,
}

impl Default for AnalysisProviderType {
    fn default() -> Self {
        Self::Pattern
    }
}

/// Evidence of a capability requirement found in code.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CapabilityEvidence {
    pub file: String,
    pub line: Option<usize>,
    pub pattern: String,
    pub capability_type: String,
    pub confidence: f32,
}

/// Result of capability analysis.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CapabilityAnalysis {
    /// Inferred capability types (e.g., "NetworkAccess", "ReadAccess")
    pub inferred_types: Vec<String>,
    /// Capability types declared but not found in code
    pub excessive: Vec<String>,
    /// Capability types found in code but not declared
    pub missing: Vec<String>,
    /// Overall confidence (0.0 - 1.0)
    pub confidence: f32,
    /// Detailed evidence for each detection
    pub evidence: Vec<CapabilityEvidence>,
    /// Provider that performed the analysis
    pub provider: String,
}

/// Type of security threat detected.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum SecurityThreatType {
    /// Command injection attempt
    CommandInjection,
    /// Privilege escalation attempt
    PrivilegeEscalation,
    /// Sandbox escape attempt
    SandboxEscape,
    /// Shell injection
    ShellInjection,
    /// Destructive operation (rm -rf, etc.)
    Destructive,
    /// Resource exhaustion (fork bomb, etc.)
    ResourceExhaustion,
    /// Remote code execution
    RemoteCodeExecution,
    /// Data exfiltration
    DataExfiltration,
    /// Custom threat type
    Custom(String),
}

/// A detected security threat.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SecurityThreat {
    pub threat_type: SecurityThreatType,
    pub severity: ThreatSeverity,
    pub description: String,
    pub file: String,
    pub line: Option<usize>,
    pub pattern: String,
    pub confidence: f32,
}

/// Severity level of a security threat.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, PartialOrd)]
#[serde(rename_all = "lowercase")]
pub enum ThreatSeverity {
    Info,
    Low,
    Medium,
    High,
    Critical,
}

/// Result of security analysis.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SecurityAnalysis {
    /// Whether the code passed security analysis
    pub passed: bool,
    /// Detected security threats
    pub threats: Vec<SecurityThreat>,
    /// Whether remote/network access was detected
    pub remote_access_detected: bool,
    /// Overall confidence (0.0 - 1.0)
    pub confidence: f32,
    /// Provider that performed the analysis
    pub provider: String,
}

/// Combined analysis result from multiple providers.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CombinedAnalysis {
    pub capability: CapabilityAnalysis,
    pub security: SecurityAnalysis,
    pub requires_manual_review: bool,
    pub recommended_action: AnalysisAction,
}

/// Recommended action based on analysis.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum AnalysisAction {
    /// Proceed with install
    Approve,
    /// Require operator approval
    RequireApproval,
    /// Reject the install
    Reject,
    /// Run additional analysis (e.g., LLM review)
    Escalate,
}

/// Trait for code analysis providers.
///
/// Implement this trait to create custom analysis providers.
/// The provider is triggered by the gateway during agent.install,
/// not by the planner or other agents.
///
/// # Example
///
/// ```rust
/// struct MyAnalyzer;
///
/// impl AnalysisProvider for MyAnalyzer {
///     fn name(&self) -> &str {
///         "my_custom_analyzer"
///     }
///
///     fn analyze_capabilities(&self, files: &[FileToAnalyze]) -> CapabilityAnalysis {
///         // Custom capability detection logic
///         CapabilityAnalysis {
///             inferred_types: vec!["NetworkAccess".to_string()],
///             // ...
///         }
///     }
///
///     fn analyze_security(&self, files: &[FileToAnalyze]) -> SecurityAnalysis {
///         // Custom security analysis logic
///         SecurityAnalysis {
///             passed: true,
///             // ...
///         }
///     }
/// }
/// ```
pub trait AnalysisProvider: Send + Sync + std::fmt::Debug {
    /// Name of this provider (e.g., "pattern", "llm", "hybrid")
    fn name(&self) -> &str;

    /// Analyze files to detect required capabilities.
    ///
    /// This method is called by the gateway during agent.install to detect
    /// what capabilities the code requires based on its content.
    fn analyze_capabilities(&self, files: &[FileToAnalyze]) -> CapabilityAnalysis;

    /// Analyze files for security threats.
    ///
    /// This method is called by the gateway during agent.install to detect
    /// security issues before the agent is installed.
    fn analyze_security(&self, files: &[FileToAnalyze]) -> SecurityAnalysis;

    /// Perform both capability and security analysis in one pass.
    ///
    /// Default implementation calls both methods separately.
    /// Override this if your provider can optimize combined analysis.
    fn analyze_combined(&self, files: &[FileToAnalyze]) -> CombinedAnalysis {
        let capability = self.analyze_capabilities(files);
        let security = self.analyze_security(files);

        let requires_manual_review = security
            .threats
            .iter()
            .any(|t| matches!(t.severity, ThreatSeverity::High | ThreatSeverity::Critical))
            || !security.passed;

        let recommended_action = if !security.passed {
            AnalysisAction::Reject
        } else if requires_manual_review {
            AnalysisAction::RequireApproval
        } else if !capability.missing.is_empty() {
            AnalysisAction::Reject
        } else {
            AnalysisAction::Approve
        };

        CombinedAnalysis {
            capability,
            security,
            requires_manual_review,
            recommended_action,
        }
    }

    /// Whether this provider requires async execution (e.g., LLM calls).
    fn is_async(&self) -> bool {
        false
    }

    /// Estimated time for analysis in milliseconds.
    /// Used for timeout configuration and UX feedback.
    fn estimated_duration_ms(&self) -> u64 {
        100
    }
}
