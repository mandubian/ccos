# MCP Capability Synthesis Strategy: RTFS-First

## 🎯 Strategic Decision: RTFS-First for ALL Synthesized Capabilities

### Your Questions Answered

**Q1: "Now we can create new capability from OpenAPI, right?"**  
**A:** ✅ YES! Via `api_introspector.rs` - it works perfectly!

**Q2: "From MCP endpoint, should it be basic RTFS or use MCPCapability?"**  
**A:** 🎯 **RTFS-First** (with MCP metadata) - here's why:

## 🔄 Two Approaches to MCP

### Approach 1: MCPCapability (Current `mcp_discovery.rs`)

**How it works:**
```rust
provider: ProviderType::MCP(MCPCapability {
    server_url: "http://localhost:3000",
    tool_name: "get_weather",
    timeout_ms: 30000,
})
```

**Execution:**
```
User calls capability
    ↓
Marketplace → MCPExecutor
    ↓
HTTP POST to MCP server (JSON-RPC)
    {
      "method": "tools/call",
      "params": {"name": "get_weather", "arguments": {...}}
    }
    ↓
MCP server executes tool
    ↓
Returns result
```

**Pros:**
- ✅ Direct connection to real MCP servers
- ✅ No RTFS overhead
- ✅ Streaming support possible

**Cons:**
- ❌ **No schemas!** (`input_schema: None, output_schema: None`)
- ❌ **No RTFS control** - can't modify behavior
- ❌ **Black box** - can't inspect or debug
- ❌ **No composability** - can't combine with other RTFS
- ❌ **Requires MCP server running** - deployment dependency

### Approach 2: RTFS-First with MCP Backend (RECOMMENDED)

**How it works:**
```rust
provider: ProviderType::Local(LocalCapability {
    handler: rtfs_implementation_wrapper
})
input_schema: Some(TypeExpr::Map {...})  // ✅ Proper schemas!
output_schema: Some(TypeExpr::Map {...})
```

**capability.rtfs:**
```clojure
(capability "mcp.github.create_issue"
  :name "Create GitHub Issue"
  :version "1.0.0"
  :input-schema {
    :title :string
    :body :string
    :labels [:vector :string] ;; optional
  }
  :output-schema {
    :number :int
    :url :string
    :state :string
  }
  :permissions [:network.http]
  :effects [:network_request :mcp_call]
  :metadata {
    :mcp_server_url "http://localhost:3000"
    :mcp_tool_name "create_issue"
  }
  :implementation
    (fn [input]
      ;; Runtime validates input against input_schema
      ;; Prepare MCP JSON-RPC request
      (let [mcp_request {
              :jsonrpc "2.0"
              :id "1"
              :method "tools/call"
              :params {
                :name "create_issue"
                :arguments input
              }
            }
            mcp_url (get (get-metadata :mcp_server_url "http://localhost:3000"))
            ;; Make HTTP POST to MCP server
            response (call "ccos.network.http-fetch"
                          :method "POST"
                          :url mcp_url
                          :headers {:content-type "application/json"}
                          :body (call "ccos.data.serialize-json" mcp_request))
            result_json (call "ccos.data.parse-json" (get response :body))
            result (get result_json :result)]
        ;; Runtime validates result against output_schema
        result)))
```

**Pros:**
- ✅ **Full schemas** - input/output properly typed
- ✅ **Runtime validation** - automatic via schemas  
- ✅ **RTFS composability** - can modify, extend, debug
- ✅ **Transparent** - can see exactly what's happening
- ✅ **Portable** - can work without MCP server (mock responses)
- ✅ **Testable** - easy to unit test RTFS functions
- ✅ **Consistent** - same pattern as OpenAPI capabilities

**Cons:**
- ⚠️ RTFS parsing overhead (minimal)
- ⚠️ Slightly more complex code generation

## 🎯 Recommended: Unified RTFS-First Architecture

### For OpenAPI/REST APIs ✅ (Done!)

```clojure
(capability "openweather_api.get_current_weather"
  :input-schema { :q :string :lat :float ... }
  :output-schema { :coord {...} :main {...} }
  :implementation
    (fn [input]
      (let [url (build-url input)
            api_key (call "ccos.system.get-env" "API_KEY")]
        (call "ccos.network.http-fetch" :url url ...))))
```

### For MCP Tools 🎯 (TODO: Create MCP introspector!)

```clojure
(capability "mcp.github.create_issue"
  :input-schema { :title :string :body :string :labels [:vector :string] }
  :output-schema { :number :int :url :string :state :string }
  :metadata {
    :mcp_server_url "http://localhost:3000"
    :mcp_tool_name "create_issue"
  }
  :implementation
    (fn [input]
      (let [mcp_request {:jsonrpc "2.0"
                         :method "tools/call"
                         :params {:name "create_issue"
                                  :arguments input}}
            response (call "ccos.network.http-fetch"
                          :method "POST"
                          :url "http://localhost:3000"
                          :body (call "ccos.data.serialize-json" mcp_request))]
        (call "ccos.data.parse-json" (get response :body)))))
```

## 📋 Action Plan: MCP Introspection

### Phase 1: Create MCP Introspector (Similar to API Introspector)

**File:** `rtfs_compiler/src/ccos/synthesis/mcp_introspector.rs`

**What it should do:**
1. Connect to MCP server
2. Call `tools/list` to discover tools
3. Extract schemas from MCP tool input schemas (JSON Schema)
4. Convert to RTFS TypeExpr (reuse `json_schema_to_rtfs_type`)
5. Generate RTFS capabilities with MCP call implementation
6. Save to `capability.rtfs` files

**Key difference from current `mcp_discovery.rs`:**
- ✅ Generate **RTFS implementations** (not just metadata)
- ✅ Convert schemas to **TypeExpr** (not leave as None)
- ✅ Create **one capability per MCP tool**
- ✅ Save to `.rtfs` files for persistence

### Phase 2: Implementation Template

```rust
// In mcp_introspector.rs
pub fn create_mcp_capability_from_tool(
    &self,
    mcp_server_url: &str,
    tool: &MCPTool,
) -> RuntimeResult<CapabilityManifest> {
    let capability_id = format!("mcp.{}", tool.name);
    
    // Convert MCP JSON Schema to RTFS TypeExpr
    let input_schema = tool.input_schema.as_ref()
        .map(|schema| self.json_schema_to_rtfs_type(schema))
        .transpose()?;
    
    let output_schema = tool.output_schema.as_ref()
        .map(|schema| self.json_schema_to_rtfs_type(schema))
        .transpose()?;
    
    // Generate RTFS implementation that makes MCP JSON-RPC call
    let implementation = format!(
        r#"(fn [input]
  ;; MCP tool: {}
  ;; Runtime validates input against input_schema
  (let [mcp_request {{:jsonrpc "2.0"
                      :id "1"
                      :method "tools/call"
                      :params {{:name "{}"
                               :arguments input}}}}
        response (call "ccos.network.http-fetch"
                      :method "POST"
                      :url "{}"
                      :headers {{:content-type "application/json"}}
                      :body (call "ccos.data.serialize-json" mcp_request))
        result_json (call "ccos.data.parse-json" (get response :body))]
    ;; Runtime validates result against output_schema
    (get result_json :result)))"#,
        tool.description.as_deref().unwrap_or(&tool.name),
        tool.name,
        mcp_server_url
    );
    
    let mut metadata = HashMap::new();
    metadata.insert("mcp_server_url".to_string(), mcp_server_url.to_string());
    metadata.insert("mcp_tool_name".to_string(), tool.name.clone());
    metadata.insert("mcp_protocol".to_string(), "json-rpc-2.0".to_string());
    
    Ok(CapabilityManifest {
        id: capability_id,
        name: tool.name.clone(),
        description: tool.description.clone().unwrap_or_default(),
        provider: ProviderType::Local(LocalCapability {
            handler: Arc::new(|_| Ok(Value::String("MCP RTFS capability".to_string())))
        }),
        version: "1.0.0".to_string(),
        input_schema,      // ✅ Proper schemas!
        output_schema,     // ✅ Proper schemas!
        attestation: None,
        provenance: Some(CapabilityProvenance { ... }),
        permissions: vec!["network.http".to_string()],
        effects: vec!["network_request".to_string(), "mcp_call".to_string()],
        metadata,
        agent_metadata: None,
    })
}
```

## 🏗️ Unified Architecture

### All Synthesized Capabilities Follow Same Pattern:

```
API/MCP Introspection
        ↓
Discover endpoints/tools
        ↓
Extract schemas (JSON Schema)
        ↓
Convert to RTFS TypeExpr
        ↓
Generate RTFS implementation
        ↓
Create CapabilityManifest with:
  - input_schema: Some(TypeExpr)
  - output_schema: Some(TypeExpr)
  - provider: ProviderType::Local (RTFS wrapper)
  - metadata: Protocol-specific details
        ↓
Serialize to capability.rtfs file
        ↓
Runtime validates inputs/outputs
```

## 📊 Comparison

| Feature | MCPCapability (Old) | RTFS-First MCP (New) |
|---------|---------------------|----------------------|
| **Schemas** | ❌ None | ✅ Full TypeExpr |
| **Validation** | ❌ None | ✅ Runtime validates |
| **Composability** | ❌ Black box | ✅ Can modify RTFS |
| **Debugging** | ❌ Opaque | ✅ See exact MCP call |
| **Testing** | ❌ Needs MCP server | ✅ Can mock easily |
| **Performance** | ✅ Direct call | ⚠️ RTFS overhead |
| **Consistency** | ❌ Different from OpenAPI | ✅ Same as OpenAPI |

## ✅ Recommendation

### DO THIS:

1. **Create `mcp_introspector.rs`** (similar to `api_introspector.rs`)
   - Discover MCP tools via `tools/list`
   - Convert JSON Schema → TypeExpr
   - Generate RTFS implementations that make MCP JSON-RPC calls
   - Save to `capability.rtfs` files

2. **Deprecate direct `ProviderType::MCP` usage** for synthesized capabilities
   - Keep MCPExecutor for legacy support
   - New MCP tools use RTFS-first approach

3. **Delete or move `providers/weather_mcp.rs` and `providers/github_mcp.rs`**
   - They're examples of the old approach
   - Replace with introspected versions

### Benefits:

✅ **Consistency:** OpenAPI and MCP capabilities work the same way  
✅ **Schemas:** All capabilities have proper input/output types  
✅ **Runtime Control:** Validation, auth, rate limiting unified  
✅ **Transparency:** RTFS code is inspectable and modifiable  
✅ **Testability:** Easy to unit test and mock  

## 🚀 Next Steps

1. ✅ **OpenAPI introspection** - DONE!
2. 🔲 **Create MCP introspector** - Use same pattern as `api_introspector.rs`
3. 🔲 **Enhance `HttpCapability`** - Add endpoint/method/params (optional, for performance)
4. 🔲 **Deprecate manual providers** - Move to examples/

## 💡 Key Insight

**RTFS-first is superior for synthesized capabilities because:**

1. **Schemas are essential** - OpenAPI/MCP provide them, we must preserve them
2. **Transparency matters** - users should see what capabilities do
3. **Composability wins** - RTFS capabilities can be combined and modified
4. **Validation is critical** - runtime enforcement prevents errors
5. **Consistency is valuable** - one pattern for all synthesized capabilities

**MCPCapability/HttpCapability are for:**
- Runtime-native integrations (when you write Rust code directly)
- Performance-critical paths (skip RTFS parsing)
- Legacy support (existing capabilities)

**RTFS synthesis is for:**
- Auto-generated capabilities (OpenAPI, MCP, GraphQL)
- User-modifiable capabilities
- Schema-validated capabilities
- Transparent, debuggable capabilities

This aligns perfectly with CCOS/RTFS philosophy! 🎯

