//! Pluggable code analysis providers for agent.install validation.
//!
//! This module provides a trait-based architecture for code analysis that can be
//! swapped at runtime. Different providers can be used:
//!
//! - **PatternAnalyzer**: Fast pattern-based detection (default)
//! - **LlmAnalyzer**: LLM-powered code review (future, triggered by gateway)
//! - **HybridAnalyzer**: Combines pattern + LLM for high-risk installs
//!
//! # Configuration
//!
//! Configure in `config.yaml`:
//! ```yaml
//! code_analysis:
//!   capability_provider: "pattern"  # or "llm", "hybrid"
//!   security_provider: "pattern"    # or "llm", "hybrid"
//!   llm_model: "google/gemini-3-flash-preview"  # for LLM providers
//!   require_llm_review_for: ["NetworkAccess", "CodeExecution"]
//! ```

pub mod composite;
pub mod llm;
pub mod pattern;
pub mod provider;
pub mod python_ast;

pub use composite::CompositeAnalyzer;
pub use llm::LlmAnalyzer;
pub use pattern::PatternAnalyzer;
pub use python_ast::PythonAstAnalyzer;
pub use provider::{
    AnalysisProvider, AnalysisProviderType, CapabilityAnalysis, FileToAnalyze, SecurityAnalysis,
    SecurityThreatType,
};

use autonoetic_types::capability::Capability;

/// Factory for creating analysis providers based on configuration.
pub struct AnalysisProviderFactory;

impl AnalysisProviderFactory {
    /// Create a capability analysis provider based on type.
    pub fn create_capability_provider(
        provider_type: &AnalysisProviderType,
    ) -> Box<dyn AnalysisProvider> {
        match provider_type {
            AnalysisProviderType::Pattern => Box::new(PatternAnalyzer::new()),
            AnalysisProviderType::Llm => Box::new(LlmAnalyzer::new()),
            AnalysisProviderType::Composite => Box::new(CompositeAnalyzer::new_capability()),
            AnalysisProviderType::PythonAst => Box::new(PythonAstAnalyzer::new()),
            AnalysisProviderType::None => Box::new(NoOpAnalyzer),
        }
    }

    /// Create a security analysis provider based on type.
    pub fn create_security_provider(
        provider_type: &AnalysisProviderType,
    ) -> Box<dyn AnalysisProvider> {
        match provider_type {
            AnalysisProviderType::Pattern => Box::new(PatternAnalyzer::new()),
            AnalysisProviderType::Llm => Box::new(LlmAnalyzer::new()),
            AnalysisProviderType::Composite => Box::new(CompositeAnalyzer::new_security()),
            AnalysisProviderType::PythonAst => Box::new(PythonAstAnalyzer::new()),
            AnalysisProviderType::None => Box::new(NoOpAnalyzer),
        }
    }
}

/// No-op analyzer that always returns empty results.
/// Used when analysis is disabled.
#[derive(Debug)]
struct NoOpAnalyzer;

impl AnalysisProvider for NoOpAnalyzer {
    fn name(&self) -> &str {
        "none"
    }

    fn analyze_capabilities(&self, _files: &[FileToAnalyze]) -> CapabilityAnalysis {
        CapabilityAnalysis {
            inferred_types: vec![],
            missing: vec![],
            excessive: vec![],
            confidence: 1.0,
            evidence: vec![],
            provider: "none".to_string(),
        }
    }

    fn analyze_security(&self, _files: &[FileToAnalyze]) -> SecurityAnalysis {
        SecurityAnalysis {
            passed: true,
            threats: vec![],
            remote_access_detected: false,
            confidence: 1.0,
            provider: "none".to_string(),
        }
    }
}

/// Helper to merge declared capabilities with inferred ones.
pub fn merge_capabilities(declared: &[Capability], inferred_types: &[String]) -> Vec<Capability> {
    use std::collections::HashSet;

    let mut result: Vec<Capability> = declared.to_vec();
    let declared_types: HashSet<String> = declared
        .iter()
        .map(|c| capability_type_name(c).to_string())
        .collect();

    for cap_type in inferred_types {
        if !declared_types.contains(cap_type.as_str()) {
            if let Some(cap) = type_to_capability(cap_type) {
                result.push(cap);
            }
        }
    }

    result
}

fn capability_type_name(cap: &Capability) -> &'static str {
    match cap {
        Capability::NetworkAccess { .. } => "NetworkAccess",
        Capability::ReadAccess { .. } => "ReadAccess",
        Capability::WriteAccess { .. } => "WriteAccess",
        Capability::CodeExecution { .. } => "CodeExecution",
        Capability::AgentSpawn { .. } => "AgentSpawn",
        Capability::AgentMessage { .. } => "AgentMessage",
        Capability::SandboxFunctions { .. } => "SandboxFunctions",
        Capability::BackgroundReevaluation { .. } => "BackgroundReevaluation",
    }
}

fn type_to_capability(cap_type: &str) -> Option<Capability> {
    match cap_type {
        "NetworkAccess" => Some(Capability::NetworkAccess {
            hosts: vec!["*".to_string()],
        }),
        "ReadAccess" => Some(Capability::ReadAccess {
            scopes: vec!["*".to_string()],
        }),
        "WriteAccess" => Some(Capability::WriteAccess {
            scopes: vec!["*".to_string()],
        }),
        "CodeExecution" => Some(Capability::CodeExecution {
            patterns: vec!["*".to_string()],
        }),
        "AgentSpawn" => Some(Capability::AgentSpawn { max_children: 1 }),
        "AgentMessage" => Some(Capability::AgentMessage {
            patterns: vec!["*".to_string()],
        }),
        _ => None,
    }
}
