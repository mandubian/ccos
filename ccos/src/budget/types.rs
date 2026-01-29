use serde::{Deserialize, Serialize};

/// Immutable budget limits for a run
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct BudgetLimits {
    /// Maximum number of capability calls
    pub steps: u32,
    /// Maximum wall-clock time in milliseconds
    pub wall_clock_ms: u64,
    /// Maximum LLM tokens (input + output)
    pub llm_tokens: u64,
    /// Maximum cost in USD
    pub cost_usd: f64,
    /// Maximum network egress bytes
    pub network_egress_bytes: u64,
    /// Maximum storage write bytes
    pub storage_write_bytes: u64,
}

impl BudgetLimits {
    /// Clamp this budget to a parent budget (inheritance enforcement).
    pub fn clamp_to(&self, parent: &BudgetLimits) -> BudgetLimits {
        BudgetLimits {
            steps: self.steps.min(parent.steps),
            wall_clock_ms: self.wall_clock_ms.min(parent.wall_clock_ms),
            llm_tokens: self.llm_tokens.min(parent.llm_tokens),
            cost_usd: self.cost_usd.min(parent.cost_usd),
            network_egress_bytes: self
                .network_egress_bytes
                .min(parent.network_egress_bytes),
            storage_write_bytes: self
                .storage_write_bytes
                .min(parent.storage_write_bytes),
        }
    }
}

impl Default for BudgetLimits {
    fn default() -> Self {
        Self {
            steps: 50,
            wall_clock_ms: 60_000, // 60 seconds
            llm_tokens: 100_000,
            cost_usd: 0.50,
            network_egress_bytes: 10 * 1024 * 1024, // 10 MB
            storage_write_bytes: 50 * 1024 * 1024,  // 50 MB
        }
    }
}

/// Policy for when budget is exhausted
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum ExhaustionPolicy {
    /// Run ends immediately as Failed
    HardStop,
    /// Run enters Paused state, waits for human approval
    ApprovalRequired,
    /// Warning logged, run continues (monitoring only)
    SoftWarn,
}

impl Default for ExhaustionPolicy {
    fn default() -> Self {
        Self::HardStop
    }
}

/// Per-dimension exhaustion policies
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct BudgetPolicies {
    pub steps: ExhaustionPolicy,
    pub wall_clock: ExhaustionPolicy,
    pub llm_tokens: ExhaustionPolicy,
    pub cost_usd: ExhaustionPolicy,
    pub network_egress: ExhaustionPolicy,
    pub storage_write: ExhaustionPolicy,
}

impl Default for BudgetPolicies {
    fn default() -> Self {
        Self {
            steps: ExhaustionPolicy::HardStop,
            wall_clock: ExhaustionPolicy::HardStop,
            llm_tokens: ExhaustionPolicy::ApprovalRequired,
            cost_usd: ExhaustionPolicy::HardStop,
            network_egress: ExhaustionPolicy::HardStop,
            storage_write: ExhaustionPolicy::HardStop,
        }
    }
}

/// Mutable consumption state
#[derive(Clone, Debug, Default)]
pub struct BudgetConsumed {
    pub steps: u32,
    pub llm_input_tokens: u64,
    pub llm_output_tokens: u64,
    pub cost_usd: f64,
    pub network_egress_bytes: u64,
    pub storage_write_bytes: u64,
}

impl BudgetConsumed {
    /// Total LLM tokens consumed
    pub fn total_llm_tokens(&self) -> u64 {
        self.llm_input_tokens + self.llm_output_tokens
    }
}

/// Remaining budget
#[derive(Clone, Debug)]
pub struct BudgetRemaining {
    pub steps: u32,
    pub wall_clock_ms: u64,
    pub llm_tokens: u64,
    pub cost_usd: f64,
    pub network_egress_bytes: u64,
    pub storage_write_bytes: u64,
}

/// Result of budget check
#[derive(Clone, Debug)]
pub enum BudgetCheckResult {
    /// Budget is within limits
    Ok,
    /// Budget is at warning threshold
    Warning { dimension: String, percent: u8 },
    /// Budget is exhausted
    Exhausted {
        dimension: String,
        policy: ExhaustionPolicy,
    },
}

/// Error when budget is exhausted
#[derive(Clone, Debug)]
pub enum BudgetExhausted {
    /// Run must stop immediately
    HardStop {
        dimension: String,
        consumed: u64,
        limit: u64,
    },
    /// Run needs human approval to continue
    NeedsApproval {
        dimension: String,
        consumed: u64,
        limit: u64,
    },
}

impl std::fmt::Display for BudgetExhausted {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::HardStop {
                dimension,
                consumed,
                limit,
            } => write!(
                f,
                "Budget exhausted (hard stop): {} consumed {}/{} ",
                dimension, consumed, limit
            ),
            Self::NeedsApproval {
                dimension,
                consumed,
                limit,
            } => write!(
                f,
                "Budget exhausted (needs approval): {} consumed {}/{}",
                dimension, consumed, limit
            ),
        }
    }
}

impl std::error::Error for BudgetExhausted {}

/// Warning types that have been issued
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum BudgetWarning {
    Steps50,
    Steps80,
    WallClock50,
    WallClock80,
    LlmTokens50,
    LlmTokens80,
    Cost50,
    Cost80,
    Network50,
    Network80,
    Storage50,
    Storage80,
}
