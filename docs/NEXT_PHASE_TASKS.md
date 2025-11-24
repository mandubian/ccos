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

## Phase F: Real-World Integration (MCP)
**Goal**: Transition from generic mocks to real MCP services.

### Tasks
1.  [ ] **Integrate MCPSessionHandler**:
    *   Use `ccos::capabilities::mcp_session_handler` to connect to remote MCP servers (SSE).
2.  [ ] **Implement StdioSessionHandler**:
    *   Create a handler for local MCP servers (running via `npx` or executable).
    *   Implement JSON-RPC over stdio.
3.  [ ] **Hybrid Resolution Strategy**:
    *   Update `resolve_step` to:
        1.  Check Registry.
        2.  If found, try connecting/installing real tool.
        3.  If connection fails, fall back to Generic Mock.
