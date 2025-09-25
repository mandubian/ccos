//! CCOS Governance Kernel
//!
//! This module defines the Governance Kernel, the high-privilege, secure component
//! responsible for enforcing the system's `Constitution`. It acts as the mandatory
//! intermediary between the low-privilege Arbiter and the Orchestrator.
//!
//! The Kernel's primary responsibilities include:
//! - Validating proposed plans against the Constitution.
//! - Sanitizing intents and scaffolding plans for safety.
//! - Verifying capability attestations.
//! - Logging all decisions and actions to the Causal Chain.

use std::sync::{Arc, Mutex};

use crate::runtime::error::RuntimeResult;
use crate::runtime::security::RuntimeContext;

use super::orchestrator::Orchestrator;

use super::intent_graph::IntentGraph;
use super::types::Intent; // for delegation validation
use super::types::{ExecutionResult, Plan, PlanBody, StorableIntent};
use crate::runtime::error::RuntimeError;

/// Represents the system's constitution, a set of human-authored rules.
// TODO: This should be loaded from a secure, signed configuration file.
pub struct Constitution {
    rules: Vec<String>,
}

impl Default for Constitution {
    fn default() -> Self {
        Self { rules: vec![] }
    }
}

/// The Governance Kernel is the root of trust in the CCOS.
/// Its logic is designed to be simple, verifiable, and secure.
pub struct GovernanceKernel {
    orchestrator: Arc<Orchestrator>,
    intent_graph: Arc<Mutex<IntentGraph>>,
    constitution: Constitution,
}

impl GovernanceKernel {
    /// Creates a new Governance Kernel.
    pub fn new(orchestrator: Arc<Orchestrator>, intent_graph: Arc<Mutex<IntentGraph>>) -> Self {
        Self {
            orchestrator,
            intent_graph,
            constitution: Constitution::default(),
        }
    }

    /// The primary entry point for processing a plan from the Arbiter.
    /// It validates the plan and, if successful, passes it to the Orchestrator.
    pub async fn validate_and_execute(
        &self,
        plan: Plan,
        context: &RuntimeContext,
    ) -> RuntimeResult<ExecutionResult> {
        // --- 1. Intent Sanitization (SEP-012) ---
        let intent = self.get_intent(&plan)?;
        self.sanitize_intent(&intent, &plan)?;

        // --- 2. Plan Scaffolding (SEP-012) ---
        let safe_plan = self.scaffold_plan(plan)?;

        // --- 3. Constitution Validation (SEP-010) ---
        self.validate_against_constitution(&safe_plan)?;

        // --- 4. Attestation Verification (SEP-011) ---
        // TODO: Verify the cryptographic attestations of all capabilities
        // called within the plan.

        // --- 5. Execution ---
        // If all checks pass, delegate execution to the Orchestrator.
        self.orchestrator.execute_plan(&safe_plan, context).await
    }

    /// Retrieves the primary intent associated with the plan.
    fn get_intent(&self, plan: &Plan) -> RuntimeResult<StorableIntent> {
        let intent_id = plan
            .intent_ids
            .first()
            .ok_or_else(|| RuntimeError::Generic("Plan has no associated intent".to_string()))?;

        let graph = self
            .intent_graph
            .lock()
            .map_err(|_| RuntimeError::Generic("Failed to lock IntentGraph".to_string()))?;

        graph
            .get_intent(intent_id)
            .ok_or_else(|| RuntimeError::Generic(format!("Intent not found: {}", intent_id)))
    }

    /// Checks the plan and its originating intent for malicious patterns.
    fn sanitize_intent(&self, intent: &StorableIntent, plan: &Plan) -> RuntimeResult<()> {
        // Check for common prompt injection phrases in the original request.
        let lower_request = intent.original_request.to_lowercase();
        const INJECTION_PHRASES: &[&str] = &[
            "ignore all previous instructions",
            "you are now in developer mode",
        ];
        for phrase in INJECTION_PHRASES {
            if lower_request.contains(phrase) {
                return Err(RuntimeError::Generic(
                    "Potential prompt injection detected".to_string(),
                ));
            }
        }

        // Check for logical inconsistencies between the intent and the plan.
        // Example: If intent is to send an email, the plan shouldn't be deleting files.
        if intent.goal.contains("email") {
            if let PlanBody::Rtfs(body_text) = &plan.body {
                if body_text.contains("delete-file") {
                    return Err(RuntimeError::Generic(
                        "Plan action contradicts intent goal".to_string(),
                    ));
                }
            }
        }

        Ok(())
    }

    /// Wraps the plan's body in a safety harness.
    fn scaffold_plan(&self, mut plan: Plan) -> RuntimeResult<Plan> {
        // Extract the original body text
        let original_body = match &plan.body {
            PlanBody::Rtfs(text) => text.clone(),
            PlanBody::Wasm(_) => {
                return Err(RuntimeError::Generic(
                    "Cannot scaffold binary plan body".to_string(),
                ))
            }
        };

        // Wrap the original body in a `(do ...)` block if it isn't already.
        let wrapped_body = if original_body.trim().starts_with("(") {
            original_body
        } else {
            format!("(do {})", original_body)
        };

        // NOTE: Previously we injected unimplemented forms like `(with-resource-limits ...)` and `(on-failure ...)`.
        // Those forms are not yet supported by the parser/runtime, causing execution failures.
        // For now, keep the plan safely wrapped with `do` only.
        plan.body = PlanBody::Rtfs(wrapped_body);
        Ok(plan)
    }

    /// Validates the plan against the rules of the system's Constitution.
    fn validate_against_constitution(&self, plan: &Plan) -> RuntimeResult<()> {
        // TODO: Implement actual validation logic based on loaded constitutional rules.
        // For now, this is a placeholder.
        if let PlanBody::Rtfs(body_text) = &plan.body {
            if body_text.contains("launch-nukes") {
                return Err(RuntimeError::Generic(
                    "Plan violates Constitution: Rule against global thermonuclear war."
                        .to_string(),
                ));
            }
        }
        Ok(())
    }

    /// Delegation validation hook (M4): governance pre-approval of agent selection.
    /// Extend with constitutional / policy checks (e.g., trust tier allowlist, cost ceilings, jurisdiction constraints).
    /// Return Err(...) to veto delegation (arbiter will fall back to LLM planning path).
    pub fn validate_delegation(
        &self,
        intent: &Intent,
        agent_id: &str,
        score: f64,
    ) -> RuntimeResult<()> {
        // Placeholder policy examples (expand as specs evolve):
        // 1. Reject extremely low scores (defense in depth even if arbiter threshold handles it).
        if score < 0.50 {
            return Err(crate::runtime::error::RuntimeError::Generic(format!(
                "Delegation rejected: score {:.2} below governance floor for agent {}",
                score, agent_id
            )));
        }
        // 2. Enforce simple constraint: if intent goal mentions "EU" ensure agent id does not contain "non_eu" (placeholder heuristic).
        let goal_lower = intent.goal.to_lowercase();
        if goal_lower.contains("eu") && agent_id.contains("non_eu") {
            return Err(crate::runtime::error::RuntimeError::Generic(
                "Delegation rejected: agent jurisdiction mismatch (EU constraint)".to_string(),
            ));
        }
        Ok(())
    }
}
