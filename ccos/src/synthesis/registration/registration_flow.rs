//! Registration, Versioning, and Wiring Flow
//!
//! This module implements Phase 6 of the missing capability resolution plan:
//! - Pre-flight validation before registration
//! - Capability versioning and manifest creation
//! - Marketplace registration with audit events
//! - Parent capability integration re-evaluation
//! - End-to-end testing of resolved capabilities

use super::governance_policies::GovernancePolicy;
use super::static_analyzers::StaticAnalyzer;
use super::validation_harness::{ValidationHarness, ValidationResult, ValidationStatus};
use crate::capability_marketplace::types::{
    CapabilityAttestation, CapabilityManifest, CapabilityProvenance,
};
use crate::capability_marketplace::CapabilityMarketplace;
use chrono::Utc;
use rtfs::runtime::error::RuntimeResult;
use rtfs::runtime::values::Value;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::sync::Arc;

/// Registration flow result
#[derive(Debug, Clone)]
pub struct RegistrationResult {
    /// Whether registration was successful
    pub success: bool,
    /// Capability ID that was registered
    pub capability_id: String,
    /// Version assigned to the capability
    pub version: String,
    /// Validation result from pre-flight checks
    pub validation_result: ValidationResult,
    /// Any issues encountered during registration
    pub issues: Vec<String>,
    /// Audit event IDs generated
    pub audit_events: Vec<String>,
}

/// Registration flow orchestrator
pub struct RegistrationFlow {
    /// Validation harness for pre-flight checks
    validation_harness: ValidationHarness,
    /// Governance policies to apply
    governance_policies: Vec<Box<dyn GovernancePolicy>>,
    /// Static analyzers for code analysis
    static_analyzers: Vec<Box<dyn StaticAnalyzer>>,
    /// Marketplace for registration
    marketplace: Arc<CapabilityMarketplace>,
}

impl RegistrationFlow {
    /// Create a new registration flow
    pub fn new(marketplace: Arc<CapabilityMarketplace>) -> Self {
        Self {
            validation_harness: ValidationHarness::new(),
            governance_policies: vec![Box::new(
                crate::synthesis::governance_policies::MaxParameterCountPolicy::new(20),
            )],
            static_analyzers: vec![Box::new(
                crate::synthesis::static_analyzers::PerformanceAnalyzer::new(),
            )],
            marketplace,
        }
    }

    /// Register a capability with full validation and versioning
    pub async fn register_capability(
        &self,
        manifest: CapabilityManifest,
        rtfs_code: Option<&str>,
    ) -> RuntimeResult<RegistrationResult> {
        let capability_id = manifest.id.clone();

        // Step 1: Pre-flight validation
        let validation_result = self.validate_capability(&manifest, rtfs_code)?;

        // Step 2: Apply governance policies
        let governance_result = self.apply_governance_policies(&manifest, rtfs_code)?;

        // Step 3: Generate version and attestation
        let version = self.generate_version(&manifest, &validation_result)?;
        let attestation = self.create_attestation(&manifest, &validation_result)?;
        let provenance = self.create_provenance(&manifest, &validation_result)?;

        // Step 4: Create final manifest with metadata
        let final_manifest = self.create_final_manifest(
            manifest,
            version.clone(),
            attestation,
            provenance,
            &validation_result,
        )?;

        // Step 5: Register with marketplace
        self.marketplace
            .register_capability_manifest(final_manifest.clone())
            .await?;

        // Step 6: Emit validation audit event
        self.emit_validation_audit_event(&capability_id, &validation_result)
            .await?;

        // Step 7: Check for parent capability integration
        let integration_result = self.check_parent_integration(&capability_id).await?;

        Ok(RegistrationResult {
            success: true,
            capability_id,
            version,
            validation_result,
            issues: integration_result.issues,
            audit_events: vec![
                format!("capability_registered_{}", Utc::now().timestamp()),
                format!("capability_validated_{}", Utc::now().timestamp()),
            ],
        })
    }

    /// Validate capability before registration
    fn validate_capability(
        &self,
        manifest: &CapabilityManifest,
        rtfs_code: Option<&str>,
    ) -> RuntimeResult<ValidationResult> {
        // Use the validation harness for comprehensive validation
        let mut result = self
            .validation_harness
            .validate_capability(manifest, rtfs_code.unwrap_or(""));

        // Add static analysis results
        if let Some(code) = rtfs_code {
            for analyzer in &self.static_analyzers {
                let issues = analyzer.analyze(manifest, code);
                result.issues.extend(issues);
            }
        }

        // Update scores based on issues found
        self.update_validation_scores(&mut result);

        Ok(result)
    }

    /// Apply governance policies
    fn apply_governance_policies(
        &self,
        manifest: &CapabilityManifest,
        rtfs_code: Option<&str>,
    ) -> RuntimeResult<ValidationResult> {
        let mut result = ValidationResult {
            status: ValidationStatus::Passed,
            issues: vec![],
            security_score: 1.0,
            quality_score: 1.0,
            compliance_score: 1.0,
            metadata: HashMap::new(),
        };

        for policy in &self.governance_policies {
            let policy_result = policy.check_compliance(manifest, rtfs_code.unwrap_or(""));
            if policy_result.status == ValidationStatus::Failed {
                result.status = ValidationStatus::Failed;
            }
            result.issues.extend(policy_result.issues);
        }

        Ok(result)
    }

    /// Generate version for the capability
    fn generate_version(
        &self,
        manifest: &CapabilityManifest,
        validation_result: &ValidationResult,
    ) -> RuntimeResult<String> {
        // Create a deterministic version based on manifest content and validation
        let mut hasher = Sha256::new();
        hasher.update(manifest.id.as_bytes());
        hasher.update(manifest.name.as_bytes());
        hasher.update(manifest.description.as_bytes());

        if let Some(input_schema) = &manifest.input_schema {
            hasher.update(input_schema.to_string().as_bytes());
        }

        // Include validation status in version
        hasher.update(format!("{:?}", validation_result.status).as_bytes());

        let hash = hasher.finalize();
        let version_hash = format!("{:x}", hash)[..8].to_string();

        Ok(format!(
            "1.0.0-{}.{}",
            Utc::now().format("%Y%m%d"),
            version_hash
        ))
    }

    /// Create attestation for the capability
    fn create_attestation(
        &self,
        manifest: &CapabilityManifest,
        validation_result: &ValidationResult,
    ) -> RuntimeResult<CapabilityAttestation> {
        Ok(self
            .validation_harness
            .create_attestation(validation_result))
    }

    /// Create provenance for the capability
    fn create_provenance(
        &self,
        manifest: &CapabilityManifest,
        validation_result: &ValidationResult,
    ) -> RuntimeResult<CapabilityProvenance> {
        Ok(self.validation_harness.create_provenance(validation_result))
    }

    /// Create final manifest with all metadata
    fn create_final_manifest(
        &self,
        mut manifest: CapabilityManifest,
        version: String,
        attestation: CapabilityAttestation,
        provenance: CapabilityProvenance,
        validation_result: &ValidationResult,
    ) -> RuntimeResult<CapabilityManifest> {
        // Add version to metadata
        manifest.metadata.insert("version".to_string(), version);
        manifest
            .metadata
            .insert("registered_at".to_string(), Utc::now().to_rfc3339());

        // Add validation results to metadata
        manifest.metadata.insert(
            "validation_status".to_string(),
            format!("{:?}", validation_result.status),
        );
        manifest.metadata.insert(
            "security_score".to_string(),
            validation_result.security_score.to_string(),
        );
        manifest.metadata.insert(
            "quality_score".to_string(),
            validation_result.quality_score.to_string(),
        );
        manifest.metadata.insert(
            "compliance_score".to_string(),
            validation_result.compliance_score.to_string(),
        );

        // Add attestation and provenance
        manifest.attestation = Some(attestation);
        manifest.provenance = Some(provenance);

        Ok(manifest)
    }

    /// Emit validation audit event
    async fn emit_validation_audit_event(
        &self,
        capability_id: &str,
        validation_result: &ValidationResult,
    ) -> RuntimeResult<()> {
        let mut event_data = HashMap::new();
        event_data.insert("capability_id".to_string(), capability_id.to_string());
        event_data.insert(
            "validation_status".to_string(),
            format!("{:?}", validation_result.status),
        );
        event_data.insert(
            "security_score".to_string(),
            validation_result.security_score.to_string(),
        );
        event_data.insert(
            "quality_score".to_string(),
            validation_result.quality_score.to_string(),
        );
        event_data.insert(
            "compliance_score".to_string(),
            validation_result.compliance_score.to_string(),
        );
        event_data.insert(
            "issues_count".to_string(),
            validation_result.issues.len().to_string(),
        );
        event_data.insert("timestamp".to_string(), Utc::now().to_rfc3339());

        // Emit via marketplace audit system
        self.marketplace
            .emit_capability_audit_event("capability_validated", capability_id, Some(event_data))
            .await?;

        Ok(())
    }

    /// Check parent capability integration
    async fn check_parent_integration(
        &self,
        capability_id: &str,
    ) -> RuntimeResult<IntegrationResult> {
        // Find capabilities that might depend on this one
        let dependent_capabilities = self.find_dependent_capabilities(capability_id).await?;

        let mut issues = vec![];
        let mut updated_capabilities = vec![];

        for dependent_id in dependent_capabilities {
            // Check if the dependent capability can now be resolved
            if let Some(capability) = self.marketplace.get_capability(&dependent_id).await {
                // Re-evaluate the capability's dependencies
                if let Some(deps) = capability.metadata.get("needs_capabilities") {
                    let needed: Vec<String> = deps
                        .split(',')
                        .map(|s| s.trim().trim_matches('"').to_string())
                        .filter(|s| !s.is_empty())
                        .collect();
                    let mut still_missing: Vec<String> = vec![];
                    for dep in &needed {
                        if !self.marketplace.has_capability(dep).await {
                            still_missing.push(dep.to_string());
                        }
                    }

                    if still_missing.is_empty() {
                        // All dependencies resolved, update metadata
                        let mut updated_cap = capability.clone();
                        updated_cap
                            .metadata
                            .insert("all_dependencies_resolved".to_string(), "true".to_string());
                        updated_cap
                            .metadata
                            .insert("last_dependency_check".to_string(), Utc::now().to_rfc3339());

                        self.marketplace
                            .register_capability_manifest(updated_cap.clone())
                            .await?;
                        updated_capabilities.push(dependent_id);
                    } else {
                        issues.push(format!(
                            "Capability {} still missing dependencies: {:?}",
                            dependent_id, still_missing
                        ));
                    }
                }
            }
        }

        Ok(IntegrationResult {
            issues,
            updated_capabilities,
        })
    }

    /// Find capabilities that depend on the given capability
    async fn find_dependent_capabilities(&self, capability_id: &str) -> RuntimeResult<Vec<String>> {
        let mut dependents = vec![];

        // Get all capabilities and check their metadata for dependencies
        let capabilities = self.marketplace.list_capabilities().await;

        for capability in capabilities {
            if let Some(deps) = capability.metadata.get("needs_capabilities") {
                let needed: Vec<String> = deps
                    .split(',')
                    .map(|s| s.trim().trim_matches('"').to_string())
                    .filter(|s| !s.is_empty())
                    .collect();
                if needed.iter().any(|dep| dep == capability_id) {
                    dependents.push(capability.id.clone());
                }
            }
        }

        Ok(dependents)
    }

    /// Update validation scores based on issues found
    fn update_validation_scores(&self, result: &mut ValidationResult) {
        let issue_count = result.issues.len();
        let critical_issues = result
            .issues
            .iter()
            .filter(|issue| {
                issue.severity == crate::synthesis::validation_harness::IssueSeverity::Critical
            })
            .count();

        // Calculate scores based on issue counts
        result.security_score = if critical_issues > 0 {
            0.0
        } else {
            1.0 - (issue_count as f64 * 0.1).min(1.0)
        };
        result.quality_score = 1.0 - (issue_count as f64 * 0.05).min(0.8);
        result.compliance_score = if critical_issues > 2 {
            0.0
        } else {
            1.0 - (critical_issues as f64 * 0.3).min(0.7)
        };

        // Update status based on scores
        if result.security_score < 0.5 {
            result.status = ValidationStatus::SecurityFailed;
        } else if result.compliance_score < 0.5 {
            result.status = ValidationStatus::ComplianceFailed;
        } else if result.quality_score < 0.7 {
            result.status = ValidationStatus::PassedWithWarnings;
        } else if result.status != ValidationStatus::Failed {
            result.status = ValidationStatus::Passed;
        }
    }

    /// Run end-to-end test for a capability
    pub async fn run_end_to_end_test(&self, capability_id: &str) -> RuntimeResult<TestResult> {
        // Get the capability
        let capability = self
            .marketplace
            .get_capability(capability_id)
            .await
            .ok_or_else(|| {
                rtfs::runtime::error::RuntimeError::Generic(format!(
                    "Capability {} not found",
                    capability_id
                ))
            })?;

        // Create test inputs based on input schema
        let test_inputs = self.generate_test_inputs(&capability)?;

        // Execute the capability
        let result = self
            .marketplace
            .execute_capability(capability_id, &test_inputs)
            .await;

        match result {
            Ok(output) => Ok(TestResult {
                success: true,
                output: Some(output),
                error: None,
                execution_time_ms: 0, // TODO: Measure actual execution time
            }),
            Err(e) => Ok(TestResult {
                success: false,
                output: None,
                error: Some(e.to_string()),
                execution_time_ms: 0,
            }),
        }
    }

    /// Generate test inputs based on capability schema
    fn generate_test_inputs(&self, capability: &CapabilityManifest) -> RuntimeResult<Value> {
        if let Some(input_schema) = &capability.input_schema {
            self.generate_test_value_from_type_expr(input_schema)
        } else {
            // No schema - return empty map
            Ok(Value::Map(HashMap::new()))
        }
    }

    /// Generate a test value from a TypeExpr
    fn generate_test_value_from_type_expr(
        &self,
        type_expr: &rtfs::ast::TypeExpr,
    ) -> RuntimeResult<Value> {
        use rtfs::ast::{PrimitiveType, TypeExpr};

        match type_expr {
            TypeExpr::Primitive(prim) => match prim {
                PrimitiveType::String => Ok(Value::String("test".to_string())),
                PrimitiveType::Int => Ok(Value::Integer(0)),
                PrimitiveType::Float => Ok(Value::Float(0.0)),
                PrimitiveType::Bool => Ok(Value::Boolean(false)),
                PrimitiveType::Nil => Ok(Value::Nil),
                _ => Ok(Value::String("test".to_string())), // Fallback
            },
            TypeExpr::Vector(inner) => {
                // Generate a single-element array for testing
                let element = self.generate_test_value_from_type_expr(inner)?;
                Ok(Value::Vector(vec![element]))
            }
            TypeExpr::Map {
                entries,
                wildcard: _,
            } => {
                use rtfs::ast::MapKey;
                let mut map = HashMap::new();
                for entry in entries {
                    let key = MapKey::Keyword(entry.key.clone());
                    let value = self.generate_test_value_from_type_expr(&entry.value_type)?;
                    map.insert(key, value);
                }
                Ok(Value::Map(map))
            }
            TypeExpr::Any => Ok(Value::String("test".to_string())), // Fallback for :any
            TypeExpr::Never => Ok(Value::Nil),                      // :never - use nil as fallback
            _ => Ok(Value::String("test".to_string())),             // Fallback for other types
        }
    }
}

/// Integration result for parent capability checks
#[derive(Debug, Clone)]
struct IntegrationResult {
    issues: Vec<String>,
    updated_capabilities: Vec<String>,
}

/// Test result for end-to-end testing
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TestResult {
    pub success: bool,
    pub output: Option<Value>,
    pub error: Option<String>,
    pub execution_time_ms: u64,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::capability_marketplace::types::CapabilityManifest;
    use crate::capability_marketplace::types::LocalCapability;
    use crate::capability_marketplace::ProviderType;

    #[tokio::test]
    async fn test_registration_flow() {
        // Create a mock marketplace
        let registry = Arc::new(tokio::sync::RwLock::new(
            crate::capabilities::registry::CapabilityRegistry::new(),
        ));
        let marketplace = Arc::new(CapabilityMarketplace::new(registry));
        let flow = RegistrationFlow::new(marketplace);

        // Create a test manifest
        let manifest = CapabilityManifest {
            id: "test.capability.v1".to_string(),
            name: "Test Capability".to_string(),
            description: "A test capability".to_string(),
            version: "1.0.0".to_string(),
            provider: ProviderType::Local(LocalCapability {
                handler: Arc::new(|_args| Ok(Value::String("test".to_string()))),
            }),
            input_schema: None,
            output_schema: None,
            attestation: None,
            provenance: None,
            permissions: vec![],
            effects: vec![],
            metadata: HashMap::new(),
            agent_metadata: None,
            domains: Vec::new(),
            categories: Vec::new(),
        };

        // Test registration
        let result = flow.register_capability(manifest, None).await;
        assert!(result.is_ok());

        let reg_result = result.unwrap();
        assert!(reg_result.success);
        assert_eq!(reg_result.capability_id, "test.capability.v1");
    }

    #[test]
    fn test_version_generation() {
        let registry = Arc::new(tokio::sync::RwLock::new(
            crate::capabilities::registry::CapabilityRegistry::new(),
        ));
        let marketplace = Arc::new(CapabilityMarketplace::new(registry));
        let flow = RegistrationFlow::new(marketplace);

        let manifest = CapabilityManifest {
            id: "test.version".to_string(),
            name: "Version Test".to_string(),
            description: "Test version generation".to_string(),
            version: "1.0.0".to_string(),
            provider: ProviderType::Local(LocalCapability {
                handler: Arc::new(|_args| Ok(Value::String("test".to_string()))),
            }),
            input_schema: None,
            output_schema: None,
            attestation: None,
            provenance: None,
            permissions: vec![],
            effects: vec![],
            metadata: HashMap::new(),
            agent_metadata: None,
            domains: Vec::new(),
            categories: Vec::new(),
        };

        let validation_result = ValidationResult {
            status: ValidationStatus::Passed,
            issues: vec![],
            security_score: 1.0,
            quality_score: 1.0,
            compliance_score: 1.0,
            metadata: HashMap::new(),
        };

        let version = flow
            .generate_version(&manifest, &validation_result)
            .unwrap();
        assert!(version.starts_with("1.0.0-"));
        assert!(version.contains("."));
    }
}
