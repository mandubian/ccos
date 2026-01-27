---
name: CCOS MCP Tools
description: Reference for all MCP tools exposed by the CCOS server for agent interactions
---

# CCOS MCP Tools Reference

The CCOS MCP server exposes tools for capability discovery, execution, session management, and learning.

## 🔍 Discovery Tools

### `ccos_search`
Search for capabilities by query, ID pattern, or domain.

```json
{
  "query": "weather forecast",       // Required: search query
  "domains": ["weather", "geo"],     // Optional: domain filters
  "limit": 10,                       // Optional: max results (default: 10)
  "min_score": 0.3                   // Optional: minimum relevance (default: 0.0)
}
```

### `ccos_suggest_apis`
Get LLM suggestions for external APIs matching a goal.

```json
{
  "query": "get cryptocurrency prices"  // What you want to accomplish
}
```
**Logic:** If 1 suggestion → use it. If several and confident → choose one. If uncertain → ask user.

### `ccos_introspect_remote_api`
Introspect external server (MCP, OpenAPI, or HTML docs) and create approval request.

```json
{
  "endpoint": "https://api.example.com/openapi.json",  // Required
  "name": "Example API",                                // Optional display name
  "auth_env_var": "EXAMPLE_API_KEY"                    // Optional auth var
}
```

### `ccos_inspect_capability`
Get detailed schema information for a capability.

```json
{
  "capability_id": "weather.get_forecast"  // The capability to inspect
}
```

---

## ⚡ Execution Tools

### `ccos_execute_capability` ⭐ PRIMARY
Execute a capability with JSON inputs - **no RTFS knowledge needed**.

```json
{
  "capability_id": "geocoding_api.direct_geocoding_by_location_name",
  "inputs": { "city": "Paris", "limit": 1 },
  "session_id": "optional-session-id",          // Optional: track multi-step
  "original_goal": "get weather for Paris"      // Optional: for learning
}
```

### `ccos_execute_plan`
Execute raw RTFS code.

```json
{
  "plan": "(let [x 1] (+ x 2))",   // RTFS code string
  "dry_run": false,                // If true, validate only
  "original_goal": "test math"     // Optional
}
```

### `ccos_plan`
Decompose a goal into sub-intents using LLM.

```json
{
  "goal": "get weather in paris tomorrow and send as email"
}
```
Returns: plan with resolved/gap steps, `next_action` guidance.

---

## 📋 Session Management

### `ccos_session_start`
Start a new planning/execution session.

```json
{
  "goal": "Build a daily report agent",
  "context": { "preference": "concise" }  // Optional
}
```

### `ccos_session_plan`
Get the accumulated RTFS plan from a session.

```json
{
  "session_id": "session_123456"
}
```

### `ccos_session_end`
End session and save the RTFS plan.

```json
{
  "session_id": "session_123456",
  "save_as": "my-plan.rtfs"       // Optional filename
}
```

### `ccos_consolidate_session`
Convert a session into a reusable Agent Capability.

```json
{
  "session_id": "session_123456",
  "agent_name": "daily_reporter",
  "description": "Generates daily consolidated reports"
}
```

---

## 🧠 Memory & Learning

### `ccos_log_thought`
Record agent reasoning for learning.

```json
{
  "thought": "The user wants weather + crypto data combined",
  "plan_id": "optional-plan-id",
  "is_failure": false            // Set true if recording failure
}
```

### `ccos_recall_memories`
Retrieve relevant memories by tags.

```json
{
  "tags": ["weather", "learning"],
  "limit": 10
}
```

### `ccos_record_learning`
Explicitly record a learned pattern.

```json
{
  "pattern": "Use batch API for multiple symbols",
  "context": "cryptocurrency price fetching",
  "outcome": "3x faster than individual calls",
  "confidence": 0.9
}
```

---

## 📜 Governance & Reference

### `ccos_get_constitution`
Get system rules and policies.

```json
{}
```

### `ccos_get_guidelines`
Get official agent guidelines from `docs/agent_guidelines.md`.

```json
{}
```

### `rtfs_get_grammar`
Get RTFS language grammar reference.

```json
{
  "category": "overview"  // overview|literals|collections|special_forms|types|purity_effects|all
}
```

### `ccos_list_capabilities`
List all registered CCOS capabilities.

```json
{}
```

---

## 🔐 Secrets & Approvals

### `ccos_check_secrets`
Check if required secrets are available.

```json
{
  "secret_names": ["OPENWEATHERMAP_API_KEY", "GITHUB_TOKEN"]
}
```

### `ccos_list_approvals`
List approval requests.

```json
{
  "status": "pending",  // pending|rejected|expired|approved|all
  "limit": 20
}
```

### `ccos_register_server`
Register an approved server's tools.

```json
{
  "approval_id": "approval_123456"
}
```

---

## 🛠️ Development Tools

### `ccos_synthesize_capability`
Generate RTFS capability using LLM.

```json
{
  "description": "Fetch and format weather data",
  "capability_name": "weather.formatted_forecast",
  "input_schema": { "type": "object", "properties": { "city": { "type": "string" } } },
  "output_schema": { "type": "object" }
}
```

### `ccos_compile_rtfs`
Validate RTFS syntax without execution.

```json
{
  "code": "(defn add [x y] (+ x y))"
}
```

### `rtfs_compile`
Parse RTFS code with optional AST output.

```json
{
  "code": "(+ 1 2)",
  "show_ast": true
}
```

---

## Common Workflow Pattern

```
1. ccos_search → Find capabilities
2. ccos_inspect_capability → Get input schema
3. ccos_session_start → Begin session
4. ccos_execute_capability → Execute with inputs
5. ccos_session_end → Save RTFS plan
```
