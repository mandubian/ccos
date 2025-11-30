# Complete Guide: Missing Capability Resolution System

## Overview

This document provides a comprehensive guide to the missing capability resolution system implemented in CCOS. The system enables automatic detection, discovery, sourcing, and registration of capabilities that are referenced but not yet implemented, creating a self-healing and self-extending capability ecosystem.

## Architecture

The missing capability resolution system consists of several interconnected components:

### Core Components

1. **Dependency Extractor** (`dependency_extractor.rs`)
   - Extracts `(call :capability.id ...)` patterns from RTFS code
   - Identifies missing capabilities during synthesis
   - Creates audit events for missing dependencies

2. **Missing Capability Resolver** (`missing_capability_resolver.rs`)
   - Manages resolution queue and discovery pipeline
   - Orchestrates fan-out discovery across multiple sources
   - Handles auto-resume for checkpointed executions

3. **Runtime Trap** (`capability_registry.rs`)
   - Intercepts calls to non-existent capabilities
   - Enqueues missing capabilities for resolution
   - Triggers deferred execution model

4. **Discovery Pipeline** (Multiple sources)
   - **MCP Registry Client** (`mcp_registry_client.rs`) - Official MCP server discovery
   - **OpenAPI Importer** (`openapi_importer.rs`) - OpenAPI spec to capability conversion
   - **GraphQL Importer** (`graphql_importer.rs`) - GraphQL schema to capability conversion
   - **HTTP Wrapper** (`http_wrapper.rs`) - Generic HTTP API wrapping
   - **Web Search Discovery** (`web_search_discovery.rs`) - Online API discovery fallback

5. **Validation & Governance** (Phases 5-6)
   - **Validation Harness** (`validation_harness.rs`) - Capability validation
   - **Governance Policies** (`governance_policies.rs`) - Compliance checking
   - **Static Analyzers** (`static_analyzers.rs`) - Code analysis
   - **Registration Flow** (`registration_flow.rs`) - Registration orchestration

6. **Continuous Resolution Loop** (`continuous_resolution.rs`)
   - Automated resolution processing
   - Human-in-the-loop approvals for high-risk items
   - Backoff and retry mechanisms

## Complete Process Flow

### Phase 1: Detection and Surfacing

When a capability is synthesized or executed:

1. **Dependency Extraction**: RTFS code is scanned for `(call :capability.id ...)` patterns
2. **Missing Detection**: Dependencies are checked against the marketplace
3. **Audit Events**: Missing capabilities are logged with context
4. **Queue Management**: Missing capabilities are added to the resolution queue

```rust
// Example: Extracting dependencies from RTFS code
let dependencies = extract_dependencies(rtfs_code);
let missing = find_missing_capabilities(&dependencies, &marketplace);
if !missing.is_empty() {
    enqueue_missing_capabilities(&missing);
}
```

### Phase 2: Discovery Pipeline (Fan-out)

The system searches for missing capabilities across multiple sources:

1. **Exact Match**: Check if capability exists with exact ID
2. **Partial Match**: Find capabilities with similar names/domains
3. **MCP Registry**: Query official MCP Registry for servers
4. **Local Manifests**: Scan local capability manifests
5. **Network Catalogs**: Search external capability catalogs
6. **Web Search**: Fallback to online API discovery

```rust
// Example: Fan-out discovery process
let discovery_results = vec![
    exact_match_search(&capability_id),
    partial_match_search(&capability_id),
    mcp_registry_search(&capability_id),
    local_manifest_search(&capability_id),
    network_catalog_search(&capability_id),
    web_search_discovery(&capability_id),
];
```

### Phase 3: Importers and Wrappers

For each discovered API or service:

1. **Auth Management**: Handle authentication requirements
2. **OpenAPI Import**: Convert OpenAPI specs to CCOS capabilities
3. **GraphQL Import**: Convert GraphQL schemas to CCOS capabilities
4. **HTTP Wrapper**: Create generic HTTP API wrappers
5. **MCP Proxy**: Expose MCP tools as CCOS capabilities
6. **LLM Synthesis**: Generate capabilities with guardrails

```rust
// Example: OpenAPI to capability conversion
let capability_manifest = openapi_importer
    .operation_to_capability(&operation, &auth_config)
    .await?;
```

### Phase 4: Deferred Execution (No Stubs)

Instead of creating "stub" capabilities:

1. **Checkpoint Creation**: Execution is paused and checkpointed
2. **Missing Capability Tracking**: Checkpoint records missing capabilities
3. **Auto-Resume**: When capabilities are resolved, execution resumes automatically
4. **CLI Resume Hook**: Manual resume capability via CLI

```rust
// Example: Checkpoint with missing capabilities
let checkpoint = CheckpointRecord {
    plan_id: plan_id.clone(),
    intent_id: intent_id.clone(),
    missing_capabilities: vec!["github".to_string()],
    auto_resume_enabled: true,
    // ... other fields
};
```

### Phase 5: Validation and Governance

Before registration, capabilities undergo validation:

1. **Static Analysis**: Analyze RTFS code for issues
2. **Governance Policies**: Check compliance with enterprise policies
3. **Security Validation**: Verify security requirements
4. **Quality Assessment**: Evaluate code quality and performance

```rust
// Example: Validation pipeline
let validation_result = validation_harness
    .validate_capability(&manifest, &rtfs_code)
    .await?;

let governance_result = governance_policies
    .apply_policies(&manifest, &rtfs_code)
    .await?;
```

### Phase 6: Registration and Versioning

Capabilities are registered with proper versioning:

1. **Attestation Creation**: Generate capability attestations
2. **Provenance Tracking**: Record capability provenance
3. **Marketplace Registration**: Register in capability marketplace
4. **Version Management**: Handle capability versioning
5. **Dependency Wiring**: Wire up capability dependencies

```rust
// Example: Registration flow
let registration_result = registration_flow
    .register_capability(&manifest, &rtfs_code)
    .await?;
```

### Phase 7: Continuous Resolution Loop

The system continuously processes pending resolutions:

1. **Automated Processing**: Process low-risk resolutions automatically
2. **Human Approval**: Require approval for high-risk items
3. **Backoff Strategy**: Implement exponential backoff for failures
4. **Monitoring**: Track resolution statistics and metrics

```rust
// Example: Continuous resolution processing
let resolution_loop = ContinuousResolutionLoop::new(
    marketplace.clone(),
    validation_harness.clone(),
    governance_policies.clone(),
);

resolution_loop.process_pending_resolutions().await?;
```

## Key Features

### 1. MCP Registry Integration

The system integrates with the official MCP Registry for server discovery:

```rust
// MCP Registry client usage
let client = McpRegistryClient::new();
let servers = client.search_servers("github").await?;
for server in servers {
    let capability = client.convert_to_capability_manifest(&server, "github")?;
    marketplace.register_capability_manifest(capability).await?;
}
```

### 2. Auth Management Framework

Centralized authentication handling for external APIs:

```rust
// Auth configuration
let auth_config = AuthConfig {
    auth_type: AuthType::Bearer,
    token_env_var: "GITHUB_TOKEN".to_string(),
    header_name: "Authorization".to_string(),
};

// RTFS auth injection
let auth_code = auth_injector.generate_auth_injection_code(&auth_config);
// Generates: (call :ccos.auth.inject "Bearer" (get-env "GITHUB_TOKEN"))
```

### 3. OpenAPI to Capability Conversion

Automatic conversion of OpenAPI specs to CCOS capabilities:

```rust
// OpenAPI operation to capability
let operation = OpenAPIOperation {
    path: "/repos/{owner}/{repo}".to_string(),
    method: "GET".to_string(),
    parameters: vec![
        OpenAPIParameter {
            name: "owner".to_string(),
            param_type: ":string".to_string(),
            required: true,
        },
        OpenAPIParameter {
            name: "repo".to_string(),
            param_type: ":string".to_string(),
            required: true,
        },
    ],
};

let capability = openapi_importer
    .operation_to_capability(&operation, &auth_config)
    .await?;
```

### 4. Deferred Execution Model

No more "stub" capabilities - instead, execution is deferred until capabilities are resolved:

```rust
// When missing capability is encountered
if !marketplace.has_capability(&capability_id).await {
    // Enqueue for resolution
    registry.enqueue_missing_capability(capability_id, args, context)?;
    
    // Create checkpoint with missing capabilities
    let checkpoint = orchestrator.checkpoint_plan(
        &plan_id, 
        &intent_id, 
        &evaluator, 
        Some(vec![capability_id])
    )?;
    
    // Return RequiresHost to pause execution
    return Ok(ExecutionOutcome::RequiresHost(HostCall {
        capability_id: "checkpoint".to_string(),
        args: vec![Value::String(checkpoint.0)],
    }));
}
```

### 5. Auto-Resume Functionality

Automatic resumption when capabilities are resolved:

```rust
// Auto-resume trigger
async fn trigger_auto_resume_for_capability(&self, capability_id: &str) -> RuntimeResult<()> {
    let checkpoints = self.checkpoint_archive
        .find_checkpoints_waiting_for_capability(capability_id);
    
    for checkpoint in checkpoints {
        if self.can_resume_checkpoint(&checkpoint) {
            // Emit audit event for auto-resume readiness
            self.emit_auto_resume_ready_audit(&checkpoint).await?;
        }
    }
    Ok(())
}
```

## CLI Tools

### resolve-deps Command

The system provides a CLI tool for managing capability resolution:

```bash
# Resolve a specific capability
cargo run --bin resolve-deps -- resolve --capability-id github

# List pending resolutions
cargo run --bin resolve-deps -- list-pending

# Resume a checkpoint
cargo run --bin resolve-deps -- resume --checkpoint-id checkpoint_123 --capability-id github
```

### Command Output Example

```
🚀 Bootstrapped marketplace with 5 test capabilities
🔍 Resolving dependencies for capability: github
📋 Adding missing capability to resolution queue...
⚙️ Processing resolution queue...
🔍 DISCOVERY: Querying MCP Registry for 'github'
✅ DISCOVERY: Found matching MCP server 'modelcontextprotocol/github' for capability 'github'
CAPABILITY_AUDIT: {"event_type": "capability_registered", "capability_id": "mcp.ai.smithery.Hint-Services-obsidian-github-mcp.github", "timestamp": "2025-10-17T09:37:26.830674856+00:00"}
✅ Dependency resolution completed!
```

## File Structure

The implementation is spread across multiple files:

```
rtfs_compiler/src/ccos/synthesis/
├── dependency_extractor.rs          # Phase 1: Dependency extraction
├── missing_capability_resolver.rs   # Phase 2: Discovery orchestration
├── mcp_registry_client.rs          # MCP Registry integration
├── auth_injector.rs                # Phase 3a: Auth management
├── openapi_importer.rs             # Phase 3b: OpenAPI import
├── graphql_importer.rs             # Phase 3c: GraphQL import
├── http_wrapper.rs                 # Phase 3d: HTTP wrapper
├── mcp_proxy_adapter.rs            # Phase 3e: MCP proxy
├── capability_synthesizer.rs       # Phase 3f: LLM synthesis
├── web_search_discovery.rs         # Phase 3g: Web search
├── validation_harness.rs           # Phase 5: Validation
├── governance_policies.rs          # Phase 5: Governance
├── static_analyzers.rs             # Phase 5: Static analysis
├── registration_flow.rs            # Phase 6: Registration
└── continuous_resolution.rs        # Phase 7: Continuous loop

rtfs_compiler/src/bin/
└── resolve_deps.rs                 # CLI tool

rtfs_compiler/src/ccos/
├── capabilities/registry.rs        # Runtime trap
├── checkpoint_archive.rs           # Checkpoint management
└── orchestrator.rs                 # Orchestration integration
```

## Security Considerations

1. **Auth Token Management**: Secure handling of authentication tokens
2. **Capability Validation**: Strict validation of generated capabilities
3. **Governance Policies**: Enterprise compliance checking
4. **Static Analysis**: Security-focused code analysis
5. **Human Approval**: Human-in-the-loop for high-risk items

## Testing Strategy

The system includes comprehensive testing:

1. **Unit Tests**: Individual component testing
2. **Integration Tests**: End-to-end resolution testing
3. **Mock Services**: Mock implementations for external services
4. **CLI Testing**: Command-line tool testing

## Monitoring and Observability

1. **Audit Events**: Comprehensive audit logging
2. **Resolution Statistics**: Track resolution success rates
3. **Performance Metrics**: Monitor resolution performance
4. **Error Tracking**: Track and analyze resolution failures

## Future Enhancements

1. **Machine Learning**: ML-based capability matching
2. **Capability Composition**: Automatic capability composition
3. **Performance Optimization**: Optimize resolution performance
4. **Extended Discovery**: Additional discovery sources
5. **Advanced Validation**: More sophisticated validation rules

## Conclusion

The missing capability resolution system provides a robust, self-healing capability ecosystem that automatically discovers, validates, and integrates new capabilities. By eliminating the need for manual capability creation and providing comprehensive validation and governance, the system enables rapid capability development while maintaining security and compliance standards.

The system's modular architecture allows for easy extension and customization, while the comprehensive testing and monitoring ensure reliability and observability. The deferred execution model eliminates the need for "stub" capabilities, providing a more robust and reliable execution environment.

