//! Budget events for causal chain integration

use crate::budget::types::{BudgetLimits, BudgetRemaining, ExhaustionPolicy};

/// Consumption recorded for a single step/capability call
#[derive(Clone, Debug, Default)]
pub struct StepConsumption {
    /// Step identifier
    pub step_id: Option<String>,
    /// Capability that was called
    pub capability_id: Option<String>,
    /// LLM input tokens consumed
    pub llm_input_tokens: u64,
    /// LLM output tokens consumed
    pub llm_output_tokens: u64,
    /// Cost in USD
    pub cost_usd: f64,
    /// Network egress bytes
    pub network_egress_bytes: u64,
    /// Storage write bytes
    pub storage_write_bytes: u64,
    /// Wall-clock duration of this step in ms
    pub duration_ms: u64,
}

/// Budget events logged to causal chain
#[derive(Clone, Debug)]
pub enum BudgetEvent {
    /// Budget allocated at run start
    Allocation {
        run_id: String,
        limits: BudgetLimits,
    },
    /// Resources consumed by a step
    Consumption {
        step_id: String,
        capability_id: String,
        resources: StepConsumption,
        remaining: BudgetRemaining,
    },
    /// Warning threshold crossed
    Warning {
        dimension: String,
        percent: u8,
        consumed: u64,
        limit: u64,
    },
    /// Budget exhausted
    Exhausted {
        dimension: String,
        policy: ExhaustionPolicy,
        consumed: u64,
        limit: u64,
    },
    /// Budget extended after human approval
    Extended {
        dimension: String,
        additional: u64,
        approved_by: String,
        reason: Option<String>,
    },
    /// Run completed with final consumption
    RunCompleted {
        run_id: String,
        final_consumption: FinalConsumption,
    },
}

/// Final consumption summary for a run
#[derive(Clone, Debug)]
pub struct FinalConsumption {
    pub total_steps: u32,
    pub total_llm_tokens: u64,
    pub total_cost_usd: f64,
    pub total_duration_ms: u64,
    pub total_network_bytes: u64,
    pub total_storage_bytes: u64,
}

impl BudgetEvent {
    /// Create an allocation event
    pub fn allocation(run_id: impl Into<String>, limits: BudgetLimits) -> Self {
        Self::Allocation {
            run_id: run_id.into(),
            limits,
        }
    }

    /// Create a consumption event
    pub fn consumption(
        step_id: impl Into<String>,
        capability_id: impl Into<String>,
        resources: StepConsumption,
        remaining: BudgetRemaining,
    ) -> Self {
        Self::Consumption {
            step_id: step_id.into(),
            capability_id: capability_id.into(),
            resources,
            remaining,
        }
    }

    /// Create a warning event
    pub fn warning(dimension: impl Into<String>, percent: u8, consumed: u64, limit: u64) -> Self {
        Self::Warning {
            dimension: dimension.into(),
            percent,
            consumed,
            limit,
        }
    }

    /// Create an exhausted event
    pub fn exhausted(
        dimension: impl Into<String>,
        policy: ExhaustionPolicy,
        consumed: u64,
        limit: u64,
    ) -> Self {
        Self::Exhausted {
            dimension: dimension.into(),
            policy,
            consumed,
            limit,
        }
    }

    /// Create an extension event
    pub fn extended(
        dimension: impl Into<String>,
        additional: u64,
        approved_by: impl Into<String>,
        reason: Option<String>,
    ) -> Self {
        Self::Extended {
            dimension: dimension.into(),
            additional,
            approved_by: approved_by.into(),
            reason,
        }
    }
}
