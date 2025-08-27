# RTFS/CCOS Examples

This directory contains example configurations and code demonstrating RTFS 2.0 and CCOS integration patterns.

## Examples

### `agent.config.microvm.rtfs`

A minimal RTFS-native agent configuration demonstrating the MicroVM deployment profile as defined in the [MicroVM Deployment Profile specification](../rtfs-2.0/specs-incoming/19-microvm-deployment-profile.md).

#### Key Features Demonstrated

- **MicroVM Isolation**: Uses `:orchestrator.isolation.mode = :microvm` for strong isolation
- **Ephemeral Filesystem**: Configured with `:orchestrator.isolation.fs {:ephemeral true}`
- **Read-Only Capabilities**: Capability store mounted as read-only for security
- **Proxy Egress Control**: All network traffic routed through a proxy with domain allowlisting
- **VSock Control Plane**: Host communication via virtio-vsock for control plane operations
- **Attestation Support**: Measured boot and rootfs hash verification

#### Usage

This configuration can be used as a template for deploying RTFS agents in MicroVM environments with strict security requirements. The configuration follows the acceptance criteria from the MicroVM deployment profile specification:

- Validates conceptually against the type schema in the spec
- Includes inline comments mapping to specification sections
- Can be consumed by future validation code without structural changes

#### Specification Compliance

The configuration implements all required sections from the MicroVM deployment profile:

- **§2 Image Composition**: Minimal kernel, read-only rootfs, virtio devices
- **§3 Host Setup**: Firecracker VM spec, networking, control plane, attestation
- **§4 RTFS Config Extensions**: Complete microvm configuration block
- **§5 Execution Semantics**: Step-level profile derivation and security policies
- **§7 Egress/DLP Proxy**: Domain allowlisting, TLS pinning, DLP filters

#### Security Properties

- Rootfs and agent.bin hashes match expected (attestation)
- Capabilities directory mounted RO with verified contracts
- All egress flows through proxy with enforced ACLs/DLP
- MicroVM seccomp + minimal kernel reduce attack surface
- Causal Chain anchored periodically for tamper detection

### MCP Introspection Demo

See `mcp_introspection_demo.md` for a step-by-step guide to run the `rtfs_compiler` example that introspects an MCP server, registers discovered tools as CCOS capabilities, and executes one via the Capability Marketplace.

## Related Documentation

- [MicroVM Deployment Profile Specification](../rtfs-2.0/specs-incoming/19-microvm-deployment-profile.md)
- [Agent Configuration with RTFS](../rtfs-2.0/specs-incoming/17-agent-configuration-with-rtfs.md)
- [RTFS-CCOS Integration Guide](../rtfs-2.0/specs/13-rtfs-ccos-integration-guide.md) 