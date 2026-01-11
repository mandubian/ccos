use crate::arbiter::llm_provider::LlmProvider;
use crate::ccos_eprintln;
use crate::types::{Plan, PlanBody};
use rtfs::runtime::error::{RuntimeError, RuntimeResult};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Result of a semantic judgment
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Judgment {
    pub allowed: bool,
    pub reasoning: String,
    pub risk_score: f64,
}

pub struct PlanJudge {
    // We don't store the provider here because it's managed by the GovernanceKernel's DelegatingCognitiveEngine
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
        ccos_eprintln!(
            "   ⚖️  [SemanticJudge] Evaluating plan against goal: \"{}\"",
            goal
        );

        let plan_content = match &plan.body {
            PlanBody::Rtfs(code) => code,
            PlanBody::Wasm(_) | PlanBody::Source(_) | PlanBody::Binary(_) => {
                ccos_eprintln!("   ⚖️  [SemanticJudge] Non-RTFS plans are skipped (auto-allowed)");
                return Ok(Judgment {
                    allowed: true,
                    reasoning: "Non-RTFS plans are currently skipped by semantic judge".to_string(),
                    risk_score: 0.0,
                });
            }
        };

        let resolutions_str = resolutions
            .iter()
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

        ccos_eprintln!("   ⚖️  [SemanticJudge] Consulting LLM for semantic judgment...");
        let response = provider.generate_text(&prompt).await?;

        // Strip markdown code fences if present (LLMs sometimes wrap JSON in ```json ... ```)
        let cleaned_response = Self::strip_markdown_code_fences(&response);

        // Try to parse JSON from response
        let judgment: Judgment = serde_json::from_str(&cleaned_response).map_err(|e| {
            RuntimeError::Generic(format!(
                "Failed to parse judge response: {}. Response was: {}",
                e, response
            ))
        })?;

        if judgment.allowed && judgment.risk_score <= 0.7 {
            ccos_eprintln!(
                "   ✅ [SemanticJudge] Plan APPROVED (risk: {:.2})",
                judgment.risk_score
            );
        } else {
            ccos_eprintln!(
                "   🛑 [SemanticJudge] Plan REJECTED (risk: {:.2}): {}",
                judgment.risk_score,
                judgment.reasoning
            );
        }

        Ok(judgment)
    }

    /// Strips markdown code fences from LLM responses.
    /// LLMs sometimes wrap JSON in ```json ... ``` blocks.
    fn strip_markdown_code_fences(response: &str) -> String {
        let trimmed = response.trim();

        // Try to find JSON content between code fences
        if trimmed.starts_with("```") {
            // Find the end of the opening fence (first newline after ```)
            if let Some(start_idx) = trimmed.find('\n') {
                let after_fence = &trimmed[start_idx + 1..];
                // Find the closing fence
                if let Some(end_idx) = after_fence.rfind("```") {
                    return after_fence[..end_idx].trim().to_string();
                }
            }
        }

        // No code fences found, return as-is
        trimmed.to_string()
    }
}
