//! Circuit breaker hint handler - prevents repeated calls to failing capabilities.

use crate::hints::types::{BoxFuture, ExecutionContext, HintHandler, NextExecutor};
use crate::types::{Action, ActionType};
use rtfs::runtime::execution_outcome::HostCall;
use rtfs::runtime::values::Value;
use rtfs::runtime::{RuntimeError, RuntimeResult};
use std::collections::HashMap;
use std::sync::Mutex;
use std::time::{Duration, Instant};

/// Handler for the `runtime.learning.circuit-breaker` hint.
///
/// Implements the circuit breaker pattern to prevent cascading failures.
/// After a configurable number of failures, the circuit "opens" and
/// immediately fails requests for a cooldown period.
///
/// # Hint Format
/// ```rtfs
/// ^{:runtime.learning.circuit-breaker {:failure-threshold 5 :cooldown-ms 30000}}
/// ```
///
/// # Fields
/// - `failure-threshold`: Number of failures before circuit opens (default: 5)
/// - `cooldown-ms`: Time in ms before circuit attempts to close (default: 30000)
/// - `success-threshold`: Successes needed in half-open state to close (default: 2)
pub struct CircuitBreakerHintHandler {
    /// Circuit state per capability ID
    circuits: Mutex<HashMap<String, CircuitState>>,
}

#[derive(Debug, Clone)]
enum CircuitStatus {
    Closed,   // Normal operation
    Open,     // Blocking all requests
    HalfOpen, // Testing if service recovered
}

#[derive(Debug, Clone)]
struct CircuitState {
    status: CircuitStatus,
    failure_count: u32,
    success_count: u32,
    last_failure: Option<Instant>,
    failure_threshold: u32,
    cooldown: Duration,
    success_threshold: u32,
}

impl CircuitState {
    fn new(failure_threshold: u32, cooldown_ms: u64, success_threshold: u32) -> Self {
        Self {
            status: CircuitStatus::Closed,
            failure_count: 0,
            success_count: 0,
            last_failure: None,
            failure_threshold,
            cooldown: Duration::from_millis(cooldown_ms),
            success_threshold,
        }
    }

    fn should_allow(&mut self) -> bool {
        match self.status {
            CircuitStatus::Closed => true,
            CircuitStatus::Open => {
                // Check if cooldown has elapsed
                if let Some(last_failure) = self.last_failure {
                    if last_failure.elapsed() >= self.cooldown {
                        self.status = CircuitStatus::HalfOpen;
                        self.success_count = 0;
                        true
                    } else {
                        false
                    }
                } else {
                    true
                }
            }
            CircuitStatus::HalfOpen => true,
        }
    }

    fn record_success(&mut self) {
        match self.status {
            CircuitStatus::Closed => {
                self.failure_count = 0; // Reset on success
            }
            CircuitStatus::HalfOpen => {
                self.success_count += 1;
                if self.success_count >= self.success_threshold {
                    self.status = CircuitStatus::Closed;
                    self.failure_count = 0;
                }
            }
            CircuitStatus::Open => {}
        }
    }

    fn record_failure(&mut self) {
        self.failure_count += 1;
        self.last_failure = Some(Instant::now());

        match self.status {
            CircuitStatus::Closed => {
                if self.failure_count >= self.failure_threshold {
                    self.status = CircuitStatus::Open;
                }
            }
            CircuitStatus::HalfOpen => {
                // Single failure in half-open returns to open
                self.status = CircuitStatus::Open;
            }
            CircuitStatus::Open => {}
        }
    }
}

impl CircuitBreakerHintHandler {
    pub fn new() -> Self {
        Self {
            circuits: Mutex::new(HashMap::new()),
        }
    }

    fn extract_u64_from_map(value: &Value, key: &str) -> Option<u64> {
        if let Value::Map(map) = value {
            for (k, v) in map {
                let key_str = match k {
                    rtfs::ast::MapKey::Keyword(kw) => &kw.0,
                    rtfs::ast::MapKey::String(s) => s,
                    _ => continue,
                };
                if key_str == key {
                    return match v {
                        Value::Integer(i) => Some(*i as u64),
                        _ => None,
                    };
                }
            }
        }
        None
    }
}

impl Default for CircuitBreakerHintHandler {
    fn default() -> Self {
        Self::new()
    }
}

impl HintHandler for CircuitBreakerHintHandler {
    fn hint_key(&self) -> &str {
        "runtime.learning.circuit-breaker"
    }

    fn priority(&self) -> u32 {
        3 // Pre-execution, before rate-limit (5)
    }

    fn validate_hint(&self, hint_value: &Value) -> RuntimeResult<()> {
        if let Value::Map(_) = hint_value {
            Ok(())
        } else {
            Err(RuntimeError::Generic(
                "circuit-breaker hint value must be a map".to_string(),
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
            let failure_threshold =
                Self::extract_u64_from_map(hint_value, "failure-threshold").unwrap_or(5) as u32;
            let cooldown_ms =
                Self::extract_u64_from_map(hint_value, "cooldown-ms").unwrap_or(30000);
            let success_threshold =
                Self::extract_u64_from_map(hint_value, "success-threshold").unwrap_or(2) as u32;

            let capability_id = host_call.capability_id.clone();

            // Check if circuit allows the call
            let allowed = {
                let mut circuits = self.circuits.lock().unwrap();
                let circuit = circuits.entry(capability_id.clone()).or_insert_with(|| {
                    CircuitState::new(failure_threshold, cooldown_ms, success_threshold)
                });
                circuit.should_allow()
            };

            if !allowed {
                // Circuit is open - fail fast
                if let Ok(mut chain) = ctx.causal_chain.lock() {
                    let _ = chain.append(
                        &Action::new(
                            ActionType::HintApplied,
                            format!("capability:{}", host_call.capability_id),
                            String::new(),
                        )
                        .with_metadata("hint", "circuit-breaker:OPEN (rejected)"),
                    );
                }
                return Err(RuntimeError::Generic(format!(
                    "Circuit breaker OPEN for capability '{}' - requests blocked",
                    host_call.capability_id
                )));
            }

            // Execute the call
            let result = next().await;

            // Update circuit state based on result
            {
                let mut circuits = self.circuits.lock().unwrap();
                if let Some(circuit) = circuits.get_mut(&capability_id) {
                    if result.is_ok() {
                        circuit.record_success();
                        if let Ok(mut chain) = ctx.causal_chain.lock() {
                            let _ = chain.append(
                                &Action::new(
                                    ActionType::HintApplied,
                                    format!("capability:{}", host_call.capability_id),
                                    String::new(),
                                )
                                .with_metadata(
                                    "hint",
                                    &format!("circuit-breaker:{:?}", circuit.status),
                                ),
                            );
                        }
                    } else {
                        circuit.record_failure();
                        if let Ok(mut chain) = ctx.causal_chain.lock() {
                            let _ = chain.append(
                                &Action::new(
                                    ActionType::HintApplied,
                                    format!("capability:{}", host_call.capability_id),
                                    String::new(),
                                )
                                .with_metadata(
                                    "hint",
                                    &format!("circuit-breaker:failure #{}", circuit.failure_count),
                                ),
                            );
                        }
                    }
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
    fn test_circuit_breaker_key_and_priority() {
        let handler = CircuitBreakerHintHandler::new();
        assert_eq!(handler.hint_key(), "runtime.learning.circuit-breaker");
        assert_eq!(handler.priority(), 3);
    }

    #[test]
    fn test_circuit_state_transitions() {
        let mut state = CircuitState::new(3, 1000, 2);

        // Initially closed, should allow
        assert!(state.should_allow());

        // Record failures
        state.record_failure();
        state.record_failure();
        assert!(matches!(state.status, CircuitStatus::Closed));

        // Third failure should open circuit
        state.record_failure();
        assert!(matches!(state.status, CircuitStatus::Open));

        // Should not allow when open
        assert!(!state.should_allow());
    }
}
