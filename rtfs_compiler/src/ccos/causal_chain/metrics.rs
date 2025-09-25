use super::super::types::{Action, CapabilityId, IntentId, PlanId};
use crate::runtime::error::RuntimeError;
use std::collections::HashMap;

/// Performance metrics tracking
#[derive(Debug)]
pub struct PerformanceMetrics {
    pub capability_metrics: HashMap<CapabilityId, CapabilityMetrics>,
    pub function_metrics: HashMap<String, FunctionMetrics>,
    pub cost_tracking: CostTracker,
    pub wm_ingest_latency: WmIngestLatencyMetrics,
}

impl PerformanceMetrics {
    pub fn new() -> Self {
        Self {
            capability_metrics: HashMap::new(),
            function_metrics: HashMap::new(),
            cost_tracking: CostTracker::new(),
            wm_ingest_latency: WmIngestLatencyMetrics::new(),
        }
    }

    pub fn record_action(&mut self, action: &Action) -> Result<(), RuntimeError> {
        // Update capability metrics (for capability calls)
        if action.action_type == super::super::types::ActionType::CapabilityCall {
            if let Some(function_name) = &action.function_name {
                let metrics = self
                    .capability_metrics
                    .entry(function_name.clone())
                    .or_insert_with(CapabilityMetrics::new);
                metrics.record_action(action);
            }
        }

        // Update function metrics
        if let Some(function_name) = &action.function_name {
            let function_metrics = self
                .function_metrics
                .entry(function_name.clone())
                .or_insert_with(FunctionMetrics::new);
            function_metrics.record_action(action);
        }

        // Update cost tracking
        self.cost_tracking.record_cost(action.cost.unwrap_or(0.0));

        Ok(())
    }

    pub fn get_capability_metrics(
        &self,
        capability_id: &CapabilityId,
    ) -> Option<&CapabilityMetrics> {
        self.capability_metrics.get(capability_id)
    }

    pub fn get_function_metrics(&self, function_name: &str) -> Option<&FunctionMetrics> {
        self.function_metrics.get(function_name)
    }

    pub fn get_total_cost(&self) -> f64 {
        self.cost_tracking.total_cost
    }

    pub fn get_wm_ingest_latency_metrics(&self) -> &WmIngestLatencyMetrics {
        &self.wm_ingest_latency
    }

    pub fn record_wm_ingest_latency(&mut self, latency_ms: u64) {
        self.wm_ingest_latency.record_ingest(latency_ms);
    }
}

/// Metrics for a specific capability
#[derive(Debug, Clone)]
pub struct CapabilityMetrics {
    pub total_calls: u64,
    pub successful_calls: u64,
    pub failed_calls: u64,
    pub total_cost: f64,
    pub total_duration_ms: u64,
    pub average_duration_ms: f64,
    pub reliability_score: f64,
}

impl CapabilityMetrics {
    pub fn new() -> Self {
        Self {
            total_calls: 0,
            successful_calls: 0,
            failed_calls: 0,
            total_cost: 0.0,
            total_duration_ms: 0,
            average_duration_ms: 0.0,
            reliability_score: 1.0,
        }
    }

    pub fn record_action(&mut self, action: &Action) {
        self.total_calls += 1;
        self.total_cost += action.cost.unwrap_or(0.0);
        self.total_duration_ms += action.duration_ms.unwrap_or(0);

        // Success/failure tracking removed: Action does not have a success field

        self.average_duration_ms = self.total_duration_ms as f64 / self.total_calls as f64;
        self.reliability_score = self.successful_calls as f64 / self.total_calls as f64;
    }
}

/// Metrics for a specific function
#[derive(Debug, Clone)]
pub struct FunctionMetrics {
    pub total_calls: u64,
    pub successful_calls: u64,
    pub failed_calls: u64,
    pub total_cost: f64,
    pub total_duration_ms: u64,
    pub average_duration_ms: f64,
}

impl FunctionMetrics {
    pub fn new() -> Self {
        Self {
            total_calls: 0,
            successful_calls: 0,
            failed_calls: 0,
            total_cost: 0.0,
            total_duration_ms: 0,
            average_duration_ms: 0.0,
        }
    }

    pub fn record_action(&mut self, action: &Action) {
        self.total_calls += 1;
        self.total_cost += action.cost.unwrap_or(0.0);
        self.total_duration_ms += action.duration_ms.unwrap_or(0);

        // Success/failure tracking removed: Action does not have a success field

        self.average_duration_ms = self.total_duration_ms as f64 / self.total_calls as f64;
    }
}

/// Working Memory ingest latency metrics
#[derive(Debug)]
pub struct WmIngestLatencyMetrics {
    pub total_ingests: u64,
    pub total_latency_ms: u64,
    pub average_latency_ms: f64,
    pub max_latency_ms: u64,
    pub min_latency_ms: u64,
    pub latency_histogram: HashMap<u64, u64>, // bucket_ms -> count
}

impl WmIngestLatencyMetrics {
    pub fn new() -> Self {
        let mut latency_histogram = HashMap::new();
        // Initialize histogram buckets: 1ms, 5ms, 10ms, 25ms, 50ms, 100ms, 250ms, 500ms, 1000ms, 2500ms, 5000ms
        for &bucket in &[1, 5, 10, 25, 50, 100, 250, 500, 1000, 2500, 5000] {
            latency_histogram.insert(bucket, 0);
        }

        Self {
            total_ingests: 0,
            total_latency_ms: 0,
            average_latency_ms: 0.0,
            max_latency_ms: 0,
            min_latency_ms: u64::MAX,
            latency_histogram,
        }
    }

    pub fn record_ingest(&mut self, latency_ms: u64) {
        self.total_ingests += 1;
        self.total_latency_ms += latency_ms;

        if latency_ms > self.max_latency_ms {
            self.max_latency_ms = latency_ms;
        }

        if latency_ms < self.min_latency_ms {
            self.min_latency_ms = latency_ms;
        }

        self.average_latency_ms = self.total_latency_ms as f64 / self.total_ingests as f64;

        // Update histogram
        let bucket = self.find_histogram_bucket(latency_ms);
        *self.latency_histogram.entry(bucket).or_insert(0) += 1;
    }

    fn find_histogram_bucket(&self, latency_ms: u64) -> u64 {
        for &bucket in &[1, 5, 10, 25, 50, 100, 250, 500, 1000, 2500, 5000] {
            if latency_ms <= bucket {
                return bucket;
            }
        }
        5000 // Max bucket
    }

    pub fn get_histogram_data(&self) -> &HashMap<u64, u64> {
        &self.latency_histogram
    }
}

/// Cost tracking
#[derive(Debug)]
pub struct CostTracker {
    pub total_cost: f64,
    pub cost_by_intent: HashMap<IntentId, f64>,
    pub cost_by_plan: HashMap<PlanId, f64>,
    pub cost_by_capability: HashMap<CapabilityId, f64>,
}

impl CostTracker {
    pub fn new() -> Self {
        Self {
            total_cost: 0.0,
            cost_by_intent: HashMap::new(),
            cost_by_plan: HashMap::new(),
            cost_by_capability: HashMap::new(),
        }
    }

    pub fn record_cost(&mut self, cost: f64) {
        self.total_cost += cost;
    }

    pub fn record_action_cost(&mut self, action: &Action) {
        let cost = action.cost.unwrap_or(0.0);

        // Track by intent
        *self
            .cost_by_intent
            .entry(action.intent_id.clone())
            .or_insert(0.0) += cost;

        // Track by plan
        *self
            .cost_by_plan
            .entry(action.plan_id.clone())
            .or_insert(0.0) += cost;

        // Track by capability: Action does not have capability_id field, skip
    }
}
