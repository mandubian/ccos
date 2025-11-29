# MCP Discovery & Capability Resolution Master Guide

## 1. Overview

The **CCOS MCP Discovery Engine** bridges the gap between static RTFS capabilities and dynamic **Model Context Protocol (MCP)** servers. Instead of manually writing wrappers for every external tool, CCOS proactively "discovers" tools, introspects their schemas, and generates native RTFS capability definitions on the fly.

### Core Philosophy
1.  **Introspection over Definition**: We rely on the MCP server to tell us what it can do (`tools/list`).
2.  **RTFS-First**: All discovered tools are compiled into static `.rtfs` files (`(capability ...)`), preserving the purity of the CCOS runtime.
3.  **Smart Inference**: We don't just read schemas; we *probe* tools with safe inputs to infer missing output schemas.
4.  **Session Management**: We handle the stateful MCP capability lifecycle (Initialize → Session → Call → Terminate).

---

## 2. The Discovery Architecture

The discovery process follows a strict pipeline to ensure generated capabilities are type-safe and compliant.

### 2.1. The Pipeline

1.  **Handshake & Initialization**:
    *   Connects to MCP server (SSE/Stdio).
    *   Negotiates protocol version (default `2024-11-05`).
    *   Establishes a Session ID.

2.  **Tool Introspection (`tools/list`)**:
    *   Retrieves the list of available tools.
    *   Parses `inputSchema` (JSON Schema).

3.  **Schema Mapping**:
    *   Converts JSON Schema types to RTFS `TypeExpr`.
    *   *Mapping*: `object` → `:map`, `array` → `:vector`, `string` → `:string`.
    *   *Nuance*: Handles `nullable`, `required` fields, and `enum` constraints.

4.  **Output Schema Inference (The "Probing" Step)**:
    *   MCP `tools/list` rarely provides `outputSchema`.
    *   **Heuristic**: The engine generates "safe" test inputs based on the input schema (e.g., using `octocat/hello-world` for repo args).
    *   **Arbiter Extraction**: If a `--hint` is provided (e.g., "list issues for mandubian"), the Arbiter extracts specific arguments.
    *   **Live Probe**: The engine calls the tool *once* during discovery.
    *   **Unwrapping**: It detects wrapped responses (e.g., `{ content: [{ text: "JSON..." }] }`) and deserializes them to find the *actual* data structure.

5.  **RTFS Generation**:
    *   Generates a valid `.rtfs` source file.
    *   Embeds the `input-schema` and inferred `output-schema`.
    *   Adds sample outputs as comments for documentation.

---

## 3. Usage Guide

### 3.1. Single Tool Discovery (CLI)

Use the `single_mcp_discovery` example to fetch and compile a specific tool or set of tools from a server.

```bash
cargo run --example single_mcp_discovery -- \
  --server-name <SERVER_NAME> \
  --hint "<NATURAL_LANGUAGE_INTENT>" \
  --config config/agent_config.toml \
  --profile <LLM_PROFILE>
```

**Example: Discovering GitHub Issues**
```bash
cargo run --example single_mcp_discovery -- \
  --server-name github \
  --hint "list issues of owner mandubian and repository ccos" \
  --config config/agent_config.toml \
  --profile openrouter_free:balanced_gfl
```

**Flags:**
*   `--server-name`: The friendly name (mapped in `overrides.json`) or URL.
*   `--hint`: A natural language description. Used to (1) select the best tool from the server and (2) extract parameters for the inference probe.
*   `--output-dir`: Where to save the `.rtfs` file (default: `capabilities/discovered`).

### 3.2. Automatic Resolution (Runtime)

(Planned Feature) When the CCOS runtime encounters a `(call :github.unknown_tool ...)` instruction:
1.  The **Missing Capability Resolver** catches the error.
2.  It queries the **MCP Registry** for a server matching the namespace.
3.  It performs on-the-fly discovery and compilation.
4.  It resumes execution.

---

## 4. Configuration

The system is configured via Environment Variables or the `agent_config.toml`.

### 4.1. Core Feature Flags

| Variable | Default | Description |
|----------|---------|-------------|
| `CCOS_MISSING_CAPABILITY_ENABLED` | `false` | Master switch for the resolution system. |
| `CCOS_AUTO_RESOLUTION_ENABLED` | `false` | If true, attempts to compile missing tools at runtime. |
| `CCOS_MCP_REGISTRY_ENABLED` | `true` | Allow querying the public MCP registry. |

### 4.2. Security & Limits

| Variable | Default | Description |
|----------|---------|-------------|
| `CCOS_HUMAN_APPROVAL_REQUIRED` | `true` | If true, stops before saving/executing new capabilities. |
| `CCOS_ALLOWED_DOMAINS` | `...` | Allowlist for MCP server URLs (e.g., `api.github.com`). |
| `CCOS_REQUIRE_HTTPS` | `true` | Enforce TLS for all discovery connections. |

### 4.3. Authentication

The discovery engine looks for standard tokens in the environment:
*   `MCP_AUTH_TOKEN`: Generic bearer token.
*   `GITHUB_MCP_TOKEN` / `GITHUB_PAT`: Specific provider tokens.

---

## 5. Migration & Adoption Strategy

If you are moving from manual RTFS definitions to MCP Discovery:

### Phase 1: Hybrid Mode (Recommended)
*   Keep your existing manually written capabilities.
*   Use `single_mcp_discovery` CLI to generate *new* capabilities as needed.
*   Review generated `.rtfs` files before committing them.

### Phase 2: Runtime Detection
1.  Enable `CCOS_MISSING_CAPABILITY_ENABLED=true`.
2.  Enable `CCOS_RUNTIME_DETECTION=true`.
3.  Run your plans. If a capability is missing, the system will *log* the discovery potential but fail safely.

### Phase 3: Auto-Resolution
1.  Enable `CCOS_AUTO_RESOLUTION_ENABLED=true`.
2.  The system will download, compile, and cache capabilities on demand.
3.  **Warning**: Ensure `CCOS_ALLOWED_DOMAINS` is strictly configured for production.

---

## 6. Directory Structure

Discovered capabilities are organized hierarchically to match RTFS namespacing:

```
capabilities/
  └── discovered/
      └── mcp/
          ├── github/
          │   ├── list_issues.rtfs
          │   ├── create_issue.rtfs
          │   └── search_code.rtfs
          └── slack/
              └── post_message.rtfs
```

## 7. Troubleshooting

**"Schema inference failed"**
*   **Cause**: The tool call returned an error, or the output was empty.
*   **Fix**: Improve your `--hint` to provide valid parameters (e.g., existing repo names).

**"Tool not found"**
*   **Cause**: The server doesn't export a tool matching your hint.
*   **Fix**: Check the server's available tools using a generic hint or introspection.

**"Authentication failed"**
*   **Cause**: Missing env vars.
*   **Fix**: Ensure `MCP_AUTH_TOKEN` or provider-specific tokens are set in the shell.






