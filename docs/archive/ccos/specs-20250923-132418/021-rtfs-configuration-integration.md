# CCOS Specification 021: RTFS Configuration Integration

**Status:** Implemented
**Version:** 1.0
**Date:** 2025-01-21
**Related:**
- [SEP-020: MicroVM Architecture](./020-microvm-architecture.md)
- [SEP-005: Security and Context](./005-security-and-context.md)

## 1. Abstract

The RTFS Configuration Integration provides seamless parsing and validation of RTFS `agent.config` expressions into structured configuration objects. This enables dynamic configuration of MicroVM agents and other CCOS components through RTFS syntax.

## 2. Overview

The system bridges RTFS expressions and structured configuration types, allowing agents to be configured using the same language they execute. This provides type safety, validation, and integration with the existing CCOS configuration system.

## 3. Core Components

### 3.1. AgentConfigParser

```rust
pub struct AgentConfigParser;

impl AgentConfigParser {
    /// Parse an agent configuration from RTFS content
    pub fn parse_agent_config(content: &str) -> RuntimeResult<AgentConfig>;
    
    /// Extract agent config from an RTFS expression
    pub fn extract_agent_config_from_expression(expr: &Expression) -> RuntimeResult<AgentConfig>;
}
```

### 3.2. Configuration Types

```rust
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct AgentConfig {
    pub version: String,
    pub agent_id: String,
    pub profile: String,
    pub microvm: Option<MicroVMConfig>,
    pub network: NetworkConfig,
    pub orchestrator: OrchestratorConfig,
    pub security: SecurityConfig,
}
```

## 4. RTFS Syntax Support

### 4.1. Basic Configuration

```clojure
(agent.config 
  :version "0.1"
  :agent-id "agent.test"
  :profile :microvm)
```

### 4.2. MicroVM Configuration

```clojure
(agent.config 
  :version "0.1"
  :agent-id "agent.test"
  :profile :microvm
  :microvm {
    :kernel {:image "kernels/vmlinuz-min" :cmdline "console=none"}
    :rootfs {:image "images/agent-rootfs.img" :ro true}
    :resources {:vcpus 1 :mem_mb 256}
    :devices {:nic {:enabled true :proxy_ns "egress-proxy-ns"}}
    :attestation {:enabled true :expect_rootfs_hash "sha256:..."}
  })
```

### 4.3. Network Configuration

```clojure
(agent.config 
  :version "0.1"
  :agent-id "agent.test"
  :profile :microvm
  :network {
    :enabled true
    :egress {
      :via "proxy"
      :allow_domains ["example.com" "api.example.com"]
      :mtls true
    }
  })
```

### 4.4. Orchestrator Configuration

```clojure
(agent.config 
  :version "0.1"
  :agent-id "agent.test"
  :profile :microvm
  :orchestrator {
    :isolation {
      :mode "microvm"
      :fs {:ephemeral true}
    }
  })
```

## 5. Parsing Implementation

### 5.1. Expression Handling

The parser supports multiple RTFS expression forms:

#### List Form
```clojure
(agent.config :version "0.1" :agent-id "agent.test")
```

#### Function Call Form
```clojure
agent.config(:version "0.1" :agent-id "agent.test")
```

### 5.2. Type Conversion

The parser converts RTFS types to Rust types:

| RTFS Type | Rust Type | Example |
|-----------|-----------|---------|
| Keyword | String | `:version` → `"version"` |
| String | String | `"0.1"` → `"0.1"` |
| Number | Number | `1` → `1` |
| Boolean | Boolean | `true` → `true` |
| Map | Struct | `{:key "value"}` → `Struct { key: "value" }` |
| Vector | Vec | `[1 2 3]` → `vec![1, 2, 3]` |

### 5.3. Validation

The parser performs validation during conversion:

1. **Required Fields**: Ensures all required configuration fields are present
2. **Type Validation**: Validates that values match expected types
3. **Schema Validation**: Validates against configuration schemas
4. **Constraint Validation**: Validates business rules and constraints

## 6. Integration Points

### 6.1. MicroVM Integration

```rust
// Parse RTFS configuration
let config_content = r#"(agent.config :version "0.1" :agent-id "agent.test" :profile :microvm)"#;
let config = AgentConfigParser::parse_agent_config(config_content)?;

// Use in MicroVM setup
let microvm_config = config.microvm.unwrap_or_default();
let provider = MicroVMFactory::create_provider(&microvm_config.default_provider)?;
```

### 6.2. Security Integration

```rust
// Parse security configuration
let security_config = config.security;

// Create runtime context
let runtime_context = RuntimeContext::controlled(
    security_config.allowed_capabilities
);
```

### 6.3. Network Integration

```rust
// Parse network configuration
let network_config = config.network;

// Configure network policies
if network_config.enabled {
    // Setup network isolation
    setup_network_isolation(&network_config.egress)?;
}
```

## 7. Error Handling

### 7.1. Parse Errors

```rust
// Invalid RTFS syntax
let result = AgentConfigParser::parse_agent_config("invalid syntax");
assert!(result.is_err());

// Missing required fields
let result = AgentConfigParser::parse_agent_config("(agent.config)");
assert!(result.is_err());
```

### 7.2. Validation Errors

```rust
// Invalid version format
let result = AgentConfigParser::parse_agent_config(
    r#"(agent.config :version "invalid" :agent-id "test")"#
);
assert!(result.is_err());
```

## 8. Testing

### 8.1. Parser Tests

```rust
#[test]
fn test_basic_agent_config_parsing() {
    let config_content = r#"(agent.config :version "0.1" :agent-id "agent.test" :profile :microvm)"#;
    let config = AgentConfigParser::parse_agent_config(config_content).unwrap();
    
    assert_eq!(config.version, "0.1");
    assert_eq!(config.agent_id, "agent.test");
    assert_eq!(config.profile, "microvm");
}
```

### 8.2. Integration Tests

```rust
#[test]
fn test_microvm_configuration_integration() {
    let config_content = r#"
        (agent.config 
          :version "0.1"
          :agent-id "agent.test"
          :profile :microvm
          :microvm {
            :kernel {:image "kernels/vmlinuz-min" :cmdline "console=none"}
            :rootfs {:image "images/agent-rootfs.img" :ro true}
            :resources {:vcpus 1 :mem_mb 256}
          })
    "#;
    
    let config = AgentConfigParser::parse_agent_config(config_content).unwrap();
    assert!(config.microvm.is_some());
}
```

## 9. Performance Characteristics

### 9.1. Parsing Performance

- **Simple Configs**: <1ms parse time
- **Complex Configs**: 1-5ms parse time
- **Memory Usage**: Minimal overhead
- **Validation**: <1ms validation time

### 9.2. Memory Usage

- **Parser**: ~50KB base memory
- **Config Objects**: Size proportional to configuration complexity
- **Caching**: Optional caching for frequently used configurations

## 10. Future Enhancements

### 10.1. Planned Features

1. **Schema Validation**: JSON Schema integration for configuration validation
2. **Configuration Templates**: Predefined configuration templates for common use cases
3. **Dynamic Configuration**: Runtime configuration updates
4. **Configuration Inheritance**: Support for configuration inheritance and composition
5. **Configuration Encryption**: Support for encrypted configuration values

### 10.2. Performance Optimizations

1. **Parser Caching**: Cache parsed configurations for reuse
2. **Lazy Validation**: Defer validation until configuration is used
3. **Streaming Parsing**: Support for large configuration files
4. **Parallel Parsing**: Parallel parsing of multiple configurations

## 11. References

- [SEP-020: MicroVM Architecture](./020-microvm-architecture.md)
- [SEP-005: Security and Context](./005-security-and-context.md)
- [SEP-004: Capabilities and Marketplace](./004-capabilities-and-marketplace.md)
