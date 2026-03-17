//! Pattern-based analysis provider.
//!
//! Fast, deterministic analysis using pattern matching.
//! This is the default provider for capability and security analysis.

use super::provider::*;
use serde::{Deserialize, Serialize};

/// Pattern-based analysis provider.
///
/// Uses pre-defined patterns to detect capabilities and security threats.
/// Fast and deterministic but may have false positives/negatives.
#[derive(Debug, Clone)]
pub struct PatternAnalyzer {
    /// Custom patterns to detect (loaded from config)
    custom_capability_patterns: Vec<CustomPattern>,
    custom_security_patterns: Vec<CustomPattern>,
}

/// Custom pattern for extensibility.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CustomPattern {
    pub pattern: String,
    pub capability_type: Option<String>,
    pub threat_type: Option<String>,
    pub severity: Option<ThreatSeverity>,
    pub confidence: f32,
}

impl Default for PatternAnalyzer {
    fn default() -> Self {
        Self::new()
    }
}

impl PatternAnalyzer {
    /// Create a new pattern analyzer with default patterns.
    pub fn new() -> Self {
        Self {
            custom_capability_patterns: Vec::new(),
            custom_security_patterns: Vec::new(),
        }
    }

    /// Create with custom patterns loaded from configuration.
    pub fn with_custom_patterns(
        capability_patterns: Vec<CustomPattern>,
        security_patterns: Vec<CustomPattern>,
    ) -> Self {
        Self {
            custom_capability_patterns: capability_patterns,
            custom_security_patterns: security_patterns,
        }
    }

    /// Network access patterns
    const NETWORK_PATTERNS: &[&str] = &[
        "urllib.request",
        "urllib.urlopen",
        "requests.get",
        "requests.post",
        "httpx.",
        "fetch(",
        "axios.",
        "XMLHttpRequest",
        "http://",
        "https://",
        "socket.socket",
        "WebSocket",
        "curl ",
        "wget ",
    ];

    /// File system read patterns
    const FILE_READ_PATTERNS: &[&str] = &[
        "with open(",
        "open(path",
        "read_file",
        "fs.readFile",
        "fs.readFileSync",
        "os.path.exists",
        "pathlib.Path(",
        ".read_text()",
        ".read_bytes()",
    ];

    /// File system write patterns
    const FILE_WRITE_PATTERNS: &[&str] = &[
        "os.remove",
        "os.unlink",
        "shutil.rmtree",
        "fs.unlink",
        "fs.rm",
        "os.makedirs",
        "fs.mkdir",
        ".write_text()",
        ".write_bytes()",
        "open(",
    ];

    /// Code execution patterns
    const CODE_EXECUTION_PATTERNS: &[&str] = &[
        "subprocess.call",
        "subprocess.run",
        "subprocess.Popen",
        "subprocess.check_output",
        "os.system",
        "os.popen",
        "child_process.exec",
        "child_process.spawn",
        "exec(",
        "eval(",
        "shell=True",
    ];

    /// Security threat patterns
    const SECURITY_PATTERNS: &[(&str, SecurityThreatType, ThreatSeverity)] = &[
        (
            "rm -rf",
            SecurityThreatType::Destructive,
            ThreatSeverity::Critical,
        ),
        (
            "rm -rf /",
            SecurityThreatType::SandboxEscape,
            ThreatSeverity::Critical,
        ),
        (
            "sudo",
            SecurityThreatType::PrivilegeEscalation,
            ThreatSeverity::High,
        ),
        (
            "chmod 777",
            SecurityThreatType::PrivilegeEscalation,
            ThreatSeverity::Medium,
        ),
        (
            "/etc/passwd",
            SecurityThreatType::DataExfiltration,
            ThreatSeverity::High,
        ),
        (
            "/etc/shadow",
            SecurityThreatType::DataExfiltration,
            ThreatSeverity::Critical,
        ),
        (
            "fork()",
            SecurityThreatType::ResourceExhaustion,
            ThreatSeverity::Medium,
        ),
        (
            "while true",
            SecurityThreatType::ResourceExhaustion,
            ThreatSeverity::Low,
        ),
        (
            "eval(input(",
            SecurityThreatType::CommandInjection,
            ThreatSeverity::Critical,
        ),
        (
            "exec(input(",
            SecurityThreatType::CommandInjection,
            ThreatSeverity::Critical,
        ),
        (
            "__import__(",
            SecurityThreatType::CommandInjection,
            ThreatSeverity::High,
        ),
        (
            "subprocess.call(user_input",
            SecurityThreatType::CommandInjection,
            ThreatSeverity::Critical,
        ),
    ];

    /// Find a pattern in content with false positive handling.
    fn find_pattern(content: &str, pattern: &str) -> Option<(usize, String)> {
        let pattern_lower = pattern.to_lowercase();
        const EXCLUDE_PREFIXES: &[&str] = &["urlopen", "fileopen", "reopen"];

        for (line_num, line) in content.lines().enumerate() {
            let line_lower = line.to_lowercase();
            if let Some(pos) = line_lower.find(&pattern_lower) {
                // Check for false positive prefixes
                if pos > 0 {
                    let before = &line_lower[..pos];
                    let is_excluded = EXCLUDE_PREFIXES.iter().any(|ex| before.ends_with(ex));
                    if !is_excluded {
                        return Some((line_num + 1, line.to_string()));
                    }
                } else {
                    return Some((line_num + 1, line.to_string()));
                }
            }
        }
        None
    }

    /// Analyze patterns and build evidence list.
    fn analyze_patterns(
        files: &[FileToAnalyze],
        patterns: &[&str],
        capability_type: &str,
    ) -> (Vec<String>, Vec<CapabilityEvidence>) {
        let mut detected: std::collections::HashSet<String> = std::collections::HashSet::new();
        let mut evidence = Vec::new();

        for file in files {
            for pattern in patterns {
                if let Some((line_num, _)) = Self::find_pattern(&file.content, pattern) {
                    detected.insert(capability_type.to_string());
                    evidence.push(CapabilityEvidence {
                        file: file.path.clone(),
                        line: Some(line_num),
                        pattern: pattern.to_string(),
                        capability_type: capability_type.to_string(),
                        confidence: 0.90,
                    });
                }
            }
        }

        (detected.into_iter().collect(), evidence)
    }
}

impl AnalysisProvider for PatternAnalyzer {
    fn name(&self) -> &str {
        "pattern"
    }

    fn analyze_capabilities(&self, files: &[FileToAnalyze]) -> CapabilityAnalysis {
        let mut all_evidence = Vec::new();
        let mut all_types = std::collections::HashSet::new();

        // Network access detection
        let (types, evidence) =
            Self::analyze_patterns(files, Self::NETWORK_PATTERNS, "NetworkAccess");
        all_types.extend(types);
        all_evidence.extend(evidence);

        // Read access detection
        let (types, evidence) =
            Self::analyze_patterns(files, Self::FILE_READ_PATTERNS, "ReadAccess");
        all_types.extend(types);
        all_evidence.extend(evidence);

        // Write access detection
        let (types, evidence) =
            Self::analyze_patterns(files, Self::FILE_WRITE_PATTERNS, "WriteAccess");
        all_types.extend(types);
        all_evidence.extend(evidence);

        // Code execution detection
        let (types, evidence) =
            Self::analyze_patterns(files, Self::CODE_EXECUTION_PATTERNS, "CodeExecution");
        all_types.extend(types);
        all_evidence.extend(evidence);

        // Add custom patterns
        for custom in &self.custom_capability_patterns {
            for file in files {
                if let Some((line_num, _)) = Self::find_pattern(&file.content, &custom.pattern) {
                    if let Some(cap_type) = &custom.capability_type {
                        all_types.insert(cap_type.clone());
                        all_evidence.push(CapabilityEvidence {
                            file: file.path.clone(),
                            line: Some(line_num),
                            pattern: custom.pattern.clone(),
                            capability_type: cap_type.clone(),
                            confidence: custom.confidence,
                        });
                    }
                }
            }
        }

        let inferred_types: Vec<String> = all_types.into_iter().collect();
        let confidence = if all_evidence.is_empty() { 0.5 } else { 0.90 };

        CapabilityAnalysis {
            inferred_types,
            missing: vec![],   // Filled in by caller
            excessive: vec![], // Filled in by caller
            confidence,
            evidence: all_evidence,
            provider: "pattern".to_string(),
        }
    }

    fn analyze_security(&self, files: &[FileToAnalyze]) -> SecurityAnalysis {
        let mut threats = Vec::new();
        let mut remote_access_detected = false;

        for file in files {
            // Check security patterns
            for (pattern, threat_type, severity) in Self::SECURITY_PATTERNS {
                if let Some((line_num, _)) = Self::find_pattern(&file.content, pattern) {
                    threats.push(SecurityThreat {
                        threat_type: threat_type.clone(),
                        severity: severity.clone(),
                        description: format!("Detected pattern: {}", pattern),
                        file: file.path.clone(),
                        line: Some(line_num),
                        pattern: pattern.to_string(),
                        confidence: 0.90,
                    });
                }
            }

            // Check for remote access (informational, not a threat)
            for pattern in Self::NETWORK_PATTERNS {
                if Self::find_pattern(&file.content, pattern).is_some() {
                    remote_access_detected = true;
                    break;
                }
            }
        }

        // Add custom security patterns
        for custom in &self.custom_security_patterns {
            for file in files {
                if let Some((line_num, _)) = Self::find_pattern(&file.content, &custom.pattern) {
                    if let Some(threat_name) = &custom.threat_type {
                        threats.push(SecurityThreat {
                            threat_type: SecurityThreatType::Custom(threat_name.clone()),
                            severity: custom.severity.clone().unwrap_or(ThreatSeverity::Medium),
                            description: format!("Custom pattern detected: {}", custom.pattern),
                            file: file.path.clone(),
                            line: Some(line_num),
                            pattern: custom.pattern.clone(),
                            confidence: custom.confidence,
                        });
                    }
                }
            }
        }

        let has_critical = threats
            .iter()
            .any(|t| matches!(t.severity, ThreatSeverity::Critical));
        let has_high = threats
            .iter()
            .any(|t| matches!(t.severity, ThreatSeverity::High));
        let passed = !has_critical && !has_high;
        let threats_empty = threats.is_empty();

        SecurityAnalysis {
            passed,
            threats,
            remote_access_detected,
            confidence: if threats_empty { 0.70 } else { 0.90 },
            provider: "pattern".to_string(),
        }
    }

    fn estimated_duration_ms(&self) -> u64 {
        // Pattern analysis is very fast
        10
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_file(path: &str, content: &str) -> FileToAnalyze {
        FileToAnalyze {
            path: path.to_string(),
            content: content.to_string(),
        }
    }

    #[test]
    fn test_pattern_analyzer_network_access() {
        let analyzer = PatternAnalyzer::new();
        let files = vec![test_file(
            "main.py",
            r#"
import urllib.request

def fetch():
    return urllib.request.urlopen("https://api.example.com").read()
"#,
        )];

        let result = analyzer.analyze_capabilities(&files);
        assert!(result.inferred_types.contains(&"NetworkAccess".to_string()));
        assert_eq!(result.provider, "pattern");
    }

    #[test]
    fn test_pattern_analyzer_security_threats() {
        let analyzer = PatternAnalyzer::new();
        let files = vec![test_file(
            "bad.py",
            r#"
import os
os.system("rm -rf /")
"#,
        )];

        let result = analyzer.analyze_security(&files);
        assert!(!result.passed);
        assert!(!result.threats.is_empty());
        assert!(result
            .threats
            .iter()
            .any(|t| matches!(t.severity, ThreatSeverity::Critical)));
    }

    #[test]
    fn test_pattern_analyzer_custom_patterns() {
        let custom_patterns = vec![CustomPattern {
            pattern: "my_api_key".to_string(),
            capability_type: Some("NetworkAccess".to_string()),
            threat_type: None,
            severity: None,
            confidence: 0.95,
        }];

        let analyzer = PatternAnalyzer::with_custom_patterns(custom_patterns, vec![]);
        let files = vec![test_file(
            "config.py",
            r#"
my_api_key = "secret"
"#,
        )];

        let result = analyzer.analyze_capabilities(&files);
        assert!(result.inferred_types.contains(&"NetworkAccess".to_string()));
    }

    #[test]
    fn test_pattern_analyzer_no_false_positive_urlopen() {
        let analyzer = PatternAnalyzer::new();
        let files = vec![test_file(
            "api.py",
            r#"
import urllib.request

def fetch():
    return urllib.request.urlopen("https://example.com").read()
"#,
        )];

        let result = analyzer.analyze_capabilities(&files);
        // Should detect NetworkAccess but NOT ReadAccess from urlopen
        assert!(result.inferred_types.contains(&"NetworkAccess".to_string()));
        assert!(
            !result.inferred_types.contains(&"ReadAccess".to_string()),
            "Should not detect ReadAccess from urlopen (false positive)"
        );
    }

    #[test]
    fn test_combined_analysis() {
        let analyzer = PatternAnalyzer::new();
        let files = vec![test_file(
            "safe.py",
            r#"
import requests

def fetch():
    return requests.get("https://api.example.com").json()
"#,
        )];

        let result = analyzer.analyze_combined(&files);
        assert!(result
            .capability
            .inferred_types
            .contains(&"NetworkAccess".to_string()));
        assert!(result.security.remote_access_detected);
        assert_eq!(result.recommended_action, AnalysisAction::Approve);
    }
}
