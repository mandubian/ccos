//! Static Analyzers for RTFS Code
//!
//! This module implements static analysis tools for validating RTFS code
//! before capability registration.

use super::validation_harness::{IssueCategory, IssueSeverity, ValidationIssue};
use crate::capability_marketplace::types::CapabilityManifest;
use std::collections::HashSet;

/// Static analyzer trait for RTFS code analysis
pub trait StaticAnalyzer {
    /// Analyze the capability manifest and RTFS code
    fn analyze(&self, manifest: &CapabilityManifest, rtfs_code: &str) -> Vec<ValidationIssue>;

    /// Get analyzer name
    fn name(&self) -> &str;

    /// Get analyzer description
    fn description(&self) -> &str;
}

/// RTFS syntax analyzer
pub struct RtfsSyntaxAnalyzer {
    name: String,
}

impl RtfsSyntaxAnalyzer {
    pub fn new() -> Self {
        Self {
            name: "RTFS Syntax Analyzer".to_string(),
        }
    }
}

impl StaticAnalyzer for RtfsSyntaxAnalyzer {
    fn analyze(&self, _manifest: &CapabilityManifest, rtfs_code: &str) -> Vec<ValidationIssue> {
        let mut issues = Vec::new();

        // Check for basic RTFS syntax issues
        if !rtfs_code.trim().starts_with('(') {
            issues.push(ValidationIssue {
                severity: IssueSeverity::Critical,
                category: IssueCategory::Quality,
                description: "RTFS code must start with an opening parenthesis".to_string(),
                location: Some("RTFS code start".to_string()),
                suggestion: Some("Ensure RTFS code follows proper syntax".to_string()),
                code: "SYNTAX001".to_string(),
            });
        }

        // Check for balanced parentheses
        let open_count = rtfs_code.matches('(').count();
        let close_count = rtfs_code.matches(')').count();
        if open_count != close_count {
            issues.push(ValidationIssue {
                severity: IssueSeverity::Critical,
                category: IssueCategory::Quality,
                description: format!(
                    "Unbalanced parentheses: {} opening, {} closing",
                    open_count, close_count
                ),
                location: Some("RTFS code".to_string()),
                suggestion: Some("Ensure all parentheses are properly balanced".to_string()),
                code: "SYNTAX002".to_string(),
            });
        }

        // Check for capability definition structure
        if !rtfs_code.contains("(capability ") {
            issues.push(ValidationIssue {
                severity: IssueSeverity::Critical,
                category: IssueCategory::Quality,
                description: "RTFS code must contain a capability definition".to_string(),
                location: Some("RTFS code".to_string()),
                suggestion: Some("Add a capability definition with proper syntax".to_string()),
                code: "SYNTAX003".to_string(),
            });
        }

        // Check for required capability properties
        if !rtfs_code.contains(":description") {
            issues.push(ValidationIssue {
                severity: IssueSeverity::High,
                category: IssueCategory::Documentation,
                description: "Capability must have a description property".to_string(),
                location: Some("Capability definition".to_string()),
                suggestion: Some("Add :description property to capability".to_string()),
                code: "SYNTAX004".to_string(),
            });
        }

        if !rtfs_code.contains(":implementation") {
            issues.push(ValidationIssue {
                severity: IssueSeverity::Critical,
                category: IssueCategory::Quality,
                description: "Capability must have an implementation".to_string(),
                location: Some("Capability definition".to_string()),
                suggestion: Some("Add :implementation property to capability".to_string()),
                code: "SYNTAX005".to_string(),
            });
        }

        issues
    }

    fn name(&self) -> &str {
        &self.name
    }

    fn description(&self) -> &str {
        "Analyzes RTFS code for syntax errors and compliance"
    }
}

/// Security analyzer for RTFS code
#[allow(dead_code)]
pub struct SecurityAnalyzer {
    name: String,
    dangerous_functions: HashSet<String>,
    secure_patterns: HashSet<String>,
}

impl SecurityAnalyzer {
    pub fn new() -> Self {
        let mut dangerous_functions = HashSet::new();
        dangerous_functions.insert("eval".to_string());
        dangerous_functions.insert("exec".to_string());
        dangerous_functions.insert("system".to_string());
        dangerous_functions.insert("shell".to_string());

        let mut secure_patterns = HashSet::new();
        secure_patterns.insert("https://".to_string());
        secure_patterns.insert(":ccos.auth.inject".to_string());

        Self {
            name: "Security Analyzer".to_string(),
            dangerous_functions,
            secure_patterns,
        }
    }
}

impl StaticAnalyzer for SecurityAnalyzer {
    fn analyze(&self, _manifest: &CapabilityManifest, rtfs_code: &str) -> Vec<ValidationIssue> {
        let mut issues = Vec::new();

        // Check for dangerous function calls
        for func in &self.dangerous_functions {
            if rtfs_code.contains(&format!("(call :{}", func)) {
                issues.push(ValidationIssue {
                    severity: IssueSeverity::Critical,
                    category: IssueCategory::Security,
                    description: format!("Dangerous function '{}' detected in capability", func),
                    location: Some("RTFS code".to_string()),
                    suggestion: Some("Remove or replace with safer alternatives".to_string()),
                    code: "SEC_ANALYZER001".to_string(),
                });
            }
        }

        // Check for insecure HTTP calls
        if rtfs_code.contains("(call :http") && !rtfs_code.contains("https://") {
            issues.push(ValidationIssue {
                severity: IssueSeverity::High,
                category: IssueCategory::Security,
                description: "HTTP calls should use HTTPS for security".to_string(),
                location: Some("RTFS code".to_string()),
                suggestion: Some("Use HTTPS URLs for all HTTP calls".to_string()),
                code: "SEC_ANALYZER002".to_string(),
            });
        }

        // Check for authentication requirements
        if rtfs_code.contains("(call :http") && !rtfs_code.contains(":ccos.auth.inject") {
            issues.push(ValidationIssue {
                severity: IssueSeverity::Medium,
                category: IssueCategory::Security,
                description: "External HTTP calls should use authentication".to_string(),
                location: Some("RTFS code".to_string()),
                suggestion: Some(
                    "Use (call :ccos.auth.inject ...) for authenticated requests".to_string(),
                ),
                code: "SEC_ANALYZER003".to_string(),
            });
        }

        // Check for hardcoded credentials
        let credential_patterns = vec!["password", "secret", "key", "token"];
        for pattern in credential_patterns {
            if rtfs_code.contains(&format!("\"{}\"", pattern))
                || rtfs_code.contains(&format!(":{}\"", pattern))
            {
                issues.push(ValidationIssue {
                    severity: IssueSeverity::Critical,
                    category: IssueCategory::Security,
                    description: format!("Potential hardcoded credential '{}' detected", pattern),
                    location: Some("RTFS code".to_string()),
                    suggestion: Some(
                        "Use environment variables or secure credential storage".to_string(),
                    ),
                    code: "SEC_ANALYZER004".to_string(),
                });
            }
        }

        issues
    }

    fn name(&self) -> &str {
        &self.name
    }

    fn description(&self) -> &str {
        "Analyzes RTFS code for syntax errors and compliance"
    }
}

/// Performance analyzer for RTFS code
#[allow(dead_code)]
pub struct PerformanceAnalyzer {
    name: String,
    max_nested_depth: usize,
    max_loop_iterations: u32,
}

impl PerformanceAnalyzer {
    pub fn new() -> Self {
        Self {
            name: "Performance Analyzer".to_string(),
            max_nested_depth: 10,
            max_loop_iterations: 1000,
        }
    }
}

impl StaticAnalyzer for PerformanceAnalyzer {
    fn analyze(&self, _manifest: &CapabilityManifest, rtfs_code: &str) -> Vec<ValidationIssue> {
        let mut issues = Vec::new();

        // Check nesting depth
        let max_depth = self.calculate_nesting_depth(rtfs_code);
        if max_depth > self.max_nested_depth {
            issues.push(ValidationIssue {
                severity: IssueSeverity::Medium,
                category: IssueCategory::Performance,
                description: format!(
                    "Excessive nesting depth: {} > {}",
                    max_depth, self.max_nested_depth
                ),
                location: Some("RTFS code".to_string()),
                suggestion: Some("Refactor to reduce nesting complexity".to_string()),
                code: "PERF_ANALYZER001".to_string(),
            });
        }

        // Check for potential infinite loops
        if rtfs_code.contains("(while true") || rtfs_code.contains("(loop") {
            issues.push(ValidationIssue {
                severity: IssueSeverity::High,
                category: IssueCategory::Performance,
                description: "Potential infinite loop detected".to_string(),
                location: Some("RTFS code".to_string()),
                suggestion: Some("Add termination conditions to loops".to_string()),
                code: "PERF_ANALYZER002".to_string(),
            });
        }

        // Check for excessive external calls
        let external_calls =
            rtfs_code.matches("(call :http").count() + rtfs_code.matches("(call :database").count();
        if external_calls > 20 {
            issues.push(ValidationIssue {
                severity: IssueSeverity::Medium,
                category: IssueCategory::Performance,
                description: format!("High number of external calls: {}", external_calls),
                location: Some("RTFS code".to_string()),
                suggestion: Some("Consider batching or caching external calls".to_string()),
                code: "PERF_ANALYZER003".to_string(),
            });
        }

        // Check for recursive patterns without base cases
        if rtfs_code.contains("(defn") && rtfs_code.contains("(call ") {
            // Simple heuristic: if a function calls itself without obvious termination
            let lines: Vec<&str> = rtfs_code.lines().collect();
            for (i, line) in lines.iter().enumerate() {
                if line.contains("(defn") {
                    // Look for function name and check if it calls itself
                    if let Some(func_name) = self.extract_function_name(line) {
                        for j in (i + 1)..lines.len() {
                            if lines[j].contains(&format!("(call :{}", func_name)) {
                                issues.push(ValidationIssue {
                                    severity: IssueSeverity::Medium,
                                    category: IssueCategory::Performance,
                                    description: format!(
                                        "Recursive function '{}' without obvious termination",
                                        func_name
                                    ),
                                    location: Some(format!("Line {}", j + 1)),
                                    suggestion: Some(
                                        "Ensure recursive function has proper base case"
                                            .to_string(),
                                    ),
                                    code: "PERF_ANALYZER004".to_string(),
                                });
                                break;
                            }
                        }
                    }
                }
            }
        }

        issues
    }

    fn name(&self) -> &str {
        &self.name
    }

    fn description(&self) -> &str {
        "Analyzes RTFS code for syntax errors and compliance"
    }
}

impl PerformanceAnalyzer {
    fn calculate_nesting_depth(&self, code: &str) -> usize {
        let mut max_depth = 0;
        let mut current_depth: usize = 0;

        for c in code.chars() {
            match c {
                '(' => {
                    current_depth += 1;
                    max_depth = max_depth.max(current_depth);
                }
                ')' => {
                    current_depth = current_depth.saturating_sub(1);
                }
                _ => {}
            }
        }

        max_depth
    }

    fn extract_function_name(&self, line: &str) -> Option<String> {
        // Simple extraction of function name from defn line
        if let Some(start) = line.find("(defn ") {
            let after_defn = &line[start + 6..];
            if let Some(end) = after_defn.find(' ') {
                return Some(after_defn[..end].to_string());
            }
        }
        None
    }
}

/// Dependency analyzer for RTFS code
pub struct DependencyAnalyzer {
    name: String,
}

impl DependencyAnalyzer {
    pub fn new() -> Self {
        Self {
            name: "Dependency Analyzer".to_string(),
        }
    }
}

impl StaticAnalyzer for DependencyAnalyzer {
    fn analyze(&self, _manifest: &CapabilityManifest, rtfs_code: &str) -> Vec<ValidationIssue> {
        let mut issues = Vec::new();

        // Extract all capability dependencies
        let mut dependencies = HashSet::new();
        let lines: Vec<&str> = rtfs_code.lines().collect();

        for line in &lines {
            if line.contains("(call :") {
                if let Some(start) = line.find("(call :") {
                    let after_call = &line[start + 7..];
                    if let Some(end) = after_call.find(|c: char| c.is_whitespace() || c == ')') {
                        let dep = &after_call[..end];
                        dependencies.insert(dep.to_string());
                    }
                }
            }
        }

        // Check for circular dependencies (simplified)
        if dependencies.len() > 10 {
            issues.push(ValidationIssue {
                severity: IssueSeverity::Medium,
                category: IssueCategory::Dependencies,
                description: format!("High number of dependencies: {}", dependencies.len()),
                location: Some("RTFS code".to_string()),
                suggestion: Some(
                    "Consider reducing dependencies or splitting capability".to_string(),
                ),
                code: "DEP_ANALYZER001".to_string(),
            });
        }

        // Check for external dependencies that might not be available
        for dep in &dependencies {
            if dep.starts_with("external.") || dep.starts_with("third-party.") {
                issues.push(ValidationIssue {
                    severity: IssueSeverity::High,
                    category: IssueCategory::Dependencies,
                    description: format!("External dependency '{}' may not be available", dep),
                    location: Some("RTFS code".to_string()),
                    suggestion: Some(
                        "Ensure external dependencies are properly registered".to_string(),
                    ),
                    code: "DEP_ANALYZER002".to_string(),
                });
            }
        }

        // Check for deprecated capabilities
        let deprecated_capabilities = vec!["old.capability", "legacy.api", "deprecated.service"];
        for dep in &dependencies {
            for deprecated in &deprecated_capabilities {
                if dep.contains(deprecated) {
                    issues.push(ValidationIssue {
                        severity: IssueSeverity::Medium,
                        category: IssueCategory::Dependencies,
                        description: format!("Deprecated dependency '{}' detected", dep),
                        location: Some("RTFS code".to_string()),
                        suggestion: Some("Update to use current version of capability".to_string()),
                        code: "DEP_ANALYZER003".to_string(),
                    });
                }
            }
        }

        issues
    }

    fn name(&self) -> &str {
        &self.name
    }

    fn description(&self) -> &str {
        "Analyzes RTFS code for syntax errors and compliance"
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::capability_marketplace::types::EffectType;

    fn create_test_manifest() -> CapabilityManifest {
        CapabilityManifest {
            id: "test.capability.v1".to_string(),
            name: "Test Capability".to_string(),
            description: "A test capability".to_string(),
            version: "1.0.0".to_string(),
            provider: crate::capability_marketplace::types::ProviderType::Local(
                crate::capability_marketplace::types::LocalCapability {
                    handler: std::sync::Arc::new(|_| {
                        Ok(rtfs::runtime::values::Value::String("test".to_string()))
                    }),
                },
            ),
            input_schema: None,
            output_schema: None,
            attestation: None,
            provenance: None,
            permissions: vec![],
            effects: vec![],
            metadata: std::collections::HashMap::new(),
            agent_metadata: None,
            domains: Vec::new(),
            categories: Vec::new(),
            effect_type: EffectType::default(),
        }
    }

    #[test]
    fn test_rtfs_syntax_analyzer() {
        let analyzer = RtfsSyntaxAnalyzer::new();
        let manifest = create_test_manifest();
        // Missing opening paren - should trigger SYNTAX001
        // Unbalanced parens - should trigger SYNTAX002
        let rtfs_code = r#"
            capability test
                :description "Test capability"
                :implementation
                (do (print "Hello")
        "#;

        let issues = analyzer.analyze(&manifest, rtfs_code);
        assert!(issues.iter().any(|i| i.code == "SYNTAX001")); // Missing opening parenthesis
        assert!(issues.iter().any(|i| i.code == "SYNTAX002")); // Unbalanced parentheses
    }

    #[test]
    fn test_security_analyzer() {
        let analyzer = SecurityAnalyzer::new();
        let manifest = create_test_manifest();
        let rtfs_code = r#"
            (capability test
                :description "Test capability"
                :implementation
                (do
                    (call :eval "malicious_code")
                    (call :http.post {:url "http://insecure.com"})
                    (let my-var "secret")
                )
            )
        "#;

        let issues = analyzer.analyze(&manifest, rtfs_code);
        assert!(issues.iter().any(|i| i.code == "SEC_ANALYZER001")); // Dangerous function
        assert!(issues.iter().any(|i| i.code == "SEC_ANALYZER002")); // Insecure HTTP
        assert!(issues.iter().any(|i| i.code == "SEC_ANALYZER004")); // Hardcoded credential
    }

    #[test]
    fn test_performance_analyzer() {
        let analyzer = PerformanceAnalyzer::new();
        let manifest = create_test_manifest();
        let rtfs_code = r#"
            (capability test
                :description "Test capability"
                :implementation
                (do
                    (while true
                        (call :http.get {:url "https://api.com"})
                    )
                )
            )
        "#;

        let issues = analyzer.analyze(&manifest, rtfs_code);
        assert!(issues.iter().any(|i| i.code == "PERF_ANALYZER002")); // Infinite loop
    }

    #[test]
    fn test_dependency_analyzer() {
        let analyzer = DependencyAnalyzer::new();
        let manifest = create_test_manifest();
        let rtfs_code = r#"
            (capability test
                :description "Test capability"
                :implementation
                (do
                    (call :external.service)
                    (call :old.capability)
                    (call :http.get {:url "https://api.com"})
                )
            )
        "#;

        let issues = analyzer.analyze(&manifest, rtfs_code);
        assert!(issues.iter().any(|i| i.code == "DEP_ANALYZER002")); // External dependency
        assert!(issues.iter().any(|i| i.code == "DEP_ANALYZER003")); // Deprecated dependency
    }
}
