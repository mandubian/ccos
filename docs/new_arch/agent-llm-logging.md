# Agent LLM Consultation Logging

**Status:** Implemented  
**Date:** 2026-02-10  
**Component:** ccos-agent, ccos-chat-gateway

## Overview

This document describes the implementation of logging agent LLM consultations to the Causal Chain. This feature preserves a complete timeline of agent decision-making, separate from capability executions.

### Problem Statement

Previously, capability executions were logged to Causal Chain, but LLM/agent interactions (prompts, responses, reasoning, decisions) were NOT logged. This limited:
- **Debugging**: Hard to understand why the agent made certain decisions
- **Analysis**: No way to analyze decision patterns over time
- **Cost tracking**: Token usage was not tracked per iteration
- **Audit trails**: Incomplete record of agent behavior

### Solution

A dedicated `/chat/agent/log` endpoint that:
1. Accepts LLM consultation data from the agent
2. Logs it to Causal Chain with `ActionType::AgentLlmConsultation`
3. Returns a unique `action_id` for correlation

## Architecture

```
┌─────────┐     ┌─────────┐     ┌───────────────┐     ┌─────────────┐
│  Agent  │────►│   LLM   │────►│    Gateway    │────►│Causal Chain │
└─────────┘     └─────────┘     └───────────────┘     └─────────────┘
     │                               │                       │
     │                               │                       │
     └─────── POST /chat/agent/log ──┴───────────────────────┘
               (AgentLogRequest)           (ActionType::AgentLlmConsultation)
```

### Timeline Preservation

The dedicated endpoint preserves a clean timeline of events:

```
Time  │ Event
──────┼────────────────────────────────────────
T+0   │ AgentLlmConsultation (iteration 1)
T+1   │ CapabilityCall (ccos.network.http-fetch)
T+2   │ CapabilityResult (success)
T+3   │ AgentLlmConsultation (iteration 2)
T+4   │ CapabilityCall (ccos.code.refined_execute)
T+5   │ CapabilityResult (success)
T+6   │ AgentLlmConsultation (iteration 3, task_complete=true)
```

## Implementation Details

### ActionType Enum Extension

**File:** `ccos/src/types.rs`

```rust
pub enum ActionType {
    // ... existing variants ...
    
    /// Agent consulted LLM for iterative planning decision
    AgentLlmConsultation,
}
```

### Request/Response Types

**File:** `ccos/src/chat/agent_log.rs`

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentLogRequest {
    pub session_id: String,
    pub run_id: String,
    pub step_id: String,
    pub iteration: u32,
    pub is_initial: bool,
    pub understanding: String,
    pub reasoning: String,
    pub task_complete: bool,
    pub planned_capabilities: Vec<PlannedCapability>,
    pub token_usage: Option<TokenUsage>,
    pub model: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlannedCapability {
    pub capability_id: String,
    pub reasoning: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TokenUsage {
    pub prompt_tokens: u64,
    pub completion_tokens: u64,
    pub total_tokens: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentLogResponse {
    pub success: bool,
    pub action_id: String,
    pub error: Option<String>,
}
```

### Gateway Endpoint

**File:** `ccos/src/chat/gateway.rs`

```rust
// Route registration
.route("/chat/agent/log", post(agent_log_handler))

// Handler
async fn agent_log_handler(
    State(state): State<Arc<GatewayState>>,
    headers: axum::http::HeaderMap,
    Json(payload): Json<AgentLogRequest>,
) -> Result<Json<AgentLogResponse>, StatusCode> {
    // 1. Validate X-Agent-Token header
    // 2. Validate session ownership
    // 3. Log to Causal Chain with ActionType::AgentLlmConsultation
    // 4. Return action_id
}
```

### Agent Integration

**File:** `ccos/src/bin/ccos_agent.rs`

```rust
async fn log_llm_consultation(
    &self,
    plan: &IterativeAgentPlan,
    iteration: u32,
    is_initial: bool,
    token_usage: Option<TokenUsage>,
) -> anyhow::Result<String> {
    let request = AgentLogRequest {
        session_id: self.session_id.clone(),
        run_id: self.run_id.clone(),
        step_id: format!("llm-consultation-{}", iteration),
        iteration,
        is_initial,
        understanding: plan.understanding.clone(),
        reasoning: plan.reasoning.clone(),
        task_complete: plan.task_complete,
        planned_capabilities: plan.actions.iter().map(|a| PlannedCapability {
            capability_id: a.capability_id.clone(),
            reasoning: a.reasoning.clone(),
        }).collect(),
        token_usage,
        model: self.llm_client.model_name(),
    };
    
    // POST to gateway
    let response = self.http_client
        .post(&format!("{}/chat/agent/log", self.gateway_url))
        .header("X-Agent-Token", &self.auth_token)
        .json(&request)
        .send()
        .await?;
    
    Ok(response.action_id)
}
```

## Usage

### In process_with_llm()

```rust
async fn process_with_llm(&mut self, event: ChatEvent) -> anyhow::Result<()> {
    let mut iteration = 0;
    
    loop {
        iteration += 1;
        
        let plan = if iteration == 1 {
            self.llm_client.process_message(...).await
        } else {
            self.llm_client.consult_after_action(...).await
        };
        
        // Log the LLM consultation
        if let Err(e) = self.log_llm_consultation(&plan, iteration, iteration == 1, None).await {
            log::warn!("Failed to log LLM consultation: {}", e);
        }
        
        if plan.task_complete {
            break;
        }
        
        // Execute action...
    }
}
```

## Causal Chain Query

To retrieve all LLM consultations for a run:

```rust
let query = CausalQuery {
    run_id: Some(run_id.clone()),
    action_type: Some(ActionType::AgentLlmConsultation),
    ..Default::default()
};
let consultations = chain.query_actions(&query);
```

## Benefits

1. **Complete Audit Trail**: Every LLM decision is recorded with reasoning
2. **Debugging Support**: Understand why agent took certain actions
3. **Cost Tracking**: Token usage per iteration enables cost analysis
4. **Pattern Analysis**: Identify common decision patterns across sessions
5. **Timeline Integrity**: Separate events preserve temporal ordering

## Security

- Endpoint requires `X-Agent-Token` header
- Token validated against session registry
- Only the session owner can log consultations for their session

## Future Enhancements

1. **Prompt Logging**: Optionally log the actual prompt sent to LLM (privacy considerations)
2. **Response Logging**: Store full LLM response for replay
3. **Summarization**: Compress old consultations to save space
4. **Real-time Streaming**: Stream consultation events to WebSocket clients

## Related Documentation

- [Iterative LLM Consultation](./iterative-llm-consultation.md) - Main feature documentation
- [Causal Chain](./spec-causal-chain.md) - Causal Chain architecture
- [Gateway Agent Features](./gateway-agent-features.md) - Complete gateway feature list
