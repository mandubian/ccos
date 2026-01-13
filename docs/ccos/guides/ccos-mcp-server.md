# CCOS MCP Server Guide (for Cursor / Claude / any MCP client)

This guide explains how to run **CCOS as an MCP server** (the `ccos-mcp` binary) so that any MCP-capable agent can:

- **Search** available capabilities
- **Execute** capabilities (with governance, approvals, and causal logging)
- **Introspect** external MCP/OpenAPI/doc endpoints and register them safely
- **Build sessions** and export an RTFS plan from an execution trace

## What the server exposes

CCOS runs an MCP server that provides a set of **tools** (MCP “tools/list”) implemented in:
- `ccos/src/bin/ccos-mcp.rs`

The HTTP transport follows MCP Streamable HTTP and exposes:
- `POST /mcp` (JSON-RPC requests)
- `GET /mcp` (SSE stream)
- `DELETE /mcp` (terminate session)

It also serves a lightweight **approval UI**:
- `GET /approvals`

(See `ccos/src/mcp/http_transport.rs` for routes and details.)

## Run the server

### HTTP (recommended)

From the repo root:

```bash
cargo run --bin ccos-mcp
```

Defaults:
- **HTTP** transport
- **`0.0.0.0:3000`**
- MCP endpoint: `http://localhost:3000/mcp`
- Approvals UI: `http://localhost:3000/approvals`

Custom port:

```bash
cargo run --bin ccos-mcp -- --port 8080
```

### stdio (subprocess mode)

```bash
cargo run --bin ccos-mcp -- --transport stdio
```

Use this mode if your agent prefers “command-based” MCP servers instead of HTTP.

## Connect from an agent (generic)

Most MCP clients support either:

- **HTTP server (recommended)**: configure the MCP base URL as `http://localhost:3000/mcp`  
  This runs CCOS as a long-lived process and enables **persistence across client sessions** (sessions, approvals UI, in-memory state while the server stays up).
- **stdio server (subprocess mode)**: configure a command that runs `ccos-mcp` with `--transport stdio`  
  This is **not persistent**: state/sessions do not survive process restarts, and it’s not the right choice if you want long-lived session workflows.

Once connected, the agent should call `tools/list` and you should see CCOS tools such as `ccos_search`, `ccos_execute_capability`, etc.

## Start here (level 0 / bootstrap)

The very first call an agent should make after connecting is:

- **`ccos_get_guidelines`**: returns the CCOS operating guidelines for agents (how to behave around approvals, secrets, governance, and safe execution).

Optionally, for governance context:

- **`ccos_get_constitution`**: returns the current governance rules.

## The tool surface (current implementation)

### Capability discovery & execution

- **`ccos_search`**: search the local capability catalog (IDs/names/descriptions).
- **`ccos_list_capabilities`**: list all registered capabilities in the marketplace.
- **`ccos_inspect_capability`**: inspect a capability and its RTFS schemas.
- **`ccos_execute_capability`**: execute a capability using **JSON inputs** (preferred “happy path”).
- **`ccos_execute_plan`**: execute an RTFS plan string (or dry-run syntax validation).

### “Planning” helpers (lightweight)

- **`ccos_plan`**: decompose a goal into a sequence of sub-intents, detect gaps, and return a next action (e.g., `ccos_suggest_apis`).
- **`ccos_decompose`**: find capabilities that can fulfill a given intent description.
- **`ccos_suggest_apis`**: ask the configured LLM to suggest well-known APIs for a goal (does not auto-approve).

### Sessions (build a plan from execution trace)

- **`ccos_session_start`**: start a session and get a `session_id`.
- **`ccos_session_plan`**: retrieve the accumulated RTFS plan from session steps.
- **`ccos_session_end`**: end the session and save the plan to disk (optional filename).
- **`ccos_consolidate_session`**: turn a saved session trace into a reusable “agent capability” (via planner synthesis).

### Introspection, approvals, and registration

- **`ccos_introspect_remote_api`**: introspect an MCP endpoint, OpenAPI spec, or documentation URL and create an approval request.
- **`ccos_list_approvals`**: list approvals (pending/rejected/expired/approved).
- **`ccos_reapprove`**: re-approve an expired/rejected approval.
- **`ccos_register_server`**: after the user approves, register the server’s tools into the marketplace.

### Secrets / governance helpers

- **`ccos_list_secrets`**: list approved secret names and env mappings (values not revealed).
- **`ccos_check_secrets`**: check for missing secrets and queue approval requests when missing.
- **`ccos_get_constitution`**: fetch governance rules.
- **`ccos_get_guidelines`**: fetch agent guidelines from `docs/agent_guidelines.md`.

### RTFS “teaching” tools

- **`rtfs_get_grammar`**
- **`rtfs_get_samples`**
- **`rtfs_compile`**
- **`rtfs_explain_error`**
- **`rtfs_repair`** (heuristic or LLM-based, depending on parameters / config)

## Recommended workflows

### A) Execute an existing capability (fast path)

1. `ccos_search` (or `ccos_list_capabilities`)
2. `ccos_inspect_capability` (optional: confirm required inputs)
3. `ccos_check_secrets` (if the capability needs secrets)
4. `ccos_execute_capability`

### B) Bring a new MCP/OpenAPI tool into CCOS (governed)

1. `ccos_introspect_remote_api` with:
   - MCP endpoint URL, **or**
   - OpenAPI spec URL, **or**
   - documentation URL, **or**
   - `npx -y ...` command for an MCP server
2. User approves in `http://localhost:3000/approvals`
3. `ccos_register_server` (with the returned `approval_id`)
4. `ccos_search` → `ccos_execute_capability`

### C) Build an RTFS plan from a trace

1. `ccos_session_start`
2. Repeated `ccos_execute_capability` calls with the same `session_id`
3. `ccos_session_plan` to export the RTFS plan so far
4. `ccos_session_end` to finalize/save

## Related docs

- **MCP runtime inside CCOS (client-side use)**: [`mcp-runtime-guide.md`](mcp-runtime-guide.md)
- **MCP discovery tuning (advanced)**: [`mcp-discovery-tuning.md`](mcp-discovery-tuning.md)
- **Trust & user interaction**: [`server-trust-user-interaction.md`](server-trust-user-interaction.md)

