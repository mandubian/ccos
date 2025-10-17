# Missing Capability Resolution Quickstart Guide

## Overview

This quickstart guide will help you get the missing capability resolution system up and running in your CCOS deployment. The system automatically detects missing capabilities and attempts to resolve them through various discovery and synthesis methods.

## Prerequisites

- CCOS deployment with RTFS compiler
- Access to external APIs (optional, for discovery)
- Basic understanding of CCOS capabilities and RTFS

## Quick Start (5 minutes)

### 1. Enable the System

Set the basic environment variables:

```bash
# Enable missing capability resolution
export CCOS_MISSING_CAPABILITY_ENABLED=true

# Enable runtime detection
export CCOS_AUTO_RESOLUTION_ENABLED=true

# Enable MCP Registry (safe to enable)
export CCOS_MCP_REGISTRY_ENABLED=true
```

### 2. Test the System

Run the simple example to verify everything works:

```bash
cd rtfs_compiler
cargo run --example simple_missing_capability_example
```

You should see output like:
```
🔍 DISCOVERY: Querying MCP Registry for 'github'
✅ DISCOVERY: Found matching MCP server 'modelcontextprotocol/github' for capability 'github'
📊 RESOLUTION: Successfully resolved 1 missing capabilities
```

### 3. Use the CLI Tool

Check the resolution queue status:

```bash
cargo run --bin resolve-deps -- show-queue
```

Resolve a specific capability:

```bash
cargo run --bin resolve-deps -- resolve --capability-id github
```

## Basic Configuration

### Development Setup

For development, use the permissive configuration:

```rust
use rtfs_compiler::ccos::synthesis::feature_flags::*;

let config = MissingCapabilityConfig {
    feature_flags: MissingCapabilityFeatureFlags::development(),
    ..Default::default()
};
```

### Production Setup

For production, use conservative defaults:

```rust
let config = MissingCapabilityConfig::default();
```

## Common Use Cases

### 1. Resolve Missing GitHub Capabilities

When your RTFS code calls `(call :github.create-issue ...)` but the capability doesn't exist:

```bash
# The system will automatically detect this and attempt resolution
cargo run --bin resolve-deps -- resolve --capability-id github.create-issue
```

### 2. Resolve Missing API Capabilities

For external API capabilities:

```bash
# Enable HTTP wrapper for generic API calls
export CCOS_HTTP_WRAPPER_ENABLED=true

# Resolve API capability
cargo run --bin resolve-deps -- resolve --capability-id api.weather.get-forecast
```

### 3. Synthesize Custom Capabilities

For LLM-generated capabilities:

```bash
# Enable LLM synthesis (requires human approval)
export CCOS_LLM_SYNTHESIS_ENABLED=true

# Resolve custom capability
cargo run --bin resolve-deps -- resolve --capability-id custom.data-processor
```

## Monitoring and Observability

### Check System Status

```bash
# View resolution queue
cargo run --bin resolve-deps -- show-queue

# View system statistics
cargo run --bin resolve-deps -- stats

# Monitor resolution activity
cargo run --bin resolve-deps -- monitor --interval 5 --duration 60
```

### View Audit Logs

The system logs all resolution activities:

```
CAPABILITY_AUDIT: {"timestamp": "2025-01-16T13:28:37.068654598+00:00", "capability_id": "github.create-issue", "event_type": "capability_registered"}
```

## Security Configuration

### Basic Security

```bash
# Require HTTPS for external calls
export CCOS_REQUIRE_HTTPS=true

# Allow only trusted domains
export CCOS_ALLOWED_DOMAINS="registry.modelcontextprotocol.io,api.github.com"

# Require human approval for high-risk resolutions
export CCOS_HUMAN_APPROVAL_REQUIRED=true
```

### Advanced Security

```bash
# Block specific domains
export CCOS_BLOCKED_DOMAINS="malicious-site.com,phishing-site.com"

# Limit request/response sizes
export CCOS_MAX_REQUEST_SIZE_BYTES=10485760  # 10MB
export CCOS_MAX_RESPONSE_SIZE_BYTES=52428800 # 50MB

# Set execution time limits
export CCOS_MAX_EXECUTION_TIME_SECONDS=300   # 5 minutes
```

## Troubleshooting

### Common Issues

#### 1. Resolution Failures

**Problem**: Capabilities fail to resolve
**Solution**: Check network connectivity and API keys

```bash
# Check resolution queue for failed attempts
cargo run --bin resolve-deps -- show-queue

# Retry failed resolutions
cargo run --bin resolve-deps -- resolve --capability-id <failed-capability>
```

#### 2. Timeout Issues

**Problem**: Resolutions timeout
**Solution**: Increase timeout values

```bash
export CCOS_RESOLUTION_TIMEOUT_SECONDS=60
export CCOS_MCP_REGISTRY_TIMEOUT_SECONDS=20
```

#### 3. Security Violations

**Problem**: External API calls blocked
**Solution**: Check domain allowlists

```bash
# Add domain to allowlist
export CCOS_ALLOWED_DOMAINS="registry.modelcontextprotocol.io,api.github.com,your-api.com"
```

### Debug Mode

Enable debug logging for troubleshooting:

```bash
export RUST_LOG=debug
export CCOS_DEBUG_MISSING_CAPABILITY=true
```

### Health Checks

```bash
# Validate configuration
cargo run --bin resolve-deps -- validate

# Check system health
cargo run --bin resolve-deps -- info
```

## Advanced Features

### 1. Continuous Resolution

Enable automatic background resolution:

```bash
export CCOS_CONTINUOUS_RESOLUTION=true
export CCOS_AUTO_RESUME_ENABLED=true
```

### 2. Web Search Discovery

Enable web search for API discovery:

```bash
export CCOS_WEB_SEARCH_ENABLED=true
export CCOS_WEB_SEARCH_MAX_RESULTS=10
```

### 3. Custom Resolution Methods

Implement custom resolution methods by extending the system:

```rust
use rtfs_compiler::ccos::synthesis::missing_capability_resolver::MissingCapabilityResolver;

// Add custom resolution logic
impl MissingCapabilityResolver {
    pub fn add_custom_resolver(&mut self, resolver: Box<dyn CustomResolver>) {
        // Implementation
    }
}
```

## Integration Examples

### 1. With Existing CCOS Deployment

```rust
use rtfs_compiler::ccos::synthesis::feature_flags::*;

// Initialize with configuration
let config = MissingCapabilityConfig::from_env();
let resolver = MissingCapabilityResolver::new(marketplace, checkpoint_archive, config);

// Integrate with orchestrator
orchestrator.set_missing_capability_resolver(resolver);
```

### 2. With Custom Capability Marketplace

```rust
// Create custom marketplace
let marketplace = Arc::new(CapabilityMarketplace::new());

// Add existing capabilities
marketplace.register_local_capability(
    "existing.capability",
    "Existing Capability",
    "Description",
    handler
);

// Initialize resolver
let resolver = MissingCapabilityResolver::new(marketplace, checkpoint_archive, config);
```

### 3. With Checkpoint System

```rust
// Enable auto-resume for paused executions
let config = MissingCapabilityConfig {
    feature_flags: MissingCapabilityFeatureFlags {
        auto_resume_enabled: true,
        ..Default::default()
    },
    ..Default::default()
};

// Resolver will automatically resume paused executions when capabilities are resolved
```

## Performance Tuning

### 1. Concurrent Resolution

Adjust concurrent resolution limits:

```bash
# Increase for high-throughput systems
export CCOS_MAX_CONCURRENT_RESOLUTIONS=10

# Decrease for resource-constrained systems
export CCOS_MAX_CONCURRENT_RESOLUTIONS=2
```

### 2. Caching

Enable caching for better performance:

```bash
# Cache MCP Registry data
export CCOS_MCP_REGISTRY_CACHE_TTL_SECONDS=3600

# Cache resolution results
export CCOS_RESOLUTION_CACHE_TTL_SECONDS=1800
```

### 3. Resource Limits

Set appropriate resource limits:

```bash
# Limit pending capabilities
export CCOS_MAX_PENDING_CAPABILITIES=100

# Limit resolution attempts
export CCOS_MAX_RESOLUTION_ATTEMPTS=3
```

## Best Practices

### 1. Configuration Management

- Use environment variables for different environments
- Keep configuration files in version control
- Document all configuration changes
- Use secure secret management for API keys

### 2. Monitoring

- Set up alerts for failed resolutions
- Monitor resolution success rates
- Track performance metrics
- Review audit logs regularly

### 3. Security

- Use HTTPS for all external calls
- Implement domain allowlists
- Require human approval for high-risk resolutions
- Regular security audits

### 4. Testing

- Test resolution pipeline regularly
- Use mock data for testing
- Validate configuration changes
- Test failure scenarios

## Next Steps

1. **Read the Full Documentation**: See [020-missing-capability-resolution-complete-guide.md](020-missing-capability-resolution-complete-guide.md)
2. **Configure for Production**: See [021-missing-capability-resolution-configuration.md](021-missing-capability-resolution-configuration.md)
3. **Explore Advanced Features**: Check the API documentation
4. **Join the Community**: Ask questions and share experiences

## Support

- **Documentation**: Check the complete guide and configuration docs
- **Examples**: Run the provided examples
- **CLI Help**: Use `cargo run --bin resolve-deps -- --help`
- **Community**: Join the discussion forum
- **Issues**: Report bugs and request features

## Changelog

### Version 1.0.0
- Initial quickstart guide
- Basic configuration examples
- Common use cases
- Troubleshooting guide
- Performance tuning tips

