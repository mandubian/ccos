# Iterative LLM Consultation for CCOS Agent

**Status:** Implemented  
**Date:** 2026-02-10  
**Component:** ccos-agent  

## Overview

The CCOS Agent now supports **iterative LLM consultation**, enabling autonomous multi-step task execution. Instead of planning all actions upfront and executing them in a batch, the agent consults the LLM after each action, allowing the LLM to analyze results and decide on the next step dynamically.

### Key Benefits

- **True autonomy**: Agent continues working until task completion without user intervention
- **Adaptive planning**: LLM can adjust strategy based on actual results (e.g., API responses, errors)
- **No brittle heuristics**: No hardcoded logic for "what to do next" - the LLM decides
- **Better error recovery**: Failed actions can be retried with different parameters
- **Context-aware**: Full action history is available to inform decisions

## Architecture

### Flow Diagram

```
┌─────────┐     ┌─────────┐     ┌─────────┐     ┌─────────┐
│  User   │────▶│  Agent  │────▶│   LLM   │◀────│  Agent  │
└─────────┘     └────┬────┘     └────┬────┘     └────▲────┘
                     │               │               │
                     │         [Initial Plan]        │
                     │               │               │
                     │               ▼               │
                     │     ┌───────────────────┐     │
                     │     │  Execute Action 1 │     │
                     │     └─────────┬─────────┘     │
                     │               │               │
                     │               ▼               │
                     │     ┌───────────────────┐     │
                     │     │  Consult LLM      │─────┘
                     │     │  (with results)   │
                     │     └─────────┬─────────┘
                     │               │
                     │         [Next Action?]
                     │               │
                     │     ┌─────────▼─────────┐
                     │     │  task_complete?   │──Yes──▶ Final Response
                     │     └─────────┬─────────┘
                     │               │ No
                     │               ▼
                     │     ┌───────────────────┐
                     │     │  Execute Action N │
                     │     └───────────────────┘
                     │               │
                     └───────────────┘
```

### Components

#### 1. Configuration Layer
- **File**: `config/agent_config.toml`
- **Struct**: `AutonomousAgentConfig` in `ccos/src/config/types.rs`
- Controls behavior: max iterations, failure handling, context management

#### 2. LLM Client Extension
- **File**: `ccos/src/chat/agent_llm.rs`
- **New Types**:
  - `ActionResult`: Records action execution history
  - `IterativeAgentPlan`: Extended plan with `task_complete` flag
- **New Method**: `consult_after_action()` - Consults LLM with action results

#### 3. Agent Runtime
- **File**: `ccos/src/bin/ccos_agent.rs`
- **Core Function**: `process_with_llm()` - Implements iterative loop
- Manages: context, action history, iteration limits, failure handling

## Configuration

### Config File (`config/agent_config.toml`)

```toml
[autonomous_agent]
# Enable iterative mode (agent consults LLM after each action)
enabled = true

# Maximum iterations per user request (safety limit)
max_iterations = 10

# Maximum context entries to keep in conversation history
max_context_entries = 20

# Context management strategy when max_context_entries exceeded:
# - "truncate": Remove oldest entries (default)
# - "summarize": Compress old entries (falls back to truncate if not implemented)
context_strategy = "truncate"

# Enable intermediate progress responses to user
# When true, agent sends updates after each action
send_intermediate_responses = false

# On action failure: "ask_user" or "abort"
failure_handling = "ask_user"

# Max consecutive failures before asking user (prevents infinite loops)
max_consecutive_failures = 2
```

### CLI Overrides

Command-line arguments override config file values:

```bash
# Enable/disable autonomous mode
ccos-agent --autonomous-enabled true

# Set max iterations
ccos-agent --autonomous-max-iterations 15

# Enable intermediate responses
ccos-agent --autonomous-intermediate

# Change failure handling
ccos-agent --autonomous-failure abort
```

### Environment Variables

```bash
export CCOS_AUTONOMOUS_ENABLED=true
export CCOS_AUTONOMOUS_MAX_ITERATIONS=10
export CCOS_AUTONOMOUS_INTERMEDIATE=false
export CCOS_AUTONOMOUS_FAILURE=ask_user
```

## Implementation Details

### Iterative Loop Algorithm

```rust
async fn process_with_llm(&mut self, event: ChatEvent) -> anyhow::Result<()> {
    let mut iteration = 0;
    let mut action_history: Vec<ActionResult> = Vec::new();
    
    loop {
        iteration += 1;
        
        // Safety check
        if iteration > config.max_iterations {
            inform_user_max_reached();
            break;
        }
        
        // Consult LLM
        let plan = if iteration == 1 {
            // First iteration - get initial plan
            llm.process_message(...).await
        } else {
            // Subsequent iterations - include action results
            llm.consult_after_action(
                original_request,
                &action_history,
                &last_result,
                ...
            ).await
        };
        
        // Check completion
        if plan.task_complete {
            send_final_response(plan.response);
            break;
        }
        
        // Execute ONE action (not batch)
        let action = &plan.actions[0];
        let result = execute_capability(action).await;
        
        // Record for next iteration
        action_history.push(ActionResult {
            capability_id: action.capability_id,
            success: result.success,
            result: result.result,
            error: result.error,
            iteration,
        });
        
        // Handle failure
        if !result.success {
            if should_ask_user(&config, consecutive_failures) {
                ask_user_what_to_do();
                break;
            }
        }
        
        // Context management
        update_context(&mut context, action, result);
        if context.len() > config.max_context_entries {
            truncate_or_summarize(&mut context, config.context_strategy);
        }
    }
}
```

### LLM System Prompt for Iterative Consultation

```
You are a CCOS autonomous agent working iteratively to complete a user's request.

ORIGINAL USER REQUEST:
{original_request}

ACTION HISTORY SO FAR:
{formatted_history}

RESULT OF LAST ACTION:
{last_result_json}

Your task:
1. Analyze the last action result above
2. Determine if the user's request is FULLY completed
3. If COMPLETE: Set task_complete to true and provide final response
4. If NOT COMPLETE: Plan exactly ONE next action

Guidelines:
- Be decisive: if the task is done, say so immediately
- Only plan ONE action at a time (not multiple)
- Don't repeat actions that already succeeded
- If an action failed, you may retry with different parameters

Response format (JSON):
{
  "understanding": "brief description of current state",
  "task_complete": true/false,
  "reasoning": "why complete or what needs to happen next",
  "actions": [
    {
      "capability_id": "capability.name",
      "reasoning": "why this action now",
      "inputs": { "param": "value" }
    }
  ],
  "response": "response to user (final if task_complete=true)"
}
```

### ActionResult Structure

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ActionResult {
    pub capability_id: String,      // e.g., "ccos.network.http-fetch"
    pub success: bool,              // Did the action succeed?
    pub result: Option<Value>,      // Response data
    pub error: Option<String>,      // Error message if failed
    pub iteration: u32,             // Which iteration this was
}
```

### IterativeAgentPlan Structure

```rust
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct IterativeAgentPlan {
    pub understanding: String,      // Current state summary
    pub actions: Vec<PlannedAction>, // Next actions (usually 1)
    pub response: String,           // Response to user
    pub task_complete: bool,        // NEW: Is task done?
    pub reasoning: String,          // NEW: Why continue/stop
}
```

## Usage Examples

### Example 1: Bitcoin Price + Calculation

**User Message**: "Fetch bitcoin rate from public endpoint and compute how many dollars are 0.5btc"

**Iteration 1**:
```json
{
  "understanding": "User wants Bitcoin price and calculation",
  "task_complete": false,
  "reasoning": "Need to fetch Bitcoin price first",
  "actions": [{
    "capability_id": "ccos.network.http-fetch",
    "reasoning": "Fetch BTC price from CoinGecko",
    "inputs": {"url": "https://api.coingecko.com/..."}
  }],
  "response": "Fetching Bitcoin price..."
}
```

**Result**: `{"bitcoin": {"usd": 70455}}`

**Iteration 2**:
```json
{
  "understanding": "Have BTC price: $70,455. Need to calculate 0.5 BTC value",
  "task_complete": false,
  "reasoning": "Need to compute 0.5 * 70455",
  "actions": [{
    "capability_id": "ccos.code.refined_execute",
    "reasoning": "Calculate dollar value of 0.5 BTC",
    "inputs": {
      "task": "Calculate 0.5 * 70455",
      "language": "python"
    }
  }],
  "response": "Calculating value..."
}
```

**Result**: `$35,227.50`

**Iteration 3**:
```json
{
  "understanding": "Calculation complete: 0.5 BTC = $35,227.50",
  "task_complete": true,
  "reasoning": "Task fully completed",
  "actions": [],
  "response": "0.5 Bitcoin is worth $35,227.50 at the current rate of $70,455 per BTC."
}
```

### Example 2: Skill Onboarding

**User Message**: "Load the weather skill from http://example.com/weather.md and onboard it"

**Iteration 1**: Load skill
**Iteration 2**: Check skill definition, plan onboarding steps  
**Iteration 3**: Execute first onboarding action  
**Iteration 4**: Execute second onboarding action  
**Iteration 5**: Mark complete

### Example 3: Error Recovery

**Iteration 1**: HTTP fetch fails (403 error)

**Iteration 2**:
```json
{
  "understanding": "HTTP fetch failed with 403 Forbidden",
  "task_complete": false,
  "reasoning": "Previous request blocked. Try with different headers or endpoint.",
  "actions": [{
    "capability_id": "ccos.network.http-fetch",
    "reasoning": "Retry with User-Agent header",
    "inputs": {
      "url": "https://api.example.com/data",
      "headers": {"User-Agent": "ccos-agent/1.0"}
    }
  }],
  "response": "First request failed. Trying alternative approach..."
}
```

## Comparison: Old vs New Behavior

### Old Behavior (Batch Execution)

```
User: Fetch bitcoin and calculate 0.5btc value
  ↓
LLM plans [http-fetch, refined_execute]
  ↓
Execute ALL actions in batch
  ↓
Send summary: "Completed: http-fetch, refined_execute"
  ↓
User: ??? (task incomplete, no calculation shown)
```

**Problems**:
- LLM planned all actions upfront without seeing intermediate results
- No adaptation based on actual API responses
- Heuristic-based follow-up (only triggered on "python" keyword)
- User had to manually continue

### New Behavior (Iterative)

```
User: Fetch bitcoin and calculate 0.5btc value
  ↓
Iteration 1: LLM plans [http-fetch]
  ↓
Execute http-fetch → Get price: $70,455
  ↓
Iteration 2: LLM sees result, plans [refined_execute]
  ↓
Execute refined_execute → Get calculation: $35,227.50
  ↓
Iteration 3: LLM marks complete
  ↓
Final response: "0.5 BTC = $35,227.50"
```

**Advantages**:
- LLM adapts to actual results
- No heuristics - pure LLM decision making
- Task completes autonomously
- Better error handling

## Failure Handling

### Consecutive Failures

If actions keep failing, the agent asks the user:

```
I encountered an issue with ccos.network.http-fetch:

Error: Connection timeout

Here's what I've accomplished so far:

✗ Step 1: ccos.network.http-fetch - failed

Would you like me to retry, try a different approach, 
or would you prefer to handle this manually?
```

### Configurable Strategies

**ask_user** (default):
- Ask user after `max_consecutive_failures` failures
- Provides action history
- Waits for user guidance

**abort**:
- Stop immediately on failure
- Send error message
- Exit processing loop

## Context Management

### Truncate Strategy (Default)

When `context.len() > max_context_entries`:
```rust
// Remove oldest entries, keep most recent
let remove_count = context.len() - max_context_entries;
context.drain(0..remove_count);
```

### Future: Summarize Strategy

Planned for future implementation:
- Use LLM to compress old context into summary
- Maintain semantic meaning with fewer tokens
- Fall back to truncate if summarization fails

## Safety Limits

### Max Iterations

Hard limit to prevent infinite loops:
```toml
max_iterations = 10
```

When reached:
```
I've reached the maximum number of steps (10). 
Here's what I completed:

✓ Step 1: ccos.network.http-fetch - {...}
✓ Step 2: ccos.code.refined_execute - {...}
...

Please let me know if you'd like me to continue.
```

### Budget Enforcement

Budget checks happen at each iteration:
- Max steps (action count)
- Max duration (time limit)
- Cost limits (if implemented)

## Testing

### Unit Tests

Test iterative consultation logic:
```rust
#[tokio::test]
async fn test_iterative_consultation() {
    let client = AgentLlmClient::new(config);
    let history = vec![ActionResult { ... }];
    
    let plan = client
        .consult_after_action(
            "fetch bitcoin",
            &history,
            &last_result,
            &[],
            &capabilities,
            ""
        )
        .await
        .unwrap();
    
    assert!(plan.task_complete || !plan.actions.is_empty());
}
```

### Integration Tests

Test full flow:
```bash
# Start gateway
cargo run --bin ccos-chat-gateway

# Start agent with autonomous mode
cargo run --bin ccos-agent -- \
  --enable-llm \
  --config-path config/agent_config.toml

# Send test message
curl -X POST http://localhost:8822/chat/message \
  -H "Content-Type: application/json" \
  -d '{"content": "@agent fetch bitcoin rate"}'
```

### Manual Testing Checklist

- [ ] Simple single-action task completes in 1 iteration
- [ ] Multi-step task (fetch + process) completes in N iterations
- [ ] Failed action triggers retry or user query
- [ ] Max iterations limit works correctly
- [ ] Context truncation doesn't lose critical info
- [ ] Intermediate responses sent when enabled
- [ ] Final response includes all results

## Migration Guide

### From Heuristic Mode

No migration needed - the new mode is **opt-in via config**:

```toml
[autonomous_agent]
enabled = true  # Set to false to use old batch mode
```

However, old batch mode code has been removed, so:
- Set `enabled = true` (recommended)
- Or use `--autonomous-enabled false` flag to disable

### Behavior Changes

| Aspect | Old | New |
|--------|-----|-----|
| Planning | Batch (all upfront) | Iterative (one at a time) |
| Adaptation | None - static plan | Dynamic based on results |
| Completion | After all actions | When LLM marks complete |
| Error Handling | Stop and report | Consult LLM for retry strategy |
| User Prompts | Based on heuristics | Based on LLM decision |

## Future Enhancements

### Planned

1. **Context Summarization**: Compress old context instead of truncating
2. **Parallel Actions**: Allow LLM to plan multiple independent actions
3. **Checkpointing**: Save state mid-task for resumption
4. **Cost Tracking**: Track LLM API costs across iterations
5. **User Intervention**: Allow user to modify plan mid-execution

### Under Consideration

1. **Sub-tasks**: Decompose complex tasks into parallel sub-tasks
2. **Plan Revision**: Allow LLM to revise earlier parts of plan
3. **Human-in-the-loop**: Optional approval gates between iterations
4. **Learning**: Remember successful action sequences for similar tasks

## Related Documentation

- `ccos-chat-gateway-autonomy-backlog.md` - Upcoming autonomy features
- `autonomy-user-guide.md` - User-facing guide to agent capabilities
- `gateway-agent-features.md` - Complete feature list
- `spec-resource-budget-enforcement.md` - Budget system details

## Code References

- Configuration: `ccos/src/config/types.rs` - `AutonomousAgentConfig`
- LLM Client: `ccos/src/chat/agent_llm.rs` - `consult_after_action()`
- Agent Runtime: `ccos/src/bin/ccos_agent.rs` - `process_with_llm()`
- Config File: `config/agent_config.toml` - `[autonomous_agent]` section
