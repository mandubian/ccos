# MCP Synthesis Guide: Creating MCP Capabilities

## Overview

This guide explains how to **create** MCP capabilities for CCOS by introspecting MCP servers and generating RTFS capability files.

## Quick Start

### Generate GitHub MCP Capabilities

```rust
use rtfs_compiler::ccos::synthesis::CapabilitySynthesizer;

// Create synthesizer
let mut synthesizer = CapabilitySynthesizer::new(false); // false = real mode

// Introspect GitHub MCP server
let auth_headers = vec![
    ("Authorization".to_string(), format!("Bearer {}", github_pat)),
    ("Content-Type".to_string(), "application/json".to_string()),
];

let results = synthesizer.synthesize_from_mcp_introspection_with_auth(
    "github",
    "https://api.githubcopilot.com/mcp/",
    &auth_headers,
    "/path/to/output/capabilities"
)?;

println!("Generated {} MCP capabilities", results.len());
```

### Output Structure

Generated capabilities are organized hierarchically:

```
capabilities/
└── mcp/
    └── github/
        ├── list_issues.rtfs
        ├── create_issue.rtfs
        ├── get_me.rtfs
        └── ... (46 tools total)
```

## The Synthesis Process

### Step 1: MCP Server Introspection

The synthesizer:
1. Initializes MCP session (with auth if needed)
2. Calls `tools/list` endpoint
3. Parses tool schemas (JSON Schema → RTFS TypeExpr)
4. Terminates session

**MCP Protocol**:
```json
POST https://api.githubcopilot.com/mcp/
{
  "jsonrpc": "2.0",
  "method": "initialize",
  "params": {
    "protocolVersion": "2024-11-05",
    "clientInfo": {"name": "ccos-rtfs", "version": "0.1.0"}
  }
}

POST https://api.githubcopilot.com/mcp/
Mcp-Session-Id: <session-id>
{
  "jsonrpc": "2.0",
  "method": "tools/list",
  "params": {}
}
```

### Step 2: Schema Conversion

JSON Schema from MCP → RTFS TypeExpr:

**Input** (JSON Schema):
```json
{
  "type": "object",
  "properties": {
    "owner": {"type": "string"},
    "repo": {"type": "string"},
    "state": {"type": "string", "enum": ["OPEN", "CLOSED"]}
  },
  "required": ["owner", "repo"]
}
```

**Output** (RTFS TypeExpr):
```rtfs
{
  :owner :string
  :repo :string
  :state [:optional :string]
}
```

### Step 3: RTFS Implementation Generation

For each MCP tool, generate an RTFS function:

```rtfs
(fn [input]
  ;; Runtime validates input against input_schema
  (let [default_url "https://api.githubcopilot.com/mcp/"
        env_url (call "ccos.system.get-env" "MCP_SERVER_URL")
        mcp_url (if env_url env_url default_url)
        auth_token (or (get input :auth-token)
                       (call "ccos.system.get-env" "MCP_AUTH_TOKEN"))
        mcp_request {:jsonrpc "2.0"
                     :id "mcp_call"
                     :method "tools/call"
                     :params {:name "list_issues"
                              :arguments input}}
        headers (if auth_token
                  {:content-type "application/json"
                   :authorization (str "Bearer " auth_token)}
                  {:content-type "application/json"})]
    (let [response (call "ccos.network.http-fetch"
                        :method "POST"
                        :url mcp_url
                        :headers headers
                        :body (call "ccos.data.serialize-json" mcp_request))]
      (if (get response :body)
        (let [response_json (call "ccos.data.parse-json" (get response :body))
              result (get response_json :result)]
          result)
        {:error "No response from MCP server" :url mcp_url}))))
```

### Step 4: Capability File Generation

Complete `.rtfs` file with metadata:

```rtfs
;; MCP Capability: list_issues
;; Generated from MCP tool introspection
;; MCP Server: github (https://api.githubcopilot.com/mcp/)

(capability "mcp.github.list_issues"
  :name "list_issues"
  :version "1.0.0"
  :description "List issues in a GitHub repository..."
  :provider "MCP"
  :permissions ["network" "api_access"]
  :effects ["external_api_call" "data_retrieval"]
  :metadata {
    :mcp {
      :server_url "https://api.githubcopilot.com/mcp/"
      :server_name "github"
      :tool_name "list_issues"
      :protocol_version "2024-11-05"
      :requires_session "auto"
      :auth_env_var "MCP_AUTH_TOKEN"
      :server_url_override_env "MCP_SERVER_URL"
    }
    :discovery {
      :method "mcp_introspection"
      :source_url "https://api.githubcopilot.com/mcp/"
      :created_at "2025-10-23T21:35:43Z"
      :capability_type "mcp_tool"
    }
  }
  :input-schema {
    :owner :string
    :repo :string
    :state [:optional :string]
    ...
  }
  :output-schema {
    :issues [:vector :map]
    :totalCount :int
    ...
  }
  :implementation
    (fn [input] ...))
```

## Code Organization

### Synthesis Components

**Location**: `rtfs_compiler/src/ccos/synthesis/`

**Files**:
- `mcp_introspector.rs` - MCP server introspection
- `mcp_session.rs` - Session lifecycle management
- `schema_serializer.rs` - TypeExpr → RTFS string conversion
- `capability_synthesizer.rs` - Orchestrates synthesis

### Key Functions

**Introspect MCP Server**:
```rust
pub fn synthesize_from_mcp_introspection_with_auth(
    &mut self,
    server_name: &str,
    server_url: &str,
    auth_headers: &[(String, String)],
    output_dir: &str,
) -> RuntimeResult<Vec<SynthesisResult>>
```

**Save Capability to File**:
```rust
pub fn save_capability_to_rtfs(
    capability: &CapabilityManifest,
    implementation_code: &str,
    metadata: &HashMap<String, String>,
    output_dir: &str,
) -> RuntimeResult<String>
```

## Directory Structure

Generated capabilities follow this pattern:

```
<output_dir>/
└── mcp/
    └── <namespace>/
        └── <tool_name>.rtfs
```

**Example**:
- Input: server_name="github", tool_name="list_issues"
- Output: `capabilities/mcp/github/list_issues.rtfs`

This structure:
- ✅ Organizes capabilities by provider and namespace
- ✅ Prevents naming conflicts
- ✅ Makes discovery easier
- ✅ Scales to unlimited MCP servers

## Session Management in Generated Capabilities

### Metadata-Driven

All generated MCP capabilities include:

```rtfs
:metadata {
  :mcp {
    :requires_session "auto"  ; Triggers automatic session management
    ...
  }
}
```

### Runtime Behavior

When called, the runtime:
1. Detects `:requires_session "auto"` from metadata
2. Delegates to `SessionPoolManager`
3. Manager routes to `MCPSessionHandler`
4. Handler initializes/reuses session
5. Executes with proper headers
6. Returns result

**No manual session handling in generated code!**

### Implementation Pattern

Generated RTFS implementations are **generic** and delegate to runtime:

```rtfs
:implementation
  (fn [input]
    ;; Get server URL (metadata default or env override)
    (let [mcp_url (if env_url env_url default_url)
          ;; Get auth token (from input or env)
          auth_token (or (get input :auth-token)
                         (call "ccos.system.get-env" "MCP_AUTH_TOKEN"))
          ;; Build MCP request (generic pattern)
          mcp_request {:jsonrpc "2.0" :method "tools/call" ...}
          ;; Let runtime handle HTTP call (with session management!)
          response (call "ccos.network.http-fetch" ...)]
      ...))
```

The `call "ccos.network.http-fetch"` triggers session management automatically!

## Extending to New MCP Servers

### Example: Slack MCP Server

```rust
// Introspect Slack MCP server
let slack_pat = std::env::var("SLACK_TOKEN")?;
let auth_headers = vec![
    ("Authorization".to_string(), format!("Bearer {}", slack_pat)),
];

synthesizer.synthesize_from_mcp_introspection_with_auth(
    "slack",
    "https://slack-mcp-server.example.com/mcp/",
    &auth_headers,
    "capabilities"
)?;
```

**Output**:
```
capabilities/mcp/slack/
  ├── send_message.rtfs
  ├── list_channels.rtfs
  └── ...
```

**Usage**:
```rtfs
;; Set auth token
;; export MCP_AUTH_TOKEN="xoxb-slack-token"

(call "mcp.slack.send_message" {
  :channel "general"
  :text "Hello from CCOS!"
})
```

Session management works automatically - same infrastructure!

## Schema Mapping Reference

### JSON Schema → RTFS TypeExpr

| JSON Schema | RTFS TypeExpr |
|-------------|---------------|
| `{"type": "string"}` | `:string` |
| `{"type": "integer"}` | `:int` |
| `{"type": "number"}` | `:float` |
| `{"type": "boolean"}` | `:bool` |
| `{"type": "array", "items": {...}}` | `[:vector <type>]` |
| `{"type": "object", "properties": {...}}` | `{:key1 <type1> :key2 <type2>}` |
| Not in `required` array | `[:optional <type>]` |

### Example Conversion

**JSON Schema**:
```json
{
  "type": "object",
  "properties": {
    "name": {"type": "string"},
    "age": {"type": "integer"},
    "tags": {"type": "array", "items": {"type": "string"}},
    "active": {"type": "boolean"}
  },
  "required": ["name", "age"]
}
```

**RTFS TypeExpr**:
```rtfs
{
  :name :string
  :age :int
  :tags [:optional [:vector :string]]
  :active [:optional :bool]
}
```

## Testing Generated Capabilities

### Unit Test Example

```rust
#[test]
fn test_mcp_capability_metadata() {
    let env = CCOSBuilder::new().build().unwrap();
    env.execute_file("capabilities/mcp/github/get_me.rtfs").unwrap();
    
    let marketplace = env.marketplace();
    let caps = futures::executor::block_on(marketplace.list_capabilities());
    
    let cap = caps.iter().find(|c| c.id == "mcp.github.get_me").unwrap();
    
    assert_eq!(cap.metadata.get("mcp_requires_session"), Some(&"auto".to_string()));
    assert_eq!(cap.metadata.get("mcp_server_url"), 
               Some(&"https://api.githubcopilot.com/mcp/".to_string()));
}
```

### Integration Test

See `rtfs_compiler/src/bin/test_end_to_end_session.rs` for a complete example.

## Related Documentation

- **Using MCP capabilities**: `mcp-runtime-guide.md` (this guide's companion)
- **Session management architecture**: `SESSION_MANAGEMENT_COMPLETE.md`
- **Overall success summary**: `PHASE_2_FINAL_SUCCESS.md`

---

**Status**: Production Ready ✅  
**GitHub MCP**: 46 capabilities generated and tested  
**Extensible**: Works for any MCP server  

