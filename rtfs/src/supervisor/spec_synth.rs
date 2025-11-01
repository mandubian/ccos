//! Firecracker/Cloud Hypervisor spec synthesis
//!
//! This module transforms RTFS agent.config.microvm into a Firecracker-style
//! machine configuration JSON for actual VM deployment.

use crate::config::types::*;
use crate::runtime::error::{RuntimeError, RuntimeResult};
use serde::{Deserialize, Serialize};

/// Firecracker machine configuration specification
/// Maps to the Firecracker API v1.0.0 specification
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct FirecrackerSpec {
    /// Boot source configuration
    pub boot_source: BootSource,
    /// Drive configurations
    pub drives: Vec<Drive>,
    /// Network interface configurations
    pub network_interfaces: Vec<NetworkInterface>,
    /// Machine configuration
    pub machine_config: MachineConfig,
    /// VSock configuration
    pub vsock: Option<VSockConfig>,
    /// Balloon device configuration (optional)
    pub balloon: Option<BalloonConfig>,
    /// Logger configuration
    pub logger: Option<LoggerConfig>,
    /// Metrics configuration
    pub metrics: Option<MetricsConfig>,
    /// Entropy device configuration
    pub entropy: Option<EntropyConfig>,
}

/// Boot source configuration
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct BootSource {
    /// Kernel image path
    pub kernel_image_path: String,
    /// Kernel command line arguments
    pub boot_args: Option<String>,
    /// Initrd path (optional)
    pub initrd_path: Option<String>,
}

/// Drive configuration
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Drive {
    /// Drive ID
    pub drive_id: String,
    /// Path to the drive image
    pub path_on_host: String,
    /// Whether the drive is read-only
    pub is_read_only: bool,
    /// Whether the drive is the root device
    pub is_root_device: bool,
    /// Cache type for the drive
    pub cache_type: Option<String>,
    /// Rate limiter configuration (optional)
    pub rate_limiter: Option<RateLimiter>,
}

/// Network interface configuration
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct NetworkInterface {
    /// Interface ID
    pub iface_id: String,
    /// Host device name
    pub host_dev_name: String,
    /// Guest MAC address (optional)
    pub guest_mac: Option<String>,
    /// Rate limiter configuration (optional)
    pub rx_rate_limiter: Option<RateLimiter>,
    /// Rate limiter configuration (optional)
    pub tx_rate_limiter: Option<RateLimiter>,
    /// Allow MMDS requests
    pub allow_mmds_requests: Option<bool>,
}

/// Machine configuration
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct MachineConfig {
    /// Number of vCPUs
    pub vcpu_count: u32,
    /// Memory size in MB
    pub mem_size_mib: u32,
    /// Whether to enable SMT (Simultaneous Multi-Threading)
    pub smt: Option<bool>,
    /// CPU template (optional)
    pub cpu_template: Option<String>,
    /// Track dirty pages
    pub track_dirty_pages: Option<bool>,
}

/// VSock configuration
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct VSockConfig {
    /// VSock device ID
    pub vsock_id: String,
    /// Guest CID
    pub guest_cid: u32,
    /// Unix domain socket path
    pub uds_path: String,
}

/// Balloon device configuration
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct BalloonConfig {
    /// Balloon device ID
    pub balloon_id: String,
    /// Target balloon size in MB
    pub amount_mib: u32,
    /// Whether to deflate the balloon on VM exit
    pub deflate_on_oom: Option<bool>,
    /// Statistics polling interval in seconds
    pub stats_polling_interval_s: Option<u32>,
}

/// Logger configuration
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct LoggerConfig {
    /// Log file path
    pub log_fifo: Option<String>,
    /// Metrics file path
    pub metrics_fifo: Option<String>,
    /// Log level
    pub level: Option<String>,
    /// Whether to show log level
    pub show_level: Option<bool>,
    /// Whether to show log origin
    pub show_log_origin: Option<bool>,
}

/// Metrics configuration
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct MetricsConfig {
    /// Metrics file path
    pub metrics_fifo: String,
}

/// Entropy device configuration
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct EntropyConfig {
    /// Entropy device ID
    pub entropy_id: String,
}

/// Rate limiter configuration
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct RateLimiter {
    /// Bandwidth configuration
    pub bandwidth: Option<TokenBucket>,
    /// Operations configuration
    pub ops: Option<TokenBucket>,
}

/// Token bucket configuration
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct TokenBucket {
    /// Bucket size
    pub size: u32,
    /// Token refill time in milliseconds
    pub refill_time: u32,
    /// One time burst (optional)
    pub one_time_burst: Option<u32>,
}

/// Synthesizes a Firecracker machine specification from an agent configuration
///
/// This function transforms the RTFS agent.config.microvm into a complete
/// Firecracker JSON specification that can be used to spawn a MicroVM.
///
/// # Arguments
/// * `agent_config` - The agent configuration containing MicroVM settings
///
/// # Returns
/// * `FirecrackerSpec` - Complete Firecracker machine specification
///
/// # Errors
/// * Returns `RuntimeError` if the configuration is invalid or missing required fields
pub fn synthesize_vm_spec(agent_config: &AgentConfig) -> RuntimeResult<FirecrackerSpec> {
    // Extract MicroVM configuration
    let microvm_config = agent_config.microvm.as_ref().ok_or_else(|| {
        RuntimeError::InvalidArgument(
            "Agent configuration does not contain MicroVM settings".to_string(),
        )
    })?;

    // Build boot source configuration
    let boot_source = BootSource {
        kernel_image_path: microvm_config.kernel.image.clone(),
        boot_args: Some(microvm_config.kernel.cmdline.clone()),
        initrd_path: None, // Not used in our minimal configuration
    };

    // Build root drive configuration (read-only rootfs)
    let root_drive = Drive {
        drive_id: "rootfs".to_string(),
        path_on_host: microvm_config.rootfs.image.clone(),
        is_read_only: microvm_config.rootfs.ro,
        is_root_device: true,
        cache_type: Some("Unsafe".to_string()), // Use unsafe cache for better performance
        rate_limiter: None,
    };

    // Build machine configuration
    let machine_config = MachineConfig {
        vcpu_count: microvm_config.resources.vcpus,
        mem_size_mib: microvm_config.resources.mem_mb,
        smt: Some(false), // Disable SMT for security
        cpu_template: None,
        track_dirty_pages: Some(false),
    };

    // Build network interface if enabled
    let mut network_interfaces = Vec::new();
    if microvm_config.devices.nic.enabled {
        let network_interface = NetworkInterface {
            iface_id: "eth0".to_string(),
            host_dev_name: format!("tap-{}", agent_config.agent_id.replace(".", "-")),
            guest_mac: None, // Let Firecracker assign MAC
            rx_rate_limiter: None,
            tx_rate_limiter: None,
            allow_mmds_requests: Some(false), // Disable MMDS for security
        };
        network_interfaces.push(network_interface);
    }

    // Build VSock configuration if enabled
    let vsock = if microvm_config.devices.vsock.enabled {
        Some(VSockConfig {
            vsock_id: "vsock0".to_string(),
            guest_cid: microvm_config.devices.vsock.cid,
            uds_path: format!(
                "/tmp/firecracker-vsock-{}.sock",
                agent_config.agent_id.replace(".", "-")
            ),
        })
    } else {
        None
    };

    // Build logger configuration
    let logger = Some(LoggerConfig {
        log_fifo: Some(format!(
            "/tmp/firecracker-{}-log.fifo",
            agent_config.agent_id.replace(".", "-")
        )),
        metrics_fifo: Some(format!(
            "/tmp/firecracker-{}-metrics.fifo",
            agent_config.agent_id.replace(".", "-")
        )),
        level: Some("Info".to_string()),
        show_level: Some(true),
        show_log_origin: Some(false),
    });

    // Build metrics configuration
    let metrics = Some(MetricsConfig {
        metrics_fifo: format!(
            "/tmp/firecracker-{}-metrics.fifo",
            agent_config.agent_id.replace(".", "-")
        ),
    });

    // Build entropy device for better randomness
    let entropy = Some(EntropyConfig {
        entropy_id: "entropy0".to_string(),
    });

    // Build balloon device for memory management
    let balloon = Some(BalloonConfig {
        balloon_id: "balloon0".to_string(),
        amount_mib: microvm_config.resources.mem_mb,
        deflate_on_oom: Some(true),
        stats_polling_interval_s: Some(1),
    });

    Ok(FirecrackerSpec {
        boot_source,
        drives: vec![root_drive],
        network_interfaces,
        machine_config,
        vsock,
        balloon,
        logger,
        metrics,
        entropy,
    })
}

/// Serializes a Firecracker specification to JSON string
///
/// # Arguments
/// * `spec` - The Firecracker specification to serialize
///
/// # Returns
/// * `String` - JSON representation of the specification
///
/// # Errors
/// * Returns `RuntimeError` if serialization fails
pub fn serialize_firecracker_spec(spec: &FirecrackerSpec) -> RuntimeResult<String> {
    serde_json::to_string_pretty(spec).map_err(|e| {
        RuntimeError::JsonError(format!("Failed to serialize Firecracker spec: {}", e))
    })
}

/// Creates a minimal Firecracker specification for testing
///
/// This function creates a basic Firecracker spec that can be used for
/// testing and development purposes.
///
/// # Arguments
/// * `agent_id` - The agent identifier
/// * `kernel_path` - Path to the kernel image
/// * `rootfs_path` - Path to the rootfs image
///
/// # Returns
/// * `FirecrackerSpec` - Minimal Firecracker specification
pub fn create_minimal_firecracker_spec(
    agent_id: &str,
    kernel_path: &str,
    rootfs_path: &str,
) -> FirecrackerSpec {
    FirecrackerSpec {
        boot_source: BootSource {
            kernel_image_path: kernel_path.to_string(),
            boot_args: Some("console=ttyS0 reboot=k panic=1 pci=off".to_string()),
            initrd_path: None,
        },
        drives: vec![Drive {
            drive_id: "rootfs".to_string(),
            path_on_host: rootfs_path.to_string(),
            is_read_only: true,
            is_root_device: true,
            cache_type: Some("Unsafe".to_string()),
            rate_limiter: None,
        }],
        network_interfaces: vec![],
        machine_config: MachineConfig {
            vcpu_count: 1,
            mem_size_mib: 128,
            smt: Some(false),
            cpu_template: None,
            track_dirty_pages: Some(false),
        },
        vsock: Some(VSockConfig {
            vsock_id: "vsock0".to_string(),
            guest_cid: 3,
            uds_path: format!("/tmp/firecracker-vsock-{}.sock", agent_id.replace(".", "-")),
        }),
        balloon: Some(BalloonConfig {
            balloon_id: "balloon0".to_string(),
            amount_mib: 128,
            deflate_on_oom: Some(true),
            stats_polling_interval_s: Some(1),
        }),
        logger: Some(LoggerConfig {
            log_fifo: Some(format!(
                "/tmp/firecracker-{}-log.fifo",
                agent_id.replace(".", "-")
            )),
            metrics_fifo: Some(format!(
                "/tmp/firecracker-{}-metrics.fifo",
                agent_id.replace(".", "-")
            )),
            level: Some("Info".to_string()),
            show_level: Some(true),
            show_log_origin: Some(false),
        }),
        metrics: Some(MetricsConfig {
            metrics_fifo: format!(
                "/tmp/firecracker-{}-metrics.fifo",
                agent_id.replace(".", "-")
            ),
        }),
        entropy: Some(EntropyConfig {
            entropy_id: "entropy0".to_string(),
        }),
    }
}

/// Validates a Firecracker specification
///
/// This function performs basic validation on a Firecracker specification
/// to ensure it meets the requirements for deployment.
///
/// # Arguments
/// * `spec` - The Firecracker specification to validate
///
/// # Returns
/// * `RuntimeResult<()>` - Success if valid, error if invalid
pub fn validate_firecracker_spec(spec: &FirecrackerSpec) -> RuntimeResult<()> {
    // Validate boot source
    if spec.boot_source.kernel_image_path.is_empty() {
        return Err(RuntimeError::InvalidArgument(
            "Kernel image path cannot be empty".to_string(),
        ));
    }

    // Validate drives
    if spec.drives.is_empty() {
        return Err(RuntimeError::InvalidArgument(
            "At least one drive must be configured".to_string(),
        ));
    }

    // Check for root device
    let has_root_device = spec.drives.iter().any(|drive| drive.is_root_device);
    if !has_root_device {
        return Err(RuntimeError::InvalidArgument(
            "Exactly one drive must be configured as root device".to_string(),
        ));
    }

    // Validate machine configuration
    if spec.machine_config.vcpu_count == 0 {
        return Err(RuntimeError::InvalidArgument(
            "vCPU count must be greater than 0".to_string(),
        ));
    }

    if spec.machine_config.mem_size_mib == 0 {
        return Err(RuntimeError::InvalidArgument(
            "Memory size must be greater than 0".to_string(),
        ));
    }

    // Validate VSock configuration if present
    if let Some(vsock) = &spec.vsock {
        if vsock.guest_cid == 0 {
            return Err(RuntimeError::InvalidArgument(
                "VSock guest CID must be greater than 0".to_string(),
            ));
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn create_test_agent_config() -> AgentConfig {
        let mut config = AgentConfig::default();
        config.agent_id = "agent.test.microvm".to_string();
        config.orchestrator.isolation.mode = "microvm".to_string();
        config.orchestrator.isolation.fs.ephemeral = true;
        config.orchestrator.isolation.fs.mounts.insert(
            "capabilities".to_string(),
            crate::config::types::MountConfig {
                mode: "ro".to_string(),
            },
        );
        config.network.enabled = true;
        config.network.egress.via = "proxy".to_string();
        config.network.egress.allow_domains = vec!["example.com".to_string()];
        config.microvm = Some(MicroVMConfig {
            kernel: KernelConfig {
                image: "/path/to/kernel".to_string(),
                cmdline: "console=ttyS0".to_string(),
            },
            rootfs: RootFSConfig {
                image: "/path/to/rootfs".to_string(),
                ro: true,
            },
            resources: ResourceConfig {
                vcpus: 1,
                mem_mb: 256,
            },
            devices: DeviceConfig {
                nic: NICConfig {
                    enabled: true,
                    proxy_ns: "proxy-ns".to_string(),
                },
                vsock: crate::config::types::VSockConfig {
                    enabled: true,
                    cid: 3,
                },
            },
            attestation: AttestationConfig {
                enabled: true,
                expect_rootfs_hash: "sha256:abc123...".to_string(),
            },
        });
        config
    }

    #[test]
    fn test_synthesize_vm_spec() {
        let config = create_test_agent_config();
        let spec = synthesize_vm_spec(&config).unwrap();

        // Verify boot source
        assert_eq!(spec.boot_source.kernel_image_path, "/path/to/kernel");
        assert_eq!(
            spec.boot_source.boot_args,
            Some("console=ttyS0".to_string())
        );

        // Verify root drive
        assert_eq!(spec.drives.len(), 1);
        let root_drive = &spec.drives[0];
        assert_eq!(root_drive.drive_id, "rootfs");
        assert_eq!(root_drive.path_on_host, "/path/to/rootfs");
        assert!(root_drive.is_read_only);
        assert!(root_drive.is_root_device);

        // Verify machine configuration
        assert_eq!(spec.machine_config.vcpu_count, 1);
        assert_eq!(spec.machine_config.mem_size_mib, 256);

        // Verify network interface
        assert_eq!(spec.network_interfaces.len(), 1);
        let nic = &spec.network_interfaces[0];
        assert_eq!(nic.iface_id, "eth0");

        // Verify VSock
        assert!(spec.vsock.is_some());
        let vsock = spec.vsock.as_ref().unwrap();
        assert_eq!(vsock.guest_cid, 3);
    }

    #[test]
    fn test_synthesize_vm_spec_no_microvm() {
        let mut config = AgentConfig::default();
        config.microvm = None;

        let result = synthesize_vm_spec(&config);
        assert!(result.is_err());
        assert!(matches!(
            result.unwrap_err(),
            RuntimeError::InvalidArgument(_)
        ));
    }

    #[test]
    fn test_serialize_firecracker_spec() {
        let spec =
            create_minimal_firecracker_spec("agent.test", "/path/to/kernel", "/path/to/rootfs");

        let json = serialize_firecracker_spec(&spec).unwrap();
        assert!(json.contains("boot_source"));
        assert!(json.contains("drives"));
        assert!(json.contains("machine_config"));
    }

    #[test]
    fn test_validate_firecracker_spec_valid() {
        let spec =
            create_minimal_firecracker_spec("agent.test", "/path/to/kernel", "/path/to/rootfs");

        let result = validate_firecracker_spec(&spec);
        assert!(result.is_ok());
    }

    #[test]
    fn test_validate_firecracker_spec_invalid_vcpu() {
        let mut spec =
            create_minimal_firecracker_spec("agent.test", "/path/to/kernel", "/path/to/rootfs");
        spec.machine_config.vcpu_count = 0;

        let result = validate_firecracker_spec(&spec);
        assert!(result.is_err());
    }

    #[test]
    fn test_validate_firecracker_spec_invalid_memory() {
        let mut spec =
            create_minimal_firecracker_spec("agent.test", "/path/to/kernel", "/path/to/rootfs");
        spec.machine_config.mem_size_mib = 0;

        let result = validate_firecracker_spec(&spec);
        assert!(result.is_err());
    }

    #[test]
    fn test_validate_firecracker_spec_no_root_device() {
        let mut spec =
            create_minimal_firecracker_spec("agent.test", "/path/to/kernel", "/path/to/rootfs");
        spec.drives[0].is_root_device = false;

        let result = validate_firecracker_spec(&spec);
        assert!(result.is_err());
    }

    #[test]
    fn test_create_minimal_firecracker_spec() {
        let spec =
            create_minimal_firecracker_spec("agent.test", "/path/to/kernel", "/path/to/rootfs");

        assert_eq!(spec.boot_source.kernel_image_path, "/path/to/kernel");
        assert_eq!(spec.drives.len(), 1);
        assert_eq!(spec.machine_config.vcpu_count, 1);
        assert_eq!(spec.machine_config.mem_size_mib, 128);
        assert!(spec.vsock.is_some());
        assert!(spec.balloon.is_some());
        assert!(spec.logger.is_some());
        assert!(spec.metrics.is_some());
        assert!(spec.entropy.is_some());
    }
}
