use rtfs::config::types::AdaptiveThresholdConfig;
use std::collections::HashMap;
use std::env;

/// Adaptive threshold calculator for delegation decisions
pub struct AdaptiveThresholdCalculator {
    config: AdaptiveThresholdConfig,
    agent_performance: HashMap<String, AgentPerformance>,
}

/// Performance metrics for an individual agent
#[derive(Debug, Clone)]
pub struct AgentPerformance {
    pub agent_id: String,
    pub success_rate: f64,
    pub decay_weighted_rate: f64,
    pub samples: u64,
    pub last_update: std::time::SystemTime,
}

impl AdaptiveThresholdCalculator {
    /// Create a new adaptive threshold calculator
    pub fn new(config: AdaptiveThresholdConfig) -> Self {
        Self {
            config,
            agent_performance: HashMap::new(),
        }
    }

    /// Calculate the adaptive threshold for a specific agent
    pub fn calculate_threshold(&self, agent_id: &str, base_threshold: f64) -> f64 {
        // Check if adaptive threshold is enabled
        if !self.config.enabled.unwrap_or(true) {
            return base_threshold;
        }

        // Get environment variable overrides
        let env_threshold = self.get_env_override("THRESHOLD");
        if let Some(threshold) = env_threshold {
            return threshold.clamp(
                self.config.min_threshold.unwrap_or(0.3),
                self.config.max_threshold.unwrap_or(0.9),
            );
        }

        // Get agent performance data
        let performance = self.agent_performance.get(agent_id);
        if performance.is_none() {
            return base_threshold;
        }

        let performance = performance.unwrap();
        let min_samples = self.config.min_samples.unwrap_or(5);

        // Don't apply adaptive threshold until we have enough samples
        if performance.samples < min_samples as u64 {
            return base_threshold;
        }

        // Calculate adaptive threshold based on performance
        let success_rate_weight = self.config.success_rate_weight.unwrap_or(0.7);
        let historical_weight = self.config.historical_weight.unwrap_or(0.3);

        let adaptive_component = (performance.success_rate * success_rate_weight)
            + (performance.decay_weighted_rate * historical_weight);

        let base_threshold_weight = 1.0 - (success_rate_weight + historical_weight);
        let adaptive_threshold = (base_threshold * base_threshold_weight) + adaptive_component;

        // Apply bounds
        let min_threshold = self.config.min_threshold.unwrap_or(0.3);
        let max_threshold = self.config.max_threshold.unwrap_or(0.9);

        adaptive_threshold.clamp(min_threshold, max_threshold)
    }

    /// Update agent performance with new feedback
    pub fn update_performance(&mut self, agent_id: &str, success: bool) {
        let now = std::time::SystemTime::now();
        let decay_factor = self.config.decay_factor.unwrap_or(0.8);

        let performance = self
            .agent_performance
            .entry(agent_id.to_string())
            .or_insert_with(|| AgentPerformance {
                agent_id: agent_id.to_string(),
                success_rate: 0.0,
                decay_weighted_rate: 0.0,
                samples: 0,
                last_update: now,
            });

        // Update basic success rate
        let successes_prior = performance.success_rate * performance.samples as f64;
        let successes_new = successes_prior + if success { 1.0 } else { 0.0 };
        performance.samples += 1;
        performance.success_rate = if performance.samples == 0 {
            0.0
        } else {
            successes_new / performance.samples as f64
        };

        // Update decay-weighted rate
        let success_value = if success { 1.0 } else { 0.0 };
        if performance.samples == 1 {
            performance.decay_weighted_rate = success_value;
        } else {
            performance.decay_weighted_rate = (performance.decay_weighted_rate * decay_factor)
                + (success_value * (1.0 - decay_factor));
        }

        performance.last_update = now;
    }

    /// Get agent performance data
    pub fn get_performance(&self, agent_id: &str) -> Option<&AgentPerformance> {
        self.agent_performance.get(agent_id)
    }

    /// Get all agent performance data
    pub fn get_all_performance(&self) -> &HashMap<String, AgentPerformance> {
        &self.agent_performance
    }

    /// Get environment variable override
    fn get_env_override(&self, key: &str) -> Option<f64> {
        let prefix = self
            .config
            .env_prefix
            .as_deref()
            .unwrap_or("CCOS_DELEGATION_");
        let env_key = format!("{}{}", prefix, key);

        env::var(&env_key)
            .ok()
            .and_then(|value| value.parse::<f64>().ok())
    }

    /// Reset performance data for an agent
    pub fn reset_performance(&mut self, agent_id: &str) {
        self.agent_performance.remove(agent_id);
    }

    /// Reset all performance data
    pub fn reset_all_performance(&mut self) {
        self.agent_performance.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_adaptive_threshold_disabled() {
        let mut config = AdaptiveThresholdConfig::default();
        config.enabled = Some(false);

        let calculator = AdaptiveThresholdCalculator::new(config);
        let threshold = calculator.calculate_threshold("test_agent", 0.65);

        assert_eq!(threshold, 0.65);
    }

    #[test]
    fn test_adaptive_threshold_bounds() {
        let mut config = AdaptiveThresholdConfig::default();
        config.min_threshold = Some(0.4);
        config.max_threshold = Some(0.8);

        let mut calculator = AdaptiveThresholdCalculator::new(config);

        // Add performance data that would push threshold outside bounds
        for _ in 0..10 {
            calculator.update_performance("test_agent", true); // 100% success
        }

        let threshold = calculator.calculate_threshold("test_agent", 0.65);

        // Should be clamped to max_threshold
        assert_eq!(threshold, 0.8);
    }

    #[test]
    fn test_adaptive_threshold_min_samples() {
        let mut config = AdaptiveThresholdConfig::default();
        config.min_samples = Some(5);

        let mut calculator = AdaptiveThresholdCalculator::new(config);

        // Add only 3 samples (below minimum)
        for _ in 0..3 {
            calculator.update_performance("test_agent", true);
        }

        let threshold = calculator.calculate_threshold("test_agent", 0.65);

        // Should return base threshold due to insufficient samples
        assert_eq!(threshold, 0.65);
    }

    #[test]
    fn test_performance_update() {
        let mut calculator = AdaptiveThresholdCalculator::new(AdaptiveThresholdConfig::default());

        // Add some performance data
        calculator.update_performance("test_agent", true);
        calculator.update_performance("test_agent", false);
        calculator.update_performance("test_agent", true);

        let performance = calculator.get_performance("test_agent").unwrap();

        assert_eq!(performance.samples, 3);
        assert_eq!(performance.success_rate, 2.0 / 3.0);
        assert!(performance.decay_weighted_rate > 0.0);
    }

    #[test]
    fn test_reset_performance() {
        let mut calculator = AdaptiveThresholdCalculator::new(AdaptiveThresholdConfig::default());

        calculator.update_performance("test_agent", true);
        assert!(calculator.get_performance("test_agent").is_some());

        calculator.reset_performance("test_agent");
        assert!(calculator.get_performance("test_agent").is_none());
    }
}
