use std::collections::HashMap;
use crate::arbiter::llm_provider::LlmProvider;
use crate::types::{Plan, PlanBody};
use rtfs::runtime::error::{RuntimeError, RuntimeResult};
use serde::{Deserialize, Serialize};

/// Result of a semantic judgment
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Judgment {
    pub allowed: bool,
    pub reasoning: String,
    pub risk_score: f64,
}

pub struct PlanJudge {
    // We don't store the provider here because it's managed by the GovernanceKernel's DelegatingArbiter
}

impl PlanJudge {
    pub fn new() -> Self {
        Self {}
    }

    pub async fn judge_plan(
        &self,
        provider: &dyn LlmProvider,
        goal: &str,
        plan: &Plan,
        resolutions: &HashMap<String, String>,
    ) -> RuntimeResult<Judgment> {
        let plan_content = match &plan.body {
            PlanBody::Rtfs(code) => code,
            PlanBody::Wasm(_) => return Ok(Judgment {
                allowed: true,
                reasoning: "WASM plans are currently skipped by semantic judge".to_string(),
                risk_score: 0.0,
            }),
        };

        let resolutions_str = resolutions.iter()
            .map(|(k, v)| format!("- {} -> {}", k, v))
            .collect::<Vec<_>>()
            .join("\n");

        let prompt = format!(
            r#"You are the CCOS Semantic Plan Judge. Your task is to evaluate if a proposed execution plan is semantically sound and safe given the user's goal.

USER GOAL:
"{}"

PROPOSED PLAN (RTFS):
```clojure
{}
```

CAPABILITY RESOLUTIONS:
{}

EVALUATION CRITERIA:
1. GOAL ALIGNMENT: Does the plan actually achieve the stated goal?
2. SEMANTIC SAFETY: Are the tools appropriate for the action? (e.g., flagging "delete" goals mapped to "read-only" tools).
3. HALLUCINATION CHECK: Does the plan invent parameters or steps that don't make sense for the resolved capabilities?

Respond ONLY with a JSON object in the following format:
{{
  "allowed": boolean,
  "reasoning": "detailed explanation of your decision",
  "risk_score": float (0.0 to 1.0)
}}
"#,
            goal, plan_content, resolutions_str
        );

        let response = provider.generate_text(&prompt).await?;
        
        // Try to parse JSON from response
        let judgment: Judgment = serde_json::from_str(&response)
            .map_err(|e| RuntimeError::Generic(format!("Failed to parse judge response: {}. Response was: {}", e, response)))?;

        Ok(judgment)
    }
}
