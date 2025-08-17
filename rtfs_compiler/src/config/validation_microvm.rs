//! MicroVM configuration validation
//!
//! This module provides validation for MicroVM-specific agent configurations,
//! enforcing constraints and security policies as defined in the MicroVM deployment profile.

use crate::runtime::error::RuntimeResult;
use super::types::*;

/// Validation result for MicroVM configuration
#[derive(Debug, Clone)]
pub struct MicroVMValidationResult {
    /// Whether the configuration is valid
    pub is_valid: bool,
    /// List of validation errors
    pub errors: Vec<MicroVMValidationError>,
    /// List of validation warnings
    pub warnings: Vec<MicroVMValidationWarning>,
}

/// MicroVM validation error
#[derive(Debug, Clone)]
pub struct MicroVMValidationError {
    /// Error code for programmatic handling
    pub code: String,
    /// Human-readable error message
    pub message: String,
    /// Field path where the error occurred
    pub field_path: String,
    /// Suggested fix for the error
    pub suggestion: Option<String>,
}

/// MicroVM validation warning
#[derive(Debug, Clone)]
pub struct MicroVMValidationWarning {
    /// Warning code for programmatic handling
    pub code: String,
    /// Human-readable warning message
    pub message: String,
    /// Field path where the warning occurred
    pub field_path: String,
    /// Suggested improvement
    pub suggestion: Option<String>,
}

/// Validates a complete agent configuration for MicroVM deployment
/// 
/// This function implements all validation rules from Issue 3:
/// - When :microvm present → :orchestrator.isolation.mode must be :microvm
/// - :orchestrator.isolation.fs.ephemeral must be true
/// - Capabilities mount must be RO
/// - :resources.vcpus >= 1; :resources.mem_mb >= 64
/// - If :devices.vsock.enabled → :cid > 0
/// - If :devices.nic.enabled → require :network.egress {:via :proxy, :allow-domains non-empty}
/// - If :attestation.enabled → expect_rootfs_hash present
pub fn validate_microvm_config(config: &AgentConfig) -> RuntimeResult<MicroVMValidationResult> {
    let mut result = MicroVMValidationResult {
        is_valid: true,
        errors: Vec::new(),
        warnings: Vec::new(),
    };

    // If no MicroVM config is present, return early (not a MicroVM deployment)
    let microvm_config = match &config.microvm {
        Some(mv) => mv,
        None => {
            return Ok(result);
        }
    };

    // Validation Rule 1: When :microvm present → :orchestrator.isolation.mode must be :microvm
    validate_isolation_mode(config, &mut result);

    // Validation Rule 2: :orchestrator.isolation.fs.ephemeral must be true
    validate_ephemeral_fs(config, &mut result);

    // Validation Rule 3: Capabilities mount must be RO
    validate_capabilities_mount(config, &mut result);

    // Validation Rule 4: :resources.vcpus >= 1; :resources.mem_mb >= 64
    validate_resource_constraints(microvm_config, &mut result);

    // Validation Rule 5: If :devices.vsock.enabled → :cid > 0
    validate_vsock_config(microvm_config, &mut result);

    // Validation Rule 6: If :devices.nic.enabled → require :network.egress {:via :proxy, :allow-domains non-empty}
    validate_nic_config(config, &mut result);

    // Validation Rule 7: If :attestation.enabled → expect_rootfs_hash present
    validate_attestation_config(microvm_config, &mut result);

    // Additional security validations
    validate_security_policies(config, &mut result);

    // Update validity based on errors
    result.is_valid = result.errors.is_empty();

    Ok(result)
}

/// Validates that isolation mode is set to microvm when MicroVM config is present
fn validate_isolation_mode(config: &AgentConfig, result: &mut MicroVMValidationResult) {
    if config.orchestrator.isolation.mode != "microvm" {
        result.errors.push(MicroVMValidationError {
            code: "INVALID_ISOLATION_MODE".to_string(),
            message: "When MicroVM configuration is present, orchestrator.isolation.mode must be 'microvm'".to_string(),
            field_path: "orchestrator.isolation.mode".to_string(),
            suggestion: Some("Set orchestrator.isolation.mode to 'microvm'".to_string()),
        });
    }
}

/// Validates that filesystem is ephemeral for MicroVM deployments
fn validate_ephemeral_fs(config: &AgentConfig, result: &mut MicroVMValidationResult) {
    if !config.orchestrator.isolation.fs.ephemeral {
        result.errors.push(MicroVMValidationError {
            code: "NON_EPHEMERAL_FS".to_string(),
            message: "MicroVM deployments require ephemeral filesystem for security".to_string(),
            field_path: "orchestrator.isolation.fs.ephemeral".to_string(),
            suggestion: Some("Set orchestrator.isolation.fs.ephemeral to true".to_string()),
        });
    }
}

/// Validates that capabilities mount is read-only
fn validate_capabilities_mount(config: &AgentConfig, result: &mut MicroVMValidationResult) {
    if let Some(capabilities_mount) = config.orchestrator.isolation.fs.mounts.get("capabilities") {
        if capabilities_mount.mode != "ro" {
            result.errors.push(MicroVMValidationError {
                code: "CAPABILITIES_NOT_READONLY".to_string(),
                message: "Capabilities mount must be read-only for MicroVM security".to_string(),
                field_path: "orchestrator.isolation.fs.mounts.capabilities.mode".to_string(),
                suggestion: Some("Set capabilities mount mode to 'ro'".to_string()),
            });
        }
    } else {
        result.warnings.push(MicroVMValidationWarning {
            code: "MISSING_CAPABILITIES_MOUNT".to_string(),
            message: "No capabilities mount configured - consider adding read-only capabilities mount".to_string(),
            field_path: "orchestrator.isolation.fs.mounts.capabilities".to_string(),
            suggestion: Some("Add capabilities mount with mode 'ro'".to_string()),
        });
    }
}

/// Validates resource constraints for MicroVM
fn validate_resource_constraints(microvm_config: &MicroVMConfig, result: &mut MicroVMValidationResult) {
    // Validate vCPUs >= 1
    if microvm_config.resources.vcpus < 1 {
        result.errors.push(MicroVMValidationError {
            code: "INVALID_VCPUS".to_string(),
            message: format!("vCPUs must be >= 1, got {}", microvm_config.resources.vcpus),
            field_path: "microvm.resources.vcpus".to_string(),
            suggestion: Some("Set vcpus to 1 or higher".to_string()),
        });
    }

    // Validate memory >= 64MB
    if microvm_config.resources.mem_mb < 64 {
        result.errors.push(MicroVMValidationError {
            code: "INVALID_MEMORY".to_string(),
            message: format!("Memory must be >= 64MB, got {}MB", microvm_config.resources.mem_mb),
            field_path: "microvm.resources.mem_mb".to_string(),
            suggestion: Some("Set mem_mb to 64 or higher".to_string()),
        });
    }

    // Warning for very high resource allocation
    if microvm_config.resources.vcpus > 4 {
        result.warnings.push(MicroVMValidationWarning {
            code: "HIGH_VCPU_ALLOCATION".to_string(),
            message: format!("High vCPU allocation: {} vCPUs", microvm_config.resources.vcpus),
            field_path: "microvm.resources.vcpus".to_string(),
            suggestion: Some("Consider reducing vCPUs for better resource utilization".to_string()),
        });
    }

    if microvm_config.resources.mem_mb > 1024 {
        result.warnings.push(MicroVMValidationWarning {
            code: "HIGH_MEMORY_ALLOCATION".to_string(),
            message: format!("High memory allocation: {}MB", microvm_config.resources.mem_mb),
            field_path: "microvm.resources.mem_mb".to_string(),
            suggestion: Some("Consider reducing memory allocation for better resource utilization".to_string()),
        });
    }
}

/// Validates VSock configuration
fn validate_vsock_config(microvm_config: &MicroVMConfig, result: &mut MicroVMValidationResult) {
    if microvm_config.devices.vsock.enabled {
        if microvm_config.devices.vsock.cid == 0 {
            result.errors.push(MicroVMValidationError {
                code: "INVALID_VSOCK_CID".to_string(),
                message: "VSock CID must be > 0 when VSock is enabled".to_string(),
                field_path: "microvm.devices.vsock.cid".to_string(),
                suggestion: Some("Set vsock.cid to a value > 0".to_string()),
            });
        }
    }
}

/// Validates NIC configuration and network egress requirements
fn validate_nic_config(config: &AgentConfig, result: &mut MicroVMValidationResult) {
    let microvm_config = config.microvm.as_ref().unwrap();
    
    if microvm_config.devices.nic.enabled {
        // Check that network is enabled
        if !config.network.enabled {
            result.errors.push(MicroVMValidationError {
                code: "NIC_ENABLED_BUT_NETWORK_DISABLED".to_string(),
                message: "NIC is enabled but network is disabled".to_string(),
                field_path: "network.enabled".to_string(),
                suggestion: Some("Enable network when NIC is enabled".to_string()),
            });
        } else {
            // Check egress configuration
            if config.network.egress.via != "proxy" {
                result.errors.push(MicroVMValidationError {
                    code: "INVALID_EGRESS_METHOD".to_string(),
                    message: "MicroVM deployments require proxy-based egress".to_string(),
                    field_path: "network.egress.via".to_string(),
                    suggestion: Some("Set network.egress.via to 'proxy'".to_string()),
                });
            }

            if config.network.egress.allow_domains.is_empty() {
                result.errors.push(MicroVMValidationError {
                    code: "EMPTY_ALLOWED_DOMAINS".to_string(),
                    message: "Network egress requires at least one allowed domain".to_string(),
                    field_path: "network.egress.allow_domains".to_string(),
                    suggestion: Some("Add at least one domain to network.egress.allow_domains".to_string()),
                });
            }
        }
    }
}

/// Validates attestation configuration
fn validate_attestation_config(microvm_config: &MicroVMConfig, result: &mut MicroVMValidationResult) {
    if microvm_config.attestation.enabled {
        if microvm_config.attestation.expect_rootfs_hash.is_empty() {
            result.errors.push(MicroVMValidationError {
                code: "MISSING_ROOTFS_HASH".to_string(),
                message: "Attestation is enabled but expect_rootfs_hash is not provided".to_string(),
                field_path: "microvm.attestation.expect_rootfs_hash".to_string(),
                suggestion: Some("Provide the expected rootfs hash for attestation".to_string()),
            });
        } else if !microvm_config.attestation.expect_rootfs_hash.starts_with("sha256:") {
            result.warnings.push(MicroVMValidationWarning {
                code: "INVALID_HASH_FORMAT".to_string(),
                message: "Rootfs hash should be in 'sha256:...' format".to_string(),
                field_path: "microvm.attestation.expect_rootfs_hash".to_string(),
                suggestion: Some("Use 'sha256:' prefix for the rootfs hash".to_string()),
            });
        }
    }
}

/// Validates additional security policies
fn validate_security_policies(config: &AgentConfig, result: &mut MicroVMValidationResult) {
    // Check DLP policy
    if !config.orchestrator.dlp.enabled {
        result.warnings.push(MicroVMValidationWarning {
            code: "DLP_DISABLED".to_string(),
            message: "DLP is disabled - consider enabling for MicroVM deployments".to_string(),
            field_path: "orchestrator.dlp.enabled".to_string(),
            suggestion: Some("Enable DLP for better security".to_string()),
        });
    } else if config.orchestrator.dlp.policy != "strict" {
        result.warnings.push(MicroVMValidationWarning {
            code: "NON_STRICT_DLP".to_string(),
            message: "Consider using 'strict' DLP policy for MicroVM deployments".to_string(),
            field_path: "orchestrator.dlp.policy".to_string(),
            suggestion: Some("Set DLP policy to 'strict'".to_string()),
        });
    }

    // Check marketplace readonly setting
    if let Some(readonly) = config.marketplace.readonly {
        if !readonly {
            result.warnings.push(MicroVMValidationWarning {
                code: "MARKETPLACE_NOT_READONLY".to_string(),
                message: "Consider making marketplace readonly for MicroVM security".to_string(),
                field_path: "marketplace.readonly".to_string(),
                suggestion: Some("Set marketplace.readonly to true".to_string()),
            });
        }
    }
}

/// Validates a specific field in the MicroVM configuration
pub fn validate_microvm_field(
    field_path: &str,
    value: &str,
    config: &AgentConfig,
) -> RuntimeResult<bool> {
    let validation_result = validate_microvm_config(config)?;
    
    // Check if there are any errors for the specific field
    for error in &validation_result.errors {
        if error.field_path == field_path {
            return Ok(false);
        }
    }
    
    Ok(true)
}

/// Gets validation errors for a specific field
pub fn get_field_errors(
    field_path: &str,
    config: &AgentConfig,
) -> RuntimeResult<Vec<MicroVMValidationError>> {
    let validation_result = validate_microvm_config(config)?;
    
    Ok(validation_result
        .errors
        .into_iter()
        .filter(|error| error.field_path == field_path)
        .collect())
}

/// Formats validation errors as a human-readable string
pub fn format_validation_errors(result: &MicroVMValidationResult) -> String {
    let mut output = String::new();
    
    if result.errors.is_empty() && result.warnings.is_empty() {
        output.push_str("✅ MicroVM configuration is valid\n");
        return output;
    }
    
    if !result.errors.is_empty() {
        output.push_str("❌ Validation Errors:\n");
        for error in &result.errors {
            output.push_str(&format!("  • {}: {}\n", error.field_path, error.message));
            if let Some(suggestion) = &error.suggestion {
                output.push_str(&format!("    Suggestion: {}\n", suggestion));
            }
        }
    }
    
    if !result.warnings.is_empty() {
        output.push_str("⚠️  Validation Warnings:\n");
        for warning in &result.warnings {
            output.push_str(&format!("  • {}: {}\n", warning.field_path, warning.message));
            if let Some(suggestion) = &warning.suggestion {
                output.push_str(&format!("    Suggestion: {}\n", suggestion));
            }
        }
    }
    
    output
}

#[cfg(test)]
mod tests {
    use super::*;

    fn create_valid_microvm_config() -> AgentConfig {
        let mut config = AgentConfig::default();
        config.orchestrator.isolation.mode = "microvm".to_string();
        config.orchestrator.isolation.fs.ephemeral = true;
        config.orchestrator.isolation.fs.mounts.insert(
            "capabilities".to_string(),
            MountConfig { mode: "ro".to_string() },
        );
        config.orchestrator.dlp.enabled = true;
        config.orchestrator.dlp.policy = "strict".to_string();
        config.network.enabled = true;
        config.network.egress.via = "proxy".to_string();
        config.network.egress.allow_domains = vec!["example.com".to_string()];
        config.network.egress.mtls = true;
        config.microvm = Some(MicroVMConfig {
            kernel: KernelConfig {
                image: "kernels/vmlinuz-min".to_string(),
                cmdline: "console=none".to_string(),
            },
            rootfs: RootFSConfig {
                image: "images/agent-rootfs.img".to_string(),
                ro: true,
            },
            resources: ResourceConfig {
                vcpus: 1,
                mem_mb: 256,
            },
            devices: DeviceConfig {
                nic: NICConfig {
                    enabled: true,
                    proxy_ns: "egress-proxy-ns".to_string(),
                },
                vsock: VSockConfig {
                    enabled: true,
                    cid: 3,
                },
            },
            attestation: AttestationConfig {
                enabled: true,
                expect_rootfs_hash: "sha256:abc123...".to_string(),
            },
        });
        config.marketplace.readonly = Some(true);
        config
    }

    #[test]
    fn test_valid_microvm_config() {
        let config = create_valid_microvm_config();
        let result = validate_microvm_config(&config).unwrap();
        
        assert!(result.is_valid);
        assert!(result.errors.is_empty());
    }

    #[test]
    fn test_invalid_isolation_mode() {
        let mut config = create_valid_microvm_config();
        config.orchestrator.isolation.mode = "wasm".to_string();
        
        let result = validate_microvm_config(&config).unwrap();
        
        assert!(!result.is_valid);
        assert!(result.errors.iter().any(|e| e.code == "INVALID_ISOLATION_MODE"));
    }

    #[test]
    fn test_non_ephemeral_fs() {
        let mut config = create_valid_microvm_config();
        config.orchestrator.isolation.fs.ephemeral = false;
        
        let result = validate_microvm_config(&config).unwrap();
        
        assert!(!result.is_valid);
        assert!(result.errors.iter().any(|e| e.code == "NON_EPHEMERAL_FS"));
    }

    #[test]
    fn test_capabilities_not_readonly() {
        let mut config = create_valid_microvm_config();
        config.orchestrator.isolation.fs.mounts.insert(
            "capabilities".to_string(),
            MountConfig { mode: "rw".to_string() },
        );
        
        let result = validate_microvm_config(&config).unwrap();
        
        assert!(!result.is_valid);
        assert!(result.errors.iter().any(|e| e.code == "CAPABILITIES_NOT_READONLY"));
    }

    #[test]
    fn test_invalid_vcpus() {
        let mut config = create_valid_microvm_config();
        if let Some(microvm) = &mut config.microvm {
            microvm.resources.vcpus = 0;
        }
        
        let result = validate_microvm_config(&config).unwrap();
        
        assert!(!result.is_valid);
        assert!(result.errors.iter().any(|e| e.code == "INVALID_VCPUS"));
    }

    #[test]
    fn test_invalid_memory() {
        let mut config = create_valid_microvm_config();
        if let Some(microvm) = &mut config.microvm {
            microvm.resources.mem_mb = 32;
        }
        
        let result = validate_microvm_config(&config).unwrap();
        
        assert!(!result.is_valid);
        assert!(result.errors.iter().any(|e| e.code == "INVALID_MEMORY"));
    }

    #[test]
    fn test_invalid_vsock_cid() {
        let mut config = create_valid_microvm_config();
        if let Some(microvm) = &mut config.microvm {
            microvm.devices.vsock.cid = 0;
        }
        
        let result = validate_microvm_config(&config).unwrap();
        
        assert!(!result.is_valid);
        assert!(result.errors.iter().any(|e| e.code == "INVALID_VSOCK_CID"));
    }

    #[test]
    fn test_nic_enabled_but_no_proxy() {
        let mut config = create_valid_microvm_config();
        config.network.egress.via = "direct".to_string();
        
        let result = validate_microvm_config(&config).unwrap();
        
        assert!(!result.is_valid);
        assert!(result.errors.iter().any(|e| e.code == "INVALID_EGRESS_METHOD"));
    }

    #[test]
    fn test_empty_allowed_domains() {
        let mut config = create_valid_microvm_config();
        config.network.egress.allow_domains.clear();
        
        let result = validate_microvm_config(&config).unwrap();
        
        assert!(!result.is_valid);
        assert!(result.errors.iter().any(|e| e.code == "EMPTY_ALLOWED_DOMAINS"));
    }

    #[test]
    fn test_missing_rootfs_hash() {
        let mut config = create_valid_microvm_config();
        if let Some(microvm) = &mut config.microvm {
            microvm.attestation.expect_rootfs_hash = "".to_string();
        }
        
        let result = validate_microvm_config(&config).unwrap();
        
        assert!(!result.is_valid);
        assert!(result.errors.iter().any(|e| e.code == "MISSING_ROOTFS_HASH"));
    }

    #[test]
    fn test_format_validation_errors() {
        let mut config = create_valid_microvm_config();
        config.orchestrator.isolation.mode = "wasm".to_string();
        
        let result = validate_microvm_config(&config).unwrap();
        let formatted = format_validation_errors(&result);
        
        assert!(formatted.contains("❌ Validation Errors:"));
        assert!(formatted.contains("orchestrator.isolation.mode"));
        assert!(formatted.contains("must be 'microvm'"));
    }
} 