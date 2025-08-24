//! RTFS Configuration Parser
//! 
//! This module provides parsing functionality for RTFS agent configurations,
//! specifically handling the (agent.config ...) form and converting it to
//! the structured AgentConfig type.

use std::collections::HashMap;
use crate::ast::*;
use crate::parser::parse_expression;
use crate::runtime::error::{RuntimeError, RuntimeResult};
use super::types::*;

/// Parser for RTFS agent configurations
pub struct AgentConfigParser;

impl AgentConfigParser {
    /// Parse an agent configuration from RTFS content
    pub fn parse_agent_config(content: &str) -> RuntimeResult<AgentConfig> {
        // Parse the RTFS content
        let expr = parse_expression(content)
            .map_err(|e| RuntimeError::Generic(format!("Failed to parse agent config: {:?}", e)))?;
            
        // Extract agent config from the expression
        Self::extract_agent_config_from_expression(&expr)
    }
    
    /// Parse an agent configuration from a file
    pub fn parse_agent_config_from_file<P: AsRef<std::path::Path>>(file_path: P) -> RuntimeResult<AgentConfig> {
        let content = std::fs::read_to_string(file_path)
            .map_err(|e| RuntimeError::Generic(format!("Failed to read agent config file: {}", e)))?;
            
        Self::parse_agent_config(&content)
    }
    
    /// Extract agent config from an RTFS expression
    pub fn extract_agent_config_from_expression(expr: &Expression) -> RuntimeResult<AgentConfig> {
        match expr {
            Expression::List(list_expr) => {
                // Check if this is an agent.config form
                if let Some(symbol) = list_expr.first() {
                    if let Expression::Symbol(sym) = symbol {
                        if sym.0 == "agent.config" {
                            return Self::parse_agent_config_map(&list_expr[1..]);
                        }
                    }
                }
                Err(RuntimeError::Generic("Expected (agent.config ...) form".to_string()))
            },
            Expression::FunctionCall { callee, arguments } => {
                // Check if this is an agent.config function call
                if let Expression::Symbol(sym) = &**callee {
                    if sym.0 == "agent.config" {
                        return Self::parse_agent_config_map(arguments);
                    }
                }
                Err(RuntimeError::Generic("Expected agent.config function call".to_string()))
            },
            Expression::Map(map_expr) => {
                // Direct map form
                Self::parse_agent_config_map_from_map(map_expr)
            },
            _ => Err(RuntimeError::Generic("Expected agent.config list, function call, or map".to_string())),
        }
    }
    
    /// Parse agent config from a list of key-value pairs
    fn parse_agent_config_map(elements: &[Expression]) -> RuntimeResult<AgentConfig> {
        let mut config = AgentConfig::default();
        
        // Process elements in pairs (keyword, value)
        let mut i = 0;
        while i < elements.len() {
            if i + 1 >= elements.len() {
                return Err(RuntimeError::Generic("Odd number of elements in agent.config".to_string()));
            }
            
            let key_expr = &elements[i];
            let value_expr = &elements[i + 1];
            
            if let Expression::Literal(Literal::Keyword(keyword)) = key_expr {
                Self::set_config_field(&mut config, &keyword.0, value_expr)?;
            } else {
                return Err(RuntimeError::Generic("Expected keyword as config key".to_string()));
            }
            
            i += 2;
        }
        
        Ok(config)
    }
    
    /// Parse agent config from a map expression
    fn parse_agent_config_map_from_map(map_expr: &HashMap<MapKey, Expression>) -> RuntimeResult<AgentConfig> {
        let mut config = AgentConfig::default();
        
        for (key, value) in map_expr {
            if let MapKey::Keyword(keyword) = key {
                Self::set_config_field(&mut config, &keyword.0, value)?;
            } else {
                return Err(RuntimeError::Generic("Expected keyword as config key".to_string()));
            }
        }
        
        Ok(config)
    }
    
    /// Set a configuration field based on the keyword and value
    fn set_config_field(config: &mut AgentConfig, key: &str, value: &Expression) -> RuntimeResult<()> {
        match key {
            "version" => {
                config.version = Self::extract_string(value)?;
            },
            "agent-id" | "agent_id" => {
                config.agent_id = Self::extract_string(value)?;
            },
            "profile" => {
                config.profile = Self::extract_string(value)?;
            },
            "features" => {
                config.features = Self::extract_string_vector(value)?;
            },
            "orchestrator" => {
                config.orchestrator = Self::parse_orchestrator_config(value)?;
            },
            "network" => {
                config.network = Self::parse_network_config(value)?;
            },
            "microvm" => {
                config.microvm = Some(Self::parse_microvm_config(value)?);
            },
            "capabilities" => {
                config.capabilities = Self::parse_capabilities_config(value)?;
            },
            "governance" => {
                config.governance = Self::parse_governance_config(value)?;
            },
            "causal-chain" | "causal_chain" => {
                config.causal_chain = Self::parse_causal_chain_config(value)?;
            },
            "marketplace" => {
                config.marketplace = Self::parse_marketplace_config(value)?;
            },
            "delegation" => {
                config.delegation = Self::parse_delegation_config(value)?;
            },
            _ => {
                // Unknown field - ignore for now
            }
        }
        
        Ok(())
    }
    
    /// Parse orchestrator configuration
    fn parse_orchestrator_config(expr: &Expression) -> RuntimeResult<OrchestratorConfig> {
        match expr {
            Expression::Map(map_expr) => {
                let mut config = OrchestratorConfig::default();
                
                for (key, value) in map_expr {
                    if let MapKey::Keyword(keyword) = key {
                        match keyword.0.as_str() {
                            "isolation" => {
                                config.isolation = Self::parse_isolation_config(value)?;
                            },
                            "dlp" => {
                                config.dlp = Self::parse_dlp_config(value)?;
                            },
                            _ => {}
                        }
                    }
                }
                
                Ok(config)
            },
            _ => Err(RuntimeError::Generic("Expected map for orchestrator config".to_string())),
        }
    }
    
    /// Parse isolation configuration
    fn parse_isolation_config(expr: &Expression) -> RuntimeResult<IsolationConfig> {
        match expr {
            Expression::Map(map_expr) => {
                let mut config = IsolationConfig::default();
                
                for (key, value) in map_expr {
                    if let MapKey::Keyword(keyword) = key {
                        match keyword.0.as_str() {
                            "mode" => {
                                config.mode = Self::extract_string(value)?;
                            },
                            "fs" => {
                                config.fs = Self::parse_fs_config(value)?;
                            },
                            _ => {}
                        }
                    }
                }
                
                Ok(config)
            },
            _ => Err(RuntimeError::Generic("Expected map for isolation config".to_string())),
        }
    }
    
    /// Parse filesystem configuration
    fn parse_fs_config(expr: &Expression) -> RuntimeResult<FSConfig> {
        match expr {
            Expression::Map(map_expr) => {
                let mut config = FSConfig::default();
                
                for (key, value) in map_expr {
                    if let MapKey::Keyword(keyword) = key {
                        match keyword.0.as_str() {
                            "ephemeral" => {
                                config.ephemeral = Self::extract_boolean(value)?;
                            },
                            "mounts" => {
                                config.mounts = Self::parse_mounts_config(value)?;
                            },
                            _ => {}
                        }
                    }
                }
                
                Ok(config)
            },
            _ => Err(RuntimeError::Generic("Expected map for fs config".to_string())),
        }
    }
    
    /// Parse mounts configuration
    fn parse_mounts_config(expr: &Expression) -> RuntimeResult<HashMap<String, MountConfig>> {
        match expr {
            Expression::Map(map_expr) => {
                let mut mounts = HashMap::new();
                
                for (key, value) in map_expr {
                    let mount_name = match key {
                        MapKey::Keyword(k) => k.0.clone(),
                        MapKey::String(s) => s.clone(),
                        _ => continue,
                    };
                    
                    mounts.insert(mount_name, Self::parse_mount_config(value)?);
                }
                
                Ok(mounts)
            },
            _ => Err(RuntimeError::Generic("Expected map for mounts config".to_string())),
        }
    }
    
    /// Parse mount configuration
    fn parse_mount_config(expr: &Expression) -> RuntimeResult<MountConfig> {
        match expr {
            Expression::Map(map_expr) => {
                let mut config = MountConfig {
                    mode: "ro".to_string(), // Default to read-only
                };
                
                for (key, value) in map_expr {
                    if let MapKey::Keyword(keyword) = key {
                        match keyword.0.as_str() {
                            "mode" => {
                                config.mode = Self::extract_string(value)?;
                            },
                            _ => {}
                        }
                    }
                }
                
                Ok(config)
            },
            _ => Err(RuntimeError::Generic("Expected map for mount config".to_string())),
        }
    }
    
    /// Parse DLP configuration
    fn parse_dlp_config(expr: &Expression) -> RuntimeResult<DLPConfig> {
        match expr {
            Expression::Map(map_expr) => {
                let mut config = DLPConfig::default();
                
                for (key, value) in map_expr {
                    if let MapKey::Keyword(keyword) = key {
                        match keyword.0.as_str() {
                            "enabled" => {
                                config.enabled = Self::extract_boolean(value)?;
                            },
                            "policy" => {
                                config.policy = Self::extract_string(value)?;
                            },
                            _ => {}
                        }
                    }
                }
                
                Ok(config)
            },
            _ => Err(RuntimeError::Generic("Expected map for dlp config".to_string())),
        }
    }
    
    /// Parse network configuration
    fn parse_network_config(expr: &Expression) -> RuntimeResult<NetworkConfig> {
        match expr {
            Expression::Map(map_expr) => {
                let mut config = NetworkConfig::default();
                
                for (key, value) in map_expr {
                    if let MapKey::Keyword(keyword) = key {
                        match keyword.0.as_str() {
                            "enabled" => {
                                config.enabled = Self::extract_boolean(value)?;
                            },
                            "egress" => {
                                config.egress = Self::parse_egress_config(value)?;
                            },
                            _ => {}
                        }
                    }
                }
                
                Ok(config)
            },
            _ => Err(RuntimeError::Generic("Expected map for network config".to_string())),
        }
    }
    
    /// Parse egress configuration
    fn parse_egress_config(expr: &Expression) -> RuntimeResult<EgressConfig> {
        match expr {
            Expression::Map(map_expr) => {
                let mut config = EgressConfig::default();
                
                for (key, value) in map_expr {
                    if let MapKey::Keyword(keyword) = key {
                        match keyword.0.as_str() {
                            "via" => {
                                config.via = Self::extract_string(value)?;
                            },
                            "allow-domains" | "allow_domains" => {
                                config.allow_domains = Self::extract_string_vector(value)?;
                            },
                            "mtls" => {
                                config.mtls = Self::extract_boolean(value)?;
                            },
                            "tls-pins" | "tls_pins" => {
                                config.tls_pins = Self::extract_string_vector(value)?;
                            },
                            _ => {}
                        }
                    }
                }
                
                Ok(config)
            },
            _ => Err(RuntimeError::Generic("Expected map for egress config".to_string())),
        }
    }
    
    /// Parse MicroVM configuration
    fn parse_microvm_config(expr: &Expression) -> RuntimeResult<MicroVMConfig> {
        match expr {
            Expression::Map(map_expr) => {
                let mut config = MicroVMConfig {
                    kernel: KernelConfig {
                        image: "".to_string(),
                        cmdline: "".to_string(),
                    },
                    rootfs: RootFSConfig {
                        image: "".to_string(),
                        ro: true,
                    },
                    resources: ResourceConfig {
                        vcpus: 1,
                        mem_mb: 512,
                    },
                    devices: DeviceConfig {
                        nic: NICConfig {
                            enabled: false,
                            proxy_ns: "".to_string(),
                        },
                        vsock: VSockConfig {
                            enabled: false,
                            cid: 0,
                        },
                    },
                    attestation: AttestationConfig {
                        enabled: false,
                        expect_rootfs_hash: "".to_string(),
                    },
                };
                
                for (key, value) in map_expr {
                    if let MapKey::Keyword(keyword) = key {
                        match keyword.0.as_str() {
                            "kernel" => {
                                config.kernel = Self::parse_kernel_config(value)?;
                            },
                            "rootfs" => {
                                config.rootfs = Self::parse_rootfs_config(value)?;
                            },
                            "resources" => {
                                config.resources = Self::parse_resource_config(value)?;
                            },
                            "devices" => {
                                config.devices = Self::parse_device_config(value)?;
                            },
                            "attestation" => {
                                config.attestation = Self::parse_attestation_config(value)?;
                            },
                            _ => {}
                        }
                    }
                }
                
                Ok(config)
            },
            _ => Err(RuntimeError::Generic("Expected map for microvm config".to_string())),
        }
    }
    
    /// Parse kernel configuration
    fn parse_kernel_config(expr: &Expression) -> RuntimeResult<KernelConfig> {
        match expr {
            Expression::Map(map_expr) => {
                let mut config = KernelConfig {
                    image: "".to_string(),
                    cmdline: "".to_string(),
                };
                
                for (key, value) in map_expr {
                    if let MapKey::Keyword(keyword) = key {
                        match keyword.0.as_str() {
                            "image" => {
                                config.image = Self::extract_string(value)?;
                            },
                            "cmdline" => {
                                config.cmdline = Self::extract_string(value)?;
                            },
                            _ => {}
                        }
                    }
                }
                
                Ok(config)
            },
            _ => Err(RuntimeError::Generic("Expected map for kernel config".to_string())),
        }
    }
    
    /// Parse rootfs configuration
    fn parse_rootfs_config(expr: &Expression) -> RuntimeResult<RootFSConfig> {
        match expr {
            Expression::Map(map_expr) => {
                let mut config = RootFSConfig {
                    image: "".to_string(),
                    ro: true,
                };
                
                for (key, value) in map_expr {
                    if let MapKey::Keyword(keyword) = key {
                        match keyword.0.as_str() {
                            "image" => {
                                config.image = Self::extract_string(value)?;
                            },
                            "ro" => {
                                config.ro = Self::extract_boolean(value)?;
                            },
                            _ => {}
                        }
                    }
                }
                
                Ok(config)
            },
            _ => Err(RuntimeError::Generic("Expected map for rootfs config".to_string())),
        }
    }
    
    /// Parse resource configuration
    fn parse_resource_config(expr: &Expression) -> RuntimeResult<ResourceConfig> {
        match expr {
            Expression::Map(map_expr) => {
                let mut config = ResourceConfig {
                    vcpus: 1,
                    mem_mb: 512,
                };
                
                for (key, value) in map_expr {
                    if let MapKey::Keyword(keyword) = key {
                        match keyword.0.as_str() {
                            "vcpus" => {
                                config.vcpus = Self::extract_integer(value)?;
                            },
                            "mem-mb" | "mem_mb" => {
                                config.mem_mb = Self::extract_integer(value)?;
                            },
                            _ => {}
                        }
                    }
                }
                
                Ok(config)
            },
            _ => Err(RuntimeError::Generic("Expected map for resource config".to_string())),
        }
    }
    
    /// Parse device configuration
    fn parse_device_config(expr: &Expression) -> RuntimeResult<DeviceConfig> {
        match expr {
            Expression::Map(map_expr) => {
                let mut config = DeviceConfig {
                    nic: NICConfig {
                        enabled: false,
                        proxy_ns: "".to_string(),
                    },
                    vsock: VSockConfig {
                        enabled: false,
                        cid: 0,
                    },
                };
                
                for (key, value) in map_expr {
                    if let MapKey::Keyword(keyword) = key {
                        match keyword.0.as_str() {
                            "nic" => {
                                config.nic = Self::parse_nic_config(value)?;
                            },
                            "vsock" => {
                                config.vsock = Self::parse_vsock_config(value)?;
                            },
                            _ => {}
                        }
                    }
                }
                
                Ok(config)
            },
            _ => Err(RuntimeError::Generic("Expected map for device config".to_string())),
        }
    }
    
    /// Parse NIC configuration
    fn parse_nic_config(expr: &Expression) -> RuntimeResult<NICConfig> {
        match expr {
            Expression::Map(map_expr) => {
                let mut config = NICConfig {
                    enabled: false,
                    proxy_ns: "".to_string(),
                };
                
                for (key, value) in map_expr {
                    if let MapKey::Keyword(keyword) = key {
                        match keyword.0.as_str() {
                            "enabled" => {
                                config.enabled = Self::extract_boolean(value)?;
                            },
                            "proxy-ns" | "proxy_ns" => {
                                config.proxy_ns = Self::extract_string(value)?;
                            },
                            _ => {}
                        }
                    }
                }
                
                Ok(config)
            },
            _ => Err(RuntimeError::Generic("Expected map for nic config".to_string())),
        }
    }
    
    /// Parse VSock configuration
    fn parse_vsock_config(expr: &Expression) -> RuntimeResult<VSockConfig> {
        match expr {
            Expression::Map(map_expr) => {
                let mut config = VSockConfig {
                    enabled: false,
                    cid: 0,
                };
                
                for (key, value) in map_expr {
                    if let MapKey::Keyword(keyword) = key {
                        match keyword.0.as_str() {
                            "enabled" => {
                                config.enabled = Self::extract_boolean(value)?;
                            },
                            "cid" => {
                                config.cid = Self::extract_integer(value)?;
                            },
                            _ => {}
                        }
                    }
                }
                
                Ok(config)
            },
            _ => Err(RuntimeError::Generic("Expected map for vsock config".to_string())),
        }
    }
    
    /// Parse attestation configuration
    fn parse_attestation_config(expr: &Expression) -> RuntimeResult<AttestationConfig> {
        match expr {
            Expression::Map(map_expr) => {
                let mut config = AttestationConfig {
                    enabled: false,
                    expect_rootfs_hash: "".to_string(),
                };
                
                for (key, value) in map_expr {
                    if let MapKey::Keyword(keyword) = key {
                        match keyword.0.as_str() {
                            "enabled" => {
                                config.enabled = Self::extract_boolean(value)?;
                            },
                            "expect-rootfs-hash" | "expect_rootfs_hash" => {
                                config.expect_rootfs_hash = Self::extract_string(value)?;
                            },
                            _ => {}
                        }
                    }
                }
                
                Ok(config)
            },
            _ => Err(RuntimeError::Generic("Expected map for attestation config".to_string())),
        }
    }
    
    /// Parse capabilities configuration (simplified for now)
    fn parse_capabilities_config(_expr: &Expression) -> RuntimeResult<CapabilitiesConfig> {
        // For now, return default capabilities config
        // This can be expanded later to parse specific capability configurations
        Ok(CapabilitiesConfig::default())
    }
    
    /// Parse governance configuration (simplified for now)
    fn parse_governance_config(_expr: &Expression) -> RuntimeResult<GovernanceConfig> {
        // For now, return default governance config
        // This can be expanded later to parse specific governance configurations
        Ok(GovernanceConfig::default())
    }
    
    /// Parse causal chain configuration (simplified for now)
    fn parse_causal_chain_config(_expr: &Expression) -> RuntimeResult<CausalChainConfig> {
        // For now, return default causal chain config
        // This can be expanded later to parse specific causal chain configurations
        Ok(CausalChainConfig::default())
    }
    
    /// Parse marketplace configuration (simplified for now)
    fn parse_marketplace_config(_expr: &Expression) -> RuntimeResult<MarketplaceConfig> {
        // For now, return default marketplace config
        // This can be expanded later to parse specific marketplace configurations
        Ok(MarketplaceConfig::default())
    }
    
    /// Parse delegation configuration (simplified for now)
    fn parse_delegation_config(_expr: &Expression) -> RuntimeResult<DelegationConfig> {
        // For now, return default delegation config
        // This can be expanded later to parse specific delegation configurations
        Ok(DelegationConfig::default())
    }
    
    // Helper methods for extracting primitive values
    
    fn extract_string(expr: &Expression) -> RuntimeResult<String> {
        match expr {
            Expression::Literal(Literal::String(s)) => Ok(s.clone()),
            Expression::Literal(Literal::Keyword(k)) => Ok(k.0.clone()),
            _ => Err(RuntimeError::Generic("Expected string or keyword".to_string())),
        }
    }
    
    fn extract_boolean(expr: &Expression) -> RuntimeResult<bool> {
        match expr {
            Expression::Literal(Literal::Boolean(b)) => Ok(*b),
            _ => Err(RuntimeError::Generic("Expected boolean".to_string())),
        }
    }
    
    fn extract_integer(expr: &Expression) -> RuntimeResult<u32> {
        match expr {
            Expression::Literal(Literal::Integer(i)) => {
                if *i >= 0 && *i <= u32::MAX as i64 {
                    Ok(*i as u32)
                } else {
                    Err(RuntimeError::Generic("Integer out of range for u32".to_string()))
                }
            },
            _ => Err(RuntimeError::Generic("Expected integer".to_string())),
        }
    }
    
    fn extract_string_vector(expr: &Expression) -> RuntimeResult<Vec<String>> {
        match expr {
            Expression::Vector(vec_expr) => {
                let mut strings = Vec::new();
                for element in vec_expr {
                    strings.push(Self::extract_string(element)?);
                }
                Ok(strings)
            },
            _ => Err(RuntimeError::Generic("Expected vector of strings".to_string())),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_parse_basic_agent_config() {
        let config_content = r#"
        (agent.config
          :version "0.1"
          :agent-id "agent.test"
          :profile :microvm
          :features [:network :telemetry]
          :orchestrator
            {:isolation
              {:mode :microvm
               :fs {:ephemeral true
                    :mounts {:capabilities {:mode :ro}}}}}
          :network
            {:enabled true
             :egress {:via :proxy
                      :allow-domains ["example.com"]
                      :mtls true}}
          :microvm
            {:kernel {:image "kernels/vmlinuz-min"
                      :cmdline "console=ttyS0"}
             :rootfs {:image "rootfs.img"
                      :ro true}
             :resources {:vcpus 2
                         :mem-mb 512}
             :devices {:nic {:enabled true
                            :proxy-ns "proxy"}
                      :vsock {:enabled true
                              :cid 3}}
             :attestation {:enabled true
                          :expect-rootfs-hash "sha256:abc123"}})
        "#;
        
        // Parse the expression first
        let expr = parse_expression(config_content.trim()).unwrap();
        
        // Then extract the config from the expression
        let config = AgentConfigParser::extract_agent_config_from_expression(&expr).unwrap();
        
        assert_eq!(config.version, "0.1");
        assert_eq!(config.agent_id, "agent.test");
        assert_eq!(config.profile, "microvm");
        assert_eq!(config.features, vec!["network", "telemetry"]);
        
        // Check orchestrator config
        assert_eq!(config.orchestrator.isolation.mode, "microvm");
        assert_eq!(config.orchestrator.isolation.fs.ephemeral, true);
        
        // Check network config
        assert_eq!(config.network.enabled, true);
        assert_eq!(config.network.egress.via, "proxy");
        assert_eq!(config.network.egress.allow_domains, vec!["example.com"]);
        assert_eq!(config.network.egress.mtls, true);
        
        // Check microvm config
        let microvm = config.microvm.unwrap();
        assert_eq!(microvm.kernel.image, "kernels/vmlinuz-min");
        assert_eq!(microvm.kernel.cmdline, "console=ttyS0");
        assert_eq!(microvm.rootfs.image, "rootfs.img");
        assert_eq!(microvm.rootfs.ro, true);
        assert_eq!(microvm.resources.vcpus, 2);
        assert_eq!(microvm.resources.mem_mb, 512);
        assert_eq!(microvm.devices.nic.enabled, true);
        assert_eq!(microvm.devices.nic.proxy_ns, "proxy");
        assert_eq!(microvm.devices.vsock.enabled, true);
        assert_eq!(microvm.devices.vsock.cid, 3);
        assert_eq!(microvm.attestation.enabled, true);
        assert_eq!(microvm.attestation.expect_rootfs_hash, "sha256:abc123");
    }
    
    #[test]
    fn test_parse_minimal_agent_config() {
        let config_content = r#"
        (agent.config
          :version "0.1"
          :agent-id "agent.minimal"
          :profile :minimal)
        "#;
        
        // Parse the expression first
        let expr = parse_expression(config_content.trim()).unwrap();
        
        // Then extract the config from the expression
        let config = AgentConfigParser::extract_agent_config_from_expression(&expr).unwrap();
        
        assert_eq!(config.version, "0.1");
        assert_eq!(config.agent_id, "agent.minimal");
        assert_eq!(config.profile, "minimal");
        
        // Should have defaults for other fields
        assert_eq!(config.orchestrator.isolation.mode, "wasm");
        assert_eq!(config.network.enabled, false);
        assert_eq!(config.microvm, None);
    }
    
    #[test]
    fn test_parse_agent_config_from_expression() {
        // Test parsing from a direct expression (for internal testing)
        let config_content = r#"
        (agent.config
          :version "0.1"
          :agent-id "agent.test"
          :profile :microvm)
        "#;
        
        // Parse the expression first
        let expr = parse_expression(config_content.trim()).unwrap();
        
        // Then extract the config from the expression
        let config = AgentConfigParser::extract_agent_config_from_expression(&expr).unwrap();
        
        assert_eq!(config.version, "0.1");
        assert_eq!(config.agent_id, "agent.test");
        assert_eq!(config.profile, "microvm");
    }
}
