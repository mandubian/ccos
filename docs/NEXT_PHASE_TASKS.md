# Next Phase Tasks: Autonomous Agent Evolution

## Phase D: Self-Correction & Error Recovery (Current Priority)
**Goal**: Enable the agent to recover from runtime failures (e.g., synthesized code errors, API failures) by feeding errors back to the LLM.

### Tasks
1.  [x] **Implement Execution Repair Loop**:
    *   Wrap the final `validate_and_execute_plan` in a loop (max 3 retries).
    *   If execution fails, capture the error message.
    *   Feed the *original plan* + *error message* to the Arbiter.
    *   Prompt: "The following RTFS plan failed with this error. Fix the code."
    *   Update `final_plan_rtfs` with the fixed version and retry.
2.  [x] **Simulate Failures**:
    *   Add a flag or mechanism to inject faults into the mock capabilities (e.g., return unexpected JSON structure) to verify the repair loop works.

## Phase E: Complex Data Flow & State Management (Completed)
**Goal**: Handle complex data dependencies where step N needs data from step N-2, or deep JSON extraction.

### Tasks
1.  [x] **Context Accumulation**:
    *   Modify the generated RTFS plan to pass a `context` array tracking all previous step variables instead of just the immediate previous result.
    *   `step_1` -> `context = [step_1]`
    *   `step_2` -> `context = [step_1, step_2]`
    *   `step_3` can now access both `step_1` and `step_2`.
2.  [x] **Deep Adapter Synthesis**:
    *   Update `synthesize_adapter` to create new `synthesize_adapter_with_context` that:
        - Receives full context array (all previous step variables).
        - Enhanced prompt with examples for deep JSON extraction (e.g., "Get the ID of the first item in the 'items' list").
        - Provides context awareness: tells LLM which variables are available and what they contain.

## Phase F: Real-World Integration (MCP) (Mostly Complete)
**Goal**: Transition from generic mocks to real MCP services.

### Tasks
1.  [x] **Integrate MCPDiscoveryProvider**:
    *   Use `MCPDiscoveryProvider` to connect to real MCP servers.
    *   Added `try_real_mcp_discovery()` method that uses the existing `MCPDiscoveryProvider`.
    *   Implemented in `try_install_from_registry()` with fallback to generic mocks.
2.  [x] **Add MCP Server Configuration Support**:
    *   Created `find_mcp_server_config()` to detect MCP servers from environment variables.
    *   Server URL resolution from `capabilities/mcp/overrides.json` patterns (e.g., `github.*` → GitHub MCP).
    *   Auth token injection via `MCP_AUTH_TOKEN` environment variable.
3.  [x] **Hybrid Resolution Strategy**:
    *   Updated `try_install_from_registry` to:
        1.  Check for known MCP server configurations via overrides.json patterns.
        2.  If found, attempt real MCP connection and discovery.
        3.  On success, register and return the real capability.
        4.  On failure (connection error, tool not found), fall back to synthesis.
4.  [x] **MCP Tool Matching & Scoring**:
    *   Implemented `compute_mcp_tool_score()` with keyword overlap, action verb synonyms, and description matching.
    *   Fixed `keyword_overlap()` tokenization to split on underscores (e.g., `list_issues` → `["list", "issues"]`).
    *   Threshold: score ≥ 3.0 OR overlap ≥ 0.75 to match a tool.
5.  [x] **Server Name Extraction**:
    *   Extract meaningful server names from override patterns (e.g., `"github"` from `"github.*"`).
    *   Capabilities saved with correct namespace: `mcp.github.list_issues` instead of `mcp.mcp-server.list_issues`.
6.  [x] **Synthesized Capability Execution**:
    *   Fixed arity mismatch bug in RTFS program construction (`({} {})` not `(({}) {})`).
    *   Synthesized capabilities execute via ephemeral RTFS runtime.
7.  [ ] **Implement StdioSessionHandler** (Future):
    *   Create a handler for local MCP servers (running via `npx` or executable).
    *   Implement JSON-RPC over stdio for process-based MCP tools.
8.  [ ] **MCP Tool Execution via Marketplace**:
    *   Currently MCP capabilities are discovered but execution uses synthesized code.
    *   Need to wire up `mcp.github.*` capabilities to make real MCP `tools/call` requests.

### Implementation Notes
-   The system now supports a **graceful degradation** pattern: real MCP → synthesis → generic mock.
-   MCP server configs detected via `capabilities/mcp/overrides.json` with pattern matching.
-   Discovered capabilities saved to `capabilities/servers/pending/{server_name}/`.
-   Promotion logic moves files from `pending/` to `approved/` upon approval.
-   `MCPSessionManager` handles HTTP-based MCP with `initialize` + `tools/list` + `tools/call`.

### Recent Fixes (2025-11-25)
-   **Arity mismatch**: Fixed program format from `(({}) {})` to `({} {})` for synthesized execution.
-   **Keyword overlap**: Fixed tokenization to split on underscores, enabling proper tool matching.
-   **Server name**: Now extracts from override pattern prefix instead of hardcoding.

## Phase G: MCP Execution & End-to-End Flow (COMPLETED ✅)
**Goal**: Complete the MCP integration by executing discovered tools via real MCP `tools/call` requests.

### Tasks
1.  [x] **Implement MCP Capability Executor**:
    *   `MCPExecutor` in `executors.rs` handles `mcp.*` capability IDs.
    *   Parses capability ID to extract server name and tool name.
    *   Resolves server URL from capability metadata or overrides.json.
    *   Makes `tools/call` request with arguments from the plan.
2.  [x] **Wire Up Marketplace to MCP Executor**:
    *   `CapabilityMarketplace` detects MCP capabilities via `requires_session_management` metadata.
    *   Routes execution to session pool → MCPSessionHandler.
    *   Handles response transformation (JSON → RTFS Value).
3.  [x] **Session Management**:
    *   `SessionPoolManager` manages MCP sessions with proper initialization.
    *   `MCPSessionHandler` implements full MCP lifecycle: initialize → tools/call → terminate.
    *   Session reuse via session pool for efficiency.
4.  [x] **Direct MCP Matching Before Decomposition**:
    *   `try_direct_mcp_match()` checks for MCP tools that directly match goal.
    *   Avoids over-decomposition for simple API calls.
    *   Uses `compute_mcp_tool_score()` with semantic matching (score ≥ 3.0).
5.  [x] **End-to-End Test**:
    *   `--goal "get my github user information"` works with real MCP execution!
    *   Flow: Direct MCP match (get_me, score: 5.18) → Session initialization → tools/call → User data returned

### Verified Working (2025-11-25)
```bash
cargo run --example autonomous_agent_demo -- \
  --goal "get my github user information" \
  --no-mock --config ../config/agent_config.toml
```
Output:
- Direct MCP match found: `get_me` (score: 5.18)
- Session initialized: `3d64b01c-916d-45f4-a011-23127e19e447`
- Tool executed successfully
- User data returned: `mandubian` (Pascal Voitot) with full profile

### Success Criteria
-   [x] `mcp.github.get_me` executes via real GitHub MCP `tools/call`.
-   [x] Plan execution returns actual GitHub user data.
-   [x] Direct MCP matching avoids over-decomposition.

## Phase H: Multi-Step MCP Workflows & Advanced Features (In Progress)
**Goal**: Extend MCP integration to handle complex multi-step workflows with data dependencies.

### Tasks
1.  [x] **Embedding-Based MCP Tool Matching** (Completed 2025-11-25):
    *   Integrated `EmbeddingService` into `IterativePlanner` for semantic tool matching.
    *   Provider priority: local Ollama first, then OpenRouter fallback.
    *   Combined scoring formula: `keyword_score + (embedding_similarity * 5.0)`.
    *   Added embedding scoring to both primary and secondary intent-matching loops.
    *   Extended GitHub hint patterns (pull_request, pr, commit, branch).
    *   Test confirmed: correctly matches `list_pull_requests` with score 20.02.
2.  [x] **MCP Tool with Arguments**:
    *   Test `list_issues` with `owner`/`repo` arguments.
    *   Test `list_commits` with multiple parameters.
    *   Verify argument extraction from goal description works.
    *   **Fixed issue**: Argument extraction returned `{}` because schema was missing from metadata - fixed by properly embedding `mcp_input_schema_json` in metadata.
3.  [x] **Multi-Step MCP Plans**:
    *   Test goals like "list issues in mandubian/ccos and get the first issue details".
    *   Verify data flow between MCP steps.
    *   Test context accumulation for MCP results.
    *   **Verified**: Complex goals like "list issues... but ask me for page size" now trigger decomposition and context-aware data flow.
    *   **Synthesized Capability Prompt Fix**: Improved prompt to avoid hallucinated `read-string` function and suggest correct `tool/parse-json` for numeric parsing.
    *   **Adapter Synthesis Fix**: Enhanced prompt to handle string inputs from user prompts correctly, avoiding extraction of fields from simple strings.
4.  [ ] **MCP Error Handling**:
    *   Parse MCP error responses and convert to RuntimeError.
    *   Integrate with Phase D repair loop for MCP-specific failures.
    *   Handle rate limiting, auth errors, and network failures gracefully.
5.  [ ] **Stdio MCP Server Support**:
    *   Implement `StdioSessionHandler` for local MCP servers.
    *   Support process-based MCP tools (via `npx` or executable).
    *   JSON-RPC over stdio communication.
6.  [ ] **MCP Capability Caching**:
    *   Cache discovered MCP capabilities across sessions.
    *   Refresh capabilities on session initialization.
    *   Handle capability version changes.

### Recent Progress (2025-11-25)
-   **Embedding integration**: `EmbeddingService` now used in `autonomous_agent_demo.rs` for MCP tool scoring.
-   **Provider flexibility**: Supports local Ollama (`LOCAL_EMBEDDING_URL`/`LOCAL_EMBEDDING_MODEL`) or remote OpenRouter.
-   **Improved matching**: Combined keyword + embedding scoring significantly improves tool selection accuracy.
-   **Synthesis Prompt**: Updated prompt to recommend `tool/parse-json` instead of hallucinated `read-string`.
-   **Adapter Synthesis**: Fixed crash when LLM tries to `get` fields from a simple string result (e.g., from `ccos.user.ask`).

## Phase I: Advanced Planning & Multi-Step Orchestration (Future)
**Goal**: Improve plan quality and handle complex multi-capability workflows.

### Tasks
1.  [ ] **Plan Validation Before Execution**:
    *   Validate that all referenced capabilities exist.
    *   Check argument compatibility between steps.
    *   Detect circular dependencies.
2.  [ ] **Parallel Step Execution**:
    *   Identify independent steps that can run in parallel.
    *   Use `tokio::join!` for concurrent capability execution.
3.  [ ] **Conditional Execution**:
    *   Support `if/else` in plans based on step results.
    *   Example: "If issues exist, format them; otherwise, return 'No issues found'."
4.  [ ] **Loop/Iteration Support**:
    *   Handle "for each issue, do X" patterns.
    *   Generate RTFS `map`/`filter`/`reduce` constructs.

## Phase J: Modular Planner Architecture (NEW - Completed 2025-11-25) ✅
**Goal**: Refactor planning into a modular architecture that separates WHAT (decomposition) from HOW (resolution), with proper IntentGraph integration.

### Problem Statement
The original `autonomous_agent_demo` approach had a key issue: LLM decomposition produced "capability hints" that looked like tool IDs (e.g., `github.list_issues`), but these were hallucinated and didn't match actual MCP tool names. This caused resolution to fail or require expensive secondary matching.

### Solution: Modular Planner
Created a new `modular_planner` module with pluggable strategies:

### Decomposition Strategies (WHAT to do)
1.  [x] **PatternDecomposition** (Fast, deterministic):
    *   4 regex patterns for common goal structures:
        - `"X but ask me for Y"` → user input + main action
        - `"ask me for X then Y"` → user input then action
        - `"X then Y"` → sequential actions
        - `"X and filter/sort by Y"` → action with transform
    *   Extracts `owner/repo` parameters automatically.
    *   Returns `SubIntent` with semantic `IntentType` (not tool IDs).

2.  [x] **IntentFirstDecomposition** (LLM-based):
    *   LLM produces abstract intents without tool hints.
    *   Focuses on semantic meaning, not implementation.

3.  [x] **GroundedLlmDecomposition** (Embedding-filtered):
    *   Pre-filters tools using embeddings before LLM prompt.
    *   Only includes relevant tools in context.
    *   Reduces hallucination risk.

4.  [x] **HybridDecomposition** (Pattern-first, LLM fallback):
    *   Tries patterns first for speed.
    *   Falls back to LLM for complex goals.

### Resolution Strategies (HOW to do it)
1.  [x] **SemanticResolution**:
    *   Embedding-based capability matching.
    *   Keyword fallback when embeddings unavailable.

2.  [x] **McpResolution**:
    *   Direct MCP tool discovery.
    *   Real-time tool listing from servers.

3.  [x] **CatalogResolution**:
    *   Local capability catalog search.
    *   Built-in support for `ccos.user.ask`, `ccos.io.println`.

### Core Types
-   **SubIntent**: Semantic intent with `IntentType`, `DomainHint`, dependencies, extracted params.
-   **IntentType**: `UserInput`, `ApiCall { action }`, `DataTransform { transform }`, `Output`, `Composite`.
-   **ApiAction**: `List`, `Get`, `Create`, `Update`, `Delete`, `Search`, `Execute`.
-   **DomainHint**: `GitHub`, `Git`, `DevOps`, `Data`, `AI`, `General`.
-   **ResolvedCapability**: `Local`, `Remote`, `BuiltIn`, `Synthesized`, `NeedsReferral`.

### IntentGraph Integration
-   [x] All intents stored as `StorableIntent` nodes in IntentGraph.
-   [x] Proper edge creation: `IsSubgoalOf` (parent-child), `DependsOn` (data flow).
-   [x] Root intent created for original goal.
-   [x] Sub-intents linked to root with edges.
-   [x] Resolution results attached to intent nodes.

### ModularPlanner Orchestrator
-   [x] Coordinates: decompose → store → resolve → plan.
-   [x] Configurable: `max_depth`, `persist_intents`, `create_edges`, `intent_namespace`.
-   [x] Returns `PlanResult` with: `root_intent_id`, `intent_ids`, `resolutions`, `rtfs_plan`, `trace`.
-   [x] RTFS plan generation from resolved capabilities.

### Example Demo
Created `modular_planner_demo.rs`:
```bash
# Pattern-based decomposition
cargo run --example modular_planner_demo -- \
  --goal "list issues in mandubian/ccos but ask me for the page size" --verbose

# With MCP discovery
cargo run --example modular_planner_demo -- \
  --goal "list issues then filter by label" --discover-mcp --verbose
```

### Test Results
-   19 unit tests passing for modular_planner module.
-   Pattern decomposition correctly identifies goal structures.
-   Built-in capabilities (`ccos.user.ask`) resolve without external lookup.
-   IntentGraph stores all planning decisions for audit/reuse.

### Files Created
-   `ccos/src/planner/modular_planner/mod.rs` - Module exports
-   `ccos/src/planner/modular_planner/types.rs` - SubIntent, IntentType, etc.
-   `ccos/src/planner/modular_planner/decomposition/mod.rs` - Strategy trait + exports
-   `ccos/src/planner/modular_planner/decomposition/pattern.rs` - Regex patterns
-   `ccos/src/planner/modular_planner/decomposition/intent_first.rs` - LLM-based
-   `ccos/src/planner/modular_planner/decomposition/grounded_llm.rs` - Embedding-filtered
-   `ccos/src/planner/modular_planner/decomposition/hybrid.rs` - Pattern + LLM
-   `ccos/src/planner/modular_planner/resolution/mod.rs` - Strategy trait + exports
-   `ccos/src/planner/modular_planner/resolution/semantic.rs` - Embedding-based
-   `ccos/src/planner/modular_planner/resolution/mcp.rs` - MCP discovery
-   `ccos/src/planner/modular_planner/resolution/catalog.rs` - Local catalog
-   `ccos/src/planner/modular_planner/orchestrator.rs` - Main planner
-   `ccos/examples/modular_planner_demo.rs` - Demo example

### Next Steps (Phase K)
1.  [x] Integrate modular planner into `autonomous_agent_demo` as optional mode.
2.  [ ] Add LLM provider to HybridDecomposition for fallback.
3.  [x] Connect McpResolution to real MCP session pool.
    *   Implemented `RuntimeMcpDiscovery` using `MCPSessionManager` for discovery.
    *   Integrated `SessionPoolManager` with `MCPSessionHandler` for execution.
    *   Verified 401 auth flow works (proving connectivity).
    *   Implemented `CcosCatalogAdapter` to bridge core CatalogService to planner's CapabilityCatalog.
4.  [x] Add execution support to `modular_planner_demo`.
    *   Added `CCOS` initialization and `SessionPoolManager` setup.
    *   Connected `ModularPlanner` to `CCOS` IntentGraph.
    *   Plan execution via `validate_and_execute_plan` works.
    *   Replaced placeholder `McpToolCatalog` with real `CcosCatalogAdapter`.
5.  [x] Implement composite resolution (try multiple strategies).
    *   `CompositeResolution` strategy tries Catalog first, then MCP.
    *   Working in `modular_planner_demo`.
6.  [x] Integrate modular planner into `autonomous_agent_demo` as optional mode.
    *   Added `--use-modular-planner` flag.
    *   Configured `CompositeResolution` with catalog and MCP strategies.
    *   Connected to real `SessionPool` and `IntentGraph`.
    *   Verified end-to-end execution works (e.g., `ccos.user.ask` + `mcp.github.list_issues`).

## Phase K: MCP Catalog Indexing & Data Pipeline Fixes (Completed 2025-11-27) ✅
**Goal**: Fix MCP tool resolution and data processing pipeline for end-to-end multi-step workflows.

### Problem Statement
Multiple issues blocked successful execution of complex goals like "list repositories and keep the one with most stars":
1. MCP tools resolved to wrong capabilities (e.g., `get_me` → `ccos.user.ask` score 0.4)
2. MCP tools re-discovered on every request instead of being cached
3. LLM used display names instead of capability IDs in plans
4. Data pipeline failed with "expected list, got string" for step chaining
5. `extract_list_from_value` was too GitHub-specific

### Tasks Completed
1.  [x] **MCP Tool Catalog Indexing**:
    *   Added `catalog: Option<Arc<CatalogService>>` to `RuntimeMcpDiscovery`.
    *   New `with_catalog()` builder method to inject catalog reference.
    *   `register_tool()` now automatically indexes discovered MCP tools in catalog.
    *   `list_available_tools()` triggers tool registration → catalog population.
    *   Deferred resolution: `search_catalog()` returns `None` when `_suggested_tool` exists but not found, letting MCP resolution try.

2.  [x] **Tool ID vs Display Name Fix**:
    *   `list_available_tools()` now uses `cap.id` instead of `cap.name` for tool summaries.
    *   Added display name fallback lookup in `search_catalog()` for LLM compatibility.
    *   LLM prompts now show correct capability IDs for resolution.

3.  [x] **Data Pipeline with `_previous_result`**:
    *   Updated `extract_data_and_params()` to detect string references like `"step_0_result"`.
    *   When `data` parameter is a string reference, prefers `_previous_result` from params.
    *   Fixes data flow between steps in RTFS execution.

4.  [x] **Generic List Extraction**:
    *   Refactored `extract_list_from_value()` to recursively search any map for list values.
    *   No longer GitHub-specific - works with any API response structure.
    *   Returns first list found in nested object structure.

### Files Modified
-   `ccos/src/planner/modular_planner/resolution/catalog.rs` - Deferred MCP resolution, display name fallback
-   `ccos/src/planner/modular_planner/resolution/mcp.rs` - Catalog injection, tool indexing
-   `ccos/src/capabilities/data_processing.rs` - `_previous_result` support, generic list extraction
-   `ccos/src/examples_common/builder.rs` - Wired catalog into RuntimeMcpDiscovery

### Test Results
```bash
cargo run --example modular_planner_demo --quiet -- \
  --pure-llm --goal "list repositories of user mandubian and keep the one with most stars" \
  --config config/agent_config.toml --discover-mcp
```
**Result**: Successful execution - resolves `mcp.github.list_user_repos`, chains through `ccos.data.sort` and `ccos.data.select`, returns repository data.

### Known Remaining Issues
1.  [x] **`ccos.io.println` Output**: Fixed to display `_previous_result` data instead of description text (completed 2025-11-27).
2.  [ ] **Repair Loop Integration**: Execution repair loop exists but not wired into `modular_planner_demo`.
3.  [x] **RTFS Step References**: Implemented regex-based post-processing to convert `{{stepN.result}}` handlebars syntax into valid RTFS variable references (completed 2025-11-27).

## Phase L: Polish & Remaining Issues (Next)

### HIGH Priority
1.  [x] **Fix `ccos.io.println` Output** (Completed 2025-11-27):
    *   Modified println capability to check `_previous_result` first (data pipeline flow).
    *   Updated Output intent resolution to only pass description when no dependencies exist.
    *   Ensures "list issues and display them" actually displays the issues data.

2.  [ ] **Integrate Repair Loop**:
    *   Wire existing repair loop from `autonomous_agent_demo` into `modular_planner_demo`.
    *   Enable self-correction for execution failures.

### MEDIUM Priority
3.  [ ] **MCP Error Handling**:
    *   Parse MCP error responses and convert to RuntimeError.
    *   Handle rate limiting, auth errors, network failures gracefully.
    *   Integrate with repair loop for MCP-specific failures.

4.  [x] **Domain-Based Tool Filtering**:
    *   Implemented `infer_all_from_text` in `DomainHint`.
    *   Updated `GroundedLlmDecomposition` to pre-filter tools by domain.
    *   Reduces token usage and noise for LLM when many tools are available.
    *   Includes `Generic` tools (e.g. `println`, `ask`) in all contexts.

5.  [x] **Centralized & Session-Aware MCP Discovery**:
    *   Refactored MCP discovery to use `MCPSessionManager` instead of raw HTTP requests.
    *   Centralized configuration in `LocalConfigMcpDiscovery` (`overrides.json` + ENV).
    *   Unified planner discovery logic (`RuntimeMcpDiscovery`) to delegate to `MCPDiscoveryProvider`.
    *   Ensures consistent session lifecycle, auth, and error handling across all discovery paths.

6.  [x] **Semantic Tool Filtering Fallback**:
    *   Fixed `HybridDecomposition` to respect `max_grounded_tools` configuration.
    *   Injected `EmbeddingService` into decomposition strategy via `ModularPlannerBuilder`.
    *   Ensures that when domain filtering yields no results (unknown domain), the system falls back to semantic embedding search instead of sending all tools to LLM.

7.  [x] **Improve RTFS Variable References**:
    *   Implemented regex-based post-processing to convert `{{stepN.result}}` handlebars syntax from LLM into valid RTFS variable references (e.g. `step_1`).
    *   Validates arguments to ensure only valid RTFS syntax is generated.

8.  [x] **HybridDecomposition LLM Fallback**:
    *   Add LLM provider to HybridDecomposition for complex goals.
    *   Pattern-first, LLM-fallback architecture.
    *   Added semantic tool filtering to the fallback path.

### LOW Priority
9.  [ ] **Stdio MCP Server Support**:
    *   Implement `StdioSessionHandler` for local MCP servers.
    *   Support process-based MCP tools.

10. [ ] **MCP Capability Caching**:
    *   Cache discovered MCP capabilities across sessions.
    *   Handle capability version changes.

## Phase 3: Robustness & Scaling (Future)
