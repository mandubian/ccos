# MCP Introspection → CCOS Capability Demo

This guide explains how to run the `mcp_introspection_demo` example that:
- Introspects an MCP server via JSON‑RPC `tools/list`
- Registers each discovered tool as a CCOS capability
- Executes a selected tool through the Capability Marketplace

The source code for this example lives in `ccos/examples/archived/mcp_introspection_demo.rs`.

## Prerequisites
- Rust + Cargo installed
- A running MCP server exposing a JSON‑RPC HTTP endpoint

## Running the Demo
From the project root:

- List available tools from a server:
  - `cargo run --example mcp_introspection_demo -- --server-url http://localhost:3000 --list`
- Execute a tool (e.g., `echo`) with JSON args:
  - `cargo run --example mcp_introspection_demo -- --server-url http://localhost:3000 --tool echo --args '{"text":"hello"}'`

## Implementation Details
1. **Discovery**: The demo sends a `tools/list` request to the specified server.
2. **Registration**: Discovered tools are dynamically registered in the local `CapabilityRegistry`.
3. **Execution**: The `CapabilityMarketplace` resolves the tool by its ID (e.g., `mcp.demo.echo`) and executes it using the configured MCP provider.

## Troubleshooting
- **Capability Denied**: Check the agent's isolation policy. The demo defaults to allowing `mcp.*` capabilities.
- **Connection Error**: Ensure your MCP server is reachable at the provided URL and supports POST requests.

---
*Note: This doc was migrated from the legacy docs/examples directory.*
