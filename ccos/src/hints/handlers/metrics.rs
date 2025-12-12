//! Metrics hint handler - emits timing and execution statistics.

use crate::hints::types::{BoxFuture, ExecutionContext, HintHandler, NextExecutor};
use crate::types::{Action, ActionType};
use rtfs::runtime::execution_outcome::HostCall;
use rtfs::runtime::values::Value;
use rtfs::runtime::{RuntimeError, RuntimeResult};
use std::collections::HashMap;
use std::sync::Mutex;
use std::time::Instant;

/// Handler for the `runtime.learning.metrics` hint.
///
/// Collects and logs execution timing and statistics.
/// Useful for observability and performance tuning.
///
/// # Hint Format
/// ```rtfs
/// ^{:runtime.learning.metrics {:emit-to-chain true :track-percentiles true}}
/// ```
///
/// # Fields
/// - `emit-to-chain`: Log metrics to causal chain (default: true)
/// - `track-percentiles`: Track p50/p90/p99 latencies (default: false)
/// - `label`: Custom label for this metric point (default: capability_id)
pub struct MetricsHintHandler {
    /// Accumulated metrics per capability
    metrics: Mutex<HashMap<String, CapabilityMetrics>>,
}

#[derive(Debug, Clone, Default)]
struct CapabilityMetrics {
    call_count: u64,
    success_count: u64,
    failure_count: u64,
    total_duration_ms: u64,
    min_duration_ms: Option<u64>,
    max_duration_ms: Option<u64>,
    latencies: Vec<u64>, // For percentile calculation
}

impl CapabilityMetrics {
    fn record(&mut self, duration_ms: u64, success: bool) {
        self.call_count += 1;
        self.total_duration_ms += duration_ms;

        if success {
            self.success_count += 1;
        } else {
            self.failure_count += 1;
        }

        self.min_duration_ms = Some(
            self.min_duration_ms
                .map(|m| m.min(duration_ms))
                .unwrap_or(duration_ms),
        );
        self.max_duration_ms = Some(
            self.max_duration_ms
                .map(|m| m.max(duration_ms))
                .unwrap_or(duration_ms),
        );

        // Keep last 1000 latencies for percentile calc
        if self.latencies.len() >= 1000 {
            self.latencies.remove(0);
        }
        self.latencies.push(duration_ms);
    }

    fn avg_duration_ms(&self) -> u64 {
        if self.call_count == 0 {
            0
        } else {
            self.total_duration_ms / self.call_count
        }
    }

    fn percentile(&self, p: f64) -> Option<u64> {
        if self.latencies.is_empty() {
            return None;
        }
        let mut sorted = self.latencies.clone();
        sorted.sort();
        let idx = ((sorted.len() as f64) * p / 100.0).floor() as usize;
        sorted.get(idx.min(sorted.len() - 1)).copied()
    }
}

impl MetricsHintHandler {
    pub fn new() -> Self {
        Self {
            metrics: Mutex::new(HashMap::new()),
        }
    }

    fn extract_bool_from_map(value: &Value, key: &str) -> Option<bool> {
        if let Value::Map(map) = value {
            for (k, v) in map {
                let key_str = match k {
                    rtfs::ast::MapKey::Keyword(kw) => &kw.0,
                    rtfs::ast::MapKey::String(s) => s,
                    _ => continue,
                };
                if key_str == key {
                    return match v {
                        Value::Boolean(b) => Some(*b),
                        _ => None,
                    };
                }
            }
        }
        None
    }

    fn extract_string_from_map(value: &Value, key: &str) -> Option<String> {
        if let Value::Map(map) = value {
            for (k, v) in map {
                let key_str = match k {
                    rtfs::ast::MapKey::Keyword(kw) => &kw.0,
                    rtfs::ast::MapKey::String(s) => s,
                    _ => continue,
                };
                if key_str == key {
                    return match v {
                        Value::String(s) => Some(s.clone()),
                        _ => None,
                    };
                }
            }
        }
        None
    }

    /// Get current metrics for a capability (for external access)
    pub fn get_metrics(&self, capability_id: &str) -> Option<(u64, u64, u64, u64)> {
        let metrics = self.metrics.lock().ok()?;
        metrics.get(capability_id).map(|m| {
            (
                m.call_count,
                m.success_count,
                m.failure_count,
                m.avg_duration_ms(),
            )
        })
    }
}

impl Default for MetricsHintHandler {
    fn default() -> Self {
        Self::new()
    }
}

impl HintHandler for MetricsHintHandler {
    fn hint_key(&self) -> &str {
        "runtime.learning.metrics"
    }

    fn priority(&self) -> u32 {
        1 // Very first - wraps everything to measure total time
    }

    fn validate_hint(&self, hint_value: &Value) -> RuntimeResult<()> {
        if let Value::Map(_) = hint_value {
            Ok(())
        } else {
            Err(RuntimeError::Generic(
                "metrics hint value must be a map".to_string(),
            ))
        }
    }

    fn apply<'a>(
        &'a self,
        host_call: &'a HostCall,
        hint_value: &'a Value,
        ctx: &'a ExecutionContext,
        next: NextExecutor<'a>,
    ) -> BoxFuture<'a, RuntimeResult<Value>> {
        Box::pin(async move {
            let emit_to_chain =
                Self::extract_bool_from_map(hint_value, "emit-to-chain").unwrap_or(true);
            let track_percentiles =
                Self::extract_bool_from_map(hint_value, "track-percentiles").unwrap_or(false);
            let label = Self::extract_string_from_map(hint_value, "label")
                .unwrap_or_else(|| host_call.capability_id.clone());

            let capability_id = host_call.capability_id.clone();
            let start = Instant::now();

            // Execute the chain
            let result = next().await;

            let duration_ms = start.elapsed().as_millis() as u64;
            let success = result.is_ok();

            // Record metrics
            let metrics_summary = {
                let mut metrics = self.metrics.lock().unwrap();
                let m = metrics
                    .entry(capability_id.clone())
                    .or_insert_with(CapabilityMetrics::default);
                m.record(duration_ms, success);

                if track_percentiles {
                    format!(
                        "calls={} ok={} err={} avg={}ms p50={}ms p99={}ms",
                        m.call_count,
                        m.success_count,
                        m.failure_count,
                        m.avg_duration_ms(),
                        m.percentile(50.0).unwrap_or(0),
                        m.percentile(99.0).unwrap_or(0)
                    )
                } else {
                    format!(
                        "calls={} ok={} err={} latency={}ms",
                        m.call_count, m.success_count, m.failure_count, duration_ms
                    )
                }
            };

            // Emit to causal chain if configured
            if emit_to_chain {
                if let Ok(mut chain) = ctx.causal_chain.lock() {
                    let _ = chain.append(
                        &Action::new(
                            ActionType::HintApplied,
                            format!("capability:{}", host_call.capability_id),
                            String::new(),
                        )
                        .with_metadata("hint", &format!("metrics[{}]: {}", label, metrics_summary)),
                    );
                }
            }

            result
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_metrics_handler_key_and_priority() {
        let handler = MetricsHintHandler::new();
        assert_eq!(handler.hint_key(), "runtime.learning.metrics");
        assert_eq!(handler.priority(), 1); // First in chain
    }

    #[test]
    fn test_capability_metrics_recording() {
        let mut m = CapabilityMetrics::default();
        m.record(100, true);
        m.record(200, true);
        m.record(50, false);

        assert_eq!(m.call_count, 3);
        assert_eq!(m.success_count, 2);
        assert_eq!(m.failure_count, 1);
        assert_eq!(m.min_duration_ms, Some(50));
        assert_eq!(m.max_duration_ms, Some(200));
        assert_eq!(m.avg_duration_ms(), 116); // (100+200+50)/3
    }

    #[test]
    fn test_percentile_calculation() {
        let mut m = CapabilityMetrics::default();
        for i in 1..=100 {
            m.record(i, true);
        }
        assert_eq!(m.percentile(50.0), Some(50));
        assert_eq!(m.percentile(99.0), Some(99));
    }
}
