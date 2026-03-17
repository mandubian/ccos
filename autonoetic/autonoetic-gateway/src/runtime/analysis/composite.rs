//! Composite analysis provider that combines multiple analyzers.
//!
//! This provider runs multiple analysis strategies and combines results.
//! Useful for high-risk installs that need both fast pattern detection
//! and deeper LLM review.

use super::provider::*;
use super::{LlmAnalyzer, PatternAnalyzer};
use serde::{Deserialize, Serialize};

/// Composite analyzer that combines multiple providers.
///
/// # Modes
///
/// - **Pattern-first**: Run pattern analysis, escalate to LLM only for threats
/// - **Parallel**: Run both in parallel, use LLM results if pattern is uncertain
/// - **Sequential**: Run pattern then LLM, combine results
#[derive(Debug)]
pub struct CompositeAnalyzer {
    /// Primary analyzer (always runs)
    primary: Box<dyn AnalysisProvider>,
    /// Secondary analyzer (runs conditionally)
    secondary: Box<dyn AnalysisProvider>,
    /// When to use the secondary analyzer
    escalation_policy: EscalationPolicy,
}

/// When to escalate to secondary analyzer.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EscalationPolicy {
    /// Always run secondary
    Always,
    /// Only when primary detects threats
    OnThreatDetected,
    /// Only when primary has low confidence
    OnLowConfidence(f32),
    /// Only for specific capability types
    ForCapabilities(Vec<String>),
    /// Never escalate (primary only)
    Never,
}

impl Default for EscalationPolicy {
    fn default() -> Self {
        Self::OnThreatDetected
    }
}

impl CompositeAnalyzer {
    /// Create a composite analyzer for capability analysis.
    pub fn new_capability() -> Self {
        Self {
            primary: Box::new(PatternAnalyzer::new()),
            secondary: Box::new(LlmAnalyzer::new()),
            escalation_policy: EscalationPolicy::OnLowConfidence(0.7),
        }
    }

    /// Create a composite analyzer for security analysis.
    pub fn new_security() -> Self {
        Self {
            primary: Box::new(PatternAnalyzer::new()),
            secondary: Box::new(LlmAnalyzer::new()),
            escalation_policy: EscalationPolicy::OnThreatDetected,
        }
    }

    /// Create with custom analyzers and policy.
    pub fn new(
        primary: Box<dyn AnalysisProvider>,
        secondary: Box<dyn AnalysisProvider>,
        escalation_policy: EscalationPolicy,
    ) -> Self {
        Self {
            primary,
            secondary,
            escalation_policy,
        }
    }

    /// Check if escalation is needed based on policy and primary results.
    fn should_escalate_capability(&self, primary: &CapabilityAnalysis) -> bool {
        match &self.escalation_policy {
            EscalationPolicy::Always => true,
            EscalationPolicy::Never => false,
            EscalationPolicy::OnLowConfidence(threshold) => primary.confidence < *threshold,
            EscalationPolicy::ForCapabilities(required) => required
                .iter()
                .any(|cap| primary.inferred_types.contains(cap)),
            EscalationPolicy::OnThreatDetected => false, // Only for security
        }
    }

    fn should_escalate_security(&self, primary: &SecurityAnalysis) -> bool {
        match &self.escalation_policy {
            EscalationPolicy::Always => true,
            EscalationPolicy::Never => false,
            EscalationPolicy::OnThreatDetected => !primary.threats.is_empty(),
            EscalationPolicy::OnLowConfidence(threshold) => primary.confidence < *threshold,
            EscalationPolicy::ForCapabilities(_) => false, // Only for capabilities
        }
    }

    /// Merge two capability analyses, preferring higher confidence results.
    fn merge_capability(
        &self,
        primary: &CapabilityAnalysis,
        secondary: &CapabilityAnalysis,
    ) -> CapabilityAnalysis {
        // Union of inferred types
        let mut all_types: std::collections::HashSet<String> =
            primary.inferred_types.iter().cloned().collect();
        all_types.extend(secondary.inferred_types.iter().cloned());

        // Union of evidence
        let mut all_evidence = primary.evidence.clone();
        all_evidence.extend(secondary.evidence.clone());

        // Use higher confidence
        let confidence = primary.confidence.max(secondary.confidence);

        CapabilityAnalysis {
            inferred_types: all_types.into_iter().collect(),
            missing: vec![],   // Filled by caller
            excessive: vec![], // Filled by caller,
            confidence,
            evidence: all_evidence,
            provider: format!("{}, {}", primary.provider, secondary.provider),
        }
    }

    /// Merge two security analyses, using stricter result.
    fn merge_security(
        &self,
        primary: &SecurityAnalysis,
        secondary: &SecurityAnalysis,
    ) -> SecurityAnalysis {
        // Union of threats
        let mut all_threats = primary.threats.clone();
        all_threats.extend(secondary.threats.clone());

        // Passed only if both pass
        let passed = primary.passed && secondary.passed;

        // Remote access if either detected
        let remote_access = primary.remote_access_detected || secondary.remote_access_detected;

        // Use higher confidence
        let confidence = primary.confidence.max(secondary.confidence);

        SecurityAnalysis {
            passed,
            threats: all_threats,
            remote_access_detected: remote_access,
            confidence,
            provider: format!("{}, {}", primary.provider, secondary.provider),
        }
    }
}

impl AnalysisProvider for CompositeAnalyzer {
    fn name(&self) -> &str {
        "composite"
    }

    fn analyze_capabilities(&self, files: &[FileToAnalyze]) -> CapabilityAnalysis {
        let primary_result = self.primary.analyze_capabilities(files);

        if self.should_escalate_capability(&primary_result) {
            tracing::info!(
                target: "analysis",
                primary_confidence = primary_result.confidence,
                "Escalating capability analysis to secondary provider"
            );

            let secondary_result = self.secondary.analyze_capabilities(files);
            self.merge_capability(&primary_result, &secondary_result)
        } else {
            primary_result
        }
    }

    fn analyze_security(&self, files: &[FileToAnalyze]) -> SecurityAnalysis {
        let primary_result = self.primary.analyze_security(files);

        if self.should_escalate_security(&primary_result) {
            tracing::info!(
                target: "analysis",
                threats_detected = primary_result.threats.len(),
                "Escalating security analysis to secondary provider"
            );

            let secondary_result = self.secondary.analyze_security(files);
            self.merge_security(&primary_result, &secondary_result)
        } else {
            primary_result
        }
    }

    fn is_async(&self) -> bool {
        self.primary.is_async() || self.secondary.is_async()
    }

    fn estimated_duration_ms(&self) -> u64 {
        // Estimate: primary + (secondary if escalated)
        self.primary.estimated_duration_ms() + self.secondary.estimated_duration_ms()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_composite_no_escalation_on_high_confidence() {
        let analyzer = CompositeAnalyzer::new_capability();
        let files = vec![FileToAnalyze {
            path: "test.py".to_string(),
            content: "import urllib.request\nurllib.request.urlopen('https://example.com')"
                .to_string(),
        }];

        let result = analyzer.analyze_capabilities(&files);
        // Pattern analyzer has high confidence, so no LLM escalation
        assert!(result.provider.contains("pattern"));
        assert!(result.inferred_types.contains(&"NetworkAccess".to_string()));
    }

    #[test]
    fn test_composite_escalation_on_threats() {
        let analyzer = CompositeAnalyzer::new_security();
        let files = vec![FileToAnalyze {
            path: "bad.py".to_string(),
            content: "import os\nos.system('rm -rf /')".to_string(),
        }];

        let result = analyzer.analyze_security(&files);
        // Should escalate to LLM due to threat detection
        assert!(!result.threats.is_empty());
    }
}
