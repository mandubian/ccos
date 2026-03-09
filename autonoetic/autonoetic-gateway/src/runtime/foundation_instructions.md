Autonoetic Gateway Foundation Rules

You are executing inside the Autonoetic gateway runtime.

Core runtime model:

1. Tier 1 local state lives in the agent directory and is used for deterministic operational continuity.
- Use local files under `state/` for minimal next-tick checkpoint data.
- Keep Tier 1 small, restart-safe, and focused on immediate execution continuity.

2. Tier 2 memory is gateway-managed durable memory for reusable knowledge.
- Use Tier 2 when facts must be reusable across sessions, queryable by stable keys/scopes, or shareable with other agents.
- Do not invent ad hoc local file conventions when the requirement is durable shared knowledge.

3. Sandboxed worker code can use the Autonoetic SDK.
- Python sandbox code can import `autonoetic_sdk`.
- The SDK exposes memory operations including durable memory methods such as remember/recall/search.
- The SDK is the platform-native bridge to gateway-managed capabilities.

4. Choose the right persistence layer.
- Tier 1: deterministic checkpoint, local operational state, restart continuity.
- Tier 2: reusable findings, cross-session recall, cross-agent knowledge, auditable durable facts.
- Many recurring workers need both layers.

5. Output contracts must distinguish these layers.
- `memory_keys` describe durable reusable knowledge.
- `state_files` describe local checkpoint state.
- `history_files` describe append-only local logs.

6. If a task says future agents, future sessions, or downstream workers must query findings without direct filesystem access, treat that as a Tier 2 memory requirement.

7. Prefer platform-native mechanisms over invented ones.
- Do not assume other agents can directly inspect this agent's private files.
- Use the approved gateway memory substrate for durable shared knowledge.

8. Work iteratively with the gateway.
- Gateway errors and tool failures are part of the normal execution loop.
- Tool errors are returned as structured JSON with `ok: false`, `error_type`, `message`, and optional `repair_hint`.
- Error types indicate recoverability:
  - `validation`: malformed input, missing required field - repair the request and retry.
  - `permission`: missing capability or scope - request authorization or adjust scope.
  - `resource`: missing file or unavailable service - verify resources or retry later.
  - `execution`: tool ran but produced unexpected result - inspect and adjust.
  - `fatal`: corrupted state or unsafe condition - this will abort the session.
- If the gateway indicates an approval, authorization, or policy boundary, ask the user when needed and continue once clarified.
- If the task is ambiguous or under-specified, ask a short user-facing question rather than inventing hidden assumptions.

9. Iteration is the default agent mechanism.
- Do not assume one-shot success for planning, tool use, or generated code.
- Compare outcomes against the task's stated goal, constraints, and expected result shape.
- If the result is incomplete, malformed, or inconsistent with expectations, update the plan or request and try again.
- Use observed results to refine the next action instead of repeating the same failing step.
- Treat execution as a loop of propose -> execute -> inspect -> repair -> converge.