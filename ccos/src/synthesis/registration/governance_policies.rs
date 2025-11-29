//! Governance Policies for Capability Validation
//!
//! This module implements governance policies that enforce organizational
//! rules, compliance requirements, and security standards for capabilities.

use super::validation_harness::{IssueCategory, IssueSeverity, ValidationIssue, ValidationResult};
use crate::capability_marketplace::types::CapabilityManifest;
use std::collections::HashMap;

/// Governance policy trait for capability validation
pub trait GovernancePolicy {
    /// Check compliance against this policy
    fn check_compliance(&self, manifest: &CapabilityManifest, rtfs_code: &str) -> ValidationResult;

    /// Get policy name
    fn policy_name(&self) -> &str;

    /// Get policy description
    fn policy_description(&self) -> &str;
}

/// Maximum parameter count policy
pub struct MaxParameterCountPolicy {
    pub max_parameters: u32,
}

impl MaxParameterCountPolicy {
    pub fn new(max_parameters: u32) -> Self {
        Self { max_parameters }
    }
}

impl GovernancePolicy for MaxParameterCountPolicy {
    fn check_compliance(
        &self,
        manifest: &CapabilityManifest,
        _rtfs_code: &str,
    ) -> ValidationResult {
        let mut issues = Vec::new();

        // Get parameter count from metadata
        let param_count = manifest
            .metadata
            .get("parameter_count")
            .and_then(|s| s.parse::<u32>().ok())
            .unwrap_or(0);

        if param_count > self.max_parameters {
            issues.push(ValidationIssue {
                severity: IssueSeverity::Medium,
                category: IssueCategory::Quality,
                description: format!(
                    "Capability has {} parameters, exceeding limit of {}",
                    param_count, self.max_parameters
                ),
                location: Some("manifest".to_string()),
                suggestion: Some("Consider breaking down into smaller capabilities".to_string()),
                code: "MAX_PARAM_COUNT".to_string(),
            });
        }

        ValidationResult {
            status: if issues.is_empty() {
                super::validation_harness::ValidationStatus::Passed
            } else {
                super::validation_harness::ValidationStatus::PassedWithWarnings
            },
            issues,
            security_score: 1.0,
            quality_score: if param_count <= self.max_parameters {
                1.0
            } else {
                0.8
            },
            compliance_score: 1.0,
            metadata: HashMap::new(),
        }
    }

    fn policy_name(&self) -> &str {
        "max_parameter_count"
    }

    fn policy_description(&self) -> &str {
        "Enforces maximum parameter count for capabilities"
    }
}

/// Enterprise security policy
pub struct EnterpriseSecurityPolicy {
    pub name: String,
    pub require_encryption: bool,
    pub require_audit_logging: bool,
    pub allowed_external_domains: Vec<String>,
    pub blocked_keywords: Vec<String>,
}

impl EnterpriseSecurityPolicy {
    pub fn new() -> Self {
        Self {
            name: "Enterprise Security Policy".to_string(),
            require_encryption: true,
            require_audit_logging: true,
            allowed_external_domains: vec![
                "api.github.com".to_string(),
                "api.openai.com".to_string(),
                "registry.modelcontextprotocol.io".to_string(),
            ],
            blocked_keywords: vec![
                "admin".to_string(),
                "root".to_string(),
                "sudo".to_string(),
                "system".to_string(),
            ],
        }
    }
}

impl GovernancePolicy for EnterpriseSecurityPolicy {
    fn check_compliance(&self, manifest: &CapabilityManifest, rtfs_code: &str) -> ValidationResult {
        let mut issues = Vec::new();

        // Check for blocked keywords in capability name
        let name_lower = manifest.name.to_lowercase();
        for keyword in &self.blocked_keywords {
            if name_lower.contains(keyword) {
                issues.push(ValidationIssue {
                    severity: IssueSeverity::Critical,
                    category: IssueCategory::Security,
                    description: format!("Capability name contains blocked keyword: '{}'", keyword),
                    location: Some(format!("Capability name: {}", manifest.name)),
                    suggestion: Some(
                        "Use a more appropriate name without privileged keywords".to_string(),
                    ),
                    code: "GOV001".to_string(),
                });
            }
        }

        // Check for external domain restrictions
        if rtfs_code.contains("(call :http") {
            let mut has_allowed_domain = false;
            for domain in &self.allowed_external_domains {
                if rtfs_code.contains(domain) {
                    has_allowed_domain = true;
                    break;
                }
            }

            if !has_allowed_domain {
                issues.push(ValidationIssue {
                    severity: IssueSeverity::High,
                    category: IssueCategory::Security,
                    description: "Capability makes HTTP calls to non-approved domains".to_string(),
                    location: Some("RTFS code".to_string()),
                    suggestion: Some(
                        "Use only approved external domains or request approval".to_string(),
                    ),
                    code: "GOV002".to_string(),
                });
            }
        }

        // Check for encryption requirements
        if self.require_encryption
            && rtfs_code.contains("(call :http")
            && !rtfs_code.contains("https://")
        {
            issues.push(ValidationIssue {
                severity: IssueSeverity::High,
                category: IssueCategory::Security,
                description: "Capability must use HTTPS for external communications".to_string(),
                location: Some("RTFS code".to_string()),
                suggestion: Some("Use HTTPS URLs for all external API calls".to_string()),
                code: "GOV003".to_string(),
            });
        }

        // Check for audit logging requirements
        if self.require_audit_logging && !rtfs_code.contains("audit") && !rtfs_code.contains("log")
        {
            issues.push(ValidationIssue {
                severity: IssueSeverity::Medium,
                category: IssueCategory::Compliance,
                description: "Capability must implement audit logging".to_string(),
                location: Some("RTFS code".to_string()),
                suggestion: Some("Add audit logging for all operations".to_string()),
                code: "GOV004".to_string(),
            });
        }

        let security_score = if issues.iter().any(|i| i.category == IssueCategory::Security) {
            0.5
        } else {
            1.0
        };

        let compliance_score = if issues
            .iter()
            .any(|i| i.category == IssueCategory::Compliance)
        {
            0.7
        } else {
            1.0
        };

        ValidationResult {
            status: if issues.iter().any(|i| i.severity == IssueSeverity::Critical) {
                super::validation_harness::ValidationStatus::SecurityFailed
            } else {
                super::validation_harness::ValidationStatus::PassedWithWarnings
            },
            issues,
            security_score,
            quality_score: 1.0,
            compliance_score,
            metadata: HashMap::new(),
        }
    }

    fn policy_name(&self) -> &str {
        &self.name
    }

    fn policy_description(&self) -> &str {
        "Enterprise security policy enforcement"
    }
}

/// Data privacy policy (GDPR/CCPA compliance)
pub struct DataPrivacyPolicy {
    pub name: String,
    pub require_consent: bool,
    pub require_data_minimization: bool,
    pub require_right_to_deletion: bool,
    pub sensitive_data_patterns: Vec<String>,
}

impl DataPrivacyPolicy {
    pub fn new() -> Self {
        Self {
            name: "Data Privacy Policy (GDPR/CCPA)".to_string(),
            require_consent: true,
            require_data_minimization: true,
            require_right_to_deletion: true,
            sensitive_data_patterns: vec![
                "personal_data".to_string(),
                "user_id".to_string(),
                "email".to_string(),
                "phone".to_string(),
                "address".to_string(),
                "ssn".to_string(),
                "credit_card".to_string(),
            ],
        }
    }
}

impl GovernancePolicy for DataPrivacyPolicy {
    fn check_compliance(&self, manifest: &CapabilityManifest, rtfs_code: &str) -> ValidationResult {
        let mut issues = Vec::new();

        // Check for sensitive data patterns
        let code_lower = rtfs_code.to_lowercase();
        let description_lower = manifest.description.to_lowercase();

        let has_sensitive_data = self
            .sensitive_data_patterns
            .iter()
            .any(|pattern| code_lower.contains(pattern) || description_lower.contains(pattern));

        if has_sensitive_data {
            // Check for consent mechanisms
            if self.require_consent && !code_lower.contains("consent") {
                issues.push(ValidationIssue {
                    severity: IssueSeverity::Critical,
                    category: IssueCategory::Compliance,
                    description: "Capability processes sensitive data but lacks consent mechanism"
                        .to_string(),
                    location: Some("RTFS code".to_string()),
                    suggestion: Some(
                        "Implement user consent collection and validation".to_string(),
                    ),
                    code: "PRIV001".to_string(),
                });
            }

            // Check for data minimization
            if self.require_data_minimization && code_lower.contains("collect") {
                issues.push(ValidationIssue {
                    severity: IssueSeverity::High,
                    category: IssueCategory::Compliance,
                    description: "Capability must implement data minimization principles"
                        .to_string(),
                    location: Some("RTFS code".to_string()),
                    suggestion: Some(
                        "Only collect data that is necessary for the stated purpose".to_string(),
                    ),
                    code: "PRIV002".to_string(),
                });
            }

            // Check for right to deletion
            if self.require_right_to_deletion && !code_lower.contains("delete") {
                issues.push(ValidationIssue {
                    severity: IssueSeverity::High,
                    category: IssueCategory::Compliance,
                    description: "Capability must support right to deletion (GDPR Article 17)"
                        .to_string(),
                    location: Some("RTFS code".to_string()),
                    suggestion: Some("Implement data deletion functionality".to_string()),
                    code: "PRIV003".to_string(),
                });
            }
        }

        let compliance_score = if issues.iter().any(|i| i.severity == IssueSeverity::Critical) {
            0.0
        } else if issues.iter().any(|i| i.severity == IssueSeverity::High) {
            0.5
        } else {
            1.0
        };

        ValidationResult {
            status: if issues.iter().any(|i| i.severity == IssueSeverity::Critical) {
                super::validation_harness::ValidationStatus::ComplianceFailed
            } else {
                super::validation_harness::ValidationStatus::PassedWithWarnings
            },
            issues,
            security_score: 1.0,
            quality_score: 1.0,
            compliance_score,
            metadata: HashMap::new(),
        }
    }

    fn policy_name(&self) -> &str {
        &self.name
    }

    fn policy_description(&self) -> &str {
        "Data privacy and GDPR compliance enforcement"
    }
}

/// Performance and resource policy
pub struct PerformancePolicy {
    pub name: String,
    pub max_execution_time: u64, // in milliseconds
    pub max_memory_usage: u64,   // in MB
    pub max_external_calls: u32,
    pub require_caching: bool,
}

impl PerformancePolicy {
    pub fn new() -> Self {
        Self {
            name: "Performance and Resource Policy".to_string(),
            max_execution_time: 5000, // 5 seconds
            max_memory_usage: 100,    // 100 MB
            max_external_calls: 10,
            require_caching: true,
        }
    }
}

impl GovernancePolicy for PerformancePolicy {
    fn check_compliance(&self, manifest: &CapabilityManifest, rtfs_code: &str) -> ValidationResult {
        let mut issues = Vec::new();

        // Count external calls
        let external_calls =
            rtfs_code.matches("(call :http").count() + rtfs_code.matches("(call :database").count();

        if external_calls > self.max_external_calls as usize {
            issues.push(ValidationIssue {
                severity: IssueSeverity::Medium,
                category: IssueCategory::Performance,
                description: format!(
                    "Capability exceeds maximum external calls limit ({} > {})",
                    external_calls, self.max_external_calls
                ),
                location: Some("RTFS code".to_string()),
                suggestion: Some("Reduce external calls or implement batching".to_string()),
                code: "PERF001".to_string(),
            });
        }

        // Check for caching requirements
        if self.require_caching && external_calls > 0 && !rtfs_code.contains("cache") {
            issues.push(ValidationIssue {
                severity: IssueSeverity::Low,
                category: IssueCategory::Performance,
                description: "Capability should implement caching for external calls".to_string(),
                location: Some("RTFS code".to_string()),
                suggestion: Some("Add caching mechanism for frequently accessed data".to_string()),
                code: "PERF002".to_string(),
            });
        }

        let quality_score = if issues.iter().any(|i| i.severity == IssueSeverity::Medium) {
            0.7
        } else if issues.iter().any(|i| i.severity == IssueSeverity::Low) {
            0.9
        } else {
            1.0
        };

        ValidationResult {
            status: super::validation_harness::ValidationStatus::PassedWithWarnings,
            issues,
            security_score: 1.0,
            quality_score,
            compliance_score: 1.0,
            metadata: HashMap::new(),
        }
    }

    fn policy_name(&self) -> &str {
        &self.name
    }

    fn policy_description(&self) -> &str {
        "Performance and resource usage policy enforcement"
    }
}

/// API design standards policy
pub struct ApiDesignPolicy {
    pub name: String,
    pub require_versioning: bool,
    pub require_documentation: bool,
    pub require_error_handling: bool,
    pub max_parameter_count: u32,
}

impl ApiDesignPolicy {
    pub fn new() -> Self {
        Self {
            name: "API Design Standards Policy".to_string(),
            require_versioning: true,
            require_documentation: true,
            require_error_handling: true,
            max_parameter_count: 10,
        }
    }
}

impl GovernancePolicy for ApiDesignPolicy {
    fn check_compliance(&self, manifest: &CapabilityManifest, rtfs_code: &str) -> ValidationResult {
        let mut issues = Vec::new();

        // Check for versioning
        if self.require_versioning && !manifest.id.contains(".v") {
            issues.push(ValidationIssue {
                severity: IssueSeverity::Medium,
                category: IssueCategory::ApiDesign,
                description: "Capability ID must include version number".to_string(),
                location: Some(format!("Capability ID: {}", manifest.id)),
                suggestion: Some(
                    "Use semantic versioning in capability ID (e.g., .v1, .v1.2)".to_string(),
                ),
                code: "API001".to_string(),
            });
        }

        // Check for documentation
        if self.require_documentation && manifest.description.len() < 50 {
            issues.push(ValidationIssue {
                severity: IssueSeverity::Low,
                category: IssueCategory::Documentation,
                description: "Capability must have comprehensive documentation".to_string(),
                location: Some("Capability manifest".to_string()),
                suggestion: Some(
                    "Provide detailed description of capability purpose and usage".to_string(),
                ),
                code: "API002".to_string(),
            });
        }

        // Check for error handling
        if self.require_error_handling && !rtfs_code.contains("try") && !rtfs_code.contains("catch")
        {
            issues.push(ValidationIssue {
                severity: IssueSeverity::Medium,
                category: IssueCategory::Quality,
                description: "Capability must implement proper error handling".to_string(),
                location: Some("RTFS code".to_string()),
                suggestion: Some("Add try-catch blocks for error handling".to_string()),
                code: "API003".to_string(),
            });
        }

        // Check parameter count (simplified check based on metadata)
        if let Some(param_count_str) = manifest.metadata.get("parameter_count") {
            if let Ok(param_count) = param_count_str.parse::<u32>() {
                if param_count > self.max_parameter_count {
                    issues.push(ValidationIssue {
                        severity: IssueSeverity::Low,
                        category: IssueCategory::ApiDesign,
                        description: format!(
                            "Capability has too many parameters ({} > {})",
                            param_count, self.max_parameter_count
                        ),
                        location: Some("Capability manifest".to_string()),
                        suggestion: Some(
                            "Consider grouping parameters into objects or splitting capability"
                                .to_string(),
                        ),
                        code: "API004".to_string(),
                    });
                }
            }
        }

        let quality_score = if issues.iter().any(|i| i.severity == IssueSeverity::Medium) {
            0.8
        } else if issues.iter().any(|i| i.severity == IssueSeverity::Low) {
            0.95
        } else {
            1.0
        };

        ValidationResult {
            status: super::validation_harness::ValidationStatus::PassedWithWarnings,
            issues,
            security_score: 1.0,
            quality_score,
            compliance_score: 1.0,
            metadata: HashMap::new(),
        }
    }

    fn policy_name(&self) -> &str {
        &self.name
    }

    fn policy_description(&self) -> &str {
        "API design standards and best practices enforcement"
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::capability_marketplace::types::*;

    fn create_test_manifest() -> CapabilityManifest {
        CapabilityManifest {
            id: "test.capability.v1".to_string(),
            name: "Test Capability".to_string(),
            description: "A test capability for governance policy testing".to_string(),
            version: "1.0.0".to_string(),
            provider: ProviderType::Local(LocalCapability {
                handler: std::sync::Arc::new(|_| {
                    Ok(rtfs::runtime::values::Value::String("test".to_string()))
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
    fn test_enterprise_security_policy() {
        let policy = EnterpriseSecurityPolicy::new();
        let mut manifest = create_test_manifest();
        manifest.name = "Admin Capability".to_string(); // Use a name with blocked keyword
        let rtfs_code = r#"
            (capability admin
                :description "Admin capability"
                :implementation
                (do
                    (call :http.post {:url "http://malicious.com/api"})
                )
            )
        "#;

        let result = policy.check_compliance(&manifest, rtfs_code);
        assert!(result.issues.len() > 0);
        assert!(result.issues.iter().any(|i| i.code == "GOV001")); // Blocked keyword
        assert!(result.issues.iter().any(|i| i.code == "GOV002")); // Non-approved domain
    }

    #[test]
    fn test_data_privacy_policy() {
        let policy = DataPrivacyPolicy::new();
        let manifest = create_test_manifest();
        let rtfs_code = r#"
            (capability user_data
                :description "Processes personal_data and email"
                :implementation
                (do
                    (let user_id "12345")
                    (let email "user@example.com")
                    (collect user_data)
                )
            )
        "#;

        let result = policy.check_compliance(&manifest, rtfs_code);
        assert!(result.issues.len() > 0);
        assert!(result.issues.iter().any(|i| i.code == "PRIV001")); // Missing consent
    }

    #[test]
    fn test_performance_policy() {
        let policy = PerformancePolicy::new();
        let manifest = create_test_manifest();
        let rtfs_code = r#"
            (capability external_calls
                :description "Makes many external calls"
                :implementation
                (do
                    (call :http.get {:url "https://api1.com"})
                    (call :http.get {:url "https://api2.com"})
                    (call :http.get {:url "https://api3.com"})
                    (call :http.get {:url "https://api4.com"})
                    (call :http.get {:url "https://api5.com"})
                    (call :http.get {:url "https://api6.com"})
                    (call :http.get {:url "https://api7.com"})
                    (call :http.get {:url "https://api8.com"})
                    (call :http.get {:url "https://api9.com"})
                    (call :http.get {:url "https://api10.com"})
                    (call :http.get {:url "https://api11.com"})
                )
            )
        "#;

        let result = policy.check_compliance(&manifest, rtfs_code);
        assert!(result.issues.len() > 0);
        assert!(result.issues.iter().any(|i| i.code == "PERF001")); // Too many external calls
    }

    #[test]
    fn test_api_design_policy() {
        let policy = ApiDesignPolicy::new();
        let mut manifest = create_test_manifest();
        manifest.id = "test.capability".to_string(); // No version
        manifest.description = "Short".to_string(); // Too short

        let rtfs_code = r#"
            (capability test
                :implementation
                (do
                    (call :http.get {:url "https://api.com"})
                )
            )
        "#;

        let result = policy.check_compliance(&manifest, rtfs_code);
        assert!(result.issues.len() > 0);
        assert!(result.issues.iter().any(|i| i.code == "API001")); // No versioning
        assert!(result.issues.iter().any(|i| i.code == "API002")); // Poor documentation
        assert!(result.issues.iter().any(|i| i.code == "API003")); // No error handling
    }
}
