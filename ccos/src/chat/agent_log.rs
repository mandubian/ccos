//! Agent LLM Consultation Logging
//!
//! This module provides types for logging agent LLM consultations to the Causal Chain.
//! Each time the agent consults the LLM (initial plan or iterative follow-up), we log:
//! - The reasoning/understanding of the current state
//! - Whether the task is complete
//! - What capabilities were planned
//! - Token usage for cost tracking
//!
//! This preserves a complete timeline of agent decision-making, separate from
//! capability executions, enabling debugging, analysis, and learning.

use serde::{Deserialize, Serialize};

/// Request to log an agent LLM consultation event.
///
/// Sent by the agent to the gateway via `/chat/agent/log` endpoint.
/// The gateway validates the internal secret and appends to the Causal Chain.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentLogRequest {
    /// Session ID this consultation belongs to
    pub session_id: String,
    
    /// Run ID within the session
    pub run_id: String,
    
    /// Step ID for this specific consultation
    pub step_id: String,
    
    /// Iteration number (1 for initial plan, 2+ for follow-ups)
    pub iteration: u32,
    
    /// Whether this is the initial consultation (first LLM call)
    pub is_initial: bool,
    
    /// Agent's understanding of the current state
    pub understanding: String,
    
    /// Agent's reasoning for next action or completion
    pub reasoning: String,
    
    /// Whether the agent considers the task complete
    pub task_complete: bool,
    
    /// Capabilities planned for execution (may be empty if task_complete)
    pub planned_capabilities: Vec<PlannedCapability>,
    
    /// Token usage for this consultation (if available)
    pub token_usage: Option<TokenUsage>,
    
    /// Model used for this consultation
    pub model: Option<String>,
}

/// A capability planned by the agent for execution.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlannedCapability {
    /// Capability ID (e.g., "ccos.network.http-fetch")
    pub capability_id: String,
    
    /// Agent's reasoning for choosing this capability
    pub reasoning: String,
    
    /// Input parameters (sanitized/redacted if sensitive)
    pub inputs: Option<serde_json::Value>,
}

/// Token usage information for cost tracking.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TokenUsage {
    /// Prompt tokens sent to LLM
    pub prompt_tokens: u64,
    
    /// Completion tokens received from LLM
    pub completion_tokens: u64,
    
    /// Total tokens (prompt + completion)
    pub total_tokens: u64,
}

/// Response from logging an agent LLM consultation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentLogResponse {
    /// Whether the log was successfully recorded
    pub success: bool,
    
    /// Action ID assigned in the Causal Chain
    pub action_id: String,
    
    /// Error message if success is false
    pub error: Option<String>,
}

impl AgentLogRequest {
    /// Create a new agent log request for an initial consultation.
    pub fn initial(
        session_id: impl Into<String>,
        run_id: impl Into<String>,
        step_id: impl Into<String>,
        understanding: impl Into<String>,
        reasoning: impl Into<String>,
    ) -> Self {
        Self {
            session_id: session_id.into(),
            run_id: run_id.into(),
            step_id: step_id.into(),
            iteration: 1,
            is_initial: true,
            understanding: understanding.into(),
            reasoning: reasoning.into(),
            task_complete: false,
            planned_capabilities: Vec::new(),
            token_usage: None,
            model: None,
        }
    }
    
    /// Create a new agent log request for a follow-up consultation.
    pub fn follow_up(
        session_id: impl Into<String>,
        run_id: impl Into<String>,
        step_id: impl Into<String>,
        iteration: u32,
        understanding: impl Into<String>,
        reasoning: impl Into<String>,
        task_complete: bool,
    ) -> Self {
        Self {
            session_id: session_id.into(),
            run_id: run_id.into(),
            step_id: step_id.into(),
            iteration,
            is_initial: false,
            understanding: understanding.into(),
            reasoning: reasoning.into(),
            task_complete,
            planned_capabilities: Vec::new(),
            token_usage: None,
            model: None,
        }
    }
    
    /// Add a planned capability to the request.
    pub fn with_planned_capability(mut self, capability: PlannedCapability) -> Self {
        self.planned_capabilities.push(capability);
        self
    }
    
    /// Set token usage for the request.
    pub fn with_token_usage(mut self, usage: TokenUsage) -> Self {
        self.token_usage = Some(usage);
        self
    }
    
    /// Set the model used for this consultation.
    pub fn with_model(mut self, model: impl Into<String>) -> Self {
        self.model = Some(model.into());
        self
    }
}

impl PlannedCapability {
    /// Create a new planned capability.
    pub fn new(
        capability_id: impl Into<String>,
        reasoning: impl Into<String>,
    ) -> Self {
        Self {
            capability_id: capability_id.into(),
            reasoning: reasoning.into(),
            inputs: None,
        }
    }
    
    /// Add inputs to the planned capability.
    pub fn with_inputs(mut self, inputs: serde_json::Value) -> Self {
        self.inputs = Some(inputs);
        self
    }
}

impl TokenUsage {
    /// Create new token usage information.
    pub fn new(prompt_tokens: u64, completion_tokens: u64) -> Self {
        Self {
            prompt_tokens,
            completion_tokens,
            total_tokens: prompt_tokens + completion_tokens,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn test_initial_request_creation() {
        let req = AgentLogRequest::initial(
            "session-123",
            "run-456",
            "step-789",
            "User wants to fetch bitcoin price",
            "Need to call http-fetch first",
        );
        
        assert_eq!(req.session_id, "session-123");
        assert_eq!(req.iteration, 1);
        assert!(req.is_initial);
        assert!(!req.task_complete);
        assert!(req.planned_capabilities.is_empty());
    }

    #[test]
    fn test_follow_up_request_creation() {
        let req = AgentLogRequest::follow_up(
            "session-123",
            "run-456",
            "step-789",
            2,
            "Have BTC price: $70,455",
            "Need to calculate 0.5 * 70455",
            false,
        );
        
        assert_eq!(req.iteration, 2);
        assert!(!req.is_initial);
        assert!(!req.task_complete);
    }

    #[test]
    fn test_request_with_capabilities() {
        let req = AgentLogRequest::initial(
            "session-123",
            "run-456",
            "step-789",
            "User wants to fetch bitcoin price",
            "Need to call http-fetch first",
        )
        .with_planned_capability(
            PlannedCapability::new("ccos.network.http-fetch", "Fetch BTC price from CoinGecko")
                .with_inputs(json!({"url": "https://api.coingecko.com/api/v3/simple/price"}))
        )
        .with_token_usage(TokenUsage::new(150, 75))
        .with_model("gpt-4");
        
        assert_eq!(req.planned_capabilities.len(), 1);
        assert_eq!(req.planned_capabilities[0].capability_id, "ccos.network.http-fetch");
        assert!(req.token_usage.is_some());
        assert_eq!(req.model, Some("gpt-4".to_string()));
    }

    #[test]
    fn test_serialization() {
        let req = AgentLogRequest::initial(
            "session-123",
            "run-456",
            "step-789",
            "Understanding",
            "Reasoning",
        )
        .with_planned_capability(PlannedCapability::new("test.cap", "test"));
        
        let json = serde_json::to_string(&req).unwrap();
        assert!(json.contains("session-123"));
        assert!(json.contains("test.cap"));
        
        let deserialized: AgentLogRequest = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized.session_id, req.session_id);
    }
}
