### Missing Capability Resolution Plan

#### Context
Synthesized capabilities may call external capabilities that are not yet present in the system. We need a robust, governed pipeline to detect, discover, source, validate, register, and wire those dependencies so learned workflows execute reliably while preserving graceful degradation in demos.

#### Goals
- Detect missing `(call :cap.id …)` dependencies immediately after synthesis and at runtime.
- Resolve dependencies via discovery, importers, wrappers, or synthesis with strong guardrails.
- Keep demos resilient with explicit, well-labeled stubs when providers are unavailable.
- Enforce governance: attestation, permissions/effects policies, and higher scrutiny for `:kind :agent`.
- Provide developer tooling (CLI + diagnostics) and observability (audits, metrics).

#### Non‑Goals
- Building production connectors to every external service (we’ll enable importers/wrappers first).
- Changing RTFS syntax/semantics (we align with existing grammar and marketplace contracts).

#### Deliverables
- Dependency linter and audit events.
- Automated resolution pipeline (discovery → import → wrap → synthesize → stub fallback).
- Preflight validation harness and governance gates.
- Marketplace registration/versioning flow with end‑to‑end tests.
- CLI command and minimal dashboard surfaces for developers.

### Phase 1 — Detection and Surfacing
- Implement post‑synthesis dependency extraction (scan RTFS for `(call :cap.id …)`).
- Compare against `CapabilityMarketplace` to build `missing_caps` set.
- Attach `:needs_capabilities` to artifact/plan metadata; emit `capability_deps_missing` audit event.
- Add runtime trap: on invocation error `:capability_not_found`, emit same event and enqueue resolution job.

Acceptance criteria
- Missing dependencies appear in logs/audits and artifact metadata.
- CI test: synthesized artifact with a fake dependency emits expected metadata/event.

### Phase 2 — Discovery Pipeline
- Query marketplace with `CapabilityQuery` (prefer `:kind :primitive` for leaf calls).
- Fan‑out discovery: local manifests, MCP servers, network catalogs (if configured).
- On matches: register with `register_capability_manifest`; recheck dependencies.

Acceptance criteria
- CLI "resolve deps" finds and registers an existing provider when available.

### Phase 2.5 — Web Search Discovery (NEW)
- Fallback search mechanism for capabilities not found via MCP Registry
- Query web search engines (Google, DuckDuckGo, etc.) for OpenAPI specs, SDK docs, GraphQL endpoints
- Extract API metadata and endpoints from search results
- Use heuristic matching to identify promising candidates

Acceptance criteria
- CLI can search online for missing capability documentation
- Returns candidate API endpoints with relevance scoring

### Phase 3 — Importers, Wrappers, Synthesis
- Importers (preferred):
  - OpenAPI/GraphQL → generate provider capability with strict input/output schema.
  - Known SDK shims → wrap into `ProviderType::Local`.
- Wrappers (second best):
  - HTTP/JSON generic provider with request/response mapping.
  - MCP proxy adapter (expose remote tool as capability).
- Synthesis (last resort):
  - LLM‑generated minimal `:primitive` capability using keyword types (e.g., `:string`, `:number`).
  - Guardrails: deterministic prompt, schema echoing, no side effects unless policy allows.

#### Phase 3a — OpenAPI/GraphQL Importer
**Goal**: Convert external API specs into working CCOS capabilities

**Auth Management**:
- Extract auth requirements from OpenAPI spec (`securitySchemes`, `security`)
- Store credentials securely (env vars, vault references, encrypted config)
- Generate auth injection code: `(call :ccos.auth.inject {:provider "github" :type :bearer})`
- Mark capabilities with auth effects: `[:auth :network]`
- Runtime attestation: require auth token before capability invocation

**Implementation**:
1. `openapi_importer.rs` — Parse OpenAPI 3.x specs
2. `graphql_importer.rs` — Parse GraphQL introspection schemas
3. Generate RTFS with proper parameter types (`:string`, `:number`, not `"string"`)
4. Extract endpoints, parameters, response schemas
5. Create `CapabilityManifest` with auth metadata

**Example Generated Capability**:
```lisp
(capability "github.repos.list.v1"
  :description "List repositories for authenticated user"
  :parameters {
    :per_page :number
    :page :number
    :sort :string
    :auth_token :string  ; marked as secret/sensitive
  }
  :effects [:network :auth]
  :implementation (do
    (let auth (call :ccos.auth.inject {:provider "github" :type :bearer :token auth_token}))
    (let response (call :http.get 
      {:url "https://api.github.com/user/repos"
       :headers {:Authorization (str "Bearer " auth)}
       :query {:per_page per_page :page page :sort sort}}))
    (call :json.parse response)))
```

**Acceptance criteria**:
- OpenAPI spec for GitHub API imports successfully
- Generated capability has correct RTFS types (`:string`, not `"string"`)
- Auth token parameters marked sensitive
- All HTTP calls wrapped in `(call ...)`

#### Phase 3b — HTTP/JSON Generic Wrapper
**Goal**: Wrap unknown HTTP APIs without explicit specs

**Auth Management**:
- Introspect API for auth requirements (401 responses hint at needed auth)
- Support common patterns: Bearer token, API Key (header/query), Basic auth, OAuth2
- Store auth config separately: `auth_config: {type: :bearer, header: "Authorization", prefix: "Bearer "}`

**Implementation**:
1. `http_wrapper.rs` — Generic HTTP request/response mapping
2. Parameter inference from request/response examples
3. Auth pattern detection and injection
4. Request/response transformation layer

**Example Wrapped Capability**:
```lisp
(capability "custom.api.endpoint.v1"
  :description "Generic HTTP API wrapper"
  :parameters {
    :endpoint_url :string
    :method :string  ; GET, POST, etc.
    :path :string
    :query_params :map
    :auth_type :string  ; :bearer, :api_key, :basic
    :auth_value :string  ; marked as secret
  }
  :effects [:network :auth]
  :implementation (do
    (let auth_header (call :ccos.auth.format {:type auth_type :value auth_value}))
    (let response (call :http.request 
      {:url (str endpoint_url "/" path)
       :method method
       :query query_params
       :headers {:Authorization auth_header}}))
    (call :json.parse response)))
```

**Acceptance criteria**:
- Can wrap arbitrary HTTP endpoint
- Detects and handles common auth schemes
- Transforms request/response formats

#### Phase 3c — MCP Proxy Adapter
**Goal**: Expose MCP tools as CCOS capabilities directly

**Implementation**:
- Use `MCPDiscoveryProvider` from `mcp_discovery.rs`
- Wrap MCP tool calls in CCOS capability interface
- Forward auth tokens from MCP server config
- Marshal MCP tool inputs/outputs to RTFS

#### Phase 3d — LLM Synthesis (Guardrailed)
**Goal**: Generate minimal capabilities as last resort

**Auth Handling**:
- LLM prompt explicitly: "Do NOT hardcode credentials. Use `(call :ccos.auth.inject ...)`"
- Generated code must pass static analysis: only `(call ...)` for effects
- Refuse generation if auth required but no safe mechanism available
- Mark synthesized capabilities as `:experimental` and `:guardrailed`

**Example Prompt**:
```
Generate a CCOS capability for calling {capability_name}.

CRITICAL RULES:
1. Use RTFS keyword types: :string, :number, :currency (not "string", "number")
2. NEVER hardcode credentials or API keys
3. ALL HTTP/network calls must use (call :http.* ...)
4. Auth tokens must be injected via (call :ccos.auth.inject ...)
5. Function signature: (defn impl [... :string] :map)
6. Return {:status :success :result ...} or {:status :error :message ...}

Input parameters: {json schema}
Expected output: {expected result shape}

Generate safe, minimal capability that handles auth properly.
```

**Acceptance criteria**:
- Generated code uses `:keyword` types
- All network calls through `(call ...)`
- Auth properly injected, not hardcoded
- Passes static analysis checks

#### Phase 3e — Auth Framework
**Goal**: Centralized auth injection and management

**Modules**:
1. `auth_injector.rs`:
   - `(call :ccos.auth.inject {:provider :github :type :bearer :token token_param})`
   - `(call :ccos.auth.retrieve {:provider :github :from :env|:vault})`
   - Support: Bearer, API Key, Basic, OAuth2, custom headers

2. `auth_config.rs`:
   - Load from env vars: `GITHUB_TOKEN`, `OPENAI_API_KEY`, etc.
   - Vault integration for production (deferred to Phase 5)
   - Secrets never logged or exposed in capability metadata

3. `auth_effects.rs`:
   - Mark capabilities requiring auth: `[:auth :network]`
   - Governance gate: require `auth_approved` flag for agent capabilities using auth
   - Audit: log all auth injection calls

**Acceptance criteria**:
- Auth tokens never hardcoded in generated capabilities
- Tokens retrieved from secure sources only
- All auth marked in effects and audited

#### Phase 3f — Web Search Discovery
**Goal**: Find API specs and docs online when discovery fails

**Implementation**:
1. `web_search_discovery.rs`:
   - Query: `"{capability_name} OpenAPI spec" OR "{capability_name} GraphQL"`
   - Extract URLs from results
   - Heuristic scoring: official repos (github.com, gitlab.com), openapis.org, etc.

2. Integration:
   - Called after MCP Registry returns no results
   - Return top 5 candidates with URLs and relevance score
   - User/CLI can manually inspect and import

**Example Output**:
```
🔍 WEB SEARCH: Searching for "stripe" API specs...
📄 Found candidates:
  1. https://github.com/stripe/openapi (⭐⭐⭐ official repo)
  2. https://openapis.org/stripe-api (⭐⭐ community spec)
  3. https://stripe.com/docs/api/authentication (⭐⭐ docs site)
```

**Acceptance criteria**:
- Can search and return top candidates for unknown capability
- Scores prioritize official repos and known API aggregators
- Results guide manual importer configuration

---

Acceptance criteria
- At least one importer (OpenAPI) and one wrapper (HTTP/JSON) available
- Synthesis path gated behind feature flag
- Auth management integrated throughout
- Web search provides fallback discovery

### Phase 4 — Stub Scaffolding (Graceful Degradation)
- Generate explicit stub capability when resolution is not possible.
- Manifest metadata: `{:status :stubbed :reason :missing_provider}`.
- Return small, clearly labeled placeholder results; preserve shape.

Acceptance criteria
- Demo/workflows keep running with stubs; audit shows stub usage.

### Phase 5 — Validation and Governance
- Preflight harness: sample inputs → validate input/output contract; basic execution smoke test.
- Governance gates:
  - Permissions/effects checks.
  - Attestation/provenance for remote providers.
  - Stricter policies for `:metadata {:kind :agent :planning true|false :stateful …}`.

Acceptance criteria
- Capabilities must pass preflight before being marked active.
- Agent‑class capabilities trigger additional review or require approval flag.

### Phase 6 — Registration, Versioning, Wiring
- Create `CapabilityManifest` with correct keyword parameter types and provider info.
- Register via marketplace and emit `capability_registered` + `capability_validated` audit events.
- Re‑evaluate parent capability integration; run end‑to‑end test.

Acceptance criteria
- End‑to‑end plan executes with resolved providers; all calls succeed (or stubs clearly indicated).

### Phase 7 — Continuous Resolution Loop
- On runtime failures due to missing caps, auto‑trigger the resolution pipeline.
- Backoff and persistence: remember unresolved items; retry on schedule.
- Human‑in‑loop pause for high‑risk (privileged effects or agents).

Acceptance criteria
- Repeatable resolution with safe fallbacks; avoids noisy loops.

### Phase 8 — Tooling and Observability
- CLI: `ccos resolve-deps <capability_id>` (or from artifact file) with `--importers/--wrap/--synthesize/--stub` flags.
- Minimal dashboard (text/JSON): list unresolved, resolved, stubbed capabilities; show provenance.
- Metrics: success/error‑rates and latency for new capabilities.

Acceptance criteria
- One‑command developer workflow to resolve dependencies and verify.

### Phase 9 — Tests and Examples
- Unit tests: parsing, matching, manifest generation.
- Integration tests: marketplace discovery, importer/wrapper registration, stub behavior, governance.
- Example: travel planner calling nonexistent `:travel.flights` → wrapper or stub, then upgrade to real provider.

### Phase 10 — Rollout
- Feature flag for synthesis/stub paths.
- Documentation: quickstart, governance notes, troubleshooting.
- Migration note: how to add new providers via importer/wrapper.

---

#### Risk & Mitigations
- Over‑synthesis of providers → prefer importers/wrappers first; enforce governance.
- Type mismatches → strict schema validation and preflight failures.
- Security regressions → permissions/effects gating, attestation required for remote providers.

#### Acceptance (global)
- A synthesized capability with missing deps can be resolved end‑to‑end via CLI into a working execution, or gracefully stubbed with explicit audit and no crashes.


