//! Validation Harness and Governance Gates
//!
//! This module implements Phase 5 of the missing capability resolution plan:
//! - Pre-flight validation of synthesized capabilities
//! - Governance gates for security, compliance, and quality
//! - Static analysis and security scanning
//! - Capability attestation and provenance tracking

use crate::ccos::capability_marketplace::types::{
    CapabilityAttestation, CapabilityManifest, CapabilityProvenance,
};
// Removed unused imports
use std::collections::HashMap;

/// Validation result for a capability
#[derive(Debug, Clone)]
pub struct ValidationResult {
    /// Overall validation status
    pub status: ValidationStatus,
    /// List of validation issues found
    pub issues: Vec<ValidationIssue>,
    /// Security score (0.0 to 1.0)
    pub security_score: f64,
    /// Quality score (0.0 to 1.0)
    pub quality_score: f64,
    /// Compliance score (0.0 to 1.0)
    pub compliance_score: f64,
    /// Validation metadata
    pub metadata: HashMap<String, String>,
}

/// Validation status
#[derive(Debug, Clone, PartialEq)]
pub enum ValidationStatus {
    /// Validation passed all checks
    Passed,
    /// Validation passed but with warnings
    PassedWithWarnings,
    /// Validation failed critical checks
    Failed,
    /// Validation failed security checks
    SecurityFailed,
    /// Validation failed compliance checks
    ComplianceFailed,
}

/// Individual validation issue
#[derive(Debug, Clone)]
pub struct ValidationIssue {
    /// Issue severity
    pub severity: IssueSeverity,
    /// Issue category
    pub category: IssueCategory,
    /// Issue description
    pub description: String,
    /// Issue location (file, line, etc.)
    pub location: Option<String>,
    /// Suggested fix
    pub suggestion: Option<String>,
    /// Issue code for programmatic handling
    pub code: String,
}

/// Issue severity levels
#[derive(Debug, Clone, PartialEq)]
pub enum IssueSeverity {
    /// Critical - blocks registration
    Critical,
    /// High - major concern
    High,
    /// Medium - should be addressed
    Medium,
    /// Low - minor issue
    Low,
    /// Info - informational
    Info,
}

/// Issue categories
#[derive(Debug, Clone, PartialEq)]
pub enum IssueCategory {
    /// Security-related issues
    Security,
    /// Code quality issues
    Quality,
    /// Compliance/regulatory issues
    Compliance,
    /// Performance issues
    Performance,
    /// Documentation issues
    Documentation,
    /// API design issues
    ApiDesign,
    /// Dependency issues
    Dependencies,
}

/// Validation harness for capabilities
pub struct ValidationHarness {
    /// Security validation rules
    security_rules: Vec<SecurityRule>,
    /// Quality validation rules
    quality_rules: Vec<QualityRule>,
    /// Compliance validation rules
    compliance_rules: Vec<ComplianceRule>,
    /// Static analysis tools
    static_analyzers: Vec<Box<dyn StaticAnalyzer>>,
    /// Governance policies
    governance_policies: Vec<Box<dyn GovernancePolicy>>,
}

/// Security validation rule
#[derive(Debug, Clone)]
pub struct SecurityRule {
    pub name: String,
    pub description: String,
    pub check: SecurityCheck,
}

/// Types of security checks
#[derive(Debug, Clone)]
pub enum SecurityCheck {
    /// Check for hardcoded secrets
    NoHardcodedSecrets,
    /// Check for SQL injection vulnerabilities
    NoSqlInjection,
    /// Check for XSS vulnerabilities
    NoXssVulnerabilities,
    /// Check for unsafe external calls
    NoUnsafeExternalCalls,
    /// Check for proper authentication
    RequiresAuthentication,
    /// Check for proper authorization
    RequiresAuthorization,
    /// Check for data encryption
    RequiresEncryption,
    /// Check for secure communication
    RequiresSecureTransport,
}

/// Quality validation rule
#[derive(Debug, Clone)]
pub struct QualityRule {
    pub name: String,
    pub description: String,
    pub check: QualityCheck,
}

/// Types of quality checks
#[derive(Debug, Clone)]
pub enum QualityCheck {
    /// Check for proper error handling
    ProperErrorHandling,
    /// Check for input validation
    InputValidation,
    /// Check for documentation completeness
    DocumentationComplete,
    /// Check for test coverage
    TestCoverage,
    /// Check for code complexity
    CodeComplexity,
    /// Check for performance requirements
    PerformanceRequirements,
}

/// Compliance validation rule
#[derive(Debug, Clone)]
pub struct ComplianceRule {
    pub name: String,
    pub description: String,
    pub check: ComplianceCheck,
}

/// Types of compliance checks
#[derive(Debug, Clone)]
pub enum ComplianceCheck {
    /// GDPR compliance
    GdprCompliance,
    /// SOX compliance
    SoxCompliance,
    /// HIPAA compliance
    HipaaCompliance,
    /// PCI DSS compliance
    PciDssCompliance,
    /// Data retention policies
    DataRetention,
    /// Audit trail requirements
    AuditTrail,
}

/// Static analyzer trait
pub trait StaticAnalyzer {
    /// Analyze the capability manifest and RTFS code
    fn analyze(&self, manifest: &CapabilityManifest, rtfs_code: &str) -> Vec<ValidationIssue>;
    /// Get analyzer name
    fn name(&self) -> &str;
}

/// Governance policy trait
pub trait GovernancePolicy {
    /// Check if the capability complies with this policy
    fn check_compliance(&self, manifest: &CapabilityManifest, rtfs_code: &str) -> ValidationResult;
    /// Get policy name
    fn name(&self) -> &str;
    /// Get policy severity
    fn severity(&self) -> IssueSeverity;
}

impl ValidationHarness {
    /// Create a new validation harness with default rules
    pub fn new() -> Self {
        let mut harness = Self {
            security_rules: Vec::new(),
            quality_rules: Vec::new(),
            compliance_rules: Vec::new(),
            static_analyzers: Vec::new(),
            governance_policies: Vec::new(),
        };

        harness.initialize_default_rules();
        harness
    }

    /// Initialize default validation rules
    fn initialize_default_rules(&mut self) {
        // Security rules
        self.security_rules.extend(vec![
            SecurityRule {
                name: "no_hardcoded_secrets".to_string(),
                description: "Capability must not contain hardcoded secrets or credentials"
                    .to_string(),
                check: SecurityCheck::NoHardcodedSecrets,
            },
            SecurityRule {
                name: "no_sql_injection".to_string(),
                description: "Capability must not be vulnerable to SQL injection".to_string(),
                check: SecurityCheck::NoSqlInjection,
            },
            SecurityRule {
                name: "requires_auth".to_string(),
                description: "Capability must implement proper authentication".to_string(),
                check: SecurityCheck::RequiresAuthentication,
            },
        ]);

        // Quality rules
        self.quality_rules.extend(vec![
            QualityRule {
                name: "proper_error_handling".to_string(),
                description: "Capability must have proper error handling".to_string(),
                check: QualityCheck::ProperErrorHandling,
            },
            QualityRule {
                name: "input_validation".to_string(),
                description: "Capability must validate all inputs".to_string(),
                check: QualityCheck::InputValidation,
            },
            QualityRule {
                name: "documentation_complete".to_string(),
                description: "Capability must have complete documentation".to_string(),
                check: QualityCheck::DocumentationComplete,
            },
        ]);

        // Compliance rules
        self.compliance_rules.extend(vec![
            ComplianceRule {
                name: "gdpr_compliance".to_string(),
                description: "Capability must comply with GDPR requirements".to_string(),
                check: ComplianceCheck::GdprCompliance,
            },
            ComplianceRule {
                name: "audit_trail".to_string(),
                description: "Capability must maintain audit trail".to_string(),
                check: ComplianceCheck::AuditTrail,
            },
        ]);
    }

    /// Validate a capability manifest and RTFS code
    pub fn validate_capability(
        &self,
        manifest: &CapabilityManifest,
        rtfs_code: &str,
    ) -> ValidationResult {
        let mut issues = Vec::new();
        let mut metadata = HashMap::new();

        // Run security validation
        let security_issues = self.validate_security(manifest, rtfs_code);
        issues.extend(security_issues);

        // Run quality validation
        let quality_issues = self.validate_quality(manifest, rtfs_code);
        issues.extend(quality_issues);

        // Run compliance validation
        let compliance_issues = self.validate_compliance(manifest, rtfs_code);
        issues.extend(compliance_issues);

        // Run static analysis
        for analyzer in &self.static_analyzers {
            let analyzer_issues = analyzer.analyze(manifest, rtfs_code);
            issues.extend(analyzer_issues);
        }

        // Run governance policies
        for policy in &self.governance_policies {
            let policy_result = policy.check_compliance(manifest, rtfs_code);
            issues.extend(policy_result.issues);
        }

        // Calculate scores
        let security_score = self.calculate_security_score(&issues);
        let quality_score = self.calculate_quality_score(&issues);
        let compliance_score = self.calculate_compliance_score(&issues);

        // Determine overall status
        let status = self.determine_validation_status(&issues);

        // Add metadata
        metadata.insert(
            "validation_timestamp".to_string(),
            chrono::Utc::now().to_rfc3339(),
        );
        metadata.insert("total_issues".to_string(), issues.len().to_string());
        metadata.insert(
            "security_issues".to_string(),
            issues
                .iter()
                .filter(|i| i.category == IssueCategory::Security)
                .count()
                .to_string(),
        );
        metadata.insert(
            "quality_issues".to_string(),
            issues
                .iter()
                .filter(|i| i.category == IssueCategory::Quality)
                .count()
                .to_string(),
        );
        metadata.insert(
            "compliance_issues".to_string(),
            issues
                .iter()
                .filter(|i| i.category == IssueCategory::Compliance)
                .count()
                .to_string(),
        );

        ValidationResult {
            status,
            issues,
            security_score,
            quality_score,
            compliance_score,
            metadata,
        }
    }

    /// Validate security requirements
    fn validate_security(
        &self,
        manifest: &CapabilityManifest,
        rtfs_code: &str,
    ) -> Vec<ValidationIssue> {
        let mut issues = Vec::new();

        for rule in &self.security_rules {
            match &rule.check {
                SecurityCheck::NoHardcodedSecrets => {
                    if self.has_hardcoded_secrets(rtfs_code) {
                        issues.push(ValidationIssue {
                            severity: IssueSeverity::Critical,
                            category: IssueCategory::Security,
                            description: format!(
                                "Security rule '{}' failed: {}",
                                rule.name, rule.description
                            ),
                            location: Some("RTFS code".to_string()),
                            suggestion: Some(
                                "Use environment variables or secure credential storage"
                                    .to_string(),
                            ),
                            code: "SEC001".to_string(),
                        });
                    }
                }
                SecurityCheck::NoSqlInjection => {
                    if self.has_sql_injection_vulnerabilities(rtfs_code) {
                        issues.push(ValidationIssue {
                            severity: IssueSeverity::High,
                            category: IssueCategory::Security,
                            description: format!(
                                "Security rule '{}' failed: {}",
                                rule.name, rule.description
                            ),
                            location: Some("RTFS code".to_string()),
                            suggestion: Some(
                                "Use parameterized queries and input sanitization".to_string(),
                            ),
                            code: "SEC002".to_string(),
                        });
                    }
                }
                SecurityCheck::RequiresAuthentication => {
                    if self.requires_authentication_check(manifest, rtfs_code) {
                        if !self.has_authentication(manifest, rtfs_code) {
                            issues.push(ValidationIssue {
                                severity: IssueSeverity::High,
                                category: IssueCategory::Security,
                                description: format!(
                                    "Security rule '{}' failed: {}",
                                    rule.name, rule.description
                                ),
                                location: Some("Capability manifest".to_string()),
                                suggestion: Some(
                                    "Implement authentication using auth_injector".to_string(),
                                ),
                                code: "SEC003".to_string(),
                            });
                        }
                    }
                }
                _ => {
                    // Placeholder for other security checks
                }
            }
        }

        issues
    }

    /// Validate quality requirements
    fn validate_quality(
        &self,
        manifest: &CapabilityManifest,
        rtfs_code: &str,
    ) -> Vec<ValidationIssue> {
        let mut issues = Vec::new();

        for rule in &self.quality_rules {
            match &rule.check {
                QualityCheck::ProperErrorHandling => {
                    if !self.has_proper_error_handling(rtfs_code) {
                        issues.push(ValidationIssue {
                            severity: IssueSeverity::Medium,
                            category: IssueCategory::Quality,
                            description: format!(
                                "Quality rule '{}' failed: {}",
                                rule.name, rule.description
                            ),
                            location: Some("RTFS code".to_string()),
                            suggestion: Some(
                                "Add proper error handling with try-catch blocks".to_string(),
                            ),
                            code: "QUAL001".to_string(),
                        });
                    }
                }
                QualityCheck::InputValidation => {
                    if !self.has_input_validation(manifest, rtfs_code) {
                        issues.push(ValidationIssue {
                            severity: IssueSeverity::Medium,
                            category: IssueCategory::Quality,
                            description: format!(
                                "Quality rule '{}' failed: {}",
                                rule.name, rule.description
                            ),
                            location: Some("Capability manifest".to_string()),
                            suggestion: Some(
                                "Add input validation to capability parameters".to_string(),
                            ),
                            code: "QUAL002".to_string(),
                        });
                    }
                }
                QualityCheck::DocumentationComplete => {
                    if !self.has_complete_documentation(manifest) {
                        issues.push(ValidationIssue {
                            severity: IssueSeverity::Low,
                            category: IssueCategory::Documentation,
                            description: format!(
                                "Quality rule '{}' failed: {}",
                                rule.name, rule.description
                            ),
                            location: Some("Capability manifest".to_string()),
                            suggestion: Some(
                                "Add comprehensive documentation to capability".to_string(),
                            ),
                            code: "QUAL003".to_string(),
                        });
                    }
                }
                _ => {
                    // Placeholder for other quality checks
                }
            }
        }

        issues
    }

    /// Validate compliance requirements
    fn validate_compliance(
        &self,
        manifest: &CapabilityManifest,
        rtfs_code: &str,
    ) -> Vec<ValidationIssue> {
        let mut issues = Vec::new();

        for rule in &self.compliance_rules {
            match &rule.check {
                ComplianceCheck::GdprCompliance => {
                    if self.requires_gdpr_compliance(manifest)
                        && !self.is_gdpr_compliant(manifest, rtfs_code)
                    {
                        issues.push(ValidationIssue {
                            severity: IssueSeverity::Critical,
                            category: IssueCategory::Compliance,
                            description: format!(
                                "Compliance rule '{}' failed: {}",
                                rule.name, rule.description
                            ),
                            location: Some("Capability manifest".to_string()),
                            suggestion: Some("Implement GDPR compliance measures".to_string()),
                            code: "COMP001".to_string(),
                        });
                    }
                }
                ComplianceCheck::AuditTrail => {
                    if !self.has_audit_trail(manifest, rtfs_code) {
                        issues.push(ValidationIssue {
                            severity: IssueSeverity::High,
                            category: IssueCategory::Compliance,
                            description: format!(
                                "Compliance rule '{}' failed: {}",
                                rule.name, rule.description
                            ),
                            location: Some("Capability manifest".to_string()),
                            suggestion: Some("Implement audit trail logging".to_string()),
                            code: "COMP002".to_string(),
                        });
                    }
                }
                _ => {
                    // Placeholder for other compliance checks
                }
            }
        }

        issues
    }

    /// Check for hardcoded secrets in RTFS code
    fn has_hardcoded_secrets(&self, rtfs_code: &str) -> bool {
        // Simple heuristic checks for common secret patterns
        let secret_patterns = vec![
            "password",
            "secret",
            "key",
            "token",
            "credential",
            "api_key",
            "access_token",
            "private_key",
            "auth_token",
        ];

        let code_lower = rtfs_code.to_lowercase();
        secret_patterns.iter().any(|pattern| {
            code_lower.contains(&format!(":{}", pattern))
                || code_lower.contains(&format!("{}=\"", pattern))
        })
    }

    /// Check for SQL injection vulnerabilities
    fn has_sql_injection_vulnerabilities(&self, rtfs_code: &str) -> bool {
        // Simple heuristic for SQL injection patterns
        rtfs_code.contains("SELECT")
            || rtfs_code.contains("INSERT")
            || rtfs_code.contains("UPDATE")
            || rtfs_code.contains("DELETE")
    }

    /// Check if capability requires authentication
    fn requires_authentication_check(
        &self,
        manifest: &CapabilityManifest,
        rtfs_code: &str,
    ) -> bool {
        // Check if capability handles sensitive data or external calls
        rtfs_code.contains("(call :http")
            || rtfs_code.contains("(call :database")
            || manifest.description.to_lowercase().contains("authenticate")
            || manifest.description.to_lowercase().contains("login")
    }

    /// Check if capability has authentication
    fn has_authentication(&self, manifest: &CapabilityManifest, rtfs_code: &str) -> bool {
        rtfs_code.contains("(call :ccos.auth.inject")
            || manifest.metadata.contains_key("requires_auth")
    }

    /// Check if capability has proper error handling
    fn has_proper_error_handling(&self, rtfs_code: &str) -> bool {
        rtfs_code.contains("try") || rtfs_code.contains("catch") || rtfs_code.contains("error")
    }

    /// Check if capability has input validation
    fn has_input_validation(&self, manifest: &CapabilityManifest, rtfs_code: &str) -> bool {
        // Check if parameters have type annotations
        manifest.input_schema.is_some() || rtfs_code.contains(":expects")
    }

    /// Check if capability has complete documentation
    fn has_complete_documentation(&self, manifest: &CapabilityManifest) -> bool {
        !manifest.description.is_empty() && manifest.description.len() > 20
    }

    /// Check if capability requires GDPR compliance
    fn requires_gdpr_compliance(&self, manifest: &CapabilityManifest) -> bool {
        manifest.description.to_lowercase().contains("personal")
            || manifest.description.to_lowercase().contains("user data")
            || manifest.description.to_lowercase().contains("privacy")
    }

    /// Check if capability is GDPR compliant
    fn is_gdpr_compliant(&self, manifest: &CapabilityManifest, rtfs_code: &str) -> bool {
        // Simple GDPR compliance checks
        rtfs_code.contains("data_protection")
            || rtfs_code.contains("consent")
            || manifest.metadata.contains_key("gdpr_compliant")
    }

    /// Check if capability has audit trail
    fn has_audit_trail(&self, manifest: &CapabilityManifest, rtfs_code: &str) -> bool {
        rtfs_code.contains("audit")
            || rtfs_code.contains("log")
            || manifest.metadata.contains_key("audit_enabled")
    }

    /// Calculate security score
    fn calculate_security_score(&self, issues: &[ValidationIssue]) -> f64 {
        let security_issues: Vec<&ValidationIssue> = issues
            .iter()
            .filter(|i| i.category == IssueCategory::Security)
            .collect();

        if security_issues.is_empty() {
            return 1.0;
        }

        let critical_count = security_issues
            .iter()
            .filter(|i| i.severity == IssueSeverity::Critical)
            .count();
        let high_count = security_issues
            .iter()
            .filter(|i| i.severity == IssueSeverity::High)
            .count();

        // Penalize based on severity
        let penalty = (critical_count as f64 * 0.4) + (high_count as f64 * 0.2);
        1.0 - penalty.min(1.0)
    }

    /// Calculate quality score
    fn calculate_quality_score(&self, issues: &[ValidationIssue]) -> f64 {
        let quality_issues: Vec<&ValidationIssue> = issues
            .iter()
            .filter(|i| i.category == IssueCategory::Quality)
            .collect();

        if quality_issues.is_empty() {
            return 1.0;
        }

        let total_penalty = quality_issues
            .iter()
            .map(|i| match i.severity {
                IssueSeverity::Critical => 0.3,
                IssueSeverity::High => 0.2,
                IssueSeverity::Medium => 0.1,
                IssueSeverity::Low => 0.05,
                IssueSeverity::Info => 0.01,
            })
            .sum::<f64>();

        1.0 - total_penalty.min(1.0)
    }

    /// Calculate compliance score
    fn calculate_compliance_score(&self, issues: &[ValidationIssue]) -> f64 {
        let compliance_issues: Vec<&ValidationIssue> = issues
            .iter()
            .filter(|i| i.category == IssueCategory::Compliance)
            .collect();

        if compliance_issues.is_empty() {
            return 1.0;
        }

        let critical_count = compliance_issues
            .iter()
            .filter(|i| i.severity == IssueSeverity::Critical)
            .count();
        let high_count = compliance_issues
            .iter()
            .filter(|i| i.severity == IssueSeverity::High)
            .count();

        let penalty = (critical_count as f64 * 0.5) + (high_count as f64 * 0.3);
        1.0 - penalty.min(1.0)
    }

    /// Determine overall validation status
    fn determine_validation_status(&self, issues: &[ValidationIssue]) -> ValidationStatus {
        let critical_issues: Vec<&ValidationIssue> = issues
            .iter()
            .filter(|i| i.severity == IssueSeverity::Critical)
            .collect();

        let security_critical = critical_issues
            .iter()
            .any(|i| i.category == IssueCategory::Security);
        let compliance_critical = critical_issues
            .iter()
            .any(|i| i.category == IssueCategory::Compliance);

        if security_critical {
            ValidationStatus::SecurityFailed
        } else if compliance_critical {
            ValidationStatus::ComplianceFailed
        } else if !critical_issues.is_empty() {
            ValidationStatus::Failed
        } else if issues
            .iter()
            .any(|i| i.severity == IssueSeverity::High || i.severity == IssueSeverity::Medium)
        {
            ValidationStatus::PassedWithWarnings
        } else {
            ValidationStatus::Passed
        }
    }

    /// Add a static analyzer
    pub fn add_static_analyzer(&mut self, analyzer: Box<dyn StaticAnalyzer>) {
        self.static_analyzers.push(analyzer);
    }

    /// Add a governance policy
    pub fn add_governance_policy(&mut self, policy: Box<dyn GovernancePolicy>) {
        self.governance_policies.push(policy);
    }

    /// Create attestation from validation result
    pub fn create_attestation(
        &self,
        validation_result: &ValidationResult,
    ) -> CapabilityAttestation {
        let mut metadata = validation_result.metadata.clone();
        metadata.insert(
            "validator_id".to_string(),
            "validation_harness_v1".to_string(),
        );
        metadata.insert(
            "security_score".to_string(),
            validation_result.security_score.to_string(),
        );
        metadata.insert(
            "quality_score".to_string(),
            validation_result.quality_score.to_string(),
        );
        metadata.insert(
            "compliance_score".to_string(),
            validation_result.compliance_score.to_string(),
        );
        metadata.insert(
            "validation_status".to_string(),
            format!("{:?}", validation_result.status),
        );
        metadata.insert(
            "issues_count".to_string(),
            validation_result.issues.len().to_string(),
        );

        CapabilityAttestation {
            signature: "validation_harness_signature".to_string(),
            authority: "validation_harness_v1".to_string(),
            created_at: chrono::Utc::now(),
            expires_at: Some(chrono::Utc::now() + chrono::Duration::days(365)),
            metadata,
        }
    }

    /// Create provenance from validation result
    pub fn create_provenance(&self, validation_result: &ValidationResult) -> CapabilityProvenance {
        CapabilityProvenance {
            version: Some("1.0".to_string()),
            content_hash: "".to_string(), // Will be filled by caller
            custody_chain: vec!["validation_harness".to_string()],
            registered_at: chrono::Utc::now(),
            source: "synthesized".to_string(),
        }
    }
}

impl Default for ValidationHarness {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ccos::capability_marketplace::types::*;

    fn create_test_manifest() -> CapabilityManifest {
        CapabilityManifest {
            id: "test.capability.v1".to_string(),
            name: "Test Capability".to_string(),
            description: "A test capability for validation".to_string(),
            version: "1.0.0".to_string(),
            provider: ProviderType::Local(LocalCapability {
                handler: std::sync::Arc::new(|_| {
                    Ok(crate::runtime::values::Value::String("test".to_string()))
                }),
            }),
            input_schema: None,
            output_schema: None,
            attestation: None,
            provenance: None,
            permissions: vec![],
            effects: vec![],
            metadata: HashMap::new(),
            agent_metadata: None,
        }
    }

    #[test]
    fn test_validation_harness_creation() {
        let harness = ValidationHarness::new();
        assert!(!harness.security_rules.is_empty());
        assert!(!harness.quality_rules.is_empty());
        assert!(!harness.compliance_rules.is_empty());
    }

    #[test]
    fn test_security_validation_hardcoded_secrets() {
        let harness = ValidationHarness::new();
        let manifest = create_test_manifest();
        let rtfs_code = r#"
            (capability test
                :description "Test with hardcoded secret"
                :implementation
                (do
                    (let password "secret123")
                    (call :http.post {:url "https://api.example.com" :password password})
                )
            )
        "#;

        let result = harness.validate_capability(&manifest, rtfs_code);
        assert!(result.security_score < 1.0);
        assert!(result.issues.iter().any(|i| i.code == "SEC001"));
    }

    #[test]
    fn test_quality_validation_documentation() {
        let harness = ValidationHarness::new();
        let mut manifest = create_test_manifest();
        manifest.description = "".to_string(); // Empty description

        let result = harness.validate_capability(&manifest, "");
        assert!(result.issues.iter().any(|i| i.code == "QUAL003"));
    }

    #[test]
    fn test_validation_status_determination() {
        let harness = ValidationHarness::new();

        // Test passed status
        let manifest = create_test_manifest();
        let rtfs_code = r#"
            (capability test
                :description "Well-documented capability with proper implementation"
                :expects (map :url string)
                :implementation
                (do
                    (try
                        (call :ccos.auth.inject {:auth "env"})
                        (call :http.get {:url "https://api.example.com"})
                        (catch error (log error)))
                )
            )
        "#;

        let result = harness.validate_capability(&manifest, rtfs_code);
        assert_eq!(result.status, ValidationStatus::Passed);
    }

    #[test]
    fn test_attestation_creation() {
        let harness = ValidationHarness::new();
        let manifest = create_test_manifest();
        let result = harness.validate_capability(&manifest, "");

        let attestation = harness.create_attestation(&result);
        assert_eq!(attestation.authority, "validation_harness_v1");
        assert!(!attestation.signature.is_empty());
    }
}
