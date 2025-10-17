# Missing Capability Resolution Configuration Guide

## Overview

This document provides comprehensive configuration options for the missing capability resolution system, including feature flags, security settings, and deployment guidelines.

## Feature Flags

The missing capability resolution system uses feature flags to control functionality in different environments:

### Core Feature Flags

| Flag | Default | Description |
|------|---------|-------------|
| `enabled` | `false` | Master switch for the entire missing capability resolution system |
| `runtime_detection` | `false` | Enable runtime detection of missing capabilities |
| `auto_resolution` | `false` | Enable automatic resolution attempts |
| `auto_resume_enabled` | `false` | Enable auto-resume of paused executions |

### Discovery Feature Flags

| Flag | Default | Description |
|------|---------|-------------|
| `mcp_registry_enabled` | `true` | Enable MCP Registry integration |
| `importers_enabled` | `false` | Enable OpenAPI/GraphQL importers |
| `http_wrapper_enabled` | `false` | Enable HTTP/JSON generic wrapper |
| `llm_synthesis_enabled` | `false` | Enable LLM synthesis with guardrails |
| `web_search_enabled` | `false` | Enable web search discovery fallback |

### Operational Feature Flags

| Flag | Default | Description |
|------|---------|-------------|
| `continuous_resolution` | `false` | Enable continuous resolution loop |
| `human_approval_required` | `true` | Require human approval for high-risk resolutions |
| `audit_logging_enabled` | `true` | Enable audit logging for all activities |
| `validation_enabled` | `true` | Enable validation harness and governance |
| `cli_tooling_enabled` | `true` | Enable CLI tooling and observability |

## Configuration Profiles

### Development Profile

```rust
let config = MissingCapabilityConfig {
    feature_flags: MissingCapabilityFeatureFlags::development(),
    ..Default::default()
};
```

**Characteristics:**
- All features enabled
- Human approval disabled
- External dependencies allowed
- Relaxed security settings

### Production Profile

```rust
let config = MissingCapabilityConfig::default();
```

**Characteristics:**
- Conservative defaults
- Human approval required
- External dependencies restricted
- Strict security settings

### Testing Profile

```rust
let config = MissingCapabilityConfig {
    feature_flags: MissingCapabilityFeatureFlags::testing(),
    ..Default::default()
};
```

**Characteristics:**
- Minimal features enabled
- Mock data only
- No external dependencies
- Fast execution

## Environment Variables

Configure the system using environment variables:

### Core Configuration

```bash
# Enable missing capability resolution
export CCOS_MISSING_CAPABILITY_ENABLED=true

# Enable auto-resolution
export CCOS_AUTO_RESOLUTION_ENABLED=true

# Maximum resolution attempts
export CCOS_MAX_RESOLUTION_ATTEMPTS=3

# Resolution timeout (seconds)
export CCOS_RESOLUTION_TIMEOUT_SECONDS=30

# Maximum concurrent resolutions
export CCOS_MAX_CONCURRENT_RESOLUTIONS=5

# Human approval timeout (seconds)
export CCOS_HUMAN_APPROVAL_TIMEOUT_SECONDS=3600
```

### Feature Flags

```bash
# Enable MCP Registry integration
export CCOS_MCP_REGISTRY_ENABLED=true

# Enable LLM synthesis
export CCOS_LLM_SYNTHESIS_ENABLED=false

# Enable web search discovery
export CCOS_WEB_SEARCH_ENABLED=false

# Require human approval
export CCOS_HUMAN_APPROVAL_REQUIRED=true
```

### Security Configuration

```bash
# Require HTTPS for external calls
export CCOS_REQUIRE_HTTPS=true

# Allowed domains (comma-separated)
export CCOS_ALLOWED_DOMAINS="registry.modelcontextprotocol.io,api.github.com"

# Blocked domains (comma-separated)
export CCOS_BLOCKED_DOMAINS="malicious-site.com,phishing-site.com"
```

### MCP Registry Configuration

```bash
# MCP Registry base URL
export CCOS_MCP_REGISTRY_BASE_URL="https://registry.modelcontextprotocol.io"

# MCP Registry timeout (seconds)
export CCOS_MCP_REGISTRY_TIMEOUT_SECONDS=10
```

## Configuration File

Create a `missing-capability-config.json` file:

```json
{
  "feature_flags": {
    "enabled": true,
    "runtime_detection": true,
    "auto_resolution": false,
    "mcp_registry_enabled": true,
    "importers_enabled": false,
    "http_wrapper_enabled": false,
    "llm_synthesis_enabled": false,
    "web_search_enabled": false,
    "continuous_resolution": false,
    "auto_resume_enabled": false,
    "human_approval_required": true,
    "audit_logging_enabled": true,
    "validation_enabled": true,
    "cli_tooling_enabled": true
  },
  "max_resolution_attempts": 3,
  "resolution_timeout_seconds": 30,
  "max_concurrent_resolutions": 5,
  "human_approval_timeout_seconds": 3600,
  "max_pending_capabilities": 100,
  "security_config": {
    "require_https": true,
    "allowed_domains": [
      "registry.modelcontextprotocol.io",
      "api.github.com",
      "api.openai.com"
    ],
    "blocked_domains": [],
    "max_request_size_bytes": 10485760,
    "max_response_size_bytes": 52428800,
    "require_auth": true,
    "max_execution_time_seconds": 300
  },
  "mcp_registry_config": {
    "base_url": "https://registry.modelcontextprotocol.io",
    "timeout_seconds": 10,
    "max_servers": 100,
    "cache_ttl_seconds": 3600
  },
  "llm_synthesis_config": {
    "max_tokens": 4000,
    "temperature": 0.1,
    "max_attempts": 3,
    "require_human_approval": true,
    "allowed_capability_types": [
      "utility",
      "data_processing",
      "format_conversion"
    ]
  },
  "web_search_config": {
    "enabled": false,
    "max_results": 10,
    "timeout_seconds": 15,
    "allowed_search_engines": [
      "duckduckgo",
      "bing"
    ]
  }
}
```

## Security Considerations

### Network Security

1. **HTTPS Enforcement**: Always require HTTPS for external API calls
2. **Domain Allowlisting**: Restrict external API calls to trusted domains
3. **Request/Response Limits**: Set appropriate size limits to prevent abuse
4. **Authentication**: Require authentication for external API calls

### Code Security

1. **LLM Synthesis**: Require human approval for synthesized capabilities
2. **Validation**: Enable validation harness for all capabilities
3. **Audit Logging**: Enable comprehensive audit logging
4. **Execution Limits**: Set maximum execution time for capabilities

### Data Security

1. **Sensitive Data**: Never log sensitive data in audit logs
2. **Token Management**: Use secure token storage and rotation
3. **Access Control**: Implement proper access controls for resolution features

## Deployment Strategies

### Gradual Rollout

1. **Phase 1**: Enable with `human_approval_required=true`
2. **Phase 2**: Enable auto-resolution for low-risk capabilities
3. **Phase 3**: Enable continuous resolution loop
4. **Phase 4**: Enable LLM synthesis with strict guardrails

### Monitoring and Observability

1. **Metrics**: Monitor resolution success rates and performance
2. **Alerts**: Set up alerts for failed resolutions and security violations
3. **Logs**: Review audit logs regularly for security issues
4. **Dashboards**: Create dashboards for resolution pipeline health

## Troubleshooting

### Common Issues

1. **Resolution Failures**: Check network connectivity and API keys
2. **Timeout Issues**: Increase timeout values for slow external APIs
3. **Security Violations**: Review domain allowlists and HTTPS requirements
4. **Performance Issues**: Reduce concurrent resolution limits

### Debug Mode

Enable debug logging:

```bash
export RUST_LOG=debug
export CCOS_DEBUG_MISSING_CAPABILITY=true
```

### Health Checks

Use the CLI tool to check system health:

```bash
# Check resolution queue status
cargo run --bin resolve-deps -- show-queue

# Check system statistics
cargo run --bin resolve-deps -- stats

# Validate configuration
cargo run --bin resolve-deps -- validate
```

## Best Practices

### Configuration Management

1. **Version Control**: Keep configuration files in version control
2. **Environment Separation**: Use different configs for dev/staging/prod
3. **Secrets Management**: Use secure secret management for API keys
4. **Documentation**: Document all configuration changes

### Operational Excellence

1. **Monitoring**: Set up comprehensive monitoring and alerting
2. **Backup**: Regular backup of resolution queue and audit logs
3. **Testing**: Regular testing of resolution pipeline
4. **Updates**: Keep external dependencies updated

### Security

1. **Least Privilege**: Use minimal required permissions
2. **Regular Audits**: Regular security audits of configuration
3. **Incident Response**: Have incident response procedures
4. **Training**: Train operators on security best practices

## Migration Guide

### From Stub System

1. **Disable Stubs**: Set `enabled=false` initially
2. **Enable Detection**: Enable `runtime_detection=true`
3. **Test Resolution**: Test resolution with `human_approval_required=true`
4. **Enable Auto-Resolution**: Gradually enable auto-resolution
5. **Remove Stubs**: Remove stub-related code

### From Manual Resolution

1. **Enable System**: Set `enabled=true`
2. **Configure Discovery**: Enable appropriate discovery methods
3. **Set Up Monitoring**: Configure monitoring and alerting
4. **Train Operators**: Train operators on new system
5. **Gradual Rollout**: Roll out to production gradually

## Support

For configuration issues:

1. **Documentation**: Check this guide and related documentation
2. **Logs**: Review system logs for error messages
3. **Community**: Ask questions in the community forum
4. **Support**: Contact support for critical issues

## Changelog

### Version 1.0.0
- Initial configuration system
- Feature flags for all major components
- Security configuration options
- Environment variable support
- Configuration validation

