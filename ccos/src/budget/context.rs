//! Budget context - runtime state for budget tracking

use std::collections::HashSet;
use std::time::Instant;

use crate::budget::events::StepConsumption;
use crate::budget::types::{
    BudgetCheckResult, BudgetConsumed, BudgetLimits, BudgetPolicies, BudgetRemaining,
    BudgetWarning, ExhaustionPolicy,
};
use crate::sandbox::resources::ResourceMetrics;

/// Runtime budget context for an agent run
#[derive(Debug)]
pub struct BudgetContext {
    /// Immutable limits for this run
    limits: BudgetLimits,
    /// Per-dimension enforcement policies
    policies: BudgetPolicies,
    /// Mutable consumption state
    consumed: BudgetConsumed,
    /// Run start time for wall-clock tracking
    start_time: Instant,
    /// Warnings already issued (to avoid duplicates)
    warnings_issued: HashSet<BudgetWarning>,
}

impl BudgetContext {
    /// Create a new budget context with given limits and policies
    pub fn new(limits: BudgetLimits, policies: BudgetPolicies) -> Self {
        Self {
            limits,
            policies,
            consumed: BudgetConsumed::default(),
            start_time: Instant::now(),
            warnings_issued: HashSet::new(),
        }
    }

    /// Create with default limits and policies
    pub fn with_defaults() -> Self {
        Self::new(BudgetLimits::default(), BudgetPolicies::default())
    }

    /// Get current consumption
    pub fn consumed(&self) -> &BudgetConsumed {
        &self.consumed
    }

    /// Get limits
    pub fn limits(&self) -> &BudgetLimits {
        &self.limits
    }

    /// Calculate remaining budget
    pub fn remaining(&self) -> BudgetRemaining {
        let elapsed_ms = self.start_time.elapsed().as_millis() as u64;
        BudgetRemaining {
            steps: self.limits.steps.saturating_sub(self.consumed.steps),
            wall_clock_ms: self.limits.wall_clock_ms.saturating_sub(elapsed_ms),
            llm_tokens: self
                .limits
                .llm_tokens
                .saturating_sub(self.consumed.total_llm_tokens()),
            cost_usd: (self.limits.cost_usd - self.consumed.cost_usd).max(0.0),
            network_egress_bytes: self
                .limits
                .network_egress_bytes
                .saturating_sub(self.consumed.network_egress_bytes),
            storage_write_bytes: self
                .limits
                .storage_write_bytes
                .saturating_sub(self.consumed.storage_write_bytes),
            sandbox_cpu_ms: self
                .limits
                .sandbox_cpu_ms
                .saturating_sub(self.consumed.sandbox_cpu_ms),
            sandbox_memory_peak_mb: self
                .limits
                .sandbox_memory_peak_mb
                .saturating_sub(self.consumed.sandbox_memory_peak_mb),
        }
    }

    /// Check budget status before a capability call
    ///
    /// Returns Ok if budget available, Warning if at threshold, Exhausted if over limit
    pub fn check(&mut self) -> BudgetCheckResult {
        let elapsed_ms = self.start_time.elapsed().as_millis() as u64;

        // Check each dimension in order of priority
        // 1. Steps
        if let Some(result) = self.check_dimension(
            "steps",
            self.consumed.steps as u64,
            self.limits.steps as u64,
            self.policies.steps,
            BudgetWarning::Steps50,
            BudgetWarning::Steps80,
        ) {
            return result;
        }

        // 2. Wall clock
        if let Some(result) = self.check_dimension(
            "wall_clock",
            elapsed_ms,
            self.limits.wall_clock_ms,
            self.policies.wall_clock,
            BudgetWarning::WallClock50,
            BudgetWarning::WallClock80,
        ) {
            return result;
        }

        // 3. LLM tokens
        if let Some(result) = self.check_dimension(
            "llm_tokens",
            self.consumed.total_llm_tokens(),
            self.limits.llm_tokens,
            self.policies.llm_tokens,
            BudgetWarning::LlmTokens50,
            BudgetWarning::LlmTokens80,
        ) {
            return result;
        }

        // 4. Cost
        let cost_consumed = (self.consumed.cost_usd * 1000.0) as u64;
        let cost_limit = (self.limits.cost_usd * 1000.0) as u64;
        if let Some(result) = self.check_dimension(
            "cost_usd",
            cost_consumed,
            cost_limit,
            self.policies.cost_usd,
            BudgetWarning::Cost50,
            BudgetWarning::Cost80,
        ) {
            return result;
        }

        // 5. Network egress
        if let Some(result) = self.check_dimension(
            "network_egress",
            self.consumed.network_egress_bytes,
            self.limits.network_egress_bytes,
            self.policies.network_egress,
            BudgetWarning::Network50,
            BudgetWarning::Network80,
        ) {
            return result;
        }

        // 6. Storage writes
        if let Some(result) = self.check_dimension(
            "storage_write",
            self.consumed.storage_write_bytes,
            self.limits.storage_write_bytes,
            self.policies.storage_write,
            BudgetWarning::Storage50,
            BudgetWarning::Storage80,
        ) {
            return result;
        }

        // 7. Sandbox CPU time
        if let Some(result) = self.check_dimension(
            "sandbox_cpu_ms",
            self.consumed.sandbox_cpu_ms,
            self.limits.sandbox_cpu_ms,
            self.policies.sandbox_cpu,
            BudgetWarning::SandboxCpu50,
            BudgetWarning::SandboxCpu80,
        ) {
            return result;
        }

        // 8. Sandbox memory peak
        if let Some(result) = self.check_dimension(
            "sandbox_memory_peak_mb",
            self.consumed.sandbox_memory_peak_mb,
            self.limits.sandbox_memory_peak_mb,
            self.policies.sandbox_memory,
            BudgetWarning::SandboxMemory50,
            BudgetWarning::SandboxMemory80,
        ) {
            return result;
        }

        BudgetCheckResult::Ok
    }

    /// Check a single dimension, returning Some if warning or exhausted
    fn check_dimension(
        &mut self,
        name: &str,
        consumed: u64,
        limit: u64,
        policy: ExhaustionPolicy,
        warn_50: BudgetWarning,
        warn_80: BudgetWarning,
    ) -> Option<BudgetCheckResult> {
        if limit == 0 {
            return None; // Unlimited
        }

        let percent = ((consumed as f64 / limit as f64) * 100.0) as u8;

        // Check exhaustion (>= 100%)
        if consumed >= limit {
            return Some(BudgetCheckResult::Exhausted {
                dimension: name.to_string(),
                policy,
            });
        }

        // Check 80% warning
        if percent >= 80 && !self.warnings_issued.contains(&warn_80) {
            self.warnings_issued.insert(warn_80);
            return Some(BudgetCheckResult::Warning {
                dimension: name.to_string(),
                percent: 80,
            });
        }

        // Check 50% warning
        if percent >= 50 && !self.warnings_issued.contains(&warn_50) {
            self.warnings_issued.insert(warn_50);
            return Some(BudgetCheckResult::Warning {
                dimension: name.to_string(),
                percent: 50,
            });
        }

        None
    }

    /// Get consumed and limit values for a dimension name
    pub fn consumed_and_limit_for(&self, dimension: &str) -> Option<(u64, u64)> {
        match dimension {
            "steps" => Some((self.consumed.steps as u64, self.limits.steps as u64)),
            "wall_clock" | "wall_clock_ms" => {
                let elapsed_ms = self.start_time.elapsed().as_millis() as u64;
                Some((elapsed_ms, self.limits.wall_clock_ms))
            }
            "llm_tokens" => Some((
                self.consumed.total_llm_tokens(),
                self.limits.llm_tokens,
            )),
            "cost_usd" => Some((
                (self.consumed.cost_usd * 1000.0) as u64,
                (self.limits.cost_usd * 1000.0) as u64,
            )),
            "network_egress" | "network_egress_bytes" => Some((
                self.consumed.network_egress_bytes,
                self.limits.network_egress_bytes,
            )),
            "storage_write" | "storage_write_bytes" => Some((
                self.consumed.storage_write_bytes,
                self.limits.storage_write_bytes,
            )),
            "sandbox_cpu" | "sandbox_cpu_ms" => {
                Some((self.consumed.sandbox_cpu_ms, self.limits.sandbox_cpu_ms))
            }
            "sandbox_memory" | "sandbox_memory_peak_mb" => Some((
                self.consumed.sandbox_memory_peak_mb,
                self.limits.sandbox_memory_peak_mb,
            )),
            _ => None,
        }
    }

    /// Record consumption from a capability call
    pub fn record_step(&mut self, consumption: StepConsumption) {
        self.consumed.steps += 1;
        self.consumed.llm_input_tokens += consumption.llm_input_tokens;
        self.consumed.llm_output_tokens += consumption.llm_output_tokens;
        self.consumed.cost_usd += consumption.cost_usd;
        self.consumed.network_egress_bytes += consumption.network_egress_bytes;
        self.consumed.storage_write_bytes += consumption.storage_write_bytes;
    }

    /// Record sandbox resource consumption in the budget.
    pub fn record_sandbox_consumption(&mut self, metrics: &ResourceMetrics) {
        self.consumed.sandbox_cpu_ms += metrics.cpu_time_ms;
        if metrics.memory_peak_mb > self.consumed.sandbox_memory_peak_mb {
            self.consumed.sandbox_memory_peak_mb = metrics.memory_peak_mb;
        }
    }

    /// Extend budget (after human approval)
    pub fn extend_steps(&mut self, additional: u32) {
        self.limits.steps += additional;
        // Reset warnings for this dimension
        self.warnings_issued.remove(&BudgetWarning::Steps50);
        self.warnings_issued.remove(&BudgetWarning::Steps80);
    }

    /// Extend LLM token budget (after human approval)
    pub fn extend_llm_tokens(&mut self, additional: u64) {
        self.limits.llm_tokens += additional;
        self.warnings_issued.remove(&BudgetWarning::LlmTokens50);
        self.warnings_issued.remove(&BudgetWarning::LlmTokens80);
    }

    /// Extend cost budget (after human approval)
    pub fn extend_cost(&mut self, additional: f64) {
        self.limits.cost_usd += additional;
        self.warnings_issued.remove(&BudgetWarning::Cost50);
        self.warnings_issued.remove(&BudgetWarning::Cost80);
    }

    /// Extend wall-clock budget (after human approval)
    pub fn extend_wall_clock_ms(&mut self, additional: u64) {
        self.limits.wall_clock_ms += additional;
        self.warnings_issued.remove(&BudgetWarning::WallClock50);
        self.warnings_issued.remove(&BudgetWarning::WallClock80);
    }

    /// Extend network egress budget (after human approval)
    pub fn extend_network_egress_bytes(&mut self, additional: u64) {
        self.limits.network_egress_bytes += additional;
        self.warnings_issued.remove(&BudgetWarning::Network50);
        self.warnings_issued.remove(&BudgetWarning::Network80);
    }

    /// Extend storage write budget (after human approval)
    pub fn extend_storage_write_bytes(&mut self, additional: u64) {
        self.limits.storage_write_bytes += additional;
        self.warnings_issued.remove(&BudgetWarning::Storage50);
        self.warnings_issued.remove(&BudgetWarning::Storage80);
    }

    /// Extend sandbox CPU budget (after human approval)
    pub fn extend_sandbox_cpu_ms(&mut self, additional: u64) {
        self.limits.sandbox_cpu_ms += additional;
        self.warnings_issued.remove(&BudgetWarning::SandboxCpu50);
        self.warnings_issued.remove(&BudgetWarning::SandboxCpu80);
    }

    /// Extend sandbox memory budget (after human approval)
    pub fn extend_sandbox_memory_peak_mb(&mut self, additional: u64) {
        self.limits.sandbox_memory_peak_mb += additional;
        self.warnings_issued.remove(&BudgetWarning::SandboxMemory50);
        self.warnings_issued.remove(&BudgetWarning::SandboxMemory80);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_remaining_calculation() {
        let limits = BudgetLimits {
            steps: 10,
            llm_tokens: 1000,
            ..Default::default()
        };
        let mut ctx = BudgetContext::new(limits, BudgetPolicies::default());

        // Record some consumption
        ctx.record_step(StepConsumption {
            llm_input_tokens: 100,
            llm_output_tokens: 50,
            ..Default::default()
        });

        let remaining = ctx.remaining();
        assert_eq!(remaining.steps, 9);
        assert_eq!(remaining.llm_tokens, 850); // 1000 - 150
    }

    #[test]
    fn test_warning_not_repeated() {
        let limits = BudgetLimits {
            steps: 10,
            ..Default::default()
        };
        let mut ctx = BudgetContext::new(limits, BudgetPolicies::default());

        // Use 5 steps (50%)
        for _ in 0..5 {
            ctx.record_step(StepConsumption::default());
        }

        // First check should warn
        assert!(matches!(
            ctx.check(),
            BudgetCheckResult::Warning { percent: 50, .. }
        ));

        // Second check should be Ok (warning already issued)
        assert!(matches!(ctx.check(), BudgetCheckResult::Ok));
    }

    #[test]
    fn test_extend_budget() {
        let limits = BudgetLimits {
            steps: 2,
            ..Default::default()
        };
        let mut ctx = BudgetContext::new(limits, BudgetPolicies::default());

        // Exhaust steps
        ctx.record_step(StepConsumption::default());
        ctx.record_step(StepConsumption::default());

        assert!(matches!(ctx.check(), BudgetCheckResult::Exhausted { .. }));

        // Extend budget
        ctx.extend_steps(5);

        // Should be Ok now
        assert!(matches!(ctx.check(), BudgetCheckResult::Ok));
    }
}
