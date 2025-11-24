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
-   Discovered capabilities saved to `capabilities/discovered/mcp/{server_name}/`.
-   `MCPSessionManager` handles HTTP-based MCP with `initialize` + `tools/list` + `tools/call`.

### Recent Fixes (2025-11-25)
-   **Arity mismatch**: Fixed program format from `(({}) {})` to `({} {})` for synthesized execution.
-   **Keyword overlap**: Fixed tokenization to split on underscores, enabling proper tool matching.
-   **Server name**: Now extracts from override pattern prefix instead of hardcoding.

## Phase G: MCP Execution & End-to-End Flow (Next Priority)
**Goal**: Complete the MCP integration by executing discovered tools via real MCP `tools/call` requests.

### Tasks
1.  [ ] **Implement MCP Capability Executor**:
    *   Create an executor that handles `mcp.*` capability IDs.
    *   Parse capability ID to extract server name and tool name (e.g., `mcp.github.list_issues` → server: `github`, tool: `list_issues`).
    *   Resolve server URL from overrides.json.
    *   Make `tools/call` request with arguments from the plan.
2.  [ ] **Wire Up Marketplace to MCP Executor**:
    *   Modify `CapabilityMarketplace` to detect `mcp.*` capabilities.
    *   Route execution to MCP executor instead of RTFS evaluation.
    *   Handle response transformation (JSON → RTFS Value).
3.  [ ] **Session Management**:
    *   Reuse MCP sessions across multiple tool calls in a plan.
    *   Cache session by server URL to avoid repeated initialization.
    *   Handle session expiry and reconnection.
4.  [ ] **Error Handling for MCP Calls**:
    *   Parse MCP error responses and convert to RuntimeError.
    *   Integrate with Phase D repair loop for MCP-specific failures.
    *   Handle rate limiting, auth errors, and network failures gracefully.
5.  [ ] **End-to-End Test**:
    *   Run `--goal "list issues in mandubian/ccos"` with real MCP execution.
    *   Verify: MCP discovery → MCP tool call → JSON response → formatted output.

### Success Criteria
-   `mcp.github.list_issues` executes via real GitHub MCP `tools/call`.
-   Plan execution returns actual GitHub issues data.
-   No synthesized fallback for MCP-discovered capabilities.

## Phase H: Advanced Planning & Multi-Step Orchestration (Future)
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
