---
name: Agent Workflow Patterns
description: Common patterns for building agent workflows in CCOS
---

# Agent Workflow Patterns

Common patterns for building effective agent workflows in CCOS.

## Discovery → Execution Flow

The standard pattern for accomplishing goals:

```
┌─────────────────┐
│ 1. Search       │  ccos_search or ccos_plan
└────────┬────────┘
         ▼
┌─────────────────┐
│ 2. Inspect      │  ccos_inspect_capability
└────────┬────────┘
         ▼
┌─────────────────┐
│ 3. Execute      │  ccos_execute_capability
└────────┬────────┘
         ▼
┌─────────────────┐
│ 4. Learn        │  ccos_log_thought (if needed)
└─────────────────┘
```

### Example: Weather Query
```
1. ccos_search { query: "weather current city" }
   → Returns: weather.get_current (score: 0.85)

2. ccos_inspect_capability { capability_id: "weather.get_current" }
   → Returns: input_schema with { city: string, units: string? }

3. ccos_execute_capability {
     capability_id: "weather.get_current",
     inputs: { city: "Paris", units: "metric" }
   }
   → Returns: { temperature: 18, conditions: "cloudy" }
```

## Session-Based Multi-Step

For complex goals requiring multiple capabilities:

```
1. ccos_session_start { goal: "Generate daily report" }
   → Returns: session_id

2. ccos_execute_capability {
     capability_id: "weather.get_current",
     inputs: { city: "Paris" },
     session_id: "session_xxx"
   }

3. ccos_execute_capability {
     capability_id: "crypto.get_price",
     inputs: { symbol: "BTC" },
     session_id: "session_xxx"
   }

4. ccos_session_plan { session_id: "session_xxx" }
   → Returns: accumulated RTFS plan

5. ccos_session_end { session_id: "session_xxx" }
   → Saves plan to file
```

## Gap Resolution Pattern

When `ccos_plan` identifies missing capabilities:

```
1. ccos_plan { goal: "get flight prices to tokyo" }
   → Returns: { status: "gap", suggested_query: "flight booking API" }

2. ccos_suggest_apis { query: "flight booking API" }
   → Returns: [{ name: "Skyscanner", docs_url: "..." }]

3. ccos_introspect_remote_api { endpoint: "..." }
   → Creates approval request

4. [User approves at /approvals]

5. ccos_register_server { approval_id: "..." }
   → Capabilities now available

6. ccos_plan { goal: "get flight prices to tokyo" }
   → Now resolves successfully!
```

## Error Handling Pattern

Handle failures gracefully using logging:

```
1. ccos_execute_capability { ... }
   → Returns: { success: false, error: "API rate limited" }

2. ccos_log_thought {
     thought: "API rate limited, will retry after delay",
     is_failure: true
   }

3. [Wait or try alternative]

4. ccos_execute_capability { ... }  # Retry
```

## Learning from Failures

Record patterns to avoid repeating mistakes:

```clojure
;; When something goes wrong
ccos_log_thought {
  thought: "Parameter 'symbol' requires uppercase (BTC not btc)",
  plan_id: "crypto_fetch",
  is_failure: true
}

;; Explicitly record the learning
ccos_record_learning {
  pattern: "Always uppercase cryptocurrency symbols",
  context: "crypto API calls",
  outcome: "Prevents 400 errors",
  confidence: 0.95
}

;; Later, recall before similar tasks
ccos_recall_memories { tags: ["crypto", "learning"] }
```

## Reusable Agent Creation

Convert successful sessions into permanent agents:

```
1. Complete a multi-step session successfully

2. ccos_consolidate_session {
     session_id: "session_xxx",
     agent_name: "daily_reporter",
     description: "Generates weather + crypto reports"
   }
   → Creates: capabilities/agents/daily_reporter.rtfs

3. Future use:
   ccos_execute_capability {
     capability_id: "agent.daily_reporter",
     inputs: { city: "Paris" }
   }
```

## Governance-First Pattern

Always check approvals for sensitive operations:

```
1. ccos_check_secrets { secret_names: ["GITHUB_TOKEN"] }
   → If missing: "CRITICAL: Ask user to approve at /approvals"

2. [Wait for user confirmation]

3. ccos_execute_capability { ... }
```

## Best Practices

### Do's ✅
- Use `ccos_execute_capability` for capability calls (not raw RTFS)
- Start sessions for multi-step workflows
- Log thoughts for debugging and learning
- Check secrets before API calls
- Wait for user approval when required

### Don'ts ❌
- Don't guess API keys or secrets
- Don't bypass the approval system
- Don't retry indefinitely on failures
- Don't ignore `agent_guidance` in responses
- Don't write raw RTFS unless specifically needed

## Quick Reference

| Goal | Primary Tool |
|------|-------------|
| Find capabilities | `ccos_search` |
| Decompose goal | `ccos_plan` |
| Execute capability | `ccos_execute_capability` |
| Track multi-step | `ccos_session_start/end` |
| Add external API | `ccos_introspect_remote_api` |
| Check available secrets | `ccos_check_secrets` |
| Learn from execution | `ccos_log_thought` |
| Create agent | `ccos_consolidate_session` |
