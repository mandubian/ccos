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

## Phase F: Real-World Integration (MCP) (In Progress)
**Goal**: Transition from generic mocks to real MCP services.

### Tasks
1.  [x] **Integrate MCPDiscoveryProvider**:
    *   Use `MCPDiscoveryProvider` to connect to real MCP servers.
    *   Added `try_real_mcp_discovery()` method that uses the existing `MCPDiscoveryProvider`.
    *   Implemented in `try_install_from_registry()` with fallback to generic mocks.
2.  [x] **Add MCP Server Configuration Support**:
    *   Created `find_mcp_server_config()` to detect MCP servers from environment variables.
    *   Example: GitHub MCP server detection via `GITHUB_MCP_ENDPOINT` and `GITHUB_TOKEN`.
    *   Extensible design for adding more MCP server configurations.
3.  [x] **Hybrid Resolution Strategy**:
    *   Updated `try_install_from_registry` to:
        1.  Check for known MCP server configurations.
        2.  If found, attempt real MCP connection and discovery.
        3.  On success, register and return the real capability.
        4.  On failure (connection error, tool not found), fall back to generic mock.
4.  [ ] **Implement StdioSessionHandler** (Future):
    *   Create a handler for local MCP servers (running via `npx` or executable).
    *   Implement JSON-RPC over stdio for process-based MCP tools.
5.  [ ] **Test with Real MCP Service**:
    *   Set up a real MCP server (e.g., GitHub MCP, filesystem MCP).
    *   Configure environment variables (e.g., `GITHUB_MCP_ENDPOINT`, `GITHUB_TOKEN`).
    *   Run the autonomous agent and verify it connects to real services.

### Implementation Notes
-   The system now supports a **graceful degradation** pattern: real MCP → generic mock.
-   MCP server configs are currently detected via environment variables but can be extended to read from `agent_config.toml`.
-   The `MCPDiscoveryProvider::discover()` method returns `CapabilityManifest` objects that are already compatible with the marketplace.
