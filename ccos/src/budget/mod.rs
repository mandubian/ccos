//! CCOS Resource Budget Enforcement
//!
//! This module provides runtime budget enforcement for agent runs, ensuring
//! every execution is controllable end-to-end. Budget exhaustion triggers
//! stop or approval flow, never silent continuation.
//!
//! # Architecture
//!
//! ```text
//! ┌─────────────────────────────────────────────────────────────┐
//! │                    Agent Run                                 │
//! │  ┌─────────────────────────────────────────────────────────┐│
//! │  │                  BudgetContext                          ││
//! │  │  • Immutable limits (steps, time, tokens, cost)         ││
//! │  │  • Mutable consumption tracking                          ││
//! │  │  • Per-dimension exhaustion policies                     ││
//! │  └─────────────────────────────────────────────────────────┘│
//! │                           │                                   │
//! │         ┌─────────────────┼─────────────────┐                │
//! │         ▼                 ▼                 ▼                │
//! │  ┌───────────┐     ┌───────────┐     ┌───────────┐          │
//! │  │  Step 1   │     │  Step 2   │     │  Step N   │          │
//! │  │ pre-check │     │ pre-check │     │ pre-check │          │
//! │  │  meter    │     │  meter    │     │  meter    │          │
//! │  └───────────┘     └───────────┘     └───────────┘          │
//! └─────────────────────────────────────────────────────────────┘
//! ```
//!
//! # Usage
//!
//! ```rust,ignore
//! use ccos::budget::{BudgetContext, BudgetLimits, BudgetPolicies};
//!
//! // Create budget for a run
//! let limits = BudgetLimits::default();
//! let policies = BudgetPolicies::default();
//! let mut ctx = BudgetContext::new(limits, policies);
//!
//! // Before each capability call
//! match ctx.check() {
//!     BudgetCheckResult::Ok => { /* proceed */ }
//!     BudgetCheckResult::Warning { dimension, percent } => {
//!         log::warn!("Budget {}: {}% consumed", dimension, percent);
//!     }
//!     BudgetCheckResult::Exhausted { dimension, policy } => {
//!         match policy {
//!             ExhaustionPolicy::HardStop => return Err(BudgetExhausted),
//!             ExhaustionPolicy::ApprovalRequired => return Err(NeedsApproval),
//!             ExhaustionPolicy::SoftWarn => { /* log and continue */ }
//!         }
//!     }
//! }
//!
//! // After capability returns
//! ctx.record_step(StepConsumption {
//!     llm_input_tokens: 500,
//!     llm_output_tokens: 200,
//!     cost_usd: 0.001,
//! });
//! ```

mod context;
mod events;
mod types;

pub use context::BudgetContext;
pub use events::{BudgetEvent, StepConsumption};
pub use types::{
    BudgetCheckResult, BudgetConsumed, BudgetExhausted, BudgetLimits, BudgetPolicies,
    BudgetRemaining, ExhaustionPolicy,
};

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_budget_context_basic() {
        let limits = BudgetLimits::default();
        let policies = BudgetPolicies::default();
        let mut ctx = BudgetContext::new(limits, policies);

        // Initial check should be Ok
        assert!(matches!(ctx.check(), BudgetCheckResult::Ok));

        // Record a step
        ctx.record_step(StepConsumption::default());

        // Should still be Ok (1 of 50 steps)
        assert!(matches!(ctx.check(), BudgetCheckResult::Ok));
    }

    #[test]
    fn test_budget_exhaustion() {
        let limits = BudgetLimits {
            steps: 2,
            ..Default::default()
        };
        let policies = BudgetPolicies::default();
        let mut ctx = BudgetContext::new(limits, policies);

        // Use up steps
        ctx.record_step(StepConsumption::default());
        ctx.record_step(StepConsumption::default());

        // Should be exhausted
        match ctx.check() {
            BudgetCheckResult::Exhausted { dimension, .. } => {
                assert_eq!(dimension, "steps");
            }
            _ => panic!("Expected exhaustion"),
        }
    }

    #[test]
    fn test_budget_warning_threshold() {
        let limits = BudgetLimits {
            steps: 10,
            ..Default::default()
        };
        let policies = BudgetPolicies::default();
        let mut ctx = BudgetContext::new(limits, policies);

        // Use 5 steps (50%)
        for _ in 0..5 {
            ctx.record_step(StepConsumption::default());
        }

        // Should trigger 50% warning
        match ctx.check() {
            BudgetCheckResult::Warning { dimension, percent } => {
                assert_eq!(dimension, "steps");
                assert_eq!(percent, 50);
            }
            _ => panic!("Expected warning at 50%"),
        }
    }
}
