# Phase 3: Importers, Wrappers, Synthesis Implementation Guide

## Overview

Phase 3 converts discovered external APIs into working CCOS capabilities through a tiered approach:

1. **Importers** (Preferred): Parse API specs (OpenAPI, GraphQL) into type-safe capabilities
2. **Wrappers** (Second): Generic HTTP/JSON wrappers for APIs without specs
3. **Synthesis** (Last Resort): LLM-generated minimal capabilities with guardrails
4. **Web Search** (Fallback): Find API specs online when discovery fails

### Core Principle: All Effectful Calls via `(call ...)`

**CRITICAL**: In RTFS, ALL HTTP/network operations MUST go through host-coordinated `(call ...)` forms. Never hardcode network calls in generated capabilities.

```lisp
;; ❌ WRONG - Direct network call in implementation
:implementation (http-get "https://api.github.com/repos")

;; ✅ CORRECT - All effects through (call ...)
:implementation (call :http.get {:url "https://api.github.com/repos"})
```

---

## Phase 3a: Auth Management Framework

### Auth Types Supported

```rust
pub enum AuthType {
    Bearer,      // Authorization: Bearer {token}
    ApiKey,      // X-API-Key: {key} or query param
    Basic,       // Authorization: Basic {base64}
    OAuth2,      // Bearer-style with OAuth2 flow
    Custom,      // Custom header schemes
}
```

### Auth Injection Pattern

**Module**: `auth_injector.rs`

Never store credentials in capability code. Instead:

1. Store auth tokens in environment variables
2. Inject at runtime via `(call :ccos.auth.inject ...)`
3. Mark capability with `:auth` effect
4. Audit all auth injections

### Generated Capability with Auth

```lisp
(capability "github.repos.list.v1"
  :description "List repositories for authenticated user"
  :parameters {
    :per_page :number
    :page :number
    :sort :string
    :auth_token :string  ; User provides token (extracted from env)
  }
  :effects [:network :auth]  ; Mark auth requirement
  :implementation (do
    ;; Inject auth token through host call
    (let auth (call :ccos.auth.inject 
      {:provider "github"
       :type :bearer 
       :token auth_token}))
    
    ;; Make HTTP request with injected auth
    (let response (call :http.get 
      {:url "https://api.github.com/user/repos"
       :headers {:Authorization auth}
       :query {:per_page per_page :page page :sort sort}}))
    
    ;; Parse response
    (call :json.parse response)))
```

### Auth Configuration Loading

```rust
// Load from environment with priority order
pub fn retrieve_from_env(&self, provider: &str) -> RuntimeResult<String> {
    // Tries in order:
    // 1. GITHUB_TOKEN
    // 2. GITHUB_API_KEY  
    // 3. CCOS_AUTH_GITHUB
}
```

### Extracting Auth from OpenAPI Specs

OpenAPI specs include `securitySchemes`:

```yaml
components:
  securitySchemes:
    bearerAuth:
      type: http
      scheme: bearer
    apiKeyAuth:
      type: apiKey
      name: X-API-Key
      in: header
security:
  - bearerAuth: []
  - apiKeyAuth: []
```

The importer automatically:
- Detects security requirements
- Creates `:auth` effect
- Adds `auth_token` parameter
- Marks metadata: `{auth_required: true, auth_providers: [...]}`

---

## Phase 3b: OpenAPI/GraphQL Importer

### Module: `openapi_importer.rs`

**Responsibility**: Parse OpenAPI 3.x specs and generate CCOS capabilities with correct RTFS types.

### Type Mapping: JSON Schema → RTFS Keywords

```rust
// CRITICAL: Must use RTFS keyword types, NOT string literals

{
  "type": "string"    → :string   (not "string")
  "type": "number"    → :number   (not "number")
  "type": "integer"   → :number   (not "integer")
  "type": "boolean"   → :boolean  (not "boolean")
  "type": "array"     → :list     (not "array")
  "type": "object"    → :map      (not "object")
}
```

### Processing Flow

```
OpenAPI Spec (JSON)
        ↓
[Load from URL or file]
        ↓
[Extract operations from /paths]
        ↓
[For each operation]:
  - Parse parameters with correct types
  - Extract request/response schemas
  - Detect security requirements
  - Generate auth_token parameter if needed
        ↓
[Generate capability]:
  - ID: openapi.{api_name}.{method}.{operation_id}
  - Parameters: {:param_name :type ...}
  - Effects: [:network :auth] (if auth required)
  - Metadata: {auth_required, auth_providers, ...}
        ↓
[Register in marketplace]
```

### Example: GitHub API Operation

**Input OpenAPI excerpt**:
```yaml
/repos/{owner}/{repo}:
  get:
    operationId: getRepo
    parameters:
      - name: owner
        in: path
        required: true
        schema: {type: string}
    security:
      - bearerAuth: []
    responses:
      '200': {description: Success}
```

**Generated Capability**:
```lisp
(capability "github.repos.get.getRepo.v1"
  :description "Get a repository"
  :parameters {
    :owner :string
    :auth_token :string
  }
  :effects [:network :auth]
  :implementation (do
    (let auth (call :ccos.auth.inject {:provider "github" :type :bearer :token auth_token}))
    (let response (call :http.get 
      {:url (str "https://api.github.com/repos/" owner "/" repo)
       :headers {:Authorization auth}}))
    (call :json.parse response)))
```

---

## Phase 3c: HTTP/JSON Generic Wrapper

### Module: `http_wrapper.rs`

**Responsibility**: Wrap arbitrary HTTP endpoints without specs.

### Features

1. **Auth Detection**: 
   - Attempt request → 401 → Infer Bearer/API-Key
   
2. **Parameter Inference**:
   - From URL pattern: `/api/{id}` → requires `id` param
   - From query string examples
   
3. **Request/Response Mapping**:
   - JSON request body transformation
   - Response unwrapping (extract from `{data: {...}}`)

### Example Generated Wrapper

```lisp
(capability "custom.api.call.v1"
  :description "Generic HTTP API wrapper"
  :parameters {
    :endpoint_url :string
    :method :string           ; :get, :post, etc.
    :path :string
    :query_params :map
    :auth_type :string        ; :bearer, :api_key, :basic
    :auth_value :string       ; auth token
  }
  :effects [:network :auth]
  :implementation (do
    (let auth_header (call :ccos.auth.format 
      {:type auth_type :value auth_value}))
    (let response (call :http.request 
      {:url (str endpoint_url "/" path)
       :method method
       :query query_params
       :headers {:Authorization auth_header}}))
    (call :json.parse response)))
```

---

## Phase 3d: MCP Proxy Adapter

### Module: `mcp_proxy_adapter.rs`

**Responsibility**: Expose discovered MCP tools as CCOS capabilities.

### Features

- Uses `MCPDiscoveryProvider` to fetch tools
- Wraps tool inputs/outputs in RTFS
- Forwards MCP auth tokens
- Maintains effect markers

---

## Phase 3e: LLM Synthesis (Guardrailed)

### Module: `capability_synthesizer.rs` (to be implemented)

**Responsibility**: Generate minimal capabilities as last resort with strong guardrails.

### Synthesis Prompt

```
Generate a CCOS capability for calling {capability_name}.

CRITICAL SAFETY RULES:
1. Use RTFS keyword types: :string, :number, :currency (NOT "string", "number")
2. NEVER hardcode credentials or API keys
3. NEVER make direct HTTP calls
4. ALL network operations MUST use (call :http.* ...)
5. Auth tokens MUST use (call :ccos.auth.inject ...)
6. Function signature: (defn impl [... :string] :map)
7. Return format: {:status :success :result ...} or {:status :error :message ...}

Input parameters schema: {json schema}
Expected output shape: {expected result}

Generate a safe, minimal capability following RTFS semantics.
```

### Generated Capability Quality Gate

Before registering synthesized capabilities:

1. **Type Checking**: Verify all parameters use `:keyword` types
2. **Call Analysis**: Ensure no direct HTTP/network outside `(call ...)`
3. **Auth Check**: No hardcoded credentials or tokens in code
4. **Effect Marking**: Must declare all effects (`:network`, `:auth`, etc.)

### Marking Synthesized Capabilities

```lisp
:metadata {
  :source :synthesized
  :status :experimental
  :guardrailed :true
  :needs_review :true
}
```

---

## Phase 3f: Web Search Discovery

### Module: `web_search_discovery.rs`

**Responsibility**: Find API specs online as fallback when MCP Registry doesn't have results.

### Search Strategy

```
Query patterns:
  1. "{capability_name} OpenAPI spec site:github.com OR site:openapis.org"
  2. "{capability_name} GraphQL schema site:github.com"
  3. "{capability_name} API documentation"
  4. "{capability_name} REST API docs"
```

### Result Scoring

```rust
Score components:
  - Official repos (+0.3):    github.com/official-account/*
  - OpenAPI registry (+0.25): openapis.org
  - API indicators (+0.15):   /api, -api
  - File types (+0.2):        .yaml, .json, .openapi
  
Penalties:
  - StackOverflow (-0.1):  community Q&A
  - Medium posts (-0.05):  blog posts
```

### Example Output

```
🔍 WEB SEARCH: Searching for 'stripe' API specs...
📄 Found API candidates:
  1. https://github.com/stripe/openapi ⭐⭐⭐
     Type: openapi_spec
     Official Stripe OpenAPI 3.0 specification
  
  2. https://openapis.org/stripe-api ⭐⭐
     Type: openapi_spec
     Community maintained Stripe OpenAPI spec
  
  3. https://stripe.com/docs/api/authentication ⭐⭐
     Type: api_docs
     Stripe API documentation
```

### Integration with CLI

```bash
# Capability not found in MCP Registry → try web search
$ ccos resolve-deps --capability-id stripe
  ✗ Not found in MCP Registry
  🔍 Searching online for 'stripe' API specs...
  📄 Found 3 candidates (see above)
  
  Next steps:
  1. ccos import-openapi https://github.com/stripe/openapi
  2. ccos resolve-deps --capability-id stripe --retry
```

---

## Integration: Complete Resolution Pipeline

### Full Discovery→Import→Register Flow

```
1. Dependency Detection
   ├─ Extract: (call :stripe.charges.create ...)
   ├─ Audit: capability_missing event
   └─ Queue: stripe.charges.create

2. Phase 2 Discovery
   ├─ Query MCP Registry
   ├─ Query local manifests
   └─ On no match → Phase 2.5

3. Phase 2.5 Web Search
   ├─ Search online for OpenAPI specs
   ├─ Return top 5 candidates by relevance
   └─ User/CLI selects candidate

4. Phase 3 Import
   ├─ Load selected OpenAPI spec
   ├─ Extract auth requirements
   ├─ Convert to RTFS keyword types
   ├─ Generate auth injection code
   └─ Create CapabilityManifest

5. Phase 5 Validation
   ├─ Preflight: sample inputs
   ├─ Auth attestation: check token source
   ├─ Governance: check effects/permissions
   └─ Audit: register capability

6. Execution
   ├─ (call :stripe.charges.create {...auth_token...})
   ├─ Auth inject: retrieve token from env
   ├─ HTTP call: authorization header injected
   └─ Result: success with audit trail
```

### Audit Trail Example

```json
{
  "event_type": "capability_dependency_detected",
  "capability_id": "stripe.charges.create",
  "source": "synthesis"
}
↓
{
  "event_type": "web_search_discovery",
  "query": "stripe OpenAPI",
  "candidate_selected": "https://github.com/stripe/openapi",
  "relevance_score": 0.95
}
↓
{
  "event_type": "openapi_import",
  "capability_id": "stripe.charges.create",
  "auth_type": "bearer",
  "auth_required": true
}
↓
{
  "event_type": "capability_registered",
  "capability_id": "openapi.stripe.post.createCharge",
  "version": "1.0.0"
}
↓
{
  "event_type": "auth_injection",
  "capability_id": "openapi.stripe.post.createCharge",
  "auth_type": "bearer",
  "success": true
}
```

---

## Security Considerations

### 1. **Credentials Never in Code**
- ❌ `(let token "sk_live_12345...")` → FORBIDDEN
- ✅ `(call :ccos.auth.inject {:provider :stripe ...})` → REQUIRED

### 2. **Auth Token Sources** (in priority order)
- Environment variables (primary)
- Vault (Phase 5)
- Parameter (user-provided, marked `:secret`)
- Never hardcoded

### 3. **Effects Declaration**
- Capabilities must declare `:auth` effect if they use auth
- Governance gate: requires attestation for `:auth` effects
- Audit logs all auth injections

### 4. **Synthesized Capability Guardrails**
- Static analysis passes before registration
- No direct network calls allowed
- Experimental flag + human review recommended
- Feature flag gating

---

## Testing Strategy

### Unit Tests (per module)

```rust
#[test]
fn test_openapi_to_rtfs_types() { ... }

#[test]
fn test_auth_injection_code_generation() { ... }

#[tokio::test]
async fn test_web_search_discovery() { ... }
```

### Integration Tests

```rust
#[tokio::test]
async fn test_github_api_import_full_flow() {
  // Load OpenAPI spec → Extract ops → Generate caps
  // Verify auth_token parameter added
  // Verify :auth effect marked
  // Register and verify in marketplace
}
```

### End-to-End Demo

```bash
$ ccos resolve-deps --capability-id github.repos.list --verbose
  🔍 Not found in marketplace
  🔍 Querying MCP Registry... (no match)
  🔍 Searching online...
  📄 Found: https://github.com/github/rest-api-description
  
  ✅ Importing OpenAPI spec...
  📋 Extracted 100+ operations
  🔐 Auth required: bearer token
  
  ✅ Generating capabilities...
  📝 github.repos.list → :string, :string, :string (3 params)
  📝 github.repos.create → :string, :map (2 params)
  
  ✅ Registering in marketplace...
  CAPABILITY_AUDIT: {"event": "capability_registered", ...}
  
  ✅ Ready to use!
  $ (call :github.repos.list {:token my_token ...})
```

---

## Acceptance Criteria

- ✅ OpenAPI importer generates capabilities with correct RTFS keyword types
- ✅ All network calls wrapped in `(call :http.* ...)`
- ✅ Auth tokens never hardcoded; always injected via `(call :ccos.auth.inject ...)`
- ✅ Auth effects marked and audited
- ✅ HTTP generic wrapper supports common auth schemes
- ✅ Web search returns ranked candidates (top 5)
- ✅ Synthesized capabilities pass static analysis gates
- ✅ Integration tests verify full discovery→import→register flow
- ✅ CLI tool: `ccos resolve-deps --capability-id X` works end-to-end

---

## Implementation Modules Created

| Module | Status | Responsibility |
|--------|--------|-----------------|
| `auth_injector.rs` | ✅ Done | Auth management, token retrieval, injection code gen |
| `openapi_importer.rs` | ✅ Done | OpenAPI → CCOS capability conversion |
| `graphql_importer.rs` | 🚧 Stub | GraphQL schema → capabilities (future) |
| `http_wrapper.rs` | 🚧 Stub | Generic HTTP wrapping (future) |
| `mcp_proxy_adapter.rs` | 🚧 Stub | MCP tool exposure (future) |
| `capability_synthesizer.rs` | 🚧 Stub | LLM synthesis with guardrails (future) |
| `web_search_discovery.rs` | ✅ Done | Online API spec search fallback |

---

## Next Steps

1. **Phase 3 Completion**:
   - Implement GraphQL importer
   - Implement HTTP generic wrapper
   - Complete LLM synthesis guardrails

2. **Phase 4: Stub Scaffolding**
   - Generate explicit stub capabilities
   - Preserve shape with placeholder data
   - Mark as `:stubbed` in metadata

3. **Phase 5: Validation & Governance**
   - Preflight harness (sample inputs/outputs)
   - Attestation requirements
   - Stricter agent capability policies


